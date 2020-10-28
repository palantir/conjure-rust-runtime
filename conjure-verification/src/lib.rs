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

use conjure_error::Error;
use conjure_runtime::{blocking, Builder, Client};
use conjure_runtime::{Agent, UserAgent};
use std::future::Future;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::pin::Pin;
use std::process;
use std::process::{Child, Command, Stdio};

mod test {
    include!(concat!(env!("OUT_DIR"), "/test/mod.rs"));
}

fn main() {
    let mut builder = Builder::new();
    builder
        .uri("http://localhost:8000".parse().unwrap())
        .service("verification")
        .user_agent(UserAgent::new(Agent::new("blocking-client", "0.0.0")));

    let mut passed = 0;
    let mut failed = 0;
    test_blocking(builder.build_blocking().unwrap(), &mut passed, &mut failed);
    test_async(builder.build().unwrap(), &mut passed, &mut failed);

    println!("{} passed, {} failed", passed, failed);

    if failed != 0 {
        process::exit(1);
    }
}

fn test_blocking(client: blocking::Client, passed: &mut usize, failed: &mut usize) {
    let _server = VerificationServer::start();

    for test in test::BLOCKING_TESTS {
        match (test.test)(&client, test.index, test.value) {
            Ok(()) => *passed += 1,
            Err(e) => {
                println!(
                    "FAIL: blocking {} - {}: {}",
                    test.name,
                    test.value,
                    e.cause()
                );
                *failed += 1;
            }
        }
    }
}

#[tokio::main]
async fn test_async(client: Client, passed: &mut usize, failed: &mut usize) {
    let _server = VerificationServer::start();

    for test in test::ASYNC_TESTS {
        match (test.test)(&client, test.index, test.value).await {
            Ok(()) => *passed += 1,
            Err(e) => {
                println!("FAIL: async {} - {}: {}", test.name, test.value, e.cause());
                *failed += 1;
            }
        }
    }
}

struct VerificationServer(Child);

impl Drop for VerificationServer {
    fn drop(&mut self) {
        let _ = self.0.kill();
    }
}

impl VerificationServer {
    fn start() -> VerificationServer {
        let binary = Path::new(env!("VERIFICATION_SERVER"));
        let test_cases = Path::new(env!("TEST_CASES"));
        let conjure_api = Path::new(env!("CONJURE_API"));

        let mut child = Command::new(binary)
            .arg(test_cases)
            .arg(conjure_api)
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("failed to spawn verification server");

        let stdout = BufReader::new(child.stdout.take().unwrap());
        for line in stdout.lines() {
            let line = line.unwrap();
            if line.starts_with("Listening on ") {
                return VerificationServer(child);
            }
        }

        let _ = child.kill();
        panic!("verification server failed to start properly");
    }
}

pub struct BlockingTest {
    test: fn(&blocking::Client, i32, &'static str) -> Result<(), Error>,
    name: &'static str,
    index: i32,
    value: &'static str,
}

pub struct AsyncTest {
    #[allow(clippy::type_complexity)]
    test: fn(&Client, i32, &'static str) -> Pin<Box<dyn Future<Output = Result<(), Error>>>>,
    name: &'static str,
    index: i32,
    value: &'static str,
}

fn expect_serde_error<T>(r: Result<T, Error>) -> Result<(), Error> {
    match r {
        Ok(_) => Err(Error::internal_safe("expected deserialization failure")),
        // FIXME we should probably check for the style of failure here
        Err(_) => Ok(()),
    }
}
