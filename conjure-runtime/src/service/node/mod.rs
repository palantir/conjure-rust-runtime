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
use crate::util::weak_reducing_gauge::WeakReducingGauge;
use crate::{builder, Builder, ClientQos, HostMetrics};
use conjure_error::Error;
use conjure_http::client::Endpoint;
use http::{Request, Response};
use std::sync::Arc;
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

    pub fn new<T>(
        idx: usize,
        url: &Url,
        service: &str,
        builder: &Builder<builder::Complete<T>>,
    ) -> Self {
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
            limiter: if builder.mesh_mode() {
                None
            } else {
                match builder.get_client_qos() {
                    ClientQos::Enabled => Some(Limiter::new()),
                    ClientQos::DangerousDisableSympatheticClientQos => None,
                }
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

    pub async fn acquire<B>(&self, request: &Request<B>) -> AcquiredNode {
        let endpoint = request
            .extensions()
            .get::<Endpoint>()
            .expect("Endpoint extension missing from request");

        let permit = match &self.limiter {
            Some(limiter) => {
                let permit = limiter.acquire(request.method(), endpoint.path()).await;
                Some(permit)
            }
            None => None,
        };

        AcquiredNode {
            node: self.node.clone(),
            permit,
        }
    }

    pub async fn wrap<S, B1, B2>(
        &self,
        inner: &S,
        request: Request<B1>,
    ) -> Result<S::Response, S::Error>
    where
        S: Service<Request<B1>, Response = Response<B2>, Error = Error>,
    {
        // Don't create the span if client QoS is disabled.
        if self.limiter.is_some() {
            let span = zipkin::next_span()
                .with_name("conjure-runtime: acquire-permit")
                .with_tag("node", &self.node.idx.to_string());
            let permit = span.detach().bind(self.acquire(&request)).await;
            permit.wrap(inner, request).await
        } else {
            AcquiredNode {
                node: self.node.clone(),
                permit: None,
            }
            .wrap(inner, request)
            .await
        }
    }
}

pub struct AcquiredNode {
    node: Arc<Node>,
    permit: Option<Permit>,
}

impl AcquiredNode {
    pub async fn wrap<S, B1, B2>(
        self,
        inner: &S,
        mut req: Request<B1>,
    ) -> Result<S::Response, S::Error>
    where
        S: Service<Request<B1>, Response = Response<B2>, Error = Error>,
    {
        req.extensions_mut().insert(self.node);

        let response = inner.call(req).await;
        if let Some(mut permit) = self.permit {
            permit.on_response(&response);
        }

        response
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
