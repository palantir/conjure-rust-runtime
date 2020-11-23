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
use std::collections::HashMap;
use std::sync::Arc;
use tokio::time::{Duration, Instant};
use witchcraft_metrics::{Metric, MetricId, MetricRegistry};

struct Capture {
    time: Duration,
    values: HashMap<MetricId, f64>,
}

pub struct SimulationMetricsRecorder {
    metrics: Arc<MetricRegistry>,
    start: Instant,
    prefilter: Box<dyn Fn(&MetricId) -> bool + Sync + Send>,
    captures: Vec<Capture>,
}

impl SimulationMetricsRecorder {
    pub fn new(metrics: &Arc<MetricRegistry>) -> Self {
        SimulationMetricsRecorder {
            metrics: metrics.clone(),
            start: Instant::now(),
            prefilter: Box::new(|_| true),
            captures: vec![],
        }
    }

    pub fn filter_metrics<F>(&mut self, filter: F)
    where
        F: Fn(&MetricId) -> bool + 'static + Sync + Send,
    {
        self.prefilter = Box::new(filter);
    }

    pub fn record(&mut self) {
        let mut capture = Capture {
            time: self.start.elapsed(),
            values: HashMap::new(),
        };

        for (id, metric) in &self.metrics.metrics() {
            if !(self.prefilter)(id) {
                continue;
            }

            let value = match metric {
                Metric::Counter(counter) => counter.count() as f64,
                Metric::Meter(meter) => meter.count() as f64,
                Metric::Gauge(gauge) => match gauge.value().deserialize_into::<f64>() {
                    Ok(value) => value,
                    Err(_) => continue,
                },
                _ => continue,
            };

            capture.values.insert(id.clone(), value);
        }

        self.captures.push(capture);
    }
}
