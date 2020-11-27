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
use plotters::chart::{ChartBuilder, SeriesLabelPosition};
use plotters::coord::Shift;
use plotters::drawing::DrawingArea;
use plotters::element::PathElement;
use plotters::prelude::DrawingBackend;
use plotters::series::LineSeries;
use plotters::style::colors;
use plotters::style::{Color, RGBColor};
use std::collections::BTreeMap;
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
    pub fn chart<B, F>(&self, root: &DrawingArea<B, Shift>, filter: F)
    where
        B: DrawingBackend,
        F: Fn(&MetricId) -> bool,
    {
        let mut x_max = 0.;
        let mut y_max = 0.;

        let plots = self
            .captures
            .iter()
            .filter(|(id, _)| filter(id))
            .map(|(id, points)| {
                x_max = f64::max(x_max, points[points.len() - 1].0);
                y_max = points.iter().map(|p| p.1).fold(y_max, f64::max);

                (id, points)
            })
            .collect::<Vec<_>>();
        assert!(!plots.is_empty());

        let mut chart = ChartBuilder::on(root)
            .margin(5)
            .margin_right(30)
            .x_label_area_size(30)
            .y_label_area_size(50)
            .build_cartesian_2d(0f64..x_max, 0f64..y_max)
            .unwrap();

        chart.configure_mesh().x_desc("time_sec").draw().unwrap();

        // Downsample to one point per 3 pixels x-axis. The rasterizer doesn't seem to do well with higher densities.
        let width = chart.backend_coord(&(x_max, 0.)).0 - chart.backend_coord(&(0., 0.)).0;
        let period = x_max / width as f64 * 3.;

        // https://colorbrewer2.org/#type=qualitative&scheme=Set1&n=6
        let colors = [
            &RGBColor(0xe4, 0x1a, 0x1c),
            &RGBColor(0x37, 0x7e, 0xb8),
            &RGBColor(0x4d, 0xaf, 0x4a),
            &RGBColor(0x98, 0x4e, 0xa3),
            &RGBColor(0xff, 0x7f, 0x00),
            &RGBColor(0xff, 0xff, 0x33),
        ];

        for ((id, points), color) in plots.into_iter().zip(colors.iter().copied().cycle()) {
            let tags = id
                .tags()
                .iter()
                .map(|t| format!("[{}] ", t.1))
                .collect::<String>();
            let name = format!("{}{}.count", tags, id.name());

            let points = downsample(points, period);

            chart
                .draw_series(LineSeries::new(points, color))
                .unwrap()
                .label(name)
                .legend(move |(x, y)| {
                    PathElement::new(vec![(x, y), (x + 20, y)], color.stroke_width(2))
                });
        }

        chart
            .configure_series_labels()
            .position(SeriesLabelPosition::UpperLeft)
            .background_style(&colors::WHITE)
            .border_style(&colors::BLACK)
            .draw()
            .unwrap();
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
