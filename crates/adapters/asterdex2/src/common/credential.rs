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

//! Asterdex authentication and credential management.

use anyhow::{anyhow, Result};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use zeroize::Zeroizing;

type HmacSha256 = Hmac<Sha256>;

/// Asterdex API credentials for authentication.
#[derive(Clone, Debug)]
pub struct AsterdexCredentials {
    api_key: Arc<Zeroizing<String>>,
    api_secret: Arc<Zeroizing<String>>,
}

impl AsterdexCredentials {
    /// Creates new Asterdex credentials.
    pub fn new(api_key: String, api_secret: String) -> Result<Self> {
        if api_key.is_empty() {
            return Err(anyhow!("API key cannot be empty"));
        }
        if api_secret.is_empty() {
            return Err(anyhow!("API secret cannot be empty"));
        }

        Ok(Self {
            api_key: Arc::new(Zeroizing::new(api_key)),
            api_secret: Arc::new(Zeroizing::new(api_secret)),
        })
    }

    /// Returns the API key.
    #[must_use]
    pub fn api_key(&self) -> &str {
        &self.api_key
    }

    /// Returns the API secret.
    #[must_use]
    pub fn api_secret(&self) -> &str {
        &self.api_secret
    }

    /// Generates authentication signature for Asterdex API.
    ///
    /// Asterdex uses HMAC-SHA256:
    /// signature = HMAC-SHA256(secret, query_string_or_body)
    ///
    /// # Arguments
    ///
    /// * `params` - Query string or request body parameters
    ///
    /// # Returns
    ///
    /// Hex-encoded signature string
    pub fn sign_request(&self, params: &str) -> String {
        let mut mac = HmacSha256::new_from_slice(self.api_secret.as_bytes())
            .expect("HMAC can take key of any size");
        mac.update(params.as_bytes());
        hex::encode(mac.finalize().into_bytes())
    }

    /// Gets current timestamp in milliseconds.
    pub fn get_timestamp() -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("Time went backwards")
            .as_millis() as i64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_credentials_creation() {
        let creds = AsterdexCredentials::new("test_key".to_string(), "test_secret".to_string());
        assert!(creds.is_ok());

        let creds = creds.unwrap();
        assert_eq!(creds.api_key(), "test_key");
        assert_eq!(creds.api_secret(), "test_secret");
    }

    #[test]
    fn test_empty_api_key() {
        let creds = AsterdexCredentials::new("".to_string(), "test_secret".to_string());
        assert!(creds.is_err());
    }

    #[test]
    fn test_empty_api_secret() {
        let creds = AsterdexCredentials::new("test_key".to_string(), "".to_string());
        assert!(creds.is_err());
    }

    #[test]
    fn test_sign_request() {
        let creds =
            AsterdexCredentials::new("test_key".to_string(), "test_secret".to_string()).unwrap();
        let signature = creds.sign_request("symbol=BTCUSDT&side=BUY&type=LIMIT");

        assert!(!signature.is_empty());
        // Signature should be 64 characters (256 bits in hex)
        assert_eq!(signature.len(), 64);
    }

    #[test]
    fn test_timestamp() {
        let timestamp = AsterdexCredentials::get_timestamp();
        assert!(timestamp > 0);
    }
}
