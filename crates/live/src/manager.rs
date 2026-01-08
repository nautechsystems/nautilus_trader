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

//! Execution state manager for live trading.
//!
//! This module provides the execution manager for reconciling execution state between
//! the local cache and connected venues, as well as purging old state during live trading.

use std::{cell::RefCell, fmt::Debug, rc::Rc, str::FromStr};

use ahash::{AHashMap, AHashSet};
use nautilus_common::{
    cache::Cache,
    clients::ExecutionClient,
    clock::Clock,
    enums::LogColor,
    log_info,
    messages::execution::report::{GenerateOrderStatusReports, GeneratePositionStatusReports},
};
use nautilus_core::{UUID4, UnixNanos};
use nautilus_execution::reconciliation::generate_external_order_status_events;
use nautilus_model::{
    enums::OrderStatus,
    events::{
        OrderAccepted, OrderCanceled, OrderEventAny, OrderExpired, OrderFilled, OrderInitialized,
        OrderRejected, OrderTriggered,
    },
    identifiers::{
        AccountId, ClientOrderId, InstrumentId, StrategyId, TradeId, TraderId, VenueOrderId,
    },
    instruments::{Instrument, InstrumentAny},
    orders::{Order, OrderAny},
    position::Position,
    reports::{ExecutionMassStatus, FillReport, OrderStatusReport, PositionStatusReport},
    types::Quantity,
};
use rust_decimal::Decimal;
use ustr::Ustr;

use crate::config::LiveExecEngineConfig;

/// Configuration for execution manager.
#[derive(Debug, Clone)]
pub struct ExecutionManagerConfig {
    /// The trader ID for generated orders.
    pub trader_id: TraderId,
    /// If reconciliation is active at start-up.
    pub reconciliation: bool,
    /// The delay (seconds) before starting reconciliation at startup.
    pub reconciliation_startup_delay_secs: f64,
    /// Number of minutes to look back during reconciliation.
    pub lookback_mins: Option<u64>,
    /// Instrument IDs to include during reconciliation (empty => all).
    pub reconciliation_instrument_ids: AHashSet<InstrumentId>,
    /// Whether to filter unclaimed external orders.
    pub filter_unclaimed_external: bool,
    /// Whether to filter position status reports during reconciliation.
    pub filter_position_reports: bool,
    /// Client order IDs excluded from reconciliation.
    pub filtered_client_order_ids: AHashSet<ClientOrderId>,
    /// Whether to generate missing orders from reports.
    pub generate_missing_orders: bool,
    /// The interval (milliseconds) between checking whether in-flight orders have exceeded their threshold.
    pub inflight_check_interval_ms: u32,
    /// Threshold in milliseconds for inflight order checks.
    pub inflight_threshold_ms: u64,
    /// Maximum number of retries for inflight checks.
    pub inflight_max_retries: u32,
    /// The interval (seconds) between checks for open orders at the venue.
    pub open_check_interval_secs: Option<f64>,
    /// The lookback minutes for open order checks.
    pub open_check_lookback_mins: Option<u64>,
    /// Threshold in nanoseconds before acting on venue discrepancies for open orders.
    pub open_check_threshold_ns: u64,
    /// Maximum retries before resolving an open order missing at the venue.
    pub open_check_missing_retries: u32,
    /// Whether open-order polling should only request open orders from the venue.
    pub open_check_open_only: bool,
    /// The maximum number of single-order queries per consistency check cycle.
    pub max_single_order_queries_per_cycle: u32,
    /// The delay (milliseconds) between consecutive single-order queries.
    pub single_order_query_delay_ms: u32,
    /// The interval (seconds) between checks for open positions at the venue.
    pub position_check_interval_secs: Option<f64>,
    /// The lookback minutes for position consistency checks.
    pub position_check_lookback_mins: u64,
    /// Threshold in nanoseconds before acting on venue discrepancies for positions.
    pub position_check_threshold_ns: u64,
    /// The time buffer (minutes) before closed orders can be purged.
    pub purge_closed_orders_buffer_mins: Option<u32>,
    /// The time buffer (minutes) before closed positions can be purged.
    pub purge_closed_positions_buffer_mins: Option<u32>,
    /// The time buffer (minutes) before account events can be purged.
    pub purge_account_events_lookback_mins: Option<u32>,
    /// If purge operations should also delete from the backing database.
    pub purge_from_database: bool,
}

impl Default for ExecutionManagerConfig {
    fn default() -> Self {
        Self {
            trader_id: TraderId::default(),
            reconciliation: true,
            reconciliation_startup_delay_secs: 10.0,
            lookback_mins: Some(60),
            reconciliation_instrument_ids: AHashSet::new(),
            filter_unclaimed_external: false,
            filter_position_reports: false,
            filtered_client_order_ids: AHashSet::new(),
            generate_missing_orders: true,
            inflight_check_interval_ms: 2_000,
            inflight_threshold_ms: 5_000,
            inflight_max_retries: 5,
            open_check_interval_secs: None,
            open_check_lookback_mins: Some(60),
            open_check_threshold_ns: 5_000_000_000,
            open_check_missing_retries: 5,
            open_check_open_only: true,
            max_single_order_queries_per_cycle: 5,
            single_order_query_delay_ms: 100,
            position_check_interval_secs: None,
            position_check_lookback_mins: 60,
            position_check_threshold_ns: 60_000_000_000,
            purge_closed_orders_buffer_mins: None,
            purge_closed_positions_buffer_mins: None,
            purge_account_events_lookback_mins: None,
            purge_from_database: false,
        }
    }
}

impl From<&LiveExecEngineConfig> for ExecutionManagerConfig {
    fn from(config: &LiveExecEngineConfig) -> Self {
        let filtered_client_order_ids: AHashSet<ClientOrderId> = config
            .filtered_client_order_ids
            .clone()
            .unwrap_or_default()
            .into_iter()
            .map(|value| ClientOrderId::from(value.as_str()))
            .collect();

        let reconciliation_instrument_ids: AHashSet<InstrumentId> = config
            .reconciliation_instrument_ids
            .clone()
            .unwrap_or_default()
            .into_iter()
            .map(|value| InstrumentId::from(value.as_str()))
            .collect();

        Self {
            trader_id: TraderId::default(), // Must be set separately via with_trader_id
            reconciliation: config.reconciliation,
            reconciliation_startup_delay_secs: config.reconciliation_startup_delay_secs,
            lookback_mins: config.reconciliation_lookback_mins.map(|m| m as u64),
            reconciliation_instrument_ids,
            filter_unclaimed_external: config.filter_unclaimed_external_orders,
            filter_position_reports: config.filter_position_reports,
            filtered_client_order_ids,
            generate_missing_orders: config.generate_missing_orders,
            inflight_check_interval_ms: config.inflight_check_interval_ms,
            inflight_threshold_ms: config.inflight_check_threshold_ms as u64,
            inflight_max_retries: config.inflight_check_retries,
            open_check_interval_secs: config.open_check_interval_secs,
            open_check_lookback_mins: config.open_check_lookback_mins.map(|m| m as u64),
            open_check_threshold_ns: (config.open_check_threshold_ms as u64) * 1_000_000,
            open_check_missing_retries: config.open_check_missing_retries,
            open_check_open_only: config.open_check_open_only,
            max_single_order_queries_per_cycle: config.max_single_order_queries_per_cycle,
            single_order_query_delay_ms: config.single_order_query_delay_ms,
            position_check_interval_secs: config.position_check_interval_secs,
            position_check_lookback_mins: config.position_check_lookback_mins as u64,
            position_check_threshold_ns: (config.position_check_threshold_ms as u64) * 1_000_000,
            purge_closed_orders_buffer_mins: config.purge_closed_orders_buffer_mins,
            purge_closed_positions_buffer_mins: config.purge_closed_positions_buffer_mins,
            purge_account_events_lookback_mins: config.purge_account_events_lookback_mins,
            purge_from_database: config.purge_from_database,
        }
    }
}

