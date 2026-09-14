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

//! Reconciliation snapshots, outcomes, state-independent decisions, and targeted report collection.
//!
//! Shared types describe prepared checks and report results. Functions compare reports,
//! validate fill groups, replay inferred-fill history, and calculate reconciliation quantities.
//! Targeted requests collect order status and missing fills for the manager and live node.
//! The manager owns cache-dependent decisions; the live node owns recurring task lifecycles.

use std::{str::FromStr, time::Duration};

use indexmap::{IndexMap, IndexSet};
use nautilus_common::{
    clients::ExecutionClient,
    enums::LogLevel,
    live::dst,
    messages::execution::{
        TradingCommand,
        report::{
            GenerateFillReports, GenerateOrderStatusReport, GenerateOrderStatusReports,
            GeneratePositionStatusReports,
        },
    },
};
use nautilus_core::{UUID4, UnixNanos};
use nautilus_execution::reconciliation::{
    create_inferred_reconciliation_trade_id, create_position_reconciliation_venue_order_id,
    should_reconciliation_update,
};
use nautilus_model::{
    enums::{LiquiditySide, OrderSide, OrderStatus, OrderType, TimeInForce},
    events::{OrderEventAny, OrderFilled},
    identifiers::{
        AccountId, ClientId, ClientOrderId, InstrumentId, PositionId, StrategyId, TradeId,
        VenueOrderId,
    },
    instruments::{Instrument, InstrumentAny},
    orders::{Order, OrderAny},
    position::Position,
    reports::{FillReport, OrderStatusReport, PositionStatusReport},
    types::{Money, Price, Quantity},
};
use rust_decimal::Decimal;

/// Composite key identifying a position context by instrument and account.
///
/// Used to scope per-position reconciliation state (retry counters, activity
/// throttles, venue report lookups) so that multiple accounts holding the same
/// instrument do not share the same tracking entry.
pub type InstrumentAccountKey = (InstrumentId, AccountId);
pub(super) type AccountInstrumentKey = (AccountId, InstrumentId);
pub(super) type AccountInstrumentStrategyKey = (AccountId, InstrumentId, StrategyId);
pub(super) type FillKey = (AccountId, InstrumentId, TradeId);

/// Execution clients responsible for reporting one cached entity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReportClientCoverage {
    /// Every identified client provides the required report coverage.
    Resolved(IndexSet<ClientId>),
    /// Identified clients cannot provide the required report coverage.
    Unavailable(IndexSet<ClientId>),
    /// No responsible client could be identified.
    Unresolved,
}

/// Metadata for an external order that needs to be registered with the execution client.
#[derive(Debug, Clone)]
pub struct ExternalOrderMetadata {
    pub client_order_id: ClientOrderId,
    pub venue_order_id: VenueOrderId,
    pub instrument_id: InstrumentId,
    pub strategy_id: StrategyId,
    pub ts_init: UnixNanos,
}

/// Result of reconciliation containing events and external order metadata.
#[derive(Debug, Default)]
pub struct ReconciliationResult {
    /// Order events generated during reconciliation.
    pub events: Vec<OrderEventAny>,
    /// External orders that need to be registered with execution clients.
    pub external_orders: Vec<ExternalOrderMetadata>,
}

/// Result of inflight order checks containing terminal events and intermediate queries.
#[derive(Debug, Default)]
pub struct InflightCheckResult {
    /// Terminal events (rejection/cancellation) for orders that exceeded max retries.
    pub events: Vec<OrderEventAny>,
    /// Intermediate venue queries for orders still within retry budget.
    pub queries: Vec<TradingCommand>,
}

/// Events and targeted queries produced by open-order reconciliation.
#[derive(Debug, Default)]
pub(crate) struct OpenOrderReconciliationResult {
    pub events: Vec<OrderEventAny>,
    pub targeted_queries: Vec<TargetedOrderQuery>,
}

/// Order snapshot and client coverage for a targeted status query.
#[derive(Debug, Clone)]
pub(crate) struct TargetedOrderQuery {
    pub(super) client_order_id: ClientOrderId,
    pub(super) responsible_clients: IndexSet<ClientId>,
    pub(super) command: GenerateOrderStatusReport,
    pub(super) report: Option<OrderStatusReport>,
    pub(super) filled_qty: Quantity,
}

impl TargetedOrderQuery {
    /// Returns the order identifier for the targeted query.
    pub(crate) const fn client_order_id(&self) -> ClientOrderId {
        self.client_order_id
    }
}

/// Targeted status query result with fills and coverage completeness.
#[derive(Debug)]
pub(crate) struct TargetedOrderReportResult {
    pub(super) client_order_id: ClientOrderId,
    pub(super) client_id: Option<ClientId>,
    pub(super) report: Option<OrderStatusReport>,
    pub(super) fills: Vec<FillReport>,
    pub(super) coverage_complete: bool,
}

/// Order status report paired with its source execution client.
#[derive(Debug)]
pub(crate) struct SourcedOrderStatusReport {
    pub client_id: ClientId,
    pub report: OrderStatusReport,
}

/// Snapshot and command for one continuous open-order reconciliation check.
#[derive(Debug, Clone)]
pub(crate) struct OpenOrderReportCheck {
    pub command: GenerateOrderStatusReports,
    pub filtered_orders: Vec<OrderAny>,
    pub client_coverage: IndexMap<ClientOrderId, ReportClientCoverage>,
}

/// Prepare-time state and command for one continuous position reconciliation check.
#[derive(Debug, Clone)]
pub struct PositionReportCheck {
    /// The bulk position query.
    pub command: GeneratePositionStatusReports,
    /// Responsible clients by instrument and account.
    pub client_coverage: IndexMap<InstrumentAccountKey, ReportClientCoverage>,
    /// Activity revisions captured before the query.
    pub activity_revisions: IndexMap<InstrumentAccountKey, u64>,
}

