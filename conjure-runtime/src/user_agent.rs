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
use once_cell::sync::Lazy;
use regex::Regex;
use std::fmt;
use witchcraft_log::warn;

static VALID_NODE: Lazy<Regex> = Lazy::new(|| Regex::new(r"^[a-zA-Z0-9][a-zA-Z0-9.\-]*$").unwrap());
static VALID_NAME: Lazy<Regex> = Lazy::new(|| Regex::new(r"^[a-zA-Z][a-zA-Z0-9\-]*$").unwrap());
static VALID_VERSION: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^[0-9]+(\.[0-9]+)*(-rc[0-9]+)?(-[0-9]+-g[a-f0-9]+)?$").unwrap());

const DEFAULT_VERSION: &str = "0.0.0";

/// A representation of an HTTP `User-Agent` header value.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct UserAgent {
    node_id: Option<String>,
    primary: Agent,
    informational: Vec<Agent>,
}

impl fmt::Display for UserAgent {
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(fmt, "{}", self.primary)?;

        if let Some(ref node_id) = self.node_id {
            write!(fmt, " (nodeId:{})", node_id)?;
        }

        for agent in &self.informational {
            write!(fmt, " {}", agent)?;
        }

        Ok(())
    }
}

impl UserAgent {
    /// Creates a new `UserAgent`.
    pub fn new(primary: Agent) -> UserAgent {
        UserAgent {
            node_id: None,
            primary,
            informational: vec![],
        }
    }

    /// Adds an additional informational agent to the `User-Agent`.
    pub fn push_agent(&mut self, agent: Agent) {
        self.informational.push(agent);
    }

    /// Sets the identifier of this node.
    ///
    /// For example, this could be the node's IP address.
    pub fn set_node_id(&mut self, node_id: &str) {
        assert!(
            VALID_NODE.is_match(node_id),
            "invalid user agent node ID `{}`",
            node_id
        );
        self.node_id = Some(node_id.to_string());
    }

    /// Returns the identifier of this node, if provided.
    pub fn node_id(&self) -> Option<&str> {
        self.node_id.as_deref()
    }

    /// Returns the primary agent.
    pub fn primary(&self) -> &Agent {
        &self.primary
    }

    /// Returns additional informational agents.
    pub fn informational(&self) -> &[Agent] {
        &self.informational
    }
}

/// A component of a [`UserAgent`].
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Agent {
    name: String,
    version: String,
}

impl fmt::Display for Agent {
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(fmt, "{}/{}", self.name, self.version)
    }
}

impl Agent {
    /// Creates a new `Agent`.
    pub fn new(name: &str, mut version: &str) -> Agent {
        assert!(VALID_NAME.is_match(name), "invalid agent name `{}`", name);

        if !VALID_VERSION.is_match(version) {
            warn!(
                "encountered invalid user agent version",
                safe: {
                    version: version,
                }
            );
            version = DEFAULT_VERSION;
        }

        Agent {
            name: name.to_string(),
            version: version.to_string(),
        }
    }

    /// Returns the agent's name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the agent's version.
    pub fn version(&self) -> &str {
        &self.version
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn fmt() {
        let mut agent = UserAgent::new(Agent::new("foobar", "1.2.3"));
        agent.set_node_id("127.0.0.1");
        agent.push_agent(Agent::new("fizzbuzz", "0.0.0-1-g12345"));
        agent.push_agent(Agent::new("btob", "1.0.0-rc1"));
        assert_eq!(
            agent.to_string(),
            "foobar/1.2.3 (nodeId:127.0.0.1) fizzbuzz/0.0.0-1-g12345 btob/1.0.0-rc1"
        );
    }

    #[test]
    fn version_fallback() {
        let agent = Agent::new("foobar", "some-invalid-version");
        assert_eq!(agent.version(), "0.0.0");
    }
}
