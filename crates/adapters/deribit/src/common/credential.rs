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

//! Deribit API credential storage and request signing helpers.

#![allow(unused_assignments)] // Fields are accessed externally, false positive from nightly

use std::{collections::HashMap, fmt::Debug};

use aws_lc_rs::hmac;
use hex;
use nautilus_core::{UUID4, time::get_atomic_clock_realtime};
use thiserror::Error;
use ustr::Ustr;
use zeroize::ZeroizeOnDrop;

use crate::http::error::DeribitHttpError;

/// Errors that can occur when resolving credentials.
#[derive(Debug, Error)]
pub enum CredentialError {
    /// API key was provided but secret is missing.
    #[error("API key provided but secret is missing")]
    MissingSecret,
    /// API secret was provided but key is missing.
    #[error("API secret provided but key is missing")]
    MissingKey,
}

/// Deribit API credentials for signing requests.
///
/// Uses HMAC SHA256 for request signing as per Deribit API specifications.
/// Secrets are automatically zeroized on drop for security.
#[derive(Clone, ZeroizeOnDrop)]
pub struct Credential {
    #[zeroize(skip)]
    pub api_key: Ustr,
    api_secret: Box<[u8]>,
}

impl Debug for Credential {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(Credential))
            .field("api_key", &self.api_key)
            .field("api_secret", &"<redacted>")
            .finish()
    }
}

impl Credential {
    /// Creates a new [`Credential`] instance.
    #[must_use]
    pub fn new(api_key: String, api_secret: String) -> Self {
        Self {
            api_key: api_key.into(),
            api_secret: api_secret.into_bytes().into_boxed_slice(),
        }
    }

    /// Load credentials from environment variables.
    ///
    /// For mainnet: Looks for `DERIBIT_API_KEY` and `DERIBIT_API_SECRET`.
    /// For testnet: Looks for `DERIBIT_TESTNET_API_KEY` and `DERIBIT_TESTNET_API_SECRET`.
    ///
    /// Returns `None` if either key or secret is not set.
    #[must_use]
    pub fn from_env(is_testnet: bool) -> Option<Self> {
        let (key_var, secret_var) = if is_testnet {
            ("DERIBIT_TESTNET_API_KEY", "DERIBIT_TESTNET_API_SECRET")
        } else {
            ("DERIBIT_API_KEY", "DERIBIT_API_SECRET")
        };

        let key = std::env::var(key_var).ok()?;
        let secret = std::env::var(secret_var).ok()?;

        Some(Self::new(key, secret))
    }

    /// Resolves credentials from provided values or environment.
    ///
    /// If both `api_key` and `api_secret` are provided, uses those.
    /// Otherwise falls back to loading from environment variables.
    ///
    /// # Errors
    ///
    /// Returns an error if only one of `api_key` or `api_secret` is provided.
    pub fn resolve(
        api_key: Option<String>,
        api_secret: Option<String>,
        is_testnet: bool,
    ) -> Result<Option<Self>, CredentialError> {
        Self::resolve_with_env_fallback(api_key, api_secret, is_testnet, true)
    }

    /// Resolves credentials with optional environment fallback.
    ///
    /// If both `api_key` and `api_secret` are provided, uses those.
    /// If `env_fallback` is true and neither credential is provided, loads from environment.
    /// If `env_fallback` is false and neither credential is provided, returns `Ok(None)`.
    ///
    /// # Errors
    ///
    /// Returns an error if only one of `api_key` or `api_secret` is provided (partial credentials).
    /// This prevents silent fallback to environment variables when user intent is unclear.
    pub fn resolve_with_env_fallback(
        api_key: Option<String>,
        api_secret: Option<String>,
        is_testnet: bool,
        env_fallback: bool,
    ) -> Result<Option<Self>, CredentialError> {
        match (api_key, api_secret) {
            (Some(k), Some(s)) => Ok(Some(Self::new(k, s))),
            (None, None) if env_fallback => Ok(Self::from_env(is_testnet)),
            (None, None) => Ok(None),
            (Some(_), None) => Err(CredentialError::MissingSecret),
            (None, Some(_)) => Err(CredentialError::MissingKey),
        }
    }

