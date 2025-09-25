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

//! Minimal HTTP client scaffold for the Bybit REST API.
//!
//! The client currently focuses on handling request signing so that higher level
//! components can authenticate requests against Bybit's v5 REST endpoints.

use std::collections::HashMap;

use nautilus_core::consts::NAUTILUS_USER_AGENT;
use nautilus_network::http::HttpClient;
use thiserror::Error;

use crate::common::{
    consts::{BYBIT_HTTP_URL, BYBIT_NAUTILUS_BROKER_ID},
    credential::Credential,
};

const DEFAULT_RECV_WINDOW_MS: u64 = 5_000;

/// Error type produced by the [`BybitHttpClient`].
#[derive(Debug, Error, PartialEq, Eq)]
pub enum BybitHttpError {
    /// Returned when signing is attempted without credentials.
    #[error("missing credentials for signed request")]
    MissingCredentials,
}

/// Lightweight Bybit HTTP client that encapsulates default headers and signing.
#[derive(Clone, Debug)]
pub struct BybitHttpClient {
    base_url: String,
    client: HttpClient,
    credential: Option<Credential>,
    recv_window_ms: u64,
}

impl BybitHttpClient {
    /// Creates a new client without credentials for public endpoints.
    #[must_use]
    pub fn new(base_url: Option<String>, timeout_secs: Option<u64>) -> Self {
        let base_url = base_url.unwrap_or_else(|| BYBIT_HTTP_URL.to_string());
        let client = HttpClient::new(Self::default_headers(), vec![], vec![], None, timeout_secs);

        Self {
            base_url,
            client,
            credential: None,
            recv_window_ms: DEFAULT_RECV_WINDOW_MS,
        }
    }

    /// Creates a new client configured with API credentials for private endpoints.
    #[must_use]
    pub fn with_credential(
        api_key: impl Into<String>,
        api_secret: impl Into<String>,
        base_url: Option<String>,
        timeout_secs: Option<u64>,
        recv_window_ms: Option<u64>,
    ) -> Self {
        let mut client = Self::new(base_url, timeout_secs);
        client.credential = Some(Credential::new(api_key, api_secret));
        if let Some(recv_window_ms) = recv_window_ms {
            client.recv_window_ms = recv_window_ms;
        }
        client
    }

    /// Returns the configured receive window in milliseconds.
    #[must_use]
    pub fn recv_window_ms(&self) -> u64 {
        self.recv_window_ms
    }

    /// Returns a reference to the underlying [`HttpClient`].
    #[must_use]
    pub fn http_client(&self) -> &HttpClient {
        &self.client
    }

    /// Returns the API credential if configured.
    #[must_use]
    pub fn credential(&self) -> Option<&Credential> {
        self.credential.as_ref()
    }

    /// Computes the Bybit signature for the provided payload.
    ///
    /// `timestamp` must be a string representing the milliseconds timestamp.
    /// `payload` should contain the already encoded query string for GET requests
    /// or the JSON payload for POST requests.
    pub fn sign_with_payload(
        &self,
        timestamp: &str,
        payload: Option<&str>,
    ) -> Result<String, BybitHttpError> {
        let credential = self
            .credential
            .as_ref()
            .ok_or(BybitHttpError::MissingCredentials)?;

        Ok(credential.sign_with_payload(timestamp, self.recv_window_ms, payload))
    }

    /// Convenience wrapper for signing GET requests.
    pub fn sign_get(&self, timestamp: &str, query: Option<&str>) -> Result<String, BybitHttpError> {
        self.sign_with_payload(timestamp, query)
    }

    /// Convenience wrapper for signing POST requests.
    pub fn sign_post(&self, timestamp: &str, body: Option<&str>) -> Result<String, BybitHttpError> {
        self.sign_with_payload(timestamp, body)
    }

    fn default_headers() -> HashMap<String, String> {
        HashMap::from([
            ("Content-Type".to_string(), "application/json".to_string()),
            ("User-Agent".to_string(), NAUTILUS_USER_AGENT.to_string()),
            ("Referer".to_string(), BYBIT_NAUTILUS_BROKER_ID.to_string()),
        ])
    }

    /// Returns the base URL used for requests.
    #[must_use]
    pub fn base_url(&self) -> &str {
        &self.base_url
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use super::*;

    const API_KEY: &str = "test_api_key";
    const API_SECRET: &str = "test_secret";
    const TIMESTAMP: &str = "1700000000000";
    const RECV_WINDOW: u64 = 5_000;

    #[test]
    fn sign_get_matches_reference() {
        let client =
            BybitHttpClient::with_credential(API_KEY, API_SECRET, None, None, Some(RECV_WINDOW));
        let query = "category=linear&symbol=BTCUSDT";

        let signature = client.sign_get(TIMESTAMP, Some(query)).unwrap();

        assert_eq!(
            signature,
            "fd4f31228a46109dc6673062328693696df9a96c7ff04e6491a45e7f63a0fdd7"
        );
    }

    #[test]
    fn sign_post_matches_reference() {
        let client =
            BybitHttpClient::with_credential(API_KEY, API_SECRET, None, None, Some(RECV_WINDOW));
        let body = "{\"category\": \"linear\", \"symbol\": \"BTCUSDT\", \"orderLinkId\": \"test-order-1\"}";

        let signature = client.sign_post(TIMESTAMP, Some(body)).unwrap();

        assert_eq!(
            signature,
            "2df4a0603d69c08d5dea29ba85b46eb7db64ce9e9ebd34a7802a3d69700cb2a1"
        );
    }

    #[test]
    fn sign_without_credential_errors() {
        let client = BybitHttpClient::new(None, None);
        let err = client.sign_get(TIMESTAMP, Some("foo=bar")).unwrap_err();
        assert_eq!(err, BybitHttpError::MissingCredentials);
    }
}
