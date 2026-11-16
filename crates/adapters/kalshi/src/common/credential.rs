// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
//
//  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
//  You may not use this file except in compliance with the License.
//  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
//
//  Unless required by applicable law or agreed to in writing, software
//  distributed under the License is distributed on an "AS IS" BASIS,
//  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
//  See the License for the specific language governing permissions and
//  limitations under the License.
// -------------------------------------------------------------------------------------------------

//! Atomic credential resolution and RSA-PSS or Ed25519 request authentication.
//!
//! Config validation checks explicit pairs without reading the environment. Authenticated clients
//! resolve a complete pair and parse the private key before sending traffic.

use std::fmt::Debug;

use aws_lc_rs::{
    rand::SystemRandom,
    rsa::KeyPair,
    signature::{Ed25519KeyPair, RSA_PSS_SHA256},
};
use base64::prelude::*;
use nautilus_common::config::{ConfigError, ConfigResult};
use nautilus_core::string::secret::SecretString;
use zeroize::Zeroizing;

use super::{
    consts::{
        KALSHI_ACCESS_KEY_HEADER, KALSHI_ACCESS_SIGNATURE_HEADER, KALSHI_ACCESS_TIMESTAMP_HEADER,
    },
    enums::KalshiEnvironment,
};

/// A validated API identity and parsed RSA or Ed25519 private key.
///
/// Explicit credentials take precedence as a complete pair. Environment fallback applies only
/// when both explicit fields are absent. Diagnostics omit the identity and key material.
pub struct KalshiCredential {
    api_key_id: SecretString,
    key: SigningKey,
}

impl KalshiCredential {
    /// Resolves and validates one complete credential set for the selected environment.
    ///
    /// # Errors
    ///
    /// Returns an error if credentials are missing, empty, incomplete, or invalid, or the key
    /// is not an unencrypted RSA or Ed25519 private key in PEM format.
    pub fn resolve(
        api_key_id: Option<SecretString>,
        private_key: Option<SecretString>,
        environment: KalshiEnvironment,
    ) -> ConfigResult<Self> {
        validate_credentials(api_key_id.as_ref(), private_key.as_ref())?;

        let (api_key_id, private_key) = match api_key_id.zip(private_key) {
            Some(pair) => pair,
            None => {
                let (key_var, private_var) = credential_env_vars(environment);

                let key = std::env::var(key_var)
                    .map(SecretString::from)
                    .map_err(|_| {
                        ConfigError::invalid_value(
                            "api_key_id",
                            "selected environment credentials are missing or invalid",
                        )
                    })?;

                let private = std::env::var(private_var)
                    .map(SecretString::from)
                    .map_err(|_| {
                        ConfigError::invalid_value(
                            "private_key",
                            "selected environment credentials are missing or invalid",
                        )
                    })?;

                (key, private)
            }
        };

        validate_credentials(Some(&api_key_id), Some(&private_key))?;
        let key = decode_key(private_key.expose_secret())?;
        Ok(Self { api_key_id, key })
    }

    /// Signs a full API path and returns the three access headers.
    ///
    /// `timestamp_ms` is Unix time in milliseconds. The method is converted to uppercase.
    /// The path starts at `/`, for example `/trade-api/v2/portfolio/balance`; any query is excluded.
    /// Hostnames are not accepted.
    /// Header values retain zeroizing, redacted ownership until passed to the transport.
    ///
    /// # Errors
    ///
    /// Returns an error if the method or path is invalid, or signing fails.
    pub fn headers(
        &self,
        timestamp_ms: u64,
        method: &str,
        path: &str,
    ) -> ConfigResult<Vec<(String, SecretString)>> {
        let method = method.to_ascii_uppercase();
        if method.is_empty() || !method.bytes().all(|b| b.is_ascii_uppercase()) {
            return Err(ConfigError::invalid_value(
                "method",
                "expected an HTTP method",
            ));
        }

        let path = path.split('?').next().unwrap_or_default();
        if !path.starts_with('/') || path.starts_with("//") || path.contains('#') {
            return Err(ConfigError::invalid_value(
                "path",
                "expected a full API path",
            ));
        }

        let timestamp = timestamp_ms.to_string();
        let payload = Zeroizing::new(format!("{timestamp}{method}{path}"));

        let signature = match &self.key {
            SigningKey::Rsa(key) => {
                let mut bytes = Zeroizing::new(vec![0; key.public_modulus_len()]);
                key.sign(
                    &RSA_PSS_SHA256,
                    &SystemRandom::new(),
                    payload.as_bytes(),
                    &mut bytes,
                )
                .map_err(|_| ConfigError::invalid_value("signature", "RSA-PSS signing failed"))?;
                BASE64_STANDARD.encode(bytes.as_slice())
            }
            SigningKey::Ed25519(key) => {
                let bytes = key.try_sign(payload.as_bytes()).map_err(|_| {
                    ConfigError::invalid_value("signature", "Ed25519 signing failed")
                })?;

                BASE64_STANDARD.encode(bytes.as_ref())
            }
        };

        Ok(vec![
            (KALSHI_ACCESS_KEY_HEADER.into(), self.api_key_id.clone()),
            (KALSHI_ACCESS_TIMESTAMP_HEADER.into(), timestamp.into()),
            (KALSHI_ACCESS_SIGNATURE_HEADER.into(), signature.into()),
        ])
    }
}

