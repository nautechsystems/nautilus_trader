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

use std::fmt::Debug;

use hmac::{Hmac, Mac};
use sha2::Sha256;
use ustr::Ustr;
use zeroize::ZeroizeOnDrop;

type HmacSha256 = Hmac<Sha256>;

/// Delta Exchange API credentials for signing requests.
///
/// Uses HMAC SHA256 for request signing as per API specifications.
/// Secrets are automatically zeroized on drop for security.
#[derive(Clone, ZeroizeOnDrop)]
pub struct Credential {
    #[zeroize(skip)]
    pub api_key: Ustr,
    api_secret: Box<[u8]>,
}

impl Debug for Credential {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Credential")
            .field("api_key", &self.api_key)
            .field("api_secret", &"<redacted>")
            .finish()
    }
}

impl Credential {
    /// Creates a new [`Credential`] instance.
    ///
    /// # Errors
    ///
    /// Returns an error if the API secret is empty.
    pub fn new(api_key: String, api_secret: String) -> Result<Self, String> {
        if api_secret.is_empty() {
            return Err("API secret cannot be empty".to_string());
        }

        Ok(Self {
            api_key: api_key.into(),
            api_secret: api_secret.into_bytes().into_boxed_slice(),
        })
    }

    /// Signs a request message according to the Delta Exchange authentication scheme.
    ///
    /// The signature is created by HMAC-SHA256 signing the concatenation of:
    /// - HTTP method (uppercase)
    /// - Request path (without query parameters)
    /// - Request body (empty string for GET requests)
    /// - Timestamp (Unix timestamp in milliseconds)
    ///
    /// # Errors
    ///
    /// Returns an error if signature generation fails due to cryptographic errors.
    pub fn sign(
        &self,
        method: &str,
        endpoint: &str,
        body: &str,
        timestamp: u64,
    ) -> Result<String, String> {
        // Extract the path without query parameters
        let request_path = match endpoint.find('?') {
            Some(index) => &endpoint[..index],
            None => endpoint,
        };

        // Create the message to sign: method + path + body + timestamp
        let message = format!("{}{}{}{}", method.to_uppercase(), request_path, body, timestamp);

        // Create HMAC-SHA256 signature
        let mut mac = HmacSha256::new_from_slice(&self.api_secret)
            .map_err(|e| format!("Invalid key length: {}", e))?;
        
        mac.update(message.as_bytes());
        let result = mac.finalize();
        
        // Return hex-encoded signature
        Ok(hex::encode(result.into_bytes()))
    }

    /// Signs a WebSocket authentication message.
    ///
    /// For Delta Exchange WebSocket authentication, the message format is:
    /// api_key + timestamp
    ///
    /// # Errors
    ///
    /// Returns an error if signature generation fails due to cryptographic errors.
    pub fn sign_ws(&self, timestamp: u64) -> Result<String, String> {
        let message = format!("{}{}", self.api_key, timestamp);
        
        let mut mac = HmacSha256::new_from_slice(&self.api_secret)
            .map_err(|e| format!("Invalid key length: {}", e))?;
        
        mac.update(message.as_bytes());
        let result = mac.finalize();
        
        Ok(hex::encode(result.into_bytes()))
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use super::*;

    const API_KEY: &str = "test_key_123";
    const API_SECRET: &str = "test_secret_456";

    #[test]
    fn test_credential_creation() {
        let credential = Credential::new(API_KEY.to_string(), API_SECRET.to_string()).unwrap();
        assert_eq!(credential.api_key.as_str(), API_KEY);
    }

    #[test]
    fn test_credential_creation_empty_secret() {
        let result = Credential::new(API_KEY.to_string(), String::new());
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "API secret cannot be empty");
    }

    #[test]
    fn test_sign_get_request() {
        let credential = Credential::new(API_KEY.to_string(), API_SECRET.to_string()).unwrap();
        let timestamp = 1641890400000; // 2022-01-11T00:00:00Z in milliseconds
        let signature = credential.sign("GET", "/v2/products", "", timestamp).unwrap();
        
        // This should produce a consistent signature for the same inputs
        assert!(!signature.is_empty());
        assert_eq!(signature.len(), 64); // SHA256 hex string length
    }

    #[test]
    fn test_sign_post_request() {
        let credential = Credential::new(API_KEY.to_string(), API_SECRET.to_string()).unwrap();
        let timestamp = 1641890400000;
        let body = r#"{"product_id":27,"size":100,"side":"buy","order_type":"limit_order","limit_price":"50000"}"#;
        let signature = credential.sign("POST", "/v2/orders", body, timestamp).unwrap();
        
        assert!(!signature.is_empty());
        assert_eq!(signature.len(), 64);
    }

    #[test]
    fn test_sign_with_query_params() {
        let credential = Credential::new(API_KEY.to_string(), API_SECRET.to_string()).unwrap();
        let timestamp = 1641890400000;
        let signature = credential.sign("GET", "/v2/products?page_size=50", "", timestamp).unwrap();
        
        // Should ignore query parameters in signature
        let signature_without_params = credential.sign("GET", "/v2/products", "", timestamp).unwrap();
        assert_eq!(signature, signature_without_params);
    }

    #[test]
    fn test_sign_ws() {
        let credential = Credential::new(API_KEY.to_string(), API_SECRET.to_string()).unwrap();
        let timestamp = 1641890400000;
        let signature = credential.sign_ws(timestamp).unwrap();
        
        assert!(!signature.is_empty());
        assert_eq!(signature.len(), 64);
    }

    #[test]
    fn test_debug_redacts_secret() {
        let credential = Credential::new(API_KEY.to_string(), API_SECRET.to_string()).unwrap();
        let debug_output = format!("{:?}", credential);
        
        assert!(debug_output.contains("api_key"));
        assert!(debug_output.contains("test_key_123"));
        assert!(debug_output.contains("<redacted>"));
        assert!(!debug_output.contains("test_secret_456"));
    }

    #[test]
    fn test_signature_consistency() {
        let credential = Credential::new(API_KEY.to_string(), API_SECRET.to_string()).unwrap();
        let timestamp = 1641890400000;
        
        let sig1 = credential.sign("GET", "/v2/products", "", timestamp).unwrap();
        let sig2 = credential.sign("GET", "/v2/products", "", timestamp).unwrap();
        
        assert_eq!(sig1, sig2, "Signatures should be consistent for same inputs");
    }
}
