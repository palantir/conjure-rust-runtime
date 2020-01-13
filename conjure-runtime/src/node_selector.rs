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
use conjure_runtime_config::ServiceConfig;
use parking_lot::Mutex;
use rand::seq::SliceRandom;
use std::sync::Arc;
use std::time::{Duration, Instant};
use url::Url;
use witchcraft_log::debug;

use crate::{HostMetrics, HostMetricsRegistry};

pub(crate) struct Node {
    pub url: Url,
    pub host_metrics: Arc<HostMetrics>,
    timeout: Mutex<Instant>,
}

pub(crate) struct NodeSelector {
    nodes: Vec<Node>,
    failed_url_cooldown: Duration,
}

impl NodeSelector {
    pub fn new(
        service: &str,
        host_metrics: &HostMetricsRegistry,
        config: &ServiceConfig,
    ) -> NodeSelector {
        let nodes = config
            .uris()
            .iter()
            .map(|url| {
                // normalize by stripping a trailing `/` if present
                let mut url = url.clone();
                url.path_segments_mut().unwrap().pop_if_empty();

                Node {
                    host_metrics: host_metrics.get(
                        service,
                        url.host_str().unwrap(),
                        url.port_or_known_default().unwrap(),
                    ),
                    url,
                    timeout: Mutex::new(Instant::now()),
                }
            })
            .collect::<Vec<_>>();

        NodeSelector {
            nodes,
            failed_url_cooldown: config.failed_url_cooldown(),
        }
    }

    pub fn iter(&self) -> Iter<'_> {
        let mut shuffle = (0..self.nodes.len()).collect::<Vec<_>>();
        shuffle.shuffle(&mut rand::thread_rng());
        Iter {
            selector: self,
            shuffle,
            cur: 0,
        }
    }
}

pub(crate) struct Iter<'a> {
    selector: &'a NodeSelector,
    shuffle: Vec<usize>,
    cur: usize,
}

impl<'a> Iterator for Iter<'a> {
    type Item = &'a Node;

    fn next(&mut self) -> Option<&'a Node> {
        let now = Instant::now();

        for _ in 0..self.shuffle.len() {
            let node = self.next_node();
            let timeout = *node.timeout.lock();

            // NB: it's important to allow now == timeout here since we initialize all nodes with a now timeout
            if timeout <= now {
                return Some(node);
            } else {
                debug!("skipping node due to earlier failure", safe: { url: node.url });
            }
        }

        // If all of the nodes are timed out, just pick the next one. It's better to try again even on a failed node
        // than give up.
        if self.shuffle.is_empty() {
            None
        } else {
            let node = self.next_node();
            debug!("falling back to a timed out node", safe: { url: node.url });
            Some(node)
        }
    }
}

impl<'a> Iter<'a> {
    fn node(&self) -> &'a Node {
        let idx = self.shuffle[self.cur];
        &self.selector.nodes[idx]
    }

    fn next_node(&mut self) -> &'a Node {
        self.cur = (self.cur + 1) % self.shuffle.len();
        self.node()
    }

    pub fn prev_failed(&self) {
        *self.node().timeout.lock() = Instant::now() + self.selector.failed_url_cooldown;
    }
}