impl Debug for KalshiCredential {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(KalshiCredential))
            .finish_non_exhaustive()
    }
}

enum SigningKey {
    Rsa(KeyPair),
    Ed25519(Ed25519KeyPair),
}

fn decode_key(value: &str) -> ConfigResult<SigningKey> {
    let invalid_key = || {
        ConfigError::invalid_value(
            "private_key",
            "expected one unencrypted RSA or Ed25519 private key in PEM format",
        )
    };

    let pem = value.trim();
    let (tag, rest) = pem
        .strip_prefix("-----BEGIN ")
        .and_then(|s| s.split_once("-----"))
        .ok_or_else(invalid_key)?;

    if !matches!(tag, "PRIVATE KEY" | "RSA PRIVATE KEY") {
        return Err(invalid_key());
    }

    let end = format!("-----END {tag}-----");
    let body = rest.strip_suffix(&end).ok_or_else(invalid_key)?;

    let mut encoded = Zeroizing::new(String::with_capacity(body.len()));
    encoded.extend(body.chars().filter(|c| !c.is_ascii_whitespace()));

    let mut der = Zeroizing::new(Vec::new());
    BASE64_STANDARD
        .decode_vec(encoded.as_bytes(), &mut der)
        .map_err(|_| invalid_key())?;

    if !is_complete_der(&der) {
        return Err(invalid_key());
    }

    if let Ok(key) = KeyPair::from_pkcs8(&der) {
        return Ok(SigningKey::Rsa(key));
    }

    if let Ok(key) = KeyPair::from_der(&der) {
        return Ok(SigningKey::Rsa(key));
    }

    if let Ok(key) = Ed25519KeyPair::from_pkcs8(&der) {
        return Ok(SigningKey::Ed25519(key));
    }

    Err(invalid_key())
}

// The key decoders do not reject bytes after the outer ASN.1 sequence
fn is_complete_der(der: &[u8]) -> bool {
    let [0x30, first, rest @ ..] = der else {
        return false;
    };

    if first & 0x80 == 0 {
        return rest.len() == usize::from(*first);
    }

    let count = usize::from(first & 0x7f);
    if count == 0 || count > size_of::<usize>() || rest.len() < count || rest[0] == 0 {
        return false;
    }

    let length = rest[..count].iter().try_fold(0usize, |length, b| {
        length.checked_mul(256)?.checked_add(usize::from(*b))
    });

    length.is_some_and(|length| length >= 128 && length == rest.len() - count)
}

/// Environment variable names for Kalshi API credentials.
#[must_use]
pub const fn credential_env_vars(environment: KalshiEnvironment) -> (&'static str, &'static str) {
    match environment {
        KalshiEnvironment::Production => ("KALSHI_API_KEY_ID", "KALSHI_PRIVATE_KEY"),
        KalshiEnvironment::Demo => ("KALSHI_DEMO_API_KEY_ID", "KALSHI_DEMO_PRIVATE_KEY"),
    }
}

