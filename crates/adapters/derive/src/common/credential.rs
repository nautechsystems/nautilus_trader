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

//! Derive credential storage.
//!
//! Derive identifies a trading account through a three-part tuple rather than a
//! single API key:
//!
//! 1. `wallet_address`: the owner's EOA or multisig address, sent in the
//!    `X-DeriveWallet` header and the `owner` slot of every signed action.
//! 2. `session_key`: a secp256k1 private key registered to the wallet. Signs
//!    REST/WS auth headers and EIP-712 typed-data actions. May be the owner
//!    EOA's key but is more commonly a scoped session key.
//! 3. `subaccount_id`: per-wallet integer slot that holds the positions and
//!    signs each `private/order` request.
//!
//! # Credential resolution
//!
//! Credentials are resolved in the following priority order:
//!
//! 1. Explicit values from config
//! 2. `DERIVE_WALLET_ADDRESS` / `DERIVE_SESSION_PRIVATE_KEY` / `DERIVE_SUBACCOUNT_ID`
//!    env vars (or the `_TESTNET_` variants when targeting testnet)
//!
//! The session-key bytes are zeroized on drop.

use std::fmt::{Debug, Display};

use anyhow::Context;
use nautilus_core::{
    env::{get_or_env_var, get_or_env_var_opt},
    string::secret::REDACTED,
};
use zeroize::{Zeroize, ZeroizeOnDrop, Zeroizing};

use crate::common::enums::DeriveEnvironment;

/// Returns the environment-variable triple `(wallet, session_key, subaccount)`
/// for the given environment.
#[must_use]
pub fn credential_env_vars(
    environment: DeriveEnvironment,
) -> (&'static str, &'static str, &'static str) {
    match environment {
        DeriveEnvironment::Mainnet => (
            "DERIVE_WALLET_ADDRESS",
            "DERIVE_SESSION_PRIVATE_KEY",
            "DERIVE_SUBACCOUNT_ID",
        ),
        DeriveEnvironment::Testnet => (
            "DERIVE_TESTNET_WALLET_ADDRESS",
            "DERIVE_TESTNET_SESSION_PRIVATE_KEY",
            "DERIVE_TESTNET_SUBACCOUNT_ID",
        ),
    }
}

/// Owner wallet + session-key + subaccount triple.
#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct DeriveCredential {
    wallet_address: String,
    session_key: String,
    #[zeroize(skip)]
    subaccount_id: u64,
}

impl DeriveCredential {
    /// Creates a new [`DeriveCredential`] instance.
    #[must_use]
    pub fn new(wallet_address: String, session_key: String, subaccount_id: u64) -> Self {
        Self {
            wallet_address,
            session_key,
            subaccount_id,
        }
    }

    /// Returns the owner's EOA or multisig address (`X-DeriveWallet`).
    #[must_use]
    pub fn wallet_address(&self) -> &str {
        &self.wallet_address
    }

    /// Returns the secp256k1 session-key private key (hex-encoded).
    #[must_use]
    pub fn session_key(&self) -> &str {
        &self.session_key
    }

    /// Returns the subaccount integer ID.
    #[must_use]
    pub const fn subaccount_id(&self) -> u64 {
        self.subaccount_id
    }

    /// Resolves a [`DeriveCredential`] from explicit values, falling back to
    /// the documented environment variables when fields are unset.
    ///
    /// Resolution order per field is: explicit value, then env var. The env
    /// var name set is selected by `environment` via [`credential_env_vars`].
    ///
    /// # Errors
    ///
    /// Returns an error when any of the wallet address, session key, or
    /// subaccount ID cannot be resolved from either source, or when the
    /// subaccount id env var is not a valid `u64`.
    pub fn resolve(
        wallet_address: Option<String>,
        session_key: Option<String>,
        subaccount_id: Option<u64>,
        environment: DeriveEnvironment,
    ) -> anyhow::Result<Self> {
        let session_key = session_key.map(Zeroizing::new);
        let (wallet_var, key_var, subaccount_var) = credential_env_vars(environment);

        let wallet_address = get_or_env_var(wallet_address, wallet_var).with_context(|| {
            format!("Derive wallet address missing (set {wallet_var} or config)")
        })?;

        let mut session_key = match session_key {
            Some(key) => key,
            None => Zeroizing::new(get_or_env_var(None, key_var).with_context(|| {
                format!("Derive session key missing (set {key_var} or config)")
            })?),
        };

        let subaccount_id = match subaccount_id {
            Some(id) => id,
            None => get_or_env_var_opt(None, subaccount_var)
                .with_context(|| {
                    format!("Derive subaccount id missing (set {subaccount_var} or config)")
                })?
                .parse::<u64>()
                .with_context(|| format!("failed to parse {subaccount_var} as u64"))?,
        };

        Ok(Self::new(
            wallet_address,
            std::mem::take(&mut *session_key),
            subaccount_id,
        ))
    }
}

