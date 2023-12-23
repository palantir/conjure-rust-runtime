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
use crate::service::{Layer, Service};
use crate::util::spans::{self, HttpSpanFuture};
use conjure_error::Error;
use http::{Request, Response};
use std::future::Future;

/// A layer which manages the root level request span.
pub struct RootSpanLayer;

impl<S> Layer<S> for RootSpanLayer {
    type Service = RootSpanService<S>;

    fn layer(self, inner: S) -> Self::Service {
        RootSpanService { inner }
    }
}

pub struct RootSpanService<S> {
    inner: S,
}

impl<S, B1, B2> Service<Request<B1>> for RootSpanService<S>
where
    S: Service<Request<B1>, Response = Response<B2>, Error = Error>,
{
    type Response = S::Response;
    type Error = S::Error;

    fn call(&self, req: Request<B1>) -> impl Future<Output = Result<Self::Response, Self::Error>> {
        let mut span = zipkin::next_span()
            .with_name("conjure-runtime: request")
            .detach();
        spans::add_request_tags(&mut span, &req);

        HttpSpanFuture::new(self.inner.call(req), span)
    }
}
