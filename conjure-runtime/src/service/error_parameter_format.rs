// Copyright 2026 Palantir Technologies, Inc.
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
use crate::{builder, Builder};
use http::header::Entry;
use http::{HeaderName, HeaderValue, Request};
use std::future::Future;

/// The request header used to negotiate the serialization format of Conjure error parameters.
pub const ACCEPT_CONJURE_ERROR_PARAMETER_FORMAT: HeaderName =
    HeaderName::from_static("accept-conjure-error-parameter-format");

/// A layer which injects an `Accept-Conjure-Error-Parameter-Format` header into requests.
///
/// The header is only added if the client is configured with a non-legacy format, and if the request does not already
/// contain the header.
pub struct ErrorParameterFormatLayer {
    value: Option<HeaderValue>,
}

impl ErrorParameterFormatLayer {
    pub fn new(builder: &Builder<builder::Complete>) -> ErrorParameterFormatLayer {
        ErrorParameterFormatLayer {
            value: builder.get_error_parameter_format().header_value(),
        }
    }
}

impl<S> Layer<S> for ErrorParameterFormatLayer {
    type Service = ErrorParameterFormatService<S>;

    fn layer(self, inner: S) -> Self::Service {
        ErrorParameterFormatService {
            inner,
            value: self.value,
        }
    }
}

pub struct ErrorParameterFormatService<S> {
    inner: S,
    value: Option<HeaderValue>,
}

impl<S, B> Service<Request<B>> for ErrorParameterFormatService<S>
where
    S: Service<Request<B>>,
{
    type Response = S::Response;
    type Error = S::Error;

    fn call(
        &self,
        mut req: Request<B>,
    ) -> impl Future<Output = Result<Self::Response, Self::Error>> {
        if let Some(value) = &self.value {
            if let Entry::Vacant(e) = req
                .headers_mut()
                .entry(ACCEPT_CONJURE_ERROR_PARAMETER_FORMAT)
            {
                e.insert(value.clone());
            }
        }

        self.inner.call(req)
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::{service, ErrorParameterFormat};

    #[tokio::test]
    async fn legacy() {
        let layer = ErrorParameterFormatLayer::new(&Builder::for_test());
        let service = layer.layer(service::service_fn(|req| async { Ok::<_, ()>(req) }));

        let out = service
            .call(Request::builder().body(()).unwrap())
            .await
            .unwrap();
        assert!(out.headers().is_empty());
    }

    #[tokio::test]
    async fn json() {
        let layer = ErrorParameterFormatLayer::new(
            &Builder::for_test().error_parameter_format(ErrorParameterFormat::Json),
        );
        let service = layer.layer(service::service_fn(|req| async { Ok::<_, ()>(req) }));

        let out = service
            .call(Request::builder().body(()).unwrap())
            .await
            .unwrap();
        assert_eq!(
            out.headers()
                .get_all(&ACCEPT_CONJURE_ERROR_PARAMETER_FORMAT)
                .iter()
                .collect::<Vec<_>>(),
            ["JSON"],
        );
    }

    #[tokio::test]
    async fn request_header_takes_precedence() {
        let layer = ErrorParameterFormatLayer::new(
            &Builder::for_test().error_parameter_format(ErrorParameterFormat::Json),
        );
        let service = layer.layer(service::service_fn(|req| async { Ok::<_, ()>(req) }));

        let request = Request::builder()
            .header(&ACCEPT_CONJURE_ERROR_PARAMETER_FORMAT, "OTHER")
            .body(())
            .unwrap();
        let out = service.call(request).await.unwrap();
        assert_eq!(
            out.headers()
                .get_all(&ACCEPT_CONJURE_ERROR_PARAMETER_FORMAT)
                .iter()
                .collect::<Vec<_>>(),
            ["OTHER"],
        );
    }
}
