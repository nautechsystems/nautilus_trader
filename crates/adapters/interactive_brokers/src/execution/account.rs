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

//! Account management for Interactive Brokers execution client.

use std::{collections::HashMap, sync::Arc};

use anyhow::Context;
use ibapi::{
    accounts::{
        AccountSummary, AccountSummaryResult, AccountSummaryTags,
        types::{AccountGroup, AccountId as IbAccountId},
    },
    client::Client,
};
use nautilus_common::{
    live::runner::get_exec_event_sender,
    messages::{ExecutionEvent, ExecutionReport},
};
use nautilus_core::time::get_atomic_clock_realtime;
use nautilus_model::{
    enums::PositionSideSpecified,
    identifiers::AccountId,
    instruments::Instrument,
    reports::PositionStatusReport,
    types::{AccountBalance, Currency, MarginBalance, Money, Quantity},
};
use rust_decimal::{Decimal, prelude::ToPrimitive};

use crate::common::parse::ib_contract_to_instrument_id_simple;

fn raw_ib_account_code(account_id: &AccountId) -> String {
    account_id
        .to_string()
        .strip_prefix("IB-")
        .unwrap_or(account_id.as_str())
        .to_string()
}

/// Subscribe to account summary and parse to balances and margins.
///
/// # Arguments
///
/// * `client` - The IB API client
/// * `account_id` - The account ID
///
/// # Returns
///
/// Returns balances and margins parsed from account summary.
///
/// # Errors
///
/// Returns an error if subscription fails.
pub async fn subscribe_account_summary(
    client: &Arc<Client>,
    account_id: AccountId,
) -> anyhow::Result<(Vec<AccountBalance>, Vec<MarginBalance>)> {
    let raw_account_id = raw_ib_account_code(&account_id);
    // Request key account summary tags (includes TotalCashValue to match Python account summary info dict).
    let tags = &[
        AccountSummaryTags::NET_LIQUIDATION,
        AccountSummaryTags::TOTAL_CASH_VALUE,
        AccountSummaryTags::SETTLED_CASH,
        AccountSummaryTags::BUYING_POWER,
        AccountSummaryTags::EQUITY_WITH_LOAN_VALUE,
        AccountSummaryTags::AVAILABLE_FUNDS,
        AccountSummaryTags::EXCESS_LIQUIDITY,
        AccountSummaryTags::INIT_MARGIN_REQ,
        AccountSummaryTags::MAINT_MARGIN_REQ,
        AccountSummaryTags::CUSHION,
    ];

    let group = AccountGroup("All".to_string());
    let mut subscription = client
        .account_summary(&group, tags)
        .await
        .context("Failed to subscribe to account summary")?;

    tracing::info!("Subscribed to account summary for account: {}", account_id);

    // Process initial account summary snapshot
    // We collect all summary items until the API sends AccountSummaryResult::End, so the
    // returned balances/margins are complete (matches Python behavior of waiting for all tags).
    let mut balances: Vec<AccountBalance> = Vec::new();
    let mut margins: Vec<MarginBalance> = Vec::new();

    while let Some(result) = subscription.next().await {
        match result {
            Ok(AccountSummaryResult::Summary(summary)) => {
                // Filter for the specific account
                if summary.account != raw_account_id {
                    continue;
                }

                match parse_account_summary_to_balance(&summary) {
                    Ok(balance) => {
                        // Check if balance already exists for this currency
                        if let Some(existing) = balances
                            .iter_mut()
                            .find(|b| b.total.currency == balance.total.currency)
                        {
                            if let Some(merged) = merge_account_summary_balance(
                                existing,
                                summary.tag.as_str(),
                                &summary.value,
                                &summary.currency,
                            )? {
                                *existing = merged;
                            }
                        } else {
                            balances.push(balance);
                        }
                    }
                    Err(e) => {
                        tracing::warn!("Failed to parse account summary: {}", e);
                    }
                }

                // Accumulate margin requirements by currency. IB reports INIT_MARGIN_REQ
                // and MAINT_MARGIN_REQ as separate summary entries; merge them into one
                // `MarginBalance` per currency so neither half overwrites the other when
                // the account-wide margin store keys by `Currency`.
                merge_account_summary_margin(&mut margins, &summary);
            }
            Ok(AccountSummaryResult::End) => {
                break;
            }
            Err(e) => {
                tracing::warn!("Error receiving account summary: {}", e);
            }
        }
    }

    tracing::info!(
        "Received account summary: {} balances, {} margins",
        balances.len(),
        margins.len()
    );

    Ok((balances, margins))
}

