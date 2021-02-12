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
use crate::recorder::SimulationMetricsRecorder;
use bytes::Bytes;
use conjure_error::Error;
use conjure_runtime::raw::{BuildRawClient, RawBody, Service};
use conjure_runtime::Builder;
use futures::future::BoxFuture;
use http::{HeaderMap, Method, Request, Response};
use http_body::Body;
use parking_lot::Mutex;
use rand::Rng;
use std::collections::HashMap;
use std::error;
use std::mem;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use tokio::time::{self, Duration, Instant};
use witchcraft_metrics::{Counter, MetricRegistry};

#[derive(Clone)]
pub struct Endpoint {
    method: Method,
    path: &'static str,
}

impl Endpoint {
    pub const DEFAULT: Endpoint = Endpoint::post("/");

    pub const fn post(path: &'static str) -> Self {
        Endpoint {
            method: Method::POST,
            path,
        }
    }

    pub const fn get(path: &'static str) -> Self {
        Endpoint {
            method: Method::GET,
            path,
        }
    }

    pub fn method(&self) -> &Method {
        &self.method
    }

    pub fn path(&self) -> &'static str {
        self.path
    }
}

pub struct ServerBuilder0<'a> {
    pub metrics: &'a MetricRegistry,
    pub recorder: &'a mut SimulationMetricsRecorder,
}

impl<'a> ServerBuilder0<'a> {
    pub fn name(self, name: &str) -> ServerBuilder1<'a> {
        ServerBuilder1 {
            name: name.to_string(),
            active_requests: metrics::active_requests(&self.metrics, name),
            handlers: vec![],
            recorder: self.recorder,
        }
    }
}

pub struct ServerBuilder1<'a> {
    name: String,
    active_requests: Arc<Counter>,
    handlers: Vec<Handler>,
    recorder: &'a mut SimulationMetricsRecorder,
}

impl ServerBuilder1<'_> {
    pub fn handler<F>(mut self, f: F) -> Self
    where
        F: FnOnce(HandlerBuilder0) -> Handler,
    {
        let handler = f(HandlerBuilder0 {
            predicate: Box::new(|_| true),
        });
        self.handlers.push(handler);
        self
    }

    pub fn until(mut self, cutover: Duration, name: &'static str) -> Self {
        self.recorder.event(cutover, name);

        let cutover = Instant::now() + cutover;

        for handler in &mut self.handlers {
            let old_predicate = mem::replace(&mut handler.predicate, Box::new(|_| true));
            handler.predicate =
                Box::new(move |server| Instant::now() < cutover && old_predicate(server));
        }

        self
    }

    pub fn build(self) -> Server {
        Server {
            name: self.name,
            active_requests: self.active_requests,
            handlers: self.handlers,
        }
    }
}

pub struct Server {
    name: String,
    active_requests: Arc<Counter>,
    handlers: Vec<Handler>,
}

impl Server {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn active_requests(&self) -> i64 {
        self.active_requests.count()
    }
}

pub struct HandlerBuilder0 {
    predicate: Box<dyn Fn(&Request<RawBody>) -> bool + Sync + Send>,
}

impl HandlerBuilder0 {
    pub fn endpoint(mut self, endpoint: Endpoint) -> Self {
        self.predicate = Box::new(move |req| {
            *req.method() == endpoint.method() && req.uri().path() == endpoint.path()
        });
        self
    }

    pub fn response(self, status: u16) -> HandlerBuilder1 {
        self.response_with(move |_| response(status))
    }

    pub fn response_with<F>(self, f: F) -> HandlerBuilder1
    where
        F: Fn(&Server) -> Response<EmptyBody> + 'static + Sync + Send,
    {
        HandlerBuilder1 {
            predicate: self.predicate,
            response: Box::new(f),
        }
    }

    pub fn respond_200_until_capacity(self, error_status: u16, capacity: u32) -> HandlerBuilder1 {
        self.response_with(move |server| {
            if server.active_requests() > capacity as i64 {
                response(error_status)
            } else {
                response(200)
            }
        })
    }

    pub fn respond_500_at_rate(self, rate: f64) -> HandlerBuilder1 {
        let rng = Mutex::new(crate::rng());
        self.response_with(move |_| {
            if rng.lock().gen_bool(rate) {
                response(500)
            } else {
                response(200)
            }
        })
    }
}

