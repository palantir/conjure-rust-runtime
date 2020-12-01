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
#![cfg(test)]
use crate::harness::Harness;
use crate::server::Endpoint;
use crate::simulation::{Simulation, SimulationBuilder1, TokioClock};
use rand_pcg::Pcg64;
use std::sync::Arc;
use tokio::time::Duration;
use witchcraft_metrics::Meter;

mod harness;
mod metrics;
mod recorder;
mod server;
mod simulation;

// FIXME implement live_reloading and server_side_rate_limits
fn main() {
    Harness::new()
        .simulation(simplest_possible_case)
        .simulation(slowdown_and_error_thresholds)
        .simulation(slow_503s_then_revert)
        .simulation(fast_503s_then_revert)
        .simulation(fast_400s_then_revert)
        .simulation(short_outage_on_one_node)
        .simulation(drastic_slowdown)
        .simulation(all_nodes_500)
        .simulation(black_hole)
        .simulation(one_endpoint_dies_on_each_server)
        .simulation(uncommon_flakes)
        .simulation(one_big_spike)
        .simulation(server_side_rate_limits)
        .finish();
}

fn simplest_possible_case(s: SimulationBuilder1) -> Simulation {
    s.server(|s| {
        s.name("fast")
            .handler(|h| h.response(200).response_time(Duration::from_millis(600)))
    })
    .server(|s| {
        s.name("medium")
            .handler(|h| h.response(200).response_time(Duration::from_millis(800)))
    })
    .server(|s| {
        s.name("slightly_slow")
            .handler(|h| h.response(200).response_time(Duration::from_millis(1000)))
    })
    .requests_per_second(11.)
    .send_until(Duration::from_secs(20 * 60))
    .abort_after(Duration::from_secs(60 * 60))
    .clients(10)
}

fn slowdown_and_error_thresholds(s: SimulationBuilder1) -> Simulation {
    let error_threshold = 40;
    let slowdown_threshold = 30;
    s.server(|s| {
        s.name("fast").handler(|h| {
            h.respond_200_until_capacity(500, error_threshold)
                .linear_response_time(Duration::from_millis(600), slowdown_threshold)
        })
    })
    .server(|s| {
        s.name("medium").handler(|h| {
            h.respond_200_until_capacity(500, error_threshold)
                .linear_response_time(Duration::from_millis(800), slowdown_threshold)
        })
    })
    .server(|s| {
        s.name("slightly_slow").handler(|h| {
            h.respond_200_until_capacity(500, error_threshold)
                .linear_response_time(Duration::from_millis(1000), slowdown_threshold)
        })
    })
    .endpoints(vec![Endpoint::get("/")])
    .requests_per_second(500.)
    .send_until(Duration::from_secs(20))
    .abort_after(Duration::from_secs(10 * 60))
    .clients(10)
}

fn slow_503s_then_revert(s: SimulationBuilder1) -> Simulation {
    let capacity = 60;
    s.server(|s| {
        s.name("fast").handler(|h| {
            h.response(200)
                .linear_response_time(Duration::from_millis(60), capacity)
        })
    })
    .server(|s| {
        s.name("slow_failures_then_revert")
            .handler(|h| {
                h.response(200)
                    .linear_response_time(Duration::from_millis(60), capacity)
            })
            .until(Duration::from_secs(3), "slow 503s")
            .handler(|h| {
                h.response(503)
                    .linear_response_time(Duration::from_secs(1), capacity)
            })
            .until(Duration::from_secs(10), "revert")
            .handler(|h| {
                h.response(200)
                    .linear_response_time(Duration::from_millis(10), capacity)
            })
    })
    .requests_per_second(100.)
    .send_until(Duration::from_secs(15))
    .abort_after(Duration::from_secs(10 * 60))
    .clients(10)
}

fn fast_503s_then_revert(s: SimulationBuilder1) -> Simulation {
    s.server(|s| {
        s.name("normal")
            .handler(|h| h.response(200).response_time(Duration::from_millis(120)))
    })
    .server(|s| {
        s.name("fast_503s_then_revert")
            .handler(|h| h.response(200).response_time(Duration::from_millis(120)))
            .until(Duration::from_secs(3), "fast 503s")
            .handler(|h| h.response(503).response_time(Duration::from_millis(10)))
            .until(Duration::from_secs(60), "revert")
            .handler(|h| h.response(200).response_time(Duration::from_millis(120)))
    })
    .requests_per_second(500.)
    .send_until(Duration::from_secs(90))
    .abort_after(Duration::from_secs(10 * 60))
    .clients(10)
}