impl ExecutionManagerConfig {
    /// Sets the trader ID on the configuration.
    #[must_use]
    pub fn with_trader_id(mut self, trader_id: TraderId) -> Self {
        self.trader_id = trader_id;
        self
    }
}

/// Execution report for continuous reconciliation.
/// This is a simplified report type used during runtime reconciliation.
#[derive(Debug, Clone)]
pub struct ExecutionReport {
    pub client_order_id: ClientOrderId,
    pub venue_order_id: Option<VenueOrderId>,
    pub status: OrderStatus,
    pub filled_qty: Quantity,
    pub avg_px: Option<f64>,
    pub ts_event: UnixNanos,
}

/// Information about an inflight order check.
#[derive(Debug, Clone)]
struct InflightCheck {
    #[allow(dead_code)]
    pub client_order_id: ClientOrderId,
    pub ts_submitted: UnixNanos,
    pub retry_count: u32,
    pub last_query_ts: Option<UnixNanos>,
}

/// Manager for execution state.
///
/// The `ExecutionManager` handles:
/// - Startup reconciliation to align state on system start.
/// - Continuous reconciliation of inflight orders.
/// - External order discovery and claiming.
/// - Fill report processing and validation.
/// - Purging of old orders, positions, and account events.
///
/// # Thread Safety
///
/// This struct is **not thread-safe** and is designed for single-threaded use within
/// an async runtime. Internal state is managed using `AHashMap` without synchronization,
/// and the `clock` and `cache` use `Rc<RefCell<>>` which provide runtime borrow checking
/// but no thread-safety guarantees.
///
/// If concurrent access is required, this struct must be wrapped in `Arc<Mutex<>>` or
/// similar synchronization primitives. Alternatively, ensure that all methods are called
/// from the same thread/task in the async runtime.
///
/// **Warning:** Concurrent mutable access to internal AHashMaps or concurrent borrows
/// of `RefCell` contents will cause runtime panics.
#[derive(Clone)]
pub struct ExecutionManager {
    clock: Rc<RefCell<dyn Clock>>,
    cache: Rc<RefCell<Cache>>,
    config: ExecutionManagerConfig,
    inflight_checks: AHashMap<ClientOrderId, InflightCheck>,
    external_order_claims: AHashMap<InstrumentId, StrategyId>,
    processed_fills: AHashMap<TradeId, ClientOrderId>,
    recon_check_retries: AHashMap<ClientOrderId, u32>,
    ts_last_query: AHashMap<ClientOrderId, UnixNanos>,
    order_local_activity_ns: AHashMap<ClientOrderId, UnixNanos>,
    position_local_activity_ns: AHashMap<InstrumentId, UnixNanos>,
    recent_fills_cache: AHashMap<TradeId, UnixNanos>,
}

impl Debug for ExecutionManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(ExecutionManager))
            .field("config", &self.config)
            .field("inflight_checks", &self.inflight_checks)
            .field("external_order_claims", &self.external_order_claims)
            .field("processed_fills", &self.processed_fills)
            .field("recon_check_retries", &self.recon_check_retries)
            .finish()
    }
}

impl ExecutionManager {
    /// Creates a new [`ExecutionManager`] instance.
    pub fn new(
        clock: Rc<RefCell<dyn Clock>>,
        cache: Rc<RefCell<Cache>>,
        config: ExecutionManagerConfig,
    ) -> Self {
        Self {
            clock,
            cache,
            config,
            inflight_checks: AHashMap::new(),
            external_order_claims: AHashMap::new(),
            processed_fills: AHashMap::new(),
            recon_check_retries: AHashMap::new(),
            ts_last_query: AHashMap::new(),
            order_local_activity_ns: AHashMap::new(),
            position_local_activity_ns: AHashMap::new(),
            recent_fills_cache: AHashMap::new(),
        }
    }

    /// Reconciles orders and fills from a mass status report.
    pub async fn reconcile_execution_mass_status(
        &mut self,
        mass_status: ExecutionMassStatus,
    ) -> Vec<OrderEventAny> {
        let venue = mass_status.venue;
        let order_count = mass_status.order_reports().len();
        let fill_count: usize = mass_status.fill_reports().values().map(|v| v.len()).sum();
        let position_count = mass_status.position_reports().len();

        log_info!(
            "Reconciling ExecutionMassStatus for {}",
            venue,
            color = LogColor::Blue
        );
        log_info!(
            "Received {} order(s), {} fill(s), {} position(s)",
            order_count,
            fill_count,
            position_count,
            color = LogColor::Blue
        );

        let mut events = Vec::new();
        let mut orders_reconciled = 0usize;
        let mut external_orders_created = 0usize;
        let mut open_orders_initialized = 0usize;
        let mut orders_skipped_no_instrument = 0usize;
        let mut fills_applied = 0usize;

        // Process order status reports first
        for report in mass_status.order_reports().values() {
            if let Some(client_order_id) = &report.client_order_id {
                if let Some(order) = self.get_order(client_order_id) {
                    let mut order = order;
                    log::info!(
                        color = LogColor::Blue as u8;
                        "Reconciling {} {} {} [{}] -> [{}]",
                        client_order_id,
                        report.venue_order_id,
                        report.instrument_id,
                        order.status(),
                        report.order_status,
                    );
                    if let Some(event) = self.reconcile_order_report(&mut order, report) {
                        orders_reconciled += 1;
                        events.push(event);
                    }
                } else if !self.config.filter_unclaimed_external {
                    // Order has client_order_id but not in cache - external order
                    if let Some(instrument) = self.get_instrument(&report.instrument_id) {
                        let external_events = self.handle_external_order(
                            report,
                            &mass_status.account_id,
                            &instrument,
                        );
                        if !external_events.is_empty() {
                            external_orders_created += 1;
                            if report.order_status.is_open() {
                                open_orders_initialized += 1;
                            }
                            events.extend(external_events);
                        }
                    } else {
                        orders_skipped_no_instrument += 1;
                    }
                }
            } else if !self.config.filter_unclaimed_external {
                if let Some(instrument) = self.get_instrument(&report.instrument_id) {
                    let external_events =
                        self.handle_external_order(report, &mass_status.account_id, &instrument);
                    if !external_events.is_empty() {
                        external_orders_created += 1;
                        if report.order_status.is_open() {
                            open_orders_initialized += 1;
                        }
                        events.extend(external_events);
                    }
                } else {
                    orders_skipped_no_instrument += 1;
                }
            }
        }

        // Sort fills chronologically to ensure proper position updates
        let fill_reports = mass_status.fill_reports();
        let mut all_fills: Vec<&FillReport> = fill_reports.values().flatten().collect();
        all_fills.sort_by_key(|f| f.ts_event);

        for fill in all_fills {
            if let Some(client_order_id) = &fill.client_order_id
                && let Some(order) = self.get_order(client_order_id)
            {
                let mut order = order;
                let instrument_id = order.instrument_id();

                if let Some(instrument) = self.get_instrument(&instrument_id)
                    && let Some(event) = self.create_order_fill(&mut order, fill, &instrument)
                {
                    fills_applied += 1;
                    events.push(event);
                }
            }
        }

        if orders_skipped_no_instrument > 0 {
            log::warn!("{orders_skipped_no_instrument} orders skipped (instrument not in cache)");
        }

        log::info!(
            color = LogColor::Blue as u8;
            "Reconciliation complete for {venue}: reconciled={orders_reconciled}, external={external_orders_created}, open={open_orders_initialized}, fills={fills_applied}",
        );

        // Sort events chronologically to ensure proper position updates
        events.sort_by_key(|e| e.ts_event());

        events
    }

