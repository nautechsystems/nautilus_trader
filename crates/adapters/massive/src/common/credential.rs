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

use std::fmt::Debug;

use zeroize::{Zeroize, ZeroizeOnDrop};

use crate::common::consts::MASSIVE_API_KEY_ENV;

/// Massive API key with zeroization on drop.
///
/// The same key material authenticates both the REST API (as a bearer token)
/// and the WebSocket feed (via the `auth` action).
#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct MassiveCredential {
    api_key: String,
}

impl MassiveCredential {
    /// Creates a new [`MassiveCredential`] instance.
    #[must_use]
    pub fn new(api_key: String) -> Self {
        Self { api_key }
    }

    /// Resolves the credential from the provided value or the
    /// `MASSIVE_API_KEY` environment variable, returning `None` when neither
    /// yields a non-empty key.
    #[must_use]
    pub fn resolve(api_key: Option<&str>) -> Option<Self> {
        let key = api_key
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(String::from)
            .or_else(|| {
                std::env::var(MASSIVE_API_KEY_ENV)
                    .ok()
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
            })?;
        Some(Self::new(key))
    }

    /// Returns the raw API key.
    #[must_use]
    pub fn api_key(&self) -> &str {
        &self.api_key
    }

    /// Returns the `Authorization` header value for REST requests.
    #[must_use]
    pub fn bearer_header(&self) -> String {
        format!("Bearer {}", self.api_key)
    }

    /// Returns the serialized WebSocket authentication message.
    #[must_use]
    pub fn ws_auth_message(&self) -> String {
        serde_json::json!({
            "action": "auth",
            "params": self.api_key,
        })
        .to_string()
    }

    /// Returns a masked representation of the API key for logging.
    #[must_use]
    pub fn masked_key(&self) -> String {
        let key = &self.api_key;
        if key.len() <= 4 {
            "****".to_string()
        } else {
            format!("{}****", &key[..4])
        }
    }
}

impl Debug for MassiveCredential {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(MassiveCredential))
            .field("api_key", &self.masked_key())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_resolve_explicit_key() {
        let credential = MassiveCredential::resolve(Some("test-key")).unwrap();
        assert_eq!(credential.api_key(), "test-key");
    }

    #[rstest]
    fn test_resolve_trims_whitespace() {
        let credential = MassiveCredential::resolve(Some("  test-key  ")).unwrap();
        assert_eq!(credential.api_key(), "test-key");
    }

    #[rstest]
    fn test_resolve_empty_key_falls_through() {
        // Explicit empty/whitespace keys must not produce a credential
        // (environment fallback may still apply outside the test).
        if std::env::var(MASSIVE_API_KEY_ENV).is_err() {
            assert!(MassiveCredential::resolve(Some("   ")).is_none());
            assert!(MassiveCredential::resolve(None).is_none());
        }
    }

    #[rstest]
    fn test_bearer_header() {
        let credential = MassiveCredential::new("abc123".to_string());
        assert_eq!(credential.bearer_header(), "Bearer abc123");
    }

    #[rstest]
    fn test_ws_auth_message() {
        let credential = MassiveCredential::new("abc123".to_string());
        let msg: serde_json::Value = serde_json::from_str(&credential.ws_auth_message()).unwrap();
        assert_eq!(msg["action"], "auth");
        assert_eq!(msg["params"], "abc123");
    }

    #[rstest]
    fn test_debug_redacts_key() {
        let credential = MassiveCredential::new("super-secret-key".to_string());
        let debug = format!("{credential:?}");
        assert!(!debug.contains("super-secret-key"), "{debug}");
        assert!(debug.contains("supe****"), "{debug}");
    }

    #[rstest]
    fn test_masked_key_short() {
        let credential = MassiveCredential::new("abc".to_string());
        assert_eq!(credential.masked_key(), "****");
    }
}