fn merge_account_summary_margin(margins: &mut Vec<MarginBalance>, summary: &AccountSummary) {
    let currency = match parse_currency(&summary.currency) {
        Ok(currency) => currency,
        Err(e) => {
            tracing::warn!("Skipping margin summary with unknown currency: {}", e);
            return;
        }
    };
    let value = match parse_balance_decimal(&summary.value)
        .and_then(|d| Money::from_decimal(d, currency).map_err(|e| anyhow::anyhow!(e.to_string())))
    {
        Ok(money) => money,
        Err(e) => {
            tracing::warn!("Failed to parse margin value '{}': {}", summary.value, e);
            return;
        }
    };

    let existing = margins
        .iter_mut()
        .find(|m| m.currency == currency && m.instrument_id.is_none());

    match summary.tag.as_str() {
        AccountSummaryTags::INIT_MARGIN_REQ => match existing {
            Some(margin) => margin.initial = value,
            None => margins.push(MarginBalance::new(value, Money::zero(currency), None)),
        },
        AccountSummaryTags::MAINT_MARGIN_REQ => match existing {
            Some(margin) => margin.maintenance = value,
            None => margins.push(MarginBalance::new(Money::zero(currency), value, None)),
        },
        _ => {}
    }
}

fn merge_account_summary_balance(
    existing: &AccountBalance,
    tag: &str,
    value: &str,
    currency_code: &str,
) -> anyhow::Result<Option<AccountBalance>> {
    let currency = parse_currency(currency_code)?;

    match tag {
        AccountSummaryTags::SETTLED_CASH => {
            let settled_cash = parse_balance_decimal(value)?;
            Ok(Some(AccountBalance::from_total_and_locked(
                settled_cash,
                Decimal::ZERO,
                currency,
            )?))
        }
        AccountSummaryTags::NET_LIQUIDATION => {
            let net_liq = parse_balance_decimal(value)?;
            Ok(Some(AccountBalance::from_total_and_free(
                net_liq,
                existing.free.as_decimal(),
                currency,
            )?))
        }
        _ => Ok(None),
    }
}

/// Subscribe to PnL updates for the account.
///
/// This spawns a background task to handle PnL updates.
///
/// # Arguments
///
/// * `client` - The IB API client
/// * `account_id` - The account ID
///
/// # Errors
///
/// Returns an error if subscription fails.
pub async fn subscribe_pnl(client: &Arc<Client>, account_id: AccountId) -> anyhow::Result<()> {
    let account = IbAccountId(raw_ib_account_code(&account_id));
    let mut subscription = client
        .pnl(&account, None)
        .await
        .context("Failed to subscribe to PnL")?;

    tracing::info!("Subscribed to PnL updates for account: {}", account_id);

    // Process PnL updates in background task
    nautilus_common::live::get_runtime().spawn(async move {
        while let Some(result) = subscription.next().await {
            match result {
                Ok(pnl) => {
                    tracing::info!(
                        "PnL update - Daily: {:.2}, Unrealized: {:?}, Realized: {:?}",
                        pnl.daily_pnl,
                        pnl.unrealized_pnl,
                        pnl.realized_pnl
                    );
                    // Note: Account state updates are handled by position updates and account summary
                    // PnL is informational and tracked separately. If needed, account state can be
                    // generated by subscribing to account summary which includes updated balances.
                }
                Err(e) => {
                    tracing::warn!("Error receiving PnL update: {}", e);
                }
            }
        }
    });

    Ok(())
}

/// Track known positions for detecting external changes (e.g., option exercises).
pub type PositionTracker = Arc<tokio::sync::Mutex<HashMap<i32, Decimal>>>;

/// Create a new position tracker.
pub fn create_position_tracker() -> PositionTracker {
    Arc::new(tokio::sync::Mutex::new(HashMap::new()))
}

/// Check if a position update represents an external change (e.g., option exercise).
///
/// # Arguments
///
/// * `position_tracker` - Shared position tracker
/// * `contract_id` - IB contract ID
/// * `new_quantity` - New position quantity
///
/// # Returns
///
/// Returns `(is_external_change, old_quantity)` if this is an external change.
pub async fn check_external_position_change(
    position_tracker: &PositionTracker,
    contract_id: i32,
    new_quantity: Decimal,
) -> Option<(bool, Decimal)> {
    let mut tracker = position_tracker.lock().await;
    let known_quantity = tracker.get(&contract_id).copied().unwrap_or(Decimal::ZERO);

    // Skip zero positions
    if new_quantity.is_zero() {
        tracker.remove(&contract_id);
        return None;
    }

    // Check if this is an external position change
    // If quantities match, this is likely from normal trading - not external
    if known_quantity == new_quantity {
        return None;
    }

    // This is a change - determine if it's external
    // External changes occur when position changes without a corresponding execution
    // Update tracked position
    tracker.insert(contract_id, new_quantity);

    // If we had a known position and it changed, it's likely external
    if known_quantity != Decimal::ZERO && known_quantity != new_quantity {
        Some((true, known_quantity))
    } else {
        // New position or first time seeing it
        Some((false, known_quantity))
    }
}

