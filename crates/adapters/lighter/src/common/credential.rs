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

//! Lighter credential storage and resolution.
//!
//! Lighter signs L2 transactions with Schnorr signatures over the ecgfp5 curve
//! (Goldilocks quintic extension field) and Poseidon2 hashing. The cryptographic
//! primitives live in [`crate::signing`]; this module only handles credential
//! plumbing (private key bytes, account index, API key index, env-var resolution).

use std::{fmt::Debug, str};

use anyhow::Context;
use nautilus_core::{
    env::get_or_env_var_opt,
    hex,
    string::secret::{REDACTED, mask_api_key},
};
use zeroize::ZeroizeOnDrop;

use crate::{
    common::enums::LighterEnvironment,
    signing::{curve::SCALAR_BYTES, schnorr::PrivateKey},
};

const LIGHTER_API_KEY_INDEX_VAR: &str = "LIGHTER_API_KEY_INDEX";
const LIGHTER_API_SECRET_VAR: &str = "LIGHTER_API_SECRET";
const LIGHTER_ACCOUNT_INDEX_VAR: &str = "LIGHTER_ACCOUNT_INDEX";
const LIGHTER_TESTNET_API_KEY_INDEX_VAR: &str = "LIGHTER_TESTNET_API_KEY_INDEX";
const LIGHTER_TESTNET_API_SECRET_VAR: &str = "LIGHTER_TESTNET_API_SECRET";
const LIGHTER_TESTNET_ACCOUNT_INDEX_VAR: &str = "LIGHTER_TESTNET_ACCOUNT_INDEX";

/// Environment variable names for Lighter credentials.
///
/// Returns `(api_key_index_var, api_secret_var, account_index_var)`. The
/// `api_key_index_var` holds the per-account API key slot (0..=254), the
/// `api_secret_var` holds the hex-encoded private key, and the
/// `account_index_var` holds the account number assigned at registration.
#[must_use]
pub const fn credential_env_vars(
    environment: LighterEnvironment,
) -> (&'static str, &'static str, &'static str) {
    match environment {
        LighterEnvironment::Mainnet => (
            LIGHTER_API_KEY_INDEX_VAR,
            LIGHTER_API_SECRET_VAR,
            LIGHTER_ACCOUNT_INDEX_VAR,
        ),
        LighterEnvironment::Testnet => (
            LIGHTER_TESTNET_API_KEY_INDEX_VAR,
            LIGHTER_TESTNET_API_SECRET_VAR,
            LIGHTER_TESTNET_ACCOUNT_INDEX_VAR,
        ),
    }
}

/// Lighter API credentials required for authenticated REST, private WebSocket,
/// and L2 transaction signing.
///
/// Lighter identifies API keys by numeric index. The API private key signs both
/// auth tokens and L2 transactions for `(account_index, api_key_index)`.
#[derive(Clone, ZeroizeOnDrop)]
pub struct Credential {
    api_key_index: u8,
    account_index: i64,
    api_secret: Box<[u8]>,
}

impl Debug for Credential {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(Credential))
            .field("api_key_index", &self.api_key_index)
            .field("account_index", &self.account_index)
            .field("api_secret", &REDACTED)
            .finish()
    }
}

impl Credential {
    /// Creates a new [`Credential`] instance from a key index, private key, and
    /// account index.
    ///
    /// # Errors
    ///
    /// Returns an error if `account_index` exceeds the signed range used by the
    /// Lighter signer or if `api_secret` is not a 40-byte hex private key.
    pub fn new(
        api_key_index: u8,
        api_secret: impl Into<String>,
        account_index: u64,
    ) -> anyhow::Result<Self> {
        let api_key_index = ensure_api_key_index(api_key_index)?;
        let account_index = i64::try_from(account_index)
            .context("Lighter account index exceeds signed 64-bit range")?;
        let credential = Self {
            api_key_index,
            account_index,
            api_secret: api_secret.into().into_bytes().into_boxed_slice(),
        };
        credential.private_key()?;
        Ok(credential)
    }

