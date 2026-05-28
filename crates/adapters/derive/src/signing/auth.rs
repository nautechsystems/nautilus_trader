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

//! Derive REST/WebSocket session authentication.
//!
//! Authenticated sessions are built from an EIP-191 `personal_sign` over the
//! current millisecond timestamp string, plus the smart-contract wallet
//! address. The signature is produced by the session key.
//!
//! Pipeline (matching `derive_action_signing/utils.py::sign_rest_auth_header`):
//!
//! 1. Render `timestamp = utc_now_ms().to_string()`.
//! 2. Sign the bytes with EIP-191 `personal_sign(timestamp_bytes,
//!    session_key)`. Alloy's [`SignerSync::sign_message_sync`] applies the
//!    `\x19Ethereum Signed Message:\n<len>` prefix automatically.
//! 3. Send headers `X-LYRAWALLET = wallet`, `X-LYRATIMESTAMP = timestamp`,
//!    `X-LYRASIGNATURE = 0x-prefixed_signature_hex`.
//!
//! WebSocket login mirrors this with a JSON body of `{wallet, timestamp,
//! signature}` instead of headers.

use alloy::signers::{SignerSync, local::PrivateKeySigner};
use thiserror::Error;

use crate::signing::encoding::utc_now_ms;

/// Errors raised while building auth headers.
#[derive(Debug, Error)]
pub enum AuthError {
    /// The system clock is before the UNIX epoch.
    #[error("system clock is before UNIX epoch")]
    ClockBeforeEpoch,
    /// secp256k1 signing failed.
    #[error("signing failed: {message}")]
    SigningFailed {
        /// Signer error message.
        message: String,
    },
}

/// Headers sent with REST requests authenticated against a session key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthHeaders {
    /// Smart-contract wallet address (`X-LYRAWALLET`).
    pub wallet: String,
    /// Millisecond UNIX timestamp string (`X-LYRATIMESTAMP`).
    pub timestamp: String,
    /// 0x-prefixed signature hex (`X-LYRASIGNATURE`).
    pub signature: String,
}

/// Body sent on the WebSocket `public/login` request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WsLogin {
    /// Smart-contract wallet address.
    pub wallet: String,
    /// Millisecond UNIX timestamp string.
    pub timestamp: String,
    /// 0x-prefixed signature hex.
    pub signature: String,
}

/// Builds REST auth headers using the system clock as the reference time.
///
/// # Errors
///
/// Returns [`AuthError::ClockBeforeEpoch`] if the system clock is invalid,
/// or [`AuthError::SigningFailed`] when the underlying secp256k1 signer errors.
pub fn build_rest_auth_headers(
    wallet: &str,
    signer: &PrivateKeySigner,
) -> Result<AuthHeaders, AuthError> {
    let now = utc_now_ms().map_err(|_| AuthError::ClockBeforeEpoch)?;
    build_rest_auth_headers_at(wallet, signer, now)
}

/// Builds REST auth headers with an injected `now_ms` reference, suitable for
/// deterministic testing.
///
/// # Errors
///
/// Returns [`AuthError::SigningFailed`] when the underlying secp256k1 signer
/// errors.
pub fn build_rest_auth_headers_at(
    wallet: &str,
    signer: &PrivateKeySigner,
    now_ms: u64,
) -> Result<AuthHeaders, AuthError> {
    let timestamp = now_ms.to_string();
    let signature = sign_message(&timestamp, signer)?;
    Ok(AuthHeaders {
        wallet: wallet.to_owned(),
        timestamp,
        signature,
    })
}

/// Builds the WebSocket login body using the system clock.
///
/// # Errors
///
/// Returns [`AuthError::ClockBeforeEpoch`] if the system clock is invalid,
/// or [`AuthError::SigningFailed`] when the underlying secp256k1 signer errors.
pub fn build_ws_login(wallet: &str, signer: &PrivateKeySigner) -> Result<WsLogin, AuthError> {
    let now = utc_now_ms().map_err(|_| AuthError::ClockBeforeEpoch)?;
    build_ws_login_at(wallet, signer, now)
}

