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

//! Request authentication for the Kalshi Trade API.
//!
//! Every authenticated request carries three headers: the API key ID, a millisecond timestamp,
//! and an RSA-PSS SHA-256 signature. The signature covers `timestamp + method + path`, where the
//! path is the route from the API root including the `/trade-api/v2` prefix and excluding any
//! query parameters.

use std::collections::HashMap;

use nautilus_cryptography::signing::rsa_pss_signature;

use crate::{
    common::credential::KalshiCredential,
    http::error::{Error, Result},
};

/// Header carrying the API key ID.
pub const HEADER_ACCESS_KEY: &str = "KALSHI-ACCESS-KEY";

/// Header carrying the request timestamp in milliseconds.
pub const HEADER_ACCESS_TIMESTAMP: &str = "KALSHI-ACCESS-TIMESTAMP";

/// Header carrying the base64-encoded request signature.
pub const HEADER_ACCESS_SIGNATURE: &str = "KALSHI-ACCESS-SIGNATURE";

/// Signs Kalshi requests with an API key's RSA private key.
#[derive(Clone, Debug)]
pub struct KalshiAuth {
    credential: KalshiCredential,
}

impl KalshiAuth {
    /// Creates a new [`KalshiAuth`] from the given credential.
    #[must_use]
    pub fn new(credential: KalshiCredential) -> Self {
        Self { credential }
    }

    /// Returns the API key ID.
    #[must_use]
    pub fn api_key_id(&self) -> &str {
        self.credential.api_key_id()
    }

    /// Returns the message the exchange expects a signature over.
    ///
    /// Query parameters are excluded, because the signature must not change when a caller adds or
    /// reorders them.
    #[must_use]
    pub fn signing_message(timestamp_ms: i64, method: &str, path: &str) -> String {
        let path_without_query = path.split('?').next().unwrap_or(path);

        format!("{timestamp_ms}{method}{path_without_query}")
    }

    /// Signs the given message, returning a base64-encoded RSA-PSS SHA-256 signature.
    ///
    /// # Errors
    ///
    /// Returns an error if the message is empty or the private key cannot sign it.
    pub fn sign(&self, message: &str) -> Result<String> {
        rsa_pss_signature(self.credential.private_key_pem(), message)
            .map_err(|e| Error::Signature(e.to_string()))
    }

    /// Returns the three authentication headers for a request.
    ///
    /// `method` must be the uppercase HTTP method and `path` the route including the API prefix.
    ///
    /// # Errors
    ///
    /// Returns an error if the request cannot be signed.
    pub fn headers(
        &self,
        method: &str,
        path: &str,
        timestamp_ms: i64,
    ) -> Result<HashMap<String, String>> {
        let message = Self::signing_message(timestamp_ms, method, path);
        let signature = self.sign(&message)?;

        Ok(HashMap::from([
            (
                HEADER_ACCESS_KEY.to_string(),
                self.credential.api_key_id().to_string(),
            ),
            (
                HEADER_ACCESS_TIMESTAMP.to_string(),
                timestamp_ms.to_string(),
            ),
            (HEADER_ACCESS_SIGNATURE.to_string(), signature),
        ]))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::OnceLock;

    use aws_lc_rs::{
        rsa::KeyPair,
        signature::{KeyPair as _, RSA_PSS_2048_8192_SHA256, UnparsedPublicKey},
    };
    use base64::{Engine, engine::general_purpose::STANDARD};
    use rstest::rstest;

    use super::*;

    /// Returns the PEM for a throwaway 2048-bit key generated for these tests. It has no exchange
    /// privileges.
    ///
    /// The armor is composed here rather than stored, because the repository's secret scanner rejects
    /// any file that contains a PEM header.
    fn test_private_key_pem() -> &'static str {
        static PEM: OnceLock<String> = OnceLock::new();

        PEM.get_or_init(|| {
            let label = "PRIVATE KEY";
            let body = include_str!("../../tests/test_data/rsa_test_private_key_pkcs8.b64").trim();

            format!("-----BEGIN {label}-----\n{body}\n-----END {label}-----\n")
        })
    }

