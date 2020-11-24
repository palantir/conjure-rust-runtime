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
use crate::service::node::selector::balanced::reservoir::CoarseExponentialDecayReservoir;
use crate::service::node::Node;
use crate::service::Layer;
use futures::ready;
use http::{Request, Response};
use pin_project::{pin_project, pinned_drop};
use rand::seq::SliceRandom;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

mod reservoir;

const FAILURE_MEMORY: Duration = Duration::from_secs(30);
const FAILURE_WEIGHT: f64 = 10.;

struct TrackedNode {
    node: Arc<Node>,
    in_flight: AtomicUsize,
    recent_failures: CoarseExponentialDecayReservoir,
}

impl TrackedNode {
    fn new(node: Arc<Node>) -> Arc<TrackedNode> {
        Arc::new(TrackedNode {
            node,
            in_flight: AtomicUsize::new(0),
            recent_failures: CoarseExponentialDecayReservoir::new(FAILURE_MEMORY),
        })
    }

    fn snapshot<'a>(self: &'a Arc<Self>) -> TrackedNodeSnapshot<'a> {
        let requests_in_flight = self.in_flight.load(Ordering::SeqCst);
        let failure_reservoir = self.recent_failures.get();

        // float -> int casts are already saturating in Rust
        let score = requests_in_flight.saturating_add(failure_reservoir.round() as usize);

        TrackedNodeSnapshot { node: self, score }
    }
}

struct TrackedNodeSnapshot<'a> {
    node: &'a Arc<TrackedNode>,
    score: usize,
}

pub trait Entropy {
    fn shuffle<T>(&self, slice: &mut [T]);
}

pub struct RandEntropy;

impl Entropy for RandEntropy {
    fn shuffle<T>(&self, slice: &mut [T]) {
        slice.shuffle(&mut rand::thread_rng());
    }
}

struct State<T> {
    nodes: Vec<Arc<TrackedNode>>,
    entropy: T,
}

pub struct BalancedNodeSelectorLayer<T = RandEntropy> {
    state: Arc<State<T>>,
}

impl BalancedNodeSelectorLayer {
    pub fn new(nodes: Vec<Arc<Node>>) -> Self {
        Self::with_entropy(nodes, RandEntropy)
    }
}

impl<T> BalancedNodeSelectorLayer<T>
where
    T: Entropy,
{
    fn with_entropy(nodes: Vec<Arc<Node>>, entropy: T) -> Self {
        BalancedNodeSelectorLayer {
            state: Arc::new(State {
                nodes: nodes.into_iter().map(TrackedNode::new).collect(),
                entropy,
            }),
        }
    }
}

impl<T, S> Layer<S> for BalancedNodeSelectorLayer<T> {
    type Service = BalancedNodeSelectorService<S, T>;

    fn layer(self, inner: S) -> Self::Service {
        BalancedNodeSelectorService {
            state: self.state,
            inner,
        }
    }
}

pub struct BalancedNodeSelectorService<S, T = RandEntropy> {
    state: Arc<State<T>>,
    inner: S,
}

impl<S, T, B1, B2> Service<Request<B1>> for BalancedNodeSelectorService<S, T>
where
    T: Entropy,
    S: Service<Request<B1>, Response = Response<B2>>,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = BalancedNodeSelectorFuture<S::Future>;

    fn call(&self, mut req: Request<B1>) -> Self::Future {
        let mut nodes = self
            .state
            .nodes
            .iter()
            .map(|n| n.snapshot())
            .collect::<Vec<_>>();
        // shuffle so that we don't break ties the same way every request
        self.state.entropy.shuffle(&mut nodes);

        // since we don't yet support client-side QoS, we can just always pick the lowest scoring node
        let node = nodes.iter().min_by_key(|n| n.score).unwrap().node;

        req.extensions_mut().insert(node.node.clone());
        let future = self.inner.call(req);
        let node = node.clone();

        // do this last to make sure that we don't leak the in flight bump if the inner service call panics
        node.in_flight.fetch_add(1, Ordering::SeqCst);
        BalancedNodeSelectorFuture { future, node }
    }
}

#[pin_project(PinnedDrop)]
pub struct BalancedNodeSelectorFuture<F> {
    #[pin]
    future: F,
    node: Arc<TrackedNode>,
}

#[pinned_drop]
impl<F> PinnedDrop for BalancedNodeSelectorFuture<F> {
    fn drop(self: Pin<&mut Self>) {
        self.node.in_flight.fetch_sub(1, Ordering::SeqCst);
    }
}

