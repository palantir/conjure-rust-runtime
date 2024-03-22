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
//! Error types.

use conjure_error::SerializableError;
use http::StatusCode;
use std::error::Error;
use std::fmt;
use std::time::Duration;

/// An error received from a remote service.
#[derive(Debug)]
pub struct RemoteError {
    pub(crate) status: StatusCode,
    pub(crate) error: Option<SerializableError>,
}

impl fmt::Display for RemoteError {
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.error() {
            Some(error) => write!(
                fmt,
                "remote error: {} ({}) with instance ID {}",
                error.error_code(),
                error.error_name(),
                error.error_instance_id()
            ),
            None => write!(fmt, "remote error: {}", self.status),
        }
    }
}

impl Error for RemoteError {}

impl RemoteError {
    /// Returns the status code of the response.
    pub fn status(&self) -> &StatusCode {
        &self.status
    }

    /// Returns the serialized Conjure error information in the response, if available.
    pub fn error(&self) -> Option<&SerializableError> {
        self.error.as_ref()
    }
}

/// An 429 error received from a remote service.
#[derive(Debug)]
pub struct ThrottledError {
    pub(crate) retry_after: Option<Duration>,
}

impl fmt::Display for ThrottledError {
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt.write_str("got a 429 from the remote service")
    }
}

impl Error for ThrottledError {}

/// A 503 error received from a remote service.
#[derive(Debug)]
pub struct UnavailableError(pub(crate) ());

impl fmt::Display for UnavailableError {
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt.write_str("got a 503 from the remote service")
    }
}

impl Error for UnavailableError {}