    /// Resolves credentials from provided config values or environment
    /// variables.
    ///
    /// Config values take precedence, but a blank or whitespace-only
    /// `private_key` falls back to the environment variable. Environment
    /// variables follow [`credential_env_vars`]. `LIGHTER_API_KEY_INDEX` is
    /// the per-account API key slot (0..=254), separate from any hex public
    /// key the venue reports for that slot.
    ///
    /// # Errors
    ///
    /// Returns an error if any resolved numeric field cannot be parsed, if the
    /// account index exceeds the signed range, or if the API private key is not
    /// valid 40-byte hex.
    pub fn resolve(
        private_key: Option<String>,
        account_index: Option<u64>,
        api_key_index: Option<u8>,
        environment: LighterEnvironment,
    ) -> anyhow::Result<Option<Self>> {
        let (api_key_var, api_secret_var, account_index_var) = credential_env_vars(environment);

        let api_key_index = resolve_api_key_index(api_key_index, api_key_var)?;
        let account_index = resolve_account_index(account_index, account_index_var)?;
        let api_secret =
            get_or_env_var_opt(private_key.filter(|s| !s.trim().is_empty()), api_secret_var)
                .filter(|s| !s.trim().is_empty());

        credential_from_resolved_values(
            api_key_index,
            account_index,
            api_secret,
            api_key_var,
            api_secret_var,
            account_index_var,
        )
    }

    /// Returns the Lighter API key index.
    #[must_use]
    pub const fn api_key_index(&self) -> u8 {
        self.api_key_index
    }

    /// Returns the Lighter account index.
    #[must_use]
    pub const fn account_index(&self) -> i64 {
        self.account_index
    }

    /// Decodes the API private key for Lighter signing.
    ///
    /// # Errors
    ///
    /// Returns an error if the secret is not 40-byte hex, with or without a
    /// `0x` prefix.
    pub fn private_key(&self) -> anyhow::Result<PrivateKey> {
        let mut bytes = [0u8; SCALAR_BYTES];
        let secret =
            str::from_utf8(&self.api_secret).context("Lighter API secret must be UTF-8")?;
        let decoded = decode_private_key_hex(secret)?;
        bytes.copy_from_slice(&decoded);
        Ok(PrivateKey::from_le_bytes_reduce(bytes))
    }
}

/// Replaces any `auth=<token>` substring with a masked token for logs.
#[must_use]
pub(crate) fn scrub_auth(text: &str) -> String {
    let needle = "auth=";
    if !text.contains(needle) {
        return text.to_string();
    }

    let mut out = String::with_capacity(text.len());
    let mut idx = 0;
    while idx < text.len() {
        if let Some(start) = text[idx..].find(needle) {
            let abs_start = idx + start + needle.len();
            out.push_str(&text[idx..abs_start]);
            let end = text[abs_start..]
                .find(|c: char| c == '&' || c.is_whitespace())
                .map_or(text.len(), |p| abs_start + p);
            let token = &text[abs_start..end];
            out.push_str(&mask_api_key(token));
            idx = end;
        } else {
            out.push_str(&text[idx..]);
            break;
        }
    }
    out
}

fn credential_from_resolved_values(
    api_key_index: Option<u8>,
    account_index: Option<u64>,
    api_secret: Option<String>,
    api_key_var: &str,
    api_secret_var: &str,
    account_index_var: &str,
) -> anyhow::Result<Option<Credential>> {
    match (api_key_index, account_index, api_secret) {
        (Some(api_key_index), Some(account_index), Some(api_secret)) => Ok(Some(Credential::new(
            api_key_index,
            api_secret,
            account_index,
        )?)),
        (None, None, None) => Ok(None),
        _ => anyhow::bail!(
            "incomplete Lighter credentials: set {api_key_var}, {api_secret_var}, and {account_index_var}"
        ),
    }
}

fn resolve_api_key_index(value: Option<u8>, env_var: &str) -> anyhow::Result<Option<u8>> {
    match value {
        Some(value) => ensure_api_key_index(value).map(Some),
        None => get_or_env_var_opt(None::<String>, env_var)
            .filter(|s| !s.trim().is_empty())
            .map(|s| parse_api_key_index(&s, env_var))
            .transpose(),
    }
}

fn resolve_account_index(value: Option<u64>, env_var: &str) -> anyhow::Result<Option<u64>> {
    match value {
        Some(value) => Ok(Some(value)),
        None => get_or_env_var_opt(None::<String>, env_var)
            .filter(|s| !s.trim().is_empty())
            .map(|s| {
                s.trim()
                    .parse::<u64>()
                    .with_context(|| format!("{env_var} must be an unsigned integer"))
            })
            .transpose(),
    }
}

fn parse_api_key_index(value: &str, env_var: &str) -> anyhow::Result<u8> {
    let index = value
        .trim()
        .parse::<u8>()
        .with_context(|| format!("{env_var} must be an API key index in 0..=254"))?;
    ensure_api_key_index(index)
}

fn ensure_api_key_index(value: u8) -> anyhow::Result<u8> {
    anyhow::ensure!(value <= 254, "Lighter API key index must be in 0..=254");
    Ok(value)
}

