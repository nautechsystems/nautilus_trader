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

//! AX Exchange API credential storage for bearer token authentication.

use core::fmt::Debug;

use zeroize::ZeroizeOnDrop;

/// API credentials required for Ax bearer token authentication.
///
/// Ax uses bearer token authentication where the API key and secret
/// are used to obtain a session token that is then used in the Authorization header.
#[derive(Clone, ZeroizeOnDrop)]
pub struct Credential {
    api_key: Box<str>,
    api_secret: Box<str>,
}

impl Debug for Credential {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(Credential))
            .field("api_key", &self.masked_api_key())
            .field("api_secret", &"<redacted>")
            .finish()
    }
}

impl Credential {
    /// Creates a new [`Credential`] instance from the API key and secret.
    #[must_use]
    pub fn new(api_key: impl Into<String>, api_secret: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into().into_boxed_str(),
            api_secret: api_secret.into().into_boxed_str(),
        }
    }

    /// Returns the API key associated with this credential.
    #[must_use]
    pub fn api_key(&self) -> &str {
        &self.api_key
    }

    /// Returns the API secret associated with this credential.
    ///
    /// # Safety
    ///
    /// The secret should be handled carefully and never logged or exposed.
    #[must_use]
    pub fn api_secret(&self) -> &str {
        &self.api_secret
    }

    /// Returns a masked version of the API key for logging purposes.
    ///
    /// Shows first 4 and last 4 characters with ellipsis in between.
    /// For keys shorter than 8 characters, shows asterisks only.
    #[must_use]
    pub fn masked_api_key(&self) -> String {
        let key = self.api_key.as_ref();
        let len = key.len();

        if len <= 8 {
            "*".repeat(len)
        } else {
            format!("{}...{}", &key[..4], &key[len - 4..])
        }
    }

    /// Creates an Authorization header value for bearer token authentication.
    ///
    /// Returns the value to be used in the `Authorization` HTTP header.
    #[must_use]
    pub fn bearer_token(&self, session_token: &str) -> String {
        format!("Bearer {session_token}")
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    const API_KEY: &str = "test_api_key_123";
    const API_SECRET: &str = "test_secret_456";

    #[rstest]
    fn test_credential_creation() {
        let credential = Credential::new(API_KEY, API_SECRET);

        assert_eq!(credential.api_key(), API_KEY);
        assert_eq!(credential.api_secret(), API_SECRET);
    }

    #[rstest]
    fn test_masked_api_key() {
        let credential = Credential::new(API_KEY, API_SECRET);
        let masked = credential.masked_api_key();

        assert_eq!(masked, "test..._123");
        assert!(!masked.contains("api_key"));
    }

    #[rstest]
    fn test_masked_api_key_short() {
        let credential = Credential::new("short", API_SECRET);
        let masked = credential.masked_api_key();

        assert_eq!(masked, "*****");
    }

    #[rstest]
    fn test_bearer_token() {
        let credential = Credential::new(API_KEY, API_SECRET);
        let session_token = "abc123def456"; // gitleaks:allow
        let auth_header = credential.bearer_token(session_token);

        assert_eq!(auth_header, "Bearer abc123def456");
    }

    #[rstest]
    fn test_debug_does_not_leak_secret() {
        let credential = Credential::new(API_KEY, API_SECRET);
        let debug_string = format!("{credential:?}");

        assert!(!debug_string.contains(API_SECRET));
        assert!(debug_string.contains("<redacted>"));
        assert!(debug_string.contains("test..."));
    }
}
