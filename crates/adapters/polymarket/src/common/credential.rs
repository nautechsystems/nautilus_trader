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

//! Credential management for the Polymarket adapter.

use std::{
    fmt::{Debug, Display},
    str::FromStr,
};

use alloy::signers::local::PrivateKeySigner;
use aws_lc_rs::hmac;
use base64::{Engine, engine::general_purpose::URL_SAFE};
use nautilus_core::{
    env::{get_or_env_var, get_or_env_var_opt},
    hex,
};
use ustr::Ustr;
use zeroize::{Zeroize, ZeroizeOnDrop};

use crate::http::error::{Error, Result};

const API_KEY_VAR: &str = "POLYMARKET_API_KEY";
const API_SECRET_VAR: &str = "POLYMARKET_API_SECRET";
const PASSPHRASE_VAR: &str = "POLYMARKET_PASSPHRASE";
const PRIVATE_KEY_VAR: &str = "POLYMARKET_PK";
const FUNDER_VAR: &str = "POLYMARKET_FUNDER";

/// Returns `(api_key_var, api_secret_var, passphrase_var, private_key_var, funder_var)`.
#[must_use]
pub const fn credential_env_vars() -> (
    &'static str,
    &'static str,
    &'static str,
    &'static str,
    &'static str,
) {
    (
        API_KEY_VAR,
        API_SECRET_VAR,
        PASSPHRASE_VAR,
        PRIVATE_KEY_VAR,
        FUNDER_VAR,
    )
}

/// Secure wrapper for an EVM private key, zeroized on drop.
#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct EvmPrivateKey {
    formatted_key: String,
    raw_bytes: Vec<u8>,
}

impl EvmPrivateKey {
    /// Creates a new [`EvmPrivateKey`] from a hex string (with or without `0x` prefix).
    pub fn new(key: &str) -> Result<Self> {
        let key = key.trim().to_string();
        let hex_key = key.strip_prefix("0x").unwrap_or(&key);

        if hex_key.len() != 64 {
            return Err(Error::bad_request(
                "EVM private key must be 32 bytes (64 hex chars)",
            ));
        }

        if !hex_key.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(Error::bad_request("EVM private key must be valid hex"));
        }

        let normalized = hex_key.to_lowercase();
        let formatted = format!("0x{normalized}");

        let raw_bytes = hex::decode(&normalized)
            .map_err(|_| Error::bad_request("Invalid hex in private key"))?;

        if raw_bytes.len() != 32 {
            return Err(Error::bad_request(
                "EVM private key must be exactly 32 bytes",
            ));
        }

        Ok(Self {
            formatted_key: formatted,
            raw_bytes,
        })
    }

    pub fn as_hex(&self) -> &str {
        &self.formatted_key
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.raw_bytes
    }
}

impl Debug for EvmPrivateKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("EvmPrivateKey(***)")
    }
}

impl Display for EvmPrivateKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("EvmPrivateKey(***)")
    }
}

/// L2 API credential with HMAC-SHA256 signing for authenticated requests.
///
/// Stores the API key as `Ustr` (interned, used for lookups) and the
/// decoded secret as `Box<[u8]>` (zeroized on drop). The base64 secret
/// is decoded once at construction to avoid repeated decoding per request.
#[derive(Clone)]
pub struct Credential {
    api_key: Ustr,
    secret_bytes: Box<[u8]>,
    passphrase: String,
}

impl Credential {
    /// Creates a new credential. The `api_secret` must be base64-encoded.
    pub fn new(api_key: &str, api_secret: &str, passphrase: String) -> Result<Self> {
        // Polymarket API secrets are URL-safe base64 encoded
        let secret_bytes = URL_SAFE
            .decode(api_secret)
            .map_err(|e| Error::auth(format!("Invalid base64 API secret: {e}")))?
            .into_boxed_slice();

        Ok(Self {
            api_key: Ustr::from(api_key),
            secret_bytes,
            passphrase,
        })
    }

    pub fn api_key(&self) -> Ustr {
        self.api_key
    }

    pub fn passphrase(&self) -> &str {
        &self.passphrase
    }

    /// Returns the raw API secret as a base64-encoded string.
    ///
    /// Used for WebSocket user channel authentication which expects the raw
    /// secret (not an HMAC signature).
    pub fn api_secret(&self) -> String {
        URL_SAFE.encode(&*self.secret_bytes)
    }

    /// Signs a request with HMAC-SHA256 and returns the base64-encoded signature.
    ///
    /// Message format: `{timestamp}{method}{request_path}{body}`
    pub fn sign(&self, timestamp: &str, method: &str, request_path: &str, body: &str) -> String {
        let message = format!("{timestamp}{method}{request_path}{body}");
        let key = hmac::Key::new(hmac::HMAC_SHA256, &self.secret_bytes);
        let tag = hmac::sign(&key, message.as_bytes());
        URL_SAFE.encode(tag.as_ref())
    }

