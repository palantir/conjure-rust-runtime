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
use conjure_error::Error;
use std::error;
use std::fmt;

#[derive(Debug)]
pub struct RawClientError(pub Box<dyn error::Error + Sync + Send>);

impl fmt::Display for RawClientError {
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt.write_str("raw HTTP client error")
    }
}

impl error::Error for RawClientError {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        Some(&*self.0)
    }
}

/// A layer which sits directly on top of the raw HTTP client service, wrapping its errors in `RawClientError` and then
/// converting them into an internal service `conjure_error::Error`.
pub struct MapErrorLayer;

impl<S> Layer<S> for MapErrorLayer {
    type Service = MapErrorService<S>;

    fn layer(self, inner: S) -> MapErrorService<S> {
        MapErrorService { inner }
    }
}

pub struct MapErrorService<S> {
    inner: S,
}

impl<S, R> Service<R> for MapErrorService<S>
where
    S: Service<R> + Sync + Send,
    S::Error: Into<Box<dyn error::Error + Sync + Send>>,
    R: Sync + Send,
{
    type Response = S::Response;
    type Error = Error;

    async fn call(&self, req: R) -> Result<Self::Response, Self::Error> {
        self.inner
            .call(req)
            .await
            .map_err(|e| Error::internal_safe(RawClientError(e.into())))
    }
}
