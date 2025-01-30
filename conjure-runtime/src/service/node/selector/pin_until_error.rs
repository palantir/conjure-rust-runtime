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
use crate::service::node::LimitedNode;
use crate::service::Layer;
use crate::{builder, Builder};
use arc_swap::ArcSwap;
use conjure_error::Error;
use http::{Request, Response};
use rand::distr::uniform::SampleUniform;
use rand::seq::SliceRandom;
use rand::Rng;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::time::{Duration, Instant};

// we reshuffle nodes every 10 minutes on average, with 30 seconds of jitter to either side
const RESHUFFLE_EVERY: Duration = Duration::from_secs(10 * 60 - 30);
const RESHUFFLE_JITTER: Duration = Duration::from_secs(60);

pub trait Entropy {
    fn gen_range<T>(&self, start: T, end: T) -> T
    where
        T: SampleUniform + PartialOrd;

    fn shuffle<T>(&self, slice: &mut [T]);
}

pub struct RandEntropy(ConjureRng);

impl Entropy for RandEntropy {
    fn gen_range<T>(&self, start: T, end: T) -> T
    where
        T: SampleUniform + PartialOrd,
    {
        self.0.with(|rng| rng.random_range(start..end))
    }

    fn shuffle<T>(&self, slice: &mut [T]) {
        self.0.with(|rng| slice.shuffle(rng));
    }
}

pub trait Nodes<T> {
    fn len(&self) -> usize;

    fn get(&self, idx: usize) -> &T;
}

/// A nodes implementation which shuffles nodes when initializing, but not afterwards.
pub struct FixedNodes<T = LimitedNode> {
    nodes: Vec<T>,
}

impl<T> FixedNodes<T> {
    pub fn new<U>(nodes: Vec<T>, builder: &Builder<builder::Complete<U>>) -> Self {
        Self::with_entropy(nodes, RandEntropy(ConjureRng::new(builder)))
    }

    fn with_entropy<E>(mut nodes: Vec<T>, entropy: E) -> Self
    where
        E: Entropy,
    {
        entropy.shuffle(&mut nodes);

        FixedNodes { nodes }
    }
}

impl<T> Nodes<T> for FixedNodes<T> {
    fn len(&self) -> usize {
        self.nodes.len()
    }

    fn get(&self, idx: usize) -> &T {
        &self.nodes[idx]
    }
}

/// A nodes implementation which periodically reshuffles nodes.
pub struct ReshufflingNodes<T = LimitedNode, E = RandEntropy> {
    nodes: Vec<T>,
    shuffle: ArcSwap<Vec<usize>>,
    start: Instant,
    interval_with_jitter: Duration,
    next_reshuffle_nanos: AtomicU64,
    entropy: E,
}

impl<T> ReshufflingNodes<T> {
    pub fn new<U>(nodes: Vec<T>, builder: &Builder<builder::Complete<U>>) -> Self {
        Self::with_entropy(nodes, RandEntropy(ConjureRng::new(builder)))
    }
}

impl<T, E> ReshufflingNodes<T, E>
where
    E: Entropy,
{
    fn with_entropy(nodes: Vec<T>, entropy: E) -> Self {
        let mut shuffle = (0..nodes.len()).collect::<Vec<_>>();
        entropy.shuffle(&mut shuffle);

        let interval_with_jitter =
            RESHUFFLE_EVERY + entropy.gen_range(Duration::from_secs(0), RESHUFFLE_JITTER);

        ReshufflingNodes {
            nodes,
            shuffle: ArcSwap::from_pointee(shuffle),
            start: Instant::now(),
            interval_with_jitter,
            next_reshuffle_nanos: AtomicU64::new(interval_with_jitter.as_nanos() as u64),
            entropy,
        }
    }

    fn reshuffle_if_necessary(&self) {
        let now = Instant::now();

        let next_reshuffle_nanos = self.next_reshuffle_nanos.load(Ordering::SeqCst);
        if now < self.start + Duration::from_nanos(next_reshuffle_nanos) {
            return;
        }

        let new_next_reshuffle_nanos =
            (now + self.interval_with_jitter - self.start).as_nanos() as u64;
        if self
            .next_reshuffle_nanos
            .compare_exchange(
                next_reshuffle_nanos,
                new_next_reshuffle_nanos,
                Ordering::SeqCst,
                Ordering::SeqCst,
            )
            .is_err()
        {
            return;
        }

        let mut new_shuffle = self.shuffle.load().to_vec();
        self.entropy.shuffle(&mut new_shuffle);
        self.shuffle.store(Arc::new(new_shuffle));
    }
}