    /// Resolves from provided values, falling back to environment variables.
    pub fn resolve(
        api_key: Option<String>,
        api_secret: Option<String>,
        passphrase: Option<String>,
    ) -> Result<Self> {
        let key = get_or_env_var(api_key.filter(|s| !s.trim().is_empty()), API_KEY_VAR).map_err(
            |_| Error::bad_request(format!("{API_KEY_VAR} environment variable is not set")),
        )?;

        let secret = get_or_env_var(api_secret.filter(|s| !s.trim().is_empty()), API_SECRET_VAR)
            .map_err(|_| {
                Error::bad_request(format!("{API_SECRET_VAR} environment variable is not set"))
            })?;

        let pass = get_or_env_var(passphrase.filter(|s| !s.trim().is_empty()), PASSPHRASE_VAR)
            .map_err(|_| {
                Error::bad_request(format!("{PASSPHRASE_VAR} environment variable is not set"))
            })?;

        Self::new(&key, &secret, pass)
    }

    pub fn from_env() -> Result<Self> {
        Self::resolve(None, None, None)
    }
}

impl Drop for Credential {
    fn drop(&mut self) {
        self.secret_bytes.zeroize();
        self.passphrase.zeroize();
    }
}

impl Debug for Credential {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(Credential))
            .field(
                "api_key",
                &format!("{}...", &self.api_key.as_str()[..8.min(self.api_key.len())]),
            )
            .field("secret_bytes", &"***")
            .field("passphrase", &"***")
            .finish()
    }
}

/// Complete secrets configuration for Polymarket.
///
/// Ethereum address derived from the private key (lowercased with `0x` prefix).
#[derive(Clone)]
pub struct Secrets {
    pub private_key: EvmPrivateKey,
    pub credential: Credential,
    pub funder: Option<String>,
    pub address: String,
}

impl Debug for Secrets {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(Secrets))
            .field("private_key", &self.private_key)
            .field("credential", &self.credential)
            .field("address", &self.address)
            .field(
                "funder",
                &self.funder.as_deref().map(|s| {
                    if s.len() > 10 {
                        format!("{}...{}", &s[..6], &s[s.len() - 4..])
                    } else {
                        s.to_string()
                    }
                }),
            )
            .finish()
    }
}

impl Secrets {
    /// Resolves from provided values, falling back to environment variables.
    pub fn resolve(
        private_key: Option<&str>,
        api_key: Option<String>,
        api_secret: Option<String>,
        passphrase: Option<String>,
        funder: Option<String>,
    ) -> Result<Self> {
        let pk_str = get_or_env_var(
            private_key
                .filter(|s| !s.trim().is_empty())
                .map(String::from),
            PRIVATE_KEY_VAR,
        )
        .map_err(|_| {
            Error::bad_request(format!("{PRIVATE_KEY_VAR} environment variable is not set"))
        })?;

        let private_key = EvmPrivateKey::new(&pk_str)?;
        let credential = Credential::resolve(api_key, api_secret, passphrase)?;

        let funder = get_or_env_var_opt(funder.filter(|s| !s.trim().is_empty()), FUNDER_VAR)
            .filter(|s| !s.trim().is_empty());

        let key_hex = private_key
            .as_hex()
            .strip_prefix("0x")
            .unwrap_or(private_key.as_hex());
        let signer = PrivateKeySigner::from_str(key_hex)
            .map_err(|e| Error::bad_request(format!("Failed to derive address: {e}")))?;
        let address = format!("{:#x}", signer.address());

        log::info!(
            "Polymarket credentials resolved: address={}, funder={:?}, api_key={}...)",
            address,
            funder.as_deref().map(|s| &s[..10.min(s.len())]),
            &credential.api_key()[..8]
        );

        Ok(Self {
            private_key,
            credential,
            funder,
            address,
        })
    }

