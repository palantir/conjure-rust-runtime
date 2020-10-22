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
use arc_swap::ArcSwap;
use futures::ready;
use http::{Request, Response};
use pin_project::pin_project;
use rand::distributions::uniform::SampleUniform;
use rand::seq::SliceRandom;
use rand::Rng;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::task::{Context, Poll};
use tokio::time::{Duration, Instant};
use tower::layer::Layer;
use tower::Service;

// we reshuffle nodes every 10 minutes on average, with 30 seconds of jitter to either side
const RESHUFFLE_EVERY: Duration = Duration::from_secs(10 * 60 - 30);
const RESHUFFLE_JITTER: Duration = Duration::from_secs(60);

pub trait Entropy {
    fn gen_range<T>(&self, start: T, end: T) -> T
    where
        T: SampleUniform;

    fn shuffle<T>(&self, slice: &mut [T]);
}

pub struct RandEntropy;

impl Entropy for RandEntropy {
    fn gen_range<T>(&self, start: T, end: T) -> T
    where
        T: SampleUniform,
    {
        rand::thread_rng().gen_range(start, end)
    }

    fn shuffle<T>(&self, slice: &mut [T]) {
        slice.shuffle(&mut rand::thread_rng());
    }
}

pub trait Nodes {
    fn len(&self) -> usize;

    fn get(&self, idx: usize) -> Arc<Node>;
}

/// A nodes implementation which shuffles nodes when initializing, but not afterwards.
pub struct FixedNodes {
    nodes: Vec<Arc<Node>>,
}

impl FixedNodes {
    pub fn new(nodes: Vec<Arc<Node>>) -> Self {
        Self::with_entropy(nodes, RandEntropy)
    }

    fn with_entropy<E>(mut nodes: Vec<Arc<Node>>, entropy: E) -> Self
    where
        E: Entropy,
    {
        entropy.shuffle(&mut nodes);

        FixedNodes { nodes }
    }
}

impl Nodes for FixedNodes {
    fn len(&self) -> usize {
        self.nodes.len()
    }

    fn get(&self, idx: usize) -> Arc<Node> {
        self.nodes[idx].clone()
    }
}

/// A nodes implementation which periodically reshuffles nodes.
pub struct ReshufflingNodes<E = RandEntropy> {
    nodes: ArcSwap<Vec<Arc<Node>>>,
    start: Instant,
    interval_with_jitter: Duration,
    next_reshuffle_nanos: AtomicU64,
    entropy: E,
}

impl ReshufflingNodes {
    pub fn new(nodes: Vec<Arc<Node>>) -> Self {
        Self::with_entropy(nodes, RandEntropy)
    }
}

impl<E> ReshufflingNodes<E>
where
    E: Entropy,
{
    fn with_entropy(mut nodes: Vec<Arc<Node>>, entropy: E) -> Self {
        entropy.shuffle(&mut nodes);

        let interval_with_jitter =
            RESHUFFLE_EVERY + entropy.gen_range(Duration::from_secs(0), RESHUFFLE_JITTER);

        ReshufflingNodes {
            nodes: ArcSwap::new(Arc::new(nodes)),
            start: Instant::now(),
            interval_with_jitter,
            next_reshuffle_nanos: AtomicU64::new(interval_with_jitter.as_nanos() as u64),
            entropy,
        }
    }

    fn reshuffle_if_necessary(&self) {
        let next_reshuffle_nanos = self.next_reshuffle_nanos.load(Ordering::SeqCst);
        let now = Instant::now();

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

        let mut new_nodes = self.nodes.load().to_vec();
        self.entropy.shuffle(&mut new_nodes);
        self.nodes.store(Arc::new(new_nodes));
    }
}

impl<E> Nodes for ReshufflingNodes<E>
where
    E: Entropy,
{
    fn len(&self) -> usize {
        self.nodes.load().len()
    }

    fn get(&self, idx: usize) -> Arc<Node> {
        self.reshuffle_if_necessary();
        self.nodes.load()[idx].clone()
    }
}

