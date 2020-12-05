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
use serde::Serialize;
use serde_value::Value;
use std::sync::{Arc, Weak};
use witchcraft_metrics::Gauge;

pub trait Reduce {
    type Input;
    type Value: Serialize;

    fn default(&self) -> Self::Value;

    fn map(&self, v: &Self::Input) -> Self::Value;

    fn reduce(&self, a: &mut Self::Value, b: Self::Value);
}

/// A gauge which stores weak references to a collection of values and applies a reduction function to them to produce
/// a value.
///
/// References to dropped values are cleaned up during calls to [`Gauge::value`].
pub struct WeakReducingGauge<R>
where
    R: Reduce,
{
    entries: Mutex<Vec<Weak<R::Input>>>,
    reducer: R,
}

impl<R> WeakReducingGauge<R>
where
    R: Reduce + 'static + Sync + Send,
    R::Input: 'static + Sync + Send,
{
    pub fn new(reducer: R) -> Self {
        WeakReducingGauge {
            entries: Mutex::new(vec![]),
            reducer,
        }
    }

    pub fn push(&self, value: &Arc<R::Input>) {
        self.entries.lock().push(Arc::downgrade(value));
    }
}

impl<R> Gauge for WeakReducingGauge<R>
where
    R: Reduce + 'static + Sync + Send,
    R::Input: 'static + Sync + Send,
{
    fn value(&self) -> Value {
        let mut value = None::<R::Value>;
        self.entries.lock().retain(|v| match v.upgrade() {
            Some(v) => {
                let new_value = self.reducer.map(&v);

                match &mut value {
                    Some(value) => self.reducer.reduce(value, new_value),
                    None => value = Some(new_value),
                }
                true
            }
            None => false,
        });

        serde_value::to_value(value.unwrap_or_else(|| self.reducer.default())).unwrap()
    }
}
