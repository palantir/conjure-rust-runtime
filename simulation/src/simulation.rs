// Copyright 2020 Palantir Technologies, Inc.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
// http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.
use crate::metrics;
use crate::recorder::{MetricsRecord, SimulationMetricsRecorder};
use crate::server::{
    Endpoint, Server, ServerBuilder0, SimulationRawClient, SimulationRawClientBuilder,
};
use conjure_runtime::errors::{RemoteError, ThrottledError, UnavailableError};
use conjure_runtime::{Agent, Builder, Client, NodeSelectionStrategy, UserAgent};
use futures::stream::{self, Stream, StreamExt};
use http::StatusCode;
use parking_lot::Mutex;
use rand::seq::SliceRandom;
use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::fmt;
use std::pin::Pin;
use std::sync::Arc;
use tokio::runtime::{self, Runtime};
use tokio::time::{self, Duration, Instant};
use witchcraft_metrics::MetricRegistry;

const SERVICE: &str = "simulation";

pub struct SimulationBuilder0;

impl SimulationBuilder0 {
    pub fn strategy(self, strategy: Strategy) -> SimulationBuilder1 {
        let mut runtime = runtime::Builder::new()
            .enable_time()
            .basic_scheduler()
            .build()
            .unwrap();
        runtime.block_on(async {
            time::pause();
            // https://github.com/tokio-rs/tokio/issues/3179
            time::delay_for(Duration::from_millis(1)).await;
        });

        let metrics = Arc::new(MetricRegistry::new());
        // initialize the responses timer with our custom reservoir
        metrics::responses_timer(&metrics, SERVICE);

        SimulationBuilder1 {
            runtime,
            servers: vec![],
            metrics,
            endpoints: vec![Endpoint::DEFAULT],
            strategy,
        }
    }
}

pub struct SimulationBuilder1 {
    runtime: Runtime,
    metrics: Arc<MetricRegistry>,
    servers: Vec<Server>,
    endpoints: Vec<Endpoint>,
    strategy: Strategy,
}

impl SimulationBuilder1 {
    pub fn server<F>(mut self, f: F) -> Self
    where
        F: FnOnce(ServerBuilder0) -> Server,
    {
        let server = self.runtime.enter(|| {
            f(ServerBuilder0 {
                metrics: &self.metrics,
            })
        });
        self.servers.push(server);
        self
    }

    pub fn endpoints(mut self, endpoints: Vec<Endpoint>) -> Self {
        self.endpoints = endpoints;
        self
    }

    pub fn requests_per_second(self, requests_per_second: u32) -> SimulationBuilder2 {
        let mut recorder = self
            .runtime
            .enter(|| SimulationMetricsRecorder::new(&self.metrics));
        recorder.filter_metrics(|id| {
            id.name().ends_with("activeRequests") || id.name().ends_with("request")
        });
        let recorder = Arc::new(Mutex::new(recorder));

        let mut builder = Builder::new();
        self.strategy.apply(&mut builder);
        builder
            .service(SERVICE)
            .user_agent(UserAgent::new(Agent::new("simulation", "0.0.0")))
            .metrics(self.metrics.clone())
            .rng_builder(crate::rng);
        for server in &self.servers {
            builder.uri(format!("http://{}", server.name()).parse().unwrap());
        }

        let raw_client_builder =
            SimulationRawClientBuilder::new(self.servers, &self.metrics, &recorder);
        let builder = builder.with_raw_client_builder(raw_client_builder);

        SimulationBuilder2 {
            runtime: self.runtime,
            builder,
            metrics: self.metrics,
            recorder,
            endpoints: self.endpoints,
            delay_between_requests: Duration::from_secs(1) / requests_per_second,
        }
    }
}

pub struct SimulationBuilder2 {
    runtime: Runtime,
    metrics: Arc<MetricRegistry>,
    builder: Builder<SimulationRawClientBuilder>,
    recorder: Arc<Mutex<SimulationMetricsRecorder>>,
    endpoints: Vec<Endpoint>,
    delay_between_requests: Duration,
}

impl SimulationBuilder2 {
    pub fn send_until(self, cutoff: Duration) -> SimulationBuilder3 {
        let num_requests = cutoff.as_nanos() as u64 / self.delay_between_requests.as_nanos() as u64;
        self.num_requests(num_requests)
    }

    pub fn num_requests(self, num_requests: u64) -> SimulationBuilder3 {
        let mut rng = crate::rng();

        let it = (0..num_requests).map({
            let endpoints = self.endpoints;
            move |_| endpoints.choose(&mut rng).unwrap().clone()
        });
        let stream = stream::iter(it);
        let stream = self.runtime.enter({
            let delay_between_requests = self.delay_between_requests;
            // FIXME use tokio::time::throttle when it handles sub-milli delays properly
            move || {
                let mut next_item = Instant::now();
                stream.then(move |v| {
                    let wait = next_item;
                    next_item += delay_between_requests;
                    async move {
                        time::delay_until(wait).await;
                        v
                    }
                })
            }
        });

        SimulationBuilder3 {
            runtime: self.runtime,
            metrics: self.metrics,
            recorder: self.recorder,
            builder: self.builder,
            requests: Box::pin(stream),
            abort_after: None,
        }
    }
}

pub struct SimulationBuilder3 {
    runtime: Runtime,
    metrics: Arc<MetricRegistry>,
    recorder: Arc<Mutex<SimulationMetricsRecorder>>,
    builder: Builder<SimulationRawClientBuilder>,
    requests: Pin<Box<dyn Stream<Item = Endpoint>>>,
    abort_after: Option<Duration>,
}

