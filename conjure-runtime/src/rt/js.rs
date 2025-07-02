// Copyright 2025 Palantir Technologies, Inc.
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

pub mod time {
    use std::future::Future;
    use std::ops::{Add, Sub};
    use std::pin::Pin;
    use std::task::{ready, Context, Poll};
    use std::time::Duration;
    use tokio::sync::oneshot;
    use wasm_bindgen::prelude::{wasm_bindgen, Closure, JsValue};
    use web_sys::Performance;

    #[wasm_bindgen]
    extern "C" {
        #[wasm_bindgen(js_name = "performance", thread_local_v2)]
        static PERFORMANCE: Performance;
        #[wasm_bindgen(js_name = "setTimeout")]
        fn set_timeout(cb: &Closure<dyn FnMut()>, millis: f64) -> JsValue;
        #[wasm_bindgen(js_name = "clearTimeout")]
        fn clear_timeout(timeout_id: &JsValue);
    }

    #[derive(Copy, Clone, PartialEq, PartialOrd)]
    pub struct Instant(f64);

    impl Instant {
        pub fn now() -> Self {
            Instant(PERFORMANCE.with(|p| p.now()))
        }

        pub fn elapsed(&self) -> Duration {
            Instant::now() - *self
        }
    }

    impl Add<Duration> for Instant {
        type Output = Instant;

        fn add(self, rhs: Duration) -> Self::Output {
            Instant(self.0 + rhs.as_secs_f64() * 1000.)
        }
    }

    impl Sub for Instant {
        type Output = Duration;

        fn sub(self, rhs: Instant) -> Self::Output {
            Duration::from_secs_f64((self.0 - rhs.0) / 1000.)
        }
    }

    pub struct Sleep {
        _closure: Closure<dyn FnMut()>,
        timeout_id: JsValue,
        rx: oneshot::Receiver<()>,
    }

    impl Drop for Sleep {
        fn drop(&mut self) {
            clear_timeout(&self.timeout_id);
        }
    }

    impl Future for Sleep {
        type Output = ();

        fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
            ready!(Pin::new(&mut self.rx).poll(cx)).expect("timeout dropped");
            Poll::Ready(())
        }
    }

    pub fn sleep(duration: Duration) -> Sleep {
        let (tx, rx) = oneshot::channel();
        let closure = Closure::once(|| {
            let _ = tx.send(());
        });
        let millis = duration.as_secs_f64() * 1000.;
        let timeout_id = set_timeout(&closure, millis);

        Sleep {
            _closure: closure,
            timeout_id,
            rx,
        }
    }
}
