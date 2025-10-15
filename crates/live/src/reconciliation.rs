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

//! Reconciliation managers for live execution state.
//!
//! This module provides managers for reconciling execution state between
//! the local cache and connected venues during live trading.

use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    fmt::Debug,
    rc::Rc,
};

use nautilus_common::{cache::Cache, clock::Clock};
use nautilus_core::{UUID4, UnixNanos};
use nautilus_model::{
    enums::OrderStatus,
    events::{
        OrderAccepted, OrderCanceled, OrderEventAny, OrderExpired, OrderFilled, OrderRejected,
        OrderTriggered,
    },
    identifiers::{AccountId, ClientOrderId, InstrumentId, StrategyId, TradeId, VenueOrderId},
    instruments::{Instrument, InstrumentAny},
    orders::{Order, OrderAny},
    reports::{ExecutionMassStatus, FillReport, OrderStatusReport},
    types::Quantity,
};
use ustr::Ustr;

/// Configuration for reconciliation manager.
#[derive(Debug, Clone)]
pub struct ReconciliationConfig {
    /// Number of minutes to look back during reconciliation.
    pub lookback_mins: Option<u64>,
    /// Threshold in milliseconds for inflight order checks.
    pub inflight_threshold_ms: u64,
    /// Maximum number of retries for inflight checks.
    pub inflight_max_retries: u32,
    /// Whether to filter unclaimed external orders.
    pub filter_unclaimed_external: bool,
    /// Whether to generate missing orders from reports.
    pub generate_missing_orders: bool,
    /// Client order IDs excluded from reconciliation.
    pub filtered_client_order_ids: HashSet<ClientOrderId>,
    /// Threshold in nanoseconds before acting on venue discrepancies for open orders.
    pub open_check_threshold_ns: u64,
    /// Maximum retries before resolving an open order missing at the venue.
    pub open_check_missing_retries: u32,
    /// Whether open-order polling should only request open orders from the venue.
    pub open_check_open_only: bool,
    /// Lookback window (minutes) for venue order status polling.
    pub open_check_lookback_mins: Option<u64>,
    /// Whether to filter position status reports during reconciliation.
    pub filter_position_reports: bool,
    /// Instrument IDs to include during reconciliation (empty => all).
    pub reconciliation_instrument_ids: HashSet<InstrumentId>,
}

impl Default for ReconciliationConfig {
    fn default() -> Self {
        Self {
            lookback_mins: Some(60),
            inflight_threshold_ms: 5000,
            inflight_max_retries: 5,
            filter_unclaimed_external: false,
            generate_missing_orders: true,
            filtered_client_order_ids: HashSet::new(),
            open_check_threshold_ns: 5_000_000_000,
            open_check_missing_retries: 5,
            open_check_open_only: true,
            open_check_lookback_mins: Some(60),
            filter_position_reports: false,
            reconciliation_instrument_ids: HashSet::new(),
        }
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

/// Manager for reconciling execution state between local cache and venues.
///
/// The `ReconciliationManager` handles:
/// - Startup reconciliation to align state on system start
/// - Continuous reconciliation of inflight orders
/// - External order discovery and claiming
/// - Fill report processing and validation
#[derive(Clone)]
pub struct ReconciliationManager {
    clock: Rc<RefCell<dyn Clock>>,
    cache: Rc<RefCell<Cache>>,
    config: ReconciliationConfig,
    inflight_checks: HashMap<ClientOrderId, InflightCheck>,
    external_order_claims: HashMap<InstrumentId, StrategyId>,
    processed_fills: HashMap<TradeId, ClientOrderId>,
    recon_check_retries: HashMap<ClientOrderId, u32>,
    ts_last_query: HashMap<ClientOrderId, UnixNanos>,
    order_local_activity_ns: HashMap<ClientOrderId, UnixNanos>,
}

impl Debug for ReconciliationManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(ReconciliationManager))
            .field("config", &self.config)
            .field("inflight_checks", &self.inflight_checks)
            .field("external_order_claims", &self.external_order_claims)
            .field("processed_fills", &self.processed_fills)
            .field("recon_check_retries", &self.recon_check_retries)
            .finish()
    }
}

