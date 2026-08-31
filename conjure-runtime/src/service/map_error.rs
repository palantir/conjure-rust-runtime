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
use crate::errors::TransportError;
use crate::service::{Layer, Service};
use conjure_error::Error;
use std::error;

/// A layer which sits directly on top of the raw HTTP client service, wrapping its errors in `TransportError` and then
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
    S: Service<R>,
    S::Error: Into<Box<dyn error::Error + Sync + Send>>,
{
    type Response = S::Response;
    type Error = Error;

    async fn call(&self, req: R) -> Result<Self::Response, Self::Error> {
        self.inner
            .call(req)
            .await
            .map_err(|e| Error::internal_safe(TransportError::new(e)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::service;
    use std::io;

    #[tokio::test]
    async fn wraps_transport_errors() {
        let service = MapErrorLayer.layer(service::service_fn(|()| async {
            Err::<(), _>(io::Error::other("connection refused"))
        }));

        let error = service.call(()).await.unwrap_err();

        assert!(error.cause().is::<TransportError>());
        assert!(error
            .cause()
            .source()
            .is_some_and(|cause| cause.is::<io::Error>()));
    }
}
