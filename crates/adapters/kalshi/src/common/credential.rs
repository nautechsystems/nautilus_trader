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

//! Kalshi API credentials.

use nautilus_core::{env::get_or_env_var_opt, string::secret::SecretString};

use crate::common::consts::{KALSHI_API_KEY_ID_ENV, KALSHI_API_KEY_PEM_ENV};

/// An API key ID paired with the RSA private key that signs requests.
///
/// Both members are held as [`SecretString`], so they never appear in logs or debug output.
#[derive(Clone, Debug)]
pub struct KalshiCredential {
    api_key_id: SecretString,
    private_key_pem: SecretString,
}

impl KalshiCredential {
    /// Creates a new credential from an API key ID and a PEM-encoded RSA private key.
    #[must_use]
    pub fn new(api_key_id: String, private_key_pem: String) -> Self {
        Self {
            api_key_id: api_key_id.into(),
            private_key_pem: private_key_pem.into(),
        }
    }

    /// Returns the API key ID, which identifies the key in the request headers.
    #[must_use]
    pub fn api_key_id(&self) -> &str {
        self.api_key_id.expose_secret()
    }

    /// Returns the PEM-encoded RSA private key used to sign requests.
    #[must_use]
    pub fn private_key_pem(&self) -> &str {
        self.private_key_pem.expose_secret()
    }

    /// Resolves a credential from explicit values, falling back to the environment.
    ///
    /// # Errors
    ///
    /// Returns an error when either value is absent from both the argument and the environment, or
    /// when the API key ID is blank.
    pub fn resolve(
        api_key_id: Option<String>,
        private_key_pem: Option<String>,
    ) -> anyhow::Result<Self> {
        let api_key_id =
            get_or_env_var_opt(api_key_id, KALSHI_API_KEY_ID_ENV).ok_or_else(|| {
                anyhow::anyhow!(
                    "Kalshi API key ID not found: pass an API key ID or set {KALSHI_API_KEY_ID_ENV}"
                )
            })?;
        let private_key_pem =
            get_or_env_var_opt(private_key_pem, KALSHI_API_KEY_PEM_ENV).ok_or_else(|| {
                anyhow::anyhow!(
                    "Kalshi API key PEM not found: pass a private key PEM or set {KALSHI_API_KEY_PEM_ENV}"
                )
            })?;

        anyhow::ensure!(
            !api_key_id.trim().is_empty(),
            "Kalshi API key ID must not be empty"
        );

        Ok(Self::new(api_key_id, private_key_pem))
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_credential_keeps_secrets_out_of_debug_output() {
        let credential = KalshiCredential::new(
            "a952bcbe-ec3b-4b5b-b8f9-11dae589608c".to_string(),
            "test-secret-key-material".to_string(),
        );

        assert_eq!(
            credential.api_key_id(),
            "a952bcbe-ec3b-4b5b-b8f9-11dae589608c"
        );
        assert_eq!(credential.private_key_pem(), "test-secret-key-material");

        let debug = format!("{credential:?}");

        assert!(!debug.contains("a952bcbe"), "{debug}");
        assert!(!debug.contains("secret"), "{debug}");
    }

    #[rstest]
    fn test_resolve_prefers_explicit_values() {
        let credential =
            KalshiCredential::resolve(Some("key-id".to_string()), Some("pem".to_string())).unwrap();

        assert_eq!(credential.api_key_id(), "key-id");
        assert_eq!(credential.private_key_pem(), "pem");
    }

    #[rstest]
    fn test_resolve_rejects_a_blank_api_key_id() {
        let error = KalshiCredential::resolve(Some("   ".to_string()), Some("pem".to_string()))
            .unwrap_err();

        assert!(error.to_string().contains("must not be empty"), "{error}");
    }

    #[rstest]
    fn test_resolve_names_the_missing_environment_variable() {
        if std::env::var(KALSHI_API_KEY_ID_ENV).is_ok() {
            return;
        }

        let error = KalshiCredential::resolve(None, Some("pem".to_string())).unwrap_err();

        assert!(error.to_string().contains(KALSHI_API_KEY_ID_ENV), "{error}");
    }
}
