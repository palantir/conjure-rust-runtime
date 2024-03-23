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
use crate::rng::ConjureRng;
use crate::service::node::selector::balanced::reservoir::CoarseExponentialDecayReservoir;
use crate::service::node::{AcquiredNode, LimitedNode};
use crate::service::Layer;
use crate::{builder, Builder};
use conjure_error::Error;
use http::{Request, Response};
use rand::seq::SliceRandom;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::task::{Context, Poll};
use std::time::Duration;
use witchcraft_log::debug;

mod reservoir;

const INFLIGHT_COMPARISON_THRESHOLD: usize = 5;
const UNHEALTHY_SCORE_MULTIPLIER: usize = 2;
const FAILURE_MEMORY: Duration = Duration::from_secs(30);
const FAILURE_WEIGHT: f64 = 10.;

pub struct TrackedNode {
    node: LimitedNode,
    in_flight: AtomicUsize,
    recent_failures: CoarseExponentialDecayReservoir,
}

impl TrackedNode {
    fn new(node: LimitedNode) -> TrackedNode {
        TrackedNode {
            node,
            in_flight: AtomicUsize::new(0),
            recent_failures: CoarseExponentialDecayReservoir::new(FAILURE_MEMORY),
        }
    }

    fn acquire<'a, 'b, 'c, B>(
        &'a self,
        request: &'b Request<B>,
    ) -> AcquiringNode<'a, impl Future<Output = AcquiredNode> + 'c>
    where
        'a: 'c,
        'b: 'c,
    {
        AcquiringNode {
            node: self,
            acquire: Box::pin(self.node.acquire(request)),
        }
    }

    fn score(&self) -> Score {
        let in_flight = self.in_flight.load(Ordering::SeqCst);
        let recent_failures = self.recent_failures.get();

        // float -> int casts are already saturating in Rust
        let score = in_flight.saturating_add(recent_failures.round() as usize);

        Score { in_flight, score }
    }
}

pub struct AcquiringNode<'a, F> {
    node: &'a TrackedNode,
    // FIXME(#69) ideally we'd just pin the entire Vec<AcquiringNode>
    acquire: Pin<Box<F>>,
}

struct Score {
    in_flight: usize,
    score: usize,
}

struct Snapshot<T> {
    node: T,
    score: Score,
}

pub trait Entropy {
    fn shuffle<T>(&self, slice: &mut [T]);
}

pub struct RandEntropy(ConjureRng);

impl Entropy for RandEntropy {
    fn shuffle<T>(&self, slice: &mut [T]) {
        self.0.with(|rng| slice.shuffle(rng));
    }
}

struct State<T> {
    nodes: Vec<TrackedNode>,
    entropy: T,
}

pub struct BalancedNodeSelectorLayer<T = RandEntropy> {
    state: State<T>,
}

impl BalancedNodeSelectorLayer {
    pub fn new<T>(nodes: Vec<LimitedNode>, builder: &Builder<builder::Complete<T>>) -> Self {
        Self::with_entropy(nodes, RandEntropy(ConjureRng::new(builder)))
    }
}

impl<T> BalancedNodeSelectorLayer<T>
where
    T: Entropy,
{
    fn with_entropy(nodes: Vec<LimitedNode>, entropy: T) -> Self {
        BalancedNodeSelectorLayer {
            state: State {
                nodes: nodes.into_iter().map(TrackedNode::new).collect(),
                entropy,
            },
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
    state: State<T>,
    inner: S,
}

impl<S, T> BalancedNodeSelectorService<S, T>
where
    T: Entropy,
{
    async fn acquire<'a, B1>(&'a self, req: &Request<B1>) -> (&'a TrackedNode, AcquiredNode) {
        // Dialogue skips nodes that have significantly worse scores than previous ones on each
        // attempt, but to do that here we'd need a way to notify tasks on score changes. Rather
        // than adding the complexity of implementing that, we just perform the filtering once. This
        // filtering is intended to bypass nodes that are e.g. entirely offline, so it could be fine
        // to just do it once up front for a given request.
        let mut snapshots = self
            .state
            .nodes
            .iter()
            .map(|n| Snapshot {
                node: n,
                score: n.score(),
            })
            .collect::<Vec<_>>();
        snapshots.sort_by_key(|s| s.score.score);

        let mut nodes = vec![];
        let mut give_up_threshold = usize::MAX;
        for snapshot in snapshots {
            if snapshot.score.score > give_up_threshold {
                debug!(
                    "filtering out node with score above threshold",
                    safe: {
                        score: snapshot.score.score,
                        giveUpScore: give_up_threshold,
                        hostIndex: snapshot.node.node.node.idx,
                    }
                );

                continue;
            }

            if snapshot.score.in_flight > INFLIGHT_COMPARISON_THRESHOLD {
                give_up_threshold = snapshot
                    .score
                    .score
                    .saturating_mul(UNHEALTHY_SCORE_MULTIPLIER);
            }

            nodes.push(snapshot.node.acquire(req));
        }

        // shuffle so that we don't break ties the same way every request
        self.state.entropy.shuffle(&mut nodes);

        Acquire { nodes }.await
    }
}

struct Acquire<'a, F> {
    nodes: Vec<AcquiringNode<'a, F>>,
}