    /// Returns the API key associated with this credential.
    #[must_use]
    pub fn api_key(&self) -> &Ustr {
        &self.api_key
    }

    /// Returns a masked version of the API key for logging purposes.
    ///
    /// Shows first 4 and last 4 characters with ellipsis in between.
    /// For keys shorter than 8 characters, shows asterisks only.
    #[must_use]
    pub fn api_key_masked(&self) -> String {
        nautilus_core::string::mask_api_key(self.api_key.as_str())
    }

    /// Signs a WebSocket authentication request according to Deribit specification.
    ///
    /// # Deribit WebSocket Signature Formula
    ///
    /// ```text
    /// StringToSign = Timestamp + "\n" + Nonce + "\n" + Data
    /// Signature = HEX_STRING(HMAC-SHA256(ClientSecret, StringToSign))
    /// ```
    ///
    /// # Returns
    ///
    /// Hex-encoded HMAC-SHA256 signature
    #[must_use]
    pub fn sign_ws_auth(&self, timestamp: u64, nonce: &str, data: &str) -> String {
        // Build string to sign: timestamp + "\n" + nonce + "\n" + data
        let string_to_sign = format!("{timestamp}\n{nonce}\n{data}");

        // Sign with HMAC-SHA256
        let key = hmac::Key::new(hmac::HMAC_SHA256, &self.api_secret[..]);
        let tag = hmac::sign(&key, string_to_sign.as_bytes());

        // Return hex-encoded signature
        hex::encode(tag.as_ref())
    }

    /// Signs a request message according to the Deribit HTTP authentication scheme.
    ///
    /// # Deribit Signature Specification
    ///
    /// ```text
    /// RequestData = UPPERCASE(HTTP_METHOD) + "\n" + URI + "\n" + RequestBody + "\n"
    /// StringToSign = Timestamp + "\n" + Nonce + "\n" + RequestData
    /// Signature = HEX_STRING(HMAC-SHA256(ClientSecret, StringToSign))
    /// ```
    ///
    /// # Parameters
    ///
    /// - `timestamp`: Milliseconds since UNIX epoch
    /// - `nonce`: Random string (typically UUID v4)
    /// - `request_data`: Pre-formatted string containing method, URI, and body
    ///
    /// # Returns
    ///
    /// Hex-encoded HMAC-SHA256 signature
    #[must_use]
    fn sign_message(&self, timestamp: i64, nonce: &str, request_data: &str) -> String {
        // Build string to sign: timestamp + "\n" + nonce + "\n" + request_data
        let string_to_sign = format!("{timestamp}\n{nonce}\n{request_data}");

        // Sign with HMAC-SHA256
        let key = hmac::Key::new(hmac::HMAC_SHA256, &self.api_secret[..]);
        let tag = hmac::sign(&key, string_to_sign.as_bytes());

        // Return hex-encoded signature (not base64 like OKX)
        hex::encode(tag.as_ref())
    }

