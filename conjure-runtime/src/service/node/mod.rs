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
use crate::raw::Service;
use crate::service::node::limiter::{InFlightReducer, LimitReducer, Limiter, Permit};
pub use crate::service::node::metrics::NodeMetricsLayer;
pub use crate::service::node::selector::NodeSelectorLayer;
pub use crate::service::node::uri::NodeUriLayer;
use crate::service::request::Pattern;
use crate::util::weak_reducing_gauge::WeakReducingGauge;
use crate::{Builder, ClientQos, HostMetrics};
use conjure_error::Error;
use futures::ready;
use http::{Request, Response};
use pin_project::pin_project;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use url::Url;
use witchcraft_metrics::MetricId;

pub mod limiter;
pub mod metrics;
pub mod selector;
pub mod uri;

pub struct LimitedNode {
    node: Arc<Node>,
    limiter: Option<Limiter>,
}

impl LimitedNode {
    #[cfg(test)]
    fn test(url: &str) -> Self {
        LimitedNode {
            node: Node::test(url),
            limiter: None,
        }
    }

    pub fn new<T>(idx: usize, url: &Url, service: &str, builder: &Builder<T>) -> Self {
        let node = LimitedNode {
            node: Arc::new(Node {
                idx,
                url: url.clone(),
                host_metrics: builder.get_host_metrics().map(|m| {
                    m.get(
                        service,
                        url.host_str().unwrap(),
                        url.port_or_known_default().unwrap(),
                    )
                }),
            }),
            limiter: match builder.get_client_qos() {
                ClientQos::Enabled => Some(Limiter::new()),
                ClientQos::DangerousDisableSympatheticClientQos => None,
            },
        };

        if let (Some(metrics), Some(limiter)) = (builder.get_metrics(), &node.limiter) {
            metrics
                .gauge_with(
                    MetricId::new("conjure-runtime.concurrencylimiter.max")
                        .with_tag("service", service.to_string())
                        .with_tag("hostIndex", idx.to_string()),
                    || WeakReducingGauge::new(LimitReducer),
                )
                .downcast_ref::<WeakReducingGauge<LimitReducer>>()
                .expect("conjure-runtime.concurrencylimiter.max metric already registered")
                .push(limiter.host_limiter());

            metrics
                .gauge_with(
                    MetricId::new("conjure-runtime.concurrencylimiter.in-flight")
                        .with_tag("service", service.to_string())
                        .with_tag("hostIndex", idx.to_string()),
                    || WeakReducingGauge::new(InFlightReducer),
                )
                .downcast_ref::<WeakReducingGauge<InFlightReducer>>()
                .expect("conjure-runtime.concurrencylimiter.in-flight metric already registered")
                .push(limiter.host_limiter());
        }

        node
    }

    pub fn acquire<B>(&self, request: &Request<B>) -> Acquire {
        let pattern = request
            .extensions()
            .get::<Pattern>()
            .expect("Pattern extension missing from request");

        Acquire {
            acquire: self
                .limiter
                .as_ref()
                .map(|l| l.acquire(request.method(), &pattern.pattern)),
            node: self.node.clone(),
        }
    }

    pub fn wrap<S, B1, B2>(&self, inner: &Arc<S>, request: Request<B1>) -> Wrap<S, B1>
    where
        S: Service<Request<B1>, Response = Response<B2>, Error = Error>,
    {
        // don't create the span if client QoS is disabled
        if self.limiter.is_some() {
            let span = zipkin::next_span()
                .with_name("conjure-runtime: acquire permit")
                .with_tag("node", &self.node.idx.to_string());

            Wrap::Acquire {
                future: span.detach().bind(self.acquire(&request)),
                inner: inner.clone(),
                request: Some(request),
            }
        } else {
            Wrap::NodeFuture {
                future: AcquiredNode {
                    node: self.node.clone(),
                    permit: None,
                }
                .wrap(&**inner, request),
            }
        }
    }
}

#[pin_project]
pub struct Acquire {
    node: Arc<Node>,
    #[pin]
    acquire: Option<limiter::Acquire>,
}

impl Future for Acquire {
    type Output = AcquiredNode;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();
        let permit = match this.acquire.as_pin_mut().map(|a| a.poll(cx)) {
            Some(Poll::Ready(permit)) => Some(permit),
            Some(Poll::Pending) => return Poll::Pending,
            None => None,
        };

        Poll::Ready(AcquiredNode {
            node: this.node.clone(),
            permit,
        })
    }
}

pub struct AcquiredNode {
    node: Arc<Node>,
    permit: Option<Permit>,
}

impl AcquiredNode {
    pub fn wrap<S, B1, B2>(self, inner: &S, mut req: Request<B1>) -> NodeFuture<S::Future>
    where
        S: Service<Request<B1>, Response = Response<B2>, Error = Error>,
    {
        req.extensions_mut().insert(self.node.clone());

        NodeFuture {
            future: inner.call(req),
            permit: self.permit,
        }
    }
}

#[pin_project]
pub struct NodeFuture<F> {
    #[pin]
    future: F,
    permit: Option<Permit>,
}

impl<F, B> Future for NodeFuture<F>
where
    F: Future<Output = Result<Response<B>, Error>>,
{
    type Output = F::Output;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();
        let response = ready!(this.future.poll(cx));
        if let Some(permit) = this.permit {
            permit.on_response(&response);
        }

        Poll::Ready(response)
    }
}

pub struct Node {
    idx: usize,
    url: Url,
    host_metrics: Option<Arc<HostMetrics>>,
}

impl Node {
    #[cfg(test)]
    fn test(url: &str) -> Arc<Self> {
        Arc::new(Node {
            idx: 0,
            url: url.parse().unwrap(),
            host_metrics: None,
        })
    }
}

#[pin_project(project = WrapProject)]
pub enum Wrap<S, B>
where
    S: Service<Request<B>>,
{
    Acquire {
        #[pin]
        future: zipkin::Bind<Acquire>,
        inner: Arc<S>,
        request: Option<Request<B>>,
    },
    NodeFuture {
        #[pin]
        future: NodeFuture<S::Future>,
    },
}

impl<S, B1, B2> Future for Wrap<S, B1>
where
    S: Service<Request<B1>, Response = Response<B2>, Error = Error>,
{
    type Output = Result<S::Response, S::Error>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        loop {
            let new_self = match self.as_mut().project() {
                WrapProject::Acquire {
                    future,
                    inner,
                    request,
                } => {
                    let acquired = ready!(future.poll(cx));
                    let request = request.take().unwrap();

                    Wrap::NodeFuture {
                        future: acquired.wrap(&**inner, request),
                    }
                }
                WrapProject::NodeFuture { future } => return future.poll(cx),
            };
            self.set(new_self);
        }
    }
}