fn decode_private_key_hex(value: &str) -> anyhow::Result<Vec<u8>> {
    let value = value.trim();
    let hex = value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
        .unwrap_or(value);
    let bytes = hex::decode(hex).context("Lighter API secret must be valid hex")?;
    anyhow::ensure!(
        bytes.len() == SCALAR_BYTES,
        "Lighter API secret must be a 40-byte hex private key"
    );
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    const PRIVATE_KEY_HEX: &str =
        "0b8e0f63c24d8baacd9d29ad4e9a4b73c4a8d2bb8b16dc4fa9d7c2e1d3a8b1f0e8d3a4c5b6e7f001";

    #[rstest]
    fn test_credential_env_vars_mainnet() {
        assert_eq!(
            credential_env_vars(LighterEnvironment::Mainnet),
            (
                "LIGHTER_API_KEY_INDEX",
                "LIGHTER_API_SECRET",
                "LIGHTER_ACCOUNT_INDEX"
            ),
        );
    }

    #[rstest]
    fn test_credential_env_vars_testnet() {
        assert_eq!(
            credential_env_vars(LighterEnvironment::Testnet),
            (
                "LIGHTER_TESTNET_API_KEY_INDEX",
                "LIGHTER_TESTNET_API_SECRET",
                "LIGHTER_TESTNET_ACCOUNT_INDEX"
            ),
        );
    }

    #[rstest]
    fn test_resolve_with_config_values() {
        let credential = Credential::resolve(
            Some(PRIVATE_KEY_HEX.to_string()),
            Some(12_345),
            Some(4),
            LighterEnvironment::Mainnet,
        )
        .unwrap()
        .unwrap();

        assert_eq!(credential.api_key_index(), 4);
        assert_eq!(credential.account_index(), 12_345);
        assert!(credential.private_key().is_ok());
    }

    #[rstest]
    fn test_credential_from_resolved_values() {
        let credential = credential_from_resolved_values(
            Some(4),
            Some(12_345),
            Some(PRIVATE_KEY_HEX.to_string()),
            LIGHTER_API_KEY_INDEX_VAR,
            LIGHTER_API_SECRET_VAR,
            LIGHTER_ACCOUNT_INDEX_VAR,
        )
        .unwrap()
        .unwrap();

        assert_eq!(credential.api_key_index(), 4);
        assert_eq!(credential.account_index(), 12_345);
        assert!(credential.private_key().is_ok());
    }

    #[rstest]
    fn test_credential_from_resolved_values_rejects_partial_values() {
        let err = credential_from_resolved_values(
            Some(4),
            None,
            Some(PRIVATE_KEY_HEX.to_string()),
            LIGHTER_API_KEY_INDEX_VAR,
            LIGHTER_API_SECRET_VAR,
            LIGHTER_ACCOUNT_INDEX_VAR,
        )
        .unwrap_err();

        assert!(err.to_string().contains("incomplete Lighter credentials"));
    }

    #[rstest]
    fn test_resolve_rejects_invalid_api_secret() {
        let err = Credential::resolve(
            Some("not-hex".to_string()),
            Some(12_345),
            Some(4),
            LighterEnvironment::Mainnet,
        )
        .unwrap_err();

        assert!(err.to_string().contains("valid hex"));
    }

    #[rstest]
    fn test_private_key_accepts_prefixed_hex() {
        let lower_prefixed = format!("0x{PRIVATE_KEY_HEX}");
        let upper_prefixed = format!("0X{PRIVATE_KEY_HEX}");

        let lower = Credential::new(4, lower_prefixed, 12_345).unwrap();
        let upper = Credential::new(4, upper_prefixed, 12_345).unwrap();

        assert!(lower.private_key().is_ok());
        assert!(upper.private_key().is_ok());
    }

    #[rstest]
    fn test_debug_redacts_api_secret() {
        let credential = Credential::new(4, PRIVATE_KEY_HEX, 12_345).unwrap();

        let dbg_out = format!("{credential:?}");

        assert!(dbg_out.contains(REDACTED));
        assert!(!dbg_out.contains(PRIVATE_KEY_HEX));
    }

    #[rstest]
    #[case::no_auth("no auth here", "no auth here")]
    #[case::short_token("auth=abc", "auth=***")]
    #[case::long_token("auth=abcdefghijklmnop", "auth=abcd...mnop")]
    #[case::url_with_ampersand("url?auth=abcdefghijklmnop&other=x", "url?auth=abcd...mnop&other=x")]
    #[case::empty_token_value("url?auth=&other=x", "url?auth=&other=x")]
    #[case::multiple_auth(
        "first auth=token1 mid auth=token2 end",
        "first auth=****** mid auth=****** end"
    )]
    #[case::trailing_whitespace("auth=tok end", "auth=*** end")]
    #[case::newline_boundary(
        "first auth=token1\nsecond auth=token2",
        "first auth=******\nsecond auth=******"
    )]
    fn scrub_auth_redacts_token(#[case] input: &str, #[case] expected: &str) {
        assert_eq!(scrub_auth(input), expected);
    }

    #[rstest]
    fn scrub_auth_empty_input_returns_empty() {
        assert_eq!(scrub_auth(""), "");
    }

    // Tests that observe the env-var fallback live in the workspace `serial_tests`
    // group (see `.config/nextest.toml`) so env-var mutation is pinned to a single
    // thread.
    #[allow(unsafe_code)] // env-var mutation in tests; restored via `EnvGuard`.
    mod serial_tests {
        use super::*;

        const LIGHTER_ENV_VARS: &[&str] = &[
            "LIGHTER_API_KEY_INDEX",
            "LIGHTER_API_SECRET",
            "LIGHTER_ACCOUNT_INDEX",
            "LIGHTER_TESTNET_API_KEY_INDEX",
            "LIGHTER_TESTNET_API_SECRET",
            "LIGHTER_TESTNET_ACCOUNT_INDEX",
        ];

        /// Snapshots and clears the Lighter credential env vars, restoring the
        /// original values on drop.
        struct EnvGuard {
            saved: Vec<(&'static str, Option<String>)>,
        }

        impl EnvGuard {
            fn clear_lighter() -> Self {
                let saved = LIGHTER_ENV_VARS
                    .iter()
                    .map(|&name| (name, std::env::var(name).ok()))
                    .collect::<Vec<_>>();
                for &(name, _) in &saved {
                    // SAFETY: the `serial_tests` nextest group serializes these
                    // tests, and no other lighter test reads or writes the
                    // LIGHTER_* env vars concurrently.
                    unsafe { std::env::remove_var(name) };
                }
                Self { saved }
            }
        }

        impl Drop for EnvGuard {
            fn drop(&mut self) {
                for (name, original) in &self.saved {
                    match original {
                        // SAFETY: see `EnvGuard::clear_lighter`.
                        Some(value) => unsafe { std::env::set_var(name, value) },
                        None => unsafe { std::env::remove_var(name) },
                    }
                }
            }
        }

        #[rstest]
        #[case::empty("")]
        #[case::whitespace("   ")]
        fn resolve_blank_private_key_falls_back_to_env_secret(#[case] blank: &str) {
            let _guard = EnvGuard::clear_lighter();
            // SAFETY: see `EnvGuard::clear_lighter`; the guard restores on drop.
            unsafe { std::env::set_var("LIGHTER_API_SECRET", PRIVATE_KEY_HEX) };

            let credential = Credential::resolve(
                Some(blank.to_string()),
                Some(12_345),
                Some(4),
                LighterEnvironment::Mainnet,
            )
            .unwrap()
            .unwrap();

            let from_env = Credential::new(4, PRIVATE_KEY_HEX, 12_345).unwrap();
            assert_eq!(
                credential.private_key().unwrap().to_le_bytes(),
                from_env.private_key().unwrap().to_le_bytes(),
            );
        }

        #[rstest]
        fn resolve_blank_private_key_without_env_returns_none() {
            let _guard = EnvGuard::clear_lighter();

            let resolved = Credential::resolve(
                Some("   ".to_string()),
                None,
                None,
                LighterEnvironment::Mainnet,
            )
            .unwrap();

            assert!(resolved.is_none());
        }

        #[rstest]
        fn resolve_blank_env_secret_returns_none() {
            let _guard = EnvGuard::clear_lighter();
            // SAFETY: see `EnvGuard::clear_lighter`; the guard restores on drop.
            unsafe { std::env::set_var("LIGHTER_API_SECRET", "   ") };

            let resolved =
                Credential::resolve(None, None, None, LighterEnvironment::Mainnet).unwrap();

            assert!(resolved.is_none());
        }

        #[rstest]
        fn resolve_prefers_non_blank_config_over_env_secret() {
            let _guard = EnvGuard::clear_lighter();
            // An invalid env secret: resolution succeeds only if the config value wins.
            // SAFETY: see `EnvGuard::clear_lighter`.
            unsafe { std::env::set_var("LIGHTER_API_SECRET", "not-hex") };

            let credential = Credential::resolve(
                Some(PRIVATE_KEY_HEX.to_string()),
                Some(12_345),
                Some(4),
                LighterEnvironment::Mainnet,
            )
            .unwrap()
            .unwrap();

            assert!(credential.private_key().is_ok());
        }
    }
}
