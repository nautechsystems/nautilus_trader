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

//! Credential and authentication utilities for Lighter.

use std::sync::Arc;

use anyhow::Result;
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use zeroize::Zeroizing;

/// Lighter API credentials.
#[derive(Clone)]
pub struct LighterCredentials {
    /// API key private key.
    api_key_private_key: Arc<Zeroizing<String>>,
    /// Ethereum private key for transaction signing.
    eth_private_key: Arc<Zeroizing<String>>,
    /// API key index (2-254).
    api_key_index: u8,
    /// Account index.
    account_index: u64,
}

impl LighterCredentials {
    /// Creates new Lighter credentials.
    ///
    /// # Arguments
    ///
    /// * `api_key_private_key` - The API key private key
    /// * `eth_private_key` - The Ethereum private key for signing transactions
    /// * `api_key_index` - The API key index (2-254)
    /// * `account_index` - The account index
    ///
    /// # Errors
    ///
    /// Returns an error if the API key index is out of range.
    pub fn new(
        api_key_private_key: String,
        eth_private_key: String,
        api_key_index: u8,
        account_index: u64,
    ) -> Result<Self> {
        if api_key_index < 2 || api_key_index > 254 {
            anyhow::bail!("API key index must be between 2 and 254");
        }

        Ok(Self {
            api_key_private_key: Arc::new(Zeroizing::new(api_key_private_key)),
            eth_private_key: Arc::new(Zeroizing::new(eth_private_key)),
            api_key_index,
            account_index,
        })
    }

    /// Returns the API key private key.
    #[must_use]
    pub fn api_key_private_key(&self) -> &str {
        &self.api_key_private_key
    }

    /// Returns the Ethereum private key.
    #[must_use]
    pub fn eth_private_key(&self) -> &str {
        &self.eth_private_key
    }

    /// Returns the API key index.
    #[must_use]
    pub const fn api_key_index(&self) -> u8 {
        self.api_key_index
    }

    /// Returns the account index.
    #[must_use]
    pub const fn account_index(&self) -> u64 {
        self.account_index
    }

    /// Creates an authentication token with expiry.
    ///
    /// # Arguments
    ///
    /// * `expiry_seconds` - Token expiry time in seconds
    ///
    /// # Errors
    ///
    /// Returns an error if token generation fails.
    pub fn create_auth_token(&self, expiry_seconds: u64) -> Result<String> {
        // This is a placeholder implementation
        // In production, this should use the Lighter SDK's authentication mechanism
        // or implement the proper signing algorithm

        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs();

        let expiry = timestamp + expiry_seconds;

        // Placeholder: In reality, this would sign a message with the private key
        let message = format!(
            "{}:{}:{}:{}",
            self.account_index, self.api_key_index, timestamp, expiry
        );

        Ok(BASE64.encode(message.as_bytes()))
    }
}

impl std::fmt::Debug for LighterCredentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LighterCredentials")
            .field("api_key_index", &self.api_key_index)
            .field("account_index", &self.account_index)
            .field("api_key_private_key", &"***REDACTED***")
            .field("eth_private_key", &"***REDACTED***")
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_credentials_creation() {
        let creds = LighterCredentials::new(
            "test_api_key".to_string(),
            "test_eth_key".to_string(),
            10,
            1,
        );
        assert!(creds.is_ok());
    }

    #[test]
    fn test_invalid_api_key_index() {
        let creds = LighterCredentials::new(
            "test_api_key".to_string(),
            "test_eth_key".to_string(),
            1, // Invalid: must be >= 2
            1,
        );
        assert!(creds.is_err());
    }
}
