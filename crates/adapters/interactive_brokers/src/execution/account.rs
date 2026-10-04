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
        AccountSummary, AccountSummaryResult, AccountSummaryTags, AccountUpdate, AccountValue,
        types::{AccountGroup, AccountId as IbAccountId},
    },
    client::Client,
    contracts::Contract,
    orders::ExecutionSide,
    prelude::{StreamExt, SubscriptionItemStreamExt},
};
use nautilus_common::{
    cache::fifo::FifoCache,
    live::runner::get_exec_event_sender,
    messages::{ExecutionEvent, ExecutionReport},
};
use nautilus_core::{Params, time::get_atomic_clock_realtime};
use nautilus_live::task::TaskGroup;
use nautilus_model::{
    enums::PositionSide,
    identifiers::AccountId,
    instruments::Instrument,
    reports::PositionStatusReport,
    types::{AccountBalance, Currency, MarginBalance, Money, Quantity},
};
use rust_decimal::{Decimal, prelude::ToPrimitive};
use ustr::Ustr;

/// Derives the raw IB account code sent to TWS from the configured `account_id`.
///
/// A configured bare code such as `DU123456` is used as is. A configured composite Nautilus
/// account ID such as `IB-DU123456`, or no configured value, falls back to the segment of
/// `account_id` after its last hyphen, because IB account codes contain no hyphen while client
/// names such as `IB-TEST` may.
pub(crate) fn ib_account_code(configured: Option<&str>, account_id: AccountId) -> Ustr {
    match configured {
        Some(code) if !code.contains('-') => Ustr::from(code),
        _ => Ustr::from(
            account_id
                .as_str()
                .rsplit_once('-')
                .map_or(account_id.as_str(), |(_, code)| code),
        ),
    }
}