/// Builds the WebSocket login body with an injected `now_ms` reference.
///
/// # Errors
///
/// Returns [`AuthError::SigningFailed`] when the underlying secp256k1 signer
/// errors.
pub fn build_ws_login_at(
    wallet: &str,
    signer: &PrivateKeySigner,
    now_ms: u64,
) -> Result<WsLogin, AuthError> {
    let timestamp = now_ms.to_string();
    let signature = sign_message(&timestamp, signer)?;
    Ok(WsLogin {
        wallet: wallet.to_owned(),
        timestamp,
        signature,
    })
}

fn sign_message(message: &str, signer: &PrivateKeySigner) -> Result<String, AuthError> {
    let signature =
        signer
            .sign_message_sync(message.as_bytes())
            .map_err(|e| AuthError::SigningFailed {
                message: e.to_string(),
            })?;
    Ok(format!(
        "0x{}",
        alloy_primitives::hex::encode(signature.as_bytes())
    ))
}

#[cfg(test)]
mod tests {
    use alloy_primitives::{Address, Signature, eip191_hash_message, hex};
    use rstest::rstest;

    use super::*;

    const SESSION_KEY_HEX: &str =
        "0x2ae8be44db8a590d20bffbe3b6872df9b569147d3bf6801a35a28281a4816bbd";
    const TEST_WALLET: &str = "0x000000000000000000000000000000000000aaaa";

    fn signer_address() -> Address {
        let signer: PrivateKeySigner = SESSION_KEY_HEX.parse().unwrap();
        signer.address()
    }

    #[rstest]
    fn test_rest_headers_contain_three_fields() {
        let signer: PrivateKeySigner = SESSION_KEY_HEX.parse().unwrap();
        let headers = build_rest_auth_headers_at(TEST_WALLET, &signer, 1_700_000_000_000).unwrap();
        assert_eq!(headers.wallet, TEST_WALLET);
        assert_eq!(headers.timestamp, "1700000000000");
        assert!(headers.signature.starts_with("0x"));
        assert_eq!(headers.signature.len(), 2 + 130);
    }

    #[rstest]
    fn test_rest_signature_recovers_signer_address() {
        let signer: PrivateKeySigner = SESSION_KEY_HEX.parse().unwrap();
        let headers = build_rest_auth_headers_at(TEST_WALLET, &signer, 1_700_000_000_000).unwrap();
        let raw = hex::decode(headers.signature.trim_start_matches("0x")).unwrap();
        let signature = Signature::try_from(raw.as_slice()).unwrap();
        // EIP-191 prefixed hash of the timestamp string is the digest the
        // session key signed; recovery returns the session-key address.
        let digest = eip191_hash_message(headers.timestamp.as_bytes());
        let recovered = signature
            .recover_address_from_prehash(&digest)
            .expect("recover");
        assert_eq!(recovered, signer_address());
    }

    #[rstest]
    fn test_ws_login_matches_rest_signature_for_same_timestamp() {
        let signer: PrivateKeySigner = SESSION_KEY_HEX.parse().unwrap();
        let now = 1_700_000_001_234;
        let rest = build_rest_auth_headers_at(TEST_WALLET, &signer, now).unwrap();
        let ws = build_ws_login_at(TEST_WALLET, &signer, now).unwrap();
        assert_eq!(rest.timestamp, ws.timestamp);
        assert_eq!(rest.signature, ws.signature);
        assert_eq!(rest.wallet, ws.wallet);
    }

    #[rstest]
    fn test_distinct_timestamps_produce_distinct_signatures() {
        let signer: PrivateKeySigner = SESSION_KEY_HEX.parse().unwrap();
        let a = build_rest_auth_headers_at(TEST_WALLET, &signer, 1_700_000_000_000).unwrap();
        let b = build_rest_auth_headers_at(TEST_WALLET, &signer, 1_700_000_000_001).unwrap();
        assert_ne!(a.signature, b.signature);
    }

    #[rstest]
    fn test_signature_format_is_lowercase_hex() {
        let signer: PrivateKeySigner = SESSION_KEY_HEX.parse().unwrap();
        let headers = build_rest_auth_headers_at(TEST_WALLET, &signer, 1_700_000_000_000).unwrap();
        let sig = headers.signature.trim_start_matches("0x");
        assert!(
            sig.chars()
                .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c)),
            "expected lowercase hex, was {sig}",
        );
    }
}