impl<'a, F> Future for Acquire<'a, F>
where
    F: Future<Output = AcquiredNode>,
{
    type Output = (&'a TrackedNode, AcquiredNode);

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        // even though we've filtered above in acquire, we still want to poll the nodes in order of
        // score to ensure we pick the best scoring node if multiple are available at the same time.
        let mut snapshots = self
            .nodes
            .iter_mut()
            .map(|node| Snapshot {
                score: node.node.score(),
                node,
            })
            .collect::<Vec<_>>();
        snapshots.sort_by_key(|n| n.score.score);

        for snapshot in snapshots {
            if let Poll::Ready(acquired) = snapshot.node.acquire.as_mut().poll(cx) {
                return Poll::Ready((snapshot.node.node, acquired));
            }
        }

        Poll::Pending
    }
}

impl<S, T, B1, B2> Service<Request<B1>> for BalancedNodeSelectorService<S, T>
where
    T: Entropy + Sync + Send,
    S: Service<Request<B1>, Response = Response<B2>, Error = Error> + Sync + Send,
    B1: Sync + Send,
{
    type Response = S::Response;
    type Error = S::Error;

    async fn call(&self, req: Request<B1>) -> Result<Self::Response, Self::Error> {
        let (node, tracked) = zipkin::next_span()
            .with_name("conjure-runtime: balanced-node-selection")
            .detach()
            .bind(self.acquire(&req))
            .await;

        node.in_flight.fetch_add(1, Ordering::SeqCst);
        let _guard = InFlightGuard { node };

        let result = tracked.wrap(&self.inner, req).await;

        match &result {
            Ok(response) if response.status().is_server_error() => {
                node.recent_failures.update(FAILURE_WEIGHT);
            }
            Ok(response) if response.status().is_client_error() => {
                node.recent_failures.update(FAILURE_WEIGHT / 100.)
            }
            Ok(_) => {}
            Err(_) => node.recent_failures.update(FAILURE_WEIGHT),
        }

        result
    }
}

struct InFlightGuard<'a> {
    node: &'a TrackedNode,
}

impl Drop for InFlightGuard<'_> {
    fn drop(&mut self) {
        self.node.in_flight.fetch_sub(1, Ordering::SeqCst);
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::service;
    use crate::service::node::Node;
    use conjure_http::client::Endpoint;
    use futures::channel::mpsc;
    use futures::future;
    use futures::{SinkExt, StreamExt};
    use http::StatusCode;
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use tokio::time;

    fn request() -> Request<()> {
        Request::builder()
            .extension(Endpoint::new("service", None, "endpoint", "/foo"))
            .body(())
            .unwrap()
    }

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
        let (tx, mut rx) = mpsc::channel(1);

        let service = BalancedNodeSelectorLayer::with_entropy(
            vec![
                LimitedNode::test("http://a/"),
                LimitedNode::test("http://b/"),
            ],
            TestEntropy::new(),
        )
        .layer(service::service_fn(move |req: Request<()>| {
            let mut tx = tx.clone();
            async move {
                match req.extensions().get::<Arc<Node>>().unwrap().url.as_str() {
                    "http://a/" => {
                        let _ = tx.send(()).await;
                        future::pending().await
                    }
                    "http://b/" => Ok::<_, Error>(Response::new(())),
                    _ => panic!(),
                }
            }
        }));
        let service = Arc::new(service);

        // the first request will be to a, so wait until we know the request has hit the service.
        tokio::spawn({
            let service = service.clone();
            async move { service.call(request()).await }
        });
        rx.next().await.unwrap();

        for _ in 0..100 {
            service.call(request()).await.unwrap();
        }
    }

    #[tokio::test]
    async fn when_both_channels_are_free_we_get_roughly_fair_tiebreaking() {
        let service = BalancedNodeSelectorLayer::with_entropy(
            vec![
                LimitedNode::test("http://a/"),
                LimitedNode::test("http://b/"),
            ],
            TestEntropy::new(),
        )
        .layer(service::service_fn(|req: Request<()>| async move {
            Ok::<_, Error>(Response::new(
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
            let response = service.call(request()).await.unwrap();
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
                LimitedNode::test("http://a/"),
                LimitedNode::test("http://b/"),
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

                async move { Ok::<_, Error>(response) }
            }
        }));

        time::pause();
        let mut nodes = HashMap::new();
        for _ in 0..200 {
            let response = service.call(request()).await.unwrap();
            *nodes.entry(response.into_body()).or_insert(0) += 1;

            assert_eq!(
                service
                    .state
                    .nodes
                    .iter()
                    .map(|n| n.score().score)
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
                LimitedNode::test("http://a/"),
                LimitedNode::test("http://b/"),
            ],
            TestEntropy::new(),
        )
        .layer(service::service_fn(|req: Request<()>| async move {
            let status = match req.extensions().get::<Arc<Node>>().unwrap().url.as_str() {
                "http://a/" => StatusCode::BAD_REQUEST,
                "http://b/" => StatusCode::OK,
                _ => panic!(),
            };

            Ok::<_, Error>(Response::builder().status(status).body(()).unwrap())
        }));

        time::pause();
        for _ in 0..8 {
            service.call(request()).await.unwrap();
            assert_eq!(
                service
                    .state
                    .nodes
                    .iter()
                    .map(|n| n.score().score)
                    .collect::<Vec<_>>(),
                vec![0, 0]
            );

            time::advance(Duration::from_millis(50)).await;
        }

        service.call(request()).await.unwrap();
        assert_eq!(
            service
                .state
                .nodes
                .iter()
                .map(|n| n.score().score)
                .collect::<Vec<_>>(),
            vec![1, 0]
        );

        time::advance(Duration::from_secs(5)).await;

        assert_eq!(
            service
                .state
                .nodes
                .iter()
                .map(|n| n.score().score)
                .collect::<Vec<_>>(),
            vec![0, 0]
        );
    }
}
