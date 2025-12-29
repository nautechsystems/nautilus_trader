// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

//! Request signing and authentication credentials for the Kraken API.

use std::collections::HashMap;

use aws_lc_rs::{digest, hmac};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde_urlencoded;
use zeroize::{Zeroize, ZeroizeOnDrop};

/// API credentials for Kraken authentication.
#[derive(Clone, Debug, Zeroize, ZeroizeOnDrop)]
pub struct KrakenCredential {
    api_key: String,
    api_secret: String,
}

impl KrakenCredential {
    /// Creates a new credential with the given API key and secret.
    pub fn new(api_key: impl Into<String>, api_secret: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            api_secret: api_secret.into(),
        }
    }

    /// Load credentials from environment variables for Kraken Spot.
    ///
    /// Looks for `KRAKEN_SPOT_API_KEY` and `KRAKEN_SPOT_API_SECRET`.
    ///
    /// Note: Kraken Spot does not have a testnet environment.
    ///
    /// Returns `None` if either key or secret is not set.
    #[must_use]
    pub fn from_env_spot() -> Option<Self> {
        let key = std::env::var("KRAKEN_SPOT_API_KEY").ok()?;
        let secret = std::env::var("KRAKEN_SPOT_API_SECRET").ok()?;

        Some(Self::new(key, secret))
    }

    /// Load credentials from environment variables for Kraken Futures.
    ///
    /// Looks for `KRAKEN_FUTURES_API_KEY` and `KRAKEN_FUTURES_API_SECRET` (mainnet)
    /// or `KRAKEN_FUTURES_DEMO_API_KEY` and `KRAKEN_FUTURES_DEMO_API_SECRET` (demo).
    ///
    /// Returns `None` if either key or secret is not set.
    #[must_use]
    pub fn from_env_futures(demo: bool) -> Option<Self> {
        let (key_var, secret_var) = if demo {
            (
                "KRAKEN_FUTURES_DEMO_API_KEY",
                "KRAKEN_FUTURES_DEMO_API_SECRET",
            )
        } else {
            ("KRAKEN_FUTURES_API_KEY", "KRAKEN_FUTURES_API_SECRET")
        };

        let key = std::env::var(key_var).ok()?;
        let secret = std::env::var(secret_var).ok()?;

        Some(Self::new(key, secret))
    }

    /// Resolves credentials from provided values or environment for Spot.
    ///
    /// If both `api_key` and `api_secret` are provided, uses those.
    /// Otherwise falls back to loading from environment variables.
    #[must_use]
    pub fn resolve_spot(api_key: Option<String>, api_secret: Option<String>) -> Option<Self> {
        match (api_key, api_secret) {
            (Some(k), Some(s)) => Some(Self::new(k, s)),
            _ => Self::from_env_spot(),
        }
    }

    /// Resolves credentials from provided values or environment for Futures.
    ///
    /// If both `api_key` and `api_secret` are provided, uses those.
    /// Otherwise falls back to loading from environment variables.
    #[must_use]
    pub fn resolve_futures(
        api_key: Option<String>,
        api_secret: Option<String>,
        demo: bool,
    ) -> Option<Self> {
        match (api_key, api_secret) {
            (Some(k), Some(s)) => Some(Self::new(k, s)),
            _ => Self::from_env_futures(demo),
        }
    }

    /// Returns the API key.
    pub fn api_key(&self) -> &str {
        &self.api_key
    }

    /// Returns the API key and secret as cloned strings.
    pub fn into_parts(&self) -> (String, String) {
        (self.api_key.clone(), self.api_secret.clone())
    }

    /// Sign a request for Kraken Spot REST API.
    ///
    /// Kraken Spot uses HMAC-SHA512 with the following message:
    /// - path + SHA256(nonce + POST data)
    /// - The secret is base64 decoded before signing
    ///
    /// Note: "nonce + POST data" means the nonce value string is prepended
    /// to the URL-encoded POST body, e.g., "1234567890nonce=1234567890&param=value".
    pub fn sign_spot(
        &self,
        path: &str,
        nonce: u64,
        params: &HashMap<String, String>,
    ) -> anyhow::Result<(String, String)> {
        let secret = STANDARD
            .decode(&self.api_secret)
            .map_err(|e| anyhow::anyhow!("Failed to decode API secret: {e}"))?;

        let nonce_str = nonce.to_string();
        let mut post_data = format!("nonce={nonce_str}");
        if !params.is_empty() {
            let encoded = serde_urlencoded::to_string(params)
                .map_err(|e| anyhow::anyhow!("Failed to encode params: {e}"))?;
            post_data.push('&');
            post_data.push_str(&encoded);
        }

        let sha_input = format!("{nonce_str}{post_data}");
        let hash = digest::digest(&digest::SHA256, sha_input.as_bytes());
        let mut message = path.as_bytes().to_vec();
        message.extend_from_slice(hash.as_ref());
        let key = hmac::Key::new(hmac::HMAC_SHA512, &secret);
        let signature = hmac::sign(&key, &message);

        Ok((STANDARD.encode(signature.as_ref()), post_data))
    }

    /// Sign a JSON request for Kraken Spot API (used for CancelOrderBatch, AddOrderBatch).
    ///
    /// These endpoints use JSON body instead of form-encoded.
    /// Signature: HMAC-SHA512(path + SHA256(nonce + json_body))
    pub fn sign_spot_json(
        &self,
        path: &str,
        nonce: u64,
        json_body: &str,
    ) -> anyhow::Result<String> {
        let secret = STANDARD
            .decode(&self.api_secret)
            .map_err(|e| anyhow::anyhow!("Failed to decode API secret: {e}"))?;

        let nonce_str = nonce.to_string();
        let sha_input = format!("{nonce_str}{json_body}");
        let hash = digest::digest(&digest::SHA256, sha_input.as_bytes());
        let mut message = path.as_bytes().to_vec();
        message.extend_from_slice(hash.as_ref());
        let key = hmac::Key::new(hmac::HMAC_SHA512, &secret);
        let signature = hmac::sign(&key, &message);

        Ok(STANDARD.encode(signature.as_ref()))
    }

    /// Sign a request for Kraken Futures API v3.
    ///
    /// Kraken Futures authentication steps:
    /// 1. Strip "/derivatives" prefix from endpoint path
    /// 2. Concatenate: `postData + nonce + endpointPath`
    /// 3. SHA-256 hash the concatenation
    /// 4. Base64 decode the API secret
    /// 5. HMAC-SHA-512 of the SHA-256 hash using decoded secret
    /// 6. Base64 encode the result
    ///
    /// # References
    /// - <https://docs.kraken.com/api/docs/guides/futures-rest/>
    pub fn sign_futures(&self, path: &str, post_data: &str, nonce: u64) -> anyhow::Result<String> {
        let secret = STANDARD
            .decode(&self.api_secret)
            .map_err(|e| anyhow::anyhow!("Failed to decode API secret: {e}"))?;

        let signing_path = path.strip_prefix("/derivatives").unwrap_or(path);
        let message = format!("{post_data}{nonce}{signing_path}");
        let hash = digest::digest(&digest::SHA256, message.as_bytes());
        let key = hmac::Key::new(hmac::HMAC_SHA512, &secret);
        let signature = hmac::sign(&key, hash.as_ref());

        Ok(STANDARD.encode(signature.as_ref()))
    }

    /// Sign a WebSocket challenge for Kraken Futures private feeds.
    ///
    /// The signing process is similar to REST API authentication:
    /// 1. SHA-256 hash the challenge string
    /// 2. HMAC-SHA-512 of the hash using decoded API secret
    /// 3. Base64 encode the result
    pub fn sign_ws_challenge(&self, challenge: &str) -> anyhow::Result<String> {
        let secret = STANDARD
            .decode(&self.api_secret)
            .map_err(|e| anyhow::anyhow!("Failed to decode API secret: {e}"))?;

        let hash = digest::digest(&digest::SHA256, challenge.as_bytes());
        let key = hmac::Key::new(hmac::HMAC_SHA512, &secret);
        let signature = hmac::sign(&key, hash.as_ref());

        Ok(STANDARD.encode(signature.as_ref()))
    }

    /// Returns a masked version of the API key for logging purposes.
    ///
    /// Shows first 4 and last 4 characters with ellipsis in between.
    /// For keys shorter than 8 characters, shows asterisks only.
    #[must_use]
    pub fn api_key_masked(&self) -> String {
        nautilus_core::string::mask_api_key(&self.api_key)
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_credential_creation() {
        let cred = KrakenCredential::new("test_key", "test_secret");
        assert_eq!(cred.api_key(), "test_key");
    }

    #[rstest]
    fn test_sign_futures_uses_url_encoded_post_data() {
        // This test documents that sign_futures expects URL-encoded post data,
        // which must match the body actually sent in the HTTP request.
        // Using a valid base64-encoded secret (24 bytes -> 32 base64 chars)
        let secret = STANDARD.encode(b"test_secret_key_24bytes!");
        let cred = KrakenCredential::new("test_key", secret);

        let endpoint = "/derivatives/api/v3/sendorder";
        let nonce = 1234567890u64;

        // Create params and URL-encode them (same format as HTTP client)
        let mut params = HashMap::new();
        params.insert("symbol".to_string(), "PI_XBTUSD".to_string());
        params.insert("side".to_string(), "buy".to_string());
        params.insert("orderType".to_string(), "lmt".to_string());
        params.insert("size".to_string(), "100".to_string());
        params.insert("limitPrice".to_string(), "50000.5".to_string());

        let post_data = serde_urlencoded::to_string(&params).unwrap();

        // Signature is: SHA256(postData + nonce + path) -> HMAC-SHA512 -> base64
        let signature = cred.sign_futures(endpoint, &post_data, nonce).unwrap();

        // Signature should be non-empty base64
        assert!(!signature.is_empty());
        assert!(STANDARD.decode(&signature).is_ok());

        // Same params and nonce should produce same signature (deterministic)
        let signature2 = cred.sign_futures(endpoint, &post_data, nonce).unwrap();
        assert_eq!(signature, signature2);

        // Different post_data should produce different signature
        let different_post_data = "symbol=PI_ETHUSD&side=sell";
        let different_sig = cred
            .sign_futures(endpoint, different_post_data, nonce)
            .unwrap();
        assert_ne!(signature, different_sig);

        // Different nonce should produce different signature
        let different_nonce_sig = cred.sign_futures(endpoint, &post_data, nonce + 1).unwrap();
        assert_ne!(signature, different_nonce_sig);
    }

    #[rstest]
    fn test_sign_futures_strips_derivatives_prefix() {
        // Verify that /derivatives prefix is stripped before signing
        let secret = STANDARD.encode(b"test_secret_key_24bytes!");
        let cred = KrakenCredential::new("test_key", secret);
        let nonce = 1234567890u64;

        // Signing with /derivatives prefix should produce same result as without
        let with_prefix = cred
            .sign_futures("/derivatives/api/v3/openpositions", "", nonce)
            .unwrap();
        let without_prefix = cred
            .sign_futures("/api/v3/openpositions", "", nonce)
            .unwrap();

        assert_eq!(with_prefix, without_prefix);
    }

    #[rstest]
    fn test_resolve_spot_with_both_args() {
        let result =
            KrakenCredential::resolve_spot(Some("key".to_string()), Some("secret".to_string()));
        assert!(result.is_some());
        let cred = result.unwrap();
        assert_eq!(cred.api_key(), "key");
    }

    #[rstest]
    fn test_resolve_spot_with_partial_args_falls_back_to_env() {
        // With partial args, should fall back to from_env_spot behavior
        // (either returns env creds or None if env not set)
        let result = KrakenCredential::resolve_spot(Some("key".to_string()), None);

        // If env vars are set, result should NOT use the partial key
        if let Some(cred) = result {
            assert_ne!(cred.api_key(), "key");
        }
    }

    #[rstest]
    fn test_resolve_futures_with_both_args() {
        let result = KrakenCredential::resolve_futures(
            Some("key".to_string()),
            Some("secret".to_string()),
            false,
        );
        assert!(result.is_some());
        let cred = result.unwrap();
        assert_eq!(cred.api_key(), "key");
    }

    #[rstest]
    fn test_resolve_futures_with_partial_args_falls_back_to_env() {
        // With partial args, should fall back to from_env_futures behavior
        let result = KrakenCredential::resolve_futures(Some("key".to_string()), None, false);

        // If env vars are set, result should NOT use the partial key
        if let Some(cred) = result {
            assert_ne!(cred.api_key(), "key");
        }
    }
}
