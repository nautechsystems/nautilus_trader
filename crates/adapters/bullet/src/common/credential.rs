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

//! Ed25519 credential loading for the Bullet adapter.
//!
//! Bullet uses ed25519 keys (Solana-compatible). The canonical API setup uses a *delegate* key —
//! a separate keypair registered to trade on behalf of a main account. Read endpoints use the
//! main account address; signing uses the delegate private key.
//!
//! Key resolution order (first wins):
//! 1. `key_file` config field → JSON keystore path
//! 2. `BULLET_KEY_FILE` environment variable
//! 3. `BULLET_PRIVATE_KEY` environment variable (hex)
//! 4. `private_key` config field (hex)

use std::path::Path;

use ed25519_dalek::SigningKey;
use zeroize::Zeroize;

use crate::common::error::BulletError;

/// Ed25519 keypair for signing Bullet transactions.
///
/// The private key bytes are zeroized on drop.
pub struct BulletCredential {
    signing_key: SigningKey,
}

impl std::fmt::Debug for BulletCredential {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BulletCredential")
            .field("address", &self.address())
            .finish_non_exhaustive()
    }
}

impl BulletCredential {
    /// Load from a 32-byte hex-encoded secret key (with or without `0x` prefix).
    ///
    /// # Errors
    ///
    /// Returns an error if the hex string is not 32 bytes.
    pub fn from_hex(hex: &str) -> Result<Self, BulletError> {
        let hex = hex.strip_prefix("0x").unwrap_or(hex);
        let bytes = ::hex::decode(hex).map_err(|e| {
            BulletError::Credential(format!("invalid hex private key: {e}"))
        })?;
        let secret: [u8; 32] = bytes.try_into().map_err(|_| {
            BulletError::Credential("private key must be exactly 32 bytes".to_string())
        })?;
        Ok(Self {
            signing_key: SigningKey::from_bytes(&secret),
        })
    }

    /// Load from a base58-encoded key.
    ///
    /// Accepts either:
    /// - 64-byte Solana-format keypair (`[secret(32) || public(32)]`) — e.g. from `solana-keygen`
    ///   or a Bullet key export. The first 32 bytes are used as the secret.
    /// - 32-byte raw secret key.
    ///
    /// # Errors
    ///
    /// Returns an error if the string is not valid base58 or the decoded length is not 32 or 64.
    pub fn from_base58(s: &str) -> Result<Self, BulletError> {
        let bytes = bs58::decode(s).into_vec().map_err(|e| {
            BulletError::Credential(format!("invalid base58 private key: {e}"))
        })?;
        let secret: [u8; 32] = match bytes.len() {
            64 => bytes[..32].try_into().expect("slice is 32 bytes"),
            32 => bytes.try_into().expect("slice is 32 bytes"),
            n => {
                return Err(BulletError::Credential(format!(
                    "base58 key must decode to 32 or 64 bytes, got {n}"
                )));
            }
        };
        Ok(Self {
            signing_key: SigningKey::from_bytes(&secret),
        })
    }

    /// Load from a string that may be hex or base58.
    ///
    /// Tries hex first; if that fails, tries base58.
    ///
    /// # Errors
    ///
    /// Returns an error if neither format is valid.
    pub fn from_str(s: &str) -> Result<Self, BulletError> {
        let stripped = s.strip_prefix("0x").unwrap_or(s);
        // 64 hex chars = 32 bytes; 128 hex chars = 64 bytes (less common)
        if stripped.len() == 64 && stripped.chars().all(|c| c.is_ascii_hexdigit()) {
            return Self::from_hex(s);
        }
        Self::from_base58(s)
    }

