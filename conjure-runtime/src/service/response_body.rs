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
use crate::{BaseBody, ResponseBody};
use bytes::Bytes;
use http::Response;
use http_body::Body;

/// A layer which wraps the response body in the conjure-runtime public `ResponseBody` type.
pub struct ResponseBodyLayer;

impl<S> Layer<S> for ResponseBodyLayer {
    type Service = ResponseBodyService<S>;

    fn layer(self, inner: S) -> Self::Service {
        ResponseBodyService { inner }
    }
}

pub struct ResponseBodyService<S> {
    inner: S,
}

impl<S, R, B> Service<R> for ResponseBodyService<S>
where
    S: Service<R, Response = Response<BaseBody<B>>> + Sync + Send,
    R: Send,
    B: Body<Data = Bytes>,
{
    type Response = Response<ResponseBody<B>>;
    type Error = S::Error;

    async fn call(&self, req: R) -> Result<Self::Response, Self::Error> {
        self.inner.call(req).await.map(|r| r.map(ResponseBody::new))
    }
}
