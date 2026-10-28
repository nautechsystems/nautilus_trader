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

use alloy_primitives::Address;

use crate::{
    common::{
        credential::{EvmPrivateKey, VaultAddress, credential_env_vars},
        enums::HyperliquidEnvironment,
    },
    http::error::{Error, Result},
    signing::HyperliquidEip712Signer,
};

/// Resolves the execution account address used for REST queries and WebSocket subscriptions.
///
/// Precedence is explicit account address, explicit vault address, environment account address,
/// environment vault address, then the signer address derived from the private key.
/// Explicit and environment account addresses are trimmed, validated as `0x`-prefixed lowercase
/// or EIP-55 checksummed hex, and normalized to lowercase. A blank explicit account address is
/// invalid; a blank environment account address is ignored.
///
/// # Errors
///
/// Returns an error if a selected account address, vault address, or private key is invalid.
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
    if let Some(address) = account_address {
        return normalize_account_address(address).map(Some);
    }

    if let Some(address) = trim_nonempty(vault_address) {
        return VaultAddress::parse(&address).map(|addr| Some(addr.to_hex()));
    }

    if let Some(address) = trim_nonempty(env_account_address) {
        return normalize_account_address(&address).map(Some);
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

pub(crate) fn normalize_account_address(value: &str) -> Result<String> {
    let value = value.trim();
    let address = value
        .parse::<Address>()
        .map_err(|_| Error::bad_request("Account address must be 20 bytes of valid hex"))?;
    let normalized = format!("{address:#x}");
    if value != normalized && value != address.to_checksum(None) {
        return Err(Error::bad_request(
            "Account address must be 0x-prefixed lowercase or EIP-55 checksummed hex",
        ));
    }

    Ok(normalized)
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

    #[rstest]
    #[case("0xd8da6bf26964af9d7eed9e03e53415d37aa96045")]
    #[case("0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045")]
    #[case(" \t0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045\n")]
    fn test_resolve_execution_account_address_normalizes(#[case] address: &str) {
        let explicit = resolve_execution_account_address_from_values(
            Some(PRIVATE_KEY),
            None,
            Some(address),
            None,
            None,
            None,
        )
        .unwrap();
        let from_env = resolve_execution_account_address_from_values(
            Some(PRIVATE_KEY),
            None,
            None,
            None,
            None,
            Some(address),
        )
        .unwrap();

        assert_eq!(
            explicit.as_deref(),
            Some("0xd8da6bf26964af9d7eed9e03e53415d37aa96045")
        );
        assert_eq!(from_env, explicit);
    }

    #[rstest]
    #[case("")]
    #[case(" \t\n")]
    #[case("0xd8da6bf26964af9d7eed9e03e53415d37aa9604")]
    #[case("0xd8da6bf26964af9d7eed9e03e53415d37aa960450")]
    #[case("0xd8da6bf26964af9d7eed9e03e53415d37aa9604g")]
    #[case("0xd8da6bf26964af9d7eed9e03e53415d37aa9604é")]
    fn test_resolve_execution_account_address_rejects_invalid_hex(#[case] address: &str) {
        let e = resolve_execution_account_address_from_values(
            Some(PRIVATE_KEY),
            Some(EXPLICIT_VAULT),
            Some(address),
            None,
            None,
            Some(ENV_ACCOUNT),
        )
        .unwrap_err();

        assert_eq!(
            e.to_string(),
            "bad request: Account address must be 20 bytes of valid hex"
        );
    }

    #[rstest]
    #[case("d8da6bf26964af9d7eed9e03e53415d37aa96045")]
    #[case("0Xd8da6bf26964af9d7eed9e03e53415d37aa96045")]
    #[case("0xd8Da6BF26964aF9D7eEd9e03E53415D37aA96045")]
    #[case("0xD8DA6BF26964AF9D7EED9E03E53415D37AA96045")]
    fn test_resolve_execution_account_address_rejects_invalid_format(#[case] address: &str) {
        let e = resolve_execution_account_address_from_values(
            Some(PRIVATE_KEY),
            Some(EXPLICIT_VAULT),
            Some(address),
            None,
            None,
            Some(ENV_ACCOUNT),
        )
        .unwrap_err();

        assert_eq!(
            e.to_string(),
            "bad request: Account address must be 0x-prefixed lowercase or EIP-55 checksummed hex"
        );
    }

    #[rstest]
    fn test_resolve_execution_account_address_rejects_invalid_env_account() {
        let e = resolve_execution_account_address_from_values(
            Some(PRIVATE_KEY),
            None,
            None,
            None,
            Some(ENV_VAULT),
            Some("0xinvalid"),
        )
        .unwrap_err();

        assert_eq!(
            e.to_string(),
            "bad request: Account address must be 20 bytes of valid hex"
        );
    }

    #[rstest]
    #[case(None, None, None)]
    #[case(
        Some(PRIVATE_KEY),
        None,
        Some("0xc96aaa54e2d44c299564da76e1cd3184a2386b8d")
    )]
    #[case(
        Some(PRIVATE_KEY),
        Some(ENV_VAULT),
        Some("0xdddddddddddddddddddddddddddddddddddddddd")
    )]
    fn test_resolve_execution_account_address_ignores_empty_env_account(
        #[case] private_key: Option<&str>,
        #[case] vault_address: Option<&str>,
        #[case] expected: Option<&str>,
    ) {
        let result = resolve_execution_account_address_from_values(
            None,
            None,
            None,
            private_key,
            vault_address,
            Some(" \t\n"),
        )
        .unwrap();

        assert_eq!(result.as_deref(), expected);
    }
}
