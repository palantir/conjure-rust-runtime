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
use crate::errors::{RemoteError, ThrottledError, UnavailableError};
use crate::raw::Service;
use crate::service::Layer;
use crate::{builder, Builder, ServerQos, ServiceError};
use bytes::{BufMut, BytesMut};
use conjure_error::{Error, ErrorType, Internal};
use conjure_serde::json;
use futures::StreamExt;
use http::header::RETRY_AFTER;
use http::{Request, Response, StatusCode};
use http_body::Body;
use http_body_util::BodyExt;
use std::error;
use std::pin::pin;
use std::time::Duration;
use witchcraft_log::info;

/// A layer which maps raw HTTP responses into Conjure `Error`s.
///
/// If `server_qos` is `ServerQos::Propagate429And503ToCaller`, 429 and 503 responses will be turned into Conjure
/// "throttle" and "service unavailable" errors respectively. Otherwise, they run into service errors. In both cases,
/// the error's cause will be the `ThrottledError` and `UnavailableError` types respectvely. If a `Retry-After` header
/// is present on a 429 response it will be included in the error.
///
/// If `service_error` is `ServiceError::PropagateToCaller`, Conjure errors returned by the server will be propagated,
/// with the new `Error` inheriting the incoming error's code, name, instance ID, and parameters. Otherwise it will be
/// treated as a generic internal error. In both cases, the cause will be a `RemoteError`.
pub struct HttpErrorLayer {
    server_qos: ServerQos,
    service_error: ServiceError,
}

impl HttpErrorLayer {
    pub fn new<T>(builder: &Builder<builder::Complete<T>>) -> HttpErrorLayer {
        HttpErrorLayer {
            server_qos: builder.get_server_qos(),
            service_error: builder.get_service_error(),
        }
    }
}

impl<S> Layer<S> for HttpErrorLayer {
    type Service = HttpErrorService<S>;

    fn layer(self, inner: S) -> Self::Service {
        HttpErrorService {
            inner,
            server_qos: self.server_qos,
            service_error: self.service_error,
        }
    }
}

pub struct HttpErrorService<S> {
    inner: S,
    server_qos: ServerQos,
    service_error: ServiceError,
}

