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
    Endpoint, Server, ServerBuilder0, ServerBuilder1, SimulationRawClient,
    SimulationRawClientBuilder,
};
use async_stream::stream;
use conjure_runtime::errors::{RemoteError, ThrottledError, UnavailableError};
use conjure_runtime::{Agent, Builder, Client, ClientQos, NodeSelectionStrategy, UserAgent};
use futures::stream::{Stream, StreamExt};
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
use witchcraft_metrics::{Clock, MetricRegistry};

const SERVICE: &str = "simulation";

pub struct SimulationBuilder0;

impl SimulationBuilder0 {
    pub fn strategy(self, strategy: Strategy) -> SimulationBuilder1 {
        let runtime = runtime::Builder::new_current_thread()
            .enable_time()
            .start_paused(true)
            .build()
            .unwrap();

        let mut metrics = MetricRegistry::new();
        metrics.set_clock(Arc::new(TokioClock));
        let metrics = Arc::new(metrics);

        // initialize the responses timer with our custom reservoir
        let _guard = runtime.enter();
        metrics::responses_timer(&metrics, SERVICE);

        let mut recorder = SimulationMetricsRecorder::new(&metrics);
        recorder.filter_metrics(|id| {
            id.name().ends_with("activeRequests") || id.name().ends_with("request")
        });

        SimulationBuilder1 {
            runtime,
            servers: vec![],
            metrics,
            recorder,
            endpoints: vec![Endpoint::DEFAULT],
            strategy,
        }
    }
}

pub struct SimulationBuilder1 {
    runtime: Runtime,
    metrics: Arc<MetricRegistry>,
    recorder: SimulationMetricsRecorder,
    servers: Vec<Server>,
    endpoints: Vec<Endpoint>,
    strategy: Strategy,
}

impl SimulationBuilder1 {
    pub fn server<F>(mut self, f: F) -> Self
    where
        F: FnOnce(ServerBuilder0) -> ServerBuilder1,
    {
        let _guard = self.runtime.enter();
        let server = f(ServerBuilder0 {
            metrics: &self.metrics,
            recorder: &mut self.recorder,
        });
        self.servers.push(server.build());
        self
    }

    pub fn endpoints(mut self, endpoints: Vec<Endpoint>) -> Self {
        self.endpoints = endpoints;
        self
    }

    pub fn requests_per_second(self, requests_per_second: f64) -> SimulationBuilder2 {
        let recorder = Arc::new(Mutex::new(self.recorder));

        let mut builder = Builder::new();
        self.strategy.apply(&mut builder);
        builder
            .service(SERVICE)
            .user_agent(UserAgent::new(Agent::new("simulation", "0.0.0")))
            .metrics(self.metrics.clone());
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
            delay_between_requests: Duration::from_secs_f64(1. / requests_per_second),
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

        let delay_between_requests = self.delay_between_requests;
        let endpoints = self.endpoints;
        let stream = stream! {
            let mut interval = time::interval(delay_between_requests);
            for _ in 0..num_requests {
                interval.tick().await;
                yield endpoints.choose(&mut rng).unwrap().clone();
            }
        };

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
        let runtime = self.runtime;
        let mut builder = self.builder;
        let clients = (0..clients)
            .map(|i| {
                let _guard = runtime.enter();
                builder.rng_seed(u64::from(i)).build().unwrap()
            })
            .collect::<Vec<_>>();
        let mut rng = crate::rng();

        let client_provider = move || clients.choose(&mut rng).unwrap().clone();

        Simulation {
            runtime,
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

    pub fn run(self) -> SimulationReport {
        let runner = Runner {
            metrics: self.metrics,
            recorder: self.recorder,
            client_provider: self.client_provider,
            requests: self.requests,
            abort_after: self.abort_after,
        };

        self.runtime.block_on(runner.run())
    }
}

struct Runner {
    metrics: Arc<MetricRegistry>,
    recorder: Arc<Mutex<SimulationMetricsRecorder>>,
    client_provider: Box<dyn FnMut() -> Client<Arc<SimulationRawClient>>>,
    requests: Pin<Box<dyn Stream<Item = Endpoint>>>,
    abort_after: Option<Duration>,
}

impl Runner {
    async fn run(mut self) -> SimulationReport {
        let request_runner = RequestRunner {
            status_codes: RefCell::new(BTreeMap::new()),
            num_sent: Cell::new(0),
            num_received: Cell::new(0),
        };

        let run_requests = self.requests.for_each_concurrent(None, {
            let client_provider = &mut self.client_provider;
            let request_runner = &request_runner;
            move |endpoint| {
                let client = client_provider();
                request_runner.run(client, endpoint)
            }
        });

        match self.abort_after {
            Some(abort_after) => {
                let _ = time::timeout(abort_after, run_requests).await;
            }
            None => run_requests.await,
        }

        self.recorder.lock().record();

        let status_codes = request_runner.status_codes.into_inner();
        SimulationReport {
            client_mean: Duration::from_nanos(
                metrics::responses_timer(&self.metrics, SERVICE)
                    .snapshot()
                    .mean() as u64,
            ),
            success_percentage: f64::round(
                status_codes.get(&200).copied().unwrap_or(0) as f64 * 1000.
                    / request_runner.num_sent.get() as f64,
            ) / 10.,
            server_cpu: Duration::from_nanos(
                metrics::global_server_time_nanos(&self.metrics).count() as u64,
            ),
            status_codes,
            num_sent: request_runner.num_sent.get(),
            num_received: request_runner.num_received.get(),
            num_global_responses: metrics::global_responses(&self.metrics).count(),
            record: self.recorder.lock().finish(),
        }
    }
}

struct RequestRunner {
    status_codes: RefCell<BTreeMap<u16, u64>>,
    num_sent: Cell<u64>,
    num_received: Cell<u64>,
}

impl RequestRunner {
    async fn run(&self, client: Client<Arc<SimulationRawClient>>, endpoint: Endpoint) {
        self.num_sent.set(self.num_sent.get() + 1);
        let response = client
            .request(endpoint.method().clone(), endpoint.path())
            .send()
            .await;
        self.num_received.set(self.num_received.get() + 1);

        let status = match response {
            Ok(response) => response.status(),
            Err(error) => {
                if let Some(error) = error.cause().downcast_ref::<RemoteError>() {
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

        *self
            .status_codes
            .borrow_mut()
            .entry(status.as_u16())
            .or_insert(0) += 1;
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
                builder
                    .node_selection_strategy(NodeSelectionStrategy::Balanced)
                    .client_qos(ClientQos::DangerousDisableSympatheticClientQos);
            }
        }
    }
}

pub struct SimulationReport {
    pub client_mean: Duration,
    pub success_percentage: f64,
    pub server_cpu: Duration,
    pub num_sent: u64,
    pub num_received: u64,
    pub num_global_responses: i64,
    pub status_codes: BTreeMap<u16, u64>,
    pub record: MetricsRecord,
}

pub struct TokioClock;

impl Clock for TokioClock {
    fn now(&self) -> std::time::Instant {
        Instant::now().into_std()
    }
}
