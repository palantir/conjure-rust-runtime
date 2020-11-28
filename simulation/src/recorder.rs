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
use std::collections::BTreeMap;
use std::io::{self, Write};
use std::sync::Arc;
use tokio::time::Instant;
use witchcraft_metrics::{Metric, MetricId, MetricRegistry};

pub struct SimulationMetricsRecorder {
    metrics: Arc<MetricRegistry>,
    start: Instant,
    prefilter: Box<dyn Fn(&MetricId) -> bool + Sync + Send>,
    captures: BTreeMap<MetricId, Vec<(f64, f64)>>,
}

impl SimulationMetricsRecorder {
    pub fn new(metrics: &Arc<MetricRegistry>) -> Self {
        SimulationMetricsRecorder {
            metrics: metrics.clone(),
            start: Instant::now(),
            prefilter: Box::new(|_| true),
            captures: BTreeMap::new(),
        }
    }

    pub fn filter_metrics<F>(&mut self, filter: F)
    where
        F: Fn(&MetricId) -> bool + 'static + Sync + Send,
    {
        self.prefilter = Box::new(filter);
    }

    pub fn record(&mut self) {
        let time = self.start.elapsed().as_secs_f64();

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

            self.captures
                .entry(id.clone())
                .or_insert_with(Vec::new)
                .push((time, value));
        }
    }

    pub fn finish(&self) -> MetricsRecord {
        MetricsRecord {
            captures: self.captures.clone(),
        }
    }
}

pub struct MetricsRecord {
    captures: BTreeMap<MetricId, Vec<(f64, f64)>>,
}

impl MetricsRecord {
    pub fn chart<F, W>(&self, filter: F, mut w: W) -> io::Result<()>
    where
        F: Fn(&MetricId) -> bool,
        W: Write,
    {
        let mut x_max = 0.;

        let plots = self
            .captures
            .iter()
            .filter(|(id, _)| filter(id))
            .map(|(id, points)| {
                x_max = f64::max(x_max, points[points.len() - 1].0);

                (id, points)
            })
            .collect::<Vec<_>>();
        assert!(!plots.is_empty());

        // downsample to roughly 1 point per pixel
        let period = x_max / 800.;
        let plots = plots
            .into_iter()
            .map(|(id, points)| (id, downsample(points, period)))
            .collect::<Vec<_>>();

        write!(
            w,
            "
set key inside left top Left reverse opaque box
set xlabel \"time_sec\" noenhanced
set xrange [0:*] noextend
set grid x y
plot",
        )?;

        for (i, (id, points)) in plots.iter().enumerate() {
            if i != 0 {
                write!(w, ",")?;
            }

            let tags = id
                .tags()
                .iter()
                .map(|t| format!("[{}] ", t.1))
                .collect::<String>();
            let title = format!("{}{}.count", tags, id.name());

            write!(
                w,
                r#" "-" binary endian=little record={} format="%float64" using 1:2 with lines title "{}" noenhanced"#,
                points.len(),
                title,
            )?;
        }

        writeln!(w)?;

        for (_, points) in &plots {
            for (x, y) in points {
                w.write_all(&x.to_le_bytes())?;
                w.write_all(&y.to_le_bytes())?;
            }
        }

        Ok(())
    }
}

fn downsample(raw: &[(f64, f64)], period: f64) -> Vec<(f64, f64)> {
    let mut out = vec![];

    // this does extra work by having to skip over empty buckets, but makes the implementation easier
    let mut points = raw.iter().copied().peekable();
    for i in 0.. {
        let start = i as f64 * period;
        let end = start + period;

        // FIXME this isn't ideal since sample density isn't uniform, but it shouldn't matter much in practice
        let mut sum = 0.;
        let mut count = 0;
        while let Some(&(x, y)) = points.peek() {
            if x >= end {
                break;
            }

            sum += y;
            count += 1;
            points.next();
        }

        if count > 0 {
            out.push((start + period / 2., sum / count as f64));
        }

        if points.peek().is_none() {
            break;
        }
    }

    out
}