    pub fn from_env() -> Result<Self> {
        Self::resolve(None, None, None, None, None)
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    const TEST_PRIVATE_KEY: &str =
        "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef";

    fn test_secret_b64() -> String {
        URL_SAFE.encode(b"test_secret_key_32bytes_pad12345")
    }

    #[rstest]
    fn test_evm_private_key_with_0x_prefix() {
        let key = EvmPrivateKey::new(TEST_PRIVATE_KEY).unwrap();
        assert_eq!(key.as_hex(), TEST_PRIVATE_KEY);
        assert_eq!(key.as_bytes().len(), 32);
    }

    #[rstest]
    fn test_evm_private_key_without_0x_prefix() {
        let key = EvmPrivateKey::new(&TEST_PRIVATE_KEY[2..]).unwrap();
        assert_eq!(key.as_hex(), TEST_PRIVATE_KEY);
    }

    #[rstest]
    fn test_evm_private_key_invalid_length() {
        assert!(EvmPrivateKey::new("0x123").is_err());
    }

    #[rstest]
    fn test_evm_private_key_invalid_hex() {
        let bad = "0x123g567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef";
        assert!(EvmPrivateKey::new(bad).is_err());
    }

    #[rstest]
    fn test_evm_private_key_debug_redacts() {
        let key = EvmPrivateKey::new(TEST_PRIVATE_KEY).unwrap();
        let debug = format!("{key:?}");
        assert_eq!(debug, "EvmPrivateKey(***)");
        assert!(!debug.contains("1234"));
    }

    #[rstest]
    fn test_credential_creation() {
        let cred =
            Credential::new("test_api_key", &test_secret_b64(), "test_pass".to_string()).unwrap();
        assert_eq!(cred.api_key().as_str(), "test_api_key");
        assert_eq!(cred.passphrase(), "test_pass");
    }

    #[rstest]
    fn test_credential_invalid_base64_secret() {
        let result = Credential::new("key", "not-valid-base64!!!", "pass".to_string());
        assert!(result.is_err());
    }

    #[rstest]
    fn test_credential_sign_produces_base64() {
        let cred =
            Credential::new("key", &URL_SAFE.encode(b"test_secret"), "pass".to_string()).unwrap();

        let sig = cred.sign("1234567890", "GET", "/order", "");
        assert!(URL_SAFE.decode(&sig).is_ok());
    }

    #[rstest]
    fn test_credential_sign_deterministic() {
        let cred = Credential::new(
            "key",
            &URL_SAFE.encode(b"deterministic_test"),
            "pass".to_string(),
        )
        .unwrap();

        let sig1 = cred.sign("1000", "POST", "/order", r#"{"price":"0.5"}"#);
        let sig2 = cred.sign("1000", "POST", "/order", r#"{"price":"0.5"}"#);
        assert_eq!(sig1, sig2);
    }

    #[rstest]
    fn test_credential_sign_different_timestamps() {
        let cred =
            Credential::new("key", &URL_SAFE.encode(b"test_key"), "pass".to_string()).unwrap();

        let sig1 = cred.sign("1000", "GET", "/order", "");
        let sig2 = cred.sign("1001", "GET", "/order", "");
        assert_ne!(sig1, sig2);
    }

    #[rstest]
    fn test_credential_sign_different_methods() {
        let cred =
            Credential::new("key", &URL_SAFE.encode(b"test_key"), "pass".to_string()).unwrap();

        let sig1 = cred.sign("1000", "GET", "/order", "");
        let sig2 = cred.sign("1000", "POST", "/order", "");
        assert_ne!(sig1, sig2);
    }

    #[rstest]
    fn test_credential_sign_different_paths() {
        let cred =
            Credential::new("key", &URL_SAFE.encode(b"test_key"), "pass".to_string()).unwrap();

        let sig1 = cred.sign("1000", "GET", "/order", "");
        let sig2 = cred.sign("1000", "GET", "/trades", "");
        assert_ne!(sig1, sig2);
    }

    #[rstest]
    fn test_credential_sign_different_bodies() {
        let cred =
            Credential::new("key", &URL_SAFE.encode(b"test_key"), "pass".to_string()).unwrap();

        let sig1 = cred.sign("1000", "POST", "/order", r#"{"a":1}"#);
        let sig2 = cred.sign("1000", "POST", "/order", r#"{"a":2}"#);
        assert_ne!(sig1, sig2);
    }

    #[rstest]
    fn test_credential_sign_empty_body() {
        let cred =
            Credential::new("key", &URL_SAFE.encode(b"test_key"), "pass".to_string()).unwrap();

        let sig1 = cred.sign("1000", "GET", "/order", "");
        let sig2 = cred.sign("1000", "GET", "/order", "{}");
        assert_ne!(sig1, sig2);
    }

    // Test vectors from Polymarket SDK (rs-clob-client/src/auth.rs)
    const SDK_SECRET: &str = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=";
    const SDK_PASSPHRASE: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    #[rstest]
    fn test_credential_sign_matches_sdk_l2_vector() {
        let cred = Credential::new(
            "00000000-0000-0000-0000-000000000000",
            SDK_SECRET,
            SDK_PASSPHRASE.to_string(),
        )
        .unwrap();

        // SDK test: timestamp=1, GET, "/", empty body
        let sig = cred.sign("1", "GET", "/", "");
        assert_eq!(sig, "eHaylCwqRSOa2LFD77Nt_SaTpbsxzN8eTEI3LryhEj4=");
    }

    #[rstest]
    fn test_credential_sign_matches_sdk_hmac_vector() {
        let cred = Credential::new("key", SDK_SECRET, "pass".to_string()).unwrap();

        // SDK test: raw message "1000000test-sign/orders{"hash":"0x123"}"
        let sig = cred.sign("1000000", "test-sign", "/orders", r#"{"hash":"0x123"}"#);
        assert_eq!(sig, "4gJVbox-R6XlDK4nlaicig0_ANVL1qdcahiL8CXfXLM=");
    }

    #[rstest]
    fn test_credential_debug_redacts_secret() {
        let cred = Credential::new(
            "my_api_key_12345678",
            &test_secret_b64(),
            "my_passphrase".to_string(),
        )
        .unwrap();

        let debug = format!("{cred:?}");
        assert!(debug.contains("my_api_k..."));
        assert!(debug.contains("***"));
        assert!(!debug.contains("test_secret"));
        assert!(!debug.contains("my_passphrase"));
    }
}
