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
use flate2::read::GzDecoder;
use std::path::{Path, PathBuf};
use std::{env, fs};
use tar::Archive;

const VERSION: &str = "0.19.0";

fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    let out_dir = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    get_server(&out_dir);
    get_tests(&out_dir);
    generate_api(&out_dir);
}

fn download(name: &str, classifier: &str, extension: &str) -> Vec<u8> {
    let url = format!(
        "https://repo1.maven.org/maven2/com/palantir/conjure/verification/{name}/{version}/{name}-{version}{classifier}.{extension}",
        name = name,
        version = VERSION,
        classifier = classifier,
        extension = extension,
    );

    let resp = attohttpc::get(&url).send().unwrap();

    if !resp.is_success() {
        panic!("request failed: GET {} = {}", url, resp.status());
    }

    resp.bytes().unwrap()
}

fn get_server(out_dir: &Path) {
    let dir = out_dir.join("server");
    fs::create_dir_all(&dir).unwrap();

    let classifier = if cfg!(target_os = "macos") {
        "-osx"
    } else if cfg!(target_os = "linux") {
        "-linux"
    } else {
        panic!("unsupported target")
    };
    let server = download("verification-server", classifier, "tgz");
    let mut server = &*server;

    let reader = GzDecoder::new(&mut server);
    let mut tar = Archive::new(reader);
    tar.unpack(&dir).unwrap();

    println!(
        "cargo:server={}",
        dir.join("conjure-verification-server").display()
    );
}

fn get_tests(out_dir: &Path) {
    let path = out_dir.join("test-cases.json");
    let test_cases = download("verification-server-test-cases", "", "json");
    fs::write(&path, test_cases).unwrap();
    println!("cargo:tests={}", path.display());
}

fn generate_api(out_dir: &Path) {
    let conjure_path = out_dir.join("verification-server-api.conjure.json");
    let service_api = download("verification-server-api", "", "conjure.json");
    fs::write(&conjure_path, service_api).unwrap();
    println!("cargo:api={}", conjure_path.display());

    conjure_codegen::Config::new()
        .strip_prefix(Some("com.palantir.conjure.verification".to_string()))
        .generate_files(&conjure_path, out_dir.join("conjure"))
        .unwrap();
}