impl Debug for DeriveCredential {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(DeriveCredential))
            .field("wallet_address", &self.wallet_address)
            .field("session_key", &REDACTED)
            .field("subaccount_id", &self.subaccount_id)
            .finish()
    }
}

impl Display for DeriveCredential {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "DeriveCredential(wallet={}, subaccount={})",
            self.wallet_address, self.subaccount_id
        )
    }
}

#[cfg(test)]
mod tests {
    use std::{env, process::Command};

    use rstest::rstest;

    use super::*;

    const TEST_WALLET: &str = "0x0000000000000000000000000000000000001234";
    const TEST_SESSION_KEY: &str =
        "0x2ae8be44db8a590d20bffbe3b6872df9b569147d3bf6801a35a28281a4816bbd";
    const TEST_SUBACCOUNT: u64 = 30769;

    #[rstest]
    fn test_credential_debug_redacts_session_key() {
        let cred = DeriveCredential::new(
            TEST_WALLET.to_string(),
            TEST_SESSION_KEY.to_string(),
            TEST_SUBACCOUNT,
        );
        let debug = format!("{cred:?}");
        assert!(debug.contains("redacted"));
        assert!(!debug.contains(TEST_SESSION_KEY));
        assert!(debug.contains(TEST_WALLET));
        assert!(debug.contains(&TEST_SUBACCOUNT.to_string()));
    }

    #[rstest]
    fn test_credential_display_omits_session_key() {
        let cred = DeriveCredential::new(
            TEST_WALLET.to_string(),
            TEST_SESSION_KEY.to_string(),
            TEST_SUBACCOUNT,
        );
        let display = format!("{cred}");
        assert!(display.contains(TEST_WALLET));
        assert!(!display.contains(TEST_SESSION_KEY));
    }

    #[rstest]
    fn test_credential_env_vars_for_mainnet() {
        let (wallet, key, sub) = credential_env_vars(DeriveEnvironment::Mainnet);
        assert_eq!(wallet, "DERIVE_WALLET_ADDRESS");
        assert_eq!(key, "DERIVE_SESSION_PRIVATE_KEY");
        assert_eq!(sub, "DERIVE_SUBACCOUNT_ID");
    }

    #[rstest]
    fn test_credential_env_vars_for_testnet() {
        let (wallet, key, sub) = credential_env_vars(DeriveEnvironment::Testnet);
        assert_eq!(wallet, "DERIVE_TESTNET_WALLET_ADDRESS");
        assert_eq!(key, "DERIVE_TESTNET_SESSION_PRIVATE_KEY");
        assert_eq!(sub, "DERIVE_TESTNET_SUBACCOUNT_ID");
    }

    #[rstest]
    fn test_credential_accessors() {
        let cred = DeriveCredential::new(
            TEST_WALLET.to_string(),
            TEST_SESSION_KEY.to_string(),
            TEST_SUBACCOUNT,
        );
        assert_eq!(cred.wallet_address(), TEST_WALLET);
        assert_eq!(cred.session_key(), TEST_SESSION_KEY);
        assert_eq!(cred.subaccount_id(), TEST_SUBACCOUNT);
    }

    #[rstest]
    fn test_resolve_prefers_explicit_values() {
        let cred = DeriveCredential::resolve(
            Some(TEST_WALLET.to_string()),
            Some(TEST_SESSION_KEY.to_string()),
            Some(TEST_SUBACCOUNT),
            DeriveEnvironment::Testnet,
        )
        .unwrap();
        assert_eq!(cred.wallet_address(), TEST_WALLET);
        assert_eq!(cred.session_key(), TEST_SESSION_KEY);
        assert_eq!(cred.subaccount_id(), TEST_SUBACCOUNT);
    }