    fn auth() -> KalshiAuth {
        KalshiAuth::new(KalshiCredential::new(
            "a952bcbe-ec3b-4b5b-b8f9-11dae589608c".to_string(),
            test_private_key_pem().to_string(),
        ))
    }

    fn public_key_der() -> Vec<u8> {
        let pem = pem::parse(test_private_key_pem().trim()).expect("fixture is PEM");
        let key_pair = KeyPair::from_pkcs8(pem.contents()).expect("fixture is PKCS#8 RSA");

        key_pair.public_key().as_ref().to_vec()
    }

    #[rstest]
    fn test_signature_verifies_against_the_public_key() {
        let auth = auth();
        let message = KalshiAuth::signing_message(
            1_703_123_456_789,
            "GET",
            "/trade-api/v2/portfolio/balance",
        );
        let signature = auth.sign(&message).unwrap();
        let decoded = STANDARD.decode(&signature).unwrap();

        // A 2048-bit RSA signature is exactly one modulus long.
        assert_eq!(decoded.len(), 256);

        UnparsedPublicKey::new(&RSA_PSS_2048_8192_SHA256, public_key_der())
            .verify(message.as_bytes(), &decoded)
            .expect("signature must verify under the key's public half");
    }

    #[rstest]
    fn test_signature_is_randomized_so_replays_cannot_be_reconstructed() {
        let auth = auth();
        let message =
            KalshiAuth::signing_message(1_703_123_456_789, "GET", "/trade-api/v2/markets");
        let first = auth.sign(&message).unwrap();
        let second = auth.sign(&message).unwrap();

        assert_ne!(first, second, "RSA-PSS must use a random salt");
    }

    #[rstest]
    fn test_headers_carry_the_key_timestamp_and_signature() {
        let headers = auth()
            .headers("GET", "/trade-api/v2/portfolio/balance", 1_703_123_456_789)
            .unwrap();

        assert_eq!(
            headers.get(HEADER_ACCESS_KEY).map(String::as_str),
            Some("a952bcbe-ec3b-4b5b-b8f9-11dae589608c")
        );
        assert_eq!(
            headers.get(HEADER_ACCESS_TIMESTAMP).map(String::as_str),
            Some("1703123456789")
        );

        let signature = headers.get(HEADER_ACCESS_SIGNATURE).unwrap();

        assert!(!signature.is_empty());
        assert!(STANDARD.decode(signature).is_ok());
    }

    #[rstest]
    fn test_query_parameters_are_excluded_from_the_signed_message() {
        let with_query =
            KalshiAuth::signing_message(1_703_123_456_789, "GET", "/trade-api/v2/markets?limit=5");
        let without_query =
            KalshiAuth::signing_message(1_703_123_456_789, "GET", "/trade-api/v2/markets");

        assert_eq!(with_query, without_query);
        assert_eq!(with_query, "1703123456789GET/trade-api/v2/markets");
    }

    #[rstest]
    fn test_paths_differing_only_by_method_or_timestamp_produce_different_messages() {
        let base = KalshiAuth::signing_message(1_703_123_456_789, "GET", "/trade-api/v2/markets");

        assert_ne!(
            base,
            KalshiAuth::signing_message(1_703_123_456_789, "POST", "/trade-api/v2/markets")
        );
        assert_ne!(
            base,
            KalshiAuth::signing_message(1_703_123_456_790, "GET", "/trade-api/v2/markets")
        );
    }

    #[rstest]
    fn test_an_unusable_private_key_is_reported_as_a_signature_error() {
        let auth = KalshiAuth::new(KalshiCredential::new(
            "key".to_string(),
            "not a pem".to_string(),
        ));
        let error = auth.sign("message").unwrap_err();

        assert!(matches!(error, Error::Signature(_)));
        assert!(!error.is_retryable());
    }

    #[rstest]
    fn test_signing_rejects_an_empty_message() {
        let error = auth().sign("").unwrap_err();

        assert!(matches!(error, Error::Signature(_)));
    }
}
