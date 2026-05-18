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

//! Smoke test for HIP-4 `userOutcome` exchange actions (`splitOutcome` and
//! `mergeOutcome`).
//!
//! Mode is selected via `HYPERLIQUID_OP=split|merge` (defaults to `split`).
//! Split mode reads `HYPERLIQUID_SPLIT_OUTCOME` and `HYPERLIQUID_SPLIT_AMOUNT`
//! and mints matched Yes + No side tokens. Merge mode reads
//! `HYPERLIQUID_SPLIT_OUTCOME` and an optional `HYPERLIQUID_SPLIT_AMOUNT`
//! (omit to merge the maximum balance) and burns the pair back into USDH.
//!
//! Set `HYPERLIQUID_TESTNET=1` to target testnet; otherwise mainnet is used.
//! Credentials come from the usual `HYPERLIQUID_PK` (or testnet equivalent)
//! resolved by `Secrets::env_vars`.

use std::{env, str::FromStr};

use nautilus_hyperliquid::{
    common::{credential::Secrets, enums::HyperliquidEnvironment},
    http::{
        client::HyperliquidHttpClient, error::Result as HlResult,
        models::HyperliquidExchangeResponse,
    },
};
use rust_decimal::Decimal;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    nautilus_common::logging::ensure_logging_initialized();

    let environment =
        if env::var("HYPERLIQUID_TESTNET").is_ok_and(|v| v.to_lowercase() == "true" || v == "1") {
            HyperliquidEnvironment::Testnet
        } else {
            HyperliquidEnvironment::Mainnet
        };

    let op = env::var("HYPERLIQUID_OP").unwrap_or_else(|_| "split".to_string());

    let outcome: u32 = env::var("HYPERLIQUID_SPLIT_OUTCOME")
        .map_err(|_| "HYPERLIQUID_SPLIT_OUTCOME is required (outcome index, e.g. 5)")?
        .parse()?;

    let amount = match env::var("HYPERLIQUID_SPLIT_AMOUNT") {
        Ok(v) => Some(Decimal::from_str(&v)?),
        Err(_) => None,
    };

    log::info!(
        "Hyperliquid {environment:?} userOutcome smoke: op={op} outcome={outcome} amount={amount:?}"
    );

    let client = match HyperliquidHttpClient::from_env(environment) {
        Ok(client) => client,
        Err(e) => {
            let (pk_var, _) = Secrets::env_vars(environment);
            log::error!("Failed to create client: {e}; ensure {pk_var} is set");
            return Err(e.into());
        }
    };

    let wallet = client
        .get_user_address()
        .map_err(|e| format!("get_user_address: {e}"))?;
    log::info!("Wallet: {wallet}");

    log::info!("Fetching spot balances pre-op...");
    if let Ok(state) = client.info_spot_clearinghouse_state(&wallet).await {
        log::info!(
            "Pre-op spot state: {}",
            serde_json::to_string_pretty(&state).unwrap_or_default()
        );
    }

    log::info!("Submitting {op}Outcome...");
    let response: HlResult<HyperliquidExchangeResponse> = match op.as_str() {
        "split" => {
            let amt = amount.ok_or("HYPERLIQUID_SPLIT_AMOUNT is required for split")?;
            client.submit_split_outcome(outcome, amt).await
        }
        "merge" => client.submit_merge_outcome(outcome, amount).await,
        other => return Err(format!("Unknown HYPERLIQUID_OP: {other}").into()),
    };
    log::info!(
        "{op}Outcome response: {}",
        serde_json::to_string_pretty(&response?)?
    );

    log::info!("Fetching spot balances post-op...");
    if let Ok(state) = client.info_spot_clearinghouse_state(&wallet).await {
        log::info!(
            "Post-op spot state: {}",
            serde_json::to_string_pretty(&state).unwrap_or_default()
        );
    }

    log::info!("Done");
    Ok(())
}
