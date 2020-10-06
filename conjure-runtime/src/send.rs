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
use crate::errors::{ThrottledError, TimeoutError, UnavailableError};
use crate::service::boxed::BoxService;
use crate::service::http_error::PropagationConfig;
use crate::{
    Body, BodyError, Client, ClientState, HyperBody, Request, ResetTrackingBody, Response,
};
use conjure_error::{Error, ErrorKind};
use futures::future;
use hyper::header::{
    HeaderValue, CONNECTION, CONTENT_LENGTH, CONTENT_TYPE, HOST, PROXY_AUTHORIZATION,
};
use hyper::HeaderMap;
use rand::Rng;
use std::error::Error as _;
use std::pin::Pin;
use std::time::Duration;
use tokio::time;
use tower::layer::Layer;
use tower::Service;
use url::form_urlencoded;
use witchcraft_log::info;

pub(crate) async fn send(client: &Client, request: Request<'_>) -> Result<Response, Error> {
    let client_state = client.shared.state.load_full();

    let service = client_state.layer.layer(client_state.client.clone());

    let mut state = State {
        request,
        client,
        client_state: &client_state,
        service,
        attempt: 0,
    };

    let span = zipkin::next_span()
        .with_name(&format!(
            "conjure-runtime: {} {}",
            state.request.method, state.request.pattern
        ))
        .detach();

    time::timeout(client_state.request_timeout, span.bind(state.send()))
        .await
        .unwrap_or_else(|_| Err(Error::internal_safe(TimeoutError(()))))
}

struct State<'a, 'b> {
    request: Request<'b>,
    client: &'a Client,
    client_state: &'a ClientState,
    service: BoxService<http::Request<HyperBody>, Response, Error>,
    attempt: u32,
}

impl<'a, 'b> State<'a, 'b> {
    async fn send(&mut self) -> Result<Response, Error> {
        let mut body = self.request.body.take();

        loop {
            let span = zipkin::next_span()
                .with_name(&format!("conjure-runtime: attempt {}", self.attempt))
                .detach();
            let attempt = self.send_attempt(body.as_mut().map(|p| p.as_mut()));
            let (error, retry_after) = match span.bind(attempt).await? {
                AttemptOutcome::Ok(response) => return Ok(response),
                AttemptOutcome::Retry { error, retry_after } => (error, retry_after),
            };

            self.prepare_for_retry(body.as_mut().map(|p| p.as_mut()), error, retry_after)
                .await?;
        }
    }

    async fn send_attempt(
        &mut self,
        body: Option<Pin<&mut ResetTrackingBody<dyn Body + Sync + Send + 'b>>>,
    ) -> Result<AttemptOutcome, Error> {
        match self.send_raw(body).await {
            Ok(response) => Ok(AttemptOutcome::Ok(response)),
            Err(error) => {
                // we don't want to retry after propagated QoS errors
                match error.kind() {
                    ErrorKind::Service(_) => {}
                    _ => return Err(error),
                }

                if let Some(throttled) = error.cause().downcast_ref::<ThrottledError>() {
                    Ok(AttemptOutcome::Retry {
                        retry_after: throttled.retry_after,
                        error,
                    })
                } else if error.cause().is::<UnavailableError>()
                    || error.cause().is::<hyper::Error>()
                {
                    Ok(AttemptOutcome::Retry {
                        error,
                        retry_after: None,
                    })
                } else {
                    Err(error)
                }
            }
        }
    }

    async fn send_raw(
        &mut self,
        body: Option<Pin<&mut ResetTrackingBody<dyn Body + Sync + Send + 'b>>>,
    ) -> Result<Response, Error> {
        let headers = self.new_headers(&body);
        let (body, writer) = HyperBody::new(body);
        let request = self.new_request(headers, body);

        let (body_result, response_result) =
            future::join(writer.write(), self.service.call(request)).await;

        match (body_result, response_result) {
            (Ok(()), Ok(response)) => Ok(response),
            (Ok(()), Err(e)) => Err(e),
            (Err(e), Ok(response)) => {
                info!(
                    "body write reported an error on a successful request",
                    error: e
                );
                Ok(response)
            }
            (Err(body), Err(hyper)) => Err(self.deconflict_errors(body, hyper)),
        }
    }

