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
use crate::harness::Harness;
use crate::simulation::{Simulation, SimulationBuilder1};
use tokio::time::Duration;

mod harness;
mod metrics;
mod recorder;
mod server;
mod simulation;

fn main() {
    Harness::new().simulation(simplest_possible_case);
}

fn simplest_possible_case(s: SimulationBuilder1) -> Simulation {
    s.server(|s| {
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
