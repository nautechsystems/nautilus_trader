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

//! Execution reconciliation and state tracking for live hosts.
//!
//! [`ExecutionManager`] owns activity windows, retry state, fill identities, and cache-dependent
//! decisions. It prepares queries and events, validates reports, and verifies applied fills.
//! External-order registration updates the cache and publishes its initialization notification.
//!
//! The manager requests reports for individual checks and applies mass-status events to the engine.
//! The live node owns recurring tasks, deadlines, and continuous event dispatch. Preparation snapshots
//! retain activity revisions so decisions can be rechecked after requests or event callbacks.
//! Activity revision counters are retained for the manager's lifetime so an older snapshot cannot
//! mistake a reset counter for unchanged activity.

use std::{
    cell::{Ref, RefCell},
    fmt::Debug,
    rc::Rc,
    str::FromStr,
    sync::LazyLock,
    time::Duration,
};

use indexmap::{IndexMap, IndexSet};
use nautilus_common::{
    cache::Cache,
    clients::{DEFAULT_POSITION_RECONCILIATION_TOLERANCE, ExecutionClient},
    clock::Clock,
    config::ConfigResult,
    enums::{LogColor, LogLevel},
    live::dst,
    log_info,
    messages::{
        ExecutionReport,
        execution::{
            QueryOrder, TradingCommand,
            report::{
                GenerateFillReports, GenerateOrderStatusReport, GenerateOrderStatusReports,
                GeneratePositionStatusReports,
            },
        },
    },
    msgbus::{self, MessagingSwitchboard, switchboard},
};
use nautilus_core::{DurationNanos, UUID4, UnixNanos, datetime::mins_to_secs};
use nautilus_execution::{
    engine::ExecutionEngine,
    reconciliation::{
        calculate_reconciliation_price, create_inferred_fill_for_qty,
        create_position_reconciliation_venue_order_id, create_reconciliation_rejected,
        create_reconciliation_triggered, generate_external_order_status_events_with_commission,
        generate_reconciliation_order_pre_fill_events,
        generate_reconciliation_order_snapshot_events_with_commission,
        incremental_inferred_fill_price_and_liquidity, inferred_fill_price_and_liquidity,
        process_mass_status_for_reconciliation,
        process_mass_status_for_reconciliation_without_synthetic_reports,
        reconcile_order_report_with_commission,
    },
};
use nautilus_model::{
    enums::{OmsType, OrderSide, OrderStatus, OrderType, TimeInForce},
    events::{OrderCanceled, OrderEventAny, OrderFilled, OrderInitialized},
    identifiers::{
        AccountId, ClientId, ClientOrderId, InstrumentId, PositionId, StrategyId, TradeId,
        VenueOrderId,
    },
    instruments::{Instrument, InstrumentAny},
    orders::{Order, OrderAny, TRIGGERABLE_ORDER_TYPES},
    position::{Position, PositionReplayEvent},
    reports::{ExecutionMassStatus, FillReport, OrderStatusReport, PositionStatusReport},
    types::{Money, Price, Quantity},
};
use rust_decimal::Decimal;
use ustr::Ustr;

pub(crate) use super::reconciliation::{
    OpenOrderReconciliationResult, OpenOrderReportCheck, SourcedOrderStatusReport,
    TargetedOrderQuery, TargetedOrderReportResult, request_targeted_order_reports,
    resolve_position_report_client_coverage,
};
pub use super::{
    config::ExecutionManagerConfig,
    reconciliation::{
        ExternalOrderMetadata, InflightCheckResult, InstrumentAccountKey, PositionFillReportPlan,
        PositionFillReportPreparation, PositionFillReportQuery, PositionReportCheck,
        ReconciliationResult, ReportClientCoverage,
    },
};
use super::{
    recency::RecencyMap,
    reconciliation::{
        AccountInstrumentKey, AccountInstrumentStrategyKey, FillKey, HistoricalFillGroup,
        InflightCheck, PositionQuantityComparison, PositionReconciliationState,
        PositionReportShape, ReconciliationFillQueue, RetainedFillState,
        create_cross_zero_leg_report, create_orphan_fill_order_report, has_active_inferred_fill,
        is_exact_order_match, position_avg_px, position_qty_aggregates,
        resolve_inferred_fill_commission, should_project_fill, terminal_report_has_missing_fills,
    },
};

/// Tag for orders originating from venue (external orders).
static TAG_VENUE: LazyLock<Ustr> = LazyLock::new(|| Ustr::from("VENUE"));

/// Tag for orders generated by reconciliation logic (synthetic orders).
static TAG_RECONCILIATION: LazyLock<Ustr> = LazyLock::new(|| Ustr::from("RECONCILIATION"));

/// Manager for execution state.
///
/// The `ExecutionManager` handles:
/// - Startup reconciliation to align state on system start.
/// - Continuous reconciliation of inflight orders.
/// - External order discovery and claiming.
/// - Fill report processing and validation.
/// - Purging of old orders, positions, and account events.
///
/// # Thread safety
///
/// The manager shares its clock and cache through `Rc<RefCell<_>>` and stays on one thread.
/// Hosts must release cache and engine borrows before dispatching callbacks that may reenter them.
#[derive(Clone)]
pub struct ExecutionManager {
    clock: Rc<RefCell<dyn Clock>>,
    cache: Rc<RefCell<Cache>>,
    config: ExecutionManagerConfig,

    order_activity: RecencyMap<ClientOrderId>,
    order_inflight_checks: IndexMap<ClientOrderId, InflightCheck>,
    order_query_recency: RecencyMap<ClientOrderId>,
    order_query_pending: IndexSet<ClientOrderId>,
    order_recon_retries: IndexMap<ClientOrderId, u32>,
    order_coverage_unresolved: IndexSet<ClientOrderId>,
    order_coverage_warnings: IndexSet<ClientOrderId>,
    order_lookback_warnings: IndexSet<ClientOrderId>,

    fills_processed: RecencyMap<FillKey>,
    fills_recent: RecencyMap<FillKey>,

    position_activity: RecencyMap<InstrumentAccountKey>,
    position_activity_revisions: IndexMap<InstrumentAccountKey, u64>,
    position_recon: IndexMap<InstrumentAccountKey, PositionReconciliationState>,
    position_recon_tolerances: IndexMap<AccountId, Decimal>,
}

impl Debug for ExecutionManager {
    #[rustfmt::skip]
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(ExecutionManager))
            .field("clock", &self.clock)
            .field("cache", &self.cache)
            .field("config", &self.config)
            .field("order_activity", &self.order_activity)
            .field("order_inflight_checks", &self.order_inflight_checks)
            .field("order_query_recency", &self.order_query_recency)
            .field("order_query_pending", &self.order_query_pending)
            .field("order_recon_retries", &self.order_recon_retries)
            .field("order_coverage_unresolved", &self.order_coverage_unresolved)
            .field("order_coverage_warnings", &self.order_coverage_warnings)
            .field("order_lookback_warnings", &self.order_lookback_warnings)
            .field("fills_processed", &self.fills_processed)
            .field("fills_recent", &self.fills_recent)
            .field("position_activity", &self.position_activity)
            .field("position_activity_revisions", &self.position_activity_revisions)
            .field("position_recon", &self.position_recon)
            .field("position_recon_tolerances", &self.position_recon_tolerances)
            .finish()
    }
}

impl ExecutionManager {
    /// Creates a new [`ExecutionManager`] instance.
    ///
    /// # Errors
    ///
    /// Returns a [`ConfigError`](nautilus_common::config::ConfigError) if `config` fails validation.
    pub fn new(
        clock: Rc<RefCell<dyn Clock>>,
        cache: Rc<RefCell<Cache>>,
        config: ExecutionManagerConfig,
    ) -> ConfigResult<Self> {
        config.validate()?;

        Ok(Self {
            clock,
            cache,
            config,
            order_activity: RecencyMap::default(),
            order_inflight_checks: IndexMap::new(),
            order_query_recency: RecencyMap::default(),
            order_query_pending: IndexSet::new(),
            order_recon_retries: IndexMap::new(),
            order_coverage_unresolved: IndexSet::new(),
            order_coverage_warnings: IndexSet::new(),
            order_lookback_warnings: IndexSet::new(),
            fills_processed: RecencyMap::default(),
            fills_recent: RecencyMap::default(),
            position_activity: RecencyMap::default(),
            position_activity_revisions: IndexMap::new(),
            position_recon: IndexMap::new(),
            position_recon_tolerances: IndexMap::new(),
        })
    }

    /// Returns the execution manager configuration.
    pub(crate) const fn config(&self) -> &ExecutionManagerConfig {
        &self.config
    }

    /// Returns the trading clock timestamp in nanoseconds.
    pub(crate) fn timestamp_ns(&self) -> UnixNanos {
        self.clock.borrow().timestamp_ns()
    }