    /// Reconciles a single execution report during runtime.
    ///
    /// # Errors
    ///
    /// Returns an error if the average price cannot be converted to a valid `Decimal`.
    pub fn reconcile_report(
        &mut self,
        report: ExecutionReport,
    ) -> anyhow::Result<Vec<OrderEventAny>> {
        let mut events = Vec::new();

        self.clear_recon_tracking(&report.client_order_id, true);

        if let Some(order) = self.get_order(&report.client_order_id) {
            let mut order = order;
            let Some(account_id) = order.account_id() else {
                log::error!("Cannot process fill report: order has no account_id");
                return Ok(vec![]);
            };
            let Some(venue_order_id) = report.venue_order_id else {
                log::error!("Cannot process fill report: report has no venue_order_id");
                return Ok(vec![]);
            };
            let mut order_report = OrderStatusReport::new(
                account_id,
                order.instrument_id(),
                Some(report.client_order_id),
                venue_order_id,
                order.order_side(),
                order.order_type(),
                order.time_in_force(),
                report.status,
                order.quantity(),
                report.filled_qty,
                report.ts_event, // Use ts_event as ts_accepted
                report.ts_event, // Use ts_event as ts_last
                self.clock.borrow().timestamp_ns(),
                Some(UUID4::new()),
            );

            if let Some(avg_px) = report.avg_px {
                order_report = order_report.with_avg_px(avg_px)?;
            }

            if let Some(event) = self.reconcile_order_report(&mut order, &order_report) {
                events.push(event);
            }
        }

        Ok(events)
    }

    /// Checks inflight orders and returns events for any that need reconciliation.
    pub fn check_inflight_orders(&mut self) -> Vec<OrderEventAny> {
        let mut events = Vec::new();
        let current_time = self.clock.borrow().timestamp_ns();
        let threshold_ns = self.config.inflight_threshold_ms * 1_000_000;

        let mut to_check = Vec::new();
        for (client_order_id, check) in &self.inflight_checks {
            if current_time - check.ts_submitted > threshold_ns {
                to_check.push(*client_order_id);
            }
        }

        for client_order_id in to_check {
            if self
                .config
                .filtered_client_order_ids
                .contains(&client_order_id)
            {
                continue;
            }

            if let Some(check) = self.inflight_checks.get_mut(&client_order_id) {
                if let Some(last_query_ts) = check.last_query_ts
                    && current_time - last_query_ts < threshold_ns
                {
                    continue;
                }

                check.retry_count += 1;
                check.last_query_ts = Some(current_time);
                self.ts_last_query.insert(client_order_id, current_time);
                self.recon_check_retries
                    .insert(client_order_id, check.retry_count);

                if check.retry_count >= self.config.inflight_max_retries {
                    // Generate rejection after max retries
                    if let Some(order) = self.get_order(&client_order_id) {
                        events.push(self.create_order_rejected(&order, Some("INFLIGHT_TIMEOUT")));
                    }
                    // Remove from inflight checks regardless of whether order exists
                    self.clear_recon_tracking(&client_order_id, true);
                }
            }
        }

        events
    }

    /// Checks open orders consistency between cache and venue.
    ///
    /// This method validates that open orders in the cache match the venue's state,
    /// comparing order status and filled quantities, and generating reconciliation
    /// events for any discrepancies detected.
    ///
    /// # Returns
    ///
    /// A vector of order events generated to reconcile discrepancies.
    pub async fn check_open_orders(
        &mut self,
        clients: &[Rc<dyn ExecutionClient>],
    ) -> Vec<OrderEventAny> {
        log::debug!("Checking order consistency between cached-state and venues");

        let filtered_orders: Vec<OrderAny> = {
            let cache = self.cache.borrow();
            let open_orders = cache.orders_open(None, None, None, None);

            if !self.config.reconciliation_instrument_ids.is_empty() {
                open_orders
                    .iter()
                    .filter(|o| {
                        self.config
                            .reconciliation_instrument_ids
                            .contains(&o.instrument_id())
                    })
                    .map(|o| (*o).clone())
                    .collect()
            } else {
                open_orders.iter().map(|o| (*o).clone()).collect()
            }
        };

        log::debug!(
            "Found {} order{} open in cache",
            filtered_orders.len(),
            if filtered_orders.len() == 1 { "" } else { "s" }
        );

        let mut all_reports = Vec::new();
        let mut venue_reported_ids = AHashSet::new();

        for client in clients {
            let cmd = GenerateOrderStatusReports::new(
                UUID4::new(),
                self.clock.borrow().timestamp_ns(),
                true, // open_only
                None, // instrument_id - query all
                None, // start
                None, // end
                None, // params
                None, // correlation_id
            );

            match client.generate_order_status_reports(&cmd).await {
                Ok(reports) => {
                    for report in reports {
                        if let Some(client_order_id) = &report.client_order_id {
                            venue_reported_ids.insert(*client_order_id);
                        }
                        all_reports.push(report);
                    }
                }
                Err(e) => {
                    log::error!(
                        "Failed to query order reports from {}: {e}",
                        client.client_id()
                    );
                }
            }
        }

        // Reconcile reports against cached orders
        let mut events = Vec::new();
        for report in all_reports {
            if let Some(client_order_id) = &report.client_order_id
                && let Some(mut order) = self.get_order(client_order_id)
                && let Some(event) = self.reconcile_order_report(&mut order, &report)
            {
                events.push(event);
            }
        }

        // Handle orders missing at venue
        if !self.config.open_check_open_only {
            let cached_ids: AHashSet<ClientOrderId> = filtered_orders
                .iter()
                .map(|o| o.client_order_id())
                .collect();
            let missing_at_venue: AHashSet<ClientOrderId> = cached_ids
                .difference(&venue_reported_ids)
                .copied()
                .collect();

            for client_order_id in missing_at_venue {
                events.extend(self.handle_missing_order(client_order_id));
            }
        }

        events
    }

