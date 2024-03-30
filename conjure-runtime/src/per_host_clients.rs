//! Per-host-client types.
use std::collections::{hash_map, HashMap};

use url::Url;

/// A collection of clients for each host of a replicated service.
pub struct PerHostClients<T> {
    pub(crate) clients: HashMap<Host, T>,
}

impl<T> PartialEq for PerHostClients<T> {
    fn eq(&self, other: &Self) -> bool {
        self.clients.len() == other.clients.len()
            && self.clients.keys().all(|k| other.clients.contains_key(k))
    }
}

impl<T> PerHostClients<T> {
    /// Returns an iterator over references to the clients and their hosts.
    pub fn iter(&self) -> Iter<'_, T> {
        Iter {
            iter: self.clients.iter(),
        }
    }
}

impl<'a, T> IntoIterator for &'a PerHostClients<T> {
    type Item = (&'a Host, &'a T);

    type IntoIter = Iter<'a, T>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

/// An identifier of a single host of a replicated service.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Host {
    pub(crate) uri: Url,
}

impl Host {
    /// Returns the base URI used to communicate with the host.
    pub fn uri(&self) -> &Url {
        &self.uri
    }
}

/// An iterator over references to per-host clients.
pub struct Iter<'a, T> {
    iter: hash_map::Iter<'a, Host, T>,
}

impl<'a, T> Iterator for Iter<'a, T> {
    type Item = (&'a Host, &'a T);

    fn next(&mut self) -> Option<Self::Item> {
        self.iter.next()
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.iter.size_hint()
    }
}

impl<T> ExactSizeIterator for Iter<'_, T> {}
