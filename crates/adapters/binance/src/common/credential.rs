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
//! - [`Ed25519Credential`]: Ed25519 signing for SBE market data streams

#![allow(unused_assignments)] // Fields are used in methods; false positive on some toolchains

use std::fmt::{Debug, Display};

use aws_lc_rs::hmac;
use ed25519_dalek::{Signature, Signer, SigningKey};
use ustr::Ustr;
use zeroize::ZeroizeOnDrop;

use super::enums::{BinanceEnvironment, BinanceProductType};

/// Resolves API credentials from config or environment variables.
///
/// For live environments, uses shared env vars:
/// - `BINANCE_API_KEY`
/// - `BINANCE_API_SECRET`
///
/// For testnet environments, uses product-specific env vars:
/// - Spot: `BINANCE_TESTNET_API_KEY` / `BINANCE_TESTNET_API_SECRET`
/// - Futures: `BINANCE_FUTURES_TESTNET_API_KEY` / `BINANCE_FUTURES_TESTNET_API_SECRET`
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
    let (key_var, secret_var) = match environment {
        BinanceEnvironment::Testnet => match product_type {
            BinanceProductType::Spot | BinanceProductType::Margin | BinanceProductType::Options => {
                ("BINANCE_TESTNET_API_KEY", "BINANCE_TESTNET_API_SECRET")
            }
            BinanceProductType::UsdM | BinanceProductType::CoinM => (
                "BINANCE_FUTURES_TESTNET_API_KEY",
                "BINANCE_FUTURES_TESTNET_API_SECRET",
            ),
        },
        BinanceEnvironment::Mainnet => ("BINANCE_API_KEY", "BINANCE_API_SECRET"),
    };

    let api_key = config_api_key
        .or_else(|| std::env::var(key_var).ok())
        .ok_or_else(|| anyhow::anyhow!("{key_var} not found in config or environment"))?;

    let api_secret = config_api_secret
        .or_else(|| std::env::var(secret_var).ok())
        .ok_or_else(|| anyhow::anyhow!("{secret_var} not found in config or environment"))?;

    Ok((api_key, api_secret))
}

/// Resolves optional Ed25519 credentials from config or environment variables.
///
/// Ed25519 credentials are used for SBE market data streams and are optional.
///
/// For live environments:
/// - `BINANCE_ED25519_API_KEY` / `BINANCE_ED25519_API_SECRET`
///
/// For testnet environments:
/// - Spot: `BINANCE_TESTNET_ED25519_API_KEY` / `BINANCE_TESTNET_ED25519_API_SECRET`
/// - Futures: `BINANCE_FUTURES_TESTNET_ED25519_API_KEY` / `BINANCE_FUTURES_TESTNET_ED25519_API_SECRET`
///
/// Returns `None` if credentials are not configured.
#[must_use]
pub fn resolve_ed25519_credentials(
    config_api_key: Option<String>,
    config_api_secret: Option<String>,
    environment: BinanceEnvironment,
    product_type: BinanceProductType,
) -> Option<(String, String)> {
    let (key_var, secret_var) = match environment {
        BinanceEnvironment::Testnet => match product_type {
            BinanceProductType::Spot | BinanceProductType::Margin | BinanceProductType::Options => {
                (
                    "BINANCE_TESTNET_ED25519_API_KEY",
                    "BINANCE_TESTNET_ED25519_API_SECRET",
                )
            }
            BinanceProductType::UsdM | BinanceProductType::CoinM => (
                "BINANCE_FUTURES_TESTNET_ED25519_API_KEY",
                "BINANCE_FUTURES_TESTNET_ED25519_API_SECRET",
            ),
        },
        BinanceEnvironment::Mainnet => ("BINANCE_ED25519_API_KEY", "BINANCE_ED25519_API_SECRET"), // gitleaks:allow
    };

    let api_key = config_api_key.or_else(|| std::env::var(key_var).ok())?;
    let api_secret = config_api_secret.or_else(|| std::env::var(secret_var).ok())?;

    Some((api_key, api_secret))
}

/// Binance API credentials for signing requests (HMAC SHA256).
///
/// Uses HMAC SHA256 with hexadecimal encoding, as required by Binance REST API signing.
#[derive(Clone, ZeroizeOnDrop)]
pub struct Credential {
    #[zeroize(skip)]
    pub api_key: Ustr,
    api_secret: Box<[u8]>,
}

/// Binance Ed25519 credentials for SBE market data streams.
///
/// SBE market data streams at `stream-sbe.binance.com` require Ed25519 API key
/// authentication via the `X-MBX-APIKEY` header.
#[derive(ZeroizeOnDrop)]
pub struct Ed25519Credential {
    #[zeroize(skip)]
    pub api_key: Ustr,
    signing_key: SigningKey,
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

    /// Returns the API key.
    #[must_use]
    pub fn api_key(&self) -> &str {
        self.api_key.as_str()
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
            .field("signing_key", &"<redacted>")
            .finish()
    }
}

impl Ed25519Credential {
    /// Creates a new [`Ed25519Credential`] from API key and base64-encoded private key.
    ///
    /// The private key can be provided as:
    /// - Raw 32-byte seed (base64 encoded)
    /// - PKCS#8 DER format (48 bytes, as generated by OpenSSL)
    /// - PEM format (with or without headers)
    ///
    /// For PKCS#8/PEM format, the 32-byte seed is extracted from the last 32 bytes.
    ///
    /// # Errors
    ///
    /// Returns an error if the private key is not valid base64 or not a valid
    /// Ed25519 private key.
    pub fn new(api_key: String, private_key_base64: &str) -> Result<Self, Ed25519CredentialError> {
        // Strip PEM headers/footers if present
        let key_data: String = private_key_base64
            .lines()
            .filter(|line| !line.starts_with("-----"))
            .collect();

        let private_key_bytes =
            base64::Engine::decode(&base64::engine::general_purpose::STANDARD, &key_data)
                .map_err(|e| Ed25519CredentialError::InvalidBase64(e.to_string()))?;

        // Extract 32-byte seed: works for both raw (32 bytes) and PKCS#8 (48 bytes)
        if private_key_bytes.len() < 32 {
            return Err(Ed25519CredentialError::InvalidKeyLength);
        }
        let seed_start = private_key_bytes.len() - 32;
        let key_bytes: [u8; 32] = private_key_bytes[seed_start..]
            .try_into()
            .map_err(|_| Ed25519CredentialError::InvalidKeyLength)?;

        let signing_key = SigningKey::from_bytes(&key_bytes);

        Ok(Self {
            api_key: api_key.into(),
            signing_key,
        })
    }

    /// Returns the API key.
    #[must_use]
    pub fn api_key(&self) -> &str {
        self.api_key.as_str()
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
    /// The private key is not 32 bytes.
    InvalidKeyLength,
}

impl Display for Ed25519CredentialError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidBase64(e) => write!(f, "Invalid base64 encoding: {e}"),
            Self::InvalidKeyLength => write!(f, "Ed25519 private key must be 32 bytes"),
        }
    }
}

impl std::error::Error for Ed25519CredentialError {}

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
}
