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

use nautilus_core::{
    env::resolve_env_var_pair,
    string::secret::{REDACTED, mask_api_key},
};
use zeroize::ZeroizeOnDrop;

/// Returns the environment variable names for API credentials.
#[must_use]
pub fn credential_env_vars() -> (&'static str, &'static str) {
    ("AX_API_KEY", "AX_API_SECRET")
}

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
            .field("api_secret", &REDACTED)
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

    /// Resolves credentials from provided values or environment variables.
    ///
    /// If both `api_key` and `api_secret` are provided, uses those.
    /// Otherwise falls back to environment variables.
    #[must_use]
    pub fn resolve(api_key: Option<String>, api_secret: Option<String>) -> Option<Self> {
        let (key_var, secret_var) = credential_env_vars();
        let (k, s) = resolve_env_var_pair(api_key, api_secret, key_var, secret_var)?;
        Some(Self::new(k, s))
    }

    /// Returns the API key associated with this credential.
    #[must_use]
    pub fn api_key(&self) -> &str {
        &self.api_key
    }

    /// Returns the API secret associated with this credential.
    ///
    /// # Security
    ///
    /// The secret should be handled carefully and never logged or exposed.
    #[must_use]
    pub fn api_secret(&self) -> &str {
        &self.api_secret
    }

    /// Returns a masked version of the API key for logging purposes.
    #[must_use]
    pub fn masked_api_key(&self) -> String {
        mask_api_key(&self.api_key)
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
    fn test_debug_does_not_leak_secret() {
        let credential = Credential::new(API_KEY, API_SECRET);
        let debug_string = format!("{credential:?}");

        assert!(!debug_string.contains(API_SECRET));
        assert!(debug_string.contains(REDACTED));
        assert!(debug_string.contains("test..."));
    }

    #[rstest]
    fn test_resolve_with_both_args() {
        let result = Credential::resolve(Some("my_key".to_string()), Some("my_secret".to_string()));

        assert!(result.is_some());
        assert_eq!(result.unwrap().api_key(), "my_key");
    }

    #[rstest]
    fn test_resolve_with_no_args_no_env() {
        let (key_var, secret_var) = credential_env_vars();
        if std::env::var(key_var).is_ok() || std::env::var(secret_var).is_ok() {
            return;
        }

        let result = Credential::resolve(None, None);

        assert!(result.is_none());
    }
}
