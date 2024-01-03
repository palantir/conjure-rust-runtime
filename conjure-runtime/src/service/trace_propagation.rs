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
use crate::service::Layer;
use http::Request;
use std::future::Future;

/// A request layer which injects Zipkin tracing information into an outgoing request's headers.
///
/// It uses the old `X-B3-*` headers rather than the new `b3` header.
pub struct TracePropagationLayer;

impl<S> Layer<S> for TracePropagationLayer {
    type Service = TracePropagationService<S>;

    fn layer(self, inner: S) -> TracePropagationService<S> {
        TracePropagationService { inner }
    }
}

pub struct TracePropagationService<S> {
    inner: S,
}

impl<S, B> Service<Request<B>> for TracePropagationService<S>
where
    S: Service<Request<B>>,
{
    type Response = S::Response;
    type Error = S::Error;

    fn call(
        &self,
        mut req: Request<B>,
    ) -> impl Future<Output = Result<Self::Response, Self::Error>> {
        if let Some(context) = zipkin::current() {
            http_zipkin::set_trace_context(context, req.headers_mut());
        }

        self.inner.call(req)
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::service;

    #[tokio::test]
    async fn no_span() {
        let service =
            TracePropagationLayer.layer(service::service_fn(|req| async { Ok::<_, ()>(req) }));

        let out = service.call(Request::new(())).await.unwrap();
        let out_context = http_zipkin::get_trace_context(out.headers());

        assert_eq!(out_context, None);
    }

    #[tokio::test]
    async fn with_span() {
        let span = zipkin::next_span().detach();
        let context = span.context();

        let service =
            TracePropagationLayer.layer(service::service_fn(|req| async { Ok::<_, ()>(req) }));

        let out = span
            .bind(async { service.call(Request::new(())).await })
            .await
            .unwrap();
        let out_context = http_zipkin::get_trace_context(out.headers());

        assert_eq!(out_context, Some(context));
    }
}