    /// Borrows the execution cache for reading.
    pub(crate) fn cache(&self) -> Ref<'_, Cache> {
        self.cache.borrow()
    }

    /// Registers an order as inflight for tracking.
    pub fn register_inflight(&mut self, client_order_id: ClientOrderId) {
        if self
            .config
            .filtered_client_order_ids
            .contains(&client_order_id)
        {
            return;
        }

        self.order_inflight_checks.insert(
            client_order_id,
            InflightCheck {
                submitted_at: dst::time::Instant::now(),
                retry_count: 0,
                last_query_at: None,
            },
        );

        self.order_recon_retries.insert(client_order_id, 0);
        self.order_query_recency.remove(&client_order_id);
        self.order_activity.remove(&client_order_id);
    }

    /// Records local activity for the specified order.
    ///
    /// Uses a monotonic receipt instant, not venue or domain time, to accurately
    /// track when we last processed activity for this order. This avoids race
    /// conditions where network/queue latency makes events appear "old" even
    /// though they just arrived.
    pub fn record_local_activity(&mut self, client_order_id: ClientOrderId) {
        self.order_activity.mark(client_order_id);
    }

    /// Returns the current missing-order reconciliation retry count for the
    /// given client order ID, or zero if no entry exists.
    #[must_use]
    pub fn recon_check_retry_count(&self, client_order_id: &ClientOrderId) -> u32 {
        self.order_recon_retries
            .get(client_order_id)
            .copied()
            .unwrap_or(0)
    }

    /// Clears pending targeted queries for the supplied orders.
    pub(crate) fn remove_targeted_order_queries(&mut self, client_order_ids: &[ClientOrderId]) {
        for client_order_id in client_order_ids {
            self.order_query_pending.shift_remove(client_order_id);
        }
    }

    /// Clears reconciliation tracking state for an order.
    pub fn clear_recon_tracking(&mut self, client_order_id: &ClientOrderId, drop_last_query: bool) {
        self.order_inflight_checks.shift_remove(client_order_id);
        self.order_recon_retries.shift_remove(client_order_id);
        self.order_coverage_warnings.shift_remove(client_order_id);
        self.order_lookback_warnings.shift_remove(client_order_id);
        self.order_coverage_unresolved.shift_remove(client_order_id);
        self.remove_targeted_order_queries(&[*client_order_id]);

        if drop_last_query {
            self.order_query_recency.remove(client_order_id);
        }

        self.order_activity.remove(client_order_id);
    }

    /// Prunes order activity outside the continuous reconciliation settling window.
    pub fn prune_order_local_activity(&mut self) {
        self.order_activity
            .prune_older_than(Duration::from(self.config.open_check_threshold_ns));
    }

    /// Checks if a fill has been recently processed (for deduplication).
    #[must_use]
    pub fn is_fill_recently_processed(
        &self,
        account_id: AccountId,
        instrument_id: InstrumentId,
        trade_id: TradeId,
    ) -> bool {
        self.fills_recent
            .contains_key(&(account_id, instrument_id, trade_id))
    }

    /// Marks a fill as recently processed when it is present on its canonical order.
    pub fn commit_recent_fill_if_applied(&mut self, fill: &OrderFilled) {
        let fill_key = (fill.account_id, fill.instrument_id, fill.trade_id);
        if self.is_fill_applied(fill, fill_key) {
            self.mark_fill_processed(fill_key.0, fill_key.1, fill_key.2);
        }
    }

    /// Marks a fill as recently processed with the current monotonic instant.
    pub fn mark_fill_processed(
        &mut self,
        account_id: AccountId,
        instrument_id: InstrumentId,
        trade_id: TradeId,
    ) {
        self.fills_recent
            .mark((account_id, instrument_id, trade_id));
    }

    /// Prunes expired fills from the recent fills cache.
    ///
    /// Default TTL is 60 seconds.
    pub fn prune_recent_fills_cache(&mut self, ttl_secs: f64) {
        // Map the f64 TTL to a Duration, reproducing the old
        // (ttl_secs * NANOSECONDS_IN_SECOND) as u64 cast at the boundaries
        // rather than panicking on this pub fn. The as cast saturated:
        //   - negative / NaN            -> 0        (prune everything)
        //   - positive overflow / +inf  -> u64::MAX (keep everything)
        // try_from_secs_f64 returns Err for all three, so branch on the sign
        // to keep the two behaviors distinct.
        let ttl = match Duration::try_from_secs_f64(ttl_secs) {
            Ok(ttl) => ttl,
            Err(_) if ttl_secs > 0.0 => Duration::MAX,
            Err(_) => Duration::ZERO,
        };

        self.fills_recent.prune_older_than(ttl);
    }

    /// Prunes committed mass-reconciliation fills outside the startup report window.
    ///
    /// An unbounded startup lookback requires indefinite retention because no finite
    /// horizon can safely exclude a replayed fill report.
    pub fn prune_processed_fills(&mut self) {
        let Some(lookback_mins) = self.config.lookback_mins else {
            return;
        };

        let ttl = Duration::from_mins(lookback_mins).max(Duration::from_mins(1));
        self.fills_processed.prune_older_than(ttl);
    }

    /// Sets the account tolerance, substituting the default for negative values.
    pub(crate) fn set_position_reconciliation_tolerance(
        &mut self,
        account_id: AccountId,
        tolerance: Decimal,
    ) {
        let tolerance = if tolerance < Decimal::ZERO {
            log::error!(
                "Invalid negative position reconciliation tolerance {tolerance} for \
                 {account_id}; using the default"
            );
            DEFAULT_POSITION_RECONCILIATION_TOLERANCE
        } else {
            tolerance
        };

        self.position_recon_tolerances.insert(account_id, tolerance);
    }

    /// Returns the account tolerance, falling back to the default.
    pub(crate) fn position_reconciliation_tolerance(&self, account_id: AccountId) -> Decimal {
        self.position_recon_tolerances
            .get(&account_id)
            .copied()
            .unwrap_or(DEFAULT_POSITION_RECONCILIATION_TOLERANCE)
    }

    /// Uses monotonic `dst::time` so the reconciliation grace window is unaffected
    /// by trading-clock acceleration or venue timestamps.
    pub fn record_position_activity(&mut self, instrument_id: InstrumentId, account_id: AccountId) {
        let key = (instrument_id, account_id);
        self.position_activity.mark(key);
        let revision = self.position_activity_revisions.entry(key).or_default();
        *revision = revision.saturating_add(1);
    }

    /// Checks whether position activity falls within the reconciliation grace window.
    pub(crate) fn position_activity_is_recent(&self, key: &InstrumentAccountKey) -> bool {
        self.position_activity
            .within(key, Duration::from(self.config.position_check_threshold_ns))
    }

    /// Returns the position activity revision, or zero if no activity is recorded.
    pub(crate) fn position_activity_revision(&self, key: &InstrumentAccountKey) -> u64 {
        self.position_activity_revisions
            .get(key)
            .copied()
            .unwrap_or_default()
    }

    fn set_position_reconciliation_retries(
        &mut self,
        key: InstrumentAccountKey,
        report_shape: PositionReportShape,
        retries: u32,
    ) {
        self.position_recon.insert(
            key,
            PositionReconciliationState {
                report_shape,
                retries,
            },
        );
    }

    /// Returns retries for the matching report shape, or zero if none match.
    pub(crate) fn position_reconciliation_retries(
        &self,
        key: &InstrumentAccountKey,
        report_shape: PositionReportShape,
    ) -> u32 {
        self.position_recon
            .get(key)
            .filter(|state| state.report_shape == report_shape)
            .map_or(0, |state| state.retries)
    }

    /// Returns the current position-reconciliation retry count for the given
    /// `(instrument, account)` key, or zero if no entry exists.
    #[must_use]
    pub fn position_recon_retry_count(&self, key: &InstrumentAccountKey) -> u32 {
        self.position_recon
            .get(key)
            .map_or(0, |state| state.retries)
    }

    /// Clears reconciliation retry state for an instrument and account.
    pub(crate) fn clear_position_reconciliation(&mut self, key: &InstrumentAccountKey) {
        self.position_recon.shift_remove(key);
    }

    /// Retains reconciliation retry state only for active position keys.
    pub(crate) fn retain_position_reconciliation(
        &mut self,
        active_keys: &IndexSet<InstrumentAccountKey>,
    ) {
        self.position_recon
            .retain(|key, _| active_keys.contains(key));
    }

    /// Reconciles a mass snapshot, applying order events before evaluating positions.
    ///
    /// Publishes raw reports before cache mutation and verifies each historical fill after dispatch.
    /// Returns the processed events and external orders for the host to register with its client.
    pub fn reconcile_execution_mass_status(
        &mut self,
        mass_status: &ExecutionMassStatus,
        exec_engine: &RefCell<ExecutionEngine>,
    ) -> ReconciliationResult {
        if exec_engine
            .borrow()
            .get_client(&mass_status.client_id)
            .is_none()
        {
            log::error!(
                "Cannot reconcile ExecutionMassStatus from unknown client {}",
                mass_status.client_id
            );
            return ReconciliationResult::default();
        }

        self.validate_mass_status_order_sources(mass_status);

        // Publish raw reports before any state mutation (including fill adjustment
        // below, which can synthesize replacement order/fill reports). The
        // execution engine's per-report `reconcile_*` entry points are bypassed by
        // this path, so the capture seam lives here.
        let raw_order_status_topic =
            MessagingSwitchboard::reconciliation_raw_order_status_report_topic();

        for report in mass_status.order_reports().values() {
            msgbus::publish_any(raw_order_status_topic, report);
        }

        let raw_fill_topic = MessagingSwitchboard::reconciliation_raw_fill_report_topic();

        for fills in mass_status.fill_reports().values() {
            for fill in fills {
                msgbus::publish_any(raw_fill_topic, fill);
            }
        }

        let raw_position_topic =
            MessagingSwitchboard::reconciliation_raw_position_status_report_topic();

        for reports in mass_status.position_reports().values() {
            for report in reports {
                msgbus::publish_any(raw_position_topic, report);
            }
        }

        if exec_engine
            .borrow()
            .get_client(&mass_status.client_id)
            .is_none()
        {
            log::error!(
                "Execution client {} disappeared while publishing raw mass status reports",
                mass_status.client_id
            );
            return ReconciliationResult::default();
        }

        let venue = mass_status.venue;
        let order_count = mass_status.order_reports().len();
        let fill_count: usize = mass_status.fill_reports().values().map(Vec::len).sum();
        let position_count: usize = mass_status.position_reports().values().map(Vec::len).sum();

        log_info!(
            "Reconciling ExecutionMassStatus for {venue}",
            color = LogColor::Blue
        );
        log_info!(
            "Received {order_count} order(s), {fill_count} fill(s), {position_count} position(s)",
            color = LogColor::Blue
        );

        let retained_fill_state = self.retained_fill_state();
        let reported_fill_keys: IndexSet<(AccountId, InstrumentId, TradeId)> = mass_status
            .fill_reports()
            .values()
            .flatten()
            .filter(|fill| !fill.last_qty.is_zero())
            .map(|fill| (fill.account_id, fill.instrument_id, fill.trade_id))
            .collect();
        let (adjusted_order_reports, adjusted_fill_reports) =
            self.adjust_mass_status_fills(mass_status);
        let order_only_venue_order_ids = self.order_only_venue_order_ids(
            mass_status,
            &adjusted_order_reports,
            &adjusted_fill_reports,
            &retained_fill_state,
        );

        let mut events = Vec::new();
        let mut external_orders = Vec::new();
        let mut orders_reconciled = 0usize;
        let mut external_orders_created = 0usize;
        let mut open_orders_initialized = 0usize;
        let mut orders_skipped_no_instrument = 0usize;
        let mut orders_skipped_duplicate = 0usize;
        let mut fills_applied = 0usize;
        let mut fill_queue = ReconciliationFillQueue::default();

        let fill_reports = &adjusted_fill_reports;
        let mut seen_fill_keys: IndexSet<FillKey> = IndexSet::new();

        for fills in fill_reports.values() {
            for fill in fills {
                let fill_key = (fill.account_id, fill.instrument_id, fill.trade_id);
                if !seen_fill_keys.insert(fill_key) {
                    log::warn!(
                        "Duplicate trade_id {} for {} in mass status",
                        fill.trade_id,
                        fill.instrument_id
                    );
                }
            }
        }

        let order_reports = &adjusted_order_reports;
        let mut orders_skipped_filtered = 0usize;

        for report in order_reports.values() {
            if self.should_skip_order_report(report) {
                orders_skipped_filtered += 1;
                continue;
            }

            if let Some(client_order_id) = &report.client_order_id {
                if let Some(cached_order) = self.get_order(*client_order_id)
                    && is_exact_order_match(&cached_order, report)
                {
                    log::debug!("Skipping order {client_order_id}: already in sync with venue");
                    orders_skipped_duplicate += 1;

                    // Still ensure venue_order_id is indexed even when skipping
                    if let Err(e) = self
                        .cache
                        .borrow_mut()
                        .index_venue_order_id(client_order_id, &report.venue_order_id)
                    {
                        log::warn!("Failed to index venue order ID: {e}");
                    }

                    continue;
                }

                // Skip closed reconciliation orders to prevent duplicate inferred fills on restart
                if let Some(cached_order) = self.get_order(*client_order_id)
                    && cached_order.is_closed()
                    && cached_order
                        .tags()
                        .is_some_and(|tags| tags.contains(&*TAG_RECONCILIATION))
                {
                    log::debug!(
                        "Skipping closed reconciliation order {client_order_id}: \
                         synthetic position adjustment from previous session",
                    );
                    orders_skipped_duplicate += 1;
                    continue;
                }

                if let Some(order) = self.get_order(*client_order_id) {
                    let instrument = self.get_instrument(&report.instrument_id);
                    log::info!(
                        color = LogColor::Blue as u8;
                        "Reconciling {} {} {} [{}] -> [{}]",
                        client_order_id,
                        report.venue_order_id,
                        report.instrument_id,
                        order.status(),
                        report.order_status,
                    );

                    let order_fills: Vec<&FillReport> = fill_reports
                        .get(&report.venue_order_id)
                        .map(|f| f.iter().collect())
                        .unwrap_or_default();
                    let engine_ref = exec_engine.borrow();
                    let commission_client = engine_ref.get_client(&mass_status.client_id);

                    let order_events = self.reconcile_order_with_fills(
                        true,
                        &order,
                        report,
                        &order_fills,
                        instrument.as_ref(),
                        &mut fill_queue,
                        commission_client,
                    );

                    drop(engine_ref);

                    if !order_events.is_empty() {
                        orders_reconciled += 1;
                        fills_applied += order_events
                            .iter()
                            .filter(|e| matches!(e, OrderEventAny::Filled(_)))
                            .count();
                        events.extend(order_events);
                    }

                    // Always ensure venue_order_id is indexed after reconciliation
                    if let Err(e) = self
                        .cache
                        .borrow_mut()
                        .index_venue_order_id(client_order_id, &report.venue_order_id)
                    {
                        log::warn!("Failed to index venue order ID: {e}");
                    }
                } else if let Some(order) = self.get_order_by_venue_order_id(report.venue_order_id)
                {
                    // Fallback: match by venue_order_id
                    let instrument = self.get_instrument(&report.instrument_id);

                    log::info!(
                        color = LogColor::Blue as u8;
                        "Reconciling {} (matched by venue_order_id {}) {} [{}] -> [{}]",
                        order.client_order_id(),
                        report.venue_order_id,
                        report.instrument_id,
                        order.status(),
                        report.order_status,
                    );

                    let order_fills: Vec<&FillReport> = fill_reports
                        .get(&report.venue_order_id)
                        .map(|f| f.iter().collect())
                        .unwrap_or_default();
                    let engine_ref = exec_engine.borrow();
                    let commission_client = engine_ref.get_client(&mass_status.client_id);

                    let order_events = self.reconcile_order_with_fills(
                        true,
                        &order,
                        report,
                        &order_fills,
                        instrument.as_ref(),
                        &mut fill_queue,
                        commission_client,
                    );

                    drop(engine_ref);

                    if !order_events.is_empty() {
                        orders_reconciled += 1;
                        fills_applied += order_events
                            .iter()
                            .filter(|e| matches!(e, OrderEventAny::Filled(_)))
                            .count();
                        events.extend(order_events);
                    }

                    if let Err(e) = self
                        .cache
                        .borrow_mut()
                        .index_venue_order_id(&order.client_order_id(), &report.venue_order_id)
                    {
                        log::warn!("Failed to index venue order ID: {e}");
                    }
                } else if let Some(instrument) = self.get_instrument(&report.instrument_id) {
                    let order_fills: Vec<&FillReport> = fill_reports
                        .get(&report.venue_order_id)
                        .map(|f| f.iter().collect())
                        .unwrap_or_default();
                    let engine_ref = exec_engine.borrow();
                    let commission_client = engine_ref.get_client(&mass_status.client_id);

                    let (external_events, metadata) = self.handle_external_order(
                        report,
                        mass_status.account_id,
                        &instrument,
                        &order_fills,
                        false, // Not synthetic (venue order)
                        Some(&mut fill_queue),
                        commission_client,
                    );

                    drop(engine_ref);

                    if !external_events.is_empty() {
                        external_orders_created += 1;
                        fills_applied += external_events
                            .iter()
                            .filter(|e| matches!(e, OrderEventAny::Filled(_)))
                            .count();

                        if report.order_status.is_open() {
                            open_orders_initialized += 1;
                        }

                        events.extend(external_events);

                        if let Some(m) = metadata {
                            external_orders.push(m);
                        }
                    }
                } else {
                    orders_skipped_no_instrument += 1;
                }
            } else if let Some(order) = self.get_order_by_venue_order_id(report.venue_order_id) {
                // Fallback: match by venue_order_id
                let instrument = self.get_instrument(&report.instrument_id);
                log::info!(
                    color = LogColor::Blue as u8;
                    "Reconciling {} (matched by venue_order_id {}) {} [{}] -> [{}]",
                    order.client_order_id(),
                    report.venue_order_id,
                    report.instrument_id,
                    order.status(),
                    report.order_status,
                );

                let order_fills: Vec<&FillReport> = fill_reports
                    .get(&report.venue_order_id)
                    .map(|f| f.iter().collect())
                    .unwrap_or_default();
                let engine_ref = exec_engine.borrow();
                let commission_client = engine_ref.get_client(&mass_status.client_id);

                let order_events = self.reconcile_order_with_fills(
                    true,
                    &order,
                    report,
                    &order_fills,
                    instrument.as_ref(),
                    &mut fill_queue,
                    commission_client,
                );

                drop(engine_ref);

                if !order_events.is_empty() {
                    orders_reconciled += 1;
                    fills_applied += order_events
                        .iter()
                        .filter(|e| matches!(e, OrderEventAny::Filled(_)))
                        .count();
                    events.extend(order_events);
                }

                if let Err(e) = self
                    .cache
                    .borrow_mut()
                    .index_venue_order_id(&order.client_order_id(), &report.venue_order_id)
                {
                    log::warn!("Failed to index venue order ID: {e}");
                }
            } else if let Some(instrument) = self.get_instrument(&report.instrument_id) {
                // Synthetic orders (S- prefix) are generated by reconciliation logic
                let is_synthetic = report.venue_order_id.as_str().starts_with("S-");

                let order_fills: Vec<&FillReport> = fill_reports
                    .get(&report.venue_order_id)
                    .map(|f| f.iter().collect())
                    .unwrap_or_default();
                let engine_ref = exec_engine.borrow();
                let commission_client = engine_ref.get_client(&mass_status.client_id);

                let (external_events, metadata) = self.handle_external_order(
                    report,
                    mass_status.account_id,
                    &instrument,
                    &order_fills,
                    is_synthetic,
                    Some(&mut fill_queue),
                    commission_client,
                );

                drop(engine_ref);

                if !external_events.is_empty() {
                    external_orders_created += 1;
                    fills_applied += external_events
                        .iter()
                        .filter(|e| matches!(e, OrderEventAny::Filled(_)))
                        .count();

                    if report.order_status.is_open() {
                        open_orders_initialized += 1;
                    }

                    events.extend(external_events);

                    if let Some(m) = metadata {
                        external_orders.push(m);
                    }
                }
            } else {
                orders_skipped_no_instrument += 1;
            }
        }

        // Process orphan fills (fills without matching order reports)
        let processed_venue_order_ids: IndexSet<VenueOrderId> =
            order_reports.keys().copied().collect();

        for (venue_order_id, fills) in fill_reports {
            if processed_venue_order_ids.contains(venue_order_id) {
                continue;
            }

            let Some(first_fill) = fills.first() else {
                continue;
            };

            if !self.should_reconcile_instrument(&first_fill.instrument_id) {
                log::debug!(
                    "Skipping orphan fills for {}: not in reconciliation_instrument_ids",
                    first_fill.instrument_id
                );
                continue;
            }

            // Skip if fill's client_order_id is in filtered list
            if let Some(client_order_id) = &first_fill.client_order_id
                && self
                    .config
                    .filtered_client_order_ids
                    .contains(client_order_id)
            {
                log::debug!(
                    "Skipping orphan fills for {client_order_id}: in filtered_client_order_ids"
                );
                continue;
            }

            let order = first_fill
                .client_order_id
                .as_ref()
                .and_then(|id| self.get_order(*id))
                .or_else(|| self.get_order_by_venue_order_id(*venue_order_id));

            // Skip if resolved order's client_order_id is filtered (venue_order_id lookup path)
            if let Some(ref order) = order
                && self
                    .config
                    .filtered_client_order_ids
                    .contains(&order.client_order_id())
            {
                log::debug!(
                    "Skipping orphan fills for {}: in filtered_client_order_ids",
                    order.client_order_id()
                );
                continue;
            }

            if let Some(order) = order {
                let instrument_id = order.instrument_id();
                if let Some(instrument) = self.get_instrument(&instrument_id) {
                    let mut sorted_fills: Vec<&FillReport> = fills.iter().collect();
                    sorted_fills.sort_by_key(|f| f.ts_event);

                    for fill in sorted_fills {
                        if let Some((event, fill_key)) = self.create_order_fill(
                            &order,
                            fill,
                            &instrument,
                            &fill_queue.pending_fill_keys,
                        ) {
                            fills_applied += 1;
                            fill_queue.push(&mut events, event, fill_key);
                        }
                    }
                } else {
                    orders_skipped_no_instrument += 1;
                }
            } else if fills.iter().any(FillReport::has_venue_position_id) {
                if !self.config.generate_missing_orders {
                    log::debug!(
                        "Skipping orphan fills for venue order {venue_order_id}: \
                         `generate_missing_orders` is disabled"
                    );
                    orders_skipped_filtered += 1;
                    continue;
                }

                let Some(instrument) = self.get_instrument(&first_fill.instrument_id) else {
                    orders_skipped_no_instrument += 1;
                    continue;
                };

                let mut sorted_fills: Vec<&FillReport> = fills.iter().collect();
                sorted_fills.sort_by_key(|fill| fill.ts_event);

                let report = match create_orphan_fill_order_report(&sorted_fills, &instrument) {
                    Ok(report) => report,
                    Err(e) => {
                        log::error!(
                            "Cannot materialize orphan fills for venue order {venue_order_id}: {e}"
                        );

                        continue;
                    }
                };

                let engine_ref = exec_engine.borrow();
                let commission_client = engine_ref.get_client(&mass_status.client_id);

                let (external_events, metadata) = self.handle_external_order(
                    &report,
                    mass_status.account_id,
                    &instrument,
                    &sorted_fills,
                    false,
                    Some(&mut fill_queue),
                    commission_client,
                );

                drop(engine_ref);

                if !external_events.is_empty() {
                    external_orders_created += 1;
                    fills_applied += external_events
                        .iter()
                        .filter(|event| matches!(event, OrderEventAny::Filled(_)))
                        .count();

                    events.extend(external_events);

                    if let Some(metadata) = metadata {
                        external_orders.push(metadata);
                    }
                }
            }
        }

        events.sort_by_key(OrderEventAny::ts_event);

        let mut unapplied_fill_position_ids = IndexSet::new();

        for event in &events {
            if let OrderEventAny::Filled(fill) = event
                && should_project_fill(
                    fill,
                    &retained_fill_state,
                    &reported_fill_keys,
                    &order_only_venue_order_ids,
                )
            {
                exec_engine.borrow_mut().project_reconciliation_fill(fill);
            } else {
                exec_engine.borrow_mut().process(event);
            }

            if let OrderEventAny::Filled(fill) = event
                && let Some(fill_key) = fill_queue.event_fill_keys.get(&fill.event_id).copied()
            {
                if self.is_fill_applied(fill, fill_key) {
                    self.fills_processed.mark(fill_key);
                } else if let Some(venue_position_id) = fill.position_id {
                    log::error!(
                        "Skipping reconciliation for venue position {venue_position_id}: historical fill {} was not applied",
                        fill.trade_id,
                    );

                    unapplied_fill_position_ids.insert(venue_position_id);
                }
            }
        }

        let mut positions_created = 0usize;

        if !self.config.filter_position_reports {
            // Collect instruments with fills that lack venue_position_id (can't attribute to
            // specific hedge position, so must skip all hedge reports for that instrument)
            let instruments_with_unattributed_fills: IndexSet<InstrumentId> = mass_status
                .fill_reports()
                .values()
                .flatten()
                .filter(|f| !f.last_qty.is_zero() && f.venue_position_id.is_none())
                .map(|f| f.instrument_id)
                .chain(
                    mass_status
                        .order_reports()
                        .values()
                        .filter(|r| !r.filled_qty.is_zero() && r.venue_position_id.is_none())
                        .map(|r| r.instrument_id),
                )
                .collect();

            for (instrument_id, reports) in mass_status.position_reports() {
                if !self.should_reconcile_instrument(&instrument_id) {
                    log::debug!(
                        "Skipping position reports for {instrument_id}: not in reconciliation_instrument_ids"
                    );
                    continue;
                }

                for report in reports {
                    if report.venue_position_id.is_some_and(|venue_position_id| {
                        unapplied_fill_position_ids.contains(&venue_position_id)
                    }) {
                        continue;
                    }

                    if let Some(position_events) = self.reconcile_position_report(
                        &report,
                        mass_status.account_id,
                        &instruments_with_unattributed_fills,
                    ) {
                        for event in position_events {
                            exec_engine.borrow_mut().process(&event);
                            events.push(event);
                        }

                        positions_created += 1;
                    }
                }
            }
        }

        if orders_skipped_no_instrument > 0 {
            log::warn!("{orders_skipped_no_instrument} orders skipped (instrument not in cache)");
        }

        if orders_skipped_duplicate > 0 {
            log::debug!("{orders_skipped_duplicate} orders skipped (already in sync)");
        }

        if orders_skipped_filtered > 0 {
            log::debug!("{orders_skipped_filtered} orders skipped (filtered by config)");
        }

        log::info!(
            color = LogColor::Blue as u8;
            "Reconciliation complete for {venue}: reconciled={orders_reconciled}, external={external_orders_created}, open={open_orders_initialized}, fills={fills_applied}, positions={positions_created}, skipped={orders_skipped_duplicate}, filtered={orders_skipped_filtered}",
        );

        ReconciliationResult {
            events,
            external_orders,
        }
    }

    fn retained_fill_state(&self) -> RetainedFillState {
        let cache = self.cache.borrow();
        let positions = cache.positions(None, None, None, None, None);
        let mut fill_keys = IndexSet::new();
        let mut missing_order_ids = IndexSet::new();
        let mut missing_venue_order_ids = IndexSet::new();
        let mut netting_lifecycle_starts = IndexMap::new();

        for position in positions {
            for fill in &position.events {
                fill_keys.insert((position.account_id, position.instrument_id, fill.trade_id));
                if cache.order(&fill.client_order_id).is_none() {
                    missing_order_ids.insert((
                        position.account_id,
                        position.instrument_id,
                        fill.client_order_id,
                    ));
                    missing_venue_order_ids.insert((
                        position.account_id,
                        position.instrument_id,
                        fill.venue_order_id,
                    ));
                }
            }

            if cache.oms_type(&position.id) == Some(OmsType::Netting) {
                netting_lifecycle_starts.insert(
                    (
                        position.account_id,
                        position.instrument_id,
                        position.strategy_id,
                    ),
                    position.ts_opened,
                );
            }
        }

        RetainedFillState {
            fill_keys,
            missing_order_ids,
            missing_venue_order_ids,
            netting_lifecycle_starts,
        }
    }

    fn order_only_venue_order_ids(
        &self,
        mass_status: &ExecutionMassStatus,
        order_reports: &IndexMap<VenueOrderId, OrderStatusReport>,
        fill_reports: &IndexMap<VenueOrderId, Vec<FillReport>>,
        retained_fill_state: &RetainedFillState,
    ) -> IndexSet<VenueOrderId> {
        if mass_status.lookback_start().is_none() {
            return IndexSet::new();
        }

        let expected_quantities: IndexMap<AccountInstrumentKey, Decimal> =
            if mass_status.reports_complete() {
                mass_status
                    .position_reports()
                    .into_iter()
                    .filter_map(|(instrument_id, reports)| {
                        let [report] = reports.as_slice() else {
                            return None;
                        };

                        report.venue_position_id.is_none().then_some((
                            (report.account_id, instrument_id),
                            report.signed_decimal_qty,
                        ))
                    })
                    .collect()
            } else {
                IndexMap::new()
            };

        let candidate_instruments: IndexSet<InstrumentId> = order_reports
            .values()
            .filter(|report| !report.filled_qty.is_zero())
            .map(|report| report.instrument_id)
            .chain(
                fill_reports
                    .values()
                    .flatten()
                    .map(|fill| fill.instrument_id),
            )
            .collect();

        if candidate_instruments.is_empty() {
            return IndexSet::new();
        }

        let mut venue_order_ids: IndexSet<VenueOrderId> = order_reports
            .iter()
            .filter(|(_, report)| {
                candidate_instruments.contains(&report.instrument_id)
                    && !report.filled_qty.is_zero()
            })
            .map(|(venue_order_id, _)| *venue_order_id)
            .collect();

        venue_order_ids.extend(fill_reports.iter().filter_map(|(venue_order_id, fills)| {
            fills
                .first()
                .is_some_and(|fill| candidate_instruments.contains(&fill.instrument_id))
                .then_some(*venue_order_id)
        }));

        if !mass_status.reports_complete() {
            log::error!(
                "Bounded reconciliation report set is incomplete; projecting {} historical order(s) without position or portfolio effects",
                venue_order_ids.len(),
            );

            return venue_order_ids;
        }

        let mut order_only = IndexSet::new();
        let mut groups = Vec::new();

        for venue_order_id in venue_order_ids {
            let report = order_reports.get(&venue_order_id);
            let fills = fill_reports.get(&venue_order_id);

            if report.and_then(|report| report.venue_position_id).is_some()
                || fills.is_some_and(|fills| fills.iter().any(FillReport::has_venue_position_id))
            {
                continue;
            }

            let cached_order = report
                .and_then(|report| report.client_order_id)
                .and_then(|client_order_id| self.get_order(client_order_id))
                .or_else(|| self.get_order_by_venue_order_id(venue_order_id));
            let account_id = report
                .map(|report| report.account_id)
                .or_else(|| fills.and_then(|fills| fills.first().map(|fill| fill.account_id)));
            let instrument_id = report
                .map(|report| report.instrument_id)
                .or_else(|| fills.and_then(|fills| fills.first().map(|fill| fill.instrument_id)));
            let order_side = report
                .and_then(|report| report.order_side)
                .or_else(|| fills.and_then(|fills| fills.first().map(|fill| fill.order_side)));

            let (Some(account_id), Some(instrument_id), Some(order_side)) =
                (account_id, instrument_id, order_side)
            else {
                order_only.insert(venue_order_id);
                continue;
            };

            let coherent_fills = fills.is_none_or(|fills| {
                fills.iter().all(|fill| {
                    fill.account_id == account_id
                        && fill.instrument_id == instrument_id
                        && fill.order_side == order_side
                })
            });

            let coherent_cached_order = cached_order.as_ref().is_none_or(|order| {
                order.instrument_id() == instrument_id
                    && order.order_side() == order_side
                    && order.account_id().is_none_or(|id| id == account_id)
            });

            if !coherent_fills
                || !coherent_cached_order
                || (report.is_none() && cached_order.is_none())
            {
                order_only.insert(venue_order_id);
                continue;
            }

            let strategy_id = cached_order.as_ref().map_or_else(
                || {
                    self.cache
                        .borrow()
                        .external_order_claim(&instrument_id)
                        .unwrap_or_else(|| StrategyId::from("EXTERNAL"))
                },
                Order::strategy_id,
            );

            let reduce_only = report.is_some_and(|report| report.reduce_only)
                || cached_order.as_ref().is_some_and(Order::is_reduce_only);

            let cached_filled_qty = cached_order
                .as_ref()
                .map_or(Decimal::ZERO, |order| order.filled_qty().as_decimal());

            let reported_fill_qty = fills.map_or(Decimal::ZERO, |fills| {
                fills.iter().map(|fill| fill.last_qty.as_decimal()).sum()
            });

            let unretained_fills: Vec<&FillReport> = fills
                .into_iter()
                .flatten()
                .filter(|fill| {
                    !retained_fill_state.fill_keys.contains(&(
                        fill.account_id,
                        fill.instrument_id,
                        fill.trade_id,
                    ))
                })
                .collect();

            let unretained_fill_qty: Decimal = unretained_fills
                .iter()
                .map(|fill| fill.last_qty.as_decimal())
                .sum();

            let inferred_qty = report.map_or(Decimal::ZERO, |report| {
                (report.filled_qty.as_decimal() - cached_filled_qty - reported_fill_qty)
                    .max(Decimal::ZERO)
            });

            let quantity = unretained_fill_qty + inferred_qty;

            if quantity.is_zero() {
                continue;
            }

            let inferred_ts = (!inferred_qty.is_zero())
                .then(|| report.map(|report| report.ts_last))
                .flatten();
            let ts_event = unretained_fills
                .iter()
                .map(|fill| fill.ts_event)
                .chain(inferred_ts)
                .min()
                .unwrap_or(mass_status.ts_init);
            let ts_last = unretained_fills
                .iter()
                .map(|fill| fill.ts_event)
                .chain(inferred_ts)
                .max()
                .unwrap_or(mass_status.ts_init);

            groups.push(HistoricalFillGroup {
                venue_order_id,
                account_id,
                instrument_id,
                strategy_id,
                order_side,
                quantity,
                reduce_only,
                ts_event,
                ts_last,
            });
        }

        groups.sort_by_key(|group| group.ts_event);

        let mut quantities: IndexMap<AccountInstrumentStrategyKey, Option<Decimal>> =
            IndexMap::new();
        let mut group_ids: IndexMap<AccountInstrumentStrategyKey, Vec<VenueOrderId>> =
            IndexMap::new();
        let mut interval_ends: IndexMap<AccountInstrumentStrategyKey, UnixNanos> = IndexMap::new();
        let mut ambiguous_keys = IndexSet::new();

        for group in &groups {
            let key = (group.account_id, group.instrument_id, group.strategy_id);

            if interval_ends
                .get(&key)
                .is_some_and(|end| group.ts_event <= *end)
            {
                ambiguous_keys.insert(key);
            }

            interval_ends
                .entry(key)
                .and_modify(|end| *end = (*end).max(group.ts_last))
                .or_insert(group.ts_last);
        }

        if !ambiguous_keys.is_empty() {
            log::error!(
                "Bounded reconciliation contains interleaved order fills for {} position key(s); projecting their historical order state only",
                ambiguous_keys.len(),
            );
        }

        for group in groups {
            let key = (group.account_id, group.instrument_id, group.strategy_id);
            group_ids.entry(key).or_default().push(group.venue_order_id);
            if ambiguous_keys.contains(&key) {
                order_only.insert(group.venue_order_id);
                continue;
            }

            let current_qty = quantities.entry(key).or_insert_with(|| {
                let cache = self.cache.borrow();
                let positions = cache.positions_open(
                    None,
                    Some(&group.instrument_id),
                    Some(&group.strategy_id),
                    Some(&group.account_id),
                    None,
                );

                if positions.len() > 1
                    || positions.first().is_some_and(|position| {
                        cache.oms_type(&position.id) != Some(OmsType::Netting)
                    })
                {
                    None
                } else {
                    Some(
                        positions
                            .first()
                            .map_or(Decimal::ZERO, |position| position.signed_decimal_qty()),
                    )
                }
            });

            let Some(current_qty) = current_qty else {
                order_only.insert(group.venue_order_id);
                continue;
            };

            let signed_fill_qty = match group.order_side {
                OrderSide::Buy => group.quantity,
                OrderSide::Sell => -group.quantity,
            };

            let reduces = !current_qty.is_zero()
                && current_qty.is_sign_negative() != signed_fill_qty.is_sign_negative()
                && group.quantity <= current_qty.abs();
            if group.reduce_only && !reduces {
                log::warn!(
                    "Cannot apply bounded reduce-only order {} for {} without a coherent predecessor; projecting order state only",
                    group.venue_order_id,
                    group.instrument_id,
                );
                order_only.insert(group.venue_order_id);
                continue;
            }

            *current_qty += signed_fill_qty;
        }

        let mut keys_by_position: IndexMap<
            AccountInstrumentKey,
            Vec<AccountInstrumentStrategyKey>,
        > = IndexMap::new();

        for key in quantities.keys() {
            keys_by_position
                .entry((key.0, key.1))
                .or_default()
                .push(*key);
        }

        for (position_key, keys) in keys_by_position {
            let expected_qty = expected_quantities.get(&position_key).copied();

            let matches_report = if expected_qty.is_some_and(|quantity| quantity.is_zero()) {
                keys.iter().all(|key| {
                    quantities
                        .get(key)
                        .copied()
                        .flatten()
                        .is_some_and(|quantity| quantity.is_zero())
                })
            } else if let (Some(expected_qty), [key]) = (expected_qty, keys.as_slice()) {
                let cache = self.cache.borrow();
                let positions = cache.positions_open(
                    None,
                    Some(&position_key.1),
                    None,
                    Some(&position_key.0),
                    None,
                );

                let cache_is_unambiguous = positions.len() <= 1
                    && positions.first().is_none_or(|position| {
                        position.strategy_id == key.2
                            && cache.oms_type(&position.id) == Some(OmsType::Netting)
                    });

                cache_is_unambiguous
                    && quantities
                        .get(key)
                        .copied()
                        .flatten()
                        .is_some_and(|quantity| quantity == expected_qty)
            } else {
                false
            };

            if matches_report {
                continue;
            }

            let venue_order_ids: Vec<VenueOrderId> = keys
                .iter()
                .filter_map(|key| group_ids.get(key))
                .flatten()
                .copied()
                .collect();
            log::error!(
                "Bounded reconciliation does not explain the reported position for {}; projecting {} historical order(s) without position or portfolio effects",
                position_key.1,
                venue_order_ids.len(),
            );
            order_only.extend(venue_order_ids);
        }

        order_only
    }

    /// Validates cached order origins against the mass status client, logging a warning for each
    /// kind of violation. Never fails: orders persisted before origin tracking or materialized at
    /// runtime lack origins legitimately, so reconciliation proceeds regardless.
    fn validate_mass_status_order_sources(&self, mass_status: &ExecutionMassStatus) {
        let cache = self.cache.borrow();
        let mut checked_client_order_ids = IndexSet::new();
        let mut missing_origins: Vec<ClientOrderId> = Vec::new();
        let mut mismatched_origins: Vec<(ClientOrderId, ClientId)> = Vec::new();

        let mut validate_report_source =
            |direct_client_order_id: Option<ClientOrderId>, venue_order_id: VenueOrderId| {
                let direct_client_order_id = direct_client_order_id
                    .filter(|client_order_id| cache.order_exists(client_order_id));
                let indexed_client_order_id = cache
                    .client_order_id(&venue_order_id)
                    .copied()
                    .filter(|client_order_id| cache.order_exists(client_order_id));

                for client_order_id in [direct_client_order_id, indexed_client_order_id]
                    .into_iter()
                    .flatten()
                    .filter(|client_order_id| checked_client_order_ids.insert(*client_order_id))
                {
                    match cache.client_id(&client_order_id) {
                        Some(cached_client_id) if *cached_client_id == mass_status.client_id => {}
                        Some(cached_client_id) => {
                            mismatched_origins.push((client_order_id, *cached_client_id));
                        }
                        None => missing_origins.push(client_order_id),
                    }
                }
            };

        for report in mass_status.order_reports().values() {
            validate_report_source(report.client_order_id, report.venue_order_id);
        }

        for fills in mass_status.fill_reports().values() {
            for fill in fills {
                validate_report_source(fill.client_order_id, fill.venue_order_id);
            }
        }

        if !missing_origins.is_empty() {
            let samples = missing_origins
                .iter()
                .take(5)
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(", ");

            log::warn!(
                "Found {} cached order(s) without an execution client origin ({}): \
                 continuing reconciliation against mass status client {} for compatibility \
                 with existing cache data",
                missing_origins.len(),
                samples,
                mass_status.client_id,
            );
        }

        if !mismatched_origins.is_empty() {
            let samples = mismatched_origins
                .iter()
                .take(5)
                .map(|(client_order_id, cached)| format!("{client_order_id} -> {cached}"))
                .collect::<Vec<_>>()
                .join(", ");

            log::warn!(
                "Found {} cached order(s) with an execution client origin conflicting with \
                 mass status client {} ({}): continuing reconciliation for compatibility; \
                 this conflict will become a startup error in a future release, verify cached \
                 order ownership and execution client configuration",
                mismatched_origins.len(),
                mass_status.client_id,
                samples,
            );
        }
    }

    /// Checks inflight orders and returns terminal events and intermediate venue queries.
    ///
    /// For retries below `inflight_max_retries`, generates `QueryOrder` commands to poll
    /// the venue for the order's current status. At max retries, generates terminal events
    /// (rejection or cancellation) based on the order's status.
    pub fn check_inflight_orders(&mut self) -> InflightCheckResult {
        let mut result = InflightCheckResult::default();
        let now = dst::time::Instant::now();
        let threshold = Duration::from_millis(self.config().inflight_threshold_ms);

        let mut to_check = Vec::new();

        for (client_order_id, check) in &self.order_inflight_checks {
            if now
                .checked_duration_since(check.submitted_at)
                .is_some_and(|elapsed| elapsed > threshold)
            {
                to_check.push(*client_order_id);
            }
        }

        for client_order_id in to_check {
            if self
                .config
                .filtered_client_order_ids
                .contains(&client_order_id)
            {
                self.clear_recon_tracking(&client_order_id, true);
                continue;
            }

            if self.order_query_pending.contains(&client_order_id) {
                continue;
            }

            if let Some(check) = self.order_inflight_checks.get_mut(&client_order_id) {
                if let Some(last_query_at) = check.last_query_at
                    && now
                        .checked_duration_since(last_query_at)
                        .is_none_or(|elapsed| elapsed < threshold)
                {
                    continue;
                }

                check.retry_count += 1;
                check.last_query_at = Some(now);
                self.order_query_recency.mark(client_order_id);
                self.order_recon_retries
                    .insert(client_order_id, check.retry_count);

                if check.retry_count >= self.config.inflight_max_retries {
                    let ts_now = self.clock.borrow().timestamp_ns();

                    if let Some(order) = self.get_order(client_order_id) {
                        match order.status() {
                            OrderStatus::Submitted => {
                                // Generate rejection for submitted orders that never got accepted
                                if let Some(event) = create_reconciliation_rejected(
                                    &order,
                                    Some("INFLIGHT_TIMEOUT"),
                                    ts_now,
                                ) {
                                    result.events.push(event);
                                }
                            }
                            OrderStatus::PendingUpdate | OrderStatus::PendingCancel => {
                                // Generate cancellation for orders stuck in pending modify/cancel
                                let event = OrderEventAny::Canceled(OrderCanceled::new(
                                    order.trader_id(),
                                    order.strategy_id(),
                                    order.instrument_id(),
                                    order.client_order_id(),
                                    UUID4::new(),
                                    ts_now,
                                    ts_now,
                                    true, // reconciliation
                                    order.venue_order_id(),
                                    order.account_id(),
                                    None,
                                ));
                                result.events.push(event);
                            }
                            _ => {
                                // Order already resolved, just clear tracking
                            }
                        }
                    }

                    // Remove from inflight checks regardless of whether order exists
                    self.clear_recon_tracking(&client_order_id, true);
                } else if let Some(order) = self.get_order(client_order_id) {
                    // Intermediate retry: query the venue for current order status
                    let ts_now = self.clock.borrow().timestamp_ns();
                    let client_id = self.cache.borrow().client_id(&client_order_id).copied();
                    let query = TradingCommand::QueryOrder(QueryOrder::new(
                        order.trader_id(),
                        client_id,
                        order.strategy_id(),
                        order.instrument_id(),
                        order.client_order_id(),
                        order.venue_order_id(),
                        UUID4::new(),
                        ts_now,
                        None,
                        None, // correlation_id
                    ));
                    result.queries.push(query);
                }
            }
        }

        result
    }

    fn filtered_open_orders_for_reconciliation(&self) -> Vec<OrderAny> {
        let cache = self.cache.borrow();
        let mut orders = cache.orders_open(None, None, None, None, None);
        orders.extend(cache.orders_inflight(None, None, None, None, None));
        let mut seen_client_order_ids = IndexSet::new();

        orders
            .into_iter()
            .filter(|order| {
                seen_client_order_ids.insert(order.client_order_id())
                    && !self
                        .config
                        .filtered_client_order_ids
                        .contains(&order.client_order_id())
                    && self.should_reconcile_instrument(&order.instrument_id())
            })
            .map(|order| order.clone())
            .collect()
    }

    fn open_position_keys_for_reconciliation(&self) -> IndexSet<InstrumentAccountKey> {
        let cache = self.cache.borrow();
        let positions = cache.positions_open(None, None, None, None, None);
        let mut position_keys = IndexSet::new();

        for position in positions {
            if !self.should_reconcile_instrument(&position.instrument_id) {
                continue;
            }

            position_keys.insert((position.instrument_id, position.account_id));
        }

        position_keys
    }

    /// Collects open-order reports and targeted follow-ups, returning reconciliation events.
    ///
    /// The caller applies the returned events to its execution engine.
    pub async fn check_open_orders(
        &mut self,
        clients: &[&dyn ExecutionClient],
    ) -> Vec<OrderEventAny> {
        log::debug!("Checking order consistency between cached-state and venues");

        let check = self.prepare_open_order_report_check(UUID4::new(), clients);
        let mut all_reports = Vec::new();
        let mut queried_clients = IndexSet::new();
        let mut failed_clients = IndexSet::new();

        for client in clients {
            let client_id = client.client_id();
            queried_clients.insert(client_id);

            match client.generate_order_status_reports(&check.command).await {
                Ok(reports) => {
                    all_reports.extend(
                        reports
                            .into_iter()
                            .map(|report| SourcedOrderStatusReport { client_id, report }),
                    );
                }
                Err(e) => {
                    failed_clients.insert(client_id);
                    log::warn!(
                        "Failed to query order reports from {}: {e}",
                        client.client_id()
                    );
                }
            }
        }

        let result = self.reconcile_open_order_reports(
            &check,
            all_reports,
            &queried_clients,
            &failed_clients,
            clients,
        );
        let mut events = result.events;

        if !result.targeted_queries.is_empty() {
            let query_delay =
                Duration::from_millis(u64::from(self.config.single_order_query_delay_ms));
            let query_results =
                request_targeted_order_reports(result.targeted_queries, clients, query_delay).await;
            events.extend(self.reconcile_targeted_order_reports(query_results, clients));
        }

        events
    }

    /// Prepares a bulk open-order report request and snapshots cached open orders.
    pub(crate) fn prepare_open_order_report_check(
        &mut self,
        command_id: UUID4,
        clients: &[&dyn ExecutionClient],
    ) -> OpenOrderReportCheck {
        let filtered_orders = self.filtered_open_orders_for_reconciliation();
        let active_order_ids: IndexSet<ClientOrderId> =
            filtered_orders.iter().map(Order::client_order_id).collect();
        self.order_coverage_warnings
            .retain(|client_order_id| active_order_ids.contains(client_order_id));
        self.order_lookback_warnings
            .retain(|client_order_id| active_order_ids.contains(client_order_id));
        self.order_coverage_unresolved
            .retain(|client_order_id| active_order_ids.contains(client_order_id));

        let mut client_coverage = IndexMap::new();

        for order in &filtered_orders {
            let client_order_id = order.client_order_id();
            let coverage = self.resolve_order_report_client_coverage(order, clients);

            match &coverage {
                ReportClientCoverage::Resolved(_) => {
                    if self
                        .order_coverage_unresolved
                        .shift_remove(&client_order_id)
                    {
                        self.order_coverage_warnings.shift_remove(&client_order_id);
                    }
                }
                ReportClientCoverage::Unavailable(_) | ReportClientCoverage::Unresolved => {
                    self.order_coverage_unresolved.insert(client_order_id);
                }
            }

            client_coverage.insert(client_order_id, coverage);
        }

        log::debug!(
            "Found {} order{} open in cache",
            filtered_orders.len(),
            if filtered_orders.len() == 1 { "" } else { "s" }
        );

        let ts_now = self.clock.borrow().timestamp_ns();
        let start = self
            .config
            .open_check_lookback_mins
            .map(DurationNanos::from_mins)
            .map(|lookback| ts_now.saturating_sub(lookback));

        let mut command = GenerateOrderStatusReports::new(
            command_id,
            ts_now,
            self.config.open_check_open_only,
            None,
            start,
            None,
            None,
            None,
        );
        command.log_receipt_level = LogLevel::Debug;

        OpenOrderReportCheck {
            command,
            filtered_orders,
            client_coverage,
        }
    }

    fn resolve_order_report_client_coverage(
        &self,
        order: &OrderAny,
        clients: &[&dyn ExecutionClient],
    ) -> ReportClientCoverage {
        if let Some(client_id) = self.cache.borrow().client_id(&order.client_order_id()) {
            return ReportClientCoverage::Resolved(IndexSet::from([*client_id]));
        }

        if let Some(account_id) = order.account_id() {
            let account_clients = clients
                .iter()
                .filter(|client| client.account_id() == account_id)
                .map(|client| client.client_id())
                .collect::<IndexSet<_>>();

            if !account_clients.is_empty() {
                return ReportClientCoverage::Resolved(account_clients);
            }
        }

        let venue_clients = clients
            .iter()
            .filter(|client| client.handles_order_venue(order.instrument_id().venue))
            .map(|client| client.client_id())
            .collect::<IndexSet<_>>();

        if venue_clients.is_empty() {
            ReportClientCoverage::Unresolved
        } else {
            ReportClientCoverage::Resolved(venue_clients)
        }
    }

    /// Builds per-order venue queries for fallback open-order reconciliation.
    pub fn check_open_order_queries(&mut self) -> Vec<TradingCommand> {
        self.check_open_order_queries_for_clients(None)
    }

    /// Builds throttled open-order queries, optionally restricted to selected clients.
    pub(crate) fn check_open_order_queries_for_clients(
        &mut self,
        client_ids: Option<&IndexSet<ClientId>>,
    ) -> Vec<TradingCommand> {
        let now = dst::time::Instant::now();
        let query_delay = Duration::from_millis(u64::from(self.config.single_order_query_delay_ms));
        let query_limit = self.config.max_single_order_queries_per_cycle as usize;

        if query_limit == 0 {
            return Vec::new();
        }

        let mut filtered_orders = self.filtered_open_orders_for_reconciliation();
        filtered_orders.sort_by_key(|order| {
            let client_order_id = order.client_order_id();
            (
                self.order_query_recency.last_marked(&client_order_id),
                client_order_id,
            )
        });

        let mut queries = Vec::new();

        for order in filtered_orders {
            if queries.len() >= query_limit {
                break;
            }

            let client_order_id = order.client_order_id();
            let client_id = self.cache.borrow().client_id(&client_order_id).copied();

            if let Some(client_ids) = client_ids
                && !client_id.is_some_and(|client_id| client_ids.contains(&client_id))
            {
                continue;
            }

            let threshold = Duration::from(self.config.open_check_threshold_ns);
            if let Some(elapsed) = self.order_activity.elapsed_at(&client_order_id, now)
                && elapsed < threshold
            {
                let elapsed_ms = elapsed.as_millis();
                let threshold_ms = threshold.as_millis();
                log::debug!(
                    "Deferring open order query for {client_order_id}: recent local activity \
                     ({elapsed_ms}ms < threshold={threshold_ms}ms)",
                );
                continue;
            }

            if self
                .order_query_recency
                .within_at(&client_order_id, now, query_delay)
            {
                continue;
            }

            self.order_query_recency.mark(client_order_id);
            let ts_now = self.clock.borrow().timestamp_ns();

            let cmd = TradingCommand::QueryOrder(QueryOrder::new(
                order.trader_id(),
                client_id,
                order.strategy_id(),
                order.instrument_id(),
                client_order_id,
                order.venue_order_id(),
                UUID4::new(),
                ts_now,
                None,
                None,
            ));
            queries.push(cmd);
        }

        queries
    }

    /// Reconciles bulk open-order report responses against a cached order snapshot.
    pub(crate) fn reconcile_open_order_reports(
        &mut self,
        check: &OpenOrderReportCheck,
        mut all_reports: Vec<SourcedOrderStatusReport>,
        queried_clients: &IndexSet<ClientId>,
        failed_clients: &IndexSet<ClientId>,
        clients: &[&dyn ExecutionClient],
    ) -> OpenOrderReconciliationResult {
        all_reports.retain(|sourced| !self.should_skip_order_report(&sourced.report));
        let mut venue_reported_ids = IndexSet::new();

        for sourced in &all_reports {
            let report = &sourced.report;
            if let Some(client_order_id) = &report.client_order_id {
                venue_reported_ids.insert(*client_order_id);
                self.order_coverage_warnings.shift_remove(client_order_id);
                self.order_lookback_warnings.shift_remove(client_order_id);
                // A positive report is proof the venue still knows the order:
                // reset the missing-order ladder so only consecutive misses
                // accumulate (mirrors the Python engine's per-report clear).
                self.order_recon_retries.shift_remove(client_order_id);
            } else {
                let mapped_client_order_id = self
                    .cache
                    .borrow()
                    .client_order_id(&report.venue_order_id)
                    .copied();

                // The mapped order was positively reported: it must receive
                // the full positive-report bookkeeping or the missing-order
                // loop below immediately re-increments the cleared counter.
                if let Some(client_order_id) = mapped_client_order_id {
                    venue_reported_ids.insert(client_order_id);
                    self.order_coverage_warnings.shift_remove(&client_order_id);
                    self.order_lookback_warnings.shift_remove(&client_order_id);
                    self.order_recon_retries.shift_remove(&client_order_id);
                }
            }
        }

        let mut events = Vec::new();
        let mut targeted_candidates = Vec::new();

        for sourced in all_reports {
            let report = sourced.report;

            let order = match report.client_order_id {
                Some(client_order_id) => self.get_order(client_order_id),
                None => self.get_order_by_venue_order_id(report.venue_order_id),
            };

            let Some(order) = order else {
                continue;
            };

            let client_order_id = order.client_order_id();

            // Check for recent local activity to avoid race conditions with in-flight fills
            let threshold = Duration::from(self.config.open_check_threshold_ns);
            if let Some(elapsed) = self.order_activity.elapsed(&client_order_id)
                && elapsed < threshold
            {
                let elapsed_ms = elapsed.as_millis();
                let threshold_ms = threshold.as_millis();
                log::debug!(
                    "Deferring reconciliation for {client_order_id}: recent local activity ({elapsed_ms}ms < threshold={threshold_ms}ms)",
                );
                continue;
            }

            let instrument = self.get_instrument(&report.instrument_id);

            if terminal_report_has_missing_fills(&report, order.filled_qty()) {
                targeted_candidates.push((
                    order,
                    IndexSet::from([sourced.client_id]),
                    Some(report),
                ));
                continue;
            }

            let commission_client = clients
                .iter()
                .find(|client| client.client_id() == sourced.client_id)
                .copied();

            match self.reconcile_order_report(
                &order,
                &report,
                instrument.as_ref(),
                commission_client,
            ) {
                Ok(order_events) => events.extend(order_events),
                Err(e) => log::error!(
                    "Deferring reconciliation for {client_order_id}: venue commission calculation failed: {e}"
                ),
            }
        }

        // Handle orders missing at venue (skip in open_only mode where the
        // venue response may omit recently closed orders). When a lookback
        // window is set, only consider orders within that window so older
        // GTC orders outside the query range are not falsely marked missing.
        if self.config.open_check_open_only {
            let cached_ids: IndexSet<ClientOrderId> = check
                .filtered_orders
                .iter()
                .map(Order::client_order_id)
                .collect();
            let missing_at_venue: IndexSet<ClientOrderId> = cached_ids
                .difference(&venue_reported_ids)
                .copied()
                .collect();

            if !missing_at_venue.is_empty() {
                log::debug!(
                    "{} cached open order{} not present in venue current response",
                    missing_at_venue.len(),
                    if missing_at_venue.len() == 1 {
                        " is"
                    } else {
                        "s are"
                    },
                );

                for client_order_id in missing_at_venue {
                    log::debug!("Cached open order missing from venue response: {client_order_id}");
                }
            }
        } else {
            let candidates: Vec<&OrderAny> = if let Some(cutoff) = check.command.start {
                let mut candidates = Vec::new();

                for order in &check.filtered_orders {
                    let client_order_id = order.client_order_id();
                    if order.ts_last() >= cutoff {
                        self.order_lookback_warnings.shift_remove(&client_order_id);
                        candidates.push(order);
                    } else if !venue_reported_ids.contains(&client_order_id)
                        && self.order_lookback_warnings.insert(client_order_id)
                    {
                        log::warn!(
                            "Skipping missing-order reconciliation for {client_order_id}: its last update predates the configured open-check lookback window; absence from the bulk response cannot be treated as evidence and no targeted query will be issued from it"
                        );
                    }
                }

                candidates
            } else {
                check.filtered_orders.iter().collect()
            };

            for order in candidates {
                let client_order_id = order.client_order_id();
                if venue_reported_ids.contains(&client_order_id) {
                    continue;
                }

                let coverage = check
                    .client_coverage
                    .get(&client_order_id)
                    .unwrap_or(&ReportClientCoverage::Unresolved);

                let ReportClientCoverage::Resolved(responsible_clients) = coverage else {
                    if self.order_coverage_warnings.insert(client_order_id) {
                        log::warn!(
                            "Skipping order reconciliation for {client_order_id}: responsible execution client coverage is unresolved"
                        );
                    }

                    continue;
                };

                if responsible_clients.is_empty() {
                    if self.order_coverage_warnings.insert(client_order_id) {
                        log::warn!(
                            "Skipping order reconciliation for {client_order_id}: responsible execution client coverage is unresolved"
                        );
                    }

                    continue;
                }

                let missing_clients = responsible_clients
                    .difference(queried_clients)
                    .copied()
                    .collect::<IndexSet<_>>();

                if !missing_clients.is_empty() {
                    if self.order_coverage_warnings.insert(client_order_id) {
                        log::warn!(
                            "Skipping order reconciliation for {client_order_id}: responsible execution clients were not queried: {missing_clients:?}"
                        );
                    }

                    continue;
                }

                let failed_responsible_clients = responsible_clients
                    .intersection(failed_clients)
                    .copied()
                    .collect::<IndexSet<_>>();

                if !failed_responsible_clients.is_empty() {
                    log::warn!(
                        "Skipping order reconciliation for {client_order_id}: failed to query responsible execution clients: {failed_responsible_clients:?}"
                    );
                    continue;
                }

                self.order_coverage_warnings.shift_remove(&client_order_id);
                if let Some(order) = self.prepare_missing_order_query(client_order_id) {
                    targeted_candidates.push((order, responsible_clients.clone(), None));
                }
            }
        }

        targeted_candidates.sort_by_key(|(order, _, _)| {
            let client_order_id = order.client_order_id();
            (
                self.order_query_recency.last_marked(&client_order_id),
                client_order_id,
            )
        });

        let query_limit = self.config.max_single_order_queries_per_cycle as usize;
        let mut planned_queries = 0usize;
        let mut cap_deferred_orders = 0usize;
        let mut targeted_queries = Vec::new();

        for (order, responsible_clients, report) in targeted_candidates {
            let client_order_id = order.client_order_id();

            let required_queries = responsible_clients.len();
            let exceeds_query_limit = planned_queries + required_queries > query_limit;
            let can_run_oversized_group = planned_queries == 0 && query_limit > 0;
            if required_queries == 0 || (exceeds_query_limit && !can_run_oversized_group) {
                cap_deferred_orders += 1;
                continue;
            }

            if required_queries > query_limit {
                log::warn!(
                    "Targeted order query for {client_order_id} requires {required_queries} responsible clients, exceeding the per-cycle limit {query_limit} to avoid indefinite deferral"
                );
            }

            planned_queries += required_queries;
            self.order_query_recency.mark(client_order_id);
            self.order_query_pending.insert(client_order_id);
            let command_id = UUID4::new();
            let ts_now = self.clock.borrow().timestamp_ns();

            let command = GenerateOrderStatusReport::new(
                command_id,
                ts_now,
                Some(order.instrument_id()),
                Some(client_order_id),
                order.venue_order_id(),
                None,
                None,
            );
            targeted_queries.push(TargetedOrderQuery {
                client_order_id,
                responsible_clients,
                report,
                filled_qty: order.filled_qty(),
                command,
            });
        }

        if cap_deferred_orders > 0 {
            log::warn!(
                "Reached max single-order queries ({query_limit}) this cycle, deferring {cap_deferred_orders} order(s)"
            );
        }

        OpenOrderReconciliationResult {
            events,
            targeted_queries,
        }
    }

    /// Reconciles targeted query results, resolving missing orders only with complete coverage.
    pub(crate) fn reconcile_targeted_order_reports(
        &mut self,
        results: Vec<TargetedOrderReportResult>,
        clients: &[&dyn ExecutionClient],
    ) -> Vec<OrderEventAny> {
        let mut events = Vec::new();
        let mut fill_queue = ReconciliationFillQueue::default();

        for result in results {
            let client_order_id = result.client_order_id;
            self.remove_targeted_order_queries(&[client_order_id]);

            if let Some(report) = result.report {
                self.order_recon_retries.shift_remove(&client_order_id);
                self.order_coverage_warnings.shift_remove(&client_order_id);

                let Some(order) = self.get_order(client_order_id) else {
                    continue;
                };

                let instrument = self.get_instrument(&report.instrument_id);

                let commission_client = result.client_id.and_then(|client_id| {
                    clients
                        .iter()
                        .find(|client| client.client_id() == client_id)
                        .copied()
                });

                log::info!(
                    color = LogColor::Blue as u8;
                    "Found {client_order_id} via targeted order status query: {}",
                    report.order_status,
                );

                let fills = result.fills.iter().collect::<Vec<_>>();
                events.extend(self.reconcile_order_with_fills(
                    false,
                    &order,
                    &report,
                    &fills,
                    instrument.as_ref(),
                    &mut fill_queue,
                    commission_client,
                ));
                continue;
            }

            if result.coverage_complete {
                events.extend(self.resolve_missing_order(client_order_id));
            } else {
                log::warn!(
                    "Deferring missing-order resolution for {client_order_id}: targeted order status coverage was incomplete"
                );
            }
        }

        events
    }

    /// Collects position reports and returns synthetic discrepancy events.
    ///
    /// Registers each client's tolerance before evaluating its reports. The caller applies the
    /// returned events; the live node separately queries authoritative fills before synthetic fallback.
    pub async fn check_positions_consistency(
        &mut self,
        clients: &[&dyn ExecutionClient],
    ) -> Vec<OrderEventAny> {
        let check = self.prepare_position_report_check(UUID4::new(), clients);
        let mut reports = Vec::new();
        let mut queried_clients = IndexSet::new();
        let mut failed_clients = IndexSet::new();

        for client in clients {
            let client_id = client.client_id();
            queried_clients.insert(client_id);
            self.set_position_reconciliation_tolerance(
                client.account_id(),
                client.position_reconciliation_tolerance(),
            );

            match client
                .generate_position_status_reports(&check.command)
                .await
            {
                Ok(client_reports) => {
                    reports.extend(client_reports);
                }
                Err(e) => {
                    failed_clients.insert(client_id);
                    log::warn!(
                        "Failed to query position reports from {}: {e}",
                        client.client_id()
                    );
                }
            }
        }

        let active_keys = self
            .open_position_keys_for_reconciliation()
            .into_iter()
            .chain(reports.iter().filter_map(|report| {
                (self.should_reconcile_instrument(&report.instrument_id)
                    && report.signed_decimal_qty != Decimal::ZERO)
                    .then_some((report.instrument_id, report.account_id))
            }))
            .collect();

        let events =
            self.reconcile_position_reports(&check, reports, &queried_clients, &failed_clients);

        // Global pruning requires unfiltered reports; flat reports must not preserve stale retries
        self.retain_position_reconciliation(&active_keys);

        events
    }

    /// Prepares a bulk position report request and records client coverage.
    ///
    /// Snapshots all activity revisions, including keys without open cached positions, so venue-only
    /// positions can be checked against activity that predates the request.
    #[must_use]
    pub fn prepare_position_report_check(
        &self,
        command_id: UUID4,
        clients: &[&dyn ExecutionClient],
    ) -> PositionReportCheck {
        let position_keys = self.open_position_keys_for_reconciliation();

        let client_coverage = position_keys
            .iter()
            .map(|key| (*key, resolve_position_report_client_coverage(*key, clients)))
            .collect();

        let mut activity_revisions = self.position_activity_revisions.clone();
        for key in &position_keys {
            activity_revisions
                .entry(*key)
                .or_insert_with(|| self.position_activity_revision(key));
        }

        log::debug!(
            "Found {} unique instrument/account combination{} with open positions",
            position_keys.len(),
            if position_keys.len() == 1 { "" } else { "s" }
        );

        let ts_now = self.clock.borrow().timestamp_ns();

        let mut command = GeneratePositionStatusReports::new(
            command_id, ts_now, None, // instrument_id - query all
            None, // start
            None, // end
            None, // params
            None, // correlation_id
        );
        command.log_receipt_level = LogLevel::Debug;

        PositionReportCheck {
            command,
            client_coverage,
            activity_revisions,
        }
    }

    /// Plans fill queries for settled position discrepancies with complete client coverage.
    ///
    /// Requires an unfiltered check and report snapshot for pruning. Coverage keys and nonflat
    /// venue reports retain retry state.
    pub fn plan_position_fill_reports(
        &mut self,
        check: &mut PositionReportCheck,
        reports: &[PositionStatusReport],
        queried_clients: &IndexSet<ClientId>,
        failed_clients: &IndexSet<ClientId>,
        clients: &[&dyn ExecutionClient],
    ) -> PositionFillReportPlan {
        let mut venue_positions: IndexMap<InstrumentAccountKey, Vec<PositionStatusReport>> =
            IndexMap::new();

        for report in reports {
            if self.should_reconcile_instrument(&report.instrument_id) {
                venue_positions
                    .entry((report.instrument_id, report.account_id))
                    .or_default()
                    .push(report.clone());
            }
        }

        let keys = check
            .client_coverage
            .keys()
            .copied()
            .chain(venue_positions.iter().filter_map(|(key, reports)| {
                reports
                    .iter()
                    .any(|report| report.signed_decimal_qty != Decimal::ZERO)
                    .then_some(*key)
            }))
            .collect::<IndexSet<_>>();

        let active_keys = keys.clone();
        let query_end = self.timestamp_ns();
        let lookback = DurationNanos::from_mins(self.config.position_check_lookback_mins);
        let query_start = query_end.saturating_sub(lookback);
        let mut discrepancy_keys = IndexSet::new();
        let mut queries = Vec::new();

        for key in keys {
            let coverage = check
                .client_coverage
                .entry(key)
                .or_insert_with(|| resolve_position_report_client_coverage(key, clients));
            let prepared_revision = *check.activity_revisions.entry(key).or_default();
            let venue_reports = venue_positions
                .get(&key)
                .map(Vec::as_slice)
                .unwrap_or_default();
            let comparison = self.position_quantity_comparison(key, venue_reports);
            let tolerance = self.position_reconciliation_tolerance(key.1);

            if comparison.quantities_match(tolerance) {
                self.clear_position_reconciliation(&key);
                continue;
            }

            discrepancy_keys.insert(key);

            if self.position_activity_revision(&key) > prepared_revision
                || self.position_activity_is_recent(&key)
            {
                continue;
            }

            let report_shape = comparison.report_shape();
            let retries = self.position_reconciliation_retries(&key, report_shape);
            if retries >= self.config().position_check_retries {
                continue;
            }

            let ReportClientCoverage::Resolved(responsible_clients) = coverage else {
                log::warn!(
                    "Skipping fill report query for {}/{}: responsible execution client coverage is unavailable",
                    key.0,
                    key.1,
                );
                continue;
            };

            if responsible_clients.is_empty()
                || !responsible_clients.is_subset(queried_clients)
                || !responsible_clients.is_disjoint(failed_clients)
            {
                log::warn!(
                    "Skipping fill report query for {}/{}: responsible position report coverage is incomplete",
                    key.0,
                    key.1,
                );
                continue;
            }

            for client_id in responsible_clients.iter() {
                let mut command = GenerateFillReports::new(
                    UUID4::new(),
                    query_end,
                    Some(key.0),
                    None,
                    Some(query_start),
                    Some(query_end),
                    None,
                    Some(check.command.command_id),
                );
                command.log_receipt_level = LogLevel::Debug;
                queries.push(PositionFillReportQuery {
                    key,
                    client_id: *client_id,
                    command,
                });
            }
        }

        self.retain_position_reconciliation(&active_keys);

        PositionFillReportPlan {
            queries,
            discrepancy_keys,
        }
    }

    /// Checks whether position activity is unchanged since the check was prepared.
    #[must_use]
    pub fn position_report_check_is_current(
        &self,
        check: &PositionReportCheck,
        key: &InstrumentAccountKey,
    ) -> bool {
        check
            .activity_revisions
            .get(key)
            .is_some_and(|revision| self.position_activity_revision(key) == *revision)
    }

    /// Validates fill attribution and supplies a cached position ID when unambiguous.
    ///
    /// # Errors
    ///
    /// Returns an error if cached order or position state conflicts with the fill,
    /// or inferred-fill history cannot be evaluated.
    pub fn prepare_position_fill_report(
        &self,
        report: &mut FillReport,
        venue_reports: &[PositionStatusReport],
    ) -> anyhow::Result<PositionFillReportPreparation> {
        let cache = self.cache();
        let venue_client_order_id = cache.client_order_id(&report.venue_order_id).copied();
        if let (Some(report_client_order_id), Some(venue_client_order_id)) =
            (report.client_order_id, venue_client_order_id)
        {
            anyhow::ensure!(
                report_client_order_id == venue_client_order_id,
                "fill {} client order ID {report_client_order_id} conflicts with venue order mapping {venue_client_order_id}",
                report.trade_id,
            );
        }

        let client_order_id = report.client_order_id.or(venue_client_order_id);
        let order = client_order_id.and_then(|id| cache.order(&id));
        if let Some(order) = &order {
            anyhow::ensure!(
                order.instrument_id() == report.instrument_id
                    && order.order_side() == report.order_side
                    && order
                        .account_id()
                        .is_none_or(|account_id| account_id == report.account_id)
                    && order
                        .venue_order_id()
                        .is_none_or(|venue_order_id| venue_order_id == report.venue_order_id),
                "fill {} conflicts with cached order {}",
                report.trade_id,
                order.client_order_id(),
            );
        }

        let hedge_context = report.venue_position_id.is_some()
            || venue_reports
                .iter()
                .any(|venue_report| venue_report.venue_position_id.is_some());
        let mapped_position_id = client_order_id
            .and_then(|client_order_id| cache.position_id(&client_order_id))
            .copied();

        if hedge_context
            && let (Some(venue_position_id), Some(mapped_position_id)) =
                (report.venue_position_id, mapped_position_id)
        {
            anyhow::ensure!(
                venue_position_id == mapped_position_id,
                "fill {} position ID {venue_position_id} conflicts with cached order position {mapped_position_id}",
                report.trade_id,
            );
        }

        if let Some(order) = order
            && has_active_inferred_fill(&order)?
        {
            return Ok(PositionFillReportPreparation::InferredOverlap);
        }

        if !hedge_context {
            return Ok(PositionFillReportPreparation::Ready);
        }

        if report.venue_position_id.is_some() {
            return Ok(PositionFillReportPreparation::Ready);
        }

        let Some(position_id) = mapped_position_id else {
            return Ok(PositionFillReportPreparation::Unattributed);
        };

        let position = cache.position(&position_id).ok_or_else(|| {
            anyhow::anyhow!(
                "fill {} maps to position {position_id}, which is not cached",
                report.trade_id,
            )
        })?;

        anyhow::ensure!(
            position.account_id == report.account_id
                && position.instrument_id == report.instrument_id,
            "fill {} maps to position {position_id} with a different account or instrument",
            report.trade_id,
        );
        anyhow::ensure!(
            position.is_open(),
            "fill {} maps to non-open position {position_id}",
            report.trade_id,
        );
        anyhow::ensure!(
            !position.is_opposite_side(report.order_side) || report.last_qty <= position.quantity,
            "fill {} without a venue position ID would cross position {position_id}",
            report.trade_id,
        );

        report.venue_position_id = Some(position_id);
        Ok(PositionFillReportPreparation::Ready)
    }

    /// Checks whether cached position fills match the report, including quantity and commission.
    #[must_use]
    pub fn position_contains_fill_report(&self, report: &FillReport) -> bool {
        let cache = self.cache();
        let client_order_id = report
            .client_order_id
            .or_else(|| cache.client_order_id(&report.venue_order_id).copied());
        let positions = cache.positions(
            None,
            Some(&report.instrument_id),
            None,
            Some(&report.account_id),
            None,
        );
        let mut matched = false;
        let mut quantity = Quantity::zero(report.last_qty.precision);
        let mut commission = Money::zero(report.commission.currency);

        for position in positions {
            if report
                .venue_position_id
                .is_some_and(|position_id| position.id != position_id)
            {
                continue;
            }

            for replay_event in &position.replay_events {
                let PositionReplayEvent::Filled(fill) = replay_event else {
                    continue;
                };

                if fill.account_id != report.account_id
                    || fill.instrument_id != report.instrument_id
                    || fill.venue_order_id != report.venue_order_id
                    || fill.trade_id != report.trade_id
                    || fill.order_side != report.order_side
                    || fill.last_px != report.last_px
                    || fill.liquidity_side != report.liquidity_side
                    || client_order_id.is_some_and(|id| fill.client_order_id != id)
                    || report
                        .venue_position_id
                        .is_some_and(|id| fill.position_id != Some(id))
                {
                    continue;
                }

                let Some(fill_commission) = fill.commission else {
                    return false;
                };

                if fill_commission.currency != report.commission.currency {
                    return false;
                }

                let Some(next_quantity) = quantity.checked_add(fill.last_qty) else {
                    return false;
                };

                let Some(next_commission) = commission.checked_add(fill_commission) else {
                    return false;
                };

                matched = true;
                quantity = next_quantity;
                commission = next_commission;
            }
        }

        matched && quantity == report.last_qty && commission == report.commission
    }

    /// Reconciles cached positions against venue position reports.
    ///
    /// Callers may supply a filtered check and reports without pruning retry state for other positions.
    /// Global pruning is handled by [`Self::plan_position_fill_reports`] and
    /// [`Self::check_positions_consistency`].
    #[must_use]
    pub fn reconcile_position_reports(
        &mut self,
        check: &PositionReportCheck,
        reports: Vec<PositionStatusReport>,
        queried_clients: &IndexSet<ClientId>,
        failed_clients: &IndexSet<ClientId>,
    ) -> Vec<OrderEventAny> {
        log::debug!("Checking position consistency between cached-state and venues");

        let mut venue_positions: IndexMap<InstrumentAccountKey, Vec<PositionStatusReport>> =
            IndexMap::new();

        for report in reports {
            if !self.should_reconcile_instrument(&report.instrument_id) {
                continue;
            }

            venue_positions
                .entry((report.instrument_id, report.account_id))
                .or_default()
                .push(report);
        }

        let mut events = Vec::new();

        for key in check.client_coverage.keys() {
            let prepared_revision = check
                .activity_revisions
                .get(key)
                .copied()
                .unwrap_or_default();

            if self.position_activity_revision(key) > prepared_revision {
                log::debug!(
                    "Deferring position reconciliation for {}/{}: local activity recorded during report request",
                    key.0,
                    key.1,
                );
                continue;
            }

            let venue_reports = venue_positions
                .get(key)
                .map(Vec::as_slice)
                .unwrap_or_default();

            if venue_reports.is_empty() {
                match check.client_coverage.get(key) {
                    Some(ReportClientCoverage::Resolved(responsible_clients))
                        if !responsible_clients.is_empty()
                            && responsible_clients.is_subset(queried_clients)
                            && responsible_clients.is_disjoint(failed_clients) => {}
                    Some(ReportClientCoverage::Resolved(responsible_clients))
                        if responsible_clients.is_empty() =>
                    {
                        log::warn!(
                            "Skipping position reconciliation for {}/{}: responsible execution client coverage is unresolved",
                            key.0,
                            key.1,
                        );
                        continue;
                    }
                    Some(ReportClientCoverage::Resolved(responsible_clients))
                        if !responsible_clients.is_subset(queried_clients) =>
                    {
                        log::warn!(
                            "Skipping position reconciliation for {}/{}: responsible execution clients were not all queried",
                            key.0,
                            key.1,
                        );
                        continue;
                    }
                    Some(ReportClientCoverage::Resolved(responsible_clients)) => {
                        let failed_responsible_clients = responsible_clients
                            .intersection(failed_clients)
                            .copied()
                            .collect::<IndexSet<_>>();
                        log::warn!(
                            "Skipping position reconciliation for {}/{}: failed to query responsible execution clients: {failed_responsible_clients:?}",
                            key.0,
                            key.1,
                        );
                        continue;
                    }
                    Some(ReportClientCoverage::Unavailable(responsible_clients)) => {
                        log::debug!(
                            "Skipping position reconciliation for {}/{}: complete bulk position coverage is unavailable from responsible execution clients: {responsible_clients:?}",
                            key.0,
                            key.1,
                        );
                        continue;
                    }
                    Some(ReportClientCoverage::Unresolved) | None => {
                        log::warn!(
                            "Skipping position reconciliation for {}/{}: responsible execution client coverage is unresolved",
                            key.0,
                            key.1,
                        );
                        continue;
                    }
                }
            }

            if let Some(discrepancy_events) = self.check_position_discrepancy(*key, venue_reports) {
                events.extend(discrepancy_events);
            }
        }

        let current_position_keys = self.open_position_keys_for_reconciliation();

        for (key, venue_reports) in &venue_positions {
            if check.client_coverage.contains_key(key)
                || venue_reports
                    .iter()
                    .all(|report| report.signed_decimal_qty == Decimal::ZERO)
            {
                continue;
            }

            if current_position_keys.contains(key) {
                log::debug!(
                    "Deferring position reconciliation for {}/{}: position opened after client coverage was recorded",
                    key.0,
                    key.1,
                );
                continue;
            }

            if let Some(discrepancy_events) = self.check_position_discrepancy(*key, venue_reports) {
                events.extend(discrepancy_events);
            }
        }

        events
    }

    /// Returns any external order claim for the given instrument ID.
    #[must_use]
    pub fn get_external_order_claim(&self, instrument_id: &InstrumentId) -> Option<StrategyId> {
        self.cache.borrow().external_order_claim(instrument_id)
    }

    /// Claims external orders for a specific strategy and instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the instrument already has a registered claim.
    pub fn claim_external_orders(
        &mut self,
        instrument_id: InstrumentId,
        strategy_id: StrategyId,
    ) -> anyhow::Result<()> {
        self.cache
            .borrow_mut()
            .register_external_order_claims(strategy_id, &[instrument_id])
    }

    /// Observes a local order event and updates tracking state.
    ///
    /// This is the `LiveNode` dispatch path for order events: acknowledgement
    /// events clear reconciliation tracking, fills record position
    /// activity, and every event stamps local activity. The stamp must come
    /// AFTER any [`Self::clear_recon_tracking`] call - that call drops the
    /// local-activity mark, which is the sole grace gate protecting a
    /// just-acknowledged order from missing-order reconciliation while the
    /// venue report lags.
    pub fn observe_order_event(&mut self, event: &OrderEventAny) {
        match event {
            OrderEventAny::Filled(fill) => {
                self.record_position_activity(fill.instrument_id, fill.account_id);
            }
            OrderEventAny::Accepted(_)
            | OrderEventAny::Rejected(_)
            | OrderEventAny::Canceled(_)
            | OrderEventAny::Expired(_)
            | OrderEventAny::Denied(_)
            | OrderEventAny::Updated(_)
            | OrderEventAny::ModifyRejected(_)
            | OrderEventAny::CancelRejected(_) => {
                self.clear_recon_tracking(&event.client_order_id(), true);
            }
            _ => {}
        }

        self.record_local_activity(event.client_order_id());
    }

    /// Observes an incoming execution report and updates tracking state.
    ///
    /// This should be called **before** the report is dispatched to the execution
    /// engine, so that the manager's state is current when periodic checks run.
    ///
    /// Updates performed per report variant:
    /// - `Order`: updates reconciliation tracking based on order status
    /// - `Fill`: records order activity and advances the position revision once, without marking
    ///   the fill as processed; continuous fill recovery checks this increment after dispatch
    /// - `OrderWithFills`: updates order tracking and records position activity per fill
    /// - `Position`: records position activity
    /// - `MassStatus`: no-op (handled separately via startup reconciliation)
    pub fn observe_execution_report(&mut self, report: &ExecutionReport) {
        match report {
            ExecutionReport::Order(order_report) => {
                self.observe_order_status_report(order_report);
            }
            ExecutionReport::Fill(fill_report) => {
                let client_order_id = fill_report.client_order_id.or_else(|| {
                    self.cache
                        .borrow()
                        .client_order_id(&fill_report.venue_order_id)
                        .copied()
                });

                if let Some(coid) = client_order_id {
                    self.record_local_activity(coid);
                }

                self.record_position_activity(fill_report.instrument_id, fill_report.account_id);
            }
            ExecutionReport::OrderWithFills(order_report, fills) => {
                self.observe_order_status_report(order_report);

                for fill_report in fills {
                    self.record_position_activity(
                        fill_report.instrument_id,
                        fill_report.account_id,
                    );
                }
            }
            ExecutionReport::Position(position_report) => {
                self.record_position_activity(
                    position_report.instrument_id,
                    position_report.account_id,
                );
            }
            ExecutionReport::MassStatus(_) => {
                // Handled separately via reconcile_execution_mass_status
            }
        }
    }

    fn observe_order_status_report(&mut self, report: &OrderStatusReport) {
        let Some(client_order_id) = report.client_order_id else {
            return;
        };

        let accepted_during_pending_command = report.order_status == OrderStatus::Accepted
            && self.get_order(client_order_id).is_some_and(|order| {
                matches!(
                    order.status(),
                    OrderStatus::PendingUpdate | OrderStatus::PendingCancel
                )
            });

        if !matches!(
            report.order_status,
            OrderStatus::PendingUpdate | OrderStatus::PendingCancel
        ) && !accepted_during_pending_command
        {
            self.clear_recon_tracking(&client_order_id, report.order_status.is_closed());
        }

        // Dispatch may suppress a terminal report, such as a stale cancel for the
        // old leg of a cancel-replace. Keep the settling grace until the node
        // confirms the cached order closed after dispatch.
        self.record_local_activity(client_order_id);
    }

    /// Purges closed orders from the cache that are older than the configured buffer.
    pub fn purge_closed_orders(&mut self) {
        let Some(buffer_mins) = self.config.purge_closed_orders_buffer_mins else {
            return;
        };

        let ts_now = self.timestamp_ns();
        let buffer_secs = mins_to_secs(u64::from(buffer_mins));

        self.cache
            .borrow_mut()
            .purge_closed_orders(ts_now, buffer_secs);
    }

    /// Purges closed positions from the cache that are older than the configured buffer.
    pub fn purge_closed_positions(&mut self) {
        let Some(buffer_mins) = self.config.purge_closed_positions_buffer_mins else {
            return;
        };

        let ts_now = self.clock.borrow().timestamp_ns();
        let buffer_secs = mins_to_secs(u64::from(buffer_mins));

        self.cache
            .borrow_mut()
            .purge_closed_positions(ts_now, buffer_secs);
    }

    /// Purges old account events from the cache based on the configured lookback.
    pub fn purge_account_events(&mut self) {
        let Some(lookback_mins) = self.config.purge_account_events_lookback_mins else {
            return;
        };

        let ts_now = self.clock.borrow().timestamp_ns();
        let lookback_secs = mins_to_secs(u64::from(lookback_mins));

        self.cache
            .borrow_mut()
            .purge_account_events(ts_now, lookback_secs);
    }

    fn get_order(&self, client_order_id: ClientOrderId) -> Option<OrderAny> {
        self.cache().order(&client_order_id).map(|o| o.clone())
    }

    fn get_order_by_venue_order_id(&self, venue_order_id: VenueOrderId) -> Option<OrderAny> {
        let cache = self.cache();
        cache
            .client_order_id(&venue_order_id)
            .and_then(|client_order_id| cache.order(client_order_id).map(|o| o.clone()))
    }

    fn get_instrument(&self, instrument_id: &InstrumentId) -> Option<InstrumentAny> {
        self.cache().instrument(instrument_id).cloned()
    }

    fn should_skip_order_report(&self, report: &OrderStatusReport) -> bool {
        let client_order_id = report.client_order_id.or_else(|| {
            self.cache
                .borrow()
                .client_order_id(&report.venue_order_id)
                .copied()
        });

        if let Some(client_order_id) = client_order_id
            && self
                .config
                .filtered_client_order_ids
                .contains(&client_order_id)
        {
            log::debug!(
                "Skipping order report {client_order_id}: in filtered_client_order_ids list"
            );
            return true;
        }

        if !self.should_reconcile_instrument(&report.instrument_id) {
            log::debug!(
                "Skipping order report for {}: not in reconciliation_instrument_ids",
                report.instrument_id
            );
            return true;
        }

        false
    }

    /// Checks whether the instrument passes the configured reconciliation filter.
    pub(crate) fn should_reconcile_instrument(&self, instrument_id: &InstrumentId) -> bool {
        self.config.reconciliation_instrument_ids.is_empty()
            || self
                .config
                .reconciliation_instrument_ids
                .contains(instrument_id)
    }

    fn prepare_missing_order_query(&mut self, client_order_id: ClientOrderId) -> Option<OrderAny> {
        let order = self.get_order(client_order_id)?;

        // The order may have closed while the report request was in flight;
        // the check must come before the retry increment or the stale empty
        // response recreates tracking state that nothing prunes afterwards.
        if order.status().is_closed() {
            log::debug!(
                "Skipping missing-order resolution for {client_order_id}: already {}",
                order.status()
            );
            self.clear_recon_tracking(&client_order_id, true);
            return None;
        }

        // Recent local activity is the real-time settling window for missing
        // orders. Venue/domain timestamps can be ahead of the trading clock and
        // must not stall reconciliation.
        if self.order_activity.within(
            &client_order_id,
            Duration::from(self.config.open_check_threshold_ns),
        ) {
            return None;
        }

        let retries = self.order_recon_retries.entry(client_order_id).or_insert(0);
        *retries = retries.saturating_add(1);

        if *retries < self.config.open_check_missing_retries {
            log::debug!(
                "Order {} not found at venue, retry {}/{}",
                client_order_id,
                retries,
                self.config.open_check_missing_retries
            );
            return None;
        }

        Some(order)
    }

    fn resolve_missing_order(&mut self, client_order_id: ClientOrderId) -> Vec<OrderEventAny> {
        let mut events = Vec::new();

        let Some(order) = self.get_order(client_order_id) else {
            return events;
        };

        if order.status().is_closed() {
            log::debug!(
                "Skipping missing-order resolution for {client_order_id}: already {}",
                order.status()
            );
            self.clear_recon_tracking(&client_order_id, true);
            return events;
        }

        if self.order_activity.within(
            &client_order_id,
            Duration::from(self.config.open_check_threshold_ns),
        ) {
            log::debug!(
                "Deferring missing-order resolution for {client_order_id}: recent local activity"
            );
            return events;
        }

        let retries = self
            .order_recon_retries
            .get(&client_order_id)
            .copied()
            .unwrap_or_default();
        let ts_now = self.clock.borrow().timestamp_ns();

        match order.status() {
            OrderStatus::Accepted | OrderStatus::Submitted => {
                log::warn!(
                    "Order {client_order_id} not found at venue after {retries} retries and a targeted query, marking as REJECTED"
                );

                if let Some(rejected) =
                    create_reconciliation_rejected(&order, Some("NOT_FOUND_AT_VENUE"), ts_now)
                {
                    events.push(rejected);
                }
            }
            OrderStatus::PartiallyFilled => {
                log::warn!(
                    "Order {client_order_id} not found at venue after {retries} retries and a targeted query, marking as CANCELED"
                );
                events.push(OrderEventAny::Canceled(OrderCanceled::new(
                    order.trader_id(),
                    order.strategy_id(),
                    order.instrument_id(),
                    client_order_id,
                    UUID4::new(),
                    ts_now,
                    ts_now,
                    true,
                    order.venue_order_id(),
                    order.account_id(),
                    None,
                )));
            }
            OrderStatus::PendingUpdate | OrderStatus::PendingCancel => {
                log::debug!(
                    "Deferring resolution for {client_order_id}: still inflight as {}",
                    order.status()
                );
                // Narrow tracking reset mirroring the Python engine:
                // zero the retry ladder and stamp the query time so the
                // inflight checker first observes a full threshold delay
                // and then retries from scratch. The order must stay
                // registered in `order_inflight_checks` - the inflight checker
                // walks that map, unlike Python which rescans cached
                // inflight orders every cycle - and keeps its
                // local-activity mark.
                self.order_recon_retries.shift_remove(&client_order_id);
                if let Some(check) = self.order_inflight_checks.get_mut(&client_order_id) {
                    check.retry_count = 0;
                    check.last_query_at = Some(dst::time::Instant::now());
                }

                self.order_query_recency.mark(client_order_id);
                return events;
            }
            status => {
                log::warn!(
                    "Skipping missing-order resolution for {client_order_id}: unexpected status {status}"
                );
            }
        }

        self.clear_recon_tracking(&client_order_id, true);
        events
    }

    /// Collects cached and venue net, long, and short quantities for comparison.
    pub(crate) fn position_quantity_comparison(
        &self,
        key: InstrumentAccountKey,
        venue_reports: &[PositionStatusReport],
    ) -> PositionQuantityComparison {
        let (instrument_id, account_id) = key;

        let cached_positions = {
            let cache = self.cache.borrow();
            cache
                .positions_open(None, Some(&instrument_id), None, Some(&account_id), None)
                .into_iter()
                .map(|position| (*position).clone())
                .collect::<Vec<_>>()
        };

        let (cached_signed_qty, cached_long_qty, cached_short_qty) =
            position_qty_aggregates(cached_positions.iter().map(Position::signed_decimal_qty));
        let (venue_signed_qty, venue_long_qty, venue_short_qty) =
            position_qty_aggregates(venue_reports.iter().map(|report| report.signed_decimal_qty));
        let nonflat_count = venue_reports
            .iter()
            .filter(|report| report.signed_decimal_qty != Decimal::ZERO)
            .count();
        let venue_report = venue_reports
            .iter()
            .find(|report| report.signed_decimal_qty != Decimal::ZERO)
            .or_else(|| venue_reports.last())
            .cloned();
        let venue_has_side_reports = venue_reports.iter().any(PositionStatusReport::is_long)
            && venue_reports.iter().any(PositionStatusReport::is_short);

        PositionQuantityComparison {
            cached_positions,
            cached_signed_qty,
            cached_long_qty,
            cached_short_qty,
            venue_signed_qty,
            venue_long_qty,
            venue_short_qty,
            nonflat_count,
            venue_report,
            venue_has_side_reports,
        }
    }

    fn check_position_discrepancy(
        &mut self,
        key: InstrumentAccountKey,
        venue_reports: &[PositionStatusReport],
    ) -> Option<Vec<OrderEventAny>> {
        let (instrument_id, account_id) = key;
        let comparison = self.position_quantity_comparison(key, venue_reports);
        let tolerance = self.position_reconciliation_tolerance(account_id);
        let quantities_match = comparison.quantities_match(tolerance);
        let report_shape = comparison.report_shape();
        let PositionQuantityComparison {
            cached_positions,
            cached_signed_qty,
            cached_long_qty,
            cached_short_qty,
            venue_signed_qty,
            venue_long_qty,
            venue_short_qty,
            venue_report,
            ..
        } = comparison;

        if quantities_match {
            self.clear_position_reconciliation(&key);
            return None;
        }

        if !self.config.generate_missing_orders {
            log::debug!(
                "Discrepancy for {instrument_id} position when `generate_missing_orders` disabled, skipping"
            );
            return None;
        }

        let ts_now = self.clock.borrow().timestamp_ns();

        // Grace window measured on the monotonic `dst::time` clock; see `record_position_activity`
        if self.position_activity_is_recent(&key) {
            log::debug!(
                "Skipping position reconciliation for {instrument_id}: recent activity within threshold"
            );
            return None;
        }

        let retries = self.position_reconciliation_retries(&key, report_shape);

        if retries >= self.config.position_check_retries {
            return None;
        }

        if report_shape == PositionReportShape::MultiLeg {
            let new_retries = retries + 1;
            self.set_position_reconciliation_retries(key, report_shape, new_retries);
            log::warn!(
                "Deferring position reconciliation for {instrument_id}/{account_id}: venue reports have ambiguous side aggregates (cached net={cached_signed_qty}, long={cached_long_qty}, short={cached_short_qty}; venue net={venue_signed_qty}, long={venue_long_qty}, short={venue_short_qty})"
            );

            if new_retries >= self.config.position_check_retries {
                log::error!(
                    "Position discrepancy for {instrument_id}/{account_id} unresolved after {} attempts; no further reconciliation attempts will be made for the current report shape",
                    self.config.position_check_retries,
                );
            }

            return None;
        }

        log::warn!(
            "Position discrepancy detected for {instrument_id}: cached_signed_qty={cached_signed_qty}, venue_signed_qty={venue_signed_qty}"
        );

        let Some(instrument) = self.cache.borrow().instrument(&instrument_id).cloned() else {
            log::debug!("Cannot reconcile position for {instrument_id}: instrument not in cache");
            let new_retries = retries + 1;
            self.set_position_reconciliation_retries(key, report_shape, new_retries);
            if new_retries >= self.config.position_check_retries {
                log::error!(
                    "Position discrepancy for {instrument_id} unresolved after {} attempts \
                     (cached_qty={cached_signed_qty}, venue_qty={venue_signed_qty}); \
                     no further reconciliation attempts will be made for the current report shape",
                    self.config.position_check_retries,
                );
            }

            return None;
        };

        let cached_avg_px = position_avg_px(&cached_positions);
        let venue_avg_px = venue_report.as_ref().and_then(|r| r.avg_px_open);

        let crosses_zero = (cached_signed_qty > Decimal::ZERO && venue_signed_qty < Decimal::ZERO)
            || (cached_signed_qty < Decimal::ZERO && venue_signed_qty > Decimal::ZERO);

        let result = if crosses_zero {
            let venue_ts_last = venue_report.as_ref().map_or(ts_now, |r| r.ts_last);
            let venue_position_id = venue_report
                .as_ref()
                .and_then(|report| report.venue_position_id);

            let position_ids = match venue_position_id {
                Some(open_position_id) => match cached_positions.as_slice() {
                    [position] => Some((Some(position.id), Some(open_position_id))),
                    _ => {
                        log::warn!(
                            "Deferring hedge cross-zero reconciliation for {instrument_id}/{account_id}: cached and venue position identities are ambiguous"
                        );
                        None
                    }
                },
                None => Some((None, None)),
            };

            position_ids.and_then(|(close_position_id, open_position_id)| {
                self.reconcile_cross_zero_position(
                    &instrument,
                    account_id,
                    instrument_id,
                    cached_signed_qty,
                    cached_avg_px,
                    venue_signed_qty,
                    venue_avg_px,
                    close_position_id,
                    open_position_id,
                    ts_now,
                    venue_ts_last,
                )
            })
        } else {
            let qty_diff = venue_signed_qty - cached_signed_qty;

            let order_side = if qty_diff > Decimal::ZERO {
                OrderSide::Buy
            } else {
                OrderSide::Sell
            };

            let reconciliation_px = calculate_reconciliation_price(
                cached_signed_qty,
                cached_avg_px,
                venue_signed_qty,
                venue_avg_px,
            );

            match reconciliation_px.or(venue_avg_px).or(cached_avg_px) {
                Some(fill_px) => {
                    let fill_qty = qty_diff.abs();
                    let venue_position_id = venue_report
                        .as_ref()
                        .and_then(|report| report.venue_position_id);
                    let venue_ts_last = venue_report.as_ref().map_or(ts_now, |r| r.ts_last);
                    Quantity::from_decimal_dp(fill_qty, instrument.size_precision())
                        .ok()
                        .map(|order_qty| {
                            let fill_price =
                                Price::from_decimal_dp(fill_px, instrument.price_precision()).ok();
                            let venue_order_id = create_position_reconciliation_venue_order_id(
                                account_id,
                                instrument_id,
                                order_side,
                                OrderType::Market,
                                order_qty,
                                fill_price,
                                venue_position_id,
                                None,
                                venue_ts_last,
                            );

                            let mut order_report = OrderStatusReport::new(
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
                            .with_avg_px(fill_px);

                            if let Some(venue_position_id) = venue_position_id {
                                order_report =
                                    order_report.with_venue_position_id(venue_position_id);
                            }

                            order_report
                        })
                        .map(|order_report| {
                            log::info!(
                                color = LogColor::Blue as u8;
                                "Generating synthetic fill for position reconciliation {instrument_id}: side={order_side:?}, qty={}, px={fill_px}", qty_diff.abs(),
                            );

                            let (events, _) = self.handle_external_order(
                                &order_report,
                                account_id,
                                &instrument,
                                &[],
                                true,
                                None,
                                None,
                            );
                            events
                        })
                }
                None => None,
            }
        };

        // Track retries when reconciliation didn't produce events
        if result.is_none() || result.as_ref().is_some_and(Vec::is_empty) {
            let new_retries = retries + 1;
            self.set_position_reconciliation_retries(key, report_shape, new_retries);
            if new_retries >= self.config.position_check_retries {
                log::error!(
                    "Position discrepancy for {} unresolved after {} attempts \
                     (cached_qty={}, venue_qty={}); \
                     no further reconciliation attempts will be made for the current report shape",
                    instrument_id,
                    self.config.position_check_retries,
                    cached_signed_qty,
                    venue_signed_qty,
                );
            }
        } else {
            self.clear_position_reconciliation(&key);
        }

        result
    }

    /// Handles position reconciliation when position flips sign, splitting into two
    /// fills: close existing position then open new position in opposite direction.
    #[expect(clippy::too_many_arguments)]
    fn reconcile_cross_zero_position(
        &self,
        instrument: &InstrumentAny,
        account_id: AccountId,
        instrument_id: InstrumentId,
        cached_signed_qty: Decimal,
        cached_avg_px: Option<Decimal>,
        venue_signed_qty: Decimal,
        venue_avg_px: Option<Decimal>,
        close_position_id: Option<PositionId>,
        open_position_id: Option<PositionId>,
        ts_now: UnixNanos,
        venue_ts_last: UnixNanos,
    ) -> Option<Vec<OrderEventAny>> {
        log::info!(
            color = LogColor::Blue as u8;
            "Position crosses zero for {instrument_id}: cached={cached_signed_qty}, venue={venue_signed_qty}. Splitting into two fills",
        );

        let close_qty = cached_signed_qty.abs();

        let close_side = if cached_signed_qty < Decimal::ZERO {
            OrderSide::Buy // Close short by buying
        } else {
            OrderSide::Sell // Close long by selling
        };

        let open_qty = venue_signed_qty.abs();

        let open_side = if venue_signed_qty > Decimal::ZERO {
            OrderSide::Buy // Open long
        } else {
            OrderSide::Sell // Open short
        };

        let Some(close_px) = cached_avg_px else {
            log::warn!("Cannot close position for {instrument_id}: no cached average price");
            return None;
        };

        let open_report = match venue_avg_px {
            Some(open_px) => Some((
                create_cross_zero_leg_report(
                    instrument,
                    account_id,
                    instrument_id,
                    open_side,
                    open_qty,
                    open_px,
                    open_position_id,
                    "OPEN",
                    ts_now,
                    venue_ts_last,
                )?,
                open_px,
            )),
            None => None,
        };

        let close_report = create_cross_zero_leg_report(
            instrument,
            account_id,
            instrument_id,
            close_side,
            close_qty,
            close_px,
            close_position_id,
            "CLOSE",
            ts_now,
            venue_ts_last,
        )?;

        log::info!(
            color = LogColor::Blue as u8;
            "Generating close fill for cross-zero {instrument_id}: side={close_side:?}, qty={close_qty}, px={close_px}",
        );

        let (close_events, _) = self.handle_external_order(
            &close_report,
            account_id,
            instrument,
            &[],
            true,
            None,
            None,
        );
        let mut all_events = close_events;

        if let Some((open_report, open_px)) = open_report {
            log::info!(
                color = LogColor::Blue as u8;
                "Generating open fill for cross-zero {instrument_id}: side={open_side:?}, qty={open_qty}, px={open_px}",
            );

            let (open_events, _) = self.handle_external_order(
                &open_report,
                account_id,
                instrument,
                &[],
                true,
                None,
                None,
            );
            all_events.extend(open_events);
        } else {
            log::warn!("Cannot open new position for {instrument_id}: no venue average price");
        }

        Some(all_events)
    }

    /// Creates a position from a venue position report when no orders/fills exist.
    ///
    /// This handles the case where the venue reports an open position but there are
    /// no order or fill reports to create it from (e.g., orders are already closed).
    fn create_position_from_report(
        &self,
        report: &PositionStatusReport,
        account_id: AccountId,
        instrument: &InstrumentAny,
    ) -> Option<Vec<OrderEventAny>> {
        let instrument_id = report.instrument_id;
        let venue_signed_qty = report.signed_decimal_qty;

        if venue_signed_qty == Decimal::ZERO {
            return None;
        }

        let order_side = if venue_signed_qty > Decimal::ZERO {
            OrderSide::Buy
        } else {
            OrderSide::Sell
        };

        let qty_abs = venue_signed_qty.abs();
        let venue_avg_px = report.avg_px_open?;

        let ts_now = self.clock.borrow().timestamp_ns();
        let order_qty = Quantity::from_decimal_dp(qty_abs, instrument.size_precision()).ok()?;
        let fill_price = Price::from_decimal_dp(venue_avg_px, instrument.price_precision()).ok();
        let venue_order_id = create_position_reconciliation_venue_order_id(
            account_id,
            instrument_id,
            order_side,
            OrderType::Market,
            order_qty,
            fill_price,
            report.venue_position_id,
            None,
            report.ts_last,
        );

        let mut order_report = OrderStatusReport::new(
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
        .with_avg_px(venue_avg_px);

        // Preserve venue_position_id for hedging mode
        if let Some(venue_position_id) = report.venue_position_id {
            order_report = order_report.with_venue_position_id(venue_position_id);
        }

        log::info!(
            color = LogColor::Blue as u8;
            "Creating position from venue report for {instrument_id}: side={order_side:?}, qty={qty_abs}, avg_px={venue_avg_px}",
        );

        let (events, _) = self.handle_external_order(
            &order_report,
            account_id,
            instrument,
            &[],
            true,
            None,
            None,
        );
        Some(events)
    }

    fn reconcile_position_report(
        &self,
        report: &PositionStatusReport,
        account_id: AccountId,
        instruments_with_unattributed_fills: &IndexSet<InstrumentId>,
    ) -> Option<Vec<OrderEventAny>> {
        if report.venue_position_id.is_some() {
            self.reconcile_position_report_hedging(
                report,
                account_id,
                instruments_with_unattributed_fills,
            )
        } else {
            self.reconcile_position_report_netting(report, account_id)
        }
    }

    fn reconcile_position_report_hedging(
        &self,
        report: &PositionStatusReport,
        account_id: AccountId,
        instruments_with_unattributed_fills: &IndexSet<InstrumentId>,
    ) -> Option<Vec<OrderEventAny>> {
        let venue_position_id = report.venue_position_id?;

        // Skip if fills exist for this instrument but lack venue_position_id
        // (can't determine which hedge position they belong to)
        if instruments_with_unattributed_fills.contains(&report.instrument_id) {
            log::debug!(
                "Skipping hedge position {venue_position_id} reconciliation: unattributed fills in batch"
            );
            return None;
        }

        log::debug!(
            "Reconciling HEDGE position for {}, venue_position_id={}",
            report.instrument_id,
            venue_position_id
        );

        let position = {
            let cache = self.cache.borrow();
            cache.position_owned(&venue_position_id)
        };

        match position {
            Some(position) => {
                let cached_signed_qty = position.signed_decimal_qty();
                let venue_signed_qty = report.signed_decimal_qty;

                if cached_signed_qty == venue_signed_qty {
                    log::debug!(
                        "Hedge position {venue_position_id} matches venue: qty={cached_signed_qty}"
                    );
                    return None;
                }

                if venue_signed_qty == Decimal::ZERO && cached_signed_qty == Decimal::ZERO {
                    return None;
                }

                if !self.config.generate_missing_orders {
                    log::error!(
                        "Cannot reconcile {} {}: position net qty {} != reported net qty {} \
                         and `generate_missing_orders` is disabled",
                        report.instrument_id,
                        venue_position_id,
                        cached_signed_qty,
                        venue_signed_qty
                    );
                    return None;
                }

                self.reconcile_hedge_position_discrepancy(
                    report,
                    account_id,
                    &position,
                    cached_signed_qty,
                )
            }
            None => {
                if report.signed_decimal_qty == Decimal::ZERO {
                    return None;
                }

                if !self.config.generate_missing_orders {
                    log::error!(
                        "Cannot reconcile position: {venue_position_id} not found and `generate_missing_orders` is disabled"
                    );
                    return None;
                }

                self.reconcile_missing_hedge_position(report, account_id)
            }
        }
    }

    fn reconcile_hedge_position_discrepancy(
        &self,
        report: &PositionStatusReport,
        account_id: AccountId,
        position: &Position,
        cached_signed_qty: Decimal,
    ) -> Option<Vec<OrderEventAny>> {
        let instrument = self.get_instrument(&report.instrument_id)?;
        let venue_signed_qty = report.signed_decimal_qty;

        let diff = (cached_signed_qty - venue_signed_qty).abs();
        let diff_qty = Quantity::from_decimal_dp(diff, instrument.size_precision()).ok()?;

        if diff_qty.is_zero() {
            log::debug!(
                "Difference quantity rounds to zero for {}, skipping",
                instrument.id()
            );
            return None;
        }

        let venue_position_id = report.venue_position_id?;
        log::warn!(
            "Hedge position discrepancy for {} {}: cached={}, venue={}, generating reconciliation order",
            report.instrument_id,
            venue_position_id,
            cached_signed_qty,
            venue_signed_qty
        );

        let current_avg_px = if position.avg_px_open > 0.0 {
            Decimal::from_str(&position.avg_px_open.to_string()).ok()
        } else {
            None
        };

        self.create_position_reconciliation_order(
            report,
            account_id,
            &instrument,
            cached_signed_qty,
            diff_qty,
            current_avg_px,
        )
    }

    fn reconcile_missing_hedge_position(
        &self,
        report: &PositionStatusReport,
        account_id: AccountId,
    ) -> Option<Vec<OrderEventAny>> {
        let instrument = self.get_instrument(&report.instrument_id)?;
        let venue_signed_qty = report.signed_decimal_qty;

        let qty = venue_signed_qty.abs();
        let diff_qty = Quantity::from_decimal_dp(qty, instrument.size_precision()).ok()?;

        if diff_qty.is_zero() {
            return None;
        }

        let venue_position_id = report.venue_position_id?;
        log::warn!(
            "Missing hedge position for {} {}: venue reports {}, generating reconciliation order",
            report.instrument_id,
            venue_position_id,
            venue_signed_qty
        );

        self.create_position_reconciliation_order(
            report,
            account_id,
            &instrument,
            Decimal::ZERO,
            diff_qty,
            None,
        )
    }

    fn reconcile_position_report_netting(
        &self,
        report: &PositionStatusReport,
        account_id: AccountId,
    ) -> Option<Vec<OrderEventAny>> {
        let instrument_id = report.instrument_id;

        log::debug!("Reconciling NET position for {instrument_id}");

        let instrument = self.get_instrument(&instrument_id)?;

        let (cached_signed_qty, cached_avg_px) = {
            let cache = self.cache.borrow();
            let positions =
                cache.positions_open(None, Some(&instrument_id), None, Some(&account_id), None);

            if positions.is_empty() {
                (Decimal::ZERO, None)
            } else {
                let mut total_signed_qty = Decimal::ZERO;
                let mut total_value = Decimal::ZERO;
                let mut total_qty = Decimal::ZERO;

                for pos in positions {
                    total_signed_qty += pos.signed_decimal_qty();
                    let qty = pos.signed_decimal_qty().abs();
                    if pos.avg_px_open > 0.0
                        && qty > Decimal::ZERO
                        && let Ok(avg_px) = Decimal::from_str(&pos.avg_px_open.to_string())
                    {
                        total_value += avg_px * qty;
                        total_qty += qty;
                    }
                }

                let avg_px = if total_qty > Decimal::ZERO {
                    Some(total_value / total_qty)
                } else {
                    None
                };

                (total_signed_qty, avg_px)
            }
        };

        let venue_signed_qty = report.signed_decimal_qty;

        log::debug!("venue_signed_qty={venue_signed_qty}, cached_signed_qty={cached_signed_qty}");

        let tolerance = self.position_reconciliation_tolerance(account_id);
        if (cached_signed_qty - venue_signed_qty).abs() <= tolerance {
            log::debug!("Position quantities match for {instrument_id}, no reconciliation needed");
            return None;
        }

        if !self.config.generate_missing_orders {
            log::debug!(
                "Discrepancy for {instrument_id} position when `generate_missing_orders` disabled, skipping"
            );
            return None;
        }

        let diff = (cached_signed_qty - venue_signed_qty).abs();
        let diff_qty = Quantity::from_decimal_dp(diff, instrument.size_precision()).ok()?;

        if diff_qty.is_zero() {
            log::debug!(
                "Difference quantity rounds to zero for {instrument_id}, skipping order generation"
            );
            return None;
        }

        let crosses_zero = cached_signed_qty != Decimal::ZERO
            && venue_signed_qty != Decimal::ZERO
            && ((cached_signed_qty > Decimal::ZERO && venue_signed_qty < Decimal::ZERO)
                || (cached_signed_qty < Decimal::ZERO && venue_signed_qty > Decimal::ZERO));

        if crosses_zero {
            let ts_now = self.clock.borrow().timestamp_ns();
            return self.reconcile_cross_zero_position(
                &instrument,
                account_id,
                instrument_id,
                cached_signed_qty,
                cached_avg_px,
                venue_signed_qty,
                report.avg_px_open,
                None,
                None,
                ts_now,
                report.ts_last,
            );
        }

        if cached_signed_qty == Decimal::ZERO {
            return self.create_position_from_report(report, account_id, &instrument);
        }

        self.create_position_reconciliation_order(
            report,
            account_id,
            &instrument,
            cached_signed_qty,
            diff_qty,
            cached_avg_px,
        )
    }

    fn create_position_reconciliation_order(
        &self,
        report: &PositionStatusReport,
        account_id: AccountId,
        instrument: &InstrumentAny,
        cached_signed_qty: Decimal,
        diff_qty: Quantity,
        current_avg_px: Option<Decimal>,
    ) -> Option<Vec<OrderEventAny>> {
        let venue_signed_qty = report.signed_decimal_qty;
        let instrument_id = report.instrument_id;

        let order_side = if venue_signed_qty > cached_signed_qty {
            OrderSide::Buy
        } else {
            OrderSide::Sell
        };

        let reconciliation_px = calculate_reconciliation_price(
            cached_signed_qty,
            current_avg_px,
            venue_signed_qty,
            report.avg_px_open,
        );

        let fill_px = reconciliation_px
            .or(report.avg_px_open)
            .or(current_avg_px)?;

        let ts_now = self.clock.borrow().timestamp_ns();
        let fill_price = Price::from_decimal_dp(fill_px, instrument.price_precision()).ok();
        let venue_order_id = create_position_reconciliation_venue_order_id(
            account_id,
            instrument_id,
            order_side,
            OrderType::Market,
            diff_qty,
            fill_price,
            report.venue_position_id,
            None,
            report.ts_last,
        );

        let mut order_report = OrderStatusReport::new(
            account_id,
            instrument_id,
            None,
            venue_order_id,
            order_side.into(),
            OrderType::Market,
            TimeInForce::Gtc,
            OrderStatus::Filled,
            diff_qty,
            diff_qty,
            ts_now,
            ts_now,
            ts_now,
            None,
        )
        .with_avg_px(fill_px);

        if let Some(venue_position_id) = report.venue_position_id {
            order_report = order_report.with_venue_position_id(venue_position_id);
        }

        log::info!(
            color = LogColor::Blue as u8;
            "Generating reconciliation order for {instrument_id}: side={order_side:?}, qty={diff_qty}, px={fill_px}",
        );

        let (events, _) = self.handle_external_order(
            &order_report,
            account_id,
            instrument,
            &[],
            true,
            None,
            None,
        );
        Some(events)
    }

    fn reconcile_order_report(
        &self,
        order: &OrderAny,
        report: &OrderStatusReport,
        instrument: Option<&InstrumentAny>,
        commission_client: Option<&dyn ExecutionClient>,
    ) -> anyhow::Result<Vec<OrderEventAny>> {
        let has_missing_fills = terminal_report_has_missing_fills(report, order.filled_qty());
        anyhow::ensure!(
            !has_missing_fills,
            "terminal report for {} has unaccounted fills; waiting for fill reports",
            order.client_order_id(),
        );
        let ts_now = self.clock.borrow().timestamp_ns();

        let commission = if matches!(
            report.order_status,
            OrderStatus::PartiallyFilled | OrderStatus::Filled
        ) && report.filled_qty > order.filled_qty()
            && let Some(instrument) = instrument
        {
            let fill_qty = report.filled_qty - order.filled_qty();
            let price_and_liquidity =
                incremental_inferred_fill_price_and_liquidity(order, report, instrument);

            resolve_inferred_fill_commission(
                fill_qty,
                price_and_liquidity,
                instrument,
                commission_client,
            )?
        } else {
            None
        };

        if matches!(
            report.order_status,
            OrderStatus::Canceled | OrderStatus::Expired
        ) && order.status() == OrderStatus::Filled
            && report.filled_qty == order.filled_qty()
        {
            return Ok(Vec::new());
        }

        Ok(
            reconcile_order_report_with_commission(order, report, instrument, ts_now, commission)
                .into_iter()
                .collect(),
        )
    }

    /// Reconciles an order with its associated fills atomically.
    #[expect(
        clippy::too_many_arguments,
        reason = "Snapshot and continuous reports share fill projection"
    )]
    fn reconcile_order_with_fills(
        &mut self,
        is_snapshot: bool,
        order: &OrderAny,
        report: &OrderStatusReport,
        fills: &[&FillReport],
        instrument: Option<&InstrumentAny>,
        fill_queue: &mut ReconciliationFillQueue,
        commission_client: Option<&dyn ExecutionClient>,
    ) -> Vec<OrderEventAny> {
        let mut events = Vec::new();
        let mut working = order.clone();
        let mut sorted_fills: Vec<&FillReport> = fills.to_vec();
        sorted_fills.sort_by_key(|f| f.ts_event);

        let ts_now = self.clock.borrow().timestamp_ns();

        if matches!(
            report.order_status,
            OrderStatus::Canceled | OrderStatus::Expired
        ) && report.ts_triggered.is_some()
            && working.status() != OrderStatus::Triggered
            && TRIGGERABLE_ORDER_TYPES.contains(&working.order_type())
        {
            let triggered = create_reconciliation_triggered(&working, report, ts_now);
            if working.apply(triggered.clone()).is_ok() {
                events.push(triggered);
            }
        }

        let requires_snapshot_projection = !sorted_fills.is_empty()
            || is_snapshot
                && (report.order_status == OrderStatus::Voided
                    || report.filled_qty < working.filled_qty());
        if !requires_snapshot_projection {
            match self.reconcile_order_report(&working, report, instrument, commission_client) {
                Ok(order_events) => events.extend(order_events),
                Err(e) => log::error!(
                    "Deferring order reconciliation for {}: {e}",
                    order.client_order_id(),
                ),
            }

            return events;
        }

        for event in generate_reconciliation_order_pre_fill_events(&working, report, ts_now) {
            if let Err(e) = working.apply(event.clone()) {
                log::warn!(
                    "Cannot project reconciliation event for {}: {e}",
                    order.client_order_id()
                );
                return events;
            }

            events.push(event);
        }

        if let Some(inst) = instrument {
            for fill in sorted_fills {
                let Some((event, fill_key)) =
                    self.create_order_fill(&working, fill, inst, &fill_queue.pending_fill_keys)
                else {
                    continue;
                };

                if let Err(e) = working.apply(OrderEventAny::Filled(event.clone())) {
                    if self.is_fill_applied(&event, fill_key) {
                        self.fills_processed.mark(fill_key);
                        continue;
                    }

                    log::warn!(
                        "Cannot project reconciliation fill for {}: {e}",
                        order.client_order_id()
                    );
                    return events;
                }

                fill_queue.push(&mut events, event, fill_key);
            }
        }

        // Continuous reports can precede streamed fills; only snapshots can reverse fills
        if !is_snapshot {
            match self.reconcile_order_report(&working, report, instrument, commission_client) {
                Ok(order_events) => events.extend(order_events),
                Err(e) => log::warn!("Deferring order reconciliation: {e}"),
            }

            return events;
        }

        if terminal_report_has_missing_fills(report, working.filled_qty()) {
            log::warn!(
                "Deferring terminal reconciliation for {}: fill reports are incomplete",
                order.client_order_id(),
            );
            return events;
        }

        let commission = if report.filled_qty > working.filled_qty()
            && let Some(instrument) = instrument
        {
            let fill_qty = report.filled_qty - working.filled_qty();

            let price_and_liquidity =
                incremental_inferred_fill_price_and_liquidity(&working, report, instrument);

            match resolve_inferred_fill_commission(
                fill_qty,
                price_and_liquidity,
                instrument,
                commission_client,
            ) {
                Ok(commission) => commission,
                Err(e) => {
                    log::error!(
                        "Deferring inferred fill for {}: venue commission calculation failed: {e}",
                        order.client_order_id(),
                    );
                    return events;
                }
            }
        } else {
            None
        };

        for event in generate_reconciliation_order_snapshot_events_with_commission(
            &working, report, instrument, ts_now, commission,
        ) {
            if let Err(e) = working.apply(event.clone()) {
                log::warn!(
                    "Cannot project reconciliation snapshot event for {}: {e}",
                    order.client_order_id()
                );
                break;
            }

            events.push(event);
        }

        events
    }

    #[expect(clippy::too_many_arguments)]
    fn handle_external_order(
        &self,
        report: &OrderStatusReport,
        account_id: AccountId,
        instrument: &InstrumentAny,
        fills: &[&FillReport],
        is_synthetic: bool,
        fill_queue: Option<&mut ReconciliationFillQueue>,
        commission_client: Option<&dyn ExecutionClient>,
    ) -> (Vec<OrderEventAny>, Option<ExternalOrderMetadata>) {
        let claimed_strategy = self
            .cache
            .borrow()
            .external_order_claim(&report.instrument_id);

        let (strategy_id, tags) = if let Some(claimed_strategy) = claimed_strategy {
            let order_id = report
                .client_order_id
                .map_or_else(|| report.venue_order_id.to_string(), |id| id.to_string());
            log::info!(
                color = LogColor::Blue as u8;
                "External order {} for {} claimed by strategy {}",
                order_id,
                report.instrument_id,
                claimed_strategy,
            );
            (claimed_strategy, None)
        } else {
            // Unclaimed orders use EXTERNAL strategy ID with tag distinguishing source
            let tag = if is_synthetic {
                *TAG_RECONCILIATION
            } else {
                *TAG_VENUE
            };

            (StrategyId::from("EXTERNAL"), Some(vec![tag]))
        };

        // Filter unclaimed venue orders (but not synthetic reconciliation orders)
        if self.config.filter_unclaimed_external && claimed_strategy.is_none() && !is_synthetic {
            return (Vec::new(), None);
        }

        let client_order_id = report
            .client_order_id
            .unwrap_or_else(|| ClientOrderId::from(report.venue_order_id.as_str()));

        if !report.quantity.is_positive() {
            log::error!(
                "Skipping external order {} ({}) for {}: non-positive quantity in report {:?}",
                client_order_id,
                report.venue_order_id,
                report.instrument_id,
                report,
            );
            return (Vec::new(), None);
        }

        let ts_now = self.clock.borrow().timestamp_ns();

        let Some(order_side) = report.order_side else {
            log::error!(
                "Skipping external order {} ({}) for {}: order side is not specified",
                client_order_id,
                report.venue_order_id,
                report.instrument_id,
            );
            return (Vec::new(), None);
        };

        let initialized = match OrderInitialized::new_checked(
            self.config.trader_id,
            strategy_id,
            report.instrument_id,
            client_order_id,
            order_side,
            report.order_type,
            report.quantity,
            report.time_in_force,
            report.post_only,
            report.reduce_only,
            false, // quote_quantity
            true,  // reconciliation
            UUID4::new(),
            ts_now,
            ts_now,
            report.price,
            report.activation_price,
            report.trigger_price,
            report.trigger_type,
            report.limit_offset,
            report.trailing_offset,
            report.trailing_offset_type,
            report.expire_time,
            report.display_qty,
            None, // emulation_trigger
            None, // trigger_instrument_id
            report.contingency_type,
            report.order_list_id,
            report.linked_order_ids.clone(),
            report.parent_order_id,
            None, // exec_algorithm_id
            None, // exec_algorithm_params
            None, // exec_spawn_id
            tags,
        ) {
            Ok(initialized) => initialized,
            Err(e) => {
                log::error!("Failed to create order from report: {e}");
                return (Vec::new(), None);
            }
        };

        let initialized = OrderEventAny::Initialized(initialized);

        let order = match OrderAny::from_events(vec![initialized.clone()]) {
            Ok(order) => order,
            Err(e) => {
                log::error!("Failed to create order from report: {e}");
                return (Vec::new(), None);
            }
        };

        let replace_inferred_fill = !fills.is_empty()
            && matches!(
                report.order_status,
                OrderStatus::Canceled
                    | OrderStatus::Expired
                    | OrderStatus::Filled
                    | OrderStatus::PartiallyFilled
            );
        let mut prepared_fills = Vec::new();
        let mut prepared_fill_keys = fill_queue
            .as_deref()
            .map(|queue| queue.pending_fill_keys.clone())
            .unwrap_or_default();
        let mut real_fill_total = Decimal::ZERO;

        if replace_inferred_fill {
            let mut sorted_fills: Vec<&FillReport> = fills.to_vec();
            sorted_fills.sort_by_key(|fill| fill.ts_event);

            if fill_queue.is_none() {
                log::error!(
                    "Cannot reconcile external order {client_order_id}: fill queue is unavailable"
                );
                return (Vec::new(), None);
            }

            for fill in sorted_fills {
                if let Some((fill_event, fill_key)) =
                    self.create_order_fill(&order, fill, instrument, &prepared_fill_keys)
                {
                    real_fill_total += fill.last_qty.as_decimal();
                    prepared_fill_keys.insert(fill_key);
                    prepared_fills.push((fill_event, fill_key));
                }
            }
        }

        let report_filled = report.filled_qty.as_decimal();

        let inferred_qty = if report_filled.is_zero() {
            None
        } else if replace_inferred_fill {
            if real_fill_total < report_filled {
                match Quantity::from_decimal_dp(
                    report_filled - real_fill_total,
                    instrument.size_precision(),
                ) {
                    Ok(quantity) => Some(quantity),
                    Err(e) => {
                        log::error!(
                            "Cannot reconcile external order {client_order_id}: residual fill quantity is invalid: {e}"
                        );
                        return (Vec::new(), None);
                    }
                }
            } else {
                None
            }
        } else if matches!(
            report.order_status,
            OrderStatus::PartiallyFilled
                | OrderStatus::Filled
                | OrderStatus::Canceled
                | OrderStatus::Expired
                | OrderStatus::Voided
        ) {
            Some(report.filled_qty)
        } else {
            None
        };

        let defer_terminal = !is_synthetic
            && claimed_strategy.is_some()
            && matches!(
                report.order_status,
                OrderStatus::Canceled | OrderStatus::Expired
            )
            && inferred_qty.is_some();

        if defer_terminal {
            log::warn!(
                "Deferring terminal reconciliation for claimed order {client_order_id}: fill reports are incomplete"
            );

            if prepared_fills.is_empty() {
                return (Vec::new(), None);
            }
        }

        let inferred_commission = if is_synthetic || defer_terminal {
            None
        } else if let Some(inferred_qty) = inferred_qty {
            let price_and_liquidity = inferred_fill_price_and_liquidity(&order, report, instrument);

            match resolve_inferred_fill_commission(
                inferred_qty,
                price_and_liquidity,
                instrument,
                commission_client,
            ) {
                Ok(commission) => commission,
                Err(e) => {
                    log::error!(
                        "Deferring external order {client_order_id}: venue commission calculation failed: {e}"
                    );
                    return (Vec::new(), None);
                }
            }
        } else {
            None
        };

        {
            let mut cache = self.cache.borrow_mut();

            let source_client_id = if is_synthetic {
                None
            } else {
                commission_client.map(ExecutionClient::client_id)
            };

            if let Err(e) = cache.add_order(order.clone(), None, source_client_id, false) {
                // Deterministic synthetic reconciliation IDs hash the same logical event
                // to the same client_order_id, so a restart replay can legitimately collide
                // with a cached order. Differentiate expected dedup from stuck state.
                match cache.order(&client_order_id) {
                    Some(existing) if is_synthetic && existing.is_closed() => {
                        log::debug!(
                            "Skipping synthetic reconciliation order {client_order_id} for {}: \
                             replay deduped (cached status={:?})",
                            report.instrument_id,
                            existing.status(),
                        );
                    }
                    Some(existing) if is_synthetic => {
                        log::warn!(
                            "Synthetic reconciliation order {client_order_id} for {} exists in \
                             cache in non-terminal state {:?}; fill not regenerated",
                            report.instrument_id,
                            existing.status(),
                        );
                    }
                    _ => {
                        log::error!("Failed to add external order to cache: {e}");
                    }
                }

                return (Vec::new(), None);
            }

            if let Err(e) = cache.index_venue_order_id(&client_order_id, &report.venue_order_id) {
                log::warn!("Failed to index venue order ID: {e}");
            }
        }

        Self::publish_order_event(&initialized);

        log::info!(
            color = LogColor::Blue as u8;
            "Created external order {} ({}) for {} [{}]",
            client_order_id,
            report.venue_order_id,
            report.instrument_id,
            report.order_status,
        );

        let ts_now = self.clock.borrow().timestamp_ns();
        let mut order_events = generate_external_order_status_events_with_commission(
            &order,
            report,
            &account_id,
            instrument,
            ts_now,
            inferred_commission,
        );

        if replace_inferred_fill {
            let terminal_event = if order_events.last().is_some_and(|event| {
                matches!(
                    event,
                    OrderEventAny::Canceled(_) | OrderEventAny::Expired(_),
                )
            }) {
                order_events.pop()
            } else {
                None
            };

            if order_events
                .last()
                .is_some_and(|event| matches!(event, OrderEventAny::Filled(_)))
            {
                order_events.pop();
            }

            let fill_queue =
                fill_queue.expect("fill queue availability was checked before cache mutation");
            for (fill_event, fill_key) in prepared_fills {
                fill_queue.push(&mut order_events, fill_event, fill_key);
            }

            if !defer_terminal
                && let Some(inferred_qty) = inferred_qty
                && let Some(inferred_fill) = create_inferred_fill_for_qty(
                    &order,
                    report,
                    &account_id,
                    instrument,
                    inferred_qty,
                    ts_now,
                    inferred_commission,
                )
            {
                order_events.push(inferred_fill);
            }

            if !defer_terminal && let Some(event) = terminal_event {
                order_events.push(event);
            }
        }

        let metadata = ExternalOrderMetadata {
            client_order_id,
            venue_order_id: report.venue_order_id,
            instrument_id: report.instrument_id,
            strategy_id,
            ts_init: ts_now,
        };

        (order_events, Some(metadata))
    }

    fn publish_order_event(event: &OrderEventAny) {
        let topic = switchboard::get_event_order_topic(event.strategy_id());
        msgbus::publish_order_event(topic, event);
    }

    /// Adjusts fills for instruments with incomplete first lifecycle (partial window).
    ///
    /// When historical fills don't fully explain the current position (e.g., lookback window
    /// started mid-position), this creates synthetic fills to align with the venue position.
    fn adjust_mass_status_fills(
        &self,
        mass_status: &ExecutionMassStatus,
    ) -> (
        IndexMap<VenueOrderId, OrderStatusReport>,
        IndexMap<VenueOrderId, Vec<FillReport>>,
    ) {
        let mut final_orders: IndexMap<VenueOrderId, OrderStatusReport> =
            mass_status.order_reports();
        let mut final_fills: IndexMap<VenueOrderId, Vec<FillReport>> = mass_status.fill_reports();

        final_fills.retain(|_, fills| {
            fills.retain(|fill| {
                if fill.last_qty.is_zero() {
                    log::warn!("Skipping zero-quantity fill report: {fill}");
                    return false;
                }

                true
            });

            !fills.is_empty()
        });

        if mass_status.lookback_start().is_some() {
            return (final_orders, final_fills);
        }

        let mut instruments_to_adjust = Vec::new();

        for (instrument_id, position_reports) in mass_status.position_reports() {
            if !self.should_reconcile_instrument(&instrument_id) {
                log::debug!(
                    "Skipping fill adjustment for {instrument_id}: not in reconciliation_instrument_ids"
                );
                continue;
            }

            // Skip hedge mode instruments (have venue_position_id) as partial-window
            // adjustment assumes a single net position per instrument
            let is_hedge_mode = position_reports
                .iter()
                .any(|r| r.venue_position_id.is_some());

            if is_hedge_mode {
                log::debug!(
                    "Skipping fill adjustment for {instrument_id}: hedge mode (has venue_position_id)"
                );
                continue;
            }

            let has_retained_position = {
                let cache = self.cache.borrow();
                !cache
                    .positions_open(
                        None,
                        Some(&instrument_id),
                        None,
                        Some(&mass_status.account_id),
                        None,
                    )
                    .is_empty()
            };

            if has_retained_position {
                log::debug!(
                    "Skipping fill adjustment for {instrument_id}: retained open position in cache"
                );
                continue;
            }

            if let Some(instrument) = self.get_instrument(&instrument_id) {
                instruments_to_adjust.push(instrument);
            } else {
                log::debug!(
                    "Skipping fill adjustment for {instrument_id}: instrument not found in cache"
                );
            }
        }

        if instruments_to_adjust.is_empty() {
            return (final_orders, final_fills);
        }

        log_info!(
            "Adjusting fills for {} instrument(s) with position reports",
            instruments_to_adjust.len(),
            color = LogColor::Blue
        );

        for instrument in &instruments_to_adjust {
            let instrument_id = instrument.id();

            let result = if self.config.generate_missing_orders {
                process_mass_status_for_reconciliation(mass_status, instrument, None)
            } else {
                process_mass_status_for_reconciliation_without_synthetic_reports(
                    mass_status,
                    instrument,
                    None,
                )
            };

            match result {
                Ok(result) => {
                    final_orders.retain(|_, order| order.instrument_id != instrument_id);
                    final_fills.retain(|_, fills| {
                        fills
                            .first()
                            .is_none_or(|f| f.instrument_id != instrument_id)
                    });

                    for (venue_order_id, order) in result.orders {
                        final_orders.insert(venue_order_id, order);
                    }

                    for (venue_order_id, fills) in result.fills {
                        final_fills.insert(venue_order_id, fills);
                    }
                }
                Err(e) => {
                    log::warn!("Failed to adjust fills for {instrument_id}: {e}");
                }
            }
        }

        log_info!(
            "After adjustment: {} order(s), {} fill group(s)",
            final_orders.len(),
            final_fills.len(),
            color = LogColor::Blue
        );

        (final_orders, final_fills)
    }

    fn is_fill_applied(&self, fill: &OrderFilled, fill_key: FillKey) -> bool {
        if fill.last_qty.is_zero() {
            return false;
        }

        self.get_order(fill.client_order_id)
            .or_else(|| self.get_order_by_venue_order_id(fill.venue_order_id))
            .is_some_and(|order| {
                order.account_id() == Some(fill_key.0)
                    && order.instrument_id() == fill_key.1
                    && order.trade_ids().contains(&&fill_key.2)
            })
    }

    fn create_order_fill(
        &self,
        order: &OrderAny,
        fill: &FillReport,
        instrument: &InstrumentAny,
        pending_fill_keys: &IndexSet<FillKey>,
    ) -> Option<(OrderFilled, FillKey)> {
        if fill.last_qty.is_zero() {
            log::warn!("Skipping zero-quantity fill report: {fill}");
            return None;
        }

        let fill_key = (fill.account_id, fill.instrument_id, fill.trade_id);
        if self.fills_processed.contains_key(&fill_key) || pending_fill_keys.contains(&fill_key) {
            return None;
        }

        let order_side = order.order_side();
        if fill.order_side != order_side {
            log::warn!(
                "Fill side mismatch for {}: cached={:?}, venue={:?}",
                order.client_order_id(),
                order_side,
                fill.order_side,
            );
        }

        let ts_now = self.clock.borrow().timestamp_ns();

        let event = OrderFilled::new(
            order.trader_id(),
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            fill.venue_order_id,
            fill.account_id,
            fill.trade_id,
            fill.order_side,
            order.order_type(),
            fill.last_qty,
            fill.last_px,
            instrument.quote_currency(),
            fill.liquidity_side,
            fill.report_id,
            fill.ts_event,
            ts_now,
            false,
            fill.venue_position_id,
            Some(fill.commission),
            None,
        );

        Some((event, fill_key))
    }
}

