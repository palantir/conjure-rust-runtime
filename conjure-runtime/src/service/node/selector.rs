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
use crate::service::node::Node;
use conjure_error::Error;
use futures::future::{self, Either, Ready};
use futures::ready;
use http::{Request, Response, StatusCode};
use parking_lot::Mutex;
use pin_project::pin_project;
use rand::seq::SliceRandom;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use tokio::time::{Duration, Instant};
use tower::layer::Layer;
use tower::Service;
use witchcraft_log::debug;

pub trait Shuffle {
    fn shuffle<T>(&self, slice: &mut [T]);
}

pub struct RandShuffler;

impl Shuffle for RandShuffler {
    fn shuffle<T>(&self, slice: &mut [T]) {
        slice.shuffle(&mut rand::thread_rng());
    }
}

struct State {
    nodes: Vec<TrackedNode>,
    failed_url_cooldown: Duration,
}

struct TrackedNode {
    node: Arc<Node>,
    timeout: Mutex<Instant>,
}

/// A layer which selects a node to use for the request, injecting it into the request's extensions map.
pub struct NodeSelectorLayer<T> {
    state: Arc<State>,
    shuffler: T,
}

impl<T> NodeSelectorLayer<T>
where
    T: Shuffle,
{
    pub fn new(
        nodes: Vec<Node>,
        failed_url_cooldown: Duration,
        shuffler: T,
    ) -> NodeSelectorLayer<T> {
        let now = Instant::now();

        NodeSelectorLayer {
            state: Arc::new(State {
                nodes: nodes
                    .into_iter()
                    .map(|node| TrackedNode {
                        node: Arc::new(node),
                        timeout: Mutex::new(now),
                    })
                    .collect(),
                failed_url_cooldown,
            }),
            shuffler,
        }
    }
}

impl<T, S> Layer<S> for NodeSelectorLayer<T>
where
    T: Shuffle,
{
    type Service = NodeSelectorService<S>;

    fn layer(&self, inner: S) -> NodeSelectorService<S> {
        let mut shuffle = (0..self.state.nodes.len()).collect::<Vec<_>>();
        self.shuffler.shuffle(&mut shuffle);

        NodeSelectorService {
            inner,
            state: self.state.clone(),
            shuffle,
            idx: 0,
        }
    }
}

pub struct NodeSelectorService<S> {
    inner: S,
    state: Arc<State>,
    shuffle: Vec<usize>,
    idx: usize,
}

impl<S> NodeSelectorService<S> {
    fn select_node(&mut self) -> Option<usize> {
        let now = Instant::now();

        for _ in 0..self.shuffle.len() {
            let idx = self.next_node();

            let node = &self.state.nodes[idx];

            // NB: it's important to allow now == timeout here since we initialize all nodes with a now timeout
            if *node.timeout.lock() <= now {
                return Some(idx);
            } else {
                debug!("skipping node due to earlier failure", safe: { url: node.node.url });
            }
        }

        // If all of the nodes are timed out, just pick the next one. It's better to try again even on a failed node
        // than give up.
        if self.shuffle.is_empty() {
            None
        } else {
            let idx = self.next_node();
            debug!("falling back to a timed out node", safe: { url: self.state.nodes[idx].node.url });
            Some(idx)
        }
    }

    fn next_node(&mut self) -> usize {
        let idx = self.shuffle[self.idx];
        self.idx = (self.idx + 1) % self.shuffle.len();
        idx
    }
}

impl<S, B1, B2> Service<Request<B1>> for NodeSelectorService<S>
where
    S: Service<Request<B1>, Error = Error, Response = Response<B2>>,
{
    type Error = S::Error;
    type Response = S::Response;
    #[allow(clippy::type_complexity)]
    type Future = Either<NodeSelectorFuture<S::Future>, Ready<Result<S::Response, S::Error>>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, mut req: Request<B1>) -> Self::Future {
        let idx = match self.select_node() {
            Some(idx) => idx,
            None => {
                return Either::Right(future::ready(Err(Error::internal_safe(
                    "unable to select a node for request",
                ))))
            }
        };

        req.extensions_mut()
            .insert(self.state.nodes[idx].node.clone());

        Either::Left(NodeSelectorFuture {
            future: self.inner.call(req),
            state: self.state.clone(),
            idx,
        })
    }
}

#[pin_project]
pub struct NodeSelectorFuture<F> {
    #[pin]
    future: F,
    state: Arc<State>,
    idx: usize,
}

