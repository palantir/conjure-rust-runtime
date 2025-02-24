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
use pin_project::{pin_project, pinned_drop};
use std::future::Future;
use std::marker::PhantomPinned;
use std::pin::Pin;
use std::ptr::NonNull;
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
pub struct DeficitSemaphore(Mutex<State>);

impl DeficitSemaphore {
    pub fn new(permits: usize) -> Arc<DeficitSemaphore> {
        Arc::new(DeficitSemaphore(Mutex::new(State {
            total_permits: permits,
            outstanding_permits: 0,
            head: None,
            tail: None,
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
            node: None,
            _p: PhantomPinned,
        }
    }
}

struct State {
    total_permits: usize,
    outstanding_permits: usize,
    head: Option<NonNull<Node>>,
    tail: Option<NonNull<Node>>,
}

impl State {
    fn has_permits(&self) -> bool {
        self.total_permits > self.outstanding_permits
    }

    fn maybe_wake(&mut self) {
        if self.has_permits() {
            if let Some(head) = &mut self.head {
                unsafe {
                    if let Some(waker) = head.as_mut().waker.take() {
                        waker.wake();
                    }
                }
            }
        }
    }
}

struct Node {
    waker: Option<Waker>,
    next: Option<NonNull<Node>>,
    prev: Option<NonNull<Node>>,
}

impl Node {
    // SAFETY: this node must be in `state`'s queue.
    unsafe fn dequeue(&mut self, state: &mut State) {
        let self_ptr = NonNull::from(&mut *self);

        match &mut self.prev {
            Some(prev) => {
                debug_assert_eq!(prev.as_mut().next, Some(self_ptr));
                prev.as_mut().next = self.next;
            }
            None => {
                debug_assert_eq!(state.head, Some(self_ptr));
                state.head = self.next;
            }
        }

        match &mut self.next {
            Some(next) => {
                debug_assert_eq!(next.as_mut().prev, Some(self_ptr));
                next.as_mut().prev = self.prev;
            }
            None => {
                debug_assert_eq!(state.tail, Some(self_ptr));
                state.tail = self.prev;
            }
        }

        self.prev = None;
        self.next = None;
    }

    // SAFETY: This node must not currently be in a queue and must remain alive at the same address until it is removed
    // from the queue.
    unsafe fn push_back(&mut self, state: &mut State) {
        debug_assert_eq!(self.next, None);
        debug_assert_eq!(self.prev, None);

        match &mut state.tail {
            Some(tail) => {
                debug_assert_eq!(tail.as_mut().next, None);
                tail.as_mut().next = Some(self.into());
                self.prev = state.tail;
            }
            None => state.head = Some(self.into()),
        }

        state.tail = Some(self.into());
    }
}

unsafe impl Sync for State {}
unsafe impl Send for State {}

#[pin_project(PinnedDrop)]
pub struct Acquire {
    semaphore: Arc<DeficitSemaphore>,
    node: Option<Node>,
    #[pin]
    _p: PhantomPinned,
}

unsafe impl Sync for Acquire {}
unsafe impl Send for Acquire {}

#[pinned_drop]
impl PinnedDrop for Acquire {
    fn drop(self: Pin<&mut Self>) {
        let this = self.project();

        if let Some(node) = this.node {
            let mut state = this.semaphore.0.lock();
            let is_head = node.prev.is_none();
            unsafe {
                node.dequeue(&mut state);
            }
            // if we were up next but dropped before taking our permit, wake the next waiter
            if is_head {
                state.maybe_wake();
            }
        }
    }
}

impl Future for Acquire {
    type Output = Permit;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();

        let mut state = this.semaphore.0.lock();

        // fast path where no one's queued
        if state.head.is_none() && state.has_permits() {
            state.outstanding_permits += 1;
            return Poll::Ready(Permit {
                semaphore: this.semaphore.clone(),
            });
        }

        let node = match this.node {
            Some(node) => node,
            None => {
                *this.node = Some(Node {
                    waker: None, // will be filled in below
                    next: None,
                    prev: None,
                });

                let node = this.node.as_mut().unwrap();
                unsafe {
                    node.push_back(&mut state);
                }

                node
            }
        };

        // only the waiter at the head of the queue can take a permit
        if node.prev.is_none() && state.has_permits() {
            unsafe {
                node.dequeue(&mut state);
            }
            *this.node = None;

            state.outstanding_permits += 1;
            // construct this up front just in case maybe_wait panics to avoid leaking the permit
            let permit = Permit {
                semaphore: this.semaphore.clone(),
            };

            state.maybe_wake();

            Poll::Ready(permit)
        } else {
            // if the future jumped executors (uncommon but possible), update the waker
            if !node.waker.as_ref().is_some_and(|w| w.will_wake(cx.waker())) {
                node.waker = Some(cx.waker().clone());
            }

            Poll::Pending
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
