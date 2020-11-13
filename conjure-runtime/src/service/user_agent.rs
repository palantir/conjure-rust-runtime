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
use crate::UserAgent;
use http::header::USER_AGENT;
use http::{HeaderValue, Request};
use std::convert::TryFrom;

/// A layer which injects a `User-Agent` header into requests.
pub struct UserAgentLayer {
    user_agent: HeaderValue,
}

impl UserAgentLayer {
    pub fn new(user_agent: &UserAgent) -> UserAgentLayer {
        let user_agent = user_agent.to_string();

        UserAgentLayer {
            user_agent: HeaderValue::try_from(user_agent).unwrap(),
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
    user_agent: HeaderValue,
}

impl<S, B> Service<Request<B>> for UserAgentService<S>
where
    S: Service<Request<B>>,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = S::Future;

    fn call(&self, mut req: Request<B>) -> Self::Future {
        req.headers_mut()
            .insert(USER_AGENT, self.user_agent.clone());

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
        let user_agent = UserAgent::new(Agent::new("foobar", "1.0.0"));
        let layer = UserAgentLayer::new(&user_agent);
        let service = layer.layer(service::service_fn(|req| async { Ok::<_, ()>(req) }));

        let out = service.call(Request::new(())).await.unwrap();

        let mut headers = HeaderMap::new();
        headers.insert(USER_AGENT, HeaderValue::from_static("foobar/1.0.0"));
        assert_eq!(*out.headers(), headers);
    }
}
