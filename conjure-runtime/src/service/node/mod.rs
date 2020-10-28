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
pub use crate::service::node::metrics::NodeMetricsLayer;
pub use crate::service::node::selector::NodeSelectorLayer;
pub use crate::service::node::uri::NodeUriLayer;
use crate::HostMetrics;
use std::sync::Arc;
use url::Url;

pub mod metrics;
pub mod selector;
pub mod uri;

pub struct Node {
    url: Url,
    host_metrics: Option<Arc<HostMetrics>>,
}

impl Node {
    #[cfg(test)]
    fn test(url: &str) -> Node {
        Node {
            url: url.parse().unwrap(),
            host_metrics: None,
        }
    }
}
