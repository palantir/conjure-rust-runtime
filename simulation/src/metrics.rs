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
use std::sync::Arc;
use witchcraft_metrics::{Counter, MetricId, MetricRegistry, Reservoir, Snapshot, Timer};

pub fn global_responses(registry: &MetricRegistry) -> Arc<Counter> {
    registry.counter("globalResponses")
}

pub fn global_server_time_nanos(registry: &MetricRegistry) -> Arc<Counter> {
    registry.counter("globalServerTime")
}

pub fn active_requests(registry: &MetricRegistry, server: &'static str) -> Arc<Counter> {
    registry.counter(MetricId::new("activeRequests").with_tag("server", server))
}

pub fn request_counter(
    registry: &MetricRegistry,
    server: &'static str,
    endpoint: &str,
) -> Arc<Counter> {
    registry.counter(
        MetricId::new("request")
            .with_tag("server", server)
            .with_tag("endpoint", endpoint.to_string()),
    )
}

pub fn responses_timer(registry: &MetricRegistry, service_name: &'static str) -> Arc<Timer> {
    registry.timer_with(
        MetricId::new("client.response").with_tag("service-name", service_name),
        || {
            Timer::new(FullyBufferedReservoir {
                values: Mutex::new(vec![]),
            })
        },
    )
}

pub struct FullyBufferedReservoir {
    values: Mutex<Vec<i64>>,
}

impl Reservoir for FullyBufferedReservoir {
    fn update(&self, value: i64) {
        self.values.lock().push(value);
    }

    fn snapshot(&self) -> Box<dyn Snapshot> {
        let mut values = self.values.lock().clone();
        values.sort();
        Box::new(FullyBufferedSnapshot { values })
    }
}

struct FullyBufferedSnapshot {
    values: Vec<i64>,
}

impl Snapshot for FullyBufferedSnapshot {
    fn value(&self, _: f64) -> f64 {
        unimplemented!()
    }

    fn max(&self) -> i64 {
        unimplemented!()
    }

    fn min(&self) -> i64 {
        unimplemented!()
    }

    fn mean(&self) -> f64 {
        if self.values.is_empty() {
            return 0.;
        }

        self.values.iter().copied().sum::<i64>() as f64 / self.values.len() as f64
    }

    fn stddev(&self) -> f64 {
        unimplemented!()
    }
}
