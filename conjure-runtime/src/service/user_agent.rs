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
use crate::{builder, Agent, Builder, UserAgent};
use conjure_http::client::Endpoint;
use http::header::USER_AGENT;
use http::{HeaderValue, Request};
use std::convert::TryFrom;
use std::future::Future;

/// A layer which injects a `User-Agent` header into requests.
///
/// It extends the configured user agent with agents identifying the service defining the endpoint and conjure-runtime.
pub struct UserAgentLayer {
    user_agent: UserAgent,
}

impl UserAgentLayer {
    pub fn new<B>(builder: &Builder<builder::Complete<B>>) -> UserAgentLayer {
        UserAgentLayer {
            user_agent: builder.get_user_agent().clone(),
        }
    }
}

impl<S> Layer<S> for UserAgentLayer {
    type Service = UserAgentService<S>;

    fn layer(self, inner: S) -> Self::Service {
        UserAgentService {
            inner,
            user_agent: self.user_agent,
        }
    }
}

pub struct UserAgentService<S> {
    inner: S,
    user_agent: UserAgent,
}

impl<S, B> Service<Request<B>> for UserAgentService<S>
where
    S: Service<Request<B>>,
{
    type Response = S::Response;
    type Error = S::Error;

    fn call(
        &self,
        mut req: Request<B>,
    ) -> impl Future<Output = Result<Self::Response, Self::Error>> {
        let endpoint = req
            .extensions()
            .get::<Endpoint>()
            .expect("Endpoint missing from request extensions");

        let mut user_agent = self.user_agent.clone();
        user_agent.push_agent(Agent::new(
            endpoint.service(),
            endpoint.version().unwrap_or("0.0.0"),
        ));

        user_agent.push_agent(Agent::new(
            "conjure-rust-runtime",
            env!("CARGO_PKG_VERSION"),
        ));

        let user_agent = user_agent.to_string();

        req.headers_mut()
            .insert(USER_AGENT, HeaderValue::try_from(user_agent).unwrap());

        self.inner.call(req)
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::service;
    use crate::user_agent::Agent;
    use http::HeaderMap;

    #[tokio::test]
    async fn basic() {
        let layer = UserAgentLayer::new(
            &Builder::new()
                .service("foo")
                .user_agent(UserAgent::new(Agent::new("foobar", "1.0.0"))),
        );
        let service = layer.layer(service::service_fn(|req| async { Ok::<_, ()>(req) }));

        let request = Request::builder()
            .extension(Endpoint::new(
                "serviceName",
                Some("2.3.4"),
                "endpoint",
                "/path",
            ))
            .body(())
            .unwrap();
        let out = service.call(request).await.unwrap();

        let mut headers = HeaderMap::new();
        headers.insert(
            USER_AGENT,
            HeaderValue::from_static(concat!(
                "foobar/1.0.0 serviceName/2.3.4 conjure-rust-runtime/",
                env!("CARGO_PKG_VERSION")
            )),
        );
        assert_eq!(*out.headers(), headers);
    }
}