/// Initialize position tracking with existing positions.
///
/// This fetches all current positions and initializes the position tracker
/// to avoid processing duplicates from execDetails.
///
/// # Arguments
///
/// * `client` - The IB API client
/// * `account_id` - The account ID
/// * `position_tracker` - Shared position tracker to initialize
///
/// # Errors
///
/// Returns an error if position request fails.
pub async fn initialize_position_tracking(
    client: &Arc<Client>,
    account_id: AccountId,
    position_tracker: PositionTracker,
) -> anyhow::Result<()> {
    let raw_account_id = raw_ib_account_code(&account_id);
    let mut subscription = client
        .positions()
        .await
        .context("Failed to request positions")?;

    tracing::info!("Initializing position tracking for account: {}", account_id);

    let mut position_count = 0;
    let mut tracker = position_tracker.lock().await;

    while let Some(result) = subscription.next().await {
        match result {
            Ok(ibapi::accounts::PositionUpdate::Position(position)) => {
                // Filter for the specific account
                if position.account != raw_account_id {
                    continue;
                }

                let contract_id = position.contract.contract_id;
                let quantity = Decimal::from_f64_retain(position.position).unwrap_or_default();

                // Only track non-zero positions
                if !quantity.is_zero() {
                    tracker.insert(contract_id, quantity);
                    position_count += 1;
                }
            }
            Ok(ibapi::accounts::PositionUpdate::PositionEnd) => {
                break;
            }
            Err(e) => {
                tracing::warn!("Error receiving position update: {}", e);
            }
        }
    }

    tracing::info!(
        "Initialized tracking for {} existing positions",
        position_count
    );

    Ok(())
}