/// Fill report request for one instrument, account, and execution client.
#[derive(Debug)]
pub struct PositionFillReportQuery {
    /// The instrument and account to reconcile.
    pub key: InstrumentAccountKey,
    /// The responsible execution client.
    pub client_id: ClientId,
    /// The authoritative fill query.
    pub command: GenerateFillReports,
}

/// Fill queries and discrepancy keys for a position reconciliation check.
#[derive(Debug)]
pub struct PositionFillReportPlan {
    /// Authoritative fill queries that are safe to run.
    pub queries: Vec<PositionFillReportQuery>,
    /// Position keys that still differ from the venue snapshot.
    pub discrepancy_keys: IndexSet<InstrumentAccountKey>,
}

/// Whether a fill is attributable and free of active inferred-fill overlap.
#[derive(Debug)]
pub enum PositionFillReportPreparation {
    /// The report can be applied to the cached execution state.
    Ready,
    /// An active inferred fill prevents authoritative replay.
    InferredOverlap,
    /// A hedge fill cannot be assigned to an unambiguous position.
    Unattributed,
}

/// Cached and venue position quantities and report shape for comparison.
pub(crate) struct PositionQuantityComparison {
    pub(super) cached_positions: Vec<Position>,
    pub(super) cached_signed_qty: Decimal,
    pub(super) cached_long_qty: Decimal,
    pub(super) cached_short_qty: Decimal,
    pub(super) venue_signed_qty: Decimal,
    pub(super) venue_long_qty: Decimal,
    pub(super) venue_short_qty: Decimal,
    pub(super) nonflat_count: usize,
    pub(super) venue_report: Option<PositionStatusReport>,
    pub(super) venue_has_side_reports: bool,
}

impl PositionQuantityComparison {
    /// Checks net quantities and, when both venue sides are reported, side quantities.
    pub(crate) fn quantities_match(&self, tolerance: Decimal) -> bool {
        let net_qty_matches = (self.cached_signed_qty - self.venue_signed_qty).abs() <= tolerance;
        let side_qty_matches = (self.cached_long_qty - self.venue_long_qty).abs() <= tolerance
            && (self.cached_short_qty - self.venue_short_qty).abs() <= tolerance;

        net_qty_matches && (!self.venue_has_side_reports || side_qty_matches)
    }

    /// Classifies venue reports as a single unambiguous position or multiple legs.
    pub(crate) fn report_shape(&self) -> PositionReportShape {
        if self.nonflat_count > 1 || self.venue_has_side_reports {
            PositionReportShape::MultiLeg
        } else {
            PositionReportShape::Unambiguous
        }
    }
}

/// Cached fill identities, missing orders, and netting lifecycle boundaries.
pub(super) struct RetainedFillState {
    pub(super) fill_keys: IndexSet<(AccountId, InstrumentId, TradeId)>,
    pub(super) missing_order_ids: IndexSet<(AccountId, InstrumentId, ClientOrderId)>,
    pub(super) missing_venue_order_ids: IndexSet<(AccountId, InstrumentId, VenueOrderId)>,
    pub(super) netting_lifecycle_starts: IndexMap<AccountInstrumentStrategyKey, UnixNanos>,
}

/// Historical fills grouped for a synthetic reconciliation order.
pub(super) struct HistoricalFillGroup {
    pub(super) venue_order_id: VenueOrderId,
    pub(super) account_id: AccountId,
    pub(super) instrument_id: InstrumentId,
    pub(super) strategy_id: StrategyId,
    pub(super) order_side: OrderSide,
    pub(super) quantity: Decimal,
    pub(super) reduce_only: bool,
    pub(super) ts_event: UnixNanos,
    pub(super) ts_last: UnixNanos,
}

/// Tracks pending fill identities and their generated reconciliation events.
#[derive(Default)]
pub(super) struct ReconciliationFillQueue {
    pub(super) pending_fill_keys: IndexSet<FillKey>,
    pub(super) event_fill_keys: IndexMap<UUID4, FillKey>,
}

impl ReconciliationFillQueue {
    /// Queues a fill event and records its identity for deduplication.
    pub(super) fn push(
        &mut self,
        events: &mut Vec<OrderEventAny>,
        fill: OrderFilled,
        fill_key: FillKey,
    ) {
        self.pending_fill_keys.insert(fill_key);
        self.event_fill_keys.insert(fill.event_id, fill_key);
        events.push(OrderEventAny::Filled(fill));
    }
}