impl<T, E> Nodes<T> for ReshufflingNodes<T, E>
where
    E: Entropy,
{
    fn len(&self) -> usize {
        self.nodes.len()
    }

    fn get(&self, idx: usize) -> &T {
        self.reshuffle_if_necessary();
        let shuffled_idx = self.shuffle.load()[idx];
        &self.nodes[shuffled_idx]
    }
}

/// A node selector layer which pins to a host until a request either fails with a 5xx error or IO error, after which
/// it rotates to the next.
pub struct PinUntilErrorNodeSelectorLayer<T> {
    nodes: T,
}

impl<T> PinUntilErrorNodeSelectorLayer<T>
where
    T: Nodes<LimitedNode>,
{
    pub fn new(nodes: T) -> PinUntilErrorNodeSelectorLayer<T> {
        PinUntilErrorNodeSelectorLayer { nodes }
    }
}

impl<T, S> Layer<S> for PinUntilErrorNodeSelectorLayer<T> {
    type Service = PinUntilErrorNodeSelectorService<T, S>;

    fn layer(self, inner: S) -> Self::Service {
        PinUntilErrorNodeSelectorService {
            nodes: self.nodes,
            current_pin: AtomicUsize::new(0),
            inner,
        }
    }
}

pub struct PinUntilErrorNodeSelectorService<T, S> {
    nodes: T,
    current_pin: AtomicUsize,
    inner: S,
}