pub struct HandlerBuilder1 {
    predicate: Box<dyn Fn(&Request<RawBody>) -> bool + Sync + Send>,
    response: Box<dyn Fn(&Server) -> Response<EmptyBody> + Sync + Send>,
}

impl HandlerBuilder1 {
    pub fn response_time(self, response_time: Duration) -> Handler {
        self.response_time_with(move |_| response_time)
    }

    pub fn response_time_with<F>(self, f: F) -> Handler
    where
        F: Fn(&Server) -> Duration + 'static + Sync + Send,
    {
        Handler {
            predicate: self.predicate,
            response: self.response,
            response_time: Box::new(f),
        }
    }

    pub fn linear_response_time(self, best_case: Duration, capacity: u32) -> Handler {
        self.response_time_with(move |server| {
            let in_flight = server.active_requests();

            if in_flight > capacity as i64 {
                best_case * 5
            } else {
                best_case + best_case * in_flight as u32 / capacity
            }
        })
    }
}

pub struct Handler {
    predicate: Box<dyn Fn(&Request<RawBody>) -> bool + Sync + Send>,
    response: Box<dyn Fn(&Server) -> Response<EmptyBody> + Sync + Send>,
    response_time: Box<dyn Fn(&Server) -> Duration + Sync + Send>,
}

pub fn response(status: u16) -> Response<EmptyBody> {
    Response::builder().status(status).body(EmptyBody).unwrap()
}

pub struct EmptyBody;

impl Body for EmptyBody {
    type Data = Bytes;
    type Error = Box<dyn error::Error + Sync + Send>;

    fn poll_data(
        self: Pin<&mut Self>,
        _: &mut Context<'_>,
    ) -> Poll<Option<Result<Self::Data, Self::Error>>> {
        Poll::Ready(None)
    }

    fn poll_trailers(
        self: Pin<&mut Self>,
        _: &mut Context<'_>,
    ) -> Poll<Result<Option<HeaderMap>, Self::Error>> {
        Poll::Ready(Ok(None))
    }
}

pub struct SimulationRawClient {
    servers: HashMap<String, Server>,
    recorder: Arc<Mutex<SimulationMetricsRecorder>>,
    metrics: Arc<MetricRegistry>,
    global_responses: Arc<Counter>,
    global_server_time_nanos: Arc<Counter>,
}

impl Service<Request<RawBody>> for SimulationRawClient {
    type Response = Response<EmptyBody>;
    type Error = Box<dyn error::Error + Sync + Send>;
    type Future = BoxFuture<'static, Result<Self::Response, Self::Error>>;

    fn call(&self, req: Request<RawBody>) -> Self::Future {
        let server = &self.servers[req.uri().host().unwrap()];

        let handler = server
            .handlers
            .iter()
            .find(|h| (h.predicate)(&req))
            .expect("no handler available for request");

        server.active_requests.inc();
        metrics::request_counter(&self.metrics, &server.name, req.uri().path()).inc();
        self.recorder.lock().record();

        let response = (handler.response)(server);
        let response_time = (handler.response_time)(server);
        let start = Instant::now();

        Box::pin({
            let active_requests = server.active_requests.clone();
            let global_responses = self.global_responses.clone();
            let global_server_time_nanos = self.global_server_time_nanos.clone();
            let recorder = self.recorder.clone();
            async move {
                time::sleep(response_time).await;

                active_requests.dec();
                global_responses.inc();
                global_server_time_nanos.add(start.elapsed().as_nanos() as i64);
                recorder.lock().record();

                Ok(response)
            }
        })
    }
}

pub struct SimulationRawClientBuilder {
    client: Arc<SimulationRawClient>,
}

impl SimulationRawClientBuilder {
    pub fn new(
        servers: Vec<Server>,
        metrics: &Arc<MetricRegistry>,
        recorder: &Arc<Mutex<SimulationMetricsRecorder>>,
    ) -> Self {
        SimulationRawClientBuilder {
            client: Arc::new(SimulationRawClient {
                servers: servers.into_iter().map(|s| (s.name.clone(), s)).collect(),
                recorder: recorder.clone(),
                metrics: metrics.clone(),
                global_responses: metrics::global_responses(metrics),
                global_server_time_nanos: metrics::global_server_time_nanos(metrics),
            }),
        }
    }
}

impl BuildRawClient for SimulationRawClientBuilder {
    type RawClient = Arc<SimulationRawClient>;

    fn build_raw_client(&self, _: &Builder<Self>) -> Result<Self::RawClient, Error> {
        Ok(self.client.clone())
    }
}