impl<S, B1, B2> Service<Request<B1>> for HttpErrorService<S>
where
    S: Service<Request<B1>, Response = Response<B2>, Error = Error> + Sync + Send,
    B1: Sync + Send,
    B2: Body + Send,
    B2::Data: Send,
    B2::Error: Into<Box<dyn error::Error + Sync + Send>>,
{
    type Response = Response<B2>;
    type Error = Error;

    async fn call(&self, req: Request<B1>) -> Result<Self::Response, Self::Error> {
        let response = self.inner.call(req).await?;

        if response.status().is_success() {
            return Ok(response);
        }

        match response.status() {
            StatusCode::TOO_MANY_REQUESTS => {
                let retry_after = response
                    .headers()
                    .get(RETRY_AFTER)
                    .and_then(|h| h.to_str().ok())
                    .and_then(|s| s.parse().ok())
                    .map(Duration::from_secs);
                let error = ThrottledError { retry_after };

                let e = match self.server_qos {
                    ServerQos::AutomaticRetry => Error::internal_safe(error),
                    ServerQos::Propagate429And503ToCaller => match retry_after {
                        Some(retry_after) => Error::throttle_for_safe(error, retry_after),
                        None => Error::throttle_safe(error),
                    },
                };

                Err(e)
            }
            StatusCode::SERVICE_UNAVAILABLE => {
                let error = UnavailableError(());

                let e = match self.server_qos {
                    ServerQos::AutomaticRetry => Error::internal_safe(error),
                    ServerQos::Propagate429And503ToCaller => Error::unavailable_safe(error),
                };

                Err(e)
            }
            _ => {
                let (parts, body) = response.into_parts();
                let mut stream = pin!(body.into_data_stream());

                let mut body = BytesMut::new();
                while let Some(chunk) = stream.next().await {
                    match chunk {
                        Ok(chunk) => {
                            body.put(chunk);
                            if body.len() > 500 * 1024 {
                                break;
                            }
                        }
                        Err(e) => {
                            info!("error reading response body", error: Error::internal(e));
                            break;
                        }
                    }
                }

                let error = RemoteError {
                    status: parts.status,
                    error: json::client_from_slice(&body).ok(),
                };
                let log_body = error.error.is_none();

                let mut error = match (&error.error, self.service_error) {
                    (Some(e), ServiceError::PropagateToCaller) => {
                        let e = e.clone();
                        Error::propagated_service_safe(error, e)
                    }
                    (Some(e), ServiceError::WrapInNewError) => {
                        let instance_id = e.error_instance_id();
                        Error::service_safe(error, Internal::new().with_instance_id(instance_id))
                    }
                    (None, _) => Error::internal_safe(error),
                };

                if log_body {
                    error = error.with_unsafe_param("body", String::from_utf8_lossy(&body));
                }

                Err(error)
            }
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::service;
    use bytes::Bytes;
    use conjure_error::{ErrorCode, ErrorKind, SerializableError};
    use conjure_object::Uuid;
    use http::header::CONTENT_TYPE;
    use http_body_util::{Empty, Full};

    #[tokio::test]
    async fn success_is_ok() {
        let service =
            HttpErrorLayer::new(&Builder::for_test()).layer(service::service_fn(|_| async move {
                Ok(Response::builder()
                    .status(StatusCode::OK)
                    .body(Empty::<Bytes>::new())
                    .unwrap())
            }));

        let request = Request::new(());
        let out = service.call(request).await.unwrap();
        assert_eq!(out.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn default_throttle_handling() {
        let service =
            HttpErrorLayer::new(&Builder::for_test()).layer(service::service_fn(|_| async move {
                Ok(Response::builder()
                    .status(StatusCode::TOO_MANY_REQUESTS)
                    .header(RETRY_AFTER, "100")
                    .body(Empty::<Bytes>::new())
                    .unwrap())
            }));

        let request = Request::new(());
        let error = service.call(request).await.err().unwrap();
        match error.kind() {
            ErrorKind::Service(_) => {}
            _ => panic!("expected a service error"),
        }
        let cause = error.cause().downcast_ref::<ThrottledError>().unwrap();
        assert_eq!(cause.retry_after, Some(Duration::from_secs(100)));
    }

    #[tokio::test]
    async fn propagated_throttle_handling() {
        let service = HttpErrorLayer::new(
            &Builder::for_test().server_qos(ServerQos::Propagate429And503ToCaller),
        )
        .layer(service::service_fn(|_| async move {
            Ok(Response::builder()
                .status(StatusCode::TOO_MANY_REQUESTS)
                .header(RETRY_AFTER, "100")
                .body(Empty::<Bytes>::new())
                .unwrap())
        }));

        let request = Request::new(());
        let error = service.call(request).await.err().unwrap();
        let throttle = match error.kind() {
            ErrorKind::Throttle(throttle) => throttle,
            _ => panic!("expected a service error"),
        };
        assert_eq!(throttle.duration(), Some(Duration::from_secs(100)));
    }

    #[tokio::test]
    async fn default_unavailable_handling() {
        let service =
            HttpErrorLayer::new(&Builder::for_test()).layer(service::service_fn(|_| async move {
                Ok(Response::builder()
                    .status(StatusCode::SERVICE_UNAVAILABLE)
                    .body(Empty::<Bytes>::new())
                    .unwrap())
            }));

        let request = Request::new(());
        let error = service.call(request).await.err().unwrap();
        match error.kind() {
            ErrorKind::Service(_) => {}
            _ => panic!("expected a service error"),
        }
        error.cause().downcast_ref::<UnavailableError>().unwrap();
    }

    #[tokio::test]
    async fn propagated_unavailable_handling() {
        let service = HttpErrorLayer::new(
            &Builder::for_test().server_qos(ServerQos::Propagate429And503ToCaller),
        )
        .layer(service::service_fn(|_| async move {
            Ok(Response::builder()
                .status(StatusCode::SERVICE_UNAVAILABLE)
                .body(Empty::<Bytes>::new())
                .unwrap())
        }));

        let request = Request::new(());
        let error = service.call(request).await.err().unwrap();
        match error.kind() {
            ErrorKind::Unavailable(_) => {}
            _ => panic!("expected a service error"),
        }
    }

    #[tokio::test]
    async fn default_service_handling() {
        let service_error = SerializableError::builder()
            .error_code(ErrorCode::Conflict)
            .error_name("Default:Conflict")
            .error_instance_id(Uuid::nil())
            .build();

        let service = HttpErrorLayer::new(&Builder::for_test()).layer({
            let service_error = service_error.clone();
            service::service_fn(move |_| {
                let json = json::to_vec(&service_error).unwrap();
                async move {
                    Ok(Response::builder()
                        .status(StatusCode::CONFLICT)
                        .header(CONTENT_TYPE, "application/json")
                        .body(Full::new(Bytes::from(json)))
                        .unwrap())
                }
            })
        });

        let request = Request::new(());
        let error = service.call(request).await.err().unwrap();
        let service = match error.kind() {
            ErrorKind::Service(service) => service,
            _ => panic!("expected a service error"),
        };
        assert_eq!(*service.error_code(), ErrorCode::Internal);
        assert_eq!(
            service.error_instance_id(),
            service_error.error_instance_id()
        );

        let remote_error = error.cause().downcast_ref::<RemoteError>().unwrap();
        assert_eq!(remote_error.error(), Some(&service_error));
    }

    #[tokio::test]
    async fn propagated_service_handling() {
        let service_error = SerializableError::builder()
            .error_code(ErrorCode::Conflict)
            .error_name("Default:Conflict")
            .error_instance_id(Uuid::nil())
            .build();

        let service = HttpErrorLayer::new(
            &Builder::for_test().service_error(ServiceError::PropagateToCaller),
        )
        .layer({
            let service_error = service_error.clone();
            service::service_fn(move |_| {
                let json = json::to_vec(&service_error).unwrap();
                async move {
                    Ok(Response::builder()
                        .status(StatusCode::CONFLICT)
                        .header(CONTENT_TYPE, "application/json")
                        .body(Full::new(Bytes::from(json)))
                        .unwrap())
                }
            })
        });

        let request = Request::new(());
        let error = service.call(request).await.err().unwrap();
        let service = match error.kind() {
            ErrorKind::Service(service) => service,
            _ => panic!("expected a service error"),
        };
        assert_eq!(service_error, *service);

        let remote_error = error.cause().downcast_ref::<RemoteError>().unwrap();
        assert_eq!(remote_error.error(), Some(&service_error));
    }
}
