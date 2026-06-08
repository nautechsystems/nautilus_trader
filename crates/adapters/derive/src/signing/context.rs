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

//! Execution signing context resolution for Derive.

use alloy::signers::local::PrivateKeySigner;
use alloy_primitives::{Address, B256};
use anyhow::Context;
use rust_decimal::Decimal;

use crate::{
    common::{
        consts::{ACTION_TYPEHASH, domain_separator_for, trade_module_address_for},
        credential::DeriveCredential,
    },
    config::DeriveExecClientConfig,
    signing::encoding::{parse_address_const, parse_b256_const},
};

#[derive(Debug, Clone)]
pub(crate) struct SigningContext {
    pub(crate) wallet_address: Address,
    pub(crate) signer: PrivateKeySigner,
    pub(crate) subaccount_id: u64,
    pub(crate) domain_separator: B256,
    pub(crate) action_typehash: B256,
    pub(crate) trade_module_address: Address,
    pub(crate) signature_expiry_secs: u64,
    pub(crate) max_fee_per_contract: Decimal,
    pub(crate) market_order_slippage_bps: u32,
}

pub(crate) fn resolve_signing_context(
    credential: &DeriveCredential,
    config: &DeriveExecClientConfig,
) -> anyhow::Result<SigningContext> {
    let wallet_address: Address = credential
        .wallet_address()
        .parse()
        .with_context(|| format!("invalid wallet address `{}`", credential.wallet_address()))?;
    let signer: PrivateKeySigner = credential
        .session_key()
        .parse()
        .with_context(|| "invalid session key (failed to parse as secp256k1 private key)")?;

    let domain_str = config
        .domain_separator
        .clone()
        .unwrap_or_else(|| domain_separator_for(config.environment).to_string());
    let domain_separator = parse_b256_const(&domain_str, "domain_separator")?;

    let typehash_str = config
        .action_typehash
        .clone()
        .unwrap_or_else(|| ACTION_TYPEHASH.to_string());
    let action_typehash = parse_b256_const(&typehash_str, "action_typehash")?;

    let module_str = config
        .trade_module_address
        .clone()
        .unwrap_or_else(|| trade_module_address_for(config.environment).to_string());
    let trade_module_address = parse_address_const(&module_str, "trade_module_address")?;

    let max_fee_per_contract = config.max_fee_per_contract.unwrap_or(Decimal::ZERO);

    Ok(SigningContext {
        wallet_address,
        signer,
        subaccount_id: credential.subaccount_id(),
        domain_separator,
        action_typehash,
        trade_module_address,
        signature_expiry_secs: config.signature_expiry_secs,
        max_fee_per_contract,
        market_order_slippage_bps: config.market_order_slippage_bps,
    })
}