    /// Signs a request and generates authentication headers.
    ///
    /// # Deribit Authentication Scheme
    ///
    /// ```text
    /// RequestData = UPPERCASE(HTTP_METHOD) + "\n" + URI + "\n" + RequestBody + "\n"
    /// StringToSign = Timestamp + "\n" + Nonce + "\n" + RequestData
    /// Signature = HEX_STRING(HMAC-SHA256(ClientSecret, StringToSign))
    /// Authorization: deri-hmac-sha256 id={ClientId},ts={Timestamp},nonce={Nonce},sig={Signature}
    /// ```
    ///
    /// # Errors
    ///
    /// Returns an error if credentials are not configured.
    pub fn sign_auth_headers(
        &self,
        method: &str,
        uri: &str,
        body: &[u8],
    ) -> Result<HashMap<String, String>, DeribitHttpError> {
        // Generate timestamp (milliseconds since UNIX epoch)
        let timestamp = get_atomic_clock_realtime().get_time_ms() as i64;

        // Generate random nonce (UUID v4)
        let nonce_uuid = UUID4::new();
        let nonce = nonce_uuid.as_str();

        // Build RequestData per Deribit specification
        let request_data = format!(
            "{}\n{}\n{}\n",
            method.to_uppercase(),
            uri,
            String::from_utf8_lossy(body)
        );

        // Sign the request
        let signature = self.sign_message(timestamp, nonce, &request_data);

        // Build Authorization header
        let auth_header = format!(
            "deri-hmac-sha256 id={},ts={},nonce={},sig={}",
            self.api_key(),
            timestamp,
            nonce,
            signature
        );

        let mut headers = HashMap::new();
        headers.insert("Authorization".to_string(), auth_header);

        Ok(headers)
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case("test_api_key", "test_api_secret")]
    #[case("my_key", "my_secret")]
    fn test_credential_creation(#[case] api_key: &str, #[case] api_secret: &str) {
        let credential = Credential::new(api_key.to_string(), api_secret.to_string());

        assert_eq!(credential.api_key().as_str(), api_key);
    }

    #[rstest]
    fn test_signature_generation() {
        let credential = Credential::new(
            "test_client_id".to_string(),
            "test_client_secret".to_string(),
        );

        let timestamp = 1609459200000i64;
        let nonce = "550e8400-e29b-41d4-a716-446655440000";
        let request_data = "POST\n/api/v2\n{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"private/get_account_summaries\",\"params\":{}}\n";

        let signature = credential.sign_message(timestamp, nonce, request_data);

        // Verify it's a valid hex string
        assert!(
            signature.chars().all(|c| c.is_ascii_hexdigit()),
            "Signature should be hex-encoded"
        );

        // SHA256 produces 32 bytes = 64 hex characters
        assert_eq!(
            signature.len(),
            64,
            "HMAC-SHA256 should produce 64 hex characters"
        );

        // Verify signature is deterministic
        let signature2 = credential.sign_message(timestamp, nonce, request_data);
        assert_eq!(signature, signature2, "Signature should be deterministic");
    }

    #[rstest]
    #[case(1000, 2000)]
    #[case(1000, 5000)]
    fn test_signature_changes_with_timestamp(#[case] ts1: i64, #[case] ts2: i64) {
        let credential = Credential::new("key".to_string(), "secret".to_string());
        let nonce = "nonce";
        let request_data = "POST\n/api/v2\n{}\n";

        let sig1 = credential.sign_message(ts1, nonce, request_data);
        let sig2 = credential.sign_message(ts2, nonce, request_data);

        assert_ne!(sig1, sig2, "Signature should change with timestamp");
    }

    #[rstest]
    #[case("nonce1", "nonce2")]
    #[case("abc", "xyz")]
    fn test_signature_changes_with_nonce(#[case] nonce1: &str, #[case] nonce2: &str) {
        let credential = Credential::new("key".to_string(), "secret".to_string());
        let timestamp = 1000;
        let request_data = "POST\n/api/v2\n{}\n";

        let sig1 = credential.sign_message(timestamp, nonce1, request_data);
        let sig2 = credential.sign_message(timestamp, nonce2, request_data);

        assert_ne!(sig1, sig2, "Signature should change with nonce");
    }

    #[rstest]
    #[case("POST\n/api/v2\n{\"a\":1}\n", "POST\n/api/v2\n{\"b\":2}\n")]
    #[case("GET\n/test\n\n", "POST\n/test\n\n")]
    fn test_signature_changes_with_request_data(#[case] data1: &str, #[case] data2: &str) {
        let credential = Credential::new("key".to_string(), "secret".to_string());
        let timestamp = 1000;
        let nonce = "nonce";

        let sig1 = credential.sign_message(timestamp, nonce, data1);
        let sig2 = credential.sign_message(timestamp, nonce, data2);

        assert_ne!(sig1, sig2, "Signature should change with request data");
    }