/// Information about an inflight order check.
#[derive(Debug, Clone)]
pub(super) struct InflightCheck {
    pub submitted_at: dst::time::Instant,
    pub retry_count: u32,
    // `Instant` debug output is runtime-specific and intentionally only useful
    // as an opaque monotonic offset.
    pub last_query_at: Option<dst::time::Instant>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PositionReportShape {
    Unambiguous,
    MultiLeg,
}

/// Retry count and report shape for position reconciliation.
#[derive(Debug, Clone, Copy)]
pub(super) struct PositionReconciliationState {
    pub(super) report_shape: PositionReportShape,
    pub(super) retries: u32,
}

/// Requests targeted order status and missing fills from responsible clients.
pub(crate) async fn request_targeted_order_reports(
    queries: Vec<TargetedOrderQuery>,
    clients: &[&dyn ExecutionClient],
    query_delay: Duration,
) -> Vec<TargetedOrderReportResult> {
    let mut results = Vec::with_capacity(queries.len());
    let mut request_count = 0usize;

    for mut query in queries {
        let mut report = None;
        let mut fills = Vec::new();
        let mut report_client_id = None;
        let mut coverage_complete = true;

        for client_id in &query.responsible_clients {
            let client_id = *client_id;

            let Some(client) = clients
                .iter()
                .find(|client| client.client_id() == client_id)
            else {
                coverage_complete = false;
                log::warn!(
                    "Cannot run targeted order status query for {}: execution client {client_id} is unavailable",
                    query.client_order_id,
                );
                continue;
            };

            if request_count > 0 && !query_delay.is_zero() {
                dst::time::sleep(query_delay).await;
            }

            request_count += 1;

            let response = if let Some(report) = query.report.take() {
                Ok(Some(report))
            } else {
                client.generate_order_status_report(&query.command).await
            };

            match response {
                Ok(Some(candidate)) if targeted_report_matches(&query, &candidate) => {
                    if terminal_report_has_missing_fills(&candidate, query.filled_qty) {
                        let mut command = GenerateFillReports::new(
                            UUID4::new(),
                            query.command.ts_init,
                            Some(candidate.instrument_id),
                            Some(candidate.venue_order_id),
                            None,
                            None,
                            None,
                            Some(query.command.command_id),
                        );
                        command.log_receipt_level = LogLevel::Debug;

                        match client.generate_fill_reports(command).await {
                            Ok(reports) => {
                                fills = reports
                                    .into_iter()
                                    .filter(|fill| {
                                        fill.account_id == candidate.account_id
                                            && fill.instrument_id == candidate.instrument_id
                                            && fill.venue_order_id == candidate.venue_order_id
                                            && candidate
                                                .order_side
                                                .is_none_or(|side| fill.order_side == side)
                                    })
                                    .collect();
                            }
                            Err(e) => log::warn!(
                                "Failed fill report query from {client_id} for {}: {e}",
                                query.client_order_id,
                            ),
                        }
                    }

                    report = Some(candidate);
                    report_client_id = Some(client_id);
                    break;
                }
                Ok(Some(candidate)) => {
                    coverage_complete = false;
                    log::warn!(
                        "Ignoring mismatched targeted order status report from {client_id} for {}: client_order_id={:?}, venue_order_id={}, instrument_id={}",
                        query.client_order_id,
                        candidate.client_order_id,
                        candidate.venue_order_id,
                        candidate.instrument_id,
                    );
                }
                Ok(None) => {}
                Err(e) => {
                    coverage_complete = false;
                    log::warn!(
                        "Failed targeted order status query from {client_id} for {}: {e}",
                        query.client_order_id,
                    );
                }
            }
        }

        results.push(TargetedOrderReportResult {
            client_order_id: query.client_order_id(),
            client_id: report_client_id,
            report,
            fills,
            coverage_complete,
        });
    }

    results
}

/// Checks whether cached order status, filled quantity, and report fields match.
pub(super) fn is_exact_order_match(order: &OrderAny, report: &OrderStatusReport) -> bool {
    order.status() == report.order_status
        && order.filled_qty() == report.filled_qty
        && !should_reconciliation_update(order, report)
}

fn targeted_report_matches(query: &TargetedOrderQuery, report: &OrderStatusReport) -> bool {
    let instrument_matches = query
        .command
        .instrument_id
        .is_none_or(|instrument_id| report.instrument_id == instrument_id);
    let order_matches = report.client_order_id == Some(query.client_order_id)
        || query
            .command
            .venue_order_id
            .is_some_and(|venue_order_id| report.venue_order_id == venue_order_id);

    instrument_matches && order_matches
}

/// Checks whether a canceled or expired report has more fills than the cached order.
pub(super) fn terminal_report_has_missing_fills(
    report: &OrderStatusReport,
    cached_filled_qty: Quantity,
) -> bool {
    matches!(
        report.order_status,
        OrderStatus::Canceled | OrderStatus::Expired
    ) && report.filled_qty > cached_filled_qty
}

/// Builds an order report from fills sharing the same order and venue position.
///
/// # Errors
///
/// Returns an error for empty or inconsistent fills, mismatched instrument metadata,
/// or unrepresentable aggregate quantities or prices.
pub(super) fn create_orphan_fill_order_report(
    fills: &[&FillReport],
    instrument: &InstrumentAny,
) -> anyhow::Result<OrderStatusReport> {
    let Some(first) = fills.first() else {
        anyhow::bail!("fill group is empty");
    };

    let venue_position_id = first
        .venue_position_id
        .ok_or_else(|| anyhow::anyhow!("venue position ID is missing"))?;

    for fill in fills.iter().skip(1) {
        anyhow::ensure!(
            fill.account_id == first.account_id,
            "account ID differs across fill group"
        );
        anyhow::ensure!(
            fill.instrument_id == first.instrument_id,
            "instrument ID differs across fill group"
        );
        anyhow::ensure!(
            fill.venue_order_id == first.venue_order_id,
            "venue order ID differs across fill group"
        );
        anyhow::ensure!(
            fill.client_order_id == first.client_order_id,
            "client order ID differs across fill group"
        );
        anyhow::ensure!(
            fill.order_side == first.order_side,
            "order side differs across fill group"
        );
        anyhow::ensure!(
            fill.venue_position_id == first.venue_position_id,
            "venue position ID differs across fill group"
        );
    }

    anyhow::ensure!(
        first.instrument_id == instrument.id(),
        "instrument metadata does not match fill group"
    );

    let (quantity, notional) = fills.iter().try_fold(
        (Decimal::ZERO, Decimal::ZERO),
        |(quantity, notional), fill| {
            let fill_quantity = fill.last_qty.as_decimal();

            let quantity = quantity.checked_add(fill_quantity).ok_or_else(|| {
                anyhow::anyhow!("fill quantity overflow while aggregating fill group")
            })?;

            let fill_notional = fill_quantity
                .checked_mul(fill.last_px.as_decimal())
                .ok_or_else(|| {
                    anyhow::anyhow!("fill notional overflow while aggregating fill group")
                })?;

            let notional = notional.checked_add(fill_notional).ok_or_else(|| {
                anyhow::anyhow!("fill notional overflow while aggregating fill group")
            })?;

            Ok::<_, anyhow::Error>((quantity, notional))
        },
    )?;

    anyhow::ensure!(
        quantity > Decimal::ZERO,
        "fill group quantity is not positive"
    );

    let order_qty = Quantity::from_decimal_dp(quantity, instrument.size_precision())?;
    let avg_px = notional
        .checked_div(quantity)
        .ok_or_else(|| anyhow::anyhow!("fill group average price is not representable"))?;

    let ts_accepted = fills
        .iter()
        .map(|fill| fill.ts_event)
        .min()
        .expect("non-empty fill group");

    let ts_last = fills
        .iter()
        .map(|fill| fill.ts_event)
        .max()
        .expect("non-empty fill group");

    let ts_init = fills
        .iter()
        .map(|fill| fill.ts_init)
        .max()
        .expect("non-empty fill group");

    let report = OrderStatusReport::new(
        first.account_id,
        first.instrument_id,
        first.client_order_id,
        first.venue_order_id,
        first.order_side.into(),
        OrderType::Market,
        TimeInForce::Gtc,
        OrderStatus::Filled,
        order_qty,
        order_qty,
        ts_accepted,
        ts_last,
        ts_init,
        None,
    )
    .with_avg_px(avg_px)
    .with_venue_position_id(venue_position_id);

    Ok(report)
}

/// Checks whether a fill belongs in the retained position projection.
pub(super) fn should_project_fill(
    fill: &OrderFilled,
    retained_fill_state: &RetainedFillState,
    reported_fill_keys: &IndexSet<FillKey>,
    order_only_venue_order_ids: &IndexSet<VenueOrderId>,
) -> bool {
    let fill_key = (fill.account_id, fill.instrument_id, fill.trade_id);
    if retained_fill_state.fill_keys.contains(&fill_key)
        || order_only_venue_order_ids.contains(&fill.venue_order_id)
    {
        return true;
    }

    let order_missing = retained_fill_state.missing_order_ids.contains(&(
        fill.account_id,
        fill.instrument_id,
        fill.client_order_id,
    )) || retained_fill_state.missing_venue_order_ids.contains(&(
        fill.account_id,
        fill.instrument_id,
        fill.venue_order_id,
    ));

    if order_missing && !reported_fill_keys.contains(&fill_key) {
        return true;
    }

    retained_fill_state
        .netting_lifecycle_starts
        .get(&(fill.account_id, fill.instrument_id, fill.strategy_id))
        .is_some_and(|ts_opened| fill.ts_event < *ts_opened)
}

/// Checks active fill history for deterministic inferred reconciliation IDs.
///
/// # Errors
///
/// Returns an error if the cached order history cannot be replayed.
pub(super) fn has_active_inferred_fill(order: &OrderAny) -> anyhow::Result<bool> {
    let events = order.events();
    let trade_ids = order.trade_ids();

    let Some((first, remaining)) = events.split_first() else {
        return Ok(false);
    };

    let mut projected = OrderAny::from_events(vec![(*first).clone()]).map_err(|e| {
        anyhow::anyhow!(
            "cannot replay order {} for inferred fill detection: {e}",
            order.client_order_id(),
        )
    })?;

    for event in remaining {
        projected.apply((*event).clone()).map_err(|e| {
            anyhow::anyhow!(
                "cannot replay order {} for inferred fill detection: {e}",
                order.client_order_id(),
            )
        })?;

        let OrderEventAny::Filled(fill) = event else {
            continue;
        };

        if !fill.reconciliation || !trade_ids.contains(&&fill.trade_id) {
            continue;
        }

        let external_position_id = PositionId::new(format!("{}-EXTERNAL", fill.instrument_id));
        let position_ids = [fill.position_id, Some(external_position_id)];

        let inferred = position_ids.into_iter().flatten().any(|position_id| {
            create_inferred_reconciliation_trade_id(
                fill.account_id,
                fill.instrument_id,
                fill.client_order_id,
                Some(fill.venue_order_id),
                fill.order_side,
                fill.order_type,
                projected.filled_qty(),
                fill.last_qty,
                fill.last_px,
                position_id,
                fill.ts_event,
            ) == fill.trade_id
        });

        if inferred {
            return Ok(true);
        }
    }

    Ok(false)
}

/// Calculates inferred-fill commission using the responsible execution client.
///
/// # Errors
///
/// Returns an error if the client is unavailable or commission calculation fails.
pub(super) fn resolve_inferred_fill_commission(
    fill_qty: Quantity,
    price_and_liquidity: Option<(Price, LiquiditySide)>,
    instrument: &InstrumentAny,
    client: Option<&dyn ExecutionClient>,
) -> anyhow::Result<Option<Money>> {
    let Some(client) = client else {
        anyhow::bail!("responsible execution client is unavailable");
    };

    let Some((last_px, liquidity_side)) = price_and_liquidity else {
        return Ok(None);
    };

    client.calculate_commission(instrument, fill_qty, last_px, liquidity_side)
}

/// Resolves position-report coverage by account, falling back to venue clients.
pub(crate) fn resolve_position_report_client_coverage(
    key: InstrumentAccountKey,
    clients: &[&dyn ExecutionClient],
) -> ReportClientCoverage {
    let account_clients = clients
        .iter()
        .filter(|client| client.account_id() == key.1)
        .map(|client| client.client_id())
        .collect::<IndexSet<_>>();

    if !account_clients.is_empty() {
        return if clients.iter().any(|client| {
            account_clients.contains(&client.client_id())
                && !client.provides_bulk_position_coverage(key.0)
        }) {
            ReportClientCoverage::Unavailable(account_clients)
        } else {
            ReportClientCoverage::Resolved(account_clients)
        };
    }

    let venue_clients = clients
        .iter()
        .filter(|client| client.handles_order_venue(key.0.venue))
        .map(|client| client.client_id())
        .collect::<IndexSet<_>>();

    if venue_clients.is_empty() {
        ReportClientCoverage::Unresolved
    } else if clients.iter().any(|client| {
        venue_clients.contains(&client.client_id())
            && !client.provides_bulk_position_coverage(key.0)
    }) {
        ReportClientCoverage::Unavailable(venue_clients)
    } else {
        ReportClientCoverage::Resolved(venue_clients)
    }
}

/// Returns the quantity-weighted average of positive position entry prices.
pub(super) fn position_avg_px(cached_positions: &[Position]) -> Option<Decimal> {
    let mut total_value = Decimal::ZERO;
    let mut total_qty = Decimal::ZERO;

    for position in cached_positions {
        let qty = position.signed_decimal_qty().abs();
        if position.avg_px_open > 0.0
            && qty > Decimal::ZERO
            && let Ok(avg_px) = Decimal::from_str(&position.avg_px_open.to_string())
        {
            total_value += avg_px * qty;
            total_qty += qty;
        }
    }

    if total_qty > Decimal::ZERO {
        Some(total_value / total_qty)
    } else {
        None
    }
}

/// Aggregates signed quantities into net, long, and absolute short totals.
pub(super) fn position_qty_aggregates(
    signed_quantities: impl Iterator<Item = Decimal>,
) -> (Decimal, Decimal, Decimal) {
    signed_quantities.fold(
        (Decimal::ZERO, Decimal::ZERO, Decimal::ZERO),
        |(net, long, short), qty| {
            if qty > Decimal::ZERO {
                (net + qty, long + qty, short)
            } else {
                (net + qty, long, short + qty.abs())
            }
        },
    )
}

/// Builds a filled market-order report for one leg of a position reversal.
///
/// Returns `None` if the quantity cannot be represented at instrument precision.
#[expect(clippy::too_many_arguments)]
pub(super) fn create_cross_zero_leg_report(
    instrument: &InstrumentAny,
    account_id: AccountId,
    instrument_id: InstrumentId,
    order_side: OrderSide,
    quantity: Decimal,
    avg_px: Decimal,
    venue_position_id: Option<PositionId>,
    tag: &str,
    ts_now: UnixNanos,
    venue_ts_last: UnixNanos,
) -> Option<OrderStatusReport> {
    let order_qty = Quantity::from_decimal_dp(quantity, instrument.size_precision()).ok()?;
    let fill_price = Price::from_decimal_dp(avg_px, instrument.price_precision()).ok();
    let venue_order_id = create_position_reconciliation_venue_order_id(
        account_id,
        instrument_id,
        order_side,
        OrderType::Market,
        order_qty,
        fill_price,
        venue_position_id,
        Some(tag),
        venue_ts_last,
    );

    let mut report = OrderStatusReport::new(
        account_id,
        instrument_id,
        None,
        venue_order_id,
        order_side.into(),
        OrderType::Market,
        TimeInForce::Gtc,
        OrderStatus::Filled,
        order_qty,
        order_qty,
        ts_now,
        ts_now,
        ts_now,
        None,
    )
    .with_avg_px(avg_px);

    if let Some(venue_position_id) = venue_position_id {
        report = report.with_venue_position_id(venue_position_id);
    }

    Some(report)
}

#[cfg(test)]
pub(super) mod tests {
    use std::cell::RefCell;