pub(crate) fn validate_credentials(
    api_key_id: Option<&SecretString>,
    private_key: Option<&SecretString>,
) -> ConfigResult<()> {
    match (api_key_id, private_key) {
        (None, None) => Ok(()),
        (Some(key), Some(private_key)) => {
            if key.expose_secret().is_empty()
                || !key.expose_secret().bytes().all(|b| (33..=126).contains(&b))
            {
                return Err(ConfigError::invalid_value(
                    "api_key_id",
                    "must contain non-empty visible ASCII",
                ));
            }

            if private_key.expose_secret().trim().is_empty() {
                return Err(ConfigError::empty_field("private_key"));
            }

            Ok(())
        }
        _ => Err(ConfigError::invalid_value(
            "api_key_id",
            "api_key_id and private_key must be supplied together",
        )),
    }
}

#[cfg(test)]
mod tests {
    use std::process::Command;

    use aws_lc_rs::signature::{
        KeyPair as _, RSA_PKCS1_2048_8192_SHA256, RSA_PSS_2048_8192_SHA256, UnparsedPublicKey,
    };
    use rstest::rstest;

    use super::*;

    mod keys {
        include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/integration/keys.rs"
        ));
    }

    #[derive(serde::Deserialize)]
    struct Vector {
        timestamp_ms: u64,
        method: String,
        path: String,
        payload: String,
        signature: String,
    }

    #[rstest]
    fn test_ed25519_exact_openssl_vectors_and_canonical_payloads() {
        let credential = KalshiCredential::resolve(
            Some("vector-key".into()),
            Some(keys::ed25519_pkcs8().into()),
            KalshiEnvironment::Production,
        )
        .unwrap();
        let vectors: Vec<Vector> =
            serde_json::from_str(include_str!("../../test_data/auth_vectors.json")).unwrap();

        for vector in vectors {
            assert_eq!(
                vector.payload,
                format!("{}{}{}", vector.timestamp_ms, vector.method, vector.path)
            );

            for method in [vector.method.clone(), vector.method.to_ascii_lowercase()] {
                for path in [
                    vector.path.clone(),
                    format!("{}?limit=7&token=secret-query", vector.path),
                ] {
                    let headers = credential
                        .headers(vector.timestamp_ms, &method, &path)
                        .unwrap();
                    let actual = headers
                        .iter()
                        .map(|(k, v)| (k.as_str(), v.expose_secret()))
                        .collect::<Vec<_>>();
                    assert_eq!(
                        actual,
                        vec![
                            (KALSHI_ACCESS_KEY_HEADER, "vector-key"),
                            (
                                KALSHI_ACCESS_TIMESTAMP_HEADER,
                                vector.timestamp_ms.to_string().as_str()
                            ),
                            (KALSHI_ACCESS_SIGNATURE_HEADER, vector.signature.as_str()),
                        ]
                    );
                    assert!(!format!("{headers:?}").contains(&vector.signature));
                }
            }
        }

        assert_eq!(format!("{credential:?}"), "KalshiCredential { .. }");
    }

    #[rstest]
    #[case(&keys::rsa_pkcs1())]
    #[case(&keys::rsa_pkcs8())]
    fn test_rsa_pss_parameters_and_randomized_signatures(#[case] pem: &str) {
        let credential = KalshiCredential::resolve(
            Some("rsa-key".into()),
            Some(pem.into()),
            KalshiEnvironment::Production,
        )
        .unwrap();

        let SigningKey::Rsa(key) = &credential.key else {
            panic!("expected decoded RSA key");
        };

        let payload = b"1703123456789GET/trade-api/v2/portfolio/balance";
        let mut signatures = Vec::new();

        for _ in 0..2 {
            let headers = credential
                .headers(
                    1703123456789,
                    "get",
                    "/trade-api/v2/portfolio/balance?limit=4",
                )
                .unwrap();
            let signature = BASE64_STANDARD
                .decode(headers[2].1.expose_secret())
                .unwrap();
            let public_key = key.public_key().as_ref();
            UnparsedPublicKey::new(&RSA_PSS_2048_8192_SHA256, public_key)
                .verify(payload, &signature)
                .unwrap();
            assert!(
                UnparsedPublicKey::new(&RSA_PKCS1_2048_8192_SHA256, public_key)
                    .verify(payload, &signature)
                    .is_err()
            );
            assert!(
                UnparsedPublicKey::new(&RSA_PSS_2048_8192_SHA256, public_key)
                    .verify(b"1703123456GET/trade-api/v2/portfolio/balance", &signature)
                    .is_err()
            );
            assert!(!format!("{headers:?}").contains(headers[2].1.expose_secret()));
            signatures.push(signature);
        }

        assert_ne!(signatures[0], signatures[1]);
    }

    #[rstest]
    #[case(&keys::ed25519_pkcs8(), "RSA PRIVATE KEY")]
    #[case(&keys::rsa_pkcs1(), "PRIVATE KEY")]
    fn test_algorithm_selected_from_der_not_pem_label(#[case] pem: &str, #[case] label: &str) {
        let mut lines = pem.lines().collect::<Vec<_>>();
        lines.remove(0);
        lines.pop();
        let relabeled = format!(
            "-----BEGIN {label}-----\n{}\n-----END {label}-----",
            lines.join("\n")
        );
        let actual = decode_key(&relabeled).unwrap();
        assert_eq!(
            matches!(actual, SigningKey::Ed25519(_)),
            pem == keys::ed25519_pkcs8()
        );
    }

    #[rstest]
    #[case(&keys::ed25519_pkcs8())]
    #[case(&keys::rsa_pkcs1())]
    #[case(&keys::rsa_pkcs8())]
    fn test_rejects_extra_pem_or_der_and_redacts_errors(#[case] pem: &str) {
        let lines = pem.lines().collect::<Vec<_>>();
        let mut der = BASE64_STANDARD
            .decode(lines[1..lines.len() - 1].join(""))
            .unwrap();
        der.extend_from_slice(b"trailing-sensitive-data");
        let trailing_der = format!(
            "{}\n{}\n{}",
            lines[0],
            BASE64_STANDARD.encode(der),
            lines[lines.len() - 1]
        );

        for value in [
            format!("prefix-secret{pem}"),
            format!("{pem}suffix-secret"),
            format!("{pem}{pem}"),
            trailing_der,
        ] {
            let error = KalshiCredential::resolve(
                Some("sensitive-key-id".into()),
                Some(value.clone().into()),
                KalshiEnvironment::Production,
            )
            .unwrap_err();

            for rendered in [error.to_string(), format!("{error:?}")] {
                assert!(!rendered.contains("sensitive-key-id"));
                assert!(!rendered.contains(&value));
                assert!(!rendered.contains("trailing-sensitive-data"));
            }
        }
    }

    #[rstest]
    #[case("")]
    #[case(" \n")]
    #[case("not-a-private-key-secret")]
    #[case(concat!("-----BEGIN ", "PRIVATE KEY-----\n%%%secret%%%\n-----END ", "PRIVATE KEY-----"))]
    #[case(&keys::unsupported_ec())]
    #[case(&keys::encrypted())]
    fn test_rejects_invalid_and_unsupported_keys(#[case] pem: &str) {
        assert!(
            KalshiCredential::resolve(
                Some("valid-id".into()),
                Some(pem.into()),
                KalshiEnvironment::Production
            )
            .is_err()
        );
    }

    #[rstest]
    #[case("", Some(keys::ed25519_pkcs8()))]
    #[case("\nsecret", Some(keys::ed25519_pkcs8()))]
    #[case("valid-id", None)]
    fn test_rejects_invalid_explicit_pair(#[case] key: &str, #[case] pem: Option<String>) {
        assert!(
            KalshiCredential::resolve(
                Some(key.into()),
                pem.map(SecretString::from),
                KalshiEnvironment::Production
            )
            .is_err()
        );
    }

    #[rstest]
    #[case("GET\r\nkey: secret", "/trade-api/v2")]
    #[case("", "/trade-api/v2")]
    #[case("GET", "https://secret-host/trade-api/v2")]
    #[case("GET", "//secret-host/path")]
    #[case("GET", "/path#secret-fragment")]
    fn test_rejects_invalid_signing_target(#[case] method: &str, #[case] path: &str) {
        let credential = KalshiCredential::resolve(
            Some("id".into()),
            Some(keys::ed25519_pkcs8().into()),
            KalshiEnvironment::Production,
        )
        .unwrap();
        let error = credential.headers(1, method, path).unwrap_err();
        assert!(!error.to_string().contains("secret"));
    }

    #[rstest]
    fn test_environment_resolution_in_isolated_processes() {
        for scenario in [
            "production",
            "demo",
            "explicit",
            "missing",
            "incomplete",
            "cross-environment",
            "incomplete-explicit",
            "empty",
            "malformed",
        ] {
            let mut command = Command::new(std::env::current_exe().unwrap());
            command
                .args([
                    "--exact",
                    "common::credential::tests::test_environment_resolution_child",
                ])
                .env("NAUTILUS_KALSHI_AUTH_SCENARIO", scenario);

            for environment in [KalshiEnvironment::Production, KalshiEnvironment::Demo] {
                let (key, private) = credential_env_vars(environment);
                command.env_remove(key).env_remove(private);
            }

            match scenario {
                "production" | "demo" | "incomplete-explicit" => {
                    command
                        .env("KALSHI_API_KEY_ID", "production-id")
                        .env("KALSHI_PRIVATE_KEY", keys::rsa_pkcs8())
                        .env("KALSHI_DEMO_API_KEY_ID", "demo-id")
                        .env("KALSHI_DEMO_PRIVATE_KEY", keys::ed25519_pkcs8());
                }
                "explicit" => {
                    command
                        .env("KALSHI_API_KEY_ID", "invalid\n")
                        .env("KALSHI_PRIVATE_KEY", "invalid");
                }
                "incomplete" => {
                    command.env("KALSHI_API_KEY_ID", "production-id");
                }
                "cross-environment" => {
                    command
                        .env("KALSHI_API_KEY_ID", "production-id")
                        .env("KALSHI_DEMO_PRIVATE_KEY", keys::ed25519_pkcs8());
                }
                "empty" => {
                    command
                        .env("KALSHI_API_KEY_ID", "")
                        .env("KALSHI_PRIVATE_KEY", keys::rsa_pkcs8());
                }
                "malformed" => {
                    command
                        .env("KALSHI_API_KEY_ID", "production-id")
                        .env("KALSHI_PRIVATE_KEY", "invalid-sensitive-key");
                }
                _ => {}
            }

            let output = command.output().unwrap();
            let stdout = String::from_utf8_lossy(&output.stdout);
            assert!(output.status.success(), "scenario {scenario}: {stdout}");
            assert!(
                stdout.contains("test result: ok. 1 passed; 0 failed; 0 ignored;"),
                "scenario {scenario}: {stdout}"
            );
        }
    }

    #[rstest]
    fn test_environment_resolution_child() {
        let Ok(scenario) = std::env::var("NAUTILUS_KALSHI_AUTH_SCENARIO") else {
            return;
        };

        let environment = if scenario == "demo" {
            KalshiEnvironment::Demo
        } else {
            KalshiEnvironment::Production
        };

        let (key, private) = match scenario.as_str() {
            "explicit" => (
                Some("explicit-id".into()),
                Some(keys::ed25519_pkcs8().into()),
            ),
            "incomplete-explicit" => (Some("incomplete-id".into()), None),
            _ => (None, None),
        };

        let actual = KalshiCredential::resolve(key, private, environment);

        match scenario.as_str() {
            "production" | "demo" | "explicit" => {
                let credential = actual.unwrap();
                let expected = format!("{scenario}-id");
                assert_eq!(credential.api_key_id.expose_secret(), expected);
                assert_eq!(
                    matches!(credential.key, SigningKey::Rsa(_)),
                    scenario == "production"
                );
            }
            _ => assert!(actual.is_err()),
        }
    }

    #[rstest]
    fn test_credential_env_vars() {
        assert_eq!(
            credential_env_vars(KalshiEnvironment::Production),
            ("KALSHI_API_KEY_ID", "KALSHI_PRIVATE_KEY"),
        );
        assert_eq!(
            credential_env_vars(KalshiEnvironment::Demo),
            ("KALSHI_DEMO_API_KEY_ID", "KALSHI_DEMO_PRIVATE_KEY"),
        );
    }
}
