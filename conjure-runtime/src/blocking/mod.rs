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
//! A blocking version of the client.
//!
//! This simply wraps the asynchronous client, using an internal runtime. The API is identical, with the exception of
//! methods not being async.

pub use crate::blocking::body::*;
pub use crate::blocking::client::*;
pub use crate::blocking::request::*;
pub use crate::blocking::response::*;
pub use crate::blocking::shim::*;
use once_cell::sync::OnceCell;
use std::io;
use tokio::runtime::{self, Runtime};

mod body;
mod client;
mod conjure;
mod request;
mod response;
mod shim;

fn runtime() -> io::Result<&'static Runtime> {
    static RUNTIME: OnceCell<Runtime> = OnceCell::new();
    RUNTIME.get_or_try_init(|| {
        runtime::Builder::new_multi_thread()
            .enable_all()
            .thread_name("conjure-runtime")
            .build()
    })
}
