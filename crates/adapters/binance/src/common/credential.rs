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

//! Binance API credential handling and request signing.
//!
//! This module provides two types of credentials:
//! - [`Credential`]: HMAC SHA256 signing for REST API and standard WebSocket
//! - [`Ed25519Credential`]: Ed25519 signing for WebSocket API and SBE streams
//!
//! Credentials are resolved from standard environment variables
//! (`BINANCE_API_KEY`/`BINANCE_API_SECRET`). The deprecated `*_ED25519_*`
//! variables are no longer supported and will produce a clear error.

#![allow(unused_assignments)] // Fields are used in methods; false positive on some toolchains

use std::fmt::{Debug, Display};

use aws_lc_rs::hmac;
use ed25519_dalek::{Signature, Signer, SigningKey};
use nautilus_core::{hex, string::secret::REDACTED};
use zeroize::ZeroizeOnDrop;

use super::enums::{BinanceEnvironment, BinanceProductType};

/// Resolves API credentials from config or environment variables.
///
/// Checks standard environment variables:
/// - Live: `BINANCE_API_KEY` / `BINANCE_API_SECRET`
/// - Testnet (Spot): `BINANCE_TESTNET_API_KEY` / `BINANCE_TESTNET_API_SECRET`
/// - Testnet (Futures): `BINANCE_FUTURES_TESTNET_API_KEY` / `BINANCE_FUTURES_TESTNET_API_SECRET`
/// - Demo: `BINANCE_DEMO_API_KEY` / `BINANCE_DEMO_API_SECRET`
///
/// The deprecated `*_ED25519_*` environment variables are no longer supported.
/// If detected, a clear error is returned with migration instructions.
///
/// # Errors
///
/// Returns an error if credentials cannot be resolved from config or environment.
pub fn resolve_credentials(
    config_api_key: Option<String>,
    config_api_secret: Option<String>,
    environment: BinanceEnvironment,
    product_type: BinanceProductType,
) -> anyhow::Result<(String, String)> {
    if let (Some(key), Some(secret)) = (config_api_key.clone(), config_api_secret.clone()) {
        return Ok((key, secret));
    }

    let (deprecated_key_var, deprecated_secret_var, standard_key_var, standard_secret_var) =
        match environment {
            BinanceEnvironment::Testnet => match product_type {
                BinanceProductType::Spot
                | BinanceProductType::Margin
                | BinanceProductType::Options => (
                    "BINANCE_TESTNET_ED25519_API_KEY",
                    "BINANCE_TESTNET_ED25519_API_SECRET",
                    "BINANCE_TESTNET_API_KEY",
                    "BINANCE_TESTNET_API_SECRET",
                ),
                BinanceProductType::UsdM | BinanceProductType::CoinM => (
                    "BINANCE_FUTURES_TESTNET_ED25519_API_KEY",
                    "BINANCE_FUTURES_TESTNET_ED25519_API_SECRET",
                    "BINANCE_FUTURES_TESTNET_API_KEY",
                    "BINANCE_FUTURES_TESTNET_API_SECRET",
                ),
            },

            // Demo shares API keys across all product types
            BinanceEnvironment::Demo => ("", "", "BINANCE_DEMO_API_KEY", "BINANCE_DEMO_API_SECRET"),
            BinanceEnvironment::Mainnet => (
                "BINANCE_ED25519_API_KEY",
                "BINANCE_ED25519_API_SECRET",
                "BINANCE_API_KEY",
                "BINANCE_API_SECRET",
            ),
        };

    // Futures: soft deprecation (warn + fallback),
    // Spot/Margin: hard error on removed env vars.
    let is_futures = matches!(
        product_type,
        BinanceProductType::UsdM | BinanceProductType::CoinM
    );

    let api_key = config_api_key
        .or_else(|| std::env::var(standard_key_var).ok())
        .or_else(|| resolve_deprecated_var(deprecated_key_var, standard_key_var, is_futures))
        .ok_or_else(|| anyhow::anyhow!("{standard_key_var} not found in config or environment"))?;

    let api_secret = config_api_secret
        .or_else(|| std::env::var(standard_secret_var).ok())
        .or_else(|| resolve_deprecated_var(deprecated_secret_var, standard_secret_var, is_futures))
        .ok_or_else(|| {
            anyhow::anyhow!("{standard_secret_var} not found in config or environment")
        })?;

    Ok((api_key, api_secret))
}