impl<T, S, B1, B2> Service<Request<B1>> for PinUntilErrorNodeSelectorService<T, S>
where
    T: Nodes<LimitedNode> + Sync + Send,
    S: Service<Request<B1>, Response = Response<B2>, Error = Error> + Sync + Send,
    B1: Sync + Send,
{
    type Response = S::Response;
    type Error = S::Error;

    async fn call(&self, req: Request<B1>) -> Result<Self::Response, Self::Error> {
        let pin = self.current_pin.load(Ordering::SeqCst);
        let node = self.nodes.get(pin);

        let result = node.wrap(&self.inner, req).await;

        let increment_host = match &result {
            Ok(response) => response.status().is_server_error(),
            Err(_) => true,
        };

        if increment_host {
            let new_pin = (pin + 1) % self.nodes.len();
            let _ =
                self.current_pin
                    .compare_exchange(pin, new_pin, Ordering::SeqCst, Ordering::SeqCst);
        }

        result
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::service;
    use crate::service::node::Node;
    use conjure_http::client::Endpoint;
    use http::StatusCode;
    use tokio::time;

    struct TestEntropy;

    impl Entropy for TestEntropy {
        fn gen_range<T>(&self, start: T, _: T) -> T {
            start
        }

        fn shuffle<T>(&self, slice: &mut [T]) {
            slice.reverse()
        }
    }

    #[tokio::test]
    async fn fixed_nodes_shuffle_on_construction() {
        let nodes = vec![0, 1];

        let nodes = FixedNodes::with_entropy(nodes, TestEntropy);
        assert_eq!(nodes.len(), 2);
        assert_eq!(nodes.get(0), &1);
        assert_eq!(nodes.get(1), &0);
    }

    #[tokio::test]
    async fn reshuffling_nodes_shuffle_perodically() {
        time::pause();

        let nodes = vec![0, 1];

        let nodes = ReshufflingNodes::with_entropy(nodes, TestEntropy);
        assert_eq!(nodes.len(), 2);
        assert_eq!(nodes.get(0), &1);
        assert_eq!(nodes.get(1), &0);

        time::advance(RESHUFFLE_EVERY).await;

        assert_eq!(nodes.len(), 2);
        assert_eq!(nodes.get(0), &0);
        assert_eq!(nodes.get(1), &1);
    }

    struct TestNodes {
        nodes: Vec<LimitedNode>,
    }

    impl Nodes<LimitedNode> for TestNodes {
        fn len(&self) -> usize {
            self.nodes.len()
        }

        fn get(&self, idx: usize) -> &LimitedNode {
            &self.nodes[idx]
        }
    }

    fn request() -> Request<()> {
        Request::builder()
            .extension(Endpoint::new("service", None, "endpoint", "/foo"))
            .body(())
            .unwrap()
    }

    #[tokio::test]
    async fn pin_on_success() {
        let service = PinUntilErrorNodeSelectorLayer::new(TestNodes {
            nodes: vec![
                LimitedNode::test("http://a/"),
                LimitedNode::test("http://b/"),
            ],
        })
        .layer(service::service_fn(|req: Request<()>| async move {
            assert_eq!(
                req.extensions().get::<Arc<Node>>().unwrap().url.as_str(),
                "http://a/"
            );

            Ok::<_, Error>(Response::new(()))
        }));

        service.call(request()).await.unwrap();
        service.call(request()).await.unwrap();
    }

    #[tokio::test]
    async fn pin_on_4xx() {
        let service = PinUntilErrorNodeSelectorLayer::new(TestNodes {
            nodes: vec![
                LimitedNode::test("http://a/"),
                LimitedNode::test("http://b/"),
            ],
        })
        .layer(service::service_fn(|req: Request<()>| async move {
            assert_eq!(
                req.extensions().get::<Arc<Node>>().unwrap().url.as_str(),
                "http://a/"
            );

            Ok::<_, Error>(
                Response::builder()
                    .status(StatusCode::BAD_REQUEST)
                    .body(())
                    .unwrap(),
            )
        }));

        service.call(request()).await.unwrap();
        service.call(request()).await.unwrap();
    }

    #[tokio::test]
    async fn rotate_on_io_error() {
        let service = PinUntilErrorNodeSelectorLayer::new(TestNodes {
            nodes: vec![
                LimitedNode::test("http://a/"),
                LimitedNode::test("http://b/"),
            ],
        })
        .layer(service::service_fn({
            let attempt = AtomicUsize::new(0);
            move |req: Request<()>| {
                let attempt = attempt.fetch_add(1, Ordering::SeqCst);
                async move {
                    match attempt {
                        0 => {
                            assert_eq!(
                                req.extensions().get::<Arc<Node>>().unwrap().url.as_str(),
                                "http://a/"
                            );
                            Err(Error::internal_safe("uh oh"))
                        }
                        1 => {
                            assert_eq!(
                                req.extensions().get::<Arc<Node>>().unwrap().url.as_str(),
                                "http://b/"
                            );
                            Ok(Response::new(()))
                        }
                        _ => unreachable!(),
                    }
                }
            }
        }));

        service.call(request()).await.err().unwrap();
        service.call(request()).await.unwrap();
    }

    #[tokio::test]
    async fn rotate_on_5xx() {
        let service = PinUntilErrorNodeSelectorLayer::new(TestNodes {
            nodes: vec![
                LimitedNode::test("http://a/"),
                LimitedNode::test("http://b/"),
            ],
        })
        .layer(service::service_fn({
            let attempt = AtomicUsize::new(0);
            move |req: Request<()>| {
                let attempt = attempt.fetch_add(1, Ordering::SeqCst);
                async move {
                    match attempt {
                        0 => {
                            assert_eq!(
                                req.extensions().get::<Arc<Node>>().unwrap().url.as_str(),
                                "http://a/"
                            );
                            Ok::<_, Error>(
                                Response::builder()
                                    .status(StatusCode::INTERNAL_SERVER_ERROR)
                                    .body(())
                                    .unwrap(),
                            )
                        }
                        1 => {
                            assert_eq!(
                                req.extensions().get::<Arc<Node>>().unwrap().url.as_str(),
                                "http://b/"
                            );
                            Ok(Response::new(()))
                        }
                        _ => unreachable!(),
                    }
                }
            }
        }));

        service.call(request()).await.unwrap();
        service.call(request()).await.unwrap();
    }
}