/// Subscribe to real-time position updates for detecting external position changes (e.g., option exercises).
///
/// This spawns a background task to track position changes and generate position status reports
/// for external changes.
///
/// # Arguments
///
/// * `client` - The IB API client
/// * `account_id` - The account ID
/// * `position_tracker` - Shared position tracker for detecting external changes
/// * `instrument_provider` - Instrument provider for resolving contracts to instruments
///
/// # Errors
///
/// Returns an error if subscription fails.
pub async fn subscribe_positions(
    client: &Arc<Client>,
    account_id: AccountId,
    position_tracker: PositionTracker,
    instrument_provider: Arc<crate::providers::instruments::InteractiveBrokersInstrumentProvider>,
) -> anyhow::Result<()> {
    let raw_account_id = raw_ib_account_code(&account_id);
    let mut subscription = client
        .positions()
        .await
        .context("Failed to subscribe to positions")?;

    tracing::info!("Subscribed to position updates for account: {}", account_id);

    let exec_sender = get_exec_event_sender();
    let clock = get_atomic_clock_realtime();

    // Spawn background task to handle position updates
    nautilus_common::live::get_runtime().spawn(async move {
        while let Some(result) = subscription.next().await {
            match result {
                Ok(ibapi::accounts::PositionUpdate::Position(position)) => {
                    if position.account != raw_account_id {
                        continue;
                    }

                    let contract_id = position.contract.contract_id;
                    let new_quantity =
                        Decimal::from_f64_retain(position.position).unwrap_or_default();

                    // Check if this is an external position change
                    if let Some((is_external, old_quantity)) =
                        check_external_position_change(&position_tracker, contract_id, new_quantity)
                            .await
                        && is_external
                    {
                        tracing::warn!(
                            "External position change detected (likely option exercise): \
                                Contract ID {}, quantity change: {} -> {}",
                            contract_id,
                            old_quantity,
                            new_quantity
                        );

                        // Convert IB contract to instrument ID
                        match ib_contract_to_instrument_id_simple(&position.contract) {
                            Ok(instrument_id) => {
                                // Get instrument for precision
                                if let Some(instrument) = instrument_provider.find(&instrument_id) {
                                    // Determine position side
                                    let position_side = if new_quantity.is_zero() {
                                        PositionSideSpecified::Flat
                                    } else if new_quantity > Decimal::ZERO {
                                        PositionSideSpecified::Long
                                    } else {
                                        PositionSideSpecified::Short
                                    };

                                    let quantity = Quantity::new(
                                        new_quantity.abs().to_f64().unwrap_or(0.0),
                                        instrument.size_precision(),
                                    );

                                    // Convert IB avg_cost to Nautilus Price, accounting for price magnifier and multiplier
                                    // Python: converted_avg_cost = avg_cost / (multiplier * price_magnifier)
                                    let avg_px_open = if position.average_cost > 0.0 {
                                        let price_magnifier = instrument_provider
                                            .get_price_magnifier(&instrument_id)
                                            as f64;
                                        let multiplier = instrument.multiplier().as_f64();
                                        let converted_avg_cost =
                                            position.average_cost / (multiplier * price_magnifier);
                                        let price_precision = instrument.price_precision();
                                        Some(
                                            Decimal::from_f64_retain(converted_avg_cost)
                                                .and_then(|d| {
                                                    // Round to price precision
                                                    let rounded =
                                                        d.round_dp(price_precision as u32);
                                                    Some(rounded)
                                                })
                                                .unwrap_or_default(),
                                        )
                                    } else {
                                        None
                                    };

                                    let ts_init = clock.get_time_ns();

                                    let report = PositionStatusReport::new(
                                        account_id,
                                        instrument_id,
                                        position_side,
                                        quantity,
                                        ts_init,
                                        ts_init,
                                        None, // report_id: auto-generated
                                        None, // venue_position_id
                                        avg_px_open,
                                    );

                                    // Send position status report
                                    let event = ExecutionEvent::Report(
                                        ExecutionReport::Position(
                                            Box::new(report),
                                        ),
                                    );

                                    if exec_sender.send(event).is_err() {
                                        tracing::warn!(
                                            "Failed to send position status report for external change"
                                        );
                                    } else {
                                        tracing::info!(
                                            "Generated position status report for external change (likely option exercise)"
                                        );
                                    }
                                } else {
                                    tracing::warn!(
                                        "Instrument not found for contract ID: {}",
                                        contract_id
                                    );
                                }
                            }
                            Err(e) => {
                                tracing::warn!(
                                    "Failed to convert contract to instrument ID: {}",
                                    e
                                );
                            }
                        }
                    }
                }
                Ok(ibapi::accounts::PositionUpdate::PositionEnd) => {
                    break;
                }
                Err(e) => {
                    tracing::warn!("Error receiving position update: {}", e);
                }
            }
        }
    });

    Ok(())
}

/// Parse IB account summary to Nautilus AccountBalance.
fn parse_account_summary_to_balance(summary: &AccountSummary) -> anyhow::Result<AccountBalance> {
    let currency = parse_currency(&summary.currency)?;
    let balance = parse_balance_decimal(&summary.value)?;

    match summary.tag.as_str() {
        AccountSummaryTags::SETTLED_CASH | AccountSummaryTags::TOTAL_CASH_VALUE => {
            // Cash balance - free equals total for settled cash
            AccountBalance::from_total_and_locked(balance, Decimal::ZERO, currency)
                .map_err(Into::into)
        }
        AccountSummaryTags::NET_LIQUIDATION => {
            // Net liquidation - represents total equity
            // Free would be calculated from available funds
            AccountBalance::from_total_and_locked(balance, Decimal::ZERO, currency)
                .map_err(Into::into)
        }
        AccountSummaryTags::BUYING_POWER | AccountSummaryTags::AVAILABLE_FUNDS => {
            // Available funds - this is the free amount
            AccountBalance::from_total_and_free(balance, balance, currency).map_err(Into::into)
        }
        _ => {
            // Default: treat as total balance
            AccountBalance::from_total_and_locked(balance, Decimal::ZERO, currency)
                .map_err(Into::into)
        }
    }
}

fn parse_balance_decimal(value: &str) -> anyhow::Result<Decimal> {
    value
        .parse::<Decimal>()
        .context(format!("Failed to parse balance value: {}", value))
}

fn parse_currency(currency: &str) -> anyhow::Result<Currency> {
    anyhow::ensure!(!currency.is_empty(), "Account summary currency was empty");
    Ok(Currency::from(currency))
}

#[cfg(test)]
mod tests {
    use ibapi::accounts::AccountSummary;
    use nautilus_model::types::{AccountBalance, Currency, MarginBalance, Money};
    use rstest::rstest;

    use super::{
        AccountSummaryTags, merge_account_summary_balance, merge_account_summary_margin,
        parse_currency,
    };

