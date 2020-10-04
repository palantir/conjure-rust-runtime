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
use std::task::{Context, Poll};
use tower::layer::Layer;
use tower::Service;
use zipkin::Bind;

/// A layer which wraps the execution of a service's future in a Zipkin span.
pub struct SpanLayer {
    name: &'static str,
}

impl SpanLayer {
    pub fn new(name: &'static str) -> SpanLayer {
        SpanLayer { name }
    }
}

impl<S> Layer<S> for SpanLayer {
    type Service = SpanService<S>;

    fn layer(&self, inner: S) -> SpanService<S> {
        SpanService {
            inner,
            name: self.name,
        }
    }
}

pub struct SpanService<S> {
    inner: S,
    name: &'static str,
}

impl<S, R> Service<R> for SpanService<S>
where
    S: Service<R>,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = Bind<S::Future>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: R) -> Self::Future {
        zipkin::next_span()
            .with_name(self.name)
            .detach()
            .bind(self.inner.call(req))
    }
}