#[cfg(test)]
mod tests {
    use nautilus_common::{clock::TestClock, config::ConfigError};
    use nautilus_core::{DurationNanos, Params};
    use nautilus_execution::reconciliation::generate_reconciliation_order_events;
    use nautilus_model::{
        accounts::AccountAny,
        enums::{LiquiditySide, OmsType, PositionSide},
        events::order::spec::{OrderPendingCancelSpec, OrderPendingUpdateSpec, OrderUpdatedSpec},
        identifiers::{Symbol, Venue},
        instruments::{
            CurrencyPair, Instrument,
            stubs::{crypto_perpetual_ethusdt, xbtusd_bitmex},
        },
        orders::{OrderTestBuilder, stubs::TestOrderEventStubs},
        types::{AccountBalance, Currency, MarginBalance, Money},
    };
    use rstest::rstest;
    use rust_decimal_macros::dec;

    use super::{
        super::reconciliation::tests::{CommissionOutcome, CommissionStubClient},
        *,
    };

    #[rstest]
    fn test_new_validates_open_check_lookback_mins_boundaries() {
        let create_manager = |mins| {
            ExecutionManager::new(
                Rc::new(RefCell::new(TestClock::new())),
                Rc::new(RefCell::new(Cache::default())),
                ExecutionManagerConfig {
                    open_check_lookback_mins: Some(mins),
                    ..Default::default()
                },
            )
        };

        assert!(create_manager(307_445_734).is_ok());
        assert!(matches!(
            create_manager(307_445_735),
            Err(ConfigError::Range { field, .. })
                if field == "ExecutionManagerConfig.open_check_lookback_mins"
        ));
    }

