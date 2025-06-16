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
use parking_lot::Mutex;
use pin_list::{Node, NodeData, PinList};
use pin_project::{pin_project, pinned_drop};
use std::convert::Infallible;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll, Waker};

/// An async semaphore with the ability to arbitrarily adjust its total number of permits. It is absolutely fair - i.e.
/// tasks will be granted permits in exactly the order they began attempting to acquire them.
///
/// A traditional semaphore only tracks the current number of available permits, rather than the "total" number of
/// permits in the system, including those currently checked out by tasks. This semaphore instead tracks both that total
/// as well as the number of permits checked out. The total permit count can be adjusted arbitrarily, even if it would
/// cause the available permit count to go negative.
///
/// It's implemented with an intrusive linked list of waiters, taking advantage of the fact that futures are pinned in
/// memory.
///
/// To ensure that the semaphore is absolutely fair, the only thing that can remove a waiter's node from the queue is
/// the waiter itself. Otherwise, a newer waiter could see the queue as empty and take a permit before an older waiter
/// that was woken had a chance to take it.
pub struct DeficitSemaphore(Mutex<State>);

impl DeficitSemaphore {
    pub fn new(permits: usize) -> Arc<DeficitSemaphore> {
        Arc::new(DeficitSemaphore(Mutex::new(State {
            total_permits: permits,
            outstanding_permits: 0,
            waiters: PinList::new(pin_list::id::Checked::new()),
        })))
    }

    pub fn add_permits(&self, permits: usize) {
        let mut state = self.0.lock();
        state.total_permits = state
            .total_permits
            .checked_add(permits)
            .expect("permit count overflow");
        state.maybe_wake();
    }

    pub fn remove_permits(&self, permits: usize) {
        let mut state = self.0.lock();
        state.total_permits = state
            .total_permits
            .checked_sub(permits)
            .expect("permit count underflow");
    }

    pub fn acquire(self: Arc<Self>) -> Acquire {
        Acquire {
            semaphore: self,
            node: Node::new(),
        }
    }
}

type PinListTypes = dyn pin_list::Types<
    Id = pin_list::id::Checked,
    Protected = Option<Waker>,
    // We never enter the removed state (Infallible is an empty enum)
    Removed = Infallible,
    Unprotected = (),
>;

struct State {
    total_permits: usize,
    outstanding_permits: usize,
    waiters: PinList<PinListTypes>,
}

impl State {
    fn has_permits(&self) -> bool {
        self.total_permits > self.outstanding_permits
    }

    fn maybe_wake(&mut self) {
        if self.has_permits() {
            if let Some(waker) = self
                .waiters
                .cursor_front_mut()
                .protected_mut()
                .and_then(|o| o.take())
            {
                waker.wake();
            }
        }
    }
}

#[pin_project(PinnedDrop)]
pub struct Acquire {
    semaphore: Arc<DeficitSemaphore>,
    #[pin]
    node: Node<PinListTypes>,
}

#[pinned_drop]
impl PinnedDrop for Acquire {
    fn drop(self: Pin<&mut Self>) {
        let this = self.project();

        let Some(node) = this.node.initialized_mut() else {
            return;
        };

        let mut state = this.semaphore.0.lock();

        // if we were up next but dropped before taking our permit, wake the next waiter
        if let (NodeData::Linked(None), ()) = node.reset(&mut state.waiters) {
            state.maybe_wake();
        }
    }
}

impl Future for Acquire {
    type Output = Permit;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut this = self.project();

        let mut state = this.semaphore.0.lock();

        // fast path where no one's queued
        if state.waiters.is_empty() && state.has_permits() {
            state.outstanding_permits += 1;
            return Poll::Ready(Permit {
                semaphore: this.semaphore.clone(),
            });
        }

        match this.node.as_mut().initialized_mut() {
            Some(node) => {
                match node.protected_mut(&mut state.waiters) {
                    // we're still waiting
                    Some(Some(waker)) => {
                        // if the future jumped executors (uncommon but possible), update the waker
                        if !waker.will_wake(cx.waker()) {
                            *waker = cx.waker().clone();
                        }
                        Poll::Pending
                    }
                    // we've been woken, so it's our turn to take a permit
                    Some(None) => {
                        node.unlink(&mut state.waiters)
                            .expect("node is in the list");
                        state.outstanding_permits += 1;
                        // construct this up front just in case maybe_wait panics to avoid leaking the permit
                        let permit = Permit {
                            semaphore: this.semaphore.clone(),
                        };

                        state.maybe_wake();

                        Poll::Ready(permit)
                    }
                    // We haven't queued yet
                    None => panic!("future polled after completion"),
                }
            }
            None => {
                state
                    .waiters
                    .push_back(this.node, Some(cx.waker().clone()), ());
                Poll::Pending
            }
        }
    }
}

pub struct Permit {
    semaphore: Arc<DeficitSemaphore>,
}