fn resolve_deprecated_var(
    deprecated_var: &str,
    standard_var: &str,
    allow_fallback: bool,
) -> Option<String> {
    if deprecated_var.is_empty() {
        return None;
    }

    let value = std::env::var(deprecated_var).ok()?;

    if allow_fallback {
        log::warn!(
            "'{deprecated_var}' is deprecated and will be removed in a future version. \
             Rename it to '{standard_var}' (Ed25519 keys are now auto-detected)"
        );
        Some(value)
    } else {
        log::error!(
            "'{deprecated_var}' has been removed. \
             Rename it to '{standard_var}' (Ed25519 keys are now auto-detected)"
        );
        None
    }
}

/// Binance API credentials for signing requests (HMAC SHA256).
///
/// Uses HMAC SHA256 with hexadecimal encoding, as required by Binance REST API signing.
#[derive(Clone, ZeroizeOnDrop)]
pub struct Credential {
    api_key: Box<str>,
    api_secret: Box<[u8]>,
}

/// Binance Ed25519 credentials for WebSocket API authentication.
///
/// Ed25519 is required for WebSocket API authentication (`session.logon`).
/// This is the only key type supported for execution clients.
#[derive(ZeroizeOnDrop)]
pub struct Ed25519Credential {
    api_key: Box<str>,
    signing_key: SigningKey,
}

impl Debug for Credential {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(Credential))
            .field("api_key", &self.api_key)
            .field("api_secret", &REDACTED)
            .finish()
    }
}

impl Credential {
    /// Creates a new [`Credential`] instance.
    #[must_use]
    pub fn new(api_key: String, api_secret: String) -> Self {
        Self {
            api_key: api_key.into_boxed_str(),
            api_secret: api_secret.into_bytes().into_boxed_slice(),
        }
    }

    /// Returns the API key.
    #[must_use]
    pub fn api_key(&self) -> &str {
        &self.api_key
    }

    /// Signs a message with HMAC SHA256 and returns a lowercase hex digest.
    #[must_use]
    pub fn sign(&self, message: &str) -> String {
        let key = hmac::Key::new(hmac::HMAC_SHA256, &self.api_secret);
        let tag = hmac::sign(&key, message.as_bytes());
        hex::encode(tag.as_ref())
    }
}

impl Debug for Ed25519Credential {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(Ed25519Credential))
            .field("api_key", &self.api_key)
            .field("signing_key", &REDACTED)
            .finish()
    }
}

/// Ed25519 PKCS#8 OID bytes (1.3.101.112) in DER encoding.
///
/// This five-byte sequence appears inside every PKCS#8-wrapped Ed25519 private
/// key. It is used to distinguish a genuine Ed25519 key from an arbitrary
/// base64-encoded HMAC secret, which would otherwise produce a syntactically
/// valid 32-byte signing seed and be silently misclassified.
const ED25519_OID: [u8; 5] = [0x06, 0x03, 0x2B, 0x65, 0x70];

impl Ed25519Credential {
    /// Creates a new [`Ed25519Credential`] from API key and base64-encoded private key.
    ///
    /// The private key can be provided as:
    /// - PKCS#8 DER format (48 bytes, as generated by OpenSSL)
    /// - PEM format (with or without headers)
    ///
    /// Raw 32-byte Ed25519 seeds (without PKCS#8 wrapping) are rejected: every
    /// 32-byte value is a mathematically valid seed, so accepting them would
    /// silently misclassify any base64-decodable HMAC secret as Ed25519.
    ///
    /// For PKCS#8/PEM format, the 32-byte seed is extracted from the last 32 bytes.
    ///
    /// # Errors
    ///
    /// Returns an error if the private key is not valid base64, does not carry
    /// the Ed25519 PKCS#8 OID, or is shorter than 32 bytes after decoding.
    pub fn new(api_key: String, private_key_base64: &str) -> Result<Self, Ed25519CredentialError> {
        // Strip PEM headers/footers if present
        let key_data: String = private_key_base64
            .lines()
            .filter(|line| !line.starts_with("-----"))
            .collect();

        let private_key_bytes =
            base64::Engine::decode(&base64::engine::general_purpose::STANDARD, &key_data)
                .map_err(|e| Ed25519CredentialError::InvalidBase64(e.to_string()))?;

        if !contains_subslice(&private_key_bytes, &ED25519_OID) {
            return Err(Ed25519CredentialError::NotEd25519);
        }

        if private_key_bytes.len() < 32 {
            return Err(Ed25519CredentialError::InvalidKeyLength);
        }
        let seed_start = private_key_bytes.len() - 32;
        let key_bytes: [u8; 32] = private_key_bytes[seed_start..]
            .try_into()
            .map_err(|_| Ed25519CredentialError::InvalidKeyLength)?;

        let signing_key = SigningKey::from_bytes(&key_bytes);

        Ok(Self {
            api_key: api_key.into_boxed_str(),
            signing_key,
        })
    }