    #[rstest]
    fn test_new_validates_reconciliation_lookback_mins_boundaries() {
        let create_manager = |mins| {
            ExecutionManager::new(
                Rc::new(RefCell::new(TestClock::new())),
                Rc::new(RefCell::new(Cache::default())),
                ExecutionManagerConfig {
                    lookback_mins: Some(mins),
                    ..Default::default()
                },
            )
        };

        assert!(create_manager(307_445_734_561_825_860).is_ok());
        assert!(matches!(
            create_manager(307_445_734_561_825_861),
            Err(ConfigError::Range { field, .. })
                if field == "ExecutionManagerConfig.lookback_mins"
        ));
    }

    #[rstest]
    fn test_new_reports_every_invalid_lookback_field() {
        let error = ExecutionManager::new(
            Rc::new(RefCell::new(TestClock::new())),
            Rc::new(RefCell::new(Cache::default())),
            ExecutionManagerConfig {
                lookback_mins: Some(307_445_734_561_825_861),
                open_check_lookback_mins: Some(307_445_735),
                position_check_lookback_mins: 307_445_735,
                ..Default::default()
            },
        )
        .expect_err("all lookback fields are out of range");

        let ConfigError::Multiple { errors } = error else {
            panic!("expected a `Multiple` error, was {error:?}");
        };

        let fields = errors
            .iter()
            .map(|e| match e {
                ConfigError::Range { field, .. } => field.as_str(),
                other => panic!("expected a `Range` error, was {other:?}"),
            })
            .collect::<Vec<_>>();

        assert_eq!(
            fields,
            [
                "ExecutionManagerConfig.lookback_mins",
                "ExecutionManagerConfig.open_check_lookback_mins",
                "ExecutionManagerConfig.position_check_lookback_mins",
            ]
        );
    }