    fn new_headers(
        &self,
        body: &Option<Pin<&mut ResetTrackingBody<dyn Body + Sync + Send + 'b>>>,
    ) -> HeaderMap {
        let mut headers = self.request.headers.clone();
        headers.remove(CONNECTION);
        headers.remove(HOST);
        headers.remove(PROXY_AUTHORIZATION);
        headers.remove(CONTENT_LENGTH);
        headers.remove(CONTENT_TYPE);

        if let Some(body) = body {
            if let Some(length) = body.content_length() {
                let value = HeaderValue::from_str(&length.to_string()).unwrap();
                headers.insert(CONTENT_LENGTH, value);
            }
            headers.insert(CONTENT_TYPE, body.content_type());
        }

        headers
    }

    fn new_request(&self, headers: HeaderMap, hyper_body: HyperBody) -> hyper::Request<HyperBody> {
        let url = self.build_url();

        let mut request = hyper::Request::new(hyper_body);
        *request.method_mut() = self.request.method.clone();
        *request.uri_mut() = url.parse().unwrap();
        *request.headers_mut() = headers;
        request.extensions_mut().insert(PropagationConfig {
            propagate_qos_errors: self.client.propagate_qos_errors(),
            propagate_service_errors: self.client.propagate_service_errors(),
        });
        request
    }

    fn build_url(&self) -> String {
        let mut params = self.request.params.clone();

        assert!(
            self.request.pattern.starts_with('/'),
            "pattern must start with `/`"
        );
        let mut uri = String::new();
        // make sure to skip the leading `/` to avoid an empty path segment
        for segment in self.request.pattern[1..].split('/') {
            match self.parse_param(segment) {
                Some(name) => match params.remove(name) {
                    Some(ref values) if values.len() != 1 => {
                        panic!("path segment parameter {} had multiple values", name);
                    }
                    Some(value) => {
                        uri.push('/');
                        uri.extend(form_urlencoded::byte_serialize(&value[0].as_bytes()));
                    }
                    None => panic!("path segment parameter {} had no values", name),
                },
                None => {
                    uri.push('/');
                    uri.push_str(segment);
                }
            }
        }

        for (i, (k, vs)) in params.iter().enumerate() {
            if i == 0 {
                uri.push('?');
            } else {
                uri.push('&');
            }

            for v in vs {
                uri.extend(form_urlencoded::byte_serialize(k.as_bytes()));
                uri.push('=');
                uri.extend(form_urlencoded::byte_serialize(v.as_bytes()));
            }
        }

        uri
    }

    fn parse_param<'c>(&self, segment: &'c str) -> Option<&'c str> {
        if segment.starts_with('{') && segment.ends_with('}') {
            Some(&segment[1..segment.len() - 1])
        } else {
            None
        }
    }

    // An error in the body write will cause an error on the hyper side, and vice versa.
    // To pick the right one, we see if the hyper error was due the body write aborting or not.
    fn deconflict_errors(&self, body_error: Error, hyper_error: Error) -> Error {
        if hyper_error
            .cause()
            .downcast_ref::<hyper::Error>()
            .and_then(hyper::Error::source)
            .map_or(false, |e| e.is::<BodyError>())
        {
            body_error
        } else {
            hyper_error
        }
    }

    async fn prepare_for_retry(
        &mut self,
        body: Option<Pin<&mut ResetTrackingBody<dyn Body + Sync + Send + 'b>>>,
        error: Error,
        retry_after: Option<Duration>,
    ) -> Result<(), Error> {
        self.attempt += 1;
        if self.attempt >= self.client_state.max_num_retries {
            info!("exceeded retry limits");
            return Err(error);
        }

        if !self.request.idempotent {
            info!("unable to retry non-idempotent request");
            return Err(error);
        }

        if let Some(body) = body {
            if body.needs_reset() && !body.reset().await {
                info!("unable to reset body when retrying request");
                return Err(error);
            }
        }

        let backoff = match retry_after {
            Some(backoff) => backoff,
            None => {
                let scale = 1 << self.attempt;
                let max = self.client_state.backoff_slot_size * scale;
                rand::thread_rng().gen_range(Duration::from_secs(0), max)
            }
        };

        let _span = zipkin::next_span()
            .with_name("conjure-runtime: backoff-with-jitter")
            .detach();

        time::delay_for(backoff).await;

        Ok(())
    }
}

enum AttemptOutcome {
    Ok(Response),
    Retry {
        error: Error,
        retry_after: Option<Duration>,
    },
}