impl ReconciliationManager {
    /// Creates a new [`ReconciliationManager`] instance.
    pub fn new(
        clock: Rc<RefCell<dyn Clock>>,
        cache: Rc<RefCell<Cache>>,
        config: ReconciliationConfig,
    ) -> Self {
        Self {
            clock,
            cache,
            config,
            inflight_checks: HashMap::new(),
            external_order_claims: HashMap::new(),
            processed_fills: HashMap::new(),
            recon_check_retries: HashMap::new(),
            ts_last_query: HashMap::new(),
            order_local_activity_ns: HashMap::new(),
        }
    }

    /// Reconciles orders and fills from a mass status report.
    pub async fn reconcile_execution_mass_status(
        &mut self,
        mass_status: ExecutionMassStatus,
    ) -> Vec<OrderEventAny> {
        let mut events = Vec::new();

        // Process order status reports first
        for report in mass_status.order_reports().values() {
            if let Some(client_order_id) = &report.client_order_id {
                if let Some(order) = self.get_order(client_order_id) {
                    let mut order = order;
                    if let Some(event) = self.reconcile_order_report(&mut order, report) {
                        events.push(event);
                    }
                }
            } else if !self.config.filter_unclaimed_external
                && let Some(event) = self.handle_external_order(report, &mass_status.account_id)
            {
                events.push(event);
            }
        }

        // Process fill reports
        for fills in mass_status.fill_reports().values() {
            for fill in fills {
                if let Some(client_order_id) = &fill.client_order_id
                    && let Some(order) = self.get_order(client_order_id)
                {
                    let mut order = order;
                    // Get instrument for the order
                    let instrument_id = order.instrument_id();
                    if let Some(instrument) = self.get_instrument(&instrument_id)
                        && let Some(event) = self.create_order_fill(&mut order, fill, &instrument)
                    {
                        events.push(event);
                    }
                }
            }
        }

        events
    }

