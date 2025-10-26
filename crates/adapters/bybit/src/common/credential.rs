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

//! Bybit API credential storage and signing helpers.

#![allow(unused_assignments)] // Fields are used in methods, false positive from nightly

use std::fmt::Debug;

use aws_lc_rs::hmac;
use hex;
use ustr::Ustr;
use zeroize::ZeroizeOnDrop;

/// API credentials required for signing Bybit REST requests.
#[derive(Clone, ZeroizeOnDrop)]
pub struct Credential {
    #[zeroize(skip)]
    api_key: Ustr,
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
    /// Creates a new [`Credential`] instance from the API key and secret.
    #[must_use]
    pub fn new(api_key: impl Into<String>, api_secret: impl Into<String>) -> Self {
        let api_key = api_key.into();
        let api_secret_bytes = api_secret.into().into_bytes();

        let api_key = Ustr::from(api_key.as_str());

        Self {
            api_key,
            api_secret: api_secret_bytes.into_boxed_slice(),
        }
    }

    /// Returns the API key associated with this credential.
    #[must_use]
    pub fn api_key(&self) -> &Ustr {
        &self.api_key
    }

    /// Produces the Bybit WebSocket authentication signature for the provided expiry timestamp.
    ///
    /// `expires` should be the millisecond timestamp used by the login payload.
    #[must_use]
    pub fn sign_websocket_auth(&self, expires: i64) -> String {
        let message = format!("GET/realtime{expires}");
        let key = hmac::Key::new(hmac::HMAC_SHA256, &self.api_secret);
        let tag = hmac::sign(&key, message.as_bytes());
        hex::encode(tag.as_ref())
    }

    /// Produces the Bybit HMAC signature for the provided payload.
    ///
    /// `payload` should contain either a URL-encoded query string (for GET requests)
    /// or a JSON body (for POST requests). Callers are responsible for ensuring that
    /// the encoding matches the bytes sent over the wire.
    #[must_use]
    pub fn sign_with_payload(
        &self,
        timestamp: &str,
        recv_window_ms: u64,
        payload: Option<&str>,
    ) -> String {
        let recv_window = recv_window_ms.to_string();
        let payload_len = payload.map_or(0usize, str::len);
        let mut message = String::with_capacity(
            timestamp.len() + self.api_key.len() + recv_window.len() + payload_len,
        );

        message.push_str(timestamp);
        message.push_str(self.api_key.as_str());
        message.push_str(&recv_window);
        if let Some(payload) = payload {
            message.push_str(payload);
        }

        let key = hmac::Key::new(hmac::HMAC_SHA256, &self.api_secret);
        let tag = hmac::sign(&key, message.as_bytes());
        hex::encode(tag.as_ref())
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    const API_KEY: &str = "test_api_key";
    const API_SECRET: &str = "test_secret";
    const RECV_WINDOW: u64 = 5_000;
    const TIMESTAMP: &str = "1700000000000";

    #[rstest]
    fn sign_with_payload_matches_reference_get() {
        let credential = Credential::new(API_KEY, API_SECRET);
        let query = "category=linear&symbol=BTCUSDT";

        let signature = credential.sign_with_payload(TIMESTAMP, RECV_WINDOW, Some(query));

        assert_eq!(
            signature,
            "fd4f31228a46109dc6673062328693696df9a96c7ff04e6491a45e7f63a0fdd7"
        );
    }

    #[rstest]
    fn sign_with_payload_matches_reference_post() {
        let credential = Credential::new(API_KEY, API_SECRET);
        let body = "{\"category\": \"linear\", \"symbol\": \"BTCUSDT\", \"orderLinkId\": \"test-order-1\"}";

        let signature = credential.sign_with_payload(TIMESTAMP, RECV_WINDOW, Some(body));

        assert_eq!(
            signature,
            "2df4a0603d69c08d5dea29ba85b46eb7db64ce9e9ebd34a7802a3d69700cb2a1"
        );
    }

    #[rstest]
    fn sign_with_empty_payload_omits_tail() {
        let credential = Credential::new(API_KEY, API_SECRET);

        let signature = credential.sign_with_payload(TIMESTAMP, RECV_WINDOW, None);

        let expected = credential.sign_with_payload(TIMESTAMP, RECV_WINDOW, Some(""));
        assert_eq!(signature, expected);
    }

    #[rstest]
    fn sign_websocket_auth_matches_reference() {
        let credential = Credential::new(API_KEY, API_SECRET);
        let expires: i64 = 1_700_000_000_000;

        let signature = credential.sign_websocket_auth(expires);

        assert_eq!(
            signature,
            "bacffe7500499eb829bb58c45d36d1b3e5ac67c14eaeba91df5e99ccee013925"
        );
    }
}
