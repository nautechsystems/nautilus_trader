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

use aws_lc_rs::hmac;
use base64::prelude::*;
use ustr::Ustr;
use zeroize::ZeroizeOnDrop;

/// Coinbase International API credentials for signing requests.
///
/// Uses HMAC SHA256 for request signing as per API specifications.
/// Secrets are automatically zeroized on drop for security.
#[derive(Clone, ZeroizeOnDrop)]
pub struct Credential {
    #[zeroize(skip)]
    pub api_key: Ustr,
    #[zeroize(skip)]
    pub api_passphrase: Ustr,
    api_secret: Box<[u8]>,
}

impl Debug for Credential {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(Credential))
            .field("api_key", &self.api_key)
            .field("api_passphrase", &self.api_passphrase)
            .field("api_secret", &"<redacted>")
            .finish()
    }
}

impl Credential {
    /// Creates a new [`Credential`] instance.
    ///
    /// # Panics
    ///
    /// Panics if the provided `api_secret` is not valid base64.
    #[must_use]
    pub fn new(api_key: String, api_secret: String, api_passphrase: String) -> Self {
        let decoded_secret = BASE64_STANDARD
            .decode(api_secret)
            .expect("Invalid base64 secret key");

        Self {
            api_key: api_key.into(),
            api_passphrase: api_passphrase.into(),
            api_secret: decoded_secret.into_boxed_slice(),
        }
    }

    /// Signs a request message according to the Coinbase authentication scheme.
    ///
    /// # Panics
    ///
    /// Panics if signature generation fails due to key or cryptographic errors.
    #[must_use]
    pub fn sign(&self, timestamp: &str, method: &str, endpoint: &str, body: &str) -> String {
        // Extract the path without query parameters
        let request_path = match endpoint.find('?') {
            Some(index) => &endpoint[..index],
            None => endpoint,
        };

        let message = format!("{timestamp}{method}{request_path}{body}");
        let key = hmac::Key::new(hmac::HMAC_SHA256, &self.api_secret[..]);
        let tag = hmac::sign(&key, message.as_bytes());
        BASE64_STANDARD.encode(tag.as_ref())
    }

    /// Signs a WebSocket authentication message.
    ///
    /// # Panics
    ///
    /// Panics if signature generation fails due to key or cryptographic errors.
    pub fn sign_ws(&self, timestamp: &str) -> String {
        let message = format!("{timestamp}{}CBINTLMD{}", self.api_key, self.api_passphrase);
        tracing::trace!("Signing message: {message}");

        let key = hmac::Key::new(hmac::HMAC_SHA256, &self.api_secret[..]);
        let tag = hmac::sign(&key, message.as_bytes());
        BASE64_STANDARD.encode(tag.as_ref())
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    const API_KEY: &str = "test_key_123";
    const API_SECRET: &str = "dGVzdF9zZWNyZXRfYmFzZTY0"; // base64 encoded "test_secret_base64"
    const API_PASSPHRASE: &str = "test_pass";

    #[rstest]
    fn test_simple_get() {
        let credential = Credential::new(
            API_KEY.to_string(),
            API_SECRET.to_string(),
            API_PASSPHRASE.to_string(),
        );
        let timestamp = "1641890400"; // 2022-01-11T00:00:00Z
        let signature = credential.sign(timestamp, "GET", "/api/v1/fee-rate-tiers", "");

        assert_eq!(signature, "h/9tnYzD/nsEbH1sV7dkB5uJ3Vygr4TjmOOxJNQB8ts=");
    }

    #[rstest]
    fn test_debug_redacts_secret() {
        let credential = Credential::new(
            API_KEY.to_string(),
            API_SECRET.to_string(),
            API_PASSPHRASE.to_string(),
        );
        let dbg_out = format!("{credential:?}");
        assert!(dbg_out.contains("api_secret: \"<redacted>\""));
        assert!(!dbg_out.contains("dGVz")); // base64 fragment
        let secret_bytes_dbg = format!("{:?}", BASE64_STANDARD.decode(API_SECRET).unwrap());
        assert!(
            !dbg_out.contains(&secret_bytes_dbg),
            "Debug output must not contain raw secret bytes"
        );
    }
}
