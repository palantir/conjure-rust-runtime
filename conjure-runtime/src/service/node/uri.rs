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
use crate::service::node::Node;
use crate::service::Layer;
use http::Request;
use std::future::Future;
use std::sync::Arc;

/// A layer which converts an origin-form URI to an absolute-form by joining with a node's base URI stored in the
/// request's extension map.
pub struct NodeUriLayer;

impl<S> Layer<S> for NodeUriLayer {
    type Service = NodeUriService<S>;

    fn layer(self, inner: S) -> NodeUriService<S> {
        NodeUriService { inner }
    }
}

pub struct NodeUriService<S> {
    inner: S,
}

impl<S, B> Service<Request<B>> for NodeUriService<S>
where
    S: Service<Request<B>>,
{
    type Error = S::Error;
    type Response = S::Response;

    fn call(
        &self,
        mut req: Request<B>,
    ) -> impl Future<Output = Result<Self::Response, Self::Error>> {
        // we expect the request's URI to be in origin-form
        debug_assert!(req.uri().scheme().is_none());
        debug_assert!(req.uri().authority().is_none());
        debug_assert!(req.uri().path().starts_with('/'));

        let node = req
            .extensions()
            .get::<Arc<Node>>()
            .expect("should have a Node extension");

        let mut base_uri = node.url.as_str();
        if base_uri.ends_with('/') {
            base_uri = &base_uri[..base_uri.len() - 1];
        }

        let uri = format!("{}{}", base_uri, req.uri());
        *req.uri_mut() = uri.as_str().parse().unwrap();

        self.inner.call(req)
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::service;

    #[tokio::test]
    async fn with_trailing_slash() {
        let service = NodeUriLayer.layer(service::service_fn(|req| async { Ok::<_, ()>(req) }));

        let req = Request::builder()
            .uri("/fizz/buzz?hello=true")
            .extension(Node::test("https://foobar.fizz:1234/"))
            .body(())
            .unwrap();
        let out = service.call(req).await.unwrap();

        assert_eq!(out.uri(), "https://foobar.fizz:1234/fizz/buzz?hello=true");
    }

    #[tokio::test]
    async fn without_trailing_slash() {
        let service = NodeUriLayer.layer(service::service_fn(|req| async { Ok::<_, ()>(req) }));

        let req = Request::builder()
            .uri("/fizz/buzz?hello=true")
            .extension(Node::test("https://foobar.fizz:1234/foo/bar"))
            .body(())
            .unwrap();
        let out = service.call(req).await.unwrap();

        assert_eq!(
            out.uri(),
            "https://foobar.fizz:1234/foo/bar/fizz/buzz?hello=true"
        );
    }
}