    /// Reconciles a single execution report during runtime.
    pub fn reconcile_report(&mut self, report: ExecutionReport) -> Vec<OrderEventAny> {
        let mut events = Vec::new();

        // Remove from inflight checks if present
        self.clear_recon_tracking(&report.client_order_id, true);

        if let Some(order) = self.get_order(&report.client_order_id) {
            let mut order = order;
            // Create an OrderStatusReport from the ExecutionReport
            let order_report = OrderStatusReport::new(
                order.account_id().unwrap_or_default(),
                order.instrument_id(),
                Some(report.client_order_id),
                report.venue_order_id.unwrap_or_default(),
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
            )
            .with_avg_px(report.avg_px.unwrap_or(0.0));

            if let Some(event) = self.reconcile_order_report(&mut order, &order_report) {
                events.push(event);
            }
        }

        events
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

    /// Checks open orders against the venue state.
    pub async fn check_open_orders(&mut self) -> Vec<OrderEventAny> {
        // This would need to query the venue for open orders
        // and reconcile any discrepancies
        Vec::new()
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

    // Private helper methods

    fn get_order(&self, client_order_id: &ClientOrderId) -> Option<OrderAny> {
        self.cache.borrow().order(client_order_id).cloned()
    }

    fn get_instrument(&self, instrument_id: &InstrumentId) -> Option<InstrumentAny> {
        self.cache.borrow().instrument(instrument_id).cloned()
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

        // Generate appropriate event based on status
        match report.order_status {
            OrderStatus::Accepted => Some(self.create_order_accepted(order, report)),
            OrderStatus::Rejected => {
                Some(self.create_order_rejected(order, report.cancel_reason.as_deref()))
            }
            OrderStatus::Triggered => Some(self.create_order_triggered(order, report)),
            OrderStatus::Canceled => Some(self.create_order_canceled(order, report)),
            OrderStatus::Expired => Some(self.create_order_expired(order, report)),
            _ => None,
        }
    }

    fn handle_external_order(
        &self,
        _report: &OrderStatusReport,
        _account_id: &AccountId,
    ) -> Option<OrderEventAny> {
        // This would need to create a new order from the report
        // For now, we'll skip external order handling
        None
    }

    fn create_order_accepted(&self, order: &OrderAny, report: &OrderStatusReport) -> OrderEventAny {
        OrderEventAny::Accepted(OrderAccepted::new(
            order.trader_id(),
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            order.venue_order_id().unwrap_or(report.venue_order_id),
            order.account_id().unwrap_or_default(),
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
            order.account_id().unwrap_or_default(),
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

    #[allow(dead_code)]
    fn create_order_canceled_simple(&self, order: &OrderAny, ts_event: UnixNanos) -> OrderEventAny {
        OrderEventAny::Canceled(OrderCanceled::new(
            order.trader_id(),
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            UUID4::new(),
            ts_event,
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
        // Check if this fill was already processed
        if self.processed_fills.contains_key(&fill.trade_id) {
            return None;
        }

        // Mark this fill as processed
        self.processed_fills
            .insert(fill.trade_id, order.client_order_id());

        Some(OrderEventAny::Filled(OrderFilled::new(
            order.trader_id(),
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            fill.venue_order_id,
            order.account_id().unwrap_or_default(),
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

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use nautilus_common::{cache::Cache, clock::TestClock};
    use nautilus_core::{UUID4, UnixNanos};
    use nautilus_model::{
        enums::OrderStatus,
        identifiers::{AccountId, ClientId, ClientOrderId, VenueOrderId},
        reports::ExecutionMassStatus,
        types::Quantity,
    };
    use rstest::rstest;

    use super::*;

    fn create_test_manager() -> ReconciliationManager {
        let clock = Rc::new(RefCell::new(TestClock::new()));
        let cache = Rc::new(RefCell::new(Cache::default()));
        let config = ReconciliationConfig::default();
        ReconciliationManager::new(clock, cache, config)
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

        // Register as inflight
        manager.register_inflight(client_order_id);
        assert_eq!(manager.inflight_checks.len(), 1);

        // Create execution report
        let report = ExecutionReport {
            client_order_id,
            venue_order_id: Some(VenueOrderId::from("V-123456")),
            status: OrderStatus::Accepted,
            filled_qty: Quantity::from(0),
            avg_px: None,
            ts_event: UnixNanos::default(),
        };

        // Reconcile should remove from inflight checks
        manager.reconcile_report(report);
        assert_eq!(manager.inflight_checks.len(), 0);
    }

    #[rstest]
    fn test_check_inflight_orders_generates_rejection_after_max_retries() {
        let clock = Rc::new(RefCell::new(TestClock::new()));
        let cache = Rc::new(RefCell::new(Cache::default()));
        let config = ReconciliationConfig {
            inflight_threshold_ms: 100,
            inflight_max_retries: 2,
            ..ReconciliationConfig::default()
        };
        let mut manager = ReconciliationManager::new(clock.clone(), cache, config);

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
        let config = ReconciliationConfig {
            inflight_threshold_ms: 100,
            inflight_max_retries: 3,
            ..ReconciliationConfig::default()
        };
        let mut manager = ReconciliationManager::new(clock.clone(), cache, config);

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
        let mut config = ReconciliationConfig::default();
        config.filtered_client_order_ids.insert(filtered_id);
        config.inflight_threshold_ms = 100;
        let mut manager = ReconciliationManager::new(clock.clone(), cache, config);

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
        let venue = nautilus_model::identifiers::Venue::from("BINANCE");

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
        let config = ReconciliationConfig::default();

        assert_eq!(config.lookback_mins, Some(60));
        assert_eq!(config.inflight_threshold_ms, 5000);
        assert_eq!(config.inflight_max_retries, 5);
        assert!(!config.filter_unclaimed_external);
        assert!(config.generate_missing_orders);
    }
}