impl<F, B, E> Future for NodeSelectorFuture<F>
where
    F: Future<Output = Result<Response<B>, E>>,
{
    type Output = Result<Response<B>, E>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<Response<B>, E>> {
        let this = self.project();

        let result = ready!(this.future.poll(cx));

        let prev_failed = match &result {
            Ok(response) => response.status() == StatusCode::SERVICE_UNAVAILABLE,
            Err(_) => true,
        };

        if prev_failed {
            *this.state.nodes[*this.idx].timeout.lock() =
                Instant::now() + this.state.failed_url_cooldown;
        }

        Poll::Ready(result)
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use tower::ServiceExt;

    struct ReverseShuffler;

    impl Shuffle for ReverseShuffler {
        fn shuffle<T>(&self, slice: &mut [T]) {
            slice.reverse();
        }
    }

    #[tokio::test]
    async fn empty_nodes() {
        let service = NodeSelectorLayer::new(vec![], Duration::from_secs(0), ReverseShuffler)
            .layer(tower::service_fn(|_| async { Ok(Response::new(())) }));

        service.oneshot(Request::new(())).await.err().unwrap();
    }

    #[tokio::test]
    async fn rotate_between_calls() {
        let mut service = NodeSelectorLayer::new(
            vec![Node::test("http://a/"), Node::test("http://b/")],
            Duration::from_secs(0),
            ReverseShuffler,
        )
        .layer(tower::service_fn(|req: Request<()>| async move {
            Ok(Response::new(
                req.extensions().get::<Arc<Node>>().unwrap().clone(),
            ))
        }));

        let out = service.call(Request::new(())).await.unwrap();
        assert_eq!(out.body().url.as_str(), "http://b/");

        let out = service.call(Request::new(())).await.unwrap();
        assert_eq!(out.body().url.as_str(), "http://a/");

        let out = service.call(Request::new(())).await.unwrap();
        assert_eq!(out.body().url.as_str(), "http://b/");
    }

    #[tokio::test]
    async fn reuse_on_success() {
        tokio::time::pause();

        let layer = NodeSelectorLayer::new(
            vec![Node::test("http://a/"), Node::test("http://b/")],
            Duration::from_secs(10),
            ReverseShuffler,
        );

        let service = layer.layer(tower::service_fn(|req: Request<()>| async move {
            assert_eq!(
                req.extensions().get::<Arc<Node>>().unwrap().url.as_str(),
                "http://b/",
            );

            Ok(Response::new(()))
        }));

        service.oneshot(Request::new(())).await.unwrap();

        let service = layer.layer(tower::service_fn(|req: Request<()>| async move {
            assert_eq!(
                req.extensions().get::<Arc<Node>>().unwrap().url.as_str(),
                "http://b/",
            );

            Ok(Response::new(()))
        }));

        service.oneshot(Request::new(())).await.unwrap();
    }

    #[tokio::test]
    async fn skip_on_503() {
        tokio::time::pause();

        let layer = NodeSelectorLayer::new(
            vec![Node::test("http://a/"), Node::test("http://b/")],
            Duration::from_secs(10),
            ReverseShuffler,
        );

        let service = layer.layer(tower::service_fn(|req: Request<()>| async move {
            assert_eq!(
                req.extensions().get::<Arc<Node>>().unwrap().url.as_str(),
                "http://b/",
            );

            let response = Response::builder()
                .status(StatusCode::SERVICE_UNAVAILABLE)
                .body(())
                .unwrap();
            Ok(response)
        }));

        service.oneshot(Request::new(())).await.unwrap();

        let service = layer.layer(tower::service_fn(|req: Request<()>| async move {
            assert_eq!(
                req.extensions().get::<Arc<Node>>().unwrap().url.as_str(),
                "http://a/",
            );

            Ok(Response::new(()))
        }));

        service.oneshot(Request::new(())).await.unwrap();
    }

    #[tokio::test]
    async fn skip_on_error() {
        tokio::time::pause();

        let layer = NodeSelectorLayer::new(
            vec![Node::test("http://a/"), Node::test("http://b/")],
            Duration::from_secs(10),
            ReverseShuffler,
        );

        let service = layer.layer(tower::service_fn(|req: Request<()>| async move {
            assert_eq!(
                req.extensions().get::<Arc<Node>>().unwrap().url.as_str(),
                "http://b/",
            );

            Err::<Response<()>, _>(Error::internal_safe("blammo"))
        }));

        service.oneshot(Request::new(())).await.err().unwrap();

        let service = layer.layer(tower::service_fn(|req: Request<()>| async move {
            assert_eq!(
                req.extensions().get::<Arc<Node>>().unwrap().url.as_str(),
                "http://a/",
            );

            Ok(Response::new(()))
        }));

        service.oneshot(Request::new(())).await.unwrap();
    }

    #[tokio::test]
    async fn unblock_after_cooldown() {
        tokio::time::pause();

        let layer = NodeSelectorLayer::new(
            vec![Node::test("http://a/"), Node::test("http://b/")],
            Duration::from_secs(10),
            ReverseShuffler,
        );

        let service = layer.layer(tower::service_fn(|req: Request<()>| async move {
            assert_eq!(
                req.extensions().get::<Arc<Node>>().unwrap().url.as_str(),
                "http://b/",
            );

            Err::<Response<()>, _>(Error::internal_safe("blammo"))
        }));

        service.oneshot(Request::new(())).await.err().unwrap();

        tokio::time::advance(Duration::from_secs(10)).await;

        let service = layer.layer(tower::service_fn(|req: Request<()>| async move {
            assert_eq!(
                req.extensions().get::<Arc<Node>>().unwrap().url.as_str(),
                "http://b/",
            );

            Ok(Response::new(()))
        }));

        service.oneshot(Request::new(())).await.unwrap();
    }
}
