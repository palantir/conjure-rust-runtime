// Copyright 2025 Palantir Technologies, Inc.
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
#![cfg(all(test, target_arch = "wasm32"))]

use bytes::BytesMut;
use conjure_http::client::{Endpoint, LocalAsyncClient, LocalAsyncRequestBody};
use conjure_runtime::{Agent, Builder, UserAgent};
use futures_util::TryStreamExt;
use http::Request;
use std::str;
use wasm_bindgen_test::wasm_bindgen_test;

#[wasm_bindgen_test]
async fn google() {
    let client = Builder::new()
        .service("google")
        .user_agent(UserAgent::new(Agent::new(
            "conjure-runtime",
            env!("CARGO_PKG_VERSION"),
        )))
        .uri("https://google.com".parse().unwrap())
        .build()
        .unwrap();

    let req = Request::builder()
        .uri("/")
        .extension(Endpoint::new("google", None, "get", "/"))
        .body(LocalAsyncRequestBody::Empty)
        .unwrap();
    let resp = client.send(req).await.unwrap();
    let body = resp.into_body().try_collect::<BytesMut>().await.unwrap();
    let body = str::from_utf8(&body).unwrap();

    assert!(body.contains("</html>"));
}