    #[rstest]
    fn test_debug_redacts_secret() {
        let credential = Credential::new("my_api_key".to_string(), "super_secret".to_string());

        let debug_output = format!("{credential:?}");

        assert!(
            debug_output.contains("<redacted>"),
            "Debug output should redact secret"
        );
        assert!(
            !debug_output.contains("super_secret"),
            "Debug output should not contain raw secret"
        );
        assert!(
            debug_output.contains("my_api_key"),
            "Debug output should contain API key"
        );
    }

    #[rstest]
    #[case("short")]
    #[case("xyz")]
    fn test_api_key_masked_short_key(#[case] key: &str) {
        let credential = Credential::new(key.to_string(), "secret".to_string());
        let masked = credential.api_key_masked();

        // Short keys should be masked differently (likely all asterisks)
        assert_ne!(masked, key, "Short key should be masked");
    }

    #[rstest]
    #[case("abcdefgh-1234-5678-ijkl", "abcd", "ijkl")]
    #[case("very-long-api-key-12345", "very", "2345")]
    fn test_api_key_masked_long_key(#[case] key: &str, #[case] start: &str, #[case] end: &str) {
        let credential = Credential::new(key.to_string(), "secret".to_string());
        let masked = credential.api_key_masked();

        // Should show first 4 and last 4 characters
        assert!(
            masked.starts_with(start),
            "Masked key should start with first 4 chars"
        );
        assert!(
            masked.ends_with(end),
            "Masked key should end with last 4 chars"
        );
        assert!(masked.contains("..."), "Masked key should contain ellipsis");
    }

