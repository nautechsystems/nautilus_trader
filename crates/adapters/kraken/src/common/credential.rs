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

//! Request signing and authentication credentials for the Kraken API.

use std::collections::HashMap;

use aws_lc_rs::{digest, hmac};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde_urlencoded;
use zeroize::{Zeroize, ZeroizeOnDrop};

#[derive(Clone, Debug, Zeroize, ZeroizeOnDrop)]
pub struct KrakenCredential {
    api_key: String,
    api_secret: String,
}

impl KrakenCredential {
    pub fn new(api_key: impl Into<String>, api_secret: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            api_secret: api_secret.into(),
        }
    }

    pub fn api_key(&self) -> &str {
        &self.api_key
    }

    /// Sign a request for Kraken REST API.
    ///
    /// Kraken uses HMAC-SHA512 with the following message:
    /// - path + SHA256(nonce + POST data)
    /// - The secret is base64 decoded before signing
    pub fn sign_request(
        &self,
        path: &str,
        nonce: u64,
        params: &HashMap<String, String>,
    ) -> anyhow::Result<(String, String)> {
        // Decode the secret from base64
        let secret = STANDARD
            .decode(&self.api_secret)
            .map_err(|e| anyhow::anyhow!("Failed to decode API secret: {e}"))?;

        // Create POST data string
        let mut post_data = format!("nonce={nonce}");
        if !params.is_empty() {
            let encoded = serde_urlencoded::to_string(params)
                .map_err(|e| anyhow::anyhow!("Failed to encode params: {e}"))?;
            post_data.push('&');
            post_data.push_str(&encoded);
        }

        // Hash the nonce + POST data with SHA256
        let hash = digest::digest(&digest::SHA256, post_data.as_bytes());

        // Concatenate path + hash
        let mut message = path.as_bytes().to_vec();
        message.extend_from_slice(hash.as_ref());

        // Sign with HMAC-SHA512
        let key = hmac::Key::new(hmac::HMAC_SHA512, &secret);
        let signature = hmac::sign(&key, &message);

        // Encode signature as base64 and return with post_data
        Ok((STANDARD.encode(signature.as_ref()), post_data))
    }

    /// Returns a masked version of the API key for logging purposes.
    ///
    /// Shows first 4 and last 4 characters with ellipsis in between.
    /// For keys shorter than 8 characters, shows asterisks only.
    #[must_use]
    pub fn api_key_masked(&self) -> String {
        nautilus_core::string::mask_api_key(&self.api_key)
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_credential_creation() {
        let cred = KrakenCredential::new("test_key", "test_secret");
        assert_eq!(cred.api_key(), "test_key");
    }
}
