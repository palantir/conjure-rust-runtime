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
use crate::body::ResetTrackingBody;
use crate::raw::Service;
use crate::request::Request;
use crate::service::retry::RetryConfig;
use crate::service::Layer;
use crate::Body;
use http::Uri;
use percent_encoding::AsciiSet;
use std::collections::HashMap;
use std::pin::Pin;
use zipkin::Bind;

// https://url.spec.whatwg.org/#query-percent-encode-set
const QUERY: &AsciiSet = &percent_encoding::CONTROLS
    .add(b' ')
    .add(b'"')
    .add(b'<')
    .add(b'>');

// https://url.spec.whatwg.org/#path-percent-encode-set
const PATH: &AsciiSet = &QUERY.add(b'?').add(b'`').add(b'{').add(b'}');

// https://url.spec.whatwg.org/#userinfo-percent-encode-set
const USERINFO: &AsciiSet = &PATH
    .add(b'/')
    .add(b':')
    .add(b';')
    .add(b'=')
    .add(b'@')
    .add(b'[')
    .add(b'\\')
    .add(b']')
    .add(b'^')
    .add(b'|');

// https://url.spec.whatwg.org/#component-percent-encode-set
const COMPONENT: &AsciiSet = &USERINFO.add(b'$').add(b'%').add(b'&').add(b'+').add(b',');

#[derive(Clone)]
pub struct Pattern {
    pub pattern: String,
}

/// A layer which converts a Conjure `Request` to an `http::Request`.
pub struct RequestLayer;

impl<S> Layer<S> for RequestLayer {
    type Service = RequestService<S>;

    fn layer(self, inner: S) -> Self::Service {
        RequestService { inner }
    }
}

pub struct RequestService<S> {
    inner: S,
}

impl<'a, S> Service<Request<'a>> for RequestService<S>
where
    S: Service<http::Request<Option<Pin<Box<ResetTrackingBody<dyn Body + 'a + Sync + Send>>>>>>,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = Bind<S::Future>;

    fn call(&self, req: Request<'a>) -> Self::Future {
        let span = zipkin::next_span()
            .with_name(&format!("conjure-runtime: {} {}", req.method, req.pattern))
            .detach();

        let mut new_req = http::Request::new(req.body);
        *new_req.method_mut() = req.method;
        *new_req.uri_mut() = build_url(req.pattern, req.params);
        *new_req.headers_mut() = req.headers;
        new_req.extensions_mut().insert(Pattern {
            pattern: req.pattern.to_string(),
        });
        if let Some(idempotent) = req.idempotent {
            new_req.extensions_mut().insert(RetryConfig { idempotent });
        }

        span.bind(self.inner.call(new_req))
    }
}

fn build_url(pattern: &str, mut params: HashMap<String, Vec<String>>) -> Uri {
    assert!(pattern.starts_with('/'), "pattern must start with `/`");

    let mut uri = String::new();
    // make sure to skip the leading `/` to avoid an empty path segment
    for segment in pattern[1..].split('/') {
        match parse_param(segment) {
            Some(name) => match params.remove(name) {
                Some(values) if values.len() != 1 => {
                    panic!("path segment parameter {} had multiple values", name)
                }
                Some(value) => {
                    uri.push('/');
                    uri.extend(percent_encoding::utf8_percent_encode(&value[0], COMPONENT));
                }
                None => panic!("path segment parameter {} had no values", name),
            },
            None => {
                uri.push('/');
                uri.push_str(segment);
            }
        }
    }

    let mut first = true;
    for (k, vs) in params.iter() {
        for v in vs {
            if first {
                first = false;
                uri.push('?');
            } else {
                uri.push('&');
            }

            uri.extend(percent_encoding::utf8_percent_encode(k, COMPONENT));
            uri.push('=');
            uri.extend(percent_encoding::utf8_percent_encode(v, COMPONENT));
        }
    }

    uri.parse().unwrap()
}

fn parse_param(segment: &str) -> Option<&str> {
    if segment.starts_with('{') && segment.ends_with('}') {
        Some(&segment[1..segment.len() - 1])
    } else {
        None
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::service;
    use http::Method;

    #[tokio::test]
    async fn literal_pattern() {
        let service =
            RequestLayer.layer(service::service_fn(|req| async move { Ok::<_, ()>(req) }));

        let req = Request::new(Method::GET, "/foo/bar");
        let out = service.call(req).await.unwrap();

        assert_eq!(out.uri(), "/foo/bar");
    }

    #[tokio::test]
    async fn expanded_pattern() {
        let service =
            RequestLayer.layer(service::service_fn(|req| async move { Ok::<_, ()>(req) }));

        let mut req = Request::new(Method::GET, "/foo/{bar}");
        req.param("bar", "hello/ world");
        req.param("bazes", "one?");
        req.param("bazes", "two!");
        let out = service.call(req).await.unwrap();

        assert_eq!(out.uri(), "/foo/hello%2F%20world?bazes=one%3F&bazes=two!");
    }
}