    /// Load from a Solana-compatible JSON keystore file.
    ///
    /// Format: JSON array of 64 bytes — `[secret(32) || public(32)]` — as produced by
    /// `solana-keygen new` or the Bullet `keygen` command.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be read or the format is invalid.
    pub fn from_file(path: impl AsRef<Path>) -> Result<Self, BulletError> {
        let path = path.as_ref();
        let data = std::fs::read_to_string(path).map_err(|e| {
            BulletError::Credential(format!("cannot read key file {}: {e}", path.display()))
        })?;
        let mut bytes: Vec<u8> = serde_json::from_str(&data).map_err(|e| {
            BulletError::Credential(format!("invalid key file {}: {e}", path.display()))
        })?;
        if bytes.len() < 32 {
            return Err(BulletError::Credential(format!(
                "key file {} too short: {} bytes (need ≥32)",
                path.display(),
                bytes.len()
            )));
        }
        let mut secret = [0u8; 32];
        secret.copy_from_slice(&bytes[..32]);
        bytes.zeroize();
        Ok(Self {
            signing_key: SigningKey::from_bytes(&secret),
        })
    }

    /// Resolve credential from config fields and environment variables.
    ///
    /// Priority: `key_file` config → `BULLET_KEY_FILE` env → `BULLET_PRIVATE_KEY` env →
    /// `private_key` config.
    ///
    /// # Errors
    ///
    /// Returns an error if no valid credential source is found or any source is malformed.
    pub fn resolve(
        private_key: Option<&str>,
        key_file: Option<&str>,
    ) -> Result<Self, BulletError> {
        // 1. key_file config
        if let Some(path) = key_file {
            return Self::from_file(path);
        }
        // 2. BULLET_KEY_FILE env
        if let Ok(path) = std::env::var("BULLET_KEY_FILE") {
            return Self::from_file(&path);
        }
        // 3. BULLET_PRIVATE_KEY env (hex or base58)
        if let Ok(s) = std::env::var("BULLET_PRIVATE_KEY") {
            return Self::from_str(&s);
        }
        // 4. private_key config (hex or base58)
        if let Some(s) = private_key {
            return Self::from_str(s);
        }
        Err(BulletError::Credential(
            "no private key configured (set BULLET_PRIVATE_KEY, BULLET_KEY_FILE, or config fields)"
                .to_string(),
        ))
    }

    /// Sign a message and return the 64-byte Ed25519 signature.
    #[must_use]
    pub fn sign(&self, message: &[u8]) -> [u8; 64] {
        use ed25519_dalek::Signer;
        self.signing_key.sign(message).to_bytes()
    }

    /// Return the 32-byte compressed public key.
    #[must_use]
    pub fn public_key(&self) -> [u8; 32] {
        self.signing_key.verifying_key().to_bytes()
    }

    /// Return the on-chain address (base58-encoded public key).
    ///
    /// This is the canonical Bullet address format.
    #[must_use]
    pub fn address(&self) -> String {
        bullet_exchange_interface::address::Address(self.public_key()).to_string()
    }
}

impl Drop for BulletCredential {
    fn drop(&mut self) {
        // Zeroize the signing key bytes on drop
        let mut key_bytes = self.signing_key.to_bytes();
        key_bytes.zeroize();
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    const TEST_HEX: &str =
        "0000000000000000000000000000000000000000000000000000000000000001";

    #[rstest]
    fn test_from_hex() {
        let cred = BulletCredential::from_hex(TEST_HEX).unwrap();
        assert_eq!(cred.public_key().len(), 32);
        assert!(!cred.address().is_empty());
    }

    #[rstest]
    fn test_from_hex_with_prefix() {
        let hex = format!("0x{TEST_HEX}");
        let cred = BulletCredential::from_hex(&hex).unwrap();
        assert_eq!(cred.public_key().len(), 32);
    }

    #[rstest]
    fn test_from_hex_invalid_length() {
        let err = BulletCredential::from_hex("deadbeef").unwrap_err();
        assert!(err.to_string().contains("32 bytes"));
    }

    #[rstest]
    fn test_sign_produces_64_bytes() {
        let cred = BulletCredential::from_hex(TEST_HEX).unwrap();
        let sig = cred.sign(b"test message");
        assert_eq!(sig.len(), 64);
    }
}