    /// Checks position consistency between cache and venue.
    ///
    /// This method validates that positions in the cache match the venue's state,
    /// detecting position drift and querying for missing fills when discrepancies
    /// are found.
    ///
    /// # Returns
    ///
    /// A vector of fill events generated to reconcile position discrepancies.
    pub async fn check_positions_consistency(
        &mut self,
        clients: &[Rc<dyn ExecutionClient>],
    ) -> Vec<OrderEventAny> {
        log::debug!("Checking position consistency between cached-state and venues");

        let open_positions = {
            let cache = self.cache.borrow();
            let positions = cache.positions_open(None, None, None, None);

            if !self.config.reconciliation_instrument_ids.is_empty() {
                positions
                    .iter()
                    .filter(|p| {
                        self.config
                            .reconciliation_instrument_ids
                            .contains(&p.instrument_id)
                    })
                    .map(|p| (*p).clone())
                    .collect::<Vec<_>>()
            } else {
                positions.iter().map(|p| (*p).clone()).collect()
            }
        };

        log::debug!(
            "Found {} position{} to check",
            open_positions.len(),
            if open_positions.len() == 1 { "" } else { "s" }
        );

        // Query venue for position reports
        let mut venue_positions = AHashMap::new();

        for client in clients {
            let cmd = GeneratePositionStatusReports::new(
                UUID4::new(),
                self.clock.borrow().timestamp_ns(),
                None, // instrument_id - query all
                None, // start
                None, // end
                None, // params
                None, // correlation_id
            );

            match client.generate_position_status_reports(&cmd).await {
                Ok(reports) => {
                    for report in reports {
                        venue_positions.insert(report.instrument_id, report);
                    }
                }
                Err(e) => {
                    log::error!(
                        "Failed to query position reports from {}: {e}",
                        client.client_id()
                    );
                }
            }
        }

        // Check for discrepancies
        let mut events = Vec::new();

        for position in &open_positions {
            // Skip if not in filter
            if !self.config.reconciliation_instrument_ids.is_empty()
                && !self
                    .config
                    .reconciliation_instrument_ids
                    .contains(&position.instrument_id)
            {
                continue;
            }

            let venue_report = venue_positions.get(&position.instrument_id);

            if let Some(discrepancy_events) =
                self.check_position_discrepancy(position, venue_report)
            {
                events.extend(discrepancy_events);
            }
        }

        events
    }

    /// Registers an order as inflight for tracking.
    pub fn register_inflight(&mut self, client_order_id: ClientOrderId) {
        let ts_submitted = self.clock.borrow().timestamp_ns();
        self.inflight_checks.insert(
            client_order_id,
            InflightCheck {
                client_order_id,
                ts_submitted,
                retry_count: 0,
                last_query_ts: None,
            },
        );
        self.recon_check_retries.insert(client_order_id, 0);
        self.ts_last_query.remove(&client_order_id);
        self.order_local_activity_ns.remove(&client_order_id);
    }

    /// Records local activity for the specified order.
    pub fn record_local_activity(&mut self, client_order_id: ClientOrderId, ts_event: UnixNanos) {
        self.order_local_activity_ns
            .insert(client_order_id, ts_event);
    }

    /// Clears reconciliation tracking state for an order.
    pub fn clear_recon_tracking(&mut self, client_order_id: &ClientOrderId, drop_last_query: bool) {
        self.inflight_checks.remove(client_order_id);
        self.recon_check_retries.remove(client_order_id);
        if drop_last_query {
            self.ts_last_query.remove(client_order_id);
        }
        self.order_local_activity_ns.remove(client_order_id);
    }

    /// Claims external orders for a specific strategy and instrument.
    pub fn claim_external_orders(&mut self, instrument_id: InstrumentId, strategy_id: StrategyId) {
        self.external_order_claims
            .insert(instrument_id, strategy_id);
    }

    /// Records position activity for reconciliation tracking.
    pub fn record_position_activity(&mut self, instrument_id: InstrumentId, ts_event: UnixNanos) {
        self.position_local_activity_ns
            .insert(instrument_id, ts_event);
    }

    /// Checks if a fill has been recently processed (for deduplication).
    pub fn is_fill_recently_processed(&self, trade_id: &TradeId) -> bool {
        self.recent_fills_cache.contains_key(trade_id)
    }

    /// Marks a fill as recently processed with current timestamp.
    pub fn mark_fill_processed(&mut self, trade_id: TradeId) {
        let ts_now = self.clock.borrow().timestamp_ns();
        self.recent_fills_cache.insert(trade_id, ts_now);
    }

    /// Prunes expired fills from the recent fills cache.
    ///
    /// Default TTL is 60 seconds.
    pub fn prune_recent_fills_cache(&mut self, ttl_secs: f64) {
        let ts_now = self.clock.borrow().timestamp_ns();
        let ttl_ns = (ttl_secs * 1_000_000_000.0) as u64;

        self.recent_fills_cache
            .retain(|_, &mut ts_cached| ts_now - ts_cached <= ttl_ns);
    }

    /// Purges closed orders from the cache that are older than the configured buffer.
    pub fn purge_closed_orders(&mut self) {
        let Some(buffer_mins) = self.config.purge_closed_orders_buffer_mins else {
            return;
        };

        let ts_now = self.clock.borrow().timestamp_ns();
        let buffer_secs = (buffer_mins as u64) * 60;

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
        let buffer_secs = (buffer_mins as u64) * 60;

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
        let lookback_secs = (lookback_mins as u64) * 60;

        self.cache
            .borrow_mut()
            .purge_account_events(ts_now, lookback_secs);
    }

    // Private helper methods

    fn get_order(&self, client_order_id: &ClientOrderId) -> Option<OrderAny> {
        self.cache.borrow().order(client_order_id).cloned()
    }

    fn get_instrument(&self, instrument_id: &InstrumentId) -> Option<InstrumentAny> {
        self.cache.borrow().instrument(instrument_id).cloned()
    }