struct State<T> {
    current_pin: AtomicUsize,
    nodes: T,
}

/// A node selector layer which pins to a host until a request either fails with a 5xx error or IO error, after which
/// it rotates to the next.
pub struct PinUntilErrorNodeSelectorLayer<T> {
    state: Arc<State<T>>,
}

impl<T> PinUntilErrorNodeSelectorLayer<T>
where
    T: Nodes,
{
    pub fn new(nodes: T) -> PinUntilErrorNodeSelectorLayer<T> {
        PinUntilErrorNodeSelectorLayer {
            state: Arc::new(State {
                current_pin: AtomicUsize::new(0),
                nodes,
            }),
        }
    }
}

impl<T, S> Layer<S> for PinUntilErrorNodeSelectorLayer<T> {
    type Service = PinUntilErrorNodeSelectorService<T, S>;

    fn layer(&self, inner: S) -> Self::Service {
        PinUntilErrorNodeSelectorService {
            state: self.state.clone(),
            inner,
        }
    }
}

pub struct PinUntilErrorNodeSelectorService<T, S> {
    state: Arc<State<T>>,
    inner: S,
}

impl<T, S, B1, B2> Service<Request<B1>> for PinUntilErrorNodeSelectorService<T, S>
where
    T: Nodes,
    S: Service<Request<B1>, Response = Response<B2>>,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = PinUntilErrorNodeSelectorFuture<T, S::Future>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, mut req: Request<B1>) -> Self::Future {
        let pin = self.state.current_pin.load(Ordering::SeqCst);
        let node = self.state.nodes.get(pin);
        req.extensions_mut().insert(node);

        PinUntilErrorNodeSelectorFuture {
            future: self.inner.call(req),
            state: self.state.clone(),
            pin,
        }
    }
}

#[pin_project]
pub struct PinUntilErrorNodeSelectorFuture<T, F> {
    #[pin]
    future: F,
    state: Arc<State<T>>,
    pin: usize,
}

impl<T, F, B, E> Future for PinUntilErrorNodeSelectorFuture<T, F>
where
    T: Nodes,
    F: Future<Output = Result<Response<B>, E>>,
{
    type Output = Result<Response<B>, E>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();

        let result = ready!(this.future.poll(cx));

        let increment_host = match &result {
            Ok(response) => response.status().is_server_error(),
            Err(_) => true,
        };

        if increment_host {
            let new_pin = (*this.pin + 1) % this.state.nodes.len();
            let _ = this.state.current_pin.compare_exchange(
                *this.pin,
                new_pin,
                Ordering::SeqCst,
                Ordering::SeqCst,
            );
        }

        Poll::Ready(result)
    }
}

#[cfg(test)]
mod test {
    use super::*;
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
        let nodes = vec![
            Arc::new(Node::test("http://a/")),
            Arc::new(Node::test("http://b/")),
        ];

