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
use ed25519_dalek::{
    PUBLIC_KEY_LENGTH, Signature, Signer, SigningKey,
    pkcs8::{KeypairBytes, PrivateKeyInfoRef},
};
use nautilus_core::{
    hex,
    string::secret::{REDACTED, SecretString},
};
use zeroize::{Zeroize, ZeroizeOnDrop, Zeroizing};

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
            BinanceEnvironment::Live => (
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
            .field("api_key", &REDACTED)
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
            .field("api_key", &REDACTED)
            .field("signing_key", &REDACTED)
            .finish()
    }
}

impl Ed25519Credential {
    /// Creates a new [`Ed25519Credential`] from API key and base64-encoded private key.
    ///
    /// The private key can be provided as:
    /// - PKCS#8 DER format (including an optional matching public key)
    /// - PEM format (with or without headers)
    ///
    /// Raw 32-byte Ed25519 seeds (without PKCS#8 wrapping) are rejected: every
    /// 32-byte value is a mathematically valid seed, so accepting them would
    /// silently misclassify any base64-decodable HMAC secret as Ed25519.
    ///
    /// # Errors
    ///
    /// Returns an error if the private key is not valid base64 or Ed25519 PKCS#8,
    /// or if an included public key does not match the private key.
    pub fn new(
        api_key: SecretString,
        private_key_base64: SecretString,
    ) -> Result<Self, Ed25519CredentialError> {
        let private_key_base64 = Zeroizing::new(private_key_base64.into_inner());

        // Strip PEM headers/footers if present
        let key_data = Zeroizing::new(
            private_key_base64
                .lines()
                .filter(|line| !line.starts_with("-----"))
                .collect::<String>(),
        );

        let private_key_bytes = Zeroizing::new(
            base64::Engine::decode(&base64::engine::general_purpose::STANDARD, &key_data)
                .map_err(|e| Ed25519CredentialError::InvalidBase64(e.to_string()))?,
        );

        let private_key_info = PrivateKeyInfoRef::try_from(private_key_bytes.as_slice())
            .map_err(|_| Ed25519CredentialError::NotEd25519)?;

        // Validate the public-key shape before the decoder copies the private seed
        if private_key_info
            .public_key
            .is_some_and(|key| key.has_unused_bits() || key.raw_bytes().len() != PUBLIC_KEY_LENGTH)
        {
            return Err(Ed25519CredentialError::NotEd25519);
        }

        let mut keypair = KeypairBytes::try_from(private_key_info)
            .map_err(|_| Ed25519CredentialError::NotEd25519)?;
        let signing_key = SigningKey::try_from(&keypair);

        // The dependency's KeypairBytes does not enable zeroization by default
        keypair.secret_key.zeroize();
        let signing_key = signing_key.map_err(|_| Ed25519CredentialError::NotEd25519)?;

        Ok(Self {
            api_key: api_key.into_inner().into_boxed_str(),
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
    /// The decoded key is not a valid Ed25519 PKCS#8 private key.
    NotEd25519,
    /// The private key is not 32 bytes.
    InvalidKeyLength,
}

impl Display for Ed25519CredentialError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidBase64(e) => write!(f, "Invalid base64 encoding: {e}"),
            Self::NotEd25519 => write!(f, "Decoded key is not a valid Ed25519 PKCS#8 private key"),
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
/// a valid Ed25519 PKCS#8 private key, Ed25519 is used.
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
        let api_key = SecretString::from(api_key);
        let api_secret = SecretString::from(api_secret);

        match Ed25519Credential::new(api_key.clone(), api_secret.clone()) {
            Ok(ed25519) => {
                log::debug!("Auto-detected Ed25519 API key");
                Self::Ed25519(Box::new(ed25519))
            }
            Err(_) => {
                if api_secret.expose_secret().contains("-----BEGIN ") {
                    log::warn!(
                        "PEM credential is not a valid Ed25519 PKCS#8 private key, falling back to HMAC SHA256"
                    );
                }

                log::debug!("Using HMAC SHA256 API key");
                Self::Hmac(Credential::new(
                    api_key.into_inner(),
                    api_secret.into_inner(),
                ))
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
        let key_bytes = Zeroizing::new(self.signing_key.to_bytes());
        Self {
            api_key: self.api_key.clone(),
            signing_key: SigningKey::from_bytes(&key_bytes),
        }
    }
}

/// Builds the canonical query string that Binance's WebSocket API signs.
///
/// The WS API verifies a request's signature over its parameters **sorted by
/// key**, so the signed query string must be key-sorted regardless of the order
/// the parameters happen to iterate in. `serde_json`'s object backend is a
/// sorted `BTreeMap` by default, but it silently becomes an insertion-ordered
/// `IndexMap` if *any* crate anywhere in the build graph enables
/// `serde_json/preserve_order` - a global Cargo feature-unification effect the
/// adapter cannot control (e.g. a transitive `mongodb`/`bson` dependency).
/// Signing the object in its raw iteration order therefore breaks intermittently
/// with `-1022 Signature for this request is not valid` depending on unrelated
/// dependencies. Sorting the keys here makes WS signing independent of that
/// ambient feature.
///
/// This is not needed for the REST/HTTP path, which signs the exact query string
/// it sends (Binance verifies REST signatures over the received order); only the
/// WS API re-sorts before verifying.
///
/// See <https://github.com/nautechsystems/nautilus_trader/issues/4410>.
pub(crate) fn canonical_ws_query_string<'a, I>(
    params: I,
) -> Result<String, serde_urlencoded::ser::Error>
where
    I: IntoIterator<Item = (&'a str, &'a serde_json::Value)>,
{
    // Collecting into a `BTreeMap` sorts by key regardless of the source order,
    // and `serde_urlencoded` percent-encodes each value exactly as it would the
    // original `serde_json::Value`.
    let sorted: std::collections::BTreeMap<&str, &serde_json::Value> = params.into_iter().collect();
    serde_urlencoded::to_string(sorted)
}

#[cfg(test)]
mod tests {
    use ed25519_dalek::VerifyingKey;
    use rstest::rstest;

    use super::*;

    // Official Binance test vectors from:
    // https://github.com/binance/binance-signature-examples
    const BINANCE_TEST_SECRET: &str =
        "NhqPtmdSJYdKjVHjA7PZj4Mge3R5YNiP1e3UZjInClVN65XAbvqqM6A7H5fATj0j";
    const PKCS8_PEM_LABEL: &str = "PRIVATE KEY";

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
    fn test_canonical_ws_query_string_is_key_sorted_regardless_of_input_order() {
        // Parameters in a deliberately non-alphabetical order - exactly what a
        // `serde_json/preserve_order` (IndexMap) build yields, and what broke WS
        // signing with -1022 (issue #4410). Binance verifies the WS signature
        // over the *sorted* parameters, so the query string must come out
        // key-sorted whatever order the caller supplied them in.
        let symbol = serde_json::json!("LTCBTC");
        let side = serde_json::json!("BUY");
        let quantity = serde_json::json!("1");
        let timestamp = serde_json::json!(1_499_827_319_559_i64);
        let api_key = serde_json::json!("mykey");
        let unsorted = [
            ("symbol", &symbol),
            ("side", &side),
            ("quantity", &quantity),
            ("timestamp", &timestamp),
            ("apiKey", &api_key),
        ];

        let query = canonical_ws_query_string(unsorted).unwrap();

        assert_eq!(
            query,
            "apiKey=mykey&quantity=1&side=BUY&symbol=LTCBTC&timestamp=1499827319559"
        );
    }

    #[rstest]
    fn test_canonical_ws_query_string_preserves_urlencoding() {
        let symbol = serde_json::json!("LTCBTC");
        let new_client_order_id = serde_json::json!("desk alpha");
        let unsorted = [
            ("symbol", &symbol),
            ("newClientOrderId", &new_client_order_id),
        ];

        let query = canonical_ws_query_string(unsorted).unwrap();

        assert_eq!(query, "newClientOrderId=desk+alpha&symbol=LTCBTC");
    }

    #[rstest]
    fn test_debug_redacts_secret() {
        let cred = Credential::new("test_key".to_string(), BINANCE_TEST_SECRET.to_string());
        let dbg_out = format!("{cred:?}");

        assert_eq!(dbg_out.matches(REDACTED).count(), 2);
        assert!(!dbg_out.contains("test_key"));
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
    fn test_ed25519_matches_rfc_8032_vector() {
        let key_b64 = base64::Engine::encode(
            &base64::engine::general_purpose::STANDARD,
            ED25519_PKCS8_TEST_VECTOR,
        );

        let cred = Ed25519Credential::new("test_key".into(), key_b64.into()).unwrap();

        let signature = cred.sign(b"");

        assert_eq!(
            signature,
            "5VZDAMNgrHKQhuLMgG6CioSHfx645dl02HPgZSJJAVVfuIIVkKM7rMYeOXAc+bRr0lv18FlbviRlUUFDjnoQCw=="
        );
    }

    #[rstest]
    #[case::first_key(
        "9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60",
        "d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a"
    )]
    #[case::second_key(
        "4ccd089b28ff96da9db6c346ec114e0f5b8a319f35aba624da8cf6ed4fb8a6fb",
        "3d4017c3e843895a92b70aa74d1b7ebc9c982ccf2ec4968cc0cd55f12af4660c"
    )]
    fn test_ed25519_pkcs8_signs_with_expected_key(
        #[case] seed_hex: &str,
        #[case] public_key_hex: &str,
        #[values(false, true)] include_public_key: bool,
        #[values(false, true)] pem: bool,
    ) {
        let public_key = hex::decode(public_key_hex).unwrap();
        let mut der = ED25519_PKCS8_TEST_VECTOR.to_vec();
        der[16..].copy_from_slice(&hex::decode(seed_hex).unwrap());
        if include_public_key {
            der[1] += 35;
            der[4] = 1;
            der.extend_from_slice(&[0x81, 0x21, 0x00]);
            der.extend_from_slice(&public_key);
        }

        let key_b64 = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &der);
        let secret = if pem {
            let (first_line, remaining) = key_b64.split_at(64);
            format!(
                "-----BEGIN {PKCS8_PEM_LABEL}-----\r\n{first_line}\r\n{remaining}\r\n-----END {PKCS8_PEM_LABEL}-----"
            )
        } else {
            key_b64
        };

        let cred = SigningCredential::new("test_key".into(), secret);
        let message = "symbol=BTCUSDT&timestamp=123456789";

        let signature_b64 = cred.sign(message);
        let signature_bytes =
            base64::Engine::decode(&base64::engine::general_purpose::STANDARD, signature_b64)
                .unwrap();
        let signature = Signature::from_slice(&signature_bytes).unwrap();
        let verifying_key = VerifyingKey::try_from(public_key.as_slice()).unwrap();
        let tail_seed: [u8; 32] = der[der.len() - 32..].try_into().unwrap();
        let wrong_key = SigningKey::from_bytes(&tail_seed).verifying_key();

        assert!(cred.is_ed25519());
        assert_eq!(cred.api_key(), "test_key");
        verifying_key
            .verify_strict(message.as_bytes(), &signature)
            .unwrap();

        if include_public_key {
            assert!(
                wrong_key
                    .verify_strict(message.as_bytes(), &signature)
                    .is_err()
            );
        }
    }

    #[rstest]
    #[case::empty(vec![])]
    #[case::oid_only(vec![0x06, 0x03, 0x2b, 0x65, 0x70])]
    #[case::oid_in_arbitrary_blob([&[0x06, 0x03, 0x2b, 0x65, 0x70][..], &[0xab; 43]].concat())]
    #[case::truncated(ED25519_PKCS8_TEST_VECTOR[..47].to_vec())]
    #[case::trailing_byte([&ED25519_PKCS8_TEST_VECTOR[..], &[0x00]].concat())]
    #[case::trailing_seed([&ED25519_PKCS8_TEST_VECTOR[..], &[0xab; 32]].concat())]
    fn test_ed25519_rejects_invalid_pkcs8(#[case] der: Vec<u8>) {
        let secret = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, der);

        let result = Ed25519Credential::new("test_key".into(), secret.clone().into());
        let autodetected = SigningCredential::new("test_key".into(), secret.clone());
        let hmac = Credential::new("test_key".into(), secret);

        assert!(matches!(result, Err(Ed25519CredentialError::NotEd25519)));
        assert!(!autodetected.is_ed25519());
        assert_eq!(autodetected.sign("message"), hmac.sign("message"));
    }

    #[rstest]
    #[case::wrong_outer_tag(0, 0x31)]
    #[case::wrong_outer_length(1, 0x2d)]
    #[case::invalid_version(4, 0x02)]
    #[case::wrong_algorithm(11, 0x71)]
    #[case::wrong_inner_tag(14, 0x05)]
    #[case::wrong_seed_length(15, 0x1f)]
    fn test_ed25519_rejects_malformed_pkcs8(#[case] index: usize, #[case] byte: u8) {
        let mut der = ED25519_PKCS8_TEST_VECTOR;
        der[index] = byte;
        der[16..21].copy_from_slice(&[0x06, 0x03, 0x2b, 0x65, 0x70]);
        let secret = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, der);

        let result = Ed25519Credential::new("test_key".into(), secret.into());

        assert!(matches!(result, Err(Ed25519CredentialError::NotEd25519)));
    }

    #[rstest]
    fn test_ed25519_rejects_mismatched_public_key() {
        let mut der = ED25519_PKCS8_TEST_VECTOR.to_vec();
        der[1] += 35;
        der[4] = 1;
        der.extend_from_slice(&[0x81, 0x21, 0x00]);
        let other_public_key = SigningKey::from_bytes(&[0xab; 32]).verifying_key();
        der.extend_from_slice(other_public_key.as_bytes());
        let secret = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, der);

        let result = Ed25519Credential::new("test_key".into(), secret.into());

        assert!(matches!(result, Err(Ed25519CredentialError::NotEd25519)));
    }

    #[rstest]
    #[case::attributes(&[0xa0, 0x00])]
    #[case::extension(&[0xa2, 0x03, 0x04, 0x01, 0x7f])]
    fn test_ed25519_pkcs8_extensions_preserve_seed(#[case] extra: &[u8]) {
        let mut der = ED25519_PKCS8_TEST_VECTOR.to_vec();
        der.extend_from_slice(extra);
        der[1] = u8::try_from(der.len() - 2).unwrap();
        let secret = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &der);
        let cred = Ed25519Credential::new("test_key".into(), secret.into()).unwrap();

        let signature = cred.sign(b"");

        assert_eq!(
            signature,
            "5VZDAMNgrHKQhuLMgG6CioSHfx645dl02HPgZSJJAVVfuIIVkKM7rMYeOXAc+bRr0lv18FlbviRlUUFDjnoQCw=="
        );
    }

    #[rstest]
    #[case::missing_public_key(false, 1)]
    #[case::unexpected_public_key(true, 0)]
    fn test_ed25519_rejects_pkcs8_version_mismatch(
        #[case] include_public_key: bool,
        #[case] version: u8,
    ) {
        let mut der = ED25519_PKCS8_TEST_VECTOR.to_vec();
        der[4] = version;

        if include_public_key {
            let public_key =
                hex::decode("d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a")
                    .unwrap();
            der.extend_from_slice(&[0x81, 0x21, 0x00]);
            der.extend_from_slice(&public_key);
            der[1] = u8::try_from(der.len() - 2).unwrap();
        }

        let secret = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &der);

        let result = Ed25519Credential::new("test_key".into(), secret.into());

        assert!(matches!(result, Err(Ed25519CredentialError::NotEd25519)));
    }

    #[rstest]
    #[case::short_public_key(&[0x81, 0x20, 0x00], 31)]
    #[case::explicit_public_key(&[0xa1, 0x23, 0x03, 0x21, 0x00], 32)]
    #[case::non_byte_aligned_public_key(&[0x81, 0x21, 0x01], 32)]
    fn test_ed25519_rejects_pkcs8_public_key_encoding(
        #[case] prefix: &[u8],
        #[case] public_key_len: usize,
    ) {
        let mut der = ED25519_PKCS8_TEST_VECTOR.to_vec();
        der[4] = 1;
        let public_key =
            hex::decode("d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a")
                .unwrap();
        der.extend_from_slice(prefix);
        der.extend_from_slice(&public_key[..public_key_len]);
        der[1] = u8::try_from(der.len() - 2).unwrap();
        let secret = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &der);

        let result = Ed25519Credential::new("test_key".into(), secret.into());

        assert!(matches!(result, Err(Ed25519CredentialError::NotEd25519)));
    }

    #[rstest]
    fn test_ed25519_rejects_pkcs8_algorithm_parameters() {
        let mut der = ED25519_PKCS8_TEST_VECTOR.to_vec();
        der[1] += 2;
        der[6] += 2;
        der.splice(12..12, [0x05, 0x00]);
        let secret = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &der);
        let pem =
            format!("-----BEGIN {PKCS8_PEM_LABEL}-----\n{secret}\n-----END {PKCS8_PEM_LABEL}-----");

        let result = Ed25519Credential::new("test_key".into(), secret.into());
        let autodetected = SigningCredential::new("test_key".into(), pem.clone());
        let hmac = Credential::new("test_key".into(), pem);

        assert!(matches!(result, Err(Ed25519CredentialError::NotEd25519)));
        assert!(!autodetected.is_ed25519());
        assert_eq!(autodetected.sign("message"), hmac.sign("message"));
    }

    #[rstest]
    fn test_ed25519_rejects_raw_32_byte_seed() {
        // Accepting raw seeds would silently misclassify base64 HMAC secrets as Ed25519
        let seed =
            base64::Engine::encode(&base64::engine::general_purpose::STANDARD, [0xab_u8; 32]);

        let result = Ed25519Credential::new("test_key".into(), seed.into());

        assert!(matches!(result, Err(Ed25519CredentialError::NotEd25519)));
    }

    #[rstest]
    fn test_ed25519_rejects_binance_hmac_secret() {
        // Binance HMAC secrets can decode to the same length as a PKCS#8 key
        let result = Ed25519Credential::new("test_key".into(), BINANCE_TEST_SECRET.into());

        assert!(matches!(result, Err(Ed25519CredentialError::NotEd25519)));
    }

    #[rstest]
    fn test_signing_credential_autodetect_falls_back_to_hmac_on_binance_secret() {
        let cred = SigningCredential::new("test_key".to_string(), BINANCE_TEST_SECRET.to_string());

        assert!(matches!(cred, SigningCredential::Hmac(_)));
    }

    #[rstest]
    fn test_ed25519_debug_redacts_secret() {
        let key_b64 = base64::Engine::encode(
            &base64::engine::general_purpose::STANDARD,
            ED25519_PKCS8_TEST_VECTOR,
        );

        let cred = Ed25519Credential::new("test_key".into(), key_b64.clone().into()).unwrap();
        let dbg_out = format!("{cred:?}");

        assert_eq!(dbg_out.matches(REDACTED).count(), 2);
        assert!(!dbg_out.contains("test_key"));
        assert!(!dbg_out.contains(&key_b64));
    }
}