    fn handle_missing_order(&mut self, client_order_id: ClientOrderId) -> Vec<OrderEventAny> {
        let mut events = Vec::new();

        let Some(order) = self.get_order(&client_order_id) else {
            return events;
        };

        let ts_now = self.clock.borrow().timestamp_ns();
        let ts_last = order.ts_last();

        // Check if order is too recent
        if (ts_now - ts_last) < self.config.open_check_threshold_ns {
            return events;
        }

        // Check local activity threshold
        if let Some(&last_activity) = self.order_local_activity_ns.get(&client_order_id)
            && (ts_now - last_activity) < self.config.open_check_threshold_ns
        {
            return events;
        }

        // Increment retry counter
        let retries = self.recon_check_retries.entry(client_order_id).or_insert(0);
        *retries += 1;

        // If max retries exceeded, generate rejection event
        if *retries >= self.config.open_check_missing_retries {
            log::warn!(
                "Order {client_order_id} not found at venue after {retries} retries, marking as REJECTED"
            );

            let rejected = self.create_order_rejected(&order, Some("NOT_FOUND_AT_VENUE"));
            events.push(rejected);

            self.clear_recon_tracking(&client_order_id, true);
        } else {
            log::debug!(
                "Order {} not found at venue, retry {}/{}",
                client_order_id,
                retries,
                self.config.open_check_missing_retries
            );
        }

        events
    }

    fn check_position_discrepancy(
        &mut self,
        position: &Position,
        venue_report: Option<&PositionStatusReport>,
    ) -> Option<Vec<OrderEventAny>> {
        let cached_qty = position.quantity.as_decimal();

        let venue_qty = if let Some(report) = venue_report {
            report.quantity.as_decimal()
        } else {
            Decimal::ZERO
        };

        // Check if quantities match (within tolerance)
        let tolerance = Decimal::from_str("0.00000001").unwrap();
        if (cached_qty - venue_qty).abs() <= tolerance {
            return None; // No discrepancy
        }

        // Check activity threshold
        let ts_now = self.clock.borrow().timestamp_ns();
        if let Some(&last_activity) = self.position_local_activity_ns.get(&position.instrument_id)
            && (ts_now - last_activity) < self.config.position_check_threshold_ns
        {
            log::debug!(
                "Skipping position reconciliation for {}: recent activity within threshold",
                position.instrument_id
            );
            return None;
        }

        log::warn!(
            "Position discrepancy detected for {}: cached_qty={}, venue_qty={}",
            position.instrument_id,
            cached_qty,
            venue_qty
        );

        // TODO: Query for missing fills to reconcile the discrepancy
        // For now, just log the discrepancy
        None
    }

    fn reconcile_order_report(
        &mut self,
        order: &mut OrderAny,
        report: &OrderStatusReport,
    ) -> Option<OrderEventAny> {
        // Check if reconciliation is needed
        if order.status() == report.order_status && order.filled_qty() == report.filled_qty {
            return None; // Already in sync
        }

        let event = match report.order_status {
            OrderStatus::Accepted => self.create_order_accepted(order, report),
            OrderStatus::Rejected => {
                self.create_order_rejected(order, report.cancel_reason.as_deref())
            }
            OrderStatus::Triggered => self.create_order_triggered(order, report),
            OrderStatus::Canceled => self.create_order_canceled(order, report),
            OrderStatus::Expired => self.create_order_expired(order, report),
            _ => return None,
        };

        Some(event)
    }

    fn handle_external_order(
        &mut self,
        report: &OrderStatusReport,
        account_id: &AccountId,
        instrument: &InstrumentAny,
    ) -> Vec<OrderEventAny> {
        let (strategy_id, tags) =
            if let Some(claimed_strategy) = self.external_order_claims.get(&report.instrument_id) {
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
                (*claimed_strategy, None)
            } else {
                // Unclaimed external orders use EXTERNAL strategy ID with VENUE tag
                (
                    StrategyId::from("EXTERNAL"),
                    Some(vec![Ustr::from("VENUE")]),
                )
            };

        let client_order_id = report
            .client_order_id
            .unwrap_or_else(|| ClientOrderId::from(report.venue_order_id.as_str()));

        let ts_now = self.clock.borrow().timestamp_ns();

        let initialized = OrderInitialized::new(
            self.config.trader_id,
            strategy_id,
            report.instrument_id,
            client_order_id,
            report.order_side,
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
            report.trigger_price,
            report.trigger_type,
            report.limit_offset,
            report.trailing_offset,
            Some(report.trailing_offset_type),
            report.expire_time,
            report.display_qty,
            None, // emulation_trigger
            None, // trigger_instrument_id
            Some(report.contingency_type),
            report.order_list_id,
            report.linked_order_ids.clone(),
            report.parent_order_id,
            None, // exec_algorithm_id
            None, // exec_algorithm_params
            None, // exec_spawn_id
            tags,
        );

        let events = vec![OrderEventAny::Initialized(initialized)];
        let order = match OrderAny::from_events(events) {
            Ok(order) => order,
            Err(e) => {
                log::error!("Failed to create order from report: {e}");
                return Vec::new();
            }
        };

        {
            let mut cache = self.cache.borrow_mut();
            if let Err(e) = cache.add_order(order.clone(), None, None, false) {
                log::error!("Failed to add external order to cache: {e}");
                return Vec::new();
            }

            if let Err(e) =
                cache.add_venue_order_id(&client_order_id, &report.venue_order_id, false)
            {
                log::warn!("Failed to add venue order ID index: {e}");
            }
        }

        log::info!(
            color = LogColor::Blue as u8;
            "Created external order {} ({}) for {} [{}]",
            client_order_id,
            report.venue_order_id,
            report.instrument_id,
            report.order_status,
        );

        let ts_now = self.clock.borrow().timestamp_ns();
        generate_external_order_status_events(&order, report, account_id, instrument, ts_now)
    }

    fn create_order_accepted(&self, order: &OrderAny, report: &OrderStatusReport) -> OrderEventAny {
        OrderEventAny::Accepted(OrderAccepted::new(
            order.trader_id(),
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            order.venue_order_id().unwrap_or(report.venue_order_id),
            order
                .account_id()
                .expect("Order should have account_id when creating accepted event"),
            UUID4::new(),
            report.ts_accepted,
            self.clock.borrow().timestamp_ns(),
            false,
        ))
    }

    fn create_order_rejected(&self, order: &OrderAny, reason: Option<&str>) -> OrderEventAny {
        let reason = reason.unwrap_or("UNKNOWN");
        OrderEventAny::Rejected(OrderRejected::new(
            order.trader_id(),
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            order
                .account_id()
                .expect("Order should have account_id when creating rejected event"),
            Ustr::from(reason),
            UUID4::new(),
            self.clock.borrow().timestamp_ns(),
            self.clock.borrow().timestamp_ns(),
            false,
            false, // due_post_only
        ))
    }

    fn create_order_triggered(
        &self,
        order: &OrderAny,
        report: &OrderStatusReport,
    ) -> OrderEventAny {
        OrderEventAny::Triggered(OrderTriggered::new(
            order.trader_id(),
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            UUID4::new(),
            report
                .ts_triggered
                .unwrap_or(self.clock.borrow().timestamp_ns()),
            self.clock.borrow().timestamp_ns(),
            false,
            order.venue_order_id(),
            order.account_id(),
        ))
    }

    fn create_order_canceled(&self, order: &OrderAny, report: &OrderStatusReport) -> OrderEventAny {
        OrderEventAny::Canceled(OrderCanceled::new(
            order.trader_id(),
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            UUID4::new(),
            report.ts_last,
            self.clock.borrow().timestamp_ns(),
            false,
            order.venue_order_id(),
            order.account_id(),
        ))
    }

