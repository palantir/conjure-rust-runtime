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
//! "Raw" HTTP client APIs.
//!
//! The `conjure-runtime` `Client` wraps a "raw" HTTP client, which is used to handle the actual HTTP request layer.
//! A default raw client is provided, but this can be overridden if desired.
pub use crate::raw::body::*;
pub use crate::raw::default::*;

mod body;
mod default;