    struct PositionCoverageStubClient;

    #[async_trait::async_trait(?Send)]
    impl ExecutionClient for PositionCoverageStubClient {
        fn is_connected(&self) -> bool {
            true
        }

        fn client_id(&self) -> ClientId {
            ClientId::from("BYBIT")
        }

        fn account_id(&self) -> AccountId {
            AccountId::from("TEST-001")
        }

        fn venue(&self) -> Venue {
            Venue::from("BYBIT")
        }

        fn oms_type(&self) -> OmsType {
            OmsType::Netting
        }

        fn get_account(&self) -> Option<AccountAny> {
            None
        }

        fn provides_bulk_position_coverage(&self, instrument_id: InstrumentId) -> bool {
            !instrument_id.symbol.as_str().ends_with("-SPOT")
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

    fn cached_commission_fixtures() -> (
        ExecutionManager,
        Rc<RefCell<Cache>>,
        OrderAny,
        OrderStatusReport,
        InstrumentAny,
    ) {
        let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt());
        let clock = Rc::new(RefCell::new(TestClock::new()));
        let cache = Rc::new(RefCell::new(Cache::default()));
        cache
            .borrow_mut()
            .add_instrument(instrument.clone())
            .expect("instrument is cacheable");
        let client_order_id = ClientOrderId::from("O-COMMISSION-CACHED");
        let venue_order_id = VenueOrderId::from("V-COMMISSION-CACHED");
        insert_accepted_limit_order(
            &cache,
            client_order_id,
            venue_order_id,
            instrument.id(),
            ClientId::from("STUB"),
        );
        let order = cache
            .borrow()
            .order_owned(&client_order_id)
            .expect("accepted order is cached");
        let report = OrderStatusReport::new(
            AccountId::from("TEST-001"),
            instrument.id(),
            Some(client_order_id),
            venue_order_id,
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
        let manager =
            ExecutionManager::new(clock, cache.clone(), ExecutionManagerConfig::default())
                .expect("valid config");

        (manager, cache, order, report, instrument)
    }

    fn external_report_with_partial_fill(
        instrument: &InstrumentAny,
    ) -> (OrderStatusReport, FillReport) {
        let account_id = AccountId::from("STUB-001");
        let venue_order_id = VenueOrderId::from("V-EXT-1");
        let report = OrderStatusReport::new(
            account_id,
            instrument.id(),
            None,
            venue_order_id,
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
        .with_price(Price::from("100.00"))
        .with_avg_px(dec!(100.0));

        let fill = FillReport::new(
            account_id,
            instrument.id(),
            venue_order_id,
            TradeId::from("T-EXT-1"),
            OrderSide::Buy,
            Quantity::from("4.0"),
            Price::from("100.00"),
            Money::new(0.1, Currency::USDT()),
            LiquiditySide::Taker,
            None,
            None,
            UnixNanos::from(1),
            UnixNanos::from(1),
            None,
        );

        (report, fill)
    }

    fn inferred_fills(events: &[OrderEventAny]) -> Vec<OrderFilled> {
        events
            .iter()
            .filter_map(|event| match event {
                OrderEventAny::Filled(filled) if filled.last_qty == Quantity::from("6.0") => {
                    Some(filled.clone())
                }
                _ => None,
            })
            .collect()
    }

    #[rstest]
    fn test_handle_external_order_applies_venue_commission_to_inferred_fill() {
        let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt());
        let clock = Rc::new(RefCell::new(TestClock::new()));
        let cache = Rc::new(RefCell::new(Cache::default()));
        cache
            .borrow_mut()
            .add_instrument(instrument.clone())
            .expect("instrument is cacheable");
        let manager = ExecutionManager::new(clock, cache, ExecutionManagerConfig::default())
            .expect("valid config");
        let (report, fill) = external_report_with_partial_fill(&instrument);
        let expected = Money::new(2.5, Currency::USDT());
        let client = CommissionStubClient::new(CommissionOutcome::Value(expected));
        let mut fill_queue = ReconciliationFillQueue::default();

        let (events, _) = manager.handle_external_order(
            &report,
            AccountId::from("STUB-001"),
            &instrument,
            &[&fill],
            false,
            Some(&mut fill_queue),
            Some(&client),
        );

        let inferred = inferred_fills(&events);
        assert_eq!(inferred.len(), 1, "one inferred fill covers the 6.0 gap");
        assert_eq!(inferred[0].commission, Some(expected));
    }

    #[rstest]
    fn test_handle_external_order_skips_inferred_fill_when_commission_fails() {
        let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt());
        let clock = Rc::new(RefCell::new(TestClock::new()));
        let cache = Rc::new(RefCell::new(Cache::default()));
        cache
            .borrow_mut()
            .add_instrument(instrument.clone())
            .expect("instrument is cacheable");
        let manager =
            ExecutionManager::new(clock, cache.clone(), ExecutionManagerConfig::default())
                .expect("valid config");
        let (report, fill) = external_report_with_partial_fill(&instrument);
        let client = CommissionStubClient::new(CommissionOutcome::Failure);
        let mut fill_queue = ReconciliationFillQueue::default();

        let (events, metadata) = manager.handle_external_order(
            &report,
            AccountId::from("STUB-001"),
            &instrument,
            &[&fill],
            false,
            Some(&mut fill_queue),
            Some(&client),
        );

        assert!(events.is_empty());
        assert!(metadata.is_none());
        assert!(fill_queue.pending_fill_keys.is_empty());
        assert!(
            cache
                .borrow()
                .order(&ClientOrderId::from(report.venue_order_id.as_str()))
                .is_none(),
            "commission failure must precede external order cache mutation"
        );

        let expected = Money::new(2.5, Currency::USDT());
        let retry_client = CommissionStubClient::new(CommissionOutcome::Value(expected));
        let (retry_events, retry_metadata) = manager.handle_external_order(
            &report,
            AccountId::from("STUB-001"),
            &instrument,
            &[&fill],
            false,
            Some(&mut fill_queue),
            Some(&retry_client),
        );
        let inferred = inferred_fills(&retry_events);

        assert!(retry_metadata.is_some());
        assert_eq!(inferred.len(), 1);
        assert_eq!(inferred[0].commission, Some(expected));
        assert_eq!(fill_queue.pending_fill_keys.len(), 1);
        assert!(
            cache
                .borrow()
                .order(&ClientOrderId::from(report.venue_order_id.as_str()))
                .is_some()
        );
    }

