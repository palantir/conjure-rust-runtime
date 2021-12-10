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
use conjure_serde::json;
use conjure_verification_api::server::{
    EndpointName, IgnoredTestCases, PositiveAndNegativeTestCases, TestCases,
};
use heck::ToSnakeCase;
use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::{env, fs};

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=ignored-test-cases.yml");

    let verification_server =
        PathBuf::from(env::var_os("DEP_CONJURE_VERIFICATION_SERVER").unwrap());
    let test_cases = PathBuf::from(env::var_os("DEP_CONJURE_VERIFICATION_TESTS").unwrap());
    let api = PathBuf::from(env::var_os("DEP_CONJURE_VERIFICATION_API").unwrap());

    println!(
        "cargo:rustc-env=VERIFICATION_SERVER={}",
        verification_server.display(),
    );
    println!("cargo:rustc-env=TEST_CASES={}", test_cases.display());
    println!("cargo:rustc-env=CONJURE_API={}", api.display());

    let out_dir = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    generate_tests(&out_dir, &test_cases);
}

fn generate_tests(out_dir: &Path, test_cases: &Path) {
    let test_cases = fs::read(test_cases).unwrap();
    let test_cases = json::server_from_slice::<TestCases>(&test_cases).unwrap();

    let ignored_tests = fs::read("ignored-test-cases.yml").unwrap();
    let ignored_tests = serde_yaml::from_slice::<IgnoredTestCases>(&ignored_tests).unwrap();

    let mut blocking_tests = vec![];
    let mut async_tests = vec![];
    let mut functions = vec![];

    generate_auto_deserialize_tests(
        test_cases.client().auto_deserialize(),
        ignored_tests.client().auto_deserialize(),
        &mut blocking_tests,
        &mut async_tests,
        &mut functions,
    );
    generate_single_tests(
        test_cases.client().single_header_service(),
        ignored_tests.client().single_header_service(),
        &mut blocking_tests,
        &mut async_tests,
        &mut functions,
        "SingleHeaderService",
    );
    generate_single_tests(
        test_cases.client().single_path_param_service(),
        ignored_tests.client().single_path_param_service(),
        &mut blocking_tests,
        &mut async_tests,
        &mut functions,
        "SinglePathParamService",
    );
    generate_single_tests(
        test_cases.client().single_query_param_service(),
        ignored_tests.client().single_query_param_service(),
        &mut blocking_tests,
        &mut async_tests,
        &mut functions,
        "SingleQueryParamService",
    );

    let dir = out_dir.join("test");
    fs::create_dir_all(&dir).unwrap();

    let module = quote! {
        use crate::{BlockingTest, AsyncTest};
        use conjure_http::client::{Service, AsyncService};
        use conjure_runtime::blocking;
        use conjure_runtime::Client;
        use conjure_verification_api::server::{
            AutoDeserializeServiceAsyncClient, AutoDeserializeConfirmServiceAsyncClient, AutoDeserializeServiceClient,
            AutoDeserializeConfirmServiceClient, SingleHeaderServiceAsyncClient, SingleHeaderServiceClient,
            SinglePathParamServiceAsyncClient, SinglePathParamServiceClient, SingleQueryParamServiceAsyncClient,
            SingleQueryParamServiceClient,
        };
        use conjure_error::Error;
        use conjure_serde::json;
        use std::future::Future;
        use std::pin::Pin;

        pub const BLOCKING_TESTS: &[BlockingTest] = &[#(#blocking_tests),*];
        pub const ASYNC_TESTS: &[AsyncTest] = &[#(#async_tests),*];

        #(#functions)*
    };

    let path = dir.join("mod.rs");
    fs::write(dir.join("mod.rs"), module.to_string()).unwrap();

    let _ = Command::new("rustfmt")
        .arg("--edition=2018")
        .arg(path)
        .status();
}

fn generate_auto_deserialize_tests(
    tests: &BTreeMap<EndpointName, PositiveAndNegativeTestCases>,
    ignored_tests: &BTreeMap<EndpointName, BTreeSet<String>>,
    blocking_tests: &mut Vec<TokenStream>,
    async_tests: &mut Vec<TokenStream>,
    functions: &mut Vec<TokenStream>,
) {
    let empty_ignored = BTreeSet::new();

    for (name, cases) in tests {
        let ignored_cases = ignored_tests.get(name).unwrap_or(&empty_ignored);

        let name = &**name;
        let blocking_name = Ident::new(
            &format!("blocking_positive_{}", name.to_snake_case()),
            Span::call_site(),
        );
        let async_name = Ident::new(
            &format!("async_positive_{}", name.to_snake_case()),
            Span::call_site(),
        );

        let ref_ = if by_value_test(name) {
            quote!()
        } else {
            quote!(&)
        };
        let method = Ident::new(&name.to_snake_case(), Span::call_site());

        let mut has_test_case = false;
        for (i, case) in cases.positive().iter().enumerate() {
            if ignored_cases.contains(case) {
                continue;
            }

            has_test_case = true;

            let i = i as i32;
            blocking_tests.push(quote! {
                BlockingTest {
                    test: #blocking_name,
                    name: #name,
                    index: #i,
                    value: #case,
                }
            });
            async_tests.push(quote! {
                AsyncTest {
                    test: #async_name,
                    name: #name,
                    index: #i,
                    value: #case,
                }
            });
        }

        if has_test_case {
            functions.push(quote! {
                fn #blocking_name(client: &blocking::Client, index: i32, _: &'static str) -> Result<(), Error> {
                    let service = AutoDeserializeServiceClient::new(client.clone());
                    let confirm_service = AutoDeserializeConfirmServiceClient::new(client.clone());
                    let value = service.#method(index)?;
                    confirm_service.#method(index, #ref_ value)?;
                    Ok(())
                }

                fn #async_name(
                    client: &Client,
                    index: i32,
                    _: &'static str,
                ) -> Pin<Box<dyn Future<Output = Result<(), Error>>>> {
                    let service = AutoDeserializeServiceAsyncClient::new(client.clone());
                    let confirm_service = AutoDeserializeConfirmServiceAsyncClient::new(client.clone());
                    Box::pin(async move {
                        let value = service.#method(index).await?;
                        confirm_service.#method(index, #ref_ value).await?;
                        Ok(())
                    })
                }
            });
        }

        let blocking_name = Ident::new(
            &format!("blocking_negative_{}", name.to_snake_case()),
            Span::call_site(),
        );
        let async_name = Ident::new(
            &format!("async_negative_{}", name.to_snake_case()),
            Span::call_site(),
        );

        has_test_case = false;
        for (i, case) in cases.negative().iter().enumerate() {
            if ignored_cases.contains(case) {
                continue;
            }

            has_test_case = true;

            let i = (i + cases.positive().len()) as i32;
            blocking_tests.push(quote! {
                BlockingTest {
                    test: #blocking_name,
                    name: #name,
                    index: #i,
                    value: #case,
                }
            });
            async_tests.push(quote! {
                AsyncTest {
                    test: #async_name,
                    name: #name,
                    index: #i,
                    value: #case,
                }
            });
        }

        if has_test_case {
            functions.push(quote! {
                fn #blocking_name(client: &blocking::Client, index: i32, _: &'static str) -> Result<(), Error> {
                    let service = AutoDeserializeServiceClient::new(client.clone());
                    let r = service.#method(index);
                    crate::expect_serde_error(r)
                }

                fn #async_name(
                    client: &Client,
                    index: i32,
                    _: &'static str,
                ) -> Pin<Box<dyn Future<Output = Result<(), Error>>>> {
                    let service = AutoDeserializeServiceAsyncClient::new(client.clone());
                    Box::pin(async move {
                        let r = service.#method(index).await;
                        crate::expect_serde_error(r)
                    })
                }
            })
        }
    }
}