    use nautilus_core::Params;
    use nautilus_execution::reconciliation::inferred_fill_price_and_liquidity;
    use nautilus_model::{
        accounts::AccountAny,
        enums::OmsType,
        identifiers::Venue,
        instruments::stubs::crypto_perpetual_ethusdt,
        orders::{OrderTestBuilder, stubs::TestOrderEventStubs},
        types::{AccountBalance, Currency, MarginBalance, quantity::QUANTITY_MAX},
    };
    use proptest::prelude::*;
    use rstest::rstest;
    use rust_decimal_macros::dec;

    use super::*;

    /// Configured result of a stub commission calculation.
    #[derive(Clone)]
    pub(crate) enum CommissionOutcome {
        Value(Money),
        NoOverride,
        Failure,
    }

    /// Execution client that records commission inputs and returns a configured result.
    pub(crate) struct CommissionStubClient {
        outcome: CommissionOutcome,
        seen: RefCell<Option<(Quantity, Price, LiquiditySide)>>,
    }

    impl CommissionStubClient {
        /// Creates a client with the specified commission result.
        pub(crate) fn new(outcome: CommissionOutcome) -> Self {
            Self {
                outcome,
                seen: RefCell::new(None),
            }
        }

        /// Returns the last recorded commission inputs.
        pub(crate) fn seen(&self) -> Option<(Quantity, Price, LiquiditySide)> {
            *self.seen.borrow()
        }