    fn margin_summary(tag: &str, value: &str, currency: &str) -> AccountSummary {
        AccountSummary {
            account: "DU123".to_string(),
            tag: tag.to_string(),
            value: value.to_string(),
            currency: currency.to_string(),
        }
    }

    /// Verifies the IB avg cost to Nautilus price conversion formula used in position parsing.
    /// Python: converted_avg_cost = avg_cost / (multiplier * price_magnifier)
    #[rstest]
    fn test_ib_avg_cost_to_price_conversion() {
        let avg_cost = 100.0;
        let multiplier = 10.0;
        let price_magnifier = 2.0;
        let converted = avg_cost / (multiplier * price_magnifier);
        assert_eq!(converted, 5.0);

        let avg_cost2 = 1_500_000.0;
        let multiplier2 = 50.0;
        let price_magnifier2 = 10;
        let converted2 = avg_cost2 / (multiplier2 * (price_magnifier2 as f64));
        assert_eq!(converted2, 3000.0);
    }

    #[rstest]
    fn test_parse_currency_rejects_empty_string() {
        let result = parse_currency("");
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err().to_string(),
            "Account summary currency was empty",
        );
    }

    #[rstest]
    fn test_net_liquidation_merge_clamps_free_to_total() {
        let existing = AccountBalance::from_total_and_free(
            "120.00".parse().unwrap(),
            "120.00".parse().unwrap(),
            Currency::USD(),
        )
        .unwrap();

        let merged = merge_account_summary_balance(
            &existing,
            AccountSummaryTags::NET_LIQUIDATION,
            "100.00",
            "USD",
        )
        .unwrap()
        .unwrap();

        assert_eq!(merged.total.as_decimal(), "100.00".parse().unwrap());
        assert_eq!(merged.locked.as_decimal(), "0.00".parse().unwrap());
        assert_eq!(merged.free.as_decimal(), "100.00".parse().unwrap());
    }

    #[rstest]
    fn test_merge_account_summary_margin_combines_init_and_maint() {
        // Regression: `INIT_MARGIN_REQ` and `MAINT_MARGIN_REQ` arrive as separate
        // summary entries. The merge must land in a single `MarginBalance` per
        // currency so neither half overwrites the other once the account-wide
        // store keys by `Currency`.
        let mut margins: Vec<MarginBalance> = Vec::new();

        merge_account_summary_margin(
            &mut margins,
            &margin_summary(AccountSummaryTags::INIT_MARGIN_REQ, "500.00", "USD"),
        );
        merge_account_summary_margin(
            &mut margins,
            &margin_summary(AccountSummaryTags::MAINT_MARGIN_REQ, "250.00", "USD"),
        );

        assert_eq!(margins.len(), 1);
        let margin = &margins[0];
        assert!(margin.instrument_id.is_none());
        assert_eq!(margin.currency, Currency::USD());
        assert_eq!(margin.initial, Money::from("500.00 USD"));
        assert_eq!(margin.maintenance, Money::from("250.00 USD"));
    }

    #[rstest]
    fn test_merge_account_summary_margin_order_independent() {
        // Arrival order should not matter.
        let mut margins: Vec<MarginBalance> = Vec::new();

        merge_account_summary_margin(
            &mut margins,
            &margin_summary(AccountSummaryTags::MAINT_MARGIN_REQ, "250.00", "USD"),
        );
        merge_account_summary_margin(
            &mut margins,
            &margin_summary(AccountSummaryTags::INIT_MARGIN_REQ, "500.00", "USD"),
        );

        assert_eq!(margins.len(), 1);
        let margin = &margins[0];
        assert_eq!(margin.initial, Money::from("500.00 USD"));
        assert_eq!(margin.maintenance, Money::from("250.00 USD"));
    }

    #[rstest]
    fn test_merge_account_summary_margin_separates_currencies() {
        let mut margins: Vec<MarginBalance> = Vec::new();

        merge_account_summary_margin(
            &mut margins,
            &margin_summary(AccountSummaryTags::INIT_MARGIN_REQ, "500.00", "USD"),
        );
        merge_account_summary_margin(
            &mut margins,
            &margin_summary(AccountSummaryTags::INIT_MARGIN_REQ, "400.00", "EUR"),
        );

        assert_eq!(margins.len(), 2);
        let usd = margins
            .iter()
            .find(|m| m.currency == Currency::USD())
            .unwrap();
        let eur = margins
            .iter()
            .find(|m| m.currency == Currency::EUR())
            .unwrap();
        assert_eq!(usd.initial, Money::from("500.00 USD"));
        assert_eq!(eur.initial, Money::from("400.00 EUR"));
    }
}