impl<F, B, E> Future for BalancedNodeSelectorFuture<F>
where
    F: Future<Output = Result<Response<B>, E>>,
{
    type Output = F::Output;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();

        let result = ready!(this.future.poll(cx));

        match &result {
            // dialogue has a more complex set of conditionals, but this is what it ends up being in practice
            Ok(response) if response.status().is_server_error() => {
                this.node.recent_failures.update(FAILURE_WEIGHT)
            }
            Ok(response) if response.status().is_client_error() => {
                this.node.recent_failures.update(FAILURE_WEIGHT / 100.)
            }
            Ok(_) => {}
            Err(_) => this.node.recent_failures.update(FAILURE_WEIGHT),
        }

        Poll::Ready(result)
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::service;
    use futures::future;
    use http::StatusCode;
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::time;

    struct TestEntropy(AtomicUsize);

    impl TestEntropy {
        fn new() -> TestEntropy {
            TestEntropy(AtomicUsize::new(0))
        }
    }

    impl Entropy for TestEntropy {
        fn shuffle<T>(&self, slice: &mut [T]) {
            let i = self.0.fetch_add(1, Ordering::SeqCst) % slice.len();
            slice.rotate_left(i);
        }
    }

    #[tokio::test]
    async fn when_one_channel_is_in_use_prefer_the_other() {
        let service = BalancedNodeSelectorLayer::with_entropy(
            vec![
                Arc::new(Node::test("http://a/")),
                Arc::new(Node::test("http://b/")),
            ],
            TestEntropy::new(),
        )
        .layer(service::service_fn(|req: Request<()>| async move {
            match req.extensions().get::<Arc<Node>>().unwrap().url.as_str() {
                "http://a/" => future::pending().await,
                "http://b/" => Ok::<_, ()>(Response::new(())),
                _ => panic!(),
            }
        }));

        tokio::spawn(service.call(Request::new(())));

        for _ in 0..100 {
            service.call(Request::new(())).await.unwrap();
        }
    }

    #[tokio::test]
    async fn when_both_channels_are_free_we_get_roughly_fair_tiebreaking() {
        let service = BalancedNodeSelectorLayer::with_entropy(
            vec![
                Arc::new(Node::test("http://a/")),
                Arc::new(Node::test("http://b/")),
            ],
            TestEntropy::new(),
        )
        .layer(service::service_fn(|req: Request<()>| async move {
            Ok::<_, ()>(Response::new(
                req.extensions()
                    .get::<Arc<Node>>()
                    .unwrap()
                    .url
                    .as_str()
                    .to_string(),
            ))
        }));

        let mut nodes = HashMap::new();
        for _ in 0..200 {
            let response = service.call(Request::new(())).await.unwrap();
            *nodes.entry(response.into_body()).or_insert(0) += 1;
        }

        let mut expected = HashMap::new();
        expected.insert("http://a/".to_string(), 100);
        expected.insert("http://b/".to_string(), 100);

        assert_eq!(expected, nodes);
    }

    #[tokio::test]
    async fn a_single_4xx_doesnt_move_the_needle() {
        let service = BalancedNodeSelectorLayer::with_entropy(
            vec![
                Arc::new(Node::test("http://a/")),
                Arc::new(Node::test("http://b/")),
            ],
            TestEntropy::new(),
        )
        .layer(service::service_fn({
            let i = AtomicUsize::new(0);
            move |req: Request<()>| {
                let mut response = Response::new(
                    req.extensions()
                        .get::<Arc<Node>>()
                        .unwrap()
                        .url
                        .as_str()
                        .to_string(),
                );
                if i.fetch_add(1, Ordering::SeqCst) == 0 {
                    *response.status_mut() = StatusCode::BAD_REQUEST;
                }

                async move { Ok::<_, ()>(response) }
            }
        }));

        time::pause();
        let mut nodes = HashMap::new();
        for _ in 0..200 {
            let response = service.call(Request::new(())).await.unwrap();
            *nodes.entry(response.into_body()).or_insert(0) += 1;

            assert_eq!(
                service
                    .state
                    .nodes
                    .iter()
                    .map(|s| s.snapshot().score)
                    .collect::<Vec<_>>(),
                vec![0, 0]
            );

            time::advance(Duration::from_millis(50)).await;
        }

        let mut expected = HashMap::new();
        expected.insert("http://a/".to_string(), 100);
        expected.insert("http://b/".to_string(), 100);

        assert_eq!(expected, nodes);
    }

    #[tokio::test]
    async fn constant_4xxs_do_eventually_move_the_needle_but_we_go_back_to_fair_distribution() {
        let service = BalancedNodeSelectorLayer::with_entropy(
            vec![
                Arc::new(Node::test("http://a/")),
                Arc::new(Node::test("http://b/")),
            ],
            TestEntropy::new(),
        )
        .layer(service::service_fn(|req: Request<()>| async move {
            let status = match req.extensions().get::<Arc<Node>>().unwrap().url.as_str() {
                "http://a/" => StatusCode::BAD_REQUEST,
                "http://b/" => StatusCode::OK,
                _ => panic!(),
            };

            Ok::<_, ()>(Response::builder().status(status).body(()).unwrap())
        }));

        time::pause();
        for _ in 0..8 {
            service.call(Request::new(())).await.unwrap();
            assert_eq!(
                service
                    .state
                    .nodes
                    .iter()
                    .map(|s| s.snapshot().score)
                    .collect::<Vec<_>>(),
                vec![0, 0]
            );

            time::advance(Duration::from_millis(50)).await;
        }

        service.call(Request::new(())).await.unwrap();
        assert_eq!(
            service
                .state
                .nodes
                .iter()
                .map(|s| s.snapshot().score)
                .collect::<Vec<_>>(),
            vec![1, 0]
        );

        time::advance(Duration::from_secs(5)).await;

        assert_eq!(
            service
                .state
                .nodes
                .iter()
                .map(|s| s.snapshot().score)
                .collect::<Vec<_>>(),
            vec![0, 0]
        );
    }
}