impl SimulationBuilder3 {
    pub fn abort_after(mut self, cutoff: Duration) -> Self {
        self.abort_after = Some(cutoff);
        self
    }

    pub fn clients(self, clients: u32) -> Simulation {
        let clients = (0..clients)
            .map(|_| self.runtime.enter(|| self.builder.build().unwrap()))
            .collect::<Vec<_>>();
        let mut rng = crate::rng();

        let client_provider = move || clients.choose(&mut rng).unwrap().clone();

        Simulation {
            runtime: self.runtime,
            metrics: self.metrics,
            recorder: self.recorder,
            requests: self.requests,
            client_provider: Box::new(client_provider),
            abort_after: self.abort_after,
        }
    }
}

pub struct Simulation {
    runtime: Runtime,
    metrics: Arc<MetricRegistry>,
    recorder: Arc<Mutex<SimulationMetricsRecorder>>,
    client_provider: Box<dyn FnMut() -> Client<Arc<SimulationRawClient>>>,
    requests: Pin<Box<dyn Stream<Item = Endpoint>>>,
    abort_after: Option<Duration>,
}

impl Simulation {
    pub fn builder() -> SimulationBuilder0 {
        SimulationBuilder0
    }

    pub fn run(mut self) -> SimulationReport {
        self.runtime.block_on({
            let metrics = self.metrics;
            let recorder = self.recorder;
            let mut client_provider = self.client_provider;
            let requests = self.requests;
            let abort_after = self.abort_after;
            async move {
                let start = Instant::now();

                let status_codes = RefCell::new(BTreeMap::new());
                let num_sent = Cell::new(0);
                let num_received = Cell::new(0);

                let run_requests = requests.for_each_concurrent(None, {
                    let status_codes = &status_codes;
                    let num_sent = &num_sent;
                    let num_received = &num_received;
                    move |endpoint| {
                        let client = client_provider();
                        async move {
                            num_sent.set(num_sent.get() + 1);
                            let response = client
                                .request(endpoint.method().clone(), endpoint.path())
                                .send()
                                .await;
                            num_received.set(num_received.get() + 1);

                            let status = match response {
                                Ok(response) => response.status(),
                                Err(error) => {
                                    if let Some(error) = error.cause().downcast_ref::<RemoteError>()
                                    {
                                        *error.status()
                                    } else if error.cause().is::<UnavailableError>() {
                                        StatusCode::SERVICE_UNAVAILABLE
                                    } else if error.cause().is::<ThrottledError>() {
                                        StatusCode::TOO_MANY_REQUESTS
                                    } else {
                                        panic!("unexpected client error {:?}", error);
                                    }
                                }
                            };

                            *status_codes
                                .borrow_mut()
                                .entry(status.as_u16())
                                .or_insert(0) += 1;
                        }
                    }
                });

                match abort_after {
                    Some(abort_after) => {
                        let _ = time::timeout(abort_after, run_requests).await;
                    }
                    None => run_requests.await,
                }

                let status_codes = status_codes.into_inner();
                SimulationReport {
                    end_time: start.elapsed(),
                    client_mean: Duration::from_nanos(
                        metrics::responses_timer(&metrics, SERVICE)
                            .snapshot()
                            .mean() as u64,
                    ),
                    success_percentage: f64::round(
                        status_codes.get(&200).copied().unwrap_or(0) as f64 * 1000.
                            / num_sent.get() as f64,
                    ) / 10.,
                    server_cpu: Duration::from_nanos(
                        metrics::global_server_time_nanos(&metrics).count() as u64,
                    ),
                    status_codes,
                    num_sent: num_sent.get(),
                    num_received: num_received.get(),
                    num_global_responses: metrics::global_responses(&metrics).count(),
                    record: recorder.lock().finish(),
                }
            }
        })
    }
}

#[derive(Copy, Clone)]
pub enum Strategy {
    ConcurrencyLimiterRoundRobin,
    ConcurrencyLimiterPinUntilError,
    UnlimitedRoundRobin,
}

impl fmt::Display for Strategy {
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Strategy::ConcurrencyLimiterRoundRobin => "CONCURRENCY_LIMITER_ROUND_ROBIN",
            Strategy::ConcurrencyLimiterPinUntilError => "CONCURRENCY_LIMITER_PIN_UNTIL_ERROR",
            Strategy::UnlimitedRoundRobin => "UNLIMITED_ROUND_ROBIN",
        };

        fmt::Display::fmt(s, fmt)
    }
}

impl Strategy {
    fn apply(&self, builder: &mut Builder) {
        match self {
            Strategy::ConcurrencyLimiterRoundRobin => {
                builder.node_selection_strategy(NodeSelectionStrategy::Balanced);
            }
            Strategy::ConcurrencyLimiterPinUntilError => {
                builder.node_selection_strategy(NodeSelectionStrategy::PinUntilError);
            }
            Strategy::UnlimitedRoundRobin => {
                // FIXME disable qos
                builder.node_selection_strategy(NodeSelectionStrategy::Balanced);
            }
        }
    }
}

pub struct SimulationReport {
    pub end_time: Duration,
    pub client_mean: Duration,
    pub success_percentage: f64,
    pub server_cpu: Duration,
    pub num_sent: u64,
    pub num_received: u64,
    pub num_global_responses: i64,
    pub status_codes: BTreeMap<u16, u64>,
    pub record: MetricsRecord,
}