    fn create_order_expired(&self, order: &OrderAny, report: &OrderStatusReport) -> OrderEventAny {
        OrderEventAny::Expired(OrderExpired::new(
            order.trader_id(),
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            UUID4::new(),
            report.ts_last,
            self.clock.borrow().timestamp_ns(),
            false,
            order.venue_order_id(),
            order.account_id(),
        ))
    }

    fn create_order_fill(
        &mut self,
        order: &mut OrderAny,
        fill: &FillReport,
        instrument: &InstrumentAny,
    ) -> Option<OrderEventAny> {
        if self.processed_fills.contains_key(&fill.trade_id) {
            return None;
        }

        self.processed_fills
            .insert(fill.trade_id, order.client_order_id());

        Some(OrderEventAny::Filled(OrderFilled::new(
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
            self.clock.borrow().timestamp_ns(),
            false,
            fill.venue_position_id,
            Some(fill.commission),
        )))
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use nautilus_common::{cache::Cache, clock::TestClock};
    use nautilus_core::{UUID4, UnixNanos};
    use nautilus_model::{
        enums::{LiquiditySide, OrderSide, OrderStatus, OrderType, TimeInForce},
        events::OrderEventAny,
        identifiers::{
            AccountId, ClientId, ClientOrderId, InstrumentId, TradeId, Venue, VenueOrderId,
        },
        instruments::{
            InstrumentAny,
            stubs::{audusd_sim, crypto_perpetual_ethusdt},
        },
        orders::{Order, OrderTestBuilder},
        reports::{ExecutionMassStatus, FillReport, OrderStatusReport},
        types::{Money, Price, Quantity},
    };
    use rstest::rstest;

    use super::*;

    fn create_test_manager() -> ExecutionManager {
        let clock = Rc::new(RefCell::new(TestClock::new()));
        let cache = Rc::new(RefCell::new(Cache::default()));
        let config = ExecutionManagerConfig::default();
        ExecutionManager::new(clock, cache, config)
    }

    #[rstest]
    fn test_reconciliation_manager_new() {
        let manager = create_test_manager();
        assert_eq!(manager.inflight_checks.len(), 0);
        assert_eq!(manager.external_order_claims.len(), 0);
        assert_eq!(manager.processed_fills.len(), 0);
    }

    #[rstest]
    fn test_register_inflight() {
        let mut manager = create_test_manager();
        let client_order_id = ClientOrderId::from("O-123456");

        manager.register_inflight(client_order_id);

        assert_eq!(manager.inflight_checks.len(), 1);
        assert!(manager.inflight_checks.contains_key(&client_order_id));
    }

    #[rstest]
    fn test_claim_external_orders() {
        let mut manager = create_test_manager();
        let instrument_id = InstrumentId::from("BTCUSDT.BINANCE");
        let strategy_id = StrategyId::from("STRATEGY-001");

        manager.claim_external_orders(instrument_id, strategy_id);

        assert_eq!(manager.external_order_claims.len(), 1);
        assert_eq!(
            manager.external_order_claims.get(&instrument_id),
            Some(&strategy_id)
        );
    }

    #[rstest]
    fn test_reconcile_report_removes_from_inflight() {
        let mut manager = create_test_manager();
        let client_order_id = ClientOrderId::from("O-123456");

        manager.register_inflight(client_order_id);
        assert_eq!(manager.inflight_checks.len(), 1);

        let report = ExecutionReport {
            client_order_id,
            venue_order_id: Some(VenueOrderId::from("V-123456")),
            status: OrderStatus::Accepted,
            filled_qty: Quantity::from(0),
            avg_px: None,
            ts_event: UnixNanos::default(),
        };

        // Reconcile should remove from inflight checks
        manager.reconcile_report(report).unwrap();
        assert_eq!(manager.inflight_checks.len(), 0);
    }

    #[rstest]
    fn test_check_inflight_orders_generates_rejection_after_max_retries() {
        let clock = Rc::new(RefCell::new(TestClock::new()));
        let cache = Rc::new(RefCell::new(Cache::default()));
        let config = ExecutionManagerConfig {
            inflight_threshold_ms: 100,
            inflight_max_retries: 2,
            ..ExecutionManagerConfig::default()
        };
        let mut manager = ExecutionManager::new(clock.clone(), cache, config);

        let client_order_id = ClientOrderId::from("O-123456");
        manager.register_inflight(client_order_id);

        // First check - should increment retry count
        clock
            .borrow_mut()
            .advance_time(UnixNanos::from(200_000_000), true);
        let events = manager.check_inflight_orders();
        assert_eq!(events.len(), 0);
        let first_check = manager
            .inflight_checks
            .get(&client_order_id)
            .expect("inflight check present");
        assert_eq!(first_check.retry_count, 1);
        let first_query_ts = first_check.last_query_ts.expect("last query recorded");

        // Second check - should hit max retries and generate rejection
        clock
            .borrow_mut()
            .advance_time(UnixNanos::from(400_000_000), true);
        let events = manager.check_inflight_orders();
        assert_eq!(events.len(), 0); // Would generate rejection if order existed in cache
        assert!(!manager.inflight_checks.contains_key(&client_order_id));
        // Ensure last query timestamp progressed prior to removal
        assert!(clock.borrow().timestamp_ns() > first_query_ts);
    }

    #[rstest]
    fn test_check_inflight_orders_skips_recent_query() {
        let clock = Rc::new(RefCell::new(TestClock::new()));
        let cache = Rc::new(RefCell::new(Cache::default()));
        let config = ExecutionManagerConfig {
            inflight_threshold_ms: 100,
            inflight_max_retries: 3,
            ..ExecutionManagerConfig::default()
        };
        let mut manager = ExecutionManager::new(clock.clone(), cache, config);

        let client_order_id = ClientOrderId::from("O-ABCDEF");
        manager.register_inflight(client_order_id);

        // First pass triggers a venue query and records timestamp
        clock
            .borrow_mut()
            .advance_time(UnixNanos::from(200_000_000), true);
        let events = manager.check_inflight_orders();
        assert!(events.is_empty());
        let initial_check = manager
            .inflight_checks
            .get(&client_order_id)
            .expect("inflight check retained");
        assert_eq!(initial_check.retry_count, 1);
        let last_query_ts = initial_check.last_query_ts.expect("last query recorded");

        // Subsequent pass within threshold should be skipped entirely
        clock
            .borrow_mut()
            .advance_time(UnixNanos::from(250_000_000), true);
        let events = manager.check_inflight_orders();
        assert!(events.is_empty());
        let second_check = manager
            .inflight_checks
            .get(&client_order_id)
            .expect("inflight check retained");
        assert_eq!(second_check.retry_count, 1);
        assert_eq!(second_check.last_query_ts, Some(last_query_ts));
    }

    #[rstest]
    fn test_check_inflight_orders_skips_filtered_ids() {
        let clock = Rc::new(RefCell::new(TestClock::new()));
        let cache = Rc::new(RefCell::new(Cache::default()));
        let filtered_id = ClientOrderId::from("O-FILTERED");
        let mut config = ExecutionManagerConfig::default();
        config.filtered_client_order_ids.insert(filtered_id);
        config.inflight_threshold_ms = 100;
        let mut manager = ExecutionManager::new(clock.clone(), cache, config);

        manager.register_inflight(filtered_id);
        clock
            .borrow_mut()
            .advance_time(UnixNanos::from(200_000_000), true);
        let events = manager.check_inflight_orders();
        assert!(events.is_empty());
        assert!(manager.inflight_checks.contains_key(&filtered_id));
    }

    #[rstest]
    fn test_record_and_clear_tracking() {
        let mut manager = create_test_manager();
        let client_order_id = ClientOrderId::from("O-TRACK");

        manager.register_inflight(client_order_id);
        let ts_now = UnixNanos::from(1_000_000);
        manager.record_local_activity(client_order_id, ts_now);

        assert_eq!(
            manager
                .order_local_activity_ns
                .get(&client_order_id)
                .copied(),
            Some(ts_now)
        );

        manager.clear_recon_tracking(&client_order_id, true);
        assert!(!manager.inflight_checks.contains_key(&client_order_id));
        assert!(
            !manager
                .order_local_activity_ns
                .contains_key(&client_order_id)
        );
        assert!(!manager.recon_check_retries.contains_key(&client_order_id));
        assert!(!manager.ts_last_query.contains_key(&client_order_id));
    }

    #[tokio::test]
    async fn test_reconcile_execution_mass_status_with_empty() {
        let mut manager = create_test_manager();
        let account_id = AccountId::from("ACCOUNT-001");
        let venue = Venue::from("BINANCE");

        let client_id = ClientId::from("BINANCE");
        let mass_status = ExecutionMassStatus::new(
            client_id,
            account_id,
            venue,
            UnixNanos::default(),
            Some(UUID4::new()),
        );

        let events = manager.reconcile_execution_mass_status(mass_status).await;
        assert_eq!(events.len(), 0);
    }

    #[rstest]
    fn test_reconciliation_config_default() {
        let config = ExecutionManagerConfig::default();

        assert_eq!(config.lookback_mins, Some(60));
        assert_eq!(config.inflight_threshold_ms, 5000);
        assert_eq!(config.inflight_max_retries, 5);
        assert!(!config.filter_unclaimed_external);
        assert!(config.generate_missing_orders);
    }

    #[rstest]
    fn test_create_order_fill_deduplicates_by_trade_id() {
        let mut manager = create_test_manager();
        let instrument = audusd_sim();
        let mut order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(instrument.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from(100_000))
            .build();
        let trade_id = TradeId::from("T-001");
        let fill = FillReport::new(
            AccountId::from("SIM-001"),
            instrument.id(),
            VenueOrderId::from("V-001"),
            trade_id,
            OrderSide::Buy,
            Quantity::from(100_000),
            Price::from("1.00000"),
            Money::from("1.00 USD"),
            LiquiditySide::Maker,
            Some(ClientOrderId::from("O-123456")),
            None,
            UnixNanos::from(1_000_000_000),
            UnixNanos::from(1_000_000_000),
            None,
        );
        let event1 = manager.create_order_fill(&mut order, &fill, &InstrumentAny::from(instrument));
        assert!(event1.is_some());

        // Same trade_id should be skipped
        let event2 = manager.create_order_fill(&mut order, &fill, &InstrumentAny::from(instrument));
        assert!(event2.is_none());
    }

    #[rstest]
    fn test_handle_external_order_uses_claimed_strategy() {
        let mut manager = create_test_manager();
        let instrument = crypto_perpetual_ethusdt();
        let instrument_id = instrument.id();
        let instrument_any = InstrumentAny::CryptoPerpetual(instrument);
        let strategy_id = StrategyId::from("STRATEGY-001");
        let account_id = AccountId::from("OKX-001");
        let venue_order_id = VenueOrderId::from("V-EXT-001");
        manager.claim_external_orders(instrument_id, strategy_id);
        let report = OrderStatusReport::new(
            account_id,
            instrument_id,
            None, // No client_order_id (external)
            venue_order_id,
            OrderSide::Buy,
            OrderType::Limit,
            TimeInForce::Gtc,
            OrderStatus::Accepted,
            Quantity::from(1),
            Quantity::from(0),
            UnixNanos::from(1_000_000),
            UnixNanos::from(1_000_000),
            UnixNanos::from(1_000_000),
            None,
        )
        .with_price(Price::from("3000.00"));
        let events = manager.handle_external_order(&report, &account_id, &instrument_any);

        // Initialized consumed internally, only Accepted returned
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], OrderEventAny::Accepted(_)));
        let client_order_id = ClientOrderId::from(venue_order_id.as_str());
        let order = manager.cache.borrow().order(&client_order_id).cloned();
        assert!(order.is_some());
        assert_eq!(order.unwrap().strategy_id(), strategy_id);
    }

    #[rstest]
    fn test_handle_external_order_uses_external_strategy_when_unclaimed() {
        let mut manager = create_test_manager();
        let instrument = crypto_perpetual_ethusdt();
        let instrument_id = instrument.id();
        let instrument_any = InstrumentAny::CryptoPerpetual(instrument);
        let account_id = AccountId::from("OKX-001");
        let venue_order_id = VenueOrderId::from("V-EXT-002");
        let report = OrderStatusReport::new(
            account_id,
            instrument_id,
            None, // No client_order_id (external)
            venue_order_id,
            OrderSide::Sell,
            OrderType::Limit,
            TimeInForce::Gtc,
            OrderStatus::Accepted,
            Quantity::from(1),
            Quantity::from(0),
            UnixNanos::from(1_000_000),
            UnixNanos::from(1_000_000),
            UnixNanos::from(1_000_000),
            None,
        )
        .with_price(Price::from("3000.00"));
        let events = manager.handle_external_order(&report, &account_id, &instrument_any);
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], OrderEventAny::Accepted(_)));
        let client_order_id = ClientOrderId::from(venue_order_id.as_str());
        let order = manager.cache.borrow().order(&client_order_id).cloned();
        assert!(order.is_some());
        let order = order.unwrap();
        assert_eq!(order.strategy_id(), StrategyId::from("EXTERNAL"));
        assert!(
            order
                .tags()
                .is_some_and(|t| t.iter().any(|s| s.as_str() == "VENUE"))
        );
    }

    #[rstest]
    fn test_external_order_canceled_generates_accepted_and_canceled() {
        let mut manager = create_test_manager();
        let instrument = crypto_perpetual_ethusdt();
        let instrument_id = instrument.id();
        let instrument_any = InstrumentAny::CryptoPerpetual(instrument);
        let account_id = AccountId::from("OKX-001");
        let report = OrderStatusReport::new(
            account_id,
            instrument_id,
            None, // No client_order_id (external)
            VenueOrderId::from("V-EXT-003"),
            OrderSide::Buy,
            OrderType::Limit,
            TimeInForce::Gtc,
            OrderStatus::Canceled,
            Quantity::from(1),
            Quantity::from(0),
            UnixNanos::from(1_000_000),
            UnixNanos::from(1_000_000),
            UnixNanos::from(1_000_000),
            None,
        )
        .with_price(Price::from("3000.00"));
        let events = manager.handle_external_order(&report, &account_id, &instrument_any);
        assert_eq!(events.len(), 2);
        assert!(matches!(events[0], OrderEventAny::Accepted(_)));
        assert!(matches!(events[1], OrderEventAny::Canceled(_)));
    }

    #[rstest]
    fn test_external_order_expired_generates_accepted_and_expired() {
        let mut manager = create_test_manager();
        let instrument = crypto_perpetual_ethusdt();
        let instrument_id = instrument.id();
        let instrument_any = InstrumentAny::CryptoPerpetual(instrument);
        let account_id = AccountId::from("OKX-001");
        let report = OrderStatusReport::new(
            account_id,
            instrument_id,
            None, // No client_order_id (external)
            VenueOrderId::from("V-EXT-004"),
            OrderSide::Buy,
            OrderType::Limit,
            TimeInForce::Gtc,
            OrderStatus::Expired,
            Quantity::from(1),
            Quantity::from(0),
            UnixNanos::from(1_000_000),
            UnixNanos::from(1_000_000),
            UnixNanos::from(1_000_000),
            None,
        )
        .with_price(Price::from("3000.00"));
        let events = manager.handle_external_order(&report, &account_id, &instrument_any);
        assert_eq!(events.len(), 2);
        assert!(matches!(events[0], OrderEventAny::Accepted(_)));
        assert!(matches!(events[1], OrderEventAny::Expired(_)));
    }

    #[rstest]
    fn test_external_order_filled_generates_accepted_and_filled() {
        let mut manager = create_test_manager();
        let instrument = crypto_perpetual_ethusdt();
        let instrument_id = instrument.id();
        let instrument_any = InstrumentAny::CryptoPerpetual(instrument);
        let account_id = AccountId::from("OKX-001");
        let report = OrderStatusReport::new(
            account_id,
            instrument_id,
            None, // No client_order_id (external)
            VenueOrderId::from("V-EXT-005"),
            OrderSide::Buy,
            OrderType::Limit,
            TimeInForce::Gtc,
            OrderStatus::Filled,
            Quantity::from("1.5"),
            Quantity::from("1.5"), // filled_qty
            UnixNanos::from(1_000_000),
            UnixNanos::from(1_000_000),
            UnixNanos::from(1_000_000),
            None,
        )
        .with_price(Price::from("3000.00"))
        .with_avg_px(3000.50)
        .unwrap();
        let events = manager.handle_external_order(&report, &account_id, &instrument_any);

        assert_eq!(events.len(), 2);
        assert!(matches!(events[0], OrderEventAny::Accepted(_)));
        assert!(matches!(events[1], OrderEventAny::Filled(_)));
        if let OrderEventAny::Filled(filled) = &events[1] {
            assert_eq!(filled.last_qty, Quantity::from("1.5"));
            assert!(!filled.trade_id.as_str().is_empty()); // Synthetic UUID trade ID
            assert!(filled.reconciliation);
        }
    }

    #[rstest]
    fn test_external_order_partially_filled_generates_accepted_and_filled() {
        let mut manager = create_test_manager();
        let instrument = crypto_perpetual_ethusdt();
        let instrument_id = instrument.id();
        let instrument_any = InstrumentAny::CryptoPerpetual(instrument);
        let account_id = AccountId::from("OKX-001");
        let report = OrderStatusReport::new(
            account_id,
            instrument_id,
            None, // No client_order_id (external)
            VenueOrderId::from("V-EXT-006"),
            OrderSide::Sell,
            OrderType::Limit,
            TimeInForce::Gtc,
            OrderStatus::PartiallyFilled,
            Quantity::from("2.0"),  // total quantity
            Quantity::from("0.75"), // filled_qty (partial)
            UnixNanos::from(1_000_000),
            UnixNanos::from(1_000_000),
            UnixNanos::from(1_000_000),
            None,
        )
        .with_price(Price::from("2950.00"))
        .with_avg_px(2950.25)
        .unwrap();
        let events = manager.handle_external_order(&report, &account_id, &instrument_any);

        assert_eq!(events.len(), 2);
        assert!(matches!(events[0], OrderEventAny::Accepted(_)));
        assert!(matches!(events[1], OrderEventAny::Filled(_)));
        if let OrderEventAny::Filled(filled) = &events[1] {
            assert_eq!(filled.last_qty, Quantity::from("0.75"));
            assert!(filled.reconciliation);
        }
    }

    #[rstest]
    fn test_external_order_filled_market_order_is_taker() {
        let mut manager = create_test_manager();
        let instrument = crypto_perpetual_ethusdt();
        let instrument_id = instrument.id();
        let instrument_any = InstrumentAny::CryptoPerpetual(instrument);
        let account_id = AccountId::from("OKX-001");
        let report = OrderStatusReport::new(
            account_id,
            instrument_id,
            None,
            VenueOrderId::from("V-EXT-007"),
            OrderSide::Buy,
            OrderType::Market,
            TimeInForce::Ioc,
            OrderStatus::Filled,
            Quantity::from("1.0"),
            Quantity::from("1.0"),
            UnixNanos::from(1_000_000),
            UnixNanos::from(1_000_000),
            UnixNanos::from(1_000_000),
            None,
        )
        .with_avg_px(3100.00)
        .unwrap();
        let events = manager.handle_external_order(&report, &account_id, &instrument_any);

        assert_eq!(events.len(), 2);
        if let OrderEventAny::Filled(filled) = &events[1] {
            assert_eq!(filled.liquidity_side, LiquiditySide::Taker);
        }
    }

    #[rstest]
    fn test_external_order_rejected_generates_rejected_only() {
        let mut manager = create_test_manager();
        let instrument = crypto_perpetual_ethusdt();
        let instrument_id = instrument.id();
        let instrument_any = InstrumentAny::CryptoPerpetual(instrument);
        let account_id = AccountId::from("OKX-001");
        let report = OrderStatusReport::new(
            account_id,
            instrument_id,
            None,
            VenueOrderId::from("V-EXT-008"),
            OrderSide::Buy,
            OrderType::Limit,
            TimeInForce::Gtc,
            OrderStatus::Rejected,
            Quantity::from("1.0"),
            Quantity::from("0"),
            UnixNanos::from(1_000_000),
            UnixNanos::from(1_000_000),
            UnixNanos::from(1_000_000),
            None,
        )
        .with_price(Price::from("3000.00"))
        .with_cancel_reason("INSUFFICIENT_MARGIN".to_string());
        let events = manager.handle_external_order(&report, &account_id, &instrument_any);

        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], OrderEventAny::Rejected(_)));
    }
}