fn fast_400s_then_revert(s: SimulationBuilder1) -> Simulation {
    s.server(|s| {
        s.name("normal")
            .handler(|h| h.response(200).response_time(Duration::from_millis(120)))
    })
    .server(|s| {
        s.name("fast_400s_then_revert")
            .handler(|h| h.response(200).response_time(Duration::from_millis(120)))
            .until(Duration::from_secs(3), "fast 400s")
            .handler(|h| h.response(400).response_time(Duration::from_millis(20)))
            .until(Duration::from_secs(30), "revert")
            .handler(|h| h.response(200).response_time(Duration::from_millis(120)))
    })
    .requests_per_second(100.)
    .send_until(Duration::from_secs(60))
    .abort_after(Duration::from_secs(10 * 60))
    .clients(10)
}

fn short_outage_on_one_node(s: SimulationBuilder1) -> Simulation {
    s.server(|s| {
        s.name("stable")
            .handler(|h| h.response(200).response_time(Duration::from_secs(2)))
    })
    .server(|s| {
        s.name("has_short_outage")
            .handler(|h| h.response(200).response_time(Duration::from_secs(2)))
            .until(Duration::from_secs(30), "20s outage")
            .handler(|h| h.response(500).response_time(Duration::from_nanos(10)))
            .until(Duration::from_secs(50), "revert")
            .handler(|h| h.response(200).response_time(Duration::from_secs(2)))
    })
    .requests_per_second(20.)
    .send_until(Duration::from_secs(80))
    .abort_after(Duration::from_secs(3 * 60))
    .clients(10)
}

fn drastic_slowdown(s: SimulationBuilder1) -> Simulation {
    let capacity = 60;
    s.server(|s| {
        s.name("fast").handler(|h| {
            h.response(200)
                .linear_response_time(Duration::from_millis(60), capacity)
        })
    })
    .server(|s| {
        s.name("fast_then_slow_then_fast")
            .handler(|h| {
                h.response(200)
                    .linear_response_time(Duration::from_millis(60), capacity)
            })
            .until(Duration::from_secs(3), "slow 200s")
            .handler(|h| {
                h.response(200)
                    .linear_response_time(Duration::from_secs(10), capacity)
            })
            .until(Duration::from_secs(10), "revert")
            .handler(|h| {
                h.response(200)
                    .linear_response_time(Duration::from_millis(60), capacity)
            })
    })
    .requests_per_second(200.)
    .send_until(Duration::from_secs(20))
    .abort_after(Duration::from_secs(10 * 60))
    .clients(10)
}

fn all_nodes_500(s: SimulationBuilder1) -> Simulation {
    s.server(|s| {
        s.name("node1")
            .handler(|h| h.response(500).response_time(Duration::from_millis(600)))
            .until(Duration::from_secs(10), "revert badness")
            .handler(|h| h.response(200).response_time(Duration::from_millis(600)))
    })
    .server(|s| {
        s.name("node2")
            .handler(|h| h.response(500).response_time(Duration::from_millis(600)))
            .until(Duration::from_secs(10), "revert badness")
            .handler(|h| h.response(200).response_time(Duration::from_millis(600)))
    })
    .requests_per_second(100.)
    .send_until(Duration::from_secs(20))
    .abort_after(Duration::from_secs(10 * 60))
    .clients(10)
}

fn black_hole(s: SimulationBuilder1) -> Simulation {
    s.server(|s| {
        s.name("node1")
            .handler(|h| h.response(200).response_time(Duration::from_millis(600)))
    })
    .server(|s| {
        s.name("node2_black_hole")
            .handler(|h| h.response(200).response_time(Duration::from_millis(600)))
            .until(Duration::from_secs(3), "black hole")
            .handler(|h| {
                h.response(200)
                    .response_time(Duration::from_secs(60 * 60 * 24))
            })
    })
    .requests_per_second(200.)
    .send_until(Duration::from_secs(10))
    .abort_after(Duration::from_secs(30))
    .clients(10)
}