    #[rstest]
    fn test_handle_external_order_without_explicit_fills_resolves_commission_before_cache() {
        let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt());
        let clock = Rc::new(RefCell::new(TestClock::new()));
        let cache = Rc::new(RefCell::new(Cache::default()));
        cache
            .borrow_mut()
            .add_instrument(instrument.clone())
            .expect("instrument is cacheable");
        let manager =
            ExecutionManager::new(clock, cache.clone(), ExecutionManagerConfig::default())
                .expect("valid config");
        let (report, _) = external_report_with_partial_fill(&instrument);
        let failing_client = CommissionStubClient::new(CommissionOutcome::Failure);

        let (failed_events, failed_metadata) = manager.handle_external_order(
            &report,
            AccountId::from("STUB-001"),
            &instrument,
            &[],
            false,
            None,
            Some(&failing_client),
        );

        assert!(failed_events.is_empty());
        assert!(failed_metadata.is_none());
        assert!(
            cache
                .borrow()
                .order(&ClientOrderId::from(report.venue_order_id.as_str()))
                .is_none()
        );

        let expected = Money::new(4.0, Currency::USDT());
        let retry_client = CommissionStubClient::new(CommissionOutcome::Value(expected));
        let (retry_events, retry_metadata) = manager.handle_external_order(
            &report,
            AccountId::from("STUB-001"),
            &instrument,
            &[],
            false,
            None,
            Some(&retry_client),
        );

        let fills: Vec<_> = retry_events
            .iter()
            .filter_map(|event| match event {
                OrderEventAny::Filled(fill) => Some(fill),
                _ => None,
            })
            .collect();

        assert!(retry_metadata.is_some());
        assert_eq!(fills.len(), 1);
        assert_eq!(fills[0].last_qty, Quantity::from("10.0"));
        assert_eq!(fills[0].commission, Some(expected));
    }

    #[rstest]
    fn test_handle_external_order_with_no_override_emits_fill_without_commission() {
        let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt());
        let clock = Rc::new(RefCell::new(TestClock::new()));
        let cache = Rc::new(RefCell::new(Cache::default()));
        cache
            .borrow_mut()
            .add_instrument(instrument.clone())
            .expect("instrument is cacheable");
        let manager = ExecutionManager::new(clock, cache, ExecutionManagerConfig::default())
            .expect("valid config");
        let (report, fill) = external_report_with_partial_fill(&instrument);
        let client = CommissionStubClient::new(CommissionOutcome::NoOverride);
        let mut fill_queue = ReconciliationFillQueue::default();

        let (events, _) = manager.handle_external_order(
            &report,
            AccountId::from("STUB-001"),
            &instrument,
            &[&fill],
            false,
            Some(&mut fill_queue),
            Some(&client),
        );