    /// Returns the API key.
    #[must_use]
    pub fn api_key(&self) -> &str {
        &self.api_key
    }

    /// Signs a message with Ed25519 and returns a base64-encoded signature.
    #[must_use]
    pub fn sign(&self, message: &[u8]) -> String {
        let signature: Signature = self.signing_key.sign(message);
        base64::Engine::encode(
            &base64::engine::general_purpose::STANDARD,
            signature.to_bytes(),
        )
    }
}

/// Error type for Ed25519 credential creation.
#[derive(Debug, Clone)]
pub enum Ed25519CredentialError {
    /// The private key is not valid base64.
    InvalidBase64(String),
    /// The decoded key does not carry the Ed25519 PKCS#8 OID.
    NotEd25519,
    /// The private key is not 32 bytes.
    InvalidKeyLength,
}

impl Display for Ed25519CredentialError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidBase64(e) => write!(f, "Invalid base64 encoding: {e}"),
            Self::NotEd25519 => write!(f, "Decoded key does not carry the Ed25519 PKCS#8 OID"),
            Self::InvalidKeyLength => write!(f, "Ed25519 private key must be 32 bytes"),
        }
    }
}

impl std::error::Error for Ed25519CredentialError {}

/// Unified signing credential that auto-detects Ed25519 vs HMAC key type.
///
/// Binance supports two signing methods:
/// - HMAC SHA256 (hex-encoded signature) for REST API and standard WebSocket
/// - Ed25519 (base64-encoded signature) for WebSocket API and SBE streams
///
/// The key type is detected from the secret format: if the secret decodes as
/// valid base64 with 32+ bytes (raw seed or PKCS#8), Ed25519 is used.
/// Otherwise HMAC is used.
#[derive(Clone)]
pub enum SigningCredential {
    /// HMAC SHA256 signing.
    Hmac(Credential),
    /// Ed25519 signing.
    Ed25519(Box<Ed25519Credential>),
}

impl Debug for SigningCredential {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Hmac(c) => f.debug_tuple("Hmac").field(c).finish(),
            Self::Ed25519(c) => f.debug_tuple("Ed25519").field(c).finish(),
        }
    }
}

impl SigningCredential {
    /// Creates a new signing credential, auto-detecting Ed25519 vs HMAC.
    ///
    /// Tries Ed25519 first (base64-decoded secret must be a valid Ed25519 key).
    /// Falls back to HMAC if Ed25519 parsing fails.
    #[must_use]
    pub fn new(api_key: String, api_secret: String) -> Self {
        match Ed25519Credential::new(api_key.clone(), &api_secret) {
            Ok(ed25519) => {
                log::info!("Auto-detected Ed25519 API key");
                Self::Ed25519(Box::new(ed25519))
            }
            Err(_) => {
                log::info!("Using HMAC SHA256 API key");
                Self::Hmac(Credential::new(api_key, api_secret))
            }
        }
    }

    /// Returns the API key.
    #[must_use]
    pub fn api_key(&self) -> &str {
        match self {
            Self::Hmac(c) => c.api_key(),
            Self::Ed25519(c) => c.api_key(),
        }
    }

    /// Signs a message string and returns the signature.
    ///
    /// For HMAC: returns lowercase hex digest.
    /// For Ed25519: returns base64-encoded signature.
    #[must_use]
    pub fn sign(&self, message: &str) -> String {
        match self {
            Self::Hmac(c) => c.sign(message),
            Self::Ed25519(c) => c.sign(message.as_bytes()),
        }
    }

    /// Returns whether this credential uses Ed25519 signing.
    #[must_use]
    pub fn is_ed25519(&self) -> bool {
        matches!(self, Self::Ed25519(_))
    }
}

// Ed25519Credential does not implement Clone because SigningKey doesn't.
// Provide a manual Clone for SigningCredential by re-deriving keys.
impl Clone for Ed25519Credential {
    fn clone(&self) -> Self {
        // SigningKey is 32 bytes; extract and reconstruct
        let key_bytes = self.signing_key.to_bytes();
        Self {
            api_key: self.api_key.clone(),
            signing_key: SigningKey::from_bytes(&key_bytes),
        }
    }
}