    #[rstest]
    fn test_resolve_environment_and_incomplete_credentials() {
        const CASE_VAR: &str = "NAUTILUS_DERIVE_CREDENTIAL_TEST_CASE";
        let environments = [DeriveEnvironment::Mainnet, DeriveEnvironment::Testnet];
        let cases = [
            "fallback",
            "explicit",
            "mixed",
            "wallet",
            "key",
            "subaccount",
            "invalid",
        ];

        let Ok(case) = env::var(CASE_VAR) else {
            for (index, environment) in environments.into_iter().enumerate() {
                for case in cases {
                    let mut command = Command::new(env::current_exe().unwrap());
                    command.args([
                        "--exact",
                        "common::credential::tests::test_resolve_environment_and_incomplete_credentials",
                        "--nocapture",
                    ]);

                    for (other_index, other) in environments.into_iter().enumerate() {
                        let (wallet, key, subaccount) = credential_env_vars(other);
                        command
                            .env(wallet, format!("wallet-{other_index}"))
                            .env(key, format!("key-{other_index}"))
                            .env(subaccount, (41 + other_index).to_string());
                    }

                    let (wallet, key, subaccount) = credential_env_vars(environment);

                    match case {
                        "wallet" => {
                            command.env_remove(wallet);
                        }
                        "key" => {
                            command.env_remove(key);
                        }
                        "subaccount" => {
                            command.env_remove(subaccount);
                        }
                        "invalid" => {
                            command.env(subaccount, "invalid-subaccount");
                        }
                        _ => {}
                    }

                    let output = command
                        .env(CASE_VAR, format!("{index}:{case}"))
                        .output()
                        .unwrap();

                    assert!(
                        output.status.success(),
                        "credential case {index}:{case} failed: {}{}",
                        String::from_utf8_lossy(&output.stdout),
                        String::from_utf8_lossy(&output.stderr),
                    );
                }
            }

            return;
        };

        let (index, case) = case.split_once(':').unwrap();
        let index: usize = index.parse().unwrap();
        let environment = environments[index];
        let explicit = case == "explicit";
        let result = DeriveCredential::resolve(
            (explicit || case == "mixed").then(|| TEST_WALLET.to_string()),
            explicit.then(|| TEST_SESSION_KEY.to_string()),
            explicit.then_some(TEST_SUBACCOUNT),
            environment,
        );

        if matches!(case, "fallback" | "explicit" | "mixed") {
            let credential = result.unwrap();
            assert_eq!(
                credential.wallet_address(),
                if explicit || case == "mixed" {
                    TEST_WALLET.to_string()
                } else {
                    format!("wallet-{index}")
                }
            );
            assert_eq!(
                credential.session_key(),
                if explicit {
                    TEST_SESSION_KEY.to_string()
                } else {
                    format!("key-{index}")
                }
            );
            assert_eq!(
                credential.subaccount_id(),
                if explicit {
                    TEST_SUBACCOUNT
                } else {
                    41 + index as u64
                }
            );
        } else {
            let (wallet, key, subaccount) = credential_env_vars(environment);

            let expected = match case {
                "wallet" => format!("Derive wallet address missing (set {wallet} or config)"),
                "key" => format!("Derive session key missing (set {key} or config)"),
                "subaccount" => {
                    format!("Derive subaccount id missing (set {subaccount} or config)")
                }
                "invalid" => format!("failed to parse {subaccount} as u64"),
                _ => unreachable!(),
            };

            assert_eq!(result.unwrap_err().to_string(), expected);
        }
    }

    #[rstest]
    fn test_credential_zeroizes_secret_fields() {
        fn requires_zeroize_on_drop<T: ZeroizeOnDrop>() {}
        requires_zeroize_on_drop::<DeriveCredential>();

        let mut credential = DeriveCredential::new(
            TEST_WALLET.to_string(),
            TEST_SESSION_KEY.to_string(),
            TEST_SUBACCOUNT,
        );
        credential.zeroize();

        assert_eq!(credential.wallet_address(), "");
        assert_eq!(credential.session_key(), "");
        assert_eq!(credential.subaccount_id(), TEST_SUBACCOUNT);
    }
}
