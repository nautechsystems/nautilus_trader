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

//! Account-address resolution for Hyperliquid execution queries and subscriptions.

use std::env;

use crate::{
    common::{
        credential::{EvmPrivateKey, VaultAddress, credential_env_vars},
        enums::HyperliquidEnvironment,
    },
    http::error::Result,
    signing::HyperliquidEip712Signer,
};

/// Resolves the execution account address used for REST queries and WebSocket subscriptions.
///
/// Precedence is explicit account address, explicit vault address, environment account address,
/// environment vault address, then the signer address derived from the private key.
///
/// # Errors
///
/// Returns an error if a selected vault address or private key is invalid.
pub fn resolve_execution_account_address(
    private_key: Option<&str>,
    vault_address: Option<&str>,
    account_address: Option<&str>,
    environment: HyperliquidEnvironment,
) -> Result<Option<String>> {
    let (pk_env_var, vault_env_var) = credential_env_vars(environment);
    let env_private_key = env::var(pk_env_var).ok();
    let env_vault_address = env::var(vault_env_var).ok();
    let env_account_address = env::var("HYPERLIQUID_ACCOUNT_ADDRESS").ok();

    resolve_execution_account_address_from_values(
        private_key,
        vault_address,
        account_address,
        env_private_key.as_deref(),
        env_vault_address.as_deref(),
        env_account_address.as_deref(),
    )
}

fn resolve_execution_account_address_from_values(
    private_key: Option<&str>,
    vault_address: Option<&str>,
    account_address: Option<&str>,
    env_private_key: Option<&str>,
    env_vault_address: Option<&str>,
    env_account_address: Option<&str>,
) -> Result<Option<String>> {
    if let Some(address) = trim_nonempty(account_address) {
        return Ok(Some(address));
    }

    if let Some(address) = trim_nonempty(vault_address) {
        return VaultAddress::parse(&address).map(|addr| Some(addr.to_hex()));
    }

    if let Some(address) = trim_nonempty(env_account_address) {
        return Ok(Some(address));
    }

    if let Some(address) = trim_nonempty(env_vault_address) {
        return VaultAddress::parse(&address).map(|addr| Some(addr.to_hex()));
    }

    let Some(private_key) = trim_nonempty(private_key).or_else(|| trim_nonempty(env_private_key))
    else {
        return Ok(None);
    };

    let private_key = EvmPrivateKey::new(&private_key)?;
    HyperliquidEip712Signer::new(&private_key)?
        .address()
        .map(Some)
}

fn trim_nonempty(value: Option<&str>) -> Option<String> {
    let value = value?.trim();
    (!value.is_empty()).then(|| value.to_string())
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::resolve_execution_account_address_from_values;

    const PRIVATE_KEY: &str = "0xdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef";
    const EXPLICIT_ACCOUNT: &str = " 0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa ";
    const EXPLICIT_VAULT: &str = " 0xbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb ";
    const ENV_ACCOUNT: &str = " 0xcccccccccccccccccccccccccccccccccccccccc ";
    const ENV_VAULT: &str = " 0xdddddddddddddddddddddddddddddddddddddddd ";

    #[rstest]
    #[case(
        Some(PRIVATE_KEY),
        Some(EXPLICIT_VAULT),
        Some(EXPLICIT_ACCOUNT),
        Some(PRIVATE_KEY),
        Some(ENV_VAULT),
        Some(ENV_ACCOUNT),
        Some("0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
    )]
    #[case(
        Some(PRIVATE_KEY),
        Some(EXPLICIT_VAULT),
        None,
        Some(PRIVATE_KEY),
        Some(ENV_VAULT),
        Some(ENV_ACCOUNT),
        Some("0xbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb")
    )]
    #[case(
        Some(PRIVATE_KEY),
        None,
        None,
        Some(PRIVATE_KEY),
        Some(ENV_VAULT),
        Some(ENV_ACCOUNT),
        Some("0xcccccccccccccccccccccccccccccccccccccccc")
    )]
    #[case(
        Some(PRIVATE_KEY),
        None,
        None,
        Some(PRIVATE_KEY),
        Some(ENV_VAULT),
        None,
        Some("0xdddddddddddddddddddddddddddddddddddddddd")
    )]
    #[case(None, None, None, None, None, None, None)]
    fn test_resolve_execution_account_address_precedence(
        #[case] private_key: Option<&str>,
        #[case] vault_address: Option<&str>,
        #[case] account_address: Option<&str>,
        #[case] env_private_key: Option<&str>,
        #[case] env_vault_address: Option<&str>,
        #[case] env_account_address: Option<&str>,
        #[case] expected: Option<&str>,
    ) {
        let result = resolve_execution_account_address_from_values(
            private_key,
            vault_address,
            account_address,
            env_private_key,
            env_vault_address,
            env_account_address,
        )
        .unwrap();

        assert_eq!(result.as_deref(), expected);
    }

    #[rstest]
    fn test_resolve_execution_account_address_uses_signer_fallback() {
        let result = resolve_execution_account_address_from_values(
            Some(PRIVATE_KEY),
            None,
            None,
            None,
            None,
            None,
        )
        .unwrap();

        assert_eq!(
            result.as_deref(),
            Some("0xc96aaa54e2d44c299564da76e1cd3184a2386b8d"),
        );
    }

    #[rstest]
    fn test_resolve_execution_account_address_uses_env_private_key_fallback() {
        let result = resolve_execution_account_address_from_values(
            None,
            None,
            None,
            Some(PRIVATE_KEY),
            None,
            None,
        )
        .unwrap();

        assert_eq!(
            result.as_deref(),
            Some("0xc96aaa54e2d44c299564da76e1cd3184a2386b8d"),
        );
    }

    #[rstest]
    fn test_resolve_execution_account_address_rejects_invalid_selected_vault() {
        let err = resolve_execution_account_address_from_values(
            Some(PRIVATE_KEY),
            Some("0xinvalid"),
            None,
            None,
            None,
            Some(ENV_ACCOUNT),
        )
        .unwrap_err();

        assert!(
            err.to_string()
                .contains("Vault address must be 20 bytes of valid hex"),
            "unexpected error: {err}",
        );
    }
}