fn generate_single_tests(
    tests: &BTreeMap<EndpointName, Vec<String>>,
    ignored_tests: &BTreeMap<EndpointName, BTreeSet<String>>,
    blocking_tests: &mut Vec<TokenStream>,
    async_tests: &mut Vec<TokenStream>,
    functions: &mut Vec<TokenStream>,
    service: &str,
) {
    let empty_ignored = BTreeSet::new();

    let blocking_client = Ident::new(&format!("{}Client", service), Span::call_site());
    let async_client = Ident::new(&format!("{}AsyncClient", service), Span::call_site());

    for (name, cases) in tests {
        let ignored_cases = ignored_tests.get(name).unwrap_or(&empty_ignored);

        let name = &**name;
        let blocking_name = Ident::new(
            &format!("blocking_{}", name.to_snake_case()),
            Span::call_site(),
        );
        let async_name = Ident::new(
            &format!("async_{}", name.to_snake_case()),
            Span::call_site(),
        );

        let ref_ = if by_value_test(name) {
            quote!()
        } else {
            quote!(&)
        };
        let method = Ident::new(&name.to_snake_case(), Span::call_site());

        let mut has_test_case = false;
        for (i, case) in cases.iter().enumerate() {
            if ignored_cases.contains(case) {
                continue;
            }

            has_test_case = true;

            let i = i as i32;
            blocking_tests.push(quote! {
                BlockingTest {
                    test: #blocking_name,
                    name: #name,
                    index: #i,
                    value: #case,
                }
            });
            async_tests.push(quote! {
                AsyncTest {
                    test: #async_name,
                    name: #name,
                    index: #i,
                    value: #case,
                }
            });
        }

        if has_test_case {
            functions.push(quote! {
                fn #blocking_name(client: &blocking::Client, index: i32, value: &'static str) -> Result<(), Error> {
                    let service = #blocking_client::new(client.clone());
                    let value = json::server_from_str(value).map_err(Error::internal_safe)?;
                    service.#method(index, #ref_ value)
                }

                fn #async_name(
                    client: &Client,
                    index: i32,
                    value: &'static str,
                ) -> Pin<Box<dyn Future<Output = Result<(), Error>>>>
                {
                    let service = #async_client::new(client.clone());
                    Box::pin(async move {
                        let value = json::server_from_str(value).map_err(Error::internal_safe)?;
                        service.#method(index, #ref_ value).await
                    })
                }
            })
        }
    }
}