        let inferred = inferred_fills(&events);
        assert_eq!(inferred.len(), 1);
        assert_eq!(inferred[0].commission, None);
    }

    #[rstest]
    #[case::filled(OrderStatus::Filled, "10.0", "6.0", "33.33", 1)]
    #[case::canceled(OrderStatus::Canceled, "8.0", "4.0", "20.00", 2)]
    #[case::expired(OrderStatus::Expired, "8.0", "4.0", "20.00", 2)]
    fn test_cached_reconciliation_applies_explicit_fill_and_defers_failed_residual(
        #[case] status: OrderStatus,
        #[case] filled_qty: Quantity,
        #[case] residual_qty: Quantity,
        #[case] residual_px: Price,
        #[case] event_count: usize,
    ) {
        let (mut manager, _cache, order, mut report, instrument) = cached_commission_fixtures();
        report.order_status = status;
        report.filled_qty = filled_qty;
        report.avg_px = Some(dec!(60.0));

        let explicit_fill = FillReport::new(
            report.account_id,
            report.instrument_id,
            report.venue_order_id,
            TradeId::from("T-COMMISSION-EXPLICIT"),
            OrderSide::Buy,
            Quantity::from("4.0"),
            Price::from("100.0"),
            Money::new(0.25, Currency::USDT()),
            LiquiditySide::Taker,
            report.client_order_id,
            None,
            UnixNanos::from(1),
            UnixNanos::from(1),
            None,
        );
        let failing_client = CommissionStubClient::new(CommissionOutcome::Failure);
        let mut fill_queue = ReconciliationFillQueue::default();

        let first_events = manager.reconcile_order_with_fills(
            true,
            &order,
            &report,
            &[&explicit_fill],
            Some(&instrument),
            &mut fill_queue,
            Some(&failing_client),
        );
        let mut working = order;
        for event in &first_events {
            working
                .apply(event.clone())
                .expect("explicit fill projects cleanly");
        }

        assert_eq!(first_events.len(), 1);

        let OrderEventAny::Filled(explicit) = &first_events[0] else {
            panic!("expected the valid explicit fill");
        };

        assert_eq!(explicit.last_qty, Quantity::from("4.0"));
        assert_eq!(
            explicit.commission,
            Some(Money::new(0.25, Currency::USDT()))
        );
        assert_eq!(working.status(), OrderStatus::PartiallyFilled);

        let expected = Money::new(1.5, Currency::USDT());
        let retry_client = CommissionStubClient::new(CommissionOutcome::Value(expected));
        let mut reported_residual = explicit_fill.clone();
        reported_residual.trade_id = TradeId::from("T-COMMISSION-RESIDUAL");
        reported_residual.last_qty = residual_qty;
        reported_residual.last_px = residual_px;
        reported_residual.commission = expected;

        let residual_reports = if status == OrderStatus::Filled {
            Vec::new()
        } else {
            vec![&reported_residual]
        };

        let retry_events = manager.reconcile_order_with_fills(
            true,
            &working,
            &report,
            &residual_reports,
            Some(&instrument),
            &mut fill_queue,
            Some(&retry_client),
        );

        assert_eq!(retry_events.len(), event_count);

        let OrderEventAny::Filled(residual) = &retry_events[0] else {
            panic!("expected the residual fill");
        };

        assert_eq!(residual.last_qty, residual_qty);
        assert_eq!(residual.last_px, residual_px);
        assert_eq!(residual.commission, Some(expected));
        assert_eq!(
            retry_client.seen(),
            (status == OrderStatus::Filled).then_some((
                residual_qty,
                residual.last_px,
                residual.liquidity_side
            )),
            "commission must use the exact price and liquidity carried by the residual fill"
        );

        for event in &retry_events {
            working
                .apply(event.clone())
                .expect("residual precedes terminal status");
        }

        retry_client.clear_seen();
        let replay = manager.reconcile_order_with_fills(
            true,
            &working,
            &report,
            &[],
            Some(&instrument),
            &mut fill_queue,
            Some(&retry_client),
        );

        assert_eq!(working.status(), status);
        assert_eq!(working.filled_qty(), filled_qty);
        assert_eq!(
            working.commissions().get(&Currency::USDT()),
            Some(&Money::from("1.75 USDT"))
        );
        assert!(replay.is_empty());
        assert_eq!(retry_client.seen(), None);
    }

    #[rstest]
    fn test_cached_reconciliation_preserves_explicit_fill_side() {
        let (mut manager, _cache, order, mut report, instrument) = cached_commission_fixtures();
        report.order_status = OrderStatus::PartiallyFilled;
        report.filled_qty = Quantity::from("4.0");
        let trade_id = TradeId::from("T-CONFLICTING-SIDE");

        let explicit_fill = FillReport::new(
            report.account_id,
            report.instrument_id,
            report.venue_order_id,
            trade_id,
            OrderSide::Sell,
            Quantity::from("4.0"),
            Price::from("100.0"),
            Money::new(0.25, Currency::USDT()),
            LiquiditySide::Taker,
            report.client_order_id,
            None,
            UnixNanos::from(1),
            UnixNanos::from(1),
            None,
        );
        let mut fill_queue = ReconciliationFillQueue::default();

        let events = manager.reconcile_order_with_fills(
            true,
            &order,
            &report,
            &[&explicit_fill],
            Some(&instrument),
            &mut fill_queue,
            None,
        );

        assert_eq!(events.len(), 1);

        let OrderEventAny::Filled(fill) = &events[0] else {
            panic!("expected the explicit fill");
        };

        assert_eq!(fill.trade_id, trade_id);
        assert_eq!(fill.order_side, OrderSide::Sell);
        assert_eq!(fill.last_qty, Quantity::from("4.0"));
        assert_eq!(fill.last_px, Price::from("100.0"));
    }

    #[rstest]
    fn test_continuous_report_preserves_newer_fills() {
        let (mut manager, _cache, mut order, mut report, instrument) = cached_commission_fixtures();
        let fill = TestOrderEventStubs::filled(
            &order,
            &instrument,
            Some(TradeId::from("T-NEWER-STREAM")),
            None,
            Some(Price::from("100.00")),
            Some(Quantity::from("2.0")),
            Some(LiquiditySide::Maker),
            None,
            None,
            Some(AccountId::from("TEST-001")),
        );
        order.apply(fill).unwrap();
        report.order_status = OrderStatus::PartiallyFilled;
        report.filled_qty = Quantity::from("1.0");
        let mut fill_queue = ReconciliationFillQueue::default();

        let events = manager.reconcile_order_with_fills(
            false,
            &order,
            &report,
            &[],
            Some(&instrument),
            &mut fill_queue,
            None,
        );

        assert!(events.is_empty());
        assert!(fill_queue.pending_fill_keys.is_empty());
    }

    #[rstest]
    #[case::with_fills(true)]
    #[case::without_fills(false)]
    fn test_cached_snapshot_without_instrument_defers_unaccounted_fills(#[case] has_fills: bool) {
        let (mut manager, _cache, order, mut report, _instrument) = cached_commission_fixtures();
        report.order_status = OrderStatus::Canceled;

        let explicit_fill = FillReport::new(
            report.account_id,
            report.instrument_id,
            report.venue_order_id,
            TradeId::from("T-MISSING-INSTRUMENT"),
            OrderSide::Buy,
            Quantity::from("4.0"),
            Price::from("100.0"),
            Money::new(0.25, Currency::USDT()),
            LiquiditySide::Taker,
            report.client_order_id,
            None,
            UnixNanos::from(1),
            UnixNanos::from(1),
            None,
        );
        let mut fill_queue = ReconciliationFillQueue::default();

        let fills = if has_fills {
            vec![&explicit_fill]
        } else {
            Vec::new()
        };

        let events = manager.reconcile_order_with_fills(
            true,
            &order,
            &report,
            &fills,
            None,
            &mut fill_queue,
            None,
        );

        assert!(events.is_empty());
        assert_eq!(order.status(), OrderStatus::Accepted);
        assert_eq!(order.filled_qty(), Quantity::from("0.0"));
        assert!(fill_queue.pending_fill_keys.is_empty());
    }

    #[rstest]
    #[case::canceled(OrderStatus::Canceled)]
    #[case::expired(OrderStatus::Expired)]
    fn test_terminal_order_report_does_not_void_cached_fills(#[case] status: OrderStatus) {
        let (manager, _cache, mut order, mut report, instrument) = cached_commission_fixtures();
        let fill = create_inferred_fill_for_qty(
            &order,
            &report,
            &report.account_id,
            &instrument,
            Quantity::from("4.0"),
            UnixNanos::from(1),
            None,
        )
        .unwrap();
        order.apply(fill).unwrap();
        report.order_status = status;
        report.filled_qty = Quantity::from("2.0");
        let client = CommissionStubClient::new(CommissionOutcome::Failure);

        let events = manager
            .reconcile_order_report(&order, &report, Some(&instrument), Some(&client))
            .unwrap();
        for event in &events {
            order.apply(event.clone()).unwrap();
        }

        assert_eq!(events.len(), 1);
        assert_eq!(order.status(), status);
        assert_eq!(order.filled_qty(), Quantity::from("4.0"));
        assert_eq!(client.seen(), None);
    }

    #[rstest]
    fn test_filled_order_ignores_superseded_cancel_report() {
        let (manager, cache, order, mut report, instrument) = cached_commission_fixtures();
        let venue_order_id = VenueOrderId::from("V-REPLACEMENT");
        let updated = OrderUpdatedSpec::builder()
            .trader_id(order.trader_id())
            .strategy_id(order.strategy_id())
            .instrument_id(order.instrument_id())
            .client_order_id(order.client_order_id())
            .account_id(report.account_id)
            .venue_order_id(venue_order_id)
            .quantity(order.quantity())
            .build();
        let order = cache
            .borrow_mut()
            .update_order(&OrderEventAny::Updated(updated))
            .unwrap();
        let mut fill_report = report.clone();
        fill_report.venue_order_id = venue_order_id;
        let commission = Money::from("1.25 USDT");
        let fill = create_inferred_fill_for_qty(
            &order,
            &fill_report,
            &report.account_id,
            &instrument,
            Quantity::from("10.0"),
            UnixNanos::from(2),
            Some(commission),
        )
        .unwrap();
        let order = cache.borrow_mut().update_order(&fill).unwrap();
        report.order_status = OrderStatus::Canceled;
        report.filled_qty = Quantity::from("0.0");
        report.avg_px = None;
        let client = CommissionStubClient::new(CommissionOutcome::Failure);

        let events = manager
            .reconcile_order_report(&order, &report, Some(&instrument), Some(&client))
            .unwrap();

        assert!(events.is_empty());
        assert_eq!(order.status(), OrderStatus::Filled);
        assert_eq!(order.venue_order_id(), Some(venue_order_id));
        assert_eq!(order.filled_qty(), Quantity::from("10.0"));
        assert_eq!(order.avg_px(), Some(dec!(100.0)));
        assert_eq!(
            order.commissions().get(&Currency::USDT()),
            Some(&commission)
        );
        assert_eq!(client.seen(), None);
    }

    #[rstest]
    fn test_continuous_reconciliation_uses_source_client_and_retries_commission() {
        let (mut manager, _cache, order, report, _instrument) = cached_commission_fixtures();
        let client_id = ClientId::from("STUB");

        let check = OpenOrderReportCheck {
            command: GenerateOrderStatusReports::new(
                UUID4::new(),
                UnixNanos::from(1),
                true,
                None,
                None,
                None,
                None,
                None,
            ),
            filtered_orders: vec![order],
            client_coverage: IndexMap::from([(
                report.client_order_id.unwrap(),
                ReportClientCoverage::Resolved(IndexSet::from([client_id])),
            )]),
        };

        let queried_clients = IndexSet::from([client_id]);
        let failed_clients = IndexSet::new();
        let failing_client = CommissionStubClient::new(CommissionOutcome::Failure);

        let failed = manager.reconcile_open_order_reports(
            &check,
            vec![SourcedOrderStatusReport {
                client_id,
                report: report.clone(),
            }],
            &queried_clients,
            &failed_clients,
            &[&failing_client],
        );

        assert!(failed.events.is_empty());

        let expected = Money::new(1.5, Currency::USDT());
        let retry_client = CommissionStubClient::new(CommissionOutcome::Value(expected));
        let retry = manager.reconcile_open_order_reports(
            &check,
            vec![SourcedOrderStatusReport { client_id, report }],
            &queried_clients,
            &failed_clients,
            &[&retry_client],
        );

        assert_eq!(retry.events.len(), 1);

        let OrderEventAny::Filled(fill) = &retry.events[0] else {
            panic!("expected inferred fill on valid retry");
        };

        assert_eq!(fill.last_qty, Quantity::from("10.0"));
        assert_eq!(fill.commission, Some(expected));
    }

    #[rstest]
    fn test_open_check_lookback_exclusion_warns_once_without_reconciliation_actions() {
        let client_order_id = ClientOrderId::from("O-LOOKBACK-OLD");
        let venue_order_id = VenueOrderId::from("V-LOOKBACK-OLD");
        let client_id = ClientId::from("BINANCE");
        let instrument_id = crypto_perpetual_ethusdt().id();
        let clock = Rc::new(RefCell::new(TestClock::new()));
        let cache = Rc::new(RefCell::new(Cache::default()));
        insert_accepted_limit_order(
            &cache,
            client_order_id,
            venue_order_id,
            instrument_id,
            client_id,
        );
        let order = cache.borrow().order_owned(&client_order_id).unwrap();
        let cutoff = order.ts_last().saturating_add(DurationNanos::new(1));

        let check = OpenOrderReportCheck {
            command: GenerateOrderStatusReports::new(
                UUID4::new(),
                UnixNanos::from(1),
                false,
                None,
                Some(cutoff),
                None,
                None,
                None,
            ),
            filtered_orders: vec![order],
            client_coverage: IndexMap::from([(
                client_order_id,
                ReportClientCoverage::Resolved(IndexSet::from([client_id])),
            )]),
        };

        let queried_clients = IndexSet::from([client_id]);

        let mut manager = ExecutionManager::new(
            clock,
            cache.clone(),
            ExecutionManagerConfig {
                open_check_open_only: false,
                ..Default::default()
            },
        )
        .expect("valid config");

        for _ in 0..2 {
            let result = manager.reconcile_open_order_reports(
                &check,
                Vec::new(),
                &queried_clients,
                &IndexSet::new(),
                &[],
            );

            assert!(result.events.is_empty());
            assert!(result.targeted_queries.is_empty());
            assert_eq!(
                cache.borrow().order(&client_order_id).unwrap().status(),
                OrderStatus::Accepted
            );
            assert!(!manager.order_recon_retries.contains_key(&client_order_id));
            assert!(!manager.order_query_recency.contains_key(&client_order_id));
            assert!(!manager.order_query_pending.contains(&client_order_id));
            assert_eq!(
                manager.order_lookback_warnings,
                IndexSet::from([client_order_id])
            );
            assert_eq!(manager.order_lookback_warnings.len(), 1);
        }
    }

    #[rstest]
    fn test_open_check_lookback_warning_clears_at_boundary_and_rearms() {
        let client_order_id = ClientOrderId::from("O-LOOKBACK-REARM");
        let client_id = ClientId::from("BINANCE");
        let cache = Rc::new(RefCell::new(Cache::default()));
        insert_accepted_limit_order(
            &cache,
            client_order_id,
            VenueOrderId::from("V-LOOKBACK-REARM"),
            crypto_perpetual_ethusdt().id(),
            client_id,
        );
        let order = cache.borrow().order_owned(&client_order_id).unwrap();
        let old_cutoff = order.ts_last().saturating_add(DurationNanos::new(1));

        let make_check = |start| OpenOrderReportCheck {
            command: GenerateOrderStatusReports::new(
                UUID4::new(),
                UnixNanos::from(1),
                false,
                None,
                Some(start),
                None,
                None,
                None,
            ),
            filtered_orders: vec![order.clone()],
            client_coverage: IndexMap::from([(
                client_order_id,
                ReportClientCoverage::Resolved(IndexSet::from([client_id])),
            )]),
        };

        let queried_clients = IndexSet::new();

        let mut manager = ExecutionManager::new(
            Rc::new(RefCell::new(TestClock::new())),
            cache,
            ExecutionManagerConfig {
                open_check_open_only: false,
                ..Default::default()
            },
        )
        .expect("valid config");

        manager.reconcile_open_order_reports(
            &make_check(old_cutoff),
            Vec::new(),
            &queried_clients,
            &IndexSet::new(),
            &[],
        );
        assert!(manager.order_lookback_warnings.contains(&client_order_id));

        let boundary = order.ts_last();
        let boundary_result = manager.reconcile_open_order_reports(
            &make_check(boundary),
            Vec::new(),
            &queried_clients,
            &IndexSet::new(),
            &[],
        );
        assert!(boundary_result.targeted_queries.is_empty());
        assert!(!manager.order_lookback_warnings.contains(&client_order_id));
        assert!(manager.order_coverage_warnings.contains(&client_order_id));

        manager.reconcile_open_order_reports(
            &make_check(old_cutoff),
            Vec::new(),
            &queried_clients,
            &IndexSet::new(),
            &[],
        );
        assert_eq!(
            manager.order_lookback_warnings,
            IndexSet::from([client_order_id])
        );
    }

    #[rstest]
    fn test_venue_order_id_mapped_report_clears_old_order_lookback_warning() {
        let client_order_id = ClientOrderId::from("O-LOOKBACK-MAPPED");
        let venue_order_id = VenueOrderId::from("V-LOOKBACK-MAPPED");
        let client_id = ClientId::from("BINANCE");
        let instrument_id = crypto_perpetual_ethusdt().id();
        let cache = Rc::new(RefCell::new(Cache::default()));
        insert_accepted_limit_order(
            &cache,
            client_order_id,
            venue_order_id,
            instrument_id,
            client_id,
        );
        let order = cache.borrow().order_owned(&client_order_id).unwrap();
        let cutoff = order.ts_last().saturating_add(DurationNanos::new(1));

        let check = OpenOrderReportCheck {
            command: GenerateOrderStatusReports::new(
                UUID4::new(),
                UnixNanos::from(1),
                false,
                None,
                Some(cutoff),
                None,
                None,
                None,
            ),
            filtered_orders: vec![order],
            client_coverage: IndexMap::from([(
                client_order_id,
                ReportClientCoverage::Resolved(IndexSet::from([client_id])),
            )]),
        };

        let mut manager = ExecutionManager::new(
            Rc::new(RefCell::new(TestClock::new())),
            cache,
            ExecutionManagerConfig {
                open_check_open_only: false,
                ..Default::default()
            },
        )
        .expect("valid config");

        manager.order_lookback_warnings.insert(client_order_id);

        // The report carries NO client_order_id, so it resolves through the
        // cache's venue_order_id mapping.
        let report = OrderStatusReport::new(
            AccountId::from("TEST-001"),
            instrument_id,
            None,
            venue_order_id,
            OrderSide::Buy.into(),
            OrderType::Limit,
            TimeInForce::Gtc,
            OrderStatus::Accepted,
            Quantity::from("10.0"),
            Quantity::from("0.0"),
            UnixNanos::from(0),
            UnixNanos::from(0),
            UnixNanos::from(0),
            None,
        );

        let result = manager.reconcile_open_order_reports(
            &check,
            vec![SourcedOrderStatusReport { client_id, report }],
            &IndexSet::from([client_id]),
            &IndexSet::new(),
            &[],
        );

        assert!(result.targeted_queries.is_empty());
        assert!(!manager.order_lookback_warnings.contains(&client_order_id));
    }

    #[rstest]
    fn test_positive_report_clears_old_order_lookback_warning_without_reinserting_it() {
        let client_order_id = ClientOrderId::from("O-LOOKBACK-REPORTED");
        let venue_order_id = VenueOrderId::from("V-LOOKBACK-REPORTED");
        let client_id = ClientId::from("BINANCE");
        let instrument_id = crypto_perpetual_ethusdt().id();
        let cache = Rc::new(RefCell::new(Cache::default()));
        insert_accepted_limit_order(
            &cache,
            client_order_id,
            venue_order_id,
            instrument_id,
            client_id,
        );
        let order = cache.borrow().order_owned(&client_order_id).unwrap();
        let cutoff = order.ts_last().saturating_add(DurationNanos::new(1));

        let check = OpenOrderReportCheck {
            command: GenerateOrderStatusReports::new(
                UUID4::new(),
                UnixNanos::from(1),
                false,
                None,
                Some(cutoff),
                None,
                None,
                None,
            ),
            filtered_orders: vec![order],
            client_coverage: IndexMap::from([(
                client_order_id,
                ReportClientCoverage::Resolved(IndexSet::from([client_id])),
            )]),
        };

        let mut manager = ExecutionManager::new(
            Rc::new(RefCell::new(TestClock::new())),
            cache,
            ExecutionManagerConfig {
                open_check_open_only: false,
                ..Default::default()
            },
        )
        .expect("valid config");

        manager.order_lookback_warnings.insert(client_order_id);

        let report = OrderStatusReport::new(
            AccountId::from("TEST-001"),
            instrument_id,
            Some(client_order_id),
            venue_order_id,
            OrderSide::Buy.into(),
            OrderType::Limit,
            TimeInForce::Gtc,
            OrderStatus::Accepted,
            Quantity::from("10.0"),
            Quantity::from("0.0"),
            UnixNanos::from(0),
            UnixNanos::from(0),
            UnixNanos::from(0),
            None,
        );

        let result = manager.reconcile_open_order_reports(
            &check,
            vec![SourcedOrderStatusReport { client_id, report }],
            &IndexSet::from([client_id]),
            &IndexSet::new(),
            &[],
        );

        assert!(result.targeted_queries.is_empty());
        assert!(!manager.order_lookback_warnings.contains(&client_order_id));
    }

    #[rstest]
    fn test_targeted_reconciliation_uses_source_client_and_retries_commission() {
        let (mut manager, _cache, _order, report, _instrument) = cached_commission_fixtures();
        let client_order_id = report.client_order_id.unwrap();
        let client_id = ClientId::from("STUB");
        let failing_client = CommissionStubClient::new(CommissionOutcome::Failure);

        let failed = manager.reconcile_targeted_order_reports(
            vec![TargetedOrderReportResult {
                client_order_id,
                client_id: Some(client_id),
                report: Some(report.clone()),
                fills: Vec::new(),
                coverage_complete: true,
            }],
            &[&failing_client],
        );

        assert!(failed.is_empty());

        let expected = Money::new(1.5, Currency::USDT());
        let retry_client = CommissionStubClient::new(CommissionOutcome::Value(expected));
        let retry = manager.reconcile_targeted_order_reports(
            vec![TargetedOrderReportResult {
                client_order_id,
                client_id: Some(client_id),
                report: Some(report),
                fills: Vec::new(),
                coverage_complete: true,
            }],
            &[&retry_client],
        );

        assert_eq!(retry.len(), 1);

        let OrderEventAny::Filled(fill) = &retry[0] else {
            panic!("expected inferred fill on valid targeted retry");
        };

        assert_eq!(fill.last_qty, Quantity::from("10.0"));
        assert_eq!(fill.commission, Some(expected));
    }

    #[rstest]
    fn test_clear_recon_tracking_removes_targeted_query() {
        let clock = Rc::new(RefCell::new(TestClock::new()));
        let cache = Rc::new(RefCell::new(Cache::default()));
        let mut manager = ExecutionManager::new(clock, cache, ExecutionManagerConfig::default())
            .expect("valid config");
        let client_order_id = ClientOrderId::from("O-TARGETED-CLEAR");
        manager.order_query_pending.insert(client_order_id);
        manager.order_lookback_warnings.insert(client_order_id);

        manager.clear_recon_tracking(&client_order_id, true);

        assert!(manager.order_query_pending.is_empty());
        assert!(manager.order_lookback_warnings.is_empty());
    }

    #[rstest]
    fn test_register_inflight_skips_filtered_order() {
        let client_order_id = ClientOrderId::from("O-FILTERED-REGISTER");
        let clock = Rc::new(RefCell::new(TestClock::new()));
        let cache = Rc::new(RefCell::new(Cache::default()));

        let mut manager = ExecutionManager::new(
            clock,
            cache,
            ExecutionManagerConfig {
                filtered_client_order_ids: IndexSet::from([client_order_id]),
                ..Default::default()
            },
        )
        .expect("valid config");

        manager.register_inflight(client_order_id);

        assert!(!manager.order_inflight_checks.contains_key(&client_order_id));
        assert!(!manager.order_recon_retries.contains_key(&client_order_id));
    }

    #[rstest]
    #[cfg_attr(
        not(all(feature = "simulation", madsim)),
        tokio::test(start_paused = true)
    )]
    #[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
    async fn test_inflight_check_retires_order_filtered_after_registration() {
        let client_order_id = ClientOrderId::from("O-FILTERED-LATE");
        let clock = Rc::new(RefCell::new(TestClock::new()));
        let cache = Rc::new(RefCell::new(Cache::default()));

        let mut manager = ExecutionManager::new(
            clock,
            cache,
            ExecutionManagerConfig {
                inflight_threshold_ms: 100,
                ..Default::default()
            },
        )
        .expect("valid config");

        manager.register_inflight(client_order_id);
        manager
            .config
            .filtered_client_order_ids
            .insert(client_order_id);
        dst::time::sleep(Duration::from_millis(101)).await;

        let first = manager.check_inflight_orders();

        assert!(first.events.is_empty());
        assert!(first.queries.is_empty());
        assert!(!manager.order_inflight_checks.contains_key(&client_order_id));
        assert!(!manager.order_recon_retries.contains_key(&client_order_id));

        dst::time::sleep(Duration::from_millis(101)).await;
        let second = manager.check_inflight_orders();
        assert!(second.events.is_empty());
        assert!(second.queries.is_empty());
        assert!(!manager.order_inflight_checks.contains_key(&client_order_id));
    }

    #[rstest]
    #[case(false, OrderStatus::PendingUpdate, true, true, true)]
    #[case(false, OrderStatus::Accepted, false, true, true)]
    #[case(false, OrderStatus::Canceled, false, true, false)]
    #[case(true, OrderStatus::PendingCancel, true, true, true)]
    #[case(true, OrderStatus::Accepted, false, true, true)]
    #[case(true, OrderStatus::Filled, false, true, false)]
    fn test_observe_order_status_report_tracking_matrix(
        #[case] with_fills: bool,
        #[case] status: OrderStatus,
        #[case] expect_inflight: bool,
        #[case] expect_activity: bool,
        #[case] expect_last_query: bool,
    ) {
        let client_order_id = ClientOrderId::from("O-STATUS-MATRIX");
        let clock = Rc::new(RefCell::new(TestClock::new()));
        let cache = Rc::new(RefCell::new(Cache::default()));
        let mut manager = ExecutionManager::new(clock, cache, ExecutionManagerConfig::default())
            .expect("valid config");
        manager.register_inflight(client_order_id);
        manager.order_query_recency.mark(client_order_id);
        manager.order_coverage_warnings.insert(client_order_id);
        manager.order_coverage_unresolved.insert(client_order_id);
        manager.order_query_pending.insert(client_order_id);

        let order_report = OrderStatusReport::new(
            AccountId::from("TEST-001"),
            crypto_perpetual_ethusdt().id(),
            Some(client_order_id),
            VenueOrderId::from("V-STATUS-MATRIX"),
            OrderSide::Buy.into(),
            OrderType::Limit,
            TimeInForce::Gtc,
            status,
            Quantity::from("10.0"),
            Quantity::from("0.0"),
            UnixNanos::from(1_000),
            UnixNanos::from(1_000),
            UnixNanos::from(1_000),
            None,
        );

        let report = if with_fills {
            ExecutionReport::OrderWithFills(Box::new(order_report), Vec::new())
        } else {
            ExecutionReport::Order(Box::new(order_report))
        };

        manager.observe_execution_report(&report);

        assert_eq!(
            manager.order_inflight_checks.contains_key(&client_order_id),
            expect_inflight,
        );
        assert_eq!(
            manager.order_recon_retries.contains_key(&client_order_id),
            expect_inflight,
        );
        assert_eq!(
            manager.order_activity.contains_key(&client_order_id),
            expect_activity,
        );
        assert_eq!(
            manager.order_query_recency.contains_key(&client_order_id),
            expect_last_query,
        );
        assert_eq!(
            manager.order_coverage_warnings.contains(&client_order_id),
            expect_inflight,
        );
        assert_eq!(
            manager.order_coverage_unresolved.contains(&client_order_id),
            expect_inflight,
        );
        assert_eq!(
            manager.order_query_pending.contains(&client_order_id),
            expect_inflight,
        );
    }

    #[rstest]
    #[case(OrderStatus::PendingUpdate)]
    #[case(OrderStatus::PendingCancel)]
    fn test_accepted_report_during_pending_command_preserves_inflight_tracking(
        #[case] pending_status: OrderStatus,
    ) {
        let client_order_id = ClientOrderId::from("O-PENDING-COMMAND");
        let venue_order_id = VenueOrderId::from("V-PENDING-COMMAND");
        let account_id = AccountId::from("TEST-001");
        let client_id = ClientId::from("TEST");
        let instrument_id = crypto_perpetual_ethusdt().id();
        let clock = Rc::new(RefCell::new(TestClock::new()));
        let cache = Rc::new(RefCell::new(Cache::default()));
        insert_accepted_limit_order(
            &cache,
            client_order_id,
            venue_order_id,
            instrument_id,
            client_id,
        );

        let order = cache.borrow().order_owned(&client_order_id).unwrap();

        let event = match pending_status {
            OrderStatus::PendingUpdate => OrderEventAny::PendingUpdate(
                OrderPendingUpdateSpec::builder()
                    .trader_id(order.trader_id())
                    .strategy_id(order.strategy_id())
                    .instrument_id(instrument_id)
                    .client_order_id(client_order_id)
                    .account_id(account_id)
                    .venue_order_id(venue_order_id)
                    .build(),
            ),
            OrderStatus::PendingCancel => OrderEventAny::PendingCancel(
                OrderPendingCancelSpec::builder()
                    .trader_id(order.trader_id())
                    .strategy_id(order.strategy_id())
                    .instrument_id(instrument_id)
                    .client_order_id(client_order_id)
                    .account_id(account_id)
                    .venue_order_id(venue_order_id)
                    .build(),
            ),
            _ => unreachable!(),
        };

        cache.borrow_mut().update_order(&event).unwrap();

        let mut manager =
            ExecutionManager::new(clock, cache.clone(), ExecutionManagerConfig::default())
                .expect("valid config");
        manager.register_inflight(client_order_id);
        manager.order_query_recency.mark(client_order_id);
        manager.order_coverage_warnings.insert(client_order_id);
        manager.order_coverage_unresolved.insert(client_order_id);
        manager.order_query_pending.insert(client_order_id);
        let report = OrderStatusReport::new(
            account_id,
            instrument_id,
            Some(client_order_id),
            venue_order_id,
            OrderSide::Buy.into(),
            OrderType::Limit,
            TimeInForce::Gtc,
            OrderStatus::Accepted,
            Quantity::from("10.0"),
            Quantity::from("0.0"),
            UnixNanos::from(1_000),
            UnixNanos::from(1_000),
            UnixNanos::from(1_000),
            None,
        )
        .with_price(Price::from("100.0"));

        manager.observe_execution_report(&ExecutionReport::Order(Box::new(report.clone())));
        let order = cache.borrow().order_owned(&client_order_id).unwrap();
        let events =
            generate_reconciliation_order_events(&order, &report, None, UnixNanos::from(1_000));

        assert!(events.is_empty());
        assert_eq!(order.status(), pending_status);
        assert!(manager.order_inflight_checks.contains_key(&client_order_id));
        assert!(manager.order_recon_retries.contains_key(&client_order_id));
        assert!(manager.order_query_recency.contains_key(&client_order_id));
        assert!(manager.order_activity.contains_key(&client_order_id));
        assert!(manager.order_coverage_warnings.contains(&client_order_id));
        assert!(manager.order_coverage_unresolved.contains(&client_order_id));
        assert!(manager.order_query_pending.contains(&client_order_id));
    }

    #[rstest]
    fn test_superseded_cancel_report_preserves_missing_order_grace() {
        let client_order_id = ClientOrderId::from("O-CANCEL-REPLACE");
        let old_venue_order_id = VenueOrderId::from("V-CANCEL-REPLACE-OLD");
        let new_venue_order_id = VenueOrderId::from("V-CANCEL-REPLACE-NEW");
        let account_id = AccountId::from("TEST-001");
        let client_id = ClientId::from("TEST");
        let instrument_id = crypto_perpetual_ethusdt().id();
        let clock = Rc::new(RefCell::new(TestClock::new()));
        let cache = Rc::new(RefCell::new(Cache::default()));
        insert_accepted_limit_order(
            &cache,
            client_order_id,
            old_venue_order_id,
            instrument_id,
            client_id,
        );

        let order = cache.borrow().order_owned(&client_order_id).unwrap();
        let pending_update = OrderPendingUpdateSpec::builder()
            .trader_id(order.trader_id())
            .strategy_id(order.strategy_id())
            .instrument_id(order.instrument_id())
            .client_order_id(client_order_id)
            .account_id(account_id)
            .venue_order_id(old_venue_order_id)
            .build();
        cache
            .borrow_mut()
            .update_order(&OrderEventAny::PendingUpdate(pending_update))
            .unwrap();
        let order = cache.borrow().order_owned(&client_order_id).unwrap();
        let updated = OrderUpdatedSpec::builder()
            .trader_id(order.trader_id())
            .strategy_id(order.strategy_id())
            .instrument_id(order.instrument_id())
            .client_order_id(client_order_id)
            .quantity(order.quantity())
            .venue_order_id(new_venue_order_id)
            .account_id(account_id)
            .build();
        cache
            .borrow_mut()
            .update_order(&OrderEventAny::Updated(updated))
            .unwrap();

        let mut manager = ExecutionManager::new(
            clock,
            cache.clone(),
            ExecutionManagerConfig {
                open_check_missing_retries: 1,
                ..Default::default()
            },
        )
        .expect("valid config");

        manager.record_local_activity(client_order_id);
        assert!(
            manager
                .prepare_missing_order_query(client_order_id)
                .is_none()
        );

        let report = OrderStatusReport::new(
            account_id,
            instrument_id,
            Some(client_order_id),
            old_venue_order_id,
            OrderSide::Buy.into(),
            OrderType::Limit,
            TimeInForce::Gtc,
            OrderStatus::Canceled,
            Quantity::from("10.0"),
            Quantity::from("0.0"),
            UnixNanos::from(1_000),
            UnixNanos::from(2_000),
            UnixNanos::from(3_000),
            None,
        );

        manager.observe_execution_report(&ExecutionReport::Order(Box::new(report.clone())));
        let order = cache.borrow().order_owned(&client_order_id).unwrap();
        let events =
            generate_reconciliation_order_events(&order, &report, None, UnixNanos::from(1_000));

        assert!(events.is_empty());
        assert_eq!(order.status(), OrderStatus::Accepted);
        assert_eq!(order.venue_order_id(), Some(new_venue_order_id));
        assert!(manager.order_activity.contains_key(&client_order_id));
        assert!(
            manager
                .prepare_missing_order_query(client_order_id)
                .is_none()
        );
        assert_eq!(manager.recon_check_retry_count(&client_order_id), 0);
    }

    #[rstest]
    #[cfg_attr(
        not(all(feature = "simulation", madsim)),
        tokio::test(start_paused = true)
    )]
    #[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
    async fn test_prune_order_local_activity_uses_open_check_threshold() {
        let old_id = ClientOrderId::from("O-ACTIVITY-OLD");
        let fresh_id = ClientOrderId::from("O-ACTIVITY-FRESH");
        let clock = Rc::new(RefCell::new(TestClock::new()));
        let cache = Rc::new(RefCell::new(Cache::default()));

        let mut manager = ExecutionManager::new(
            clock,
            cache,
            ExecutionManagerConfig {
                open_check_threshold_ns: DurationNanos::from_millis(100),
                ..Default::default()
            },
        )
        .expect("valid config");

        manager.record_local_activity(old_id);
        dst::time::sleep(Duration::from_millis(101)).await;
        manager.record_local_activity(fresh_id);

        manager.prune_order_local_activity();

        assert!(!manager.order_activity.contains_key(&old_id));
        assert!(manager.order_activity.contains_key(&fresh_id));
    }

    #[rstest]
    fn test_prepare_open_order_report_check_builds_bulk_command_with_config() {
        let lookback_mins = 5_u64;
        let lookback = DurationNanos::from_mins(lookback_mins);
        let clock = Rc::new(RefCell::new(TestClock::new()));
        let cache = Rc::new(RefCell::new(Cache::default()));

        let mut manager = ExecutionManager::new(
            clock.clone(),
            cache.clone(),
            ExecutionManagerConfig {
                open_check_lookback_mins: Some(lookback_mins),
                open_check_open_only: false,
                reconciliation_instrument_ids: IndexSet::from([crypto_perpetual_ethusdt().id()]),
                ..Default::default()
            },
        )
        .expect("valid config");

        let included_id = ClientOrderId::from("O-REPORT-001");
        let excluded_id = ClientOrderId::from("O-REPORT-002");
        let included_instrument_id = crypto_perpetual_ethusdt().id();
        let excluded_instrument_id = xbtusd_bitmex().id();

        cache
            .borrow_mut()
            .add_instrument(InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt()))
            .unwrap();
        cache
            .borrow_mut()
            .add_instrument(InstrumentAny::CryptoPerpetual(xbtusd_bitmex()))
            .unwrap();
        insert_accepted_limit_order(
            &cache,
            included_id,
            VenueOrderId::from("V-REPORT-001"),
            included_instrument_id,
            ClientId::from("BINANCE"),
        );
        insert_accepted_limit_order(
            &cache,
            excluded_id,
            VenueOrderId::from("V-REPORT-002"),
            excluded_instrument_id,
            ClientId::from("BITMEX"),
        );
        clock
            .borrow_mut()
            .advance_time(UnixNanos::default().saturating_add(lookback * 2), true);

        let ts_now = clock.borrow().timestamp_ns();
        let command_id = UUID4::new();
        let check = manager.prepare_open_order_report_check(command_id, &[]);

        assert_eq!(check.command.command_id, command_id);
        assert_eq!(check.command.ts_init, ts_now);
        assert!(!check.command.open_only);
        assert_eq!(check.command.instrument_id, None);
        assert_eq!(check.command.start, Some(ts_now.saturating_sub(lookback)));
        assert_eq!(check.command.end, None);
        assert_eq!(check.command.log_receipt_level, LogLevel::Debug);
        assert_eq!(check.filtered_orders.len(), 1);
        assert_eq!(check.filtered_orders[0].client_order_id(), included_id);
    }

    #[rstest]
    fn test_prepare_position_report_check_builds_bulk_command_with_coverage() {
        let clock = Rc::new(RefCell::new(TestClock::new()));
        let cache = Rc::new(RefCell::new(Cache::default()));

        let manager = ExecutionManager::new(
            clock.clone(),
            cache.clone(),
            ExecutionManagerConfig {
                reconciliation_instrument_ids: IndexSet::from([crypto_perpetual_ethusdt().id()]),
                ..Default::default()
            },
        )
        .expect("valid config");

        let included_instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt());
        let excluded_instrument = InstrumentAny::CryptoPerpetual(xbtusd_bitmex());

        cache
            .borrow_mut()
            .add_instrument(included_instrument.clone())
            .unwrap();
        cache
            .borrow_mut()
            .add_instrument(excluded_instrument.clone())
            .unwrap();
        let included_position = insert_open_position(
            &cache,
            &included_instrument,
            PositionId::from("P-REPORT-001"),
            OrderSide::Buy,
            "5.0",
            "3000.00",
        );
        insert_open_position(
            &cache,
            &excluded_instrument,
            PositionId::from("P-REPORT-002"),
            OrderSide::Buy,
            "2.0",
            "40000.00",
        );

        let ts_now = clock.borrow().timestamp_ns();
        let command_id = UUID4::new();
        let check = manager.prepare_position_report_check(command_id, &[]);
        let key = (
            included_position.instrument_id,
            included_position.account_id,
        );

        assert_eq!(check.command.command_id, command_id);
        assert_eq!(check.command.ts_init, ts_now);
        assert_eq!(check.command.instrument_id, None);
        assert_eq!(check.command.start, None);
        assert_eq!(check.command.end, None);
        assert_eq!(check.command.log_receipt_level, LogLevel::Debug);
        assert_eq!(check.client_coverage.len(), 1);
        assert_eq!(
            check.client_coverage.get(&key),
            Some(&ReportClientCoverage::Unresolved)
        );
        assert_eq!(check.activity_revisions.get(&key), Some(&0));
    }

    #[rstest]
    fn test_position_reconciliation_preserves_unavailable_spot_coverage() {
        let clock = Rc::new(RefCell::new(TestClock::new()));
        let cache = Rc::new(RefCell::new(Cache::default()));
        let mut manager =
            ExecutionManager::new(clock, cache.clone(), ExecutionManagerConfig::default())
                .expect("valid config");
        let derivative = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt());
        let spot = test_bybit_spot_instrument();
        cache
            .borrow_mut()
            .add_instrument(derivative.clone())
            .unwrap();
        cache.borrow_mut().add_instrument(spot.clone()).unwrap();
        let derivative_position = insert_open_position(
            &cache,
            &derivative,
            PositionId::from("P-DERIVATIVE-RECONCILE"),
            OrderSide::Buy,
            "5.0",
            "3000.00",
        );
        let spot_position = insert_open_position(
            &cache,
            &spot,
            PositionId::from("P-SPOT-PRESERVED"),
            OrderSide::Buy,
            "2.0",
            "2000.00",
        );
        let client = PositionCoverageStubClient;
        let check = manager.prepare_position_report_check(UUID4::new(), &[&client]);
        let queried_clients = IndexSet::from([client.client_id()]);

        let events = manager.reconcile_position_reports(
            &check,
            Vec::new(),
            &queried_clients,
            &IndexSet::new(),
        );

        assert!(events.iter().any(|event| {
            matches!(event, OrderEventAny::Filled(fill) if fill.instrument_id == derivative_position.instrument_id)
        }));
        assert!(!events.iter().any(|event| {
            matches!(event, OrderEventAny::Filled(fill) if fill.instrument_id == spot_position.instrument_id)
        }));
    }

    #[rstest]
    fn test_position_reconciliation_preserves_spot_position_when_client_query_fails() {
        let clock = Rc::new(RefCell::new(TestClock::new()));
        let cache = Rc::new(RefCell::new(Cache::default()));
        let mut manager =
            ExecutionManager::new(clock, cache.clone(), ExecutionManagerConfig::default())
                .expect("valid config");
        let instrument = test_bybit_spot_instrument();
        cache
            .borrow_mut()
            .add_instrument(instrument.clone())
            .unwrap();
        let position = insert_open_position(
            &cache,
            &instrument,
            PositionId::from("P-SPOT-QUERY-FAILED"),
            OrderSide::Buy,
            "5.0",
            "3000.00",
        );
        let key = (position.instrument_id, position.account_id);
        let client_id = ClientId::from("BYBIT");
        let mut check = manager.prepare_position_report_check(UUID4::new(), &[]);
        check.client_coverage.insert(
            key,
            ReportClientCoverage::Resolved(IndexSet::from([client_id])),
        );
        let queried_clients = IndexSet::from([client_id]);
        let failed_clients = IndexSet::from([client_id]);

        let events = manager.reconcile_position_reports(
            &check,
            Vec::new(),
            &queried_clients,
            &failed_clients,
        );

        assert!(
            !events
                .iter()
                .any(|event| matches!(event, OrderEventAny::Filled(_))),
            "a failed bulk query must not generate a synthetic closing fill",
        );
        let cached_position = cache.borrow().position(&position.id).unwrap().clone();
        assert!(cached_position.is_open());
        assert_eq!(cached_position.quantity, Quantity::from("5.0"));
    }

    #[rstest]
    #[cfg_attr(
        not(all(feature = "simulation", madsim)),
        tokio::test(start_paused = true)
    )]
    #[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
    async fn test_position_report_check_defers_activity_recorded_during_delayed_request() {
        let clock = Rc::new(RefCell::new(TestClock::new()));
        let cache = Rc::new(RefCell::new(Cache::default()));

        let mut manager = ExecutionManager::new(
            clock,
            cache.clone(),
            ExecutionManagerConfig {
                position_check_threshold_ns: DurationNanos::from_secs(5),
                ..Default::default()
            },
        )
        .expect("valid config");

        let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt());
        let instrument_id = instrument.id();
        let position = insert_open_position(
            &cache,
            &instrument,
            PositionId::from("P-ACTIVITY-DURING-REQUEST"),
            OrderSide::Buy,
            "5.0",
            "3000.00",
        );
        cache
            .borrow_mut()
            .add_instrument(instrument.clone())
            .unwrap();
        let account_id = position.account_id;
        let check = manager.prepare_position_report_check(UUID4::new(), &[]);

        let report = PositionStatusReport::new(
            account_id,
            instrument_id,
            PositionSide::Long,
            Quantity::from("5.0"),
            UnixNanos::from(1_000_000),
            UnixNanos::from(1_000_000),
            None,
            None,
            Some(Decimal::from(3000)),
        );

        let closed_position = close_long_position(
            position,
            &instrument,
            TradeId::from("T-ACTIVITY-DURING-REQUEST"),
        );
        cache
            .borrow_mut()
            .update_position(&closed_position)
            .unwrap();
        manager.record_position_activity(instrument_id, account_id);

        // Client A's report is already captured while client B holds the batch open.
        dst::time::sleep(Duration::from_secs(6)).await;

        let events = manager.reconcile_position_reports(
            &check,
            vec![report],
            &IndexSet::new(),
            &IndexSet::new(),
        );

        assert!(
            !events.iter().any(|event| {
                matches!(
                    event,
                    OrderEventAny::Filled(fill)
                        if fill.order_side == OrderSide::Buy
                            && fill.last_qty == Quantity::from("5.0")
                )
            }),
            "activity recorded after the request started must defer A's stale report",
        );
    }

    #[rstest]
    fn test_position_report_check_does_not_defer_activity_recorded_before_request() {
        let clock = Rc::new(RefCell::new(TestClock::new()));
        let cache = Rc::new(RefCell::new(Cache::default()));

        let mut manager = ExecutionManager::new(
            clock,
            cache.clone(),
            ExecutionManagerConfig {
                position_check_threshold_ns: DurationNanos::ZERO,
                ..Default::default()
            },
        )
        .expect("valid config");

        let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt());
        let instrument_id = instrument.id();
        let position = insert_open_position(
            &cache,
            &instrument,
            PositionId::from("P-ACTIVITY-BEFORE-REQUEST"),
            OrderSide::Buy,
            "5.0",
            "3000.00",
        );
        cache.borrow_mut().add_instrument(instrument).unwrap();
        let account_id = position.account_id;
        manager.record_position_activity(instrument_id, account_id);
        let check = manager.prepare_position_report_check(UUID4::new(), &[]);

        let report = PositionStatusReport::new(
            account_id,
            instrument_id,
            PositionSide::Long,
            Quantity::from("10.0"),
            UnixNanos::from(1_000_000),
            UnixNanos::from(1_000_000),
            None,
            None,
            Some(Decimal::from(3000)),
        );

        let events = manager.reconcile_position_reports(
            &check,
            vec![report],
            &IndexSet::new(),
            &IndexSet::new(),
        );

        let fills: Vec<_> = events
            .iter()
            .filter_map(|event| match event {
                OrderEventAny::Filled(fill) => Some(fill),
                _ => None,
            })
            .collect();

        assert_eq!(fills.len(), 1);
        assert_eq!(fills[0].order_side, OrderSide::Buy);
        assert_eq!(fills[0].last_qty, Quantity::from("5.0"));
        assert_eq!(fills[0].commission, None);
    }

    #[rstest]
    fn test_mass_status_projects_companion_fill_before_void_correction() {
        let clock = Rc::new(RefCell::new(TestClock::new()));
        let cache = Rc::new(RefCell::new(Cache::default()));
        let mut manager =
            ExecutionManager::new(clock, cache.clone(), ExecutionManagerConfig::default())
                .expect("valid config");
        let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt());
        let client_order_id = ClientOrderId::from("O-MASS-VOID-001");
        let venue_order_id = VenueOrderId::from("V-MASS-VOID-001");
        let account_id = AccountId::from("TEST-001");
        cache
            .borrow_mut()
            .add_instrument(instrument.clone())
            .unwrap();
        insert_accepted_limit_order(
            &cache,
            client_order_id,
            venue_order_id,
            instrument.id(),
            ClientId::from("BINANCE"),
        );
        let order = cache.borrow().order_owned(&client_order_id).unwrap();
        let initial_fill = TestOrderEventStubs::filled(
            &order,
            &instrument,
            Some(TradeId::from("T-MASS-VOID-INITIAL")),
            None,
            Some(Price::from("100.0")),
            Some(Quantity::from("6.0")),
            Some(LiquiditySide::Taker),
            None,
            None,
            Some(account_id),
        );
        cache.borrow_mut().update_order(&initial_fill).unwrap();
        let order = cache.borrow().order_owned(&client_order_id).unwrap();

        let report = OrderStatusReport::new(
            account_id,
            instrument.id(),
            Some(client_order_id),
            venue_order_id,
            OrderSide::Buy.into(),
            OrderType::Limit,
            TimeInForce::Gtc,
            OrderStatus::Canceled,
            Quantity::from("10.0"),
            Quantity::from("5.0"),
            UnixNanos::from(1_000),
            UnixNanos::from(1_000),
            UnixNanos::from(1_000),
            None,
        );

        let companion_fill = FillReport::new(
            account_id,
            instrument.id(),
            venue_order_id,
            TradeId::from("T-MASS-VOID-COMPANION"),
            OrderSide::Buy,
            Quantity::from("1.0"),
            Price::from("100.0"),
            Money::zero(instrument.quote_currency()),
            LiquiditySide::Taker,
            Some(client_order_id),
            None,
            UnixNanos::from(900),
            UnixNanos::from(1_000),
            None,
        );

        let mut fill_queue = ReconciliationFillQueue::default();
        let events = manager.reconcile_order_with_fills(
            true,
            &order,
            &report,
            &[&companion_fill],
            Some(&instrument),
            &mut fill_queue,
            None,
        );
        let mut projected = order;
        for event in &events {
            projected.apply(event.clone()).unwrap();
        }

        assert!(matches!(events[0], OrderEventAny::Filled(_)));
        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(event, OrderEventAny::FillVoided(_)))
                .count(),
            2
        );
        assert_eq!(projected.status(), OrderStatus::Canceled);
        assert_eq!(projected.filled_qty(), Quantity::from("5.0"));
        assert_eq!(projected.voided_qty(), Quantity::from("2.0"));
    }

    fn insert_accepted_limit_order(
        cache: &Rc<RefCell<Cache>>,
        client_order_id: ClientOrderId,
        venue_order_id: VenueOrderId,
        instrument_id: InstrumentId,
        client_id: ClientId,
    ) {
        let account_id = AccountId::from("TEST-001");
        let order = OrderTestBuilder::new(OrderType::Limit)
            .client_order_id(client_order_id)
            .instrument_id(instrument_id)
            .quantity(Quantity::from("10.0"))
            .price(Price::from("100.0"))
            .build();
        let submitted = TestOrderEventStubs::submitted(&order, account_id);
        cache
            .borrow_mut()
            .add_order(order, None, Some(client_id), false)
            .unwrap();
        let order = cache.borrow_mut().update_order(&submitted).unwrap();
        let accepted = TestOrderEventStubs::accepted(&order, account_id, venue_order_id);
        cache.borrow_mut().update_order(&accepted).unwrap();
    }

    fn test_bybit_spot_instrument() -> InstrumentAny {
        InstrumentAny::CurrencyPair(
            CurrencyPair::builder()
                .instrument_id(InstrumentId::from("ETHUSDT-SPOT.BYBIT"))
                .raw_symbol(Symbol::from("ETHUSDT"))
                .base_currency(Currency::from("ETH"))
                .quote_currency(Currency::from("USDT"))
                .price_precision(2)
                .size_precision(5)
                .price_increment(Price::from("0.01"))
                .size_increment(Quantity::from("0.00001"))
                .ts_event(UnixNanos::default())
                .ts_init(UnixNanos::default())
                .build()
                .unwrap(),
        )
    }

    fn insert_open_position(
        cache: &Rc<RefCell<Cache>>,
        instrument: &InstrumentAny,
        position_id: PositionId,
        side: OrderSide,
        quantity: &str,
        price: &str,
    ) -> Position {
        let order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(instrument.id())
            .side(side)
            .quantity(Quantity::from(quantity))
            .build();
        let fill = TestOrderEventStubs::filled(
            &order,
            instrument,
            Some(TradeId::new("T-REPORT-001")),
            Some(position_id),
            Some(Price::from(price)),
            Some(Quantity::from(quantity)),
            None,
            None,
            None,
            Some(AccountId::from("TEST-001")),
        );
        let order_filled: OrderFilled = fill.into();
        let position = Position::new(instrument, order_filled);
        cache
            .borrow_mut()
            .add_position(&position, OmsType::Hedging)
            .unwrap();
        position
    }

    fn close_long_position(
        mut position: Position,
        instrument: &InstrumentAny,
        trade_id: TradeId,
    ) -> Position {
        let order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(instrument.id())
            .side(OrderSide::Sell)
            .quantity(position.quantity)
            .build();
        let fill = TestOrderEventStubs::filled(
            &order,
            instrument,
            Some(trade_id),
            Some(position.id),
            Some(Price::from("3000.00")),
            Some(position.quantity),
            None,
            None,
            None,
            Some(position.account_id),
        );
        let order_filled: OrderFilled = fill.into();
        position.apply(&order_filled);
        position
    }

    #[cfg(feature = "node")]
    mod node {
        use super::*;
        use crate::execution::client::LiveExecutionClient;

        #[rstest]
        fn test_plan_position_fill_reports_uses_configured_lookback() {
            let lookback_mins = 7_u64;
            let lookback = DurationNanos::from_mins(lookback_mins);
            let clock = Rc::new(RefCell::new(TestClock::new()));
            let cache = Rc::new(RefCell::new(Cache::default()));

            let mut manager = ExecutionManager::new(
                clock.clone(),
                cache.clone(),
                ExecutionManagerConfig {
                    position_check_lookback_mins: lookback_mins,
                    position_check_threshold_ns: DurationNanos::ZERO,
                    ..Default::default()
                },
            )
            .expect("valid config");

            let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt());
            cache
                .borrow_mut()
                .add_instrument(instrument.clone())
                .unwrap();
            let position = insert_open_position(
                &cache,
                &instrument,
                PositionId::from("P-FILL-LOOKBACK"),
                OrderSide::Buy,
                "1.0",
                "3000.00",
            );
            clock
                .borrow_mut()
                .advance_time(UnixNanos::default().saturating_add(lookback * 2), true);
            let client = PositionCoverageStubClient;
            let clients: [&dyn ExecutionClient; 1] = [&client];
            let mut check = manager.prepare_position_report_check(UUID4::new(), &clients);
            let query_end = clock.borrow().timestamp_ns();

            let report = PositionStatusReport::new(
                position.account_id,
                position.instrument_id,
                PositionSide::Long,
                Quantity::from("2.0"),
                query_end,
                query_end,
                None,
                None,
                Some(dec!(3000.00)),
            );
            let queried_clients = IndexSet::from([client.client_id()]);

            let plan = manager.plan_position_fill_reports(
                &mut check,
                &[report],
                &queried_clients,
                &IndexSet::new(),
                &clients,
            );

            assert_eq!(
                plan.discrepancy_keys,
                IndexSet::from([(position.instrument_id, position.account_id)])
            );
            assert_eq!(plan.queries.len(), 1);
            let query = &plan.queries[0];
            assert_eq!(
                (query.key, query.client_id),
                (
                    (position.instrument_id, position.account_id),
                    client.client_id()
                )
            );
            assert_eq!(query.command.instrument_id, Some(position.instrument_id));
            assert_eq!(query.command.venue_order_id, None);
            assert_eq!(
                query.command.start,
                Some(query_end.saturating_sub(lookback))
            );
            assert_eq!(query.command.end, Some(query_end));
            assert_eq!(query.command.correlation_id, Some(check.command.command_id));
            assert_eq!(query.command.log_receipt_level, LogLevel::Debug);
        }

        #[rstest]
        fn test_plan_position_fill_reports_defers_position_opened_during_request() {
            let clock = Rc::new(RefCell::new(TestClock::new()));
            let cache = Rc::new(RefCell::new(Cache::default()));

            let mut manager = ExecutionManager::new(
                clock,
                cache.clone(),
                ExecutionManagerConfig {
                    position_check_threshold_ns: DurationNanos::ZERO,
                    ..Default::default()
                },
            )
            .expect("valid config");

            let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt());
            cache
                .borrow_mut()
                .add_instrument(instrument.clone())
                .unwrap();
            let client = PositionCoverageStubClient;
            let clients: [&dyn ExecutionClient; 1] = [&client];
            let mut check = manager.prepare_position_report_check(UUID4::new(), &clients);
            let position = insert_open_position(
                &cache,
                &instrument,
                PositionId::from("P-FILL-DURING-REQUEST"),
                OrderSide::Buy,
                "1.0",
                "3000.00",
            );
            manager.record_position_activity(position.instrument_id, position.account_id);

            let report = PositionStatusReport::new(
                position.account_id,
                position.instrument_id,
                PositionSide::Long,
                Quantity::from("2.0"),
                UnixNanos::from(1_000_000),
                UnixNanos::from(1_000_000),
                None,
                None,
                Some(dec!(3000.00)),
            );

            let plan = manager.plan_position_fill_reports(
                &mut check,
                &[report],
                &IndexSet::from([client.client_id()]),
                &IndexSet::new(),
                &clients,
            );

            assert_eq!(
                plan.discrepancy_keys,
                IndexSet::from([(position.instrument_id, position.account_id)])
            );
            assert!(plan.queries.is_empty());
            assert_eq!(
                check
                    .activity_revisions
                    .get(&(position.instrument_id, position.account_id)),
                Some(&0)
            );
        }

        #[rstest]
        fn test_prepare_position_report_check_uses_live_client_bulk_coverage() {
            let clock = Rc::new(RefCell::new(TestClock::new()));
            let cache = Rc::new(RefCell::new(Cache::default()));
            let manager =
                ExecutionManager::new(clock, cache.clone(), ExecutionManagerConfig::default())
                    .expect("valid config");
            let derivative = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt());
            let spot = test_bybit_spot_instrument();
            cache
                .borrow_mut()
                .add_instrument(derivative.clone())
                .unwrap();
            cache.borrow_mut().add_instrument(spot.clone()).unwrap();
            let derivative_position = insert_open_position(
                &cache,
                &derivative,
                PositionId::from("P-DERIVATIVE-COVERAGE"),
                OrderSide::Buy,
                "5.0",
                "3000.00",
            );
            let spot_position = insert_open_position(
                &cache,
                &spot,
                PositionId::from("P-SPOT-COVERAGE"),
                OrderSide::Buy,
                "2.0",
                "2000.00",
            );
            let client = LiveExecutionClient::new(Box::new(PositionCoverageStubClient));
            let client: &dyn ExecutionClient = &client;

            let check = manager.prepare_position_report_check(UUID4::new(), &[client]);
            let client_id = ClientId::from("BYBIT");

            assert_eq!(
                check.client_coverage.get(&(
                    derivative_position.instrument_id,
                    derivative_position.account_id
                )),
                Some(&ReportClientCoverage::Resolved(IndexSet::from([client_id])))
            );
            assert_eq!(
                check
                    .client_coverage
                    .get(&(spot_position.instrument_id, spot_position.account_id)),
                Some(&ReportClientCoverage::Unavailable(IndexSet::from([
                    client_id
                ])))
            );
        }
    }
}