        let nodes = FixedNodes::with_entropy(nodes, TestEntropy);
        assert_eq!(nodes.len(), 2);
        assert_eq!(nodes.get(0).url.as_str(), "http://b/");
        assert_eq!(nodes.get(1).url.as_str(), "http://a/");
    }

    #[tokio::test]
    async fn reshuffling_nodes_shuffle_perodically() {
        time::pause();

        let nodes = vec![
            Arc::new(Node::test("http://a/")),
            Arc::new(Node::test("http://b/")),
        ];

        let nodes = ReshufflingNodes::with_entropy(nodes, TestEntropy);
        assert_eq!(nodes.len(), 2);
        assert_eq!(nodes.get(0).url.as_str(), "http://b/");
        assert_eq!(nodes.get(1).url.as_str(), "http://a/");

        time::advance(RESHUFFLE_EVERY).await;

        assert_eq!(nodes.len(), 2);
        assert_eq!(nodes.get(0).url.as_str(), "http://a/");
        assert_eq!(nodes.get(1).url.as_str(), "http://b/");
    }

    struct TestNodes {
        nodes: Vec<Arc<Node>>,
    }

    impl Nodes for TestNodes {
        fn len(&self) -> usize {
            self.nodes.len()
        }

        fn get(&self, idx: usize) -> Arc<Node> {
            self.nodes[idx].clone()
        }
    }

    #[tokio::test]
    async fn pin_on_success() {
        let mut service = PinUntilErrorNodeSelectorLayer::new(TestNodes {
            nodes: vec![
                Arc::new(Node::test("http://a/")),
                Arc::new(Node::test("http://a/")),
            ],
        })
        .layer(tower::service_fn(|req: Request<()>| async move {
            assert_eq!(
                req.extensions().get::<Arc<Node>>().unwrap().url.as_str(),
                "http://a/"
            );

            Ok::<_, ()>(Response::new(()))
        }));

        service.call(Request::new(())).await.unwrap();
        service.call(Request::new(())).await.unwrap();
    }

    #[tokio::test]
    async fn pin_on_4xx() {
        let mut service = PinUntilErrorNodeSelectorLayer::new(TestNodes {
            nodes: vec![
                Arc::new(Node::test("http://a/")),
                Arc::new(Node::test("http://b/")),
            ],
        })
        .layer(tower::service_fn(|req: Request<()>| async move {
            assert_eq!(
                req.extensions().get::<Arc<Node>>().unwrap().url.as_str(),
                "http://a/"
            );

            Ok::<_, ()>(
                Response::builder()
                    .status(StatusCode::BAD_REQUEST)
                    .body(())
                    .unwrap(),
            )
        }));

        service.call(Request::new(())).await.unwrap();
        service.call(Request::new(())).await.unwrap();
    }

    #[tokio::test]
    async fn rotate_on_io_error() {
        let mut attempt = 0;
        let mut service = PinUntilErrorNodeSelectorLayer::new(TestNodes {
            nodes: vec![
                Arc::new(Node::test("http://a/")),
                Arc::new(Node::test("http://b/")),
            ],
        })
        .layer(tower::service_fn(move |req: Request<()>| {
            attempt += 1;
            async move {
                match attempt {
                    1 => {
                        assert_eq!(
                            req.extensions().get::<Arc<Node>>().unwrap().url.as_str(),
                            "http://a/"
                        );
                        Err(())
                    }
                    2 => {
                        assert_eq!(
                            req.extensions().get::<Arc<Node>>().unwrap().url.as_str(),
                            "http://b/"
                        );
                        Ok(Response::new(()))
                    }
                    _ => unreachable!(),
                }
            }
        }));

        service.call(Request::new(())).await.err().unwrap();
        service.call(Request::new(())).await.unwrap();
    }

    #[tokio::test]
    async fn rotate_on_5xx() {
        let mut attempt = 0;
        let mut service = PinUntilErrorNodeSelectorLayer::new(TestNodes {
            nodes: vec![
                Arc::new(Node::test("http://a/")),
                Arc::new(Node::test("http://b/")),
            ],
        })
        .layer(tower::service_fn(move |req: Request<()>| {
            attempt += 1;
            async move {
                match attempt {
                    1 => {
                        assert_eq!(
                            req.extensions().get::<Arc<Node>>().unwrap().url.as_str(),
                            "http://a/"
                        );
                        Ok::<_, ()>(
                            Response::builder()
                                .status(StatusCode::INTERNAL_SERVER_ERROR)
                                .body(())
                                .unwrap(),
                        )
                    }
                    2 => {
                        assert_eq!(
                            req.extensions().get::<Arc<Node>>().unwrap().url.as_str(),
                            "http://b/"
                        );
                        Ok(Response::new(()))
                    }
                    _ => unreachable!(),
                }
            }
        }));

        service.call(Request::new(())).await.unwrap();
        service.call(Request::new(())).await.unwrap();
    }
}