        /// Clears the recorded commission inputs.
        pub(crate) fn clear_seen(&self) {
            *self.seen.borrow_mut() = None;
        }
    }

    #[async_trait::async_trait(?Send)]
    impl ExecutionClient for CommissionStubClient {
        fn is_connected(&self) -> bool {
            true
        }

        fn client_id(&self) -> ClientId {
            ClientId::from("STUB")
        }

        fn account_id(&self) -> AccountId {
            AccountId::from("STUB-001")
        }

        fn venue(&self) -> Venue {
            Venue::from("STUB")
        }

        fn oms_type(&self) -> OmsType {
            OmsType::Netting
        }

        fn get_account(&self) -> Option<AccountAny> {
            None
        }

        fn generate_account_state(
            &self,
            _balances: Vec<AccountBalance>,
            _margins: Vec<MarginBalance>,
            _reported: bool,
            _ts_event: UnixNanos,
            _info: Option<Params>,
        ) -> anyhow::Result<()> {
            Ok(())
        }

        fn start(&mut self) -> anyhow::Result<()> {
            Ok(())
        }

        fn stop(&mut self) -> anyhow::Result<()> {
            Ok(())
        }

        fn calculate_commission(
            &self,
            _instrument: &InstrumentAny,
            last_qty: Quantity,
            last_px: Price,
            liquidity_side: LiquiditySide,
        ) -> anyhow::Result<Option<Money>> {
            *self.seen.borrow_mut() = Some((last_qty, last_px, liquidity_side));

            match &self.outcome {
                CommissionOutcome::Value(money) => Ok(Some(*money)),
                CommissionOutcome::NoOverride => Ok(None),
                CommissionOutcome::Failure => {
                    anyhow::bail!("commission is not representable as Money")
                }
            }
        }
    }

