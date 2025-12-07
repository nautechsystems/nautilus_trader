// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

//! Transaction builder for dYdX v4 protocol.
//!
//! This module provides utilities for building and signing Cosmos SDK transactions
//! for the dYdX v4 protocol, including support for permissioned key trading via
//! authenticators.
//!
//! # Permissioned Keys
//!
//! dYdX supports permissioned keys (authenticators) that allow an account to add
//! custom logic for verifying and confirming transactions. This enables features like:
//!
//! - Delegated signing keys for sub-accounts
//! - Separated hot/cold wallet architectures
//! - Trading key separation from withdrawal keys
//!
//! See <https://docs.dydx.xyz/concepts/trading/authenticators> for details.

use std::fmt::{Debug, Formatter};

use cosmrs::{
    Any, Coin,
    tx::{self, Fee, SignDoc, SignerInfo},
};
use dydx_proto::{ToAny, dydxprotocol::accountplus::TxExtension};
use rust_decimal::{Decimal, prelude::ToPrimitive};

use super::{types::ChainId, wallet::Account};

/// Gas adjustment value to avoid rejected transactions caused by gas underestimation.
const GAS_MULTIPLIER: f64 = 1.8;

/// Transaction builder.
///
/// Handles fee calculation, transaction construction, and signing.
pub struct TxBuilder {
    chain_id: cosmrs::tendermint::chain::Id,
    fee_denom: String,
}

impl TxBuilder {
    /// Create a new transaction builder.
    ///
    /// # Errors
    ///
    /// Returns an error if the chain ID cannot be converted.
    pub fn new(chain_id: ChainId, fee_denom: String) -> Result<Self, anyhow::Error> {
        Ok(Self {
            chain_id: chain_id.try_into()?,
            fee_denom,
        })
    }

    /// Estimate a transaction fee.
    ///
    /// See also [What Are Crypto Gas Fees?](https://dydx.exchange/crypto-learning/what-are-crypto-gas-fees).
    ///
    /// # Errors
    ///
    /// Returns an error if fee calculation fails.
    pub fn calculate_fee(&self, gas_used: Option<u64>) -> Result<Fee, anyhow::Error> {
        if let Some(gas) = gas_used {
            self.calculate_fee_from_gas(gas)
        } else {
            Ok(Self::default_fee())
        }
    }

    /// Calculate fee from gas usage.
    fn calculate_fee_from_gas(&self, gas_used: u64) -> Result<Fee, anyhow::Error> {
        let gas_multiplier = Decimal::try_from(GAS_MULTIPLIER)?;
        let gas_limit = Decimal::from(gas_used) * gas_multiplier;

        // Gas price for dYdX (typically 0.025 adydx per gas)
        let gas_price = Decimal::new(25, 3); // 0.025
        let amount = (gas_price * gas_limit).ceil();

        let gas_limit_u64 = gas_limit
            .to_u64()
            .ok_or_else(|| anyhow::anyhow!("Failed converting gas limit to u64"))?;

        let amount_u128 = amount
            .to_u128()
            .ok_or_else(|| anyhow::anyhow!("Failed converting gas cost to u128"))?;

        Ok(Fee::from_amount_and_gas(
            Coin {
                amount: amount_u128,
                denom: self
                    .fee_denom
                    .parse()
                    .map_err(|e| anyhow::anyhow!("Invalid fee denom: {e}"))?,
            },
            gas_limit_u64,
        ))
    }

    /// Get default fee (zero fee).
    fn default_fee() -> Fee {
        Fee {
            amount: vec![],
            gas_limit: 0,
            payer: None,
            granter: None,
        }
    }

    /// Build a transaction for given messages.
    ///
    /// When `authenticator_ids` is provided, the transaction will include a `TxExtension`
    /// for permissioned key trading, allowing sub-accounts to trade using delegated keys.
    ///
    /// # Errors
    ///
    /// Returns an error if transaction building or signing fails.
    pub fn build_transaction(
        &self,
        account: &Account,
        msgs: impl IntoIterator<Item = Any>,
        fee: Option<Fee>,
        authenticator_ids: Option<&[u64]>,
    ) -> Result<tx::Raw, anyhow::Error> {
        let mut builder = tx::BodyBuilder::new();
        builder.msgs(msgs).memo("");

        // Add authenticators for permissioned key trading if provided
        if let Some(auth_ids) = authenticator_ids
            && !auth_ids.is_empty()
        {
            let ext = TxExtension {
                selected_authenticators: auth_ids.to_vec(),
            };
            builder.non_critical_extension_option(ext.to_any());
        }

        let tx_body = builder.finish();

        let fee = fee.unwrap_or_else(|| {
            self.calculate_fee(None)
                .unwrap_or_else(|_| Self::default_fee())
        });

        let auth_info =
            SignerInfo::single_direct(Some(account.public_key()), account.sequence_number)
                .auth_info(fee);

        let sign_doc = SignDoc::new(&tx_body, &auth_info, &self.chain_id, account.account_number)
            .map_err(|e| anyhow::anyhow!("Cannot create sign doc: {e}"))?;

        account.sign(sign_doc)
    }

    /// Build and simulate a transaction to estimate gas.
    ///
    /// Returns the raw transaction bytes suitable for simulation.
    ///
    /// # Errors
    ///
    /// Returns an error if transaction building fails.
    pub fn build_for_simulation(
        &self,
        account: &Account,
        msgs: impl IntoIterator<Item = Any>,
    ) -> Result<Vec<u8>, anyhow::Error> {
        let tx_raw = self.build_transaction(account, msgs, None, None)?;
        tx_raw.to_bytes().map_err(|e| anyhow::anyhow!("{e}"))
    }
}

impl Debug for TxBuilder {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TxBuilder")
            .field("chain_id", &self.chain_id)
            .field("fee_denom", &self.fee_denom)
            .finish()
    }
}
