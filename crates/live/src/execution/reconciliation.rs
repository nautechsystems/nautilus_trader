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

//! Report operations and supporting state for live execution reconciliation.

use std::time::Duration;

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
use nautilus_execution::reconciliation::create_position_reconciliation_venue_order_id;
use nautilus_model::{
    enums::{OrderSide, OrderStatus, OrderType, TimeInForce},
    events::OrderEventAny,
    identifiers::{
        AccountId, ClientId, ClientOrderId, InstrumentId, PositionId, StrategyId, TradeId,
        VenueOrderId,
    },
    instruments::{Instrument, InstrumentAny},
    orders::OrderAny,
    position::Position,
    reports::{FillReport, OrderStatusReport, PositionStatusReport},
    types::{Price, Quantity},
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
pub(crate) enum ReportClientCoverage {
    Resolved(IndexSet<ClientId>),
    Unavailable(IndexSet<ClientId>),
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
    pub(crate) client_order_id: ClientOrderId,
    pub(super) responsible_clients: IndexSet<ClientId>,
    pub(super) command: GenerateOrderStatusReport,
    pub(super) report: Option<OrderStatusReport>,
    pub(super) filled_qty: Quantity,
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
    pub start: Option<UnixNanos>,
}

/// Prepare-time state and command for one continuous position reconciliation check.
#[derive(Debug, Clone)]
pub(crate) struct PositionReportCheck {
    pub command: GeneratePositionStatusReports,
    pub client_coverage: IndexMap<InstrumentAccountKey, ReportClientCoverage>,
    pub activity_revisions: IndexMap<InstrumentAccountKey, u64>,
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
    ///
    /// # Panics
    ///
    /// Panics if the event is not a fill.
    pub(super) fn push(
        &mut self,
        events: &mut Vec<OrderEventAny>,
        event: OrderEventAny,
        fill_key: FillKey,
    ) {
        let OrderEventAny::Filled(fill) = &event else {
            unreachable!("reported fills always create filled events");
        };

        self.pending_fill_keys.insert(fill_key);
        self.event_fill_keys.insert(fill.event_id, fill_key);
        events.push(event);
    }
}

/// Information about an inflight order check.
#[derive(Debug, Clone)]
pub(super) struct InflightCheck {
    #[allow(dead_code)]
    pub client_order_id: ClientOrderId,
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
    clients: &[&dyn ExecutionClient],
    queries: Vec<TargetedOrderQuery>,
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
            client_order_id: query.client_order_id,
            client_id: report_client_id,
            report,
            fills,
            coverage_complete,
        });
    }

    results
}

/// Builds a filled market-order report for one leg of a position reversal.
///
/// Returns `None` if the quantity cannot be represented at instrument precision.
#[expect(clippy::too_many_arguments)]
pub(super) fn build_cross_zero_leg_report(
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
    filled_qty: Quantity,
) -> bool {
    matches!(
        report.order_status,
        OrderStatus::Canceled | OrderStatus::Expired
    ) && report.filled_qty > filled_qty
}