    #[rstest]
    #[case("POST", "/api/v2", b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"private/get_account_summaries\",\"params\":{}}")]
    #[case("GET", "/api/v2/public/test", b"")]
    #[case(
        "POST",
        "/api/v2/private/buy",
        b"{\"instrument_name\":\"BTC-PERPETUAL\",\"amount\":100}"
    )]
    fn test_sign_auth_headers(#[case] method: &str, #[case] uri: &str, #[case] body: &[u8]) {
        let credential = Credential::new(
            "test_client_id".to_string(),
            "test_client_secret".to_string(),
        );

        let result = credential.sign_auth_headers(method, uri, body);

        assert!(result.is_ok(), "Should successfully sign auth headers");

        let headers = result.unwrap();

        // Verify Authorization header exists
        assert!(
            headers.contains_key("Authorization"),
            "Should contain Authorization header"
        );

        let auth_header = headers.get("Authorization").unwrap();

        // Verify header format: deri-hmac-sha256 id=...,ts=...,nonce=...,sig=...
        assert!(
            auth_header.starts_with("deri-hmac-sha256 "),
            "Authorization header should start with 'deri-hmac-sha256 '"
        );

        // Verify it contains all required components
        assert!(
            auth_header.contains("id=test_client_id"),
            "Should contain client ID"
        );
        assert!(auth_header.contains("ts="), "Should contain timestamp");
        assert!(auth_header.contains("nonce="), "Should contain nonce");
        assert!(auth_header.contains("sig="), "Should contain signature");

        // Verify signature is hex-encoded (64 characters after sig=)
        let sig_part = auth_header.split("sig=").nth(1).unwrap();
        assert_eq!(
            sig_part.len(),
            64,
            "Signature should be 64 hex characters (HMAC-SHA256)"
        );
        assert!(
            sig_part.chars().all(|c| c.is_ascii_hexdigit()),
            "Signature should be hex-encoded"
        );
    }

    #[rstest]
    fn test_sign_auth_headers_changes_each_call() {
        let credential = Credential::new("key".to_string(), "secret".to_string());

        let method = "POST";
        let uri = "/api/v2";
        let body = b"{}";

        let headers1 = credential.sign_auth_headers(method, uri, body).unwrap();
        // Sleep briefly to ensure different timestamp
        std::thread::sleep(Duration::from_millis(10));
        let headers2 = credential.sign_auth_headers(method, uri, body).unwrap();

        let auth1 = headers1.get("Authorization").unwrap();
        let auth2 = headers2.get("Authorization").unwrap();

        // Headers should be different due to different timestamp and nonce
        assert_ne!(
            auth1, auth2,
            "Authorization headers should differ between calls due to timestamp/nonce"
        );
    }

    #[rstest]
    fn test_sign_ws_auth_basic() {
        let credential = Credential::new(
            "test_client_id".to_string(),
            "test_client_secret".to_string(),
        );

        let timestamp = 1576074319000u64;
        let nonce = "1iqt2wls";
        let data = "";

        let signature = credential.sign_ws_auth(timestamp, nonce, data);

        assert!(
            signature.chars().all(|c| c.is_ascii_hexdigit()),
            "Signature should be hex-encoded"
        );
        assert_eq!(
            signature.len(),
            64,
            "HMAC-SHA256 should produce 64 hex characters"
        );
        let signature2 = credential.sign_ws_auth(timestamp, nonce, data);
        assert_eq!(signature, signature2, "Signature should be deterministic");
    }

    #[rstest]
    fn test_sign_ws_auth_with_known_values() {
        // Test with known values from Deribit documentation example
        // ClientSecret = "AMANDASECRECT", Timestamp = 1576074319000, Nonce = "1iqt2wls", Data = ""
        // Expected signature from docs: 56590594f97921b09b18f166befe0d1319b198bbcdad7ca73382de2f88fe9aa1
        let credential = Credential::new("AMANDA".to_string(), "AMANDASECRECT".to_string());

        let timestamp = 1576074319000u64;
        let nonce = "1iqt2wls";
        let data = "";

        let signature = credential.sign_ws_auth(timestamp, nonce, data);

        assert_eq!(
            signature, "56590594f97921b09b18f166befe0d1319b198bbcdad7ca73382de2f88fe9aa1",
            "Signature should match Deribit documentation example"
        );
    }

    #[rstest]
    #[case(1000, 2000)]
    #[case(1576074319000, 1576074320000)]
    fn test_sign_ws_auth_changes_with_timestamp(#[case] ts1: u64, #[case] ts2: u64) {
        let credential = Credential::new("key".to_string(), "secret".to_string());
        let nonce = "nonce";
        let data = "";

        let sig1 = credential.sign_ws_auth(ts1, nonce, data);
        let sig2 = credential.sign_ws_auth(ts2, nonce, data);

        assert_ne!(sig1, sig2, "Signature should change with timestamp");
    }

    #[rstest]
    #[case("nonce1", "nonce2")]
    #[case("abc123", "xyz789")]
    fn test_sign_ws_auth_changes_with_nonce(#[case] nonce1: &str, #[case] nonce2: &str) {
        let credential = Credential::new("key".to_string(), "secret".to_string());
        let timestamp = 1576074319000u64;
        let data = "";

        let sig1 = credential.sign_ws_auth(timestamp, nonce1, data);
        let sig2 = credential.sign_ws_auth(timestamp, nonce2, data);

        assert_ne!(sig1, sig2, "Signature should change with nonce");
    }

    #[rstest]
    fn test_resolve_with_both_credentials() {
        let result = Credential::resolve_with_env_fallback(
            Some("key".to_string()),
            Some("secret".to_string()),
            false,
            false,
        );

        assert!(result.is_ok());
        let credential = result.unwrap();
        assert!(credential.is_some());
        assert_eq!(credential.unwrap().api_key().as_str(), "key");
    }

    #[rstest]
    fn test_resolve_with_no_credentials_no_fallback() {
        let result = Credential::resolve_with_env_fallback(None, None, false, false);

        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }

    #[rstest]
    fn test_resolve_partial_key_only_returns_error() {
        let result =
            Credential::resolve_with_env_fallback(Some("key".to_string()), None, false, false);

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            CredentialError::MissingSecret
        ));
    }

    #[rstest]
    fn test_resolve_partial_secret_only_returns_error() {
        let result =
            Credential::resolve_with_env_fallback(None, Some("secret".to_string()), false, false);

        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), CredentialError::MissingKey));
    }
}
