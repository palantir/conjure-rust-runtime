// Copyright 2026 Palantir Technologies, Inc.
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

//! The client's TLS configuration.
//!
//! The cipher suites and key exchange groups are pinned here rather than inherited from the crypto
//! backend's defaults. The backend's defaults change between versions and depend on Cargo features
//! enabled elsewhere in the dependency graph (e.g. rustls's `prefer-post-quantum`).
//!
//! In particular, we only send a key share for the first group in the ClientHello. X25519MLKEM768
//! is supported, but is not first: its ~1.2KB key share would push the ClientHello past what fits
//! in a single packet, which some middleboxes mishandle. Servers that prefer it can request it with
//! a HelloRetryRequest, after which rustls sends its key share up front on later connections to
//! that server.

use rustls::crypto::aws_lc_rs::cipher_suite::{
    TLS13_AES_128_GCM_SHA256, TLS13_AES_256_GCM_SHA384, TLS13_CHACHA20_POLY1305_SHA256,
    TLS_ECDHE_ECDSA_WITH_AES_128_GCM_SHA256, TLS_ECDHE_ECDSA_WITH_AES_256_GCM_SHA384,
    TLS_ECDHE_ECDSA_WITH_CHACHA20_POLY1305_SHA256, TLS_ECDHE_RSA_WITH_AES_128_GCM_SHA256,
    TLS_ECDHE_RSA_WITH_AES_256_GCM_SHA384, TLS_ECDHE_RSA_WITH_CHACHA20_POLY1305_SHA256,
};
use rustls::crypto::aws_lc_rs::kx_group::{SECP256R1, SECP384R1, X25519, X25519MLKEM768};
use rustls::crypto::{aws_lc_rs, CryptoProvider, SupportedKxGroup};
use rustls::SupportedCipherSuite;

static CIPHER_SUITES: &[SupportedCipherSuite] = &[
    TLS13_AES_256_GCM_SHA384,
    TLS13_AES_128_GCM_SHA256,
    TLS13_CHACHA20_POLY1305_SHA256,
    TLS_ECDHE_ECDSA_WITH_AES_256_GCM_SHA384,
    TLS_ECDHE_ECDSA_WITH_AES_128_GCM_SHA256,
    TLS_ECDHE_ECDSA_WITH_CHACHA20_POLY1305_SHA256,
    TLS_ECDHE_RSA_WITH_AES_256_GCM_SHA384,
    TLS_ECDHE_RSA_WITH_AES_128_GCM_SHA256,
    TLS_ECDHE_RSA_WITH_CHACHA20_POLY1305_SHA256,
];

static KX_GROUPS: &[&dyn SupportedKxGroup] = &[X25519, SECP256R1, SECP384R1, X25519MLKEM768];

pub fn provider() -> CryptoProvider {
    let default = aws_lc_rs::default_provider();
    // If rustls's `fips` feature is enabled, the backend's defaults are restricted to FIPS-approved
    // algorithms. Apply the same restriction to our lists.
    let fips = default.fips();
    CryptoProvider {
        cipher_suites: CIPHER_SUITES
            .iter()
            .filter(|s| !fips || s.fips())
            .copied()
            .collect(),
        kx_groups: KX_GROUPS
            .iter()
            .filter(|g| !fips || g.fips())
            .copied()
            .collect(),
        ..default
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn fips_matches_backend() {
        let provider = provider();
        assert_eq!(provider.fips(), aws_lc_rs::default_provider().fips());
        assert!(!provider.cipher_suites.is_empty());
        assert!(!provider.kx_groups.is_empty());
    }
}
