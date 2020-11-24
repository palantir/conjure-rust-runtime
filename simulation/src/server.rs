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

    pub const fn post(path: &'static str) -> Endpoint {
        Endpoint {
            method: Method::POST,
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
}

impl ServerBuilder0<'_> {
    pub fn name(self, name: &'static str) -> Server {
        Server {
            name,
            active_requests: metrics::active_requests(&self.metrics, name),
            handlers: vec![],
        }
    }
}

pub struct Server {
    name: &'static str,
    active_requests: Arc<Counter>,
    handlers: Vec<Handler>,
}

impl Server {
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

    pub fn until(mut self, cutover: Duration) -> Self {
        let cutover = Instant::now() + cutover;

        for handler in &mut self.handlers {
            let old_predicate = mem::replace(&mut handler.predicate, Box::new(|_| true));
            handler.predicate =
                Box::new(move |server| Instant::now() < cutover && old_predicate(server));
        }

        self
    }

    pub fn name(&self) -> &'static str {
        self.name
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
        self.response_with(move |_| Response::builder().status(status).body(EmptyBody).unwrap())
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
}

pub struct HandlerBuilder1 {
    predicate: Box<dyn Fn(&Request<RawBody>) -> bool + Sync + Send>,
    response: Box<dyn Fn(&Server) -> Response<EmptyBody> + Sync + Send>,
}

impl HandlerBuilder1 {
    pub fn delay(self, delay: Duration) -> Handler {
        self.delay_with(move |_| delay)
    }

    pub fn delay_with<F>(self, f: F) -> Handler
    where
        F: Fn(&Server) -> Duration + 'static + Sync + Send,
    {
        Handler {
            predicate: self.predicate,
            response: self.response,
            delay: Box::new(f),
        }
    }
}

pub struct Handler {
    predicate: Box<dyn Fn(&Request<RawBody>) -> bool + Sync + Send>,
    response: Box<dyn Fn(&Server) -> Response<EmptyBody> + Sync + Send>,
    delay: Box<dyn Fn(&Server) -> Duration + Sync + Send>,
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
    servers: HashMap<&'static str, Server>,
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
            .filter(|h| (h.predicate)(&req))
            .next()
            .expect("no handler available for request");

        server.active_requests.inc();
        metrics::request_meter(&self.metrics, server.name, req.uri().path()).mark(1);
        self.recorder.lock().record();

        let response = (handler.response)(server);
        let delay = (handler.delay)(server);
        let start = Instant::now();

        Box::pin({
            let active_requests = server.active_requests.clone();
            let global_responses = self.global_responses.clone();
            let global_server_time_nanos = self.global_server_time_nanos.clone();
            async move {
                time::delay_for(delay).await;

                active_requests.dec();
                global_responses.inc();
                global_server_time_nanos.add(start.elapsed().as_nanos() as i64);

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
                servers: servers.into_iter().map(|s| (s.name, s)).collect(),
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
