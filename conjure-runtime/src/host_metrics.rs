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
use hyper::StatusCode;
use parking_lot::Mutex;
use std::collections::hash_map;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use witchcraft_metrics::{Meter, Timer};

/// A collection of metrics about requests to service hosts.
#[derive(Default)]
pub struct HostMetricsRegistry(Mutex<Arc<HashMap<HostId, Arc<HostMetrics>>>>);

impl HostMetricsRegistry {
    /// Returns a new, empty registry.
    pub fn new() -> HostMetricsRegistry {
        Default::default()
    }

    pub(crate) fn get(&self, service: &str, host: &str, port: u16) -> Arc<HostMetrics> {
        let key = HostId {
            service: service.to_string(),
            host: host.to_string(),
            port,
        };

        Arc::make_mut(&mut *self.0.lock())
            .entry(key)
            .or_insert_with(|| Arc::new(HostMetrics::new(service, host, port)))
            .clone()
    }

    /// Returns a snapshot of the hosts in the registry.
    ///
    /// Modifications to the registry after this method is called will not affect the contents of the returned `Hosts`.
    pub fn hosts(&self) -> Hosts {
        let mut map = self.0.lock();

        // use this as an opportunity to clear out dead nodes
        Arc::make_mut(&mut *map).retain(|_, v| Arc::strong_count(v) > 1);

        Hosts(map.clone())
    }
}

#[derive(Clone, PartialEq, Eq, Hash)]
struct HostId {
    service: String,
    host: String,
    port: u16,
}

/// Metrics about requests made to a specific host of a service.
pub struct HostMetrics {
    service_name: String,
    hostname: String,
    port: u16,
    last_update: Mutex<Instant>,
    response_1xx: Timer,
    response_2xx: Timer,
    response_3xx: Timer,
    response_4xx: Timer,
    response_5xx: Timer,
    response_qos: Timer,
    response_other: Timer,
    io_error: Meter,
}

impl HostMetrics {
    pub(crate) fn new(service: &str, host: &str, port: u16) -> HostMetrics {
        HostMetrics {
            service_name: service.to_string(),
            hostname: host.to_string(),
            port,
            last_update: Mutex::new(Instant::now()),
            response_1xx: Timer::default(),
            response_2xx: Timer::default(),
            response_3xx: Timer::default(),
            response_4xx: Timer::default(),
            response_5xx: Timer::default(),
            response_qos: Timer::default(),
            response_other: Timer::default(),
            io_error: Meter::default(),
        }
    }

    pub(crate) fn update(&self, status: StatusCode, duration: Duration) {
        *self.last_update.lock() = Instant::now();
        #[allow(clippy::match_overlapping_arm)]
        let timer = match status.as_u16() {
            429 | 503 => &self.response_qos,
            100..=199 => &self.response_1xx,
            200..=299 => &self.response_2xx,
            300..=399 => &self.response_3xx,
            400..=499 => &self.response_4xx,
            500..=599 => &self.response_5xx,
            _ => &self.response_other,
        };
        timer.update(duration);
    }

    pub(crate) fn update_io_error(&self) {
        *self.last_update.lock() = Instant::now();
        self.io_error.mark(1);
    }

    /// Returns the name of the service running on this host.
    pub fn service_name(&self) -> &str {
        &self.service_name
    }

    /// Returns the hostname of the node.
    pub fn hostname(&self) -> &str {
        &self.hostname
    }

    /// Returns the port of the node.
    pub fn port(&self) -> u16 {
        self.port
    }

    /// Returns the time of the last update to the node's health metrics.
    pub fn last_update(&self) -> Instant {
        *self.last_update.lock()
    }

    /// Returns a timer recording requests to this host which returned a 1xx HTTP response.
    pub fn response_1xx(&self) -> &Timer {
        &self.response_1xx
    }

    /// Returns a timer recording requests to this host which returned a 2xx HTTP response.
    pub fn response_2xx(&self) -> &Timer {
        &self.response_2xx
    }

    /// Returns a timer recording requests to this host which returned a 3xx HTTP response.
    pub fn response_3xx(&self) -> &Timer {
        &self.response_3xx
    }

    /// Returns a timer recording requests to this host which returned a 4xx HTTP response (other than 429).
    pub fn response_4xx(&self) -> &Timer {
        &self.response_4xx
    }

    /// Returns a timer recording requests to this host which returned a 5xx HTTP response (other than 503).
    pub fn response_5xx(&self) -> &Timer {
        &self.response_5xx
    }

    /// Returns a timer recording requests to this host which returned a QoS error (a 429 or 503).
    pub fn response_qos(&self) -> &Timer {
        &self.response_qos
    }

    /// Returns a timer recording requests to this host which returned an HTTP response not in the range 100-599.
    pub fn response_other(&self) -> &Timer {
        &self.response_other
    }

    /// Returns a meter recording requests to this host which returned an IO error.
    pub fn io_error(&self) -> &Meter {
        &self.io_error
    }
}

/// A snapshot of the nodes in a registry.
pub struct Hosts(Arc<HashMap<HostId, Arc<HostMetrics>>>);

impl Hosts {
    /// Returns an iterator over the nodes.
    pub fn iter(&self) -> HostsIter<'_> {
        HostsIter(self.0.values())
    }
}

impl<'a> IntoIterator for &'a Hosts {
    type Item = &'a HostMetrics;
    type IntoIter = HostsIter<'a>;

    fn into_iter(self) -> HostsIter<'a> {
        self.iter()
    }
}

/// An iterator over host metrics.
pub struct HostsIter<'a>(hash_map::Values<'a, HostId, Arc<HostMetrics>>);

impl<'a> Iterator for HostsIter<'a> {
    type Item = &'a HostMetrics;

    #[inline]
    fn next(&mut self) -> Option<&'a HostMetrics> {
        self.0.next().map(|h| &**h)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.0.size_hint()
    }
}

#[cfg(test)]
mod test {
    use crate::HostMetricsRegistry;
    use hyper::StatusCode;
    use std::collections::HashMap;
    use std::time::Duration;

    #[test]
    fn node_gc() {
        let registry = HostMetricsRegistry::new();

        let health1 = registry.get("service", "localhost", 1234);
        let health2 = registry.get("service", "localhost", 5678);

        health1.update(StatusCode::OK, Duration::from_secs(1));
        health2.update(StatusCode::INTERNAL_SERVER_ERROR, Duration::from_secs(1));

        let snapshot = registry.hosts();
        let nodes = snapshot
            .iter()
            .map(|h| (h.port(), h))
            .collect::<HashMap<_, _>>();
        assert_eq!(nodes.len(), 2);
        assert_eq!(nodes[&1234].response_2xx().count(), 1);
        assert_eq!(nodes[&5678].response_5xx().count(), 1);
        drop(nodes);
        drop(snapshot);

        drop(health2);

        let snapshot = registry.hosts();
        let nodes = snapshot
            .iter()
            .map(|h| (h.port(), h))
            .collect::<HashMap<_, _>>();
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[&1234].response_2xx().count(), 1);
    }
}
