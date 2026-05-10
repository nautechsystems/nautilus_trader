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

//! Betfair API credential storage.

use std::fmt::Debug;

use nautilus_core::string::REDACTED;
use thiserror::Error;
use zeroize::ZeroizeOnDrop;

/// Environment variable name for the Betfair account username.
pub const BETFAIR_USERNAME_ENV: &str = "BETFAIR_USERNAME";

/// Environment variable name for the Betfair account password.
pub const BETFAIR_PASSWORD_ENV: &str = "BETFAIR_PASSWORD";

/// Environment variable name for the Betfair application key.
pub const BETFAIR_APP_KEY_ENV: &str = "BETFAIR_APP_KEY";

/// Errors that can occur when resolving credentials.
#[derive(Debug, Error)]
pub enum CredentialError {
    /// Username was provided but password is missing.
    #[error("Username provided but password is missing")]
    MissingPassword,
    /// Password was provided but username is missing.
    #[error("Password provided but username is missing")]
    MissingUsername,
    /// App key is missing.
    #[error("App key is missing")]
    MissingAppKey,
}

/// Betfair API credentials for session-token authentication.
///
/// Betfair uses username/password login to obtain a session token,
/// which is then passed as `X-Authentication` on subsequent requests.
/// The `app_key` identifies the application and is sent as `X-Application`.
///
/// Secrets are automatically zeroized on drop for security.
#[derive(Clone, ZeroizeOnDrop)]
pub struct BetfairCredential {
    username: Box<str>,
    password: Box<str>,
    app_key: Box<str>,
}

impl Debug for BetfairCredential {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(BetfairCredential))
            .field("username", &self.username)
            .field("password", &REDACTED)
            .field("app_key", &self.app_key)
            .finish()
    }
}

impl BetfairCredential {
    /// Creates a new [`BetfairCredential`] instance.
    #[must_use]
    pub fn new(username: String, password: String, app_key: String) -> Self {
        Self {
            username: username.into_boxed_str(),
            password: password.into_boxed_str(),
            app_key: app_key.into_boxed_str(),
        }
    }

    /// Load credentials from environment variables.
    ///
    /// Reads `BETFAIR_USERNAME`, `BETFAIR_PASSWORD`, and `BETFAIR_APP_KEY`.
    ///
    /// Returns `None` if any variable is not set.
    #[must_use]
    pub fn from_env() -> Option<Self> {
        let username = std::env::var(BETFAIR_USERNAME_ENV).ok()?;
        let password = std::env::var(BETFAIR_PASSWORD_ENV).ok()?;
        let app_key = std::env::var(BETFAIR_APP_KEY_ENV).ok()?;
        Some(Self::new(username, password, app_key))
    }

    /// Resolves credentials from provided values or environment.
    ///
    /// If all three values are provided, uses those directly.
    /// If none are provided, falls back to environment variables.
    ///
    /// # Errors
    ///
    /// Returns an error if credentials are partially provided.
    pub fn resolve(
        username: Option<String>,
        password: Option<String>,
        app_key: Option<String>,
    ) -> Result<Option<Self>, CredentialError> {
        match (username, password, app_key) {
            (Some(u), Some(p), Some(k)) => Ok(Some(Self::new(u, p, k))),
            (None, None, None) => Ok(Self::from_env()),
            (Some(_), None, _) => Err(CredentialError::MissingPassword),
            (None, Some(_), _) => Err(CredentialError::MissingUsername),
            (_, _, None) => Err(CredentialError::MissingAppKey),
            (None, None, Some(_)) => Err(CredentialError::MissingUsername),
        }
    }

    /// Returns the account username.
    #[must_use]
    pub fn username(&self) -> &str {
        &self.username
    }

    /// Returns the account password.
    #[must_use]
    pub fn password(&self) -> &str {
        &self.password
    }

    /// Returns the application key.
    #[must_use]
    pub fn app_key(&self) -> &str {
        &self.app_key
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_credential_creation() {
        let cred = BetfairCredential::new(
            "testuser".to_string(),
            "testpass".to_string(),
            "appkey123".to_string(),
        );

        assert_eq!(cred.username(), "testuser");
        assert_eq!(cred.password(), "testpass");
        assert_eq!(cred.app_key(), "appkey123");
    }

    #[rstest]
    fn test_debug_redacts_password() {
        let cred = BetfairCredential::new(
            "myuser".to_string(),
            "supersecret".to_string(),
            "myappkey".to_string(),
        );

        let debug_output = format!("{cred:?}");

        assert!(debug_output.contains(REDACTED));
        assert!(!debug_output.contains("supersecret"));
        assert!(debug_output.contains("myuser"));
        assert!(debug_output.contains("myappkey"));
    }

    #[rstest]
    fn test_resolve_with_all_credentials() {
        let result = BetfairCredential::resolve(
            Some("user".to_string()),
            Some("pass".to_string()),
            Some("key".to_string()),
        );

        assert!(result.is_ok());
        let cred = result.unwrap().unwrap();
        assert_eq!(cred.username(), "user");
    }

    #[rstest]
    fn test_resolve_missing_password() {
        let result =
            BetfairCredential::resolve(Some("user".to_string()), None, Some("key".to_string()));

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            CredentialError::MissingPassword
        ));
    }

    #[rstest]
    fn test_resolve_missing_username() {
        let result =
            BetfairCredential::resolve(None, Some("pass".to_string()), Some("key".to_string()));

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            CredentialError::MissingUsername
        ));
    }

    #[rstest]
    fn test_resolve_missing_app_key() {
        let result =
            BetfairCredential::resolve(Some("user".to_string()), Some("pass".to_string()), None);

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            CredentialError::MissingAppKey
        ));
    }

    #[rstest]
    fn test_resolve_none_falls_back_to_env() {
        // Without env vars set, should return None
        let result = BetfairCredential::resolve(None, None, None);

        assert!(result.is_ok());
        // Will be None unless env vars are set in the test environment
    }
}