/// Subscribe to account summary and parse to balances and margins.
///
/// The returned `info` also carries one `reqAccountUpdates` snapshot, because IB serves
/// values such as `PostExpirationExcess` only on that stream and not as summary tags.
///
/// # Errors
///
/// Returns an error if the account summary subscription fails.
pub async fn subscribe_account_summary(
    client: &Arc<Client>,
    ib_account: Ustr,
) -> anyhow::Result<(Vec<AccountBalance>, Vec<MarginBalance>, Option<Params>)> {
    let raw_account_id = ib_account.as_str();
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
    let subscription = client
        .account_summary(&group, tags)
        .await
        .context("Failed to subscribe to account summary")?;
    let mut subscription = subscription.filter_data();

    tracing::debug!("Subscribed to account summary for account: {}", ib_account);

    // Process initial account summary snapshot
    // We collect all summary items until the API sends AccountSummaryResult::End, so the
    // returned balances/margins are complete (matches Python behavior of waiting for all tags).
    let mut balance_summaries = Vec::new();
    let mut margins: Vec<MarginBalance> = Vec::new();
    let mut info = Params::new();

    while let Some(result) = subscription.next().await {
        match result {
            Ok(AccountSummaryResult::Summary(summary)) => {
                // Filter for the specific account
                if summary.account != raw_account_id {
                    continue;
                }

                // Record the raw summary tag so the account state carries the
                // venue-reported values (for example TotalCashValue) that do not
                // map to the typed balances and margins.
                info.insert(
                    summary.tag.clone(),
                    serde_json::Value::from(summary.value.as_str()),
                );

                if let Err(e) = merge_account_summary_balance(&mut balance_summaries, &summary) {
                    tracing::warn!("Failed to parse account summary: {}", e);
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

    if let Err(e) = merge_account_updates_snapshot(client, raw_account_id, &mut info).await {
        tracing::warn!("Failed to collect account updates: {}", e);
    }

    let balances = finalize_account_summary_balances(balance_summaries)?;
    margins.sort_by(|a, b| a.currency.code.as_str().cmp(b.currency.code.as_str()));

    tracing::debug!(
        "Received account summary: {} balances, {} margins",
        balances.len(),
        margins.len()
    );

    Ok((
        balances,
        margins,
        if info.is_empty() { None } else { Some(info) },
    ))
}

// Drains one `reqAccountUpdates` snapshot into `info`, keyed by the raw IB key like the
// summary tags. Dropping the subscription sends the cancel, so no stream stays open.
async fn merge_account_updates_snapshot(
    client: &Arc<Client>,
    raw_account_id: &str,
    info: &mut Params,
) -> anyhow::Result<()> {
    let account = IbAccountId(raw_account_id.to_string());
    let subscription = client
        .account_updates(&account)
        .await
        .context("Failed to subscribe to account updates")?;
    let mut subscription = subscription.filter_data();

    while let Some(result) = subscription.next().await {
        match result {
            Ok(AccountUpdate::AccountValue(value)) => {
                merge_account_value(info, raw_account_id, &value);
            }
            Ok(AccountUpdate::End) => break,
            Ok(AccountUpdate::PortfolioValue(_) | AccountUpdate::UpdateTime(_)) => {}
            Err(e) => {
                tracing::warn!("Error receiving account updates: {}", e);
            }
        }
    }

    Ok(())
}

fn merge_account_value(info: &mut Params, raw_account_id: &str, value: &AccountValue) {
    if value
        .account
        .as_deref()
        .is_some_and(|account| account != raw_account_id)
    {
        return;
    }

    info.insert(
        value.key.clone(),
        serde_json::Value::from(value.value.as_str()),
    );
}

fn merge_account_summary_margin(margins: &mut Vec<MarginBalance>, summary: &AccountSummary) {
    let relevant = matches!(
        summary.tag.as_str(),
        AccountSummaryTags::INIT_MARGIN_REQ | AccountSummaryTags::MAINT_MARGIN_REQ
    );

    if !relevant {
        return;
    }

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
        _ => unreachable!("relevant account summary tag was checked above"),
    }
}

struct AccountSummaryBalance {
    currency: Currency,
    net_liquidation: Option<Decimal>,
    settled_cash: Option<Decimal>,
    total_cash_value: Option<Decimal>,
    available_funds: Option<Decimal>,
    buying_power: Option<Decimal>,
}

impl AccountSummaryBalance {
    fn new(currency: Currency) -> Self {
        Self {
            currency,
            net_liquidation: None,
            settled_cash: None,
            total_cash_value: None,
            available_funds: None,
            buying_power: None,
        }
    }

    fn into_balance(self) -> anyhow::Result<Option<AccountBalance>> {
        let Some(total) = self
            .net_liquidation
            .or(self.settled_cash)
            .or(self.total_cash_value)
            .or(self.available_funds)
            .or(self.buying_power)
        else {
            return Ok(None);
        };
        let free = self
            .available_funds
            .or(self.buying_power)
            .or(self.settled_cash)
            .or(self.total_cash_value)
            .unwrap_or(total);

        Ok(Some(AccountBalance::from_total_and_free(
            total,
            free,
            self.currency,
        )?))
    }
}

fn merge_account_summary_balance(
    balances: &mut Vec<AccountSummaryBalance>,
    summary: &AccountSummary,
) -> anyhow::Result<()> {
    let relevant = matches!(
        summary.tag.as_str(),
        AccountSummaryTags::NET_LIQUIDATION
            | AccountSummaryTags::SETTLED_CASH
            | AccountSummaryTags::TOTAL_CASH_VALUE
            | AccountSummaryTags::AVAILABLE_FUNDS
            | AccountSummaryTags::BUYING_POWER
    );

    if !relevant {
        return Ok(());
    }

    let currency = parse_currency(&summary.currency)?;
    let value = parse_balance_decimal(&summary.value)?;
    let balance = match balances.iter_mut().find(|b| b.currency == currency) {
        Some(balance) => balance,
        None => {
            balances.push(AccountSummaryBalance::new(currency));
            balances.last_mut().expect("balance was just inserted")
        }
    };

    match summary.tag.as_str() {
        AccountSummaryTags::NET_LIQUIDATION => balance.net_liquidation = Some(value),
        AccountSummaryTags::SETTLED_CASH => balance.settled_cash = Some(value),
        AccountSummaryTags::TOTAL_CASH_VALUE => balance.total_cash_value = Some(value),
        AccountSummaryTags::AVAILABLE_FUNDS => balance.available_funds = Some(value),
        AccountSummaryTags::BUYING_POWER => balance.buying_power = Some(value),
        _ => unreachable!("relevant account summary tag was checked above"),
    }

    Ok(())
}

fn finalize_account_summary_balances(
    summaries: Vec<AccountSummaryBalance>,
) -> anyhow::Result<Vec<AccountBalance>> {
    let mut balances = summaries
        .into_iter()
        .map(AccountSummaryBalance::into_balance)
        .collect::<anyhow::Result<Vec<_>>>()?
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    balances.sort_by(|a, b| a.currency.code.as_str().cmp(b.currency.code.as_str()));
    Ok(balances)
}

/// Subscribe to PnL updates for the account.
///
/// This spawns a background task to handle PnL updates.
///
/// # Errors
///
/// Returns an error if subscription fails.
pub async fn subscribe_pnl(
    client: &Arc<Client>,
    ib_account: Ustr,
    session_tasks: &TaskGroup,
) -> anyhow::Result<()> {
    let account = IbAccountId(ib_account.to_string());
    let subscription = client
        .pnl(&account, None)
        .await
        .context("Failed to subscribe to PnL")?;
    let mut subscription = subscription.filter_data();

    tracing::debug!("Subscribed to PnL updates for account: {}", ib_account);

    // Process PnL updates in background task
    let future = async move {
        while let Some(result) = subscription.next().await {
            match result {
                Ok(pnl) => {
                    tracing::debug!(
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
    };
    session_tasks
        .spawn(future)
        .context("Failed to register IB PnL task")?;

    Ok(())
}

#[derive(Debug)]
pub struct PositionTrackerState {
    positions: HashMap<i32, Decimal>,
    own_fill_ids: FifoCache<String, 10_000>,
}

/// Track known positions for detecting external changes (e.g., option exercises).
pub type PositionTracker = Arc<tokio::sync::Mutex<PositionTrackerState>>;

/// Create a new position tracker.
pub fn create_position_tracker() -> PositionTracker {
    Arc::new(tokio::sync::Mutex::new(PositionTrackerState {
        positions: HashMap::new(),
        own_fill_ids: FifoCache::new(),
    }))
}

pub async fn record_own_fill(
    position_tracker: &PositionTracker,
    execution_id: &str,
    contract_id: i32,
    side: ExecutionSide,
    quantity: f64,
) -> anyhow::Result<bool> {
    let quantity = Decimal::from_f64_retain(quantity)
        .context("Failed to convert own fill quantity to Decimal")?;
    let signed_quantity = match side {
        ExecutionSide::Bought => quantity,
        ExecutionSide::Sold => -quantity,
    };
    let mut tracker = position_tracker.lock().await;
    if !tracker.own_fill_ids.insert(execution_id.to_string()) {
        return Ok(false);
    }

    let position = tracker
        .positions
        .entry(contract_id)
        .or_insert(Decimal::ZERO);
    *position += signed_quantity;
    if position.is_zero() {
        tracker.positions.remove(&contract_id);
    }

    Ok(true)
}

/// Check if a position update represents an external change (e.g., option exercise).
pub async fn check_external_position_change(
    position_tracker: &PositionTracker,
    contract_id: i32,
    new_quantity: Decimal,
) -> Option<(bool, Decimal)> {
    let mut tracker = position_tracker.lock().await;
    let known_quantity = tracker
        .positions
        .get(&contract_id)
        .copied()
        .unwrap_or(Decimal::ZERO);

    if new_quantity.is_zero() {
        return (!known_quantity.is_zero()).then_some((true, known_quantity));
    }

    // Check if this is an external position change
    // If quantities match, this is likely from normal trading - not external
    if known_quantity == new_quantity {
        return None;
    }

    // This is a change - determine if it's external
    // External changes occur when position changes without a corresponding execution
    // Update tracked position
    tracker.positions.insert(contract_id, new_quantity);

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
/// to avoid processing duplicates from execDetails. Returns the contracts of the
/// tracked positions so their instruments can be published before reconciliation.
///
/// # Errors
///
/// Returns an error if position request fails.
pub async fn initialize_position_tracking(
    client: &Arc<Client>,
    ib_account: Ustr,
    position_tracker: PositionTracker,
) -> anyhow::Result<Vec<Contract>> {
    let subscription = client
        .positions()
        .await
        .context("Failed to request positions")?;
    let mut subscription = subscription.filter_data();

    tracing::debug!("Initializing position tracking for account: {}", ib_account);

    let mut contracts = Vec::new();
    let mut tracker = position_tracker.lock().await;

    while let Some(result) = subscription.next().await {
        match result {
            Ok(ibapi::accounts::PositionUpdate::Position(position)) => {
                // Filter for the specific account
                if position.account != ib_account.as_str() {
                    continue;
                }

                let contract_id = position.contract.contract_id;
                let quantity = Decimal::from_f64_retain(position.position).unwrap_or_default();

                // Only track non-zero positions
                if !quantity.is_zero() {
                    tracker.positions.insert(contract_id, quantity);
                    contracts.push(position.contract);
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

    tracing::debug!(
        "Initialized tracking for {} existing positions",
        contracts.len()
    );

    Ok(contracts)
}

/// Subscribe to real-time position updates for detecting external position changes (e.g., option exercises).
///
/// This spawns a background task to track position changes and generate position status reports
/// for external changes.
///
/// # Errors
///
/// Returns an error if subscription fails.
pub async fn subscribe_positions(
    client: &Arc<Client>,
    account_id: AccountId,
    ib_account: Ustr,
    position_tracker: PositionTracker,
    instrument_provider: Arc<crate::providers::instruments::InteractiveBrokersInstrumentProvider>,
    session_tasks: &TaskGroup,
) -> anyhow::Result<()> {
    let subscription = client
        .positions()
        .await
        .context("Failed to subscribe to positions")?;
    let mut subscription = subscription.filter_data();

    tracing::debug!("Subscribed to position updates for account: {}", ib_account);

    let exec_sender = get_exec_event_sender();
    let clock = get_atomic_clock_realtime();
    let client_for_instruments = Arc::clone(client);

    // Spawn background task to handle position updates
    let future = async move {
        while let Some(result) = subscription.next().await {
            match result {
                Ok(ibapi::accounts::PositionUpdate::Position(position)) => {
                    if position.account != ib_account.as_str() {
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

                        match instrument_provider
                            .get_instrument(&client_for_instruments, &position.contract)
                            .await
                        {
                            Ok(Some(instrument)) => {
                                let instrument_id = instrument.id();
                                let position_side = if new_quantity.is_zero() {
                                    PositionSide::Flat
                                } else if new_quantity > Decimal::ZERO {
                                    PositionSide::Long
                                } else {
                                    PositionSide::Short
                                };

                                let quantity = Quantity::new(
                                    new_quantity.abs().to_f64().unwrap_or(0.0),
                                    instrument.size_precision(),
                                );

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
                                            .map(|d| d.round_dp(price_precision as u32))
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
                                    None,
                                    None,
                                    avg_px_open,
                                );
                                let event = ExecutionEvent::Report(ExecutionReport::Position(
                                    Box::new(report),
                                ));

                                if exec_sender.send(event).is_err() {
                                    tracing::warn!(
                                        "Failed to send position status report for external change"
                                    );
                                } else {
                                    if new_quantity.is_zero() {
                                        position_tracker
                                            .lock()
                                            .await
                                            .positions
                                            .remove(&contract_id);
                                    }

                                    tracing::info!(
                                        "Generated position status report for external change (likely option exercise)"
                                    );
                                }
                            }
                            Ok(None) => {
                                tracing::warn!(
                                    "Instrument not found for external position contract ID: {}",
                                    contract_id
                                );
                            }
                            Err(e) => {
                                tracing::warn!(
                                    "Failed to resolve external position contract ID {}: {}",
                                    contract_id,
                                    e
                                );
                            }
                        }
                    }
                }
                Ok(ibapi::accounts::PositionUpdate::PositionEnd) => {
                    tracing::debug!("Received end of initial IB position snapshot");
                }
                Err(e) => {
                    tracing::warn!("Error receiving position update: {}", e);
                }
            }
        }
    };
    session_tasks
        .spawn(future)
        .context("Failed to register IB position task")?;

    Ok(())
}

fn parse_balance_decimal(value: &str) -> anyhow::Result<Decimal> {
    value
        .parse::<Decimal>()
        .context(format!("Failed to parse balance value: {value}"))
}

fn parse_currency(currency: &str) -> anyhow::Result<Currency> {
    anyhow::ensure!(!currency.is_empty(), "Account summary currency was empty");
    Ok(Currency::from(currency))
}

#[cfg(test)]
mod tests {
    use ibapi::{
        accounts::{AccountSummary, AccountValue},
        orders::ExecutionSide,
    };
    use nautilus_core::Params;
    use nautilus_model::{
        identifiers::AccountId,
        types::{AccountBalance, Currency, MarginBalance, Money},
    };
    use rstest::rstest;
    use rust_decimal::Decimal;

    use super::{
        AccountSummaryTags, check_external_position_change, create_position_tracker,
        finalize_account_summary_balances, ib_account_code, merge_account_summary_balance,
        merge_account_summary_margin, merge_account_value, parse_currency, record_own_fill,
    };

    fn margin_summary(tag: &str, value: &str, currency: &str) -> AccountSummary {
        AccountSummary {
            account: "DU123".to_string(),
            tag: tag.to_string(),
            value: value.to_string(),
            currency: currency.to_string(),
        }
    }

    fn account_value(account: Option<&str>, key: &str, value: &str) -> AccountValue {
        AccountValue {
            key: key.to_string(),
            value: value.to_string(),
            currency: "USD".to_string(),
            account: account.map(str::to_string),
        }
    }

    fn balances_from_summaries(summaries: &[AccountSummary]) -> Vec<AccountBalance> {
        let mut balances = Vec::new();
        for summary in summaries {
            merge_account_summary_balance(&mut balances, summary).unwrap();
        }
        finalize_account_summary_balances(balances).unwrap()
    }

    #[rstest]
    #[case(Some("U7654321"), "IB-TEST-U7654321", "U7654321")]
    #[case(Some("IB_LIVE-U1234567"), "IB_LIVE-U1234567", "U1234567")]
    #[case(Some("IB-TEST-U7654321"), "IB-TEST-U7654321", "U7654321")]
    #[case(None, "IB-TEST-001", "001")]
    #[case(None, "IB-001", "001")]
    fn test_ib_account_code_prefers_configured_bare_code(
        #[case] configured: Option<&str>,
        #[case] account_id: &str,
        #[case] expected: &str,
    ) {
        let code = ib_account_code(configured, AccountId::from(account_id));

        assert_eq!(code.as_str(), expected);
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
    #[tokio::test]
    async fn test_external_position_change_reports_tracked_zero_close() {
        let tracker = create_position_tracker();
        tracker
            .lock()
            .await
            .positions
            .insert(42, Decimal::new(5, 0));

        let change = check_external_position_change(&tracker, 42, Decimal::ZERO).await;

        assert_eq!(change, Some((true, Decimal::new(5, 0))));
        assert_eq!(
            tracker.lock().await.positions.get(&42).copied(),
            Some(Decimal::new(5, 0))
        );
    }

    #[rstest]
    #[tokio::test]
    async fn test_own_fill_position_change_is_not_external() {
        let tracker = create_position_tracker();
        tracker
            .lock()
            .await
            .positions
            .insert(42, Decimal::new(5, 0));

        assert!(
            record_own_fill(&tracker, "EXEC-1", 42, ExecutionSide::Bought, 2.0)
                .await
                .unwrap()
        );
        let change = check_external_position_change(&tracker, 42, Decimal::new(7, 0)).await;

        assert_eq!(change, None);
        assert_eq!(
            tracker.lock().await.positions.get(&42).copied(),
            Some(Decimal::new(7, 0))
        );
    }

    #[rstest]
    #[tokio::test]
    async fn test_replayed_own_fill_does_not_advance_position_twice() {
        let tracker = create_position_tracker();

        assert!(
            record_own_fill(&tracker, "EXEC-1", 42, ExecutionSide::Sold, 2.0)
                .await
                .unwrap()
        );
        assert!(
            !record_own_fill(&tracker, "EXEC-1", 42, ExecutionSide::Sold, 2.0)
                .await
                .unwrap()
        );

        assert_eq!(
            tracker.lock().await.positions.get(&42).copied(),
            Some(Decimal::new(-2, 0))
        );
    }

    #[rstest]
    fn test_net_liquidation_merge_clamps_free_to_total() {
        let merged = balances_from_summaries(&[
            margin_summary(AccountSummaryTags::AVAILABLE_FUNDS, "120.00", "USD"),
            margin_summary(AccountSummaryTags::NET_LIQUIDATION, "100.00", "USD"),
        ])
        .remove(0);

        assert_eq!(merged.total.as_decimal(), "100.00".parse().unwrap());
        assert_eq!(merged.locked.as_decimal(), "0.00".parse().unwrap());
        assert_eq!(merged.free.as_decimal(), "100.00".parse().unwrap());
    }

    #[rstest]
    fn test_account_summary_balance_merge_is_order_independent() {
        let summaries = [
            margin_summary(AccountSummaryTags::TOTAL_CASH_VALUE, "80.00", "USD"),
            margin_summary(AccountSummaryTags::SETTLED_CASH, "75.00", "USD"),
            margin_summary(AccountSummaryTags::AVAILABLE_FUNDS, "60.00", "USD"),
            margin_summary(AccountSummaryTags::BUYING_POWER, "120.00", "USD"),
            margin_summary(AccountSummaryTags::NET_LIQUIDATION, "100.00", "USD"),
        ];
        let reversed = [
            margin_summary(AccountSummaryTags::NET_LIQUIDATION, "100.00", "USD"),
            margin_summary(AccountSummaryTags::BUYING_POWER, "120.00", "USD"),
            margin_summary(AccountSummaryTags::AVAILABLE_FUNDS, "60.00", "USD"),
            margin_summary(AccountSummaryTags::SETTLED_CASH, "75.00", "USD"),
            margin_summary(AccountSummaryTags::TOTAL_CASH_VALUE, "80.00", "USD"),
        ];

        let forward = balances_from_summaries(&summaries);
        let reverse = balances_from_summaries(&reversed);

        assert_eq!(forward, reverse);
        assert_eq!(forward.len(), 1);
        assert_eq!(forward[0].total, Money::from("100.00 USD"));
        assert_eq!(forward[0].locked, Money::from("40.00 USD"));
        assert_eq!(forward[0].free, Money::from("60.00 USD"));
    }

    #[rstest]
    fn test_account_summary_rows_without_currency_are_ignored() {
        // IB reports ratio tags such as `Cushion` with an empty currency. Neither merge may
        // treat that as a parse failure; the account is built from the currency-bearing rows.
        let cushion = margin_summary(AccountSummaryTags::CUSHION, "0.95", "");
        let mut margins: Vec<MarginBalance> = Vec::new();

        merge_account_summary_margin(&mut margins, &cushion);
        let balances = balances_from_summaries(&[
            cushion,
            margin_summary(AccountSummaryTags::NET_LIQUIDATION, "1000.00", "EUR"),
        ]);

        assert!(margins.is_empty());
        assert_eq!(
            balances,
            vec![
                AccountBalance::from_total_and_locked(
                    Decimal::new(100_000, 2),
                    Decimal::ZERO,
                    Currency::EUR(),
                )
                .unwrap()
            ]
        );
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

    #[rstest]
    #[case(Some("DU123"), true)]
    #[case(None, true)]
    #[case(Some("DU999"), false)]
    fn test_merge_account_value_filters_by_account(
        #[case] account: Option<&str>,
        #[case] expected_inserted: bool,
    ) {
        let mut info = Params::new();

        merge_account_value(
            &mut info,
            "DU123",
            &account_value(account, "PostExpirationExcess", "-326492.00"),
        );

        assert_eq!(
            info.get_str("PostExpirationExcess"),
            expected_inserted.then_some("-326492.00"),
        );
    }

    #[rstest]
    fn test_merge_account_value_keeps_summary_tags_and_overwrites_same_key() {
        let mut info = Params::new();
        info.insert(
            AccountSummaryTags::TOTAL_CASH_VALUE.to_string(),
            serde_json::Value::from("1000.00"),
        );

        merge_account_value(
            &mut info,
            "DU123",
            &account_value(Some("DU123"), "PostExpirationMargin", "391668.36"),
        );
        merge_account_value(
            &mut info,
            "DU123",
            &account_value(
                Some("DU123"),
                AccountSummaryTags::TOTAL_CASH_VALUE,
                "1250.50",
            ),
        );

        assert_eq!(info.len(), 2);
        assert_eq!(info.get_str("TotalCashValue"), Some("1250.50"));
        assert_eq!(info.get_str("PostExpirationMargin"), Some("391668.36"));
    }
}