impl Drop for Permit {
    fn drop(&mut self) {
        let mut state = self.semaphore.0.lock();
        state.outstanding_permits -= 1;
        state.maybe_wake();
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use futures::pin_mut;
    use futures_test::task;

    #[track_caller]
    fn assert_ready<T>(poll: Poll<T>) -> T {
        match poll {
            Poll::Ready(value) => value,
            Poll::Pending => panic!("expected ready"),
        }
    }

    #[track_caller]
    fn assert_pending<T>(poll: Poll<T>) {
        match poll {
            Poll::Pending => {}
            Poll::Ready(_) => panic!("expected pending"),
        }
    }

    #[test]
    fn uncontended_acquire() {
        let semaphore = DeficitSemaphore::new(2);

        let fut = semaphore.clone().acquire();
        pin_mut!(fut);
        let _permit = assert_ready(fut.poll(&mut task::panic_context()));

        let fut = semaphore.acquire();
        pin_mut!(fut);
        let _permit = assert_ready(fut.poll(&mut task::panic_context()));
    }

    #[test]
    fn single_queued_acquire() {
        let (waker, count) = task::new_count_waker();
        let mut count_cx = Context::from_waker(&waker);

        let semaphore = DeficitSemaphore::new(1);

        let fut = semaphore.clone().acquire();
        pin_mut!(fut);
        let permit = assert_ready(fut.poll(&mut task::panic_context()));

        let fut = semaphore.acquire();
        pin_mut!(fut);
        assert_pending(fut.as_mut().poll(&mut count_cx));

        assert_eq!(count, 0);
        drop(permit);
        assert_eq!(count, 1);

        assert_ready(fut.poll(&mut count_cx));
    }

    #[test]
    fn acquires_are_ordered() {
        let (waker1, count1) = task::new_count_waker();
        let mut count_cx1 = Context::from_waker(&waker1);
        let (waker2, count2) = task::new_count_waker();
        let mut count_cx2 = Context::from_waker(&waker2);

        let semaphore = DeficitSemaphore::new(1);

        let fut = semaphore.clone().acquire();
        pin_mut!(fut);
        let permit = assert_ready(fut.poll(&mut task::panic_context()));

        let fut1 = semaphore.clone().acquire();
        pin_mut!(fut1);
        assert_pending(fut1.as_mut().poll(&mut count_cx1));

        let fut2 = semaphore.acquire();
        pin_mut!(fut2);
        assert_pending(fut2.as_mut().poll(&mut count_cx2));

        assert_eq!(count1, 0);
        assert_eq!(count2, 0);
        drop(permit);
        assert_eq!(count1, 1);
        assert_eq!(count2, 0);

        assert_pending(fut2.as_mut().poll(&mut count_cx2));

        let permit = assert_ready(fut1.poll(&mut count_cx2));

        assert_pending(fut2.as_mut().poll(&mut count_cx2));

        assert_eq!(count1, 1);
        assert_eq!(count2, 0);
        drop(permit);
        assert_eq!(count1, 1);
        assert_eq!(count2, 1);

        assert_ready(fut2.poll(&mut count_cx2));
    }

    #[test]
    fn wakes_chain_on_acquire() {
        let (waker1, count1) = task::new_count_waker();
        let mut count_cx1 = Context::from_waker(&waker1);
        let (waker2, count2) = task::new_count_waker();
        let mut count_cx2 = Context::from_waker(&waker2);

        let semaphore = DeficitSemaphore::new(2);

        let fut = semaphore.clone().acquire();
        pin_mut!(fut);
        let permit1 = assert_ready(fut.poll(&mut task::panic_context()));

        let fut = semaphore.clone().acquire();
        pin_mut!(fut);
        let permit2 = assert_ready(fut.poll(&mut task::panic_context()));

        let fut1 = semaphore.clone().acquire();
        pin_mut!(fut1);
        assert_pending(fut1.as_mut().poll(&mut count_cx1));

        let fut2 = semaphore.acquire();
        pin_mut!(fut2);
        assert_pending(fut2.as_mut().poll(&mut count_cx2));

        assert_eq!(count1, 0);
        assert_eq!(count2, 0);
        drop(permit1);
        drop(permit2);
        assert_eq!(count1, 1);
        assert_eq!(count2, 0);

        let _permit = assert_ready(fut1.poll(&mut count_cx1));
        assert_eq!(count1, 1);
        assert_eq!(count2, 1);

        assert_ready(fut2.poll(&mut count_cx2));
    }

    #[test]
    fn early_head_drop() {
        let (waker1, count1) = task::new_count_waker();
        let mut count_cx1 = Context::from_waker(&waker1);
        let (waker2, count2) = task::new_count_waker();
        let mut count_cx2 = Context::from_waker(&waker2);

        let semaphore = DeficitSemaphore::new(1);

        let fut = semaphore.clone().acquire();
        pin_mut!(fut);
        let permit = assert_ready(fut.poll(&mut task::panic_context()));

        let mut fut1 = Box::pin(semaphore.clone().acquire());
        assert_pending(fut1.as_mut().poll(&mut count_cx1));

        let fut2 = semaphore.acquire();
        pin_mut!(fut2);
        assert_pending(fut2.as_mut().poll(&mut count_cx2));

        assert_eq!(count1, 0);
        assert_eq!(count2, 0);
        drop(permit);

        assert_eq!(count1, 1);
        assert_eq!(count2, 0);
        drop(fut1);

        assert_eq!(count1, 1);
        assert_eq!(count2, 1);

        assert_ready(fut2.poll(&mut count_cx2));
    }

    #[test]
    fn early_middle_drop() {
        let (waker1, count1) = task::new_count_waker();
        let mut count_cx1 = Context::from_waker(&waker1);
        let (waker2, count2) = task::new_count_waker();
        let mut count_cx2 = Context::from_waker(&waker2);
        let (waker3, count3) = task::new_count_waker();
        let mut count_cx3 = Context::from_waker(&waker3);

        let semaphore = DeficitSemaphore::new(1);

        let fut = semaphore.clone().acquire();
        pin_mut!(fut);
        let permit = assert_ready(fut.poll(&mut task::panic_context()));

        let fut1 = semaphore.clone().acquire();
        pin_mut!(fut1);
        assert_pending(fut1.as_mut().poll(&mut count_cx1));

        let mut fut2 = Box::pin(semaphore.clone().acquire());
        assert_pending(fut2.as_mut().poll(&mut count_cx2));

        let fut3 = semaphore.acquire();
        pin_mut!(fut3);
        assert_pending(fut3.as_mut().poll(&mut count_cx3));

        drop(fut2);

        assert_eq!(count1, 0);
        assert_eq!(count2, 0);
        assert_eq!(count3, 0);
        drop(permit);

        assert_eq!(count1, 1);
        assert_eq!(count2, 0);
        assert_eq!(count3, 0);

        assert_ready(fut1.poll(&mut count_cx1));
        assert_eq!(count1, 1);
        assert_eq!(count2, 0);
        assert_eq!(count3, 1);

        assert_ready(fut3.poll(&mut count_cx3));
    }

    #[test]
    fn early_tail_drop() {
        let (waker1, count1) = task::new_count_waker();
        let mut count_cx1 = Context::from_waker(&waker1);
        let (waker2, count2) = task::new_count_waker();
        let mut count_cx2 = Context::from_waker(&waker2);

        let semaphore = DeficitSemaphore::new(1);

        let fut = semaphore.clone().acquire();
        pin_mut!(fut);
        let permit = assert_ready(fut.poll(&mut task::panic_context()));

        let fut1 = semaphore.clone().acquire();
        pin_mut!(fut1);
        assert_pending(fut1.as_mut().poll(&mut count_cx1));

        let mut fut2 = Box::pin(semaphore.acquire());
        assert_pending(fut2.as_mut().poll(&mut count_cx2));

        drop(fut2);

        assert_eq!(count1, 0);
        assert_eq!(count2, 0);
        drop(permit);

        assert_eq!(count1, 1);
        assert_eq!(count2, 0);
        assert_ready(fut1.poll(&mut count_cx1));

        assert_eq!(count1, 1);
        assert_eq!(count2, 0);
    }

    #[test]
    fn add_permits() {
        let (waker, count) = task::new_count_waker();
        let mut count_cx = Context::from_waker(&waker);

        let semaphore = DeficitSemaphore::new(0);

        let fut = semaphore.clone().acquire();
        pin_mut!(fut);
        assert_pending(fut.as_mut().poll(&mut count_cx));

        assert_eq!(count, 0);
        semaphore.add_permits(1);

        assert_eq!(count, 1);
        assert_ready(fut.poll(&mut count_cx));
    }

    #[test]
    fn remove_permits() {
        let semaphore = DeficitSemaphore::new(1);

        let fut = semaphore.clone().acquire();
        pin_mut!(fut);
        let permit = assert_ready(fut.poll(&mut task::panic_context()));

        let fut = semaphore.clone().acquire();
        pin_mut!(fut);
        assert_pending(fut.as_mut().poll(&mut task::panic_context()));

        semaphore.remove_permits(1);

        drop(permit);

        assert_pending(fut.poll(&mut task::panic_context()));
    }

    #[tokio::test]
    async fn stress_test() {
        let semaphore = DeficitSemaphore::new(10);

        let mut handles = vec![];
        for _ in 0..100 {
            let handle = tokio::spawn({
                let semaphore = semaphore.clone();
                async move {
                    for _ in 0..1000 {
                        let _permit = semaphore.clone().acquire().await;
                        tokio::task::yield_now().await;
                    }
                }
            });

            handles.push(handle);
        }

        for handle in handles {
            handle.await.unwrap();
        }
    }
}