fn contains_subslice(haystack: &[u8], needle: &[u8]) -> bool {
    if needle.is_empty() || needle.len() > haystack.len() {
        return false;
    }
    haystack.windows(needle.len()).any(|w| w == needle)
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    // Official Binance test vectors from:
    // https://github.com/binance/binance-signature-examples
    const BINANCE_TEST_SECRET: &str =
        "NhqPtmdSJYdKjVHjA7PZj4Mge3R5YNiP1e3UZjInClVN65XAbvqqM6A7H5fATj0j";

    #[rstest]
    fn test_sign_matches_binance_test_vector_simple() {
        let cred = Credential::new("test_key".to_string(), BINANCE_TEST_SECRET.to_string());
        let message = "timestamp=1578963600000";
        let expected = "d84e6641b1e328e7b418fff030caed655c266299c9355e36ce801ed14631eed4";

        assert_eq!(cred.sign(message), expected);
    }

    #[rstest]
    fn test_sign_matches_binance_test_vector_order() {
        let cred = Credential::new("test_key".to_string(), BINANCE_TEST_SECRET.to_string());
        let message = "symbol=LTCBTC&side=BUY&type=LIMIT&timeInForce=GTC&quantity=1&price=0.1&recvWindow=5000&timestamp=1499827319559";
        let expected = "c8db56825ae71d6d79447849e617115f4a920fa2acdcab2b053c4b2838bd6b71";

        assert_eq!(cred.sign(message), expected);
    }

    #[rstest]
    fn test_debug_redacts_secret() {
        let cred = Credential::new("test_key".to_string(), BINANCE_TEST_SECRET.to_string());
        let dbg_out = format!("{cred:?}");

        assert!(dbg_out.contains(REDACTED));
        assert!(!dbg_out.contains("NhqPtmdSJYdKjVHjA7PZj4"));
    }

    /// PKCS#8 DER wrapping of RFC 8032 test vector 1 Ed25519 private key.
    ///
    /// Structure: SEQUENCE { INTEGER 0, SEQUENCE { OID 1.3.101.112 },
    /// OCTET STRING { OCTET STRING { 32 key bytes } } }.
    const ED25519_PKCS8_TEST_VECTOR: [u8; 48] = [
        0x30, 0x2e, 0x02, 0x01, 0x00, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x04, 0x22, 0x04,
        0x20, 0x9d, 0x61, 0xb1, 0x9d, 0xef, 0xfd, 0x5a, 0x60, 0xba, 0x84, 0x4a, 0xf4, 0x92, 0xec,
        0x2c, 0xc4, 0x44, 0x49, 0xc5, 0x69, 0x7b, 0x32, 0x69, 0x19, 0x70, 0x3b, 0xac, 0x03, 0x1c,
        0xae, 0x7f, 0x60,
    ];

    #[rstest]
    fn test_ed25519_accepts_pkcs8_wrapped_key() {
        let key_b64 = base64::Engine::encode(
            &base64::engine::general_purpose::STANDARD,
            ED25519_PKCS8_TEST_VECTOR,
        );

        let cred = Ed25519Credential::new("test_key".to_string(), &key_b64).unwrap();

        let signature = cred.sign(b"hello");
        assert!(!signature.is_empty());
    }

    #[rstest]
    fn test_ed25519_rejects_raw_32_byte_seed() {
        // Raw 32-byte seeds decode fine but carry no PKCS#8 OID. Every
        // 32-byte value is a mathematically valid seed, so accepting raw
        // seeds would silently misclassify HMAC secrets as Ed25519.
        let seed = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, [0xABu8; 32]);

        let result = Ed25519Credential::new("test_key".to_string(), &seed);

        assert!(matches!(result, Err(Ed25519CredentialError::NotEd25519)));
    }

    #[rstest]
    fn test_ed25519_rejects_binance_hmac_secret() {
        // Regression: Binance HMAC secrets are 64-char base64 (48 bytes
        // decoded). Before the OID check they matched the PKCS#8 length and
        // were silently accepted as Ed25519, producing garbage signatures.
        let result = Ed25519Credential::new("test_key".to_string(), BINANCE_TEST_SECRET);

        assert!(matches!(result, Err(Ed25519CredentialError::NotEd25519)));
    }

    #[rstest]
    fn test_signing_credential_autodetect_falls_back_to_hmac_on_binance_secret() {
        // With the OID check in place, resolve_credentials picking an HMAC
        // secret from the env vars now correctly routes through the HMAC
        // signing path instead of generating a bogus Ed25519 signature.
        let cred = SigningCredential::new("test_key".to_string(), BINANCE_TEST_SECRET.to_string());

        assert!(matches!(cred, SigningCredential::Hmac(_)));
    }

    #[rstest]
    fn test_ed25519_debug_redacts_secret() {
        let key_b64 = base64::Engine::encode(
            &base64::engine::general_purpose::STANDARD,
            ED25519_PKCS8_TEST_VECTOR,
        );

        let cred = Ed25519Credential::new("test_key".to_string(), &key_b64).unwrap();
        let dbg_out = format!("{cred:?}");

        assert!(dbg_out.contains(REDACTED));
        assert!(!dbg_out.contains(&key_b64));
    }
}
