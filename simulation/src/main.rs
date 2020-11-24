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
use crate::simulation::Simulation;
use plotters::drawing::IntoDrawingArea;
use plotters::prelude::BitMapBackend;
use plotters::style::colors;
use tokio::time::Duration;

mod metrics;
mod recorder;
mod server;
mod simulation;

fn main() {
    let result = simplest_possible_case().run();

    let status_codes = result
        .status_codes
        .iter()
        .map(|(k, v)| format!("{}={}", k, v))
        .collect::<Vec<_>>()
        .join(", ");

    let summary = format!(
        "success={}%\tclient_mean={:?}\tserver_cpu={:?}\tclient_received={}/{}\tserver_resps={}\tcodes={{{}}}",
        result.success_percentage,
        result.client_mean,
        result.server_cpu,
        result.num_received,
        result.num_sent,
        result.num_global_responses,
        status_codes,
    );
    println!("{}", summary);

    let title = format!(
        "<strategy> success={}% client_mean={:?} server_cpu={:?}",
        result.success_percentage, result.client_mean, result.server_cpu
    );

    let root = BitMapBackend::new("results/test.png", (800, 1200)).into_drawing_area();
    root.fill(&colors::WHITE).unwrap();
    let root = root
        .margin(10, 10, 0, 0)
        .titled(&title, ("sans-serif", 20))
        .unwrap();

    let parts = root.split_evenly((2, 1));
    result
        .record
        .chart(&parts[0], |id| id.name().ends_with("activeRequests"));

    result
        .record
        .chart(&parts[1], |id| id.name().ends_with("request"));
}

fn simplest_possible_case() -> Simulation {
    Simulation::new()
        .server(|s| {
            s.name("fast")
                .handler(|h| h.response(200).delay(Duration::from_millis(600)))
        })
        .server(|s| {
            s.name("medium")
                .handler(|h| h.response(200).delay(Duration::from_millis(800)))
        })
        .server(|s| {
            s.name("slightly_slow")
                .handler(|h| h.response(200).delay(Duration::from_millis(1000)))
        })
        .requests_per_second(11)
        .send_until(Duration::from_secs(20 * 60))
        .abort_after(Duration::from_secs(60 * 60))
        .clients(10)
}
