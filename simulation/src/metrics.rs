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
use std::sync::Arc;
use witchcraft_metrics::{Counter, Meter, MetricId, MetricRegistry};

pub fn global_responses(registry: &MetricRegistry) -> Arc<Counter> {
    registry.counter("globalResponses")
}

pub fn global_server_time_nanos(registry: &MetricRegistry) -> Arc<Counter> {
    registry.counter("globalServerTime")
}

pub fn active_requests(registry: &MetricRegistry, server: &'static str) -> Arc<Counter> {
    registry.counter(MetricId::new("activeRequests").with_tag("server", server))
}

pub fn request_meter(
    registry: &MetricRegistry,
    server: &'static str,
    endpoint: &str,
) -> Arc<Meter> {
    registry.meter(
        MetricId::new("request")
            .with_tag("server", server)
            .with_tag("endpoint", endpoint.to_string()),
    )
}