    fn commission_fixtures() -> (OrderAny, OrderStatusReport, InstrumentAny) {
        let instrument = crypto_perpetual_ethusdt();
        let order = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from("10.0"))
            .price(Price::from("100.00"))
            .build();
        let report = OrderStatusReport::new(
            AccountId::from("STUB-001"),
            instrument.id(),
            Some(order.client_order_id()),
            VenueOrderId::from("V-1"),
            OrderSide::Buy.into(),
            OrderType::Limit,
            TimeInForce::Gtc,
            OrderStatus::Filled,
            Quantity::from("10.0"),
            Quantity::from("10.0"),
            UnixNanos::from(1),
            UnixNanos::from(1),
            UnixNanos::from(1),
            None,
        )
        .with_avg_px(dec!(100.0));

        (order, report, InstrumentAny::CryptoPerpetual(instrument))
    }

    #[rstest]
    fn test_create_orphan_fill_order_report_preserves_aggregate_fields() {
        let (instrument, fills) = orphan_fill_fixtures();
        let reports = fills.iter().collect::<Vec<_>>();

        let report = create_orphan_fill_order_report(&reports, &instrument).unwrap();
        let expected = OrderStatusReport::new(
            AccountId::from("STUB-001"),
            instrument.id(),
            Some(ClientOrderId::from("O-ORPHAN")),
            VenueOrderId::from("V-ORPHAN"),
            OrderSide::Buy.into(),
            OrderType::Market,
            TimeInForce::Gtc,
            OrderStatus::Filled,
            Quantity::from("4.000"),
            Quantity::from("4.000"),
            UnixNanos::from(10),
            UnixNanos::from(30),
            UnixNanos::from(50),
            Some(report.report_id),
        )
        .with_avg_px(dec!(115))
        .with_venue_position_id(PositionId::from("P-ORPHAN"));

        assert_eq!(report, expected);
    }

    #[rstest]
    #[case::empty("empty", "fill group is empty")]
    #[case::missing_position("missing_position", "venue position ID is missing")]
    #[case::account("account", "account ID differs across fill group")]
    #[case::instrument("instrument", "instrument ID differs across fill group")]
    #[case::venue_order("venue_order", "venue order ID differs across fill group")]
    #[case::client_order("client_order", "client order ID differs across fill group")]
    #[case::side("side", "order side differs across fill group")]
    #[case::position("position", "venue position ID differs across fill group")]
    #[case::metadata("metadata", "instrument metadata does not match fill group")]
    #[case::zero("zero", "fill group quantity is not positive")]
    fn test_create_orphan_fill_order_report_rejects_invalid_group(
        #[case] invalid_field: &str,
        #[case] expected: &str,
    ) {
        let (instrument, mut fills) = orphan_fill_fixtures();

        match invalid_field {
            "empty" => fills.clear(),
            "missing_position" => fills[0].venue_position_id = None,
            "account" => fills[1].account_id = AccountId::from("OTHER-002"),
            "instrument" => fills[1].instrument_id = InstrumentId::from("OTHER.TEST"),
            "venue_order" => fills[1].venue_order_id = VenueOrderId::from("V-OTHER"),
            "client_order" => fills[1].client_order_id = Some(ClientOrderId::from("O-OTHER")),
            "side" => fills[1].order_side = OrderSide::Sell,
            "position" => fills[1].venue_position_id = Some(PositionId::from("P-OTHER")),
            "metadata" => {
                for fill in &mut fills {
                    fill.instrument_id = InstrumentId::from("OTHER.TEST");
                }
            }
            "zero" => {
                for fill in &mut fills {
                    fill.last_qty = Quantity::zero(3);
                }
            }
            _ => unreachable!(),
        }

        let reports = fills.iter().collect::<Vec<_>>();

        let error = create_orphan_fill_order_report(&reports, &instrument).unwrap_err();

        assert_eq!(error.to_string(), expected);
    }

    #[rstest]
    fn test_create_orphan_fill_order_report_rejects_aggregate_quantity_overflow() {
        let (instrument, mut fills) = orphan_fill_fixtures();
        let max_qty = Quantity::new(QUANTITY_MAX, 0);
        for fill in &mut fills {
            fill.last_qty = max_qty;
            fill.last_px = Price::from("1.00");
        }

        let expected =
            Quantity::from_decimal_dp(max_qty.as_decimal() * dec!(2), instrument.size_precision())
                .unwrap_err();
        let reports = fills.iter().collect::<Vec<_>>();

        let error = create_orphan_fill_order_report(&reports, &instrument).unwrap_err();

        assert_eq!(error.to_string(), expected.to_string());
    }

    fn orphan_fill_fixtures() -> (InstrumentAny, Vec<FillReport>) {
        let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt());