fn by_value_test(name: &str) -> bool {
    matches!(
        name,
        "receiveRawOptionalExample"
            | "receiveDoubleAliasExample"
            | "receiveIntegerAliasExample"
            | "receiveBooleanAliasExample"
            | "receiveSafeLongAliasExample"
            | "receiveUuidAliasExample"
            | "receiveDateTimeAliasExample"
            | "receiveBinaryAliasExample"
            | "receiveOptionalBooleanAliasExample"
            | "receiveOptionalDateTimeAliasExample"
            | "receiveOptionalDoubleAliasExample"
            | "receiveOptionalIntegerAliasExample"
            | "receiveOptionalSafeLongAliasExample"
            | "receiveOptionalUuidAliasExample"
            | "headerBoolean"
            | "headerDatetime"
            | "headerDouble"
            | "headerInteger"
            | "headerSafelong"
            | "headerString"
            | "headerUuid"
            | "headerOptionalOfString"
            | "pathParamBoolean"
            | "pathParamDatetime"
            | "pathParamDouble"
            | "pathParamInteger"
            | "pathParamSafelong"
            | "pathParamString"
            | "pathParamUuid"
            | "queryParamBoolean"
            | "queryParamDatetime"
            | "queryParamDouble"
            | "queryParamInteger"
            | "queryParamSafelong"
            | "queryParamString"
            | "queryParamUuid"
            | "queryParamOptionalOfString"
    )
}