fn one_endpoint_dies_on_each_server(s: SimulationBuilder1) -> Simulation {
    let endpoint1 = Endpoint::post("/e1");
    let endpoint2 = Endpoint::post("/e2");

    s.server(|s| {
        s.name("server_where_e1_breaks")
            .handler(|h| {
                h.endpoint(endpoint1.clone())
                    .response(200)
                    .response_time(Duration::from_millis(600))
            })
            .handler(|h| {
                h.endpoint(endpoint2.clone())
                    .response(200)
                    .response_time(Duration::from_millis(600))
            })
            .until(Duration::from_secs(3), "e1 breaks")
            .handler(|h| {
                h.endpoint(endpoint1.clone())
                    .response(500)
                    .response_time(Duration::from_millis(600))
            })
            .handler(|h| {
                h.endpoint(endpoint2.clone())
                    .response(200)
                    .response_time(Duration::from_millis(600))
            })
    })
    .server(|s| {
        s.name("server_where_e2_breaks")
            .handler(|h| {
                h.endpoint(endpoint1.clone())
                    .response(200)
                    .response_time(Duration::from_millis(600))
            })
            .handler(|h| {
                h.endpoint(endpoint2.clone())
                    .response(200)
                    .response_time(Duration::from_millis(600))
            })
            .until(Duration::from_secs(3), "e2 breaks")
            .handler(|h| {
                h.endpoint(endpoint1.clone())
                    .response(200)
                    .response_time(Duration::from_millis(600))
            })
            .handler(|h| {
                h.endpoint(endpoint2.clone())
                    .response(500)
                    .response_time(Duration::from_millis(600))
            })
    })
    .endpoints(vec![endpoint1, endpoint2])
    .requests_per_second(250.)
    .send_until(Duration::from_secs(10))
    .abort_after(Duration::from_secs(60))
    .clients(10)
}

fn uncommon_flakes(s: SimulationBuilder1) -> Simulation {
    s.server(|s| {
        s.name("fast0").handler(|h| {
            h.respond_500_at_rate(0.01)
                .response_time(Duration::from_nanos(1000))
        })
    })
    .server(|s| {
        s.name("fast1").handler(|h| {
            h.respond_500_at_rate(0.01)
                .response_time(Duration::from_nanos(1000))
        })
    })
    .requests_per_second(1000.)
    .send_until(Duration::from_secs(10))
    .abort_after(Duration::from_secs(10))
    .clients(10)
}

fn one_big_spike(s: SimulationBuilder1) -> Simulation {
    let capacity = 100;
    s.server(|s| {
        s.name("node1").handler(|h| {
            h.respond_200_until_capacity(429, capacity)
                .response_time(Duration::from_millis(150))
        })
    })
    .server(|s| {
        s.name("node2").handler(|h| {
            h.respond_200_until_capacity(429, capacity)
                .response_time(Duration::from_millis(150))
        })
    })
    .requests_per_second(30_000.)
    .num_requests(1000)
    .abort_after(Duration::from_secs(10))
    .clients(1)
}

fn server_side_rate_limits(mut s: SimulationBuilder1) -> Simulation {
    let total_rate_limit = 0.1;
    let num_servers = 4;
    let num_clients = 2;
    let per_server_rate_limit = total_rate_limit / num_servers as f64;

    for i in 0..num_servers {
        s = s.server(|s| {
            let request_rate = Meter::new_with(Arc::new(TokioClock));
            s.name(&format!("node{}", i)).handler(move |h| {
                h.response_with(move |_| {
                    if request_rate.one_minute_rate() < per_server_rate_limit {
                        request_rate.mark(1);
                        server::response(200)
                    } else {
                        server::response(429)
                    }
                })
                .response_time(Duration::from_secs(200))
            })
        });
    }

    s.requests_per_second(total_rate_limit)
        .send_until(Duration::from_secs(25_000 * 60))
        .abort_after(Duration::from_secs(1_000 * 60 * 60))
        .clients(num_clients)
}

fn rng() -> Pcg64 {
    // fixed seed from https://docs.rs/rand_pcg/0.2.1/rand_pcg/struct.Lcg128Xsl64.html
    Pcg64::new(0xcafef00dd15ea5e5, 0xa02bdbf7bb3c0a7ac28fa16a64abf96)
}