        let fills = [("1.000", "100.00", 30, 40), ("3.000", "120.00", 10, 50)]
            .into_iter()
            .enumerate()
            .map(|(index, (qty, px, ts_event, ts_init))| {
                FillReport::new(
                    AccountId::from("STUB-001"),
                    instrument.id(),
                    VenueOrderId::from("V-ORPHAN"),
                    TradeId::new(format!("T-{index}")),
                    OrderSide::Buy,
                    Quantity::from(qty),
                    Price::from(px),
                    Money::from("0.10 USDT"),
                    LiquiditySide::Taker,
                    Some(ClientOrderId::from("O-ORPHAN")),
                    Some(PositionId::from("P-ORPHAN")),
                    UnixNanos::from(ts_event),
                    UnixNanos::from(ts_init),
                    None,
                )
            })
            .collect();

        (instrument, fills)
    }

    #[rstest]
    fn test_resolve_inferred_fill_commission_without_client_fails_closed() {
        let (order, report, instrument) = commission_fixtures();

        let price_and_liquidity = inferred_fill_price_and_liquidity(&order, &report, &instrument);

        let error = resolve_inferred_fill_commission(
            Quantity::from("5.0"),
            price_and_liquidity,
            &instrument,
            None,
        )
        .expect_err("a missing responsible client must defer the fill");

        assert_eq!(
            error.to_string(),
            "responsible execution client is unavailable"
        );
    }

    #[rstest]
    fn test_resolve_inferred_fill_commission_without_price_uses_generic_path() {
        let instrument = crypto_perpetual_ethusdt();
        let order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(instrument.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from("10.0"))
            .build();

        let report = OrderStatusReport::new(
            AccountId::from("STUB-001"),
            instrument.id(),
            Some(order.client_order_id()),
            VenueOrderId::from("V-1"),
            OrderSide::Buy.into(),
            OrderType::Market,
            TimeInForce::Gtc,
            OrderStatus::Filled,
            Quantity::from("10.0"),
            Quantity::from("10.0"),
            UnixNanos::from(1),
            UnixNanos::from(1),
            UnixNanos::from(1),
            None,
        );
        let client =
            CommissionStubClient::new(CommissionOutcome::Value(Money::new(1.0, Currency::USDT())));
        let instrument = InstrumentAny::CryptoPerpetual(instrument);

        let price_and_liquidity = inferred_fill_price_and_liquidity(&order, &report, &instrument);

        let commission = resolve_inferred_fill_commission(
            Quantity::from("5.0"),
            price_and_liquidity,
            &instrument,
            Some(&client),
        )
        .expect("an unresolvable price is not a failure");

        assert_eq!(commission, None, "no price means no venue commission");
    }

    #[rstest]
    fn test_resolve_inferred_fill_commission_returns_venue_value() {
        let (order, report, instrument) = commission_fixtures();
        let expected = Money::new(2.5, Currency::USDT());
        let client = CommissionStubClient::new(CommissionOutcome::Value(expected));

        let price_and_liquidity = inferred_fill_price_and_liquidity(&order, &report, &instrument);

        let commission = resolve_inferred_fill_commission(
            Quantity::from("5.0"),
            price_and_liquidity,
            &instrument,
            Some(&client),
        )
        .expect("a representable commission succeeds");

        assert_eq!(commission, Some(expected));
        assert_eq!(
            client.seen(),
            Some((
                Quantity::from("5.0"),
                Price::from("100.00"),
                LiquiditySide::NoLiquiditySide,
            )),
            "the resolver passes the inferred fill quantity, resolved price, and liquidity side"
        );
    }

    #[rstest]
    fn test_resolve_inferred_fill_commission_honors_no_override() {
        let (order, report, instrument) = commission_fixtures();
        let client = CommissionStubClient::new(CommissionOutcome::NoOverride);

        let price_and_liquidity = inferred_fill_price_and_liquidity(&order, &report, &instrument);

        let commission = resolve_inferred_fill_commission(
            Quantity::from("5.0"),
            price_and_liquidity,
            &instrument,
            Some(&client),
        )
        .expect("no override is not a failure");

        assert_eq!(commission, None);
    }

    #[rstest]
    fn test_resolve_inferred_fill_commission_propagates_failure() {
        let (order, report, instrument) = commission_fixtures();
        let client = CommissionStubClient::new(CommissionOutcome::Failure);

        let price_and_liquidity = inferred_fill_price_and_liquidity(&order, &report, &instrument);

        let result = resolve_inferred_fill_commission(
            Quantity::from("5.0"),
            price_and_liquidity,
            &instrument,
            Some(&client),
        );

        assert_eq!(
            result.unwrap_err().to_string(),
            "commission is not representable as Money"
        );
    }

    #[rstest]
    #[case::account(true, false, true, true, "resolved")]
    #[case::account_unavailable(true, false, false, true, "unavailable")]
    #[case::venue(false, true, true, true, "resolved")]
    #[case::venue_unavailable(false, true, false, true, "unavailable")]
    #[case::unmatched(false, false, true, true, "unresolved")]
    #[case::no_clients(false, false, true, false, "unresolved")]
    fn test_resolve_position_report_client_coverage(
        #[case] account_matches: bool,
        #[case] venue_matches: bool,
        #[case] available: bool,
        #[case] include_client: bool,
        #[case] expected: &str,
    ) {
        let key = (InstrumentId::from("ETH.TEST"), AccountId::from("TEST-001"));

        let client = CoverageStubClient {
            id: ClientId::from("COVERAGE"),
            account_id: if account_matches {
                key.1
            } else {
                AccountId::from("OTHER-001")
            },
            venue: if venue_matches {
                key.0.venue
            } else {
                Venue::from("OTHER")
            },
            available,
        };

        let clients: Vec<&dyn ExecutionClient> = if include_client {
            vec![&client]
        } else {
            vec![]
        };

        let expected = match expected {
            "resolved" => ReportClientCoverage::Resolved(IndexSet::from([client.id])),
            "unavailable" => ReportClientCoverage::Unavailable(IndexSet::from([client.id])),
            "unresolved" => ReportClientCoverage::Unresolved,
            _ => unreachable!(),
        };

        let result = resolve_position_report_client_coverage(key, &clients);

        assert_eq!(result, expected);
    }

    #[rstest]
    #[case::account_available(true, false)]
    #[case::account_unavailable(false, true)]
    fn test_position_coverage_prefers_account_clients(
        #[case] account_available: bool,
        #[case] venue_available: bool,
    ) {
        let key = (InstrumentId::from("ETH.TEST"), AccountId::from("TEST-001"));

        let account_client = CoverageStubClient {
            id: ClientId::from("ACCOUNT"),
            account_id: key.1,
            venue: Venue::from("OTHER"),
            available: account_available,
        };

        let venue_client = CoverageStubClient {
            id: ClientId::from("VENUE"),
            account_id: AccountId::from("OTHER-001"),
            venue: key.0.venue,
            available: venue_available,
        };

        let expected_clients = IndexSet::from([account_client.id]);

        let expected = if account_available {
            ReportClientCoverage::Resolved(expected_clients)
        } else {
            ReportClientCoverage::Unavailable(expected_clients)
        };

        let result =
            resolve_position_report_client_coverage(key, &[&venue_client, &account_client]);

        assert_eq!(result, expected);
    }

    struct CoverageStubClient {
        id: ClientId,
        account_id: AccountId,
        venue: Venue,
        available: bool,
    }

    #[async_trait::async_trait(?Send)]
    impl ExecutionClient for CoverageStubClient {
        fn is_connected(&self) -> bool {
            true
        }

        fn client_id(&self) -> ClientId {
            self.id
        }

        fn account_id(&self) -> AccountId {
            self.account_id
        }

        fn venue(&self) -> Venue {
            self.venue
        }

        fn oms_type(&self) -> OmsType {
            OmsType::Netting
        }

        fn get_account(&self) -> Option<AccountAny> {
            None
        }

        fn provides_bulk_position_coverage(&self, _instrument_id: InstrumentId) -> bool {
            self.available
        }

        fn generate_account_state(
            &self,
            _balances: Vec<AccountBalance>,
            _margins: Vec<MarginBalance>,
            _reported: bool,
            _ts_event: UnixNanos,
            _info: Option<Params>,
        ) -> anyhow::Result<()> {
            Ok(())
        }

        fn start(&mut self) -> anyhow::Result<()> {
            Ok(())
        }

        fn stop(&mut self) -> anyhow::Result<()> {
            Ok(())
        }
    }

    #[rstest]
    #[case::empty(vec![], None)]
    #[case::flat(vec![(0, 100.0)], None)]
    #[case::weighted(vec![(1, 100.0), (3, 120.0)], Some(dec!(115)))]
    #[case::short(vec![(-1, 100.0), (-3, 120.0)], Some(dec!(115)))]
    #[case::ignored(vec![(1, 0.0), (2, -10.0), (3, 120.0)], Some(dec!(120)))]
    fn test_position_avg_px(#[case] entries: Vec<(i32, f64)>, #[case] expected: Option<Decimal>) {
        let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt());
        let order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(instrument.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from("1.000"))
            .build();
        let fill = TestOrderEventStubs::filled(
            &order,
            &instrument,
            None,
            Some(PositionId::from("P-AVG")),
            Some(Price::from("100.00")),
            Some(Quantity::from("1.000")),
            None,
            None,
            None,
            None,
        );
        let position = Position::new(&instrument, fill.into());

        let positions = entries
            .into_iter()
            .map(|(qty, px)| {
                let mut position = position.clone();
                position.signed_qty = f64::from(qty);
                position.quantity = Quantity::new(f64::from(qty.abs()), 3);
                position.avg_px_open = px;
                position
            })
            .collect::<Vec<_>>();

        let result = position_avg_px(&positions);

        assert_eq!(result, expected);
    }

    #[rstest]
    #[case::empty(vec![], (dec!(0), dec!(0), dec!(0)))]
    #[case::zero(vec![0, 0], (dec!(0), dec!(0), dec!(0)))]
    #[case::long(vec![2, 3], (dec!(5), dec!(5), dec!(0)))]
    #[case::short(vec![-2, -3], (dec!(-5), dec!(0), dec!(5)))]
    #[case::mixed(vec![2, -5, 1], (dec!(-2), dec!(3), dec!(5)))]
    #[case::offset(vec![3, -3], (dec!(0), dec!(3), dec!(3)))]
    fn test_position_qty_aggregates(
        #[case] quantities: Vec<i32>,
        #[case] expected: (Decimal, Decimal, Decimal),
    ) {
        let result = position_qty_aggregates(quantities.into_iter().map(Decimal::from));

        assert_eq!(result, expected);
    }

    proptest! {
        #[rstest]
        fn prop_position_qty_aggregates_preserves_sign_and_net(
            values in proptest::collection::vec(-1_000_000i64..=1_000_000, 0..32),
        ) {
            let quantities = values.iter().map(|v| Decimal::new(*v, 3)).collect::<Vec<_>>();
            let (net, long, short) = position_qty_aggregates(quantities.iter().copied());
            let reversed = position_qty_aggregates(quantities.iter().map(|v| -*v));
            let expected_net = Decimal::new(values.iter().sum(), 3);

            prop_assert_eq!(net, expected_net);
            prop_assert_eq!(net, long - short);
            prop_assert!(long >= Decimal::ZERO);
            prop_assert!(short >= Decimal::ZERO);
            prop_assert_eq!(reversed, (-net, short, long));
        }
    }
}
