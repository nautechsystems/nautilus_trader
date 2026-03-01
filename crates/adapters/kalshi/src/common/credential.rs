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

//! RSA-PSS credential for authenticating Kalshi API requests.
//!
//! Kalshi uses RSA-PSS with SHA-256 (MGF1-SHA256, salt = digest length = 32 bytes).
//! Each request is independently signed — there are no session tokens.
//!
//! Required headers on authenticated requests:
//! - `KALSHI-ACCESS-KEY`: the API key ID (UUID)
//! - `KALSHI-ACCESS-TIMESTAMP`: Unix time in milliseconds (string)
//! - `KALSHI-ACCESS-SIGNATURE`: Base64-encoded RSA-PSS signature
//!
//! Signature message: `{timestamp_ms}{HTTP_METHOD_UPPERCASE}{path_without_query}`

use aws_lc_rs::{
    rand::SystemRandom,
    signature::{RsaKeyPair, RSA_PSS_SHA256},
};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as B64;

/// Header name for the Kalshi API key ID.
pub const HEADER_ACCESS_KEY: &str = "KALSHI-ACCESS-KEY";
/// Header name for the request timestamp (Unix milliseconds).
pub const HEADER_TIMESTAMP: &str = "KALSHI-ACCESS-TIMESTAMP";
/// Header name for the RSA-PSS signature.
pub const HEADER_SIGNATURE: &str = "KALSHI-ACCESS-SIGNATURE";

/// Parse a PEM-encoded PKCS#8 private key ("BEGIN PRIVATE KEY") into DER bytes.
fn pem_to_der(pem: &str) -> anyhow::Result<Vec<u8>> {
    let body: String = pem
        .lines()
        .filter(|l| !l.starts_with("-----"))
        .collect::<Vec<_>>()
        .join("");
    B64.decode(body.trim())
        .map_err(|e| anyhow::anyhow!("PEM base64 decode error: {e}"))
}

/// RSA-PSS signing credential for Kalshi API authentication.
///
/// Thread-safe: `SystemRandom` and `RsaKeyPair` are `Send + Sync`.
#[derive(Debug)]
pub struct KalshiCredential {
    api_key_id: String,
    key_pair: RsaKeyPair,
    rng: SystemRandom,
}

impl KalshiCredential {
    /// Create a new credential from an API key ID and PEM-encoded RSA private key.
    ///
    /// # Errors
    ///
    /// Returns an error if the PEM cannot be decoded or the key is invalid PKCS#8.
    pub fn new(api_key_id: String, private_key_pem: &str) -> anyhow::Result<Self> {
        let der = pem_to_der(private_key_pem)?;
        let key_pair = RsaKeyPair::from_pkcs8(&der)
            .map_err(|e| anyhow::anyhow!("Invalid RSA private key (PKCS#8 required): {e}"))?;
        Ok(Self {
            api_key_id,
            key_pair,
            rng: SystemRandom::new(),
        })
    }

    /// Returns the API key ID (for the `KALSHI-ACCESS-KEY` header).
    #[must_use]
    pub fn api_key_id(&self) -> &str {
        &self.api_key_id
    }

    /// Signs a request and returns `(timestamp_ms, signature_b64)`.
    ///
    /// The caller must set all three headers:
    /// - `KALSHI-ACCESS-KEY` = `self.api_key_id()`
    /// - `KALSHI-ACCESS-TIMESTAMP` = returned `timestamp_ms`
    /// - `KALSHI-ACCESS-SIGNATURE` = returned `signature_b64`
    ///
    /// # Panics
    ///
    /// Panics if the system random number generator fails (should never happen in practice).
    #[must_use]
    pub fn sign(&self, method: &str, path: &str) -> (String, String) {
        let ts_ms = chrono::Utc::now().timestamp_millis().to_string();
        // Strip query string from path before signing.
        let clean_path = path.split('?').next().unwrap_or(path);
        let msg = format!("{ts_ms}{}{clean_path}", method.to_ascii_uppercase());

        let mut sig = vec![0u8; self.key_pair.public_modulus_len()];
        self.key_pair
            .sign(&RSA_PSS_SHA256, &self.rng, msg.as_bytes(), &mut sig)
            .expect("RSA-PSS signing failed");

        (ts_ms, B64.encode(&sig))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn load_test_key() -> String {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("test_data/test_rsa_private_key.pem");
        std::fs::read_to_string(&path)
            .unwrap_or_else(|_| panic!("Run: openssl genrsa 2048 | openssl pkcs8 -topk8 -nocrypt -out crates/adapters/kalshi/test_data/test_rsa_private_key.pem"))
    }

    pub(crate) fn make_test_credential() -> KalshiCredential {
        KalshiCredential::new("test-key-id".to_string(), &load_test_key())
            .expect("valid test RSA key")
    }

    #[test]
    fn test_credential_new_invalid_pem_fails() {
        let result = KalshiCredential::new("key-id".to_string(), "not-a-pem");
        assert!(result.is_err());
    }

    #[test]
    fn test_credential_new_with_valid_key_succeeds() {
        let cred = make_test_credential();
        assert_eq!(cred.api_key_id(), "test-key-id");
    }

    #[test]
    fn test_credential_sign_produces_valid_output() {
        let cred = make_test_credential();
        let (ts, sig) = cred.sign("GET", "/trade-api/ws/v2");
        assert!(!ts.is_empty(), "timestamp must not be empty");
        assert!(!sig.is_empty(), "signature must not be empty");
        ts.parse::<u64>().expect("timestamp must be numeric milliseconds");
        B64.decode(&sig).expect("signature must be valid base64");
    }

    #[test]
    fn test_sign_strips_query_from_path() {
        let cred = make_test_credential();
        // Must not panic when path contains a query string.
        let (ts, sig) = cred.sign("GET", "/trade-api/v2/markets?ticker=KXBTC&limit=1000");
        assert!(!ts.is_empty());
        assert!(!sig.is_empty());
    }

    #[test]
    fn test_pem_to_der_rejects_non_pem() {
        assert!(pem_to_der("not-base64!!!").is_err());
    }
}
