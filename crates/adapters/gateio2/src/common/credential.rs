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

//! Gate.io authentication and credential management.

use anyhow::{anyhow, Result};
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha512};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use zeroize::Zeroizing;

type HmacSha512 = Hmac<Sha512>;

/// Gate.io API credentials for authentication.
#[derive(Clone, Debug)]
pub struct GateioCredentials {
    api_key: Arc<Zeroizing<String>>,
    api_secret: Arc<Zeroizing<String>>,
}

impl GateioCredentials {
    /// Creates new Gate.io credentials.
    ///
    /// # Arguments
    ///
    /// * `api_key` - The API key from Gate.io
    /// * `api_secret` - The API secret from Gate.io
    ///
    /// # Errors
    ///
    /// Returns an error if the API key or secret is empty.
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

    /// Generates authentication signature for Gate.io API v4.
    ///
    /// The signature is generated using HMAC-SHA512:
    /// Sign = HMAC-SHA512(secret, payload)
    /// where payload = method + "\n" + url_path + "\n" + query_string + "\n" + hashed_payload + "\n" + timestamp
    ///
    /// # Arguments
    ///
    /// * `method` - HTTP method (GET, POST, etc.)
    /// * `url_path` - API endpoint path
    /// * `query_string` - URL query string (empty if none)
    /// * `body` - Request body (empty if none)
    ///
    /// # Returns
    ///
    /// Tuple of (signature, timestamp) where timestamp is current Unix epoch seconds
    pub fn sign_request(
        &self,
        method: &str,
        url_path: &str,
        query_string: &str,
        body: &str,
    ) -> (String, i64) {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("Time went backwards")
            .as_secs() as i64;

        // Hash the payload using SHA512
        let hashed_payload = if body.is_empty() {
            hex::encode(sha2::Sha512::digest(b""))
        } else {
            hex::encode(sha2::Sha512::digest(body.as_bytes()))
        };

        // Build the string to sign
        let payload = format!(
            "{}\n{}\n{}\n{}\n{}",
            method.to_uppercase(),
            url_path,
            query_string,
            hashed_payload,
            timestamp
        );

        // Generate HMAC-SHA512 signature
        let mut mac = HmacSha512::new_from_slice(self.api_secret.as_bytes())
            .expect("HMAC can take key of any size");
        mac.update(payload.as_bytes());
        let signature = hex::encode(mac.finalize().into_bytes());

        (signature, timestamp)
    }

    /// Generates authentication signature for WebSocket subscription.
    ///
    /// WebSocket signature format:
    /// Sign = HMAC-SHA512(secret, "channel=" + channel + "&event=" + event + "&time=" + timestamp)
    ///
    /// # Arguments
    ///
    /// * `channel` - WebSocket channel name
    /// * `event` - Event type (usually "subscribe")
    ///
    /// # Returns
    ///
    /// Tuple of (signature, timestamp)
    pub fn sign_ws_request(&self, channel: &str, event: &str) -> (String, i64) {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("Time went backwards")
            .as_secs() as i64;

        let payload = format!("channel={}&event={}&time={}", channel, event, timestamp);

        let mut mac = HmacSha512::new_from_slice(self.api_secret.as_bytes())
            .expect("HMAC can take key of any size");
        mac.update(payload.as_bytes());
        let signature = hex::encode(mac.finalize().into_bytes());

        (signature, timestamp)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_credentials_creation() {
        let creds = GateioCredentials::new("test_key".to_string(), "test_secret".to_string());
        assert!(creds.is_ok());

        let creds = creds.unwrap();
        assert_eq!(creds.api_key(), "test_key");
        assert_eq!(creds.api_secret(), "test_secret");
    }

    #[test]
    fn test_empty_api_key() {
        let creds = GateioCredentials::new("".to_string(), "test_secret".to_string());
        assert!(creds.is_err());
    }

    #[test]
    fn test_empty_api_secret() {
        let creds = GateioCredentials::new("test_key".to_string(), "".to_string());
        assert!(creds.is_err());
    }

    #[test]
    fn test_sign_request() {
        let creds =
            GateioCredentials::new("test_key".to_string(), "test_secret".to_string()).unwrap();
        let (signature, timestamp) =
            creds.sign_request("GET", "/api/v4/spot/accounts", "", "");

        assert!(!signature.is_empty());
        assert!(timestamp > 0);
        // Signature should be 128 characters (512 bits in hex)
        assert_eq!(signature.len(), 128);
    }

    #[test]
    fn test_sign_ws_request() {
        let creds =
            GateioCredentials::new("test_key".to_string(), "test_secret".to_string()).unwrap();
        let (signature, timestamp) = creds.sign_ws_request("spot.orders", "subscribe");

        assert!(!signature.is_empty());
        assert!(timestamp > 0);
        assert_eq!(signature.len(), 128);
    }
}
