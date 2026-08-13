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

//! Execution client trait definition.

use anyhow::Context;
use async_trait::async_trait;
use nautilus_core::{UnixNanos, datetime::checked_mins_to_nanos, time::get_atomic_clock_realtime};
use nautilus_model::{
    accounts::AccountAny,
    enums::{LiquiditySide, OmsType},
    identifiers::{
        AccountId, ClientId, ClientOrderId, InstrumentId, StrategyId, Venue, VenueOrderId,
    },
    instruments::InstrumentAny,
    reports::{ExecutionMassStatus, FillReport, OrderStatusReport, PositionStatusReport},
    types::{AccountBalance, MarginBalance, Money, Price, Quantity},
};
use rust_decimal::Decimal;

use super::{SocketReconnectRegistry, log_not_implemented};
use crate::messages::execution::{
    BatchCancelOrders, BatchModifyOrders, CancelAllOrders, CancelOrder, GenerateFillReports,
    GenerateFillReportsBuilder, GenerateOrderStatusReport, GenerateOrderStatusReports,
    GenerateOrderStatusReportsBuilder, GeneratePositionStatusReports,
    GeneratePositionStatusReportsBuilder, ModifyOrder, QueryAccount, QueryOrder, SubmitOrder,
    SubmitOrderList,
};

/// Default maximum absolute position difference tolerated during reconciliation.
pub const DEFAULT_POSITION_RECONCILIATION_TOLERANCE: Decimal =
    Decimal::from_parts(1, 0, 0, false, 8);

/// Defines the interface for an execution client managing order operations.
///
/// # Thread Safety
///
/// Client instances are not intended to be sent across threads. The `?Send` bound
/// allows implementations to hold non-Send state for any Python interop.
#[async_trait(?Send)]
pub trait ExecutionClient {
    fn is_connected(&self) -> bool;
    fn client_id(&self) -> ClientId;
    fn account_id(&self) -> AccountId;
    fn venue(&self) -> Venue;
    fn oms_type(&self) -> OmsType;
    fn get_account(&self) -> Option<AccountAny>;

    /// Returns endpoint-level socket reconnect controls exposed by this client.
    fn socket_reconnect_registry(&self) -> Option<&SocketReconnectRegistry> {
        None
    }

    /// Returns the maximum absolute position difference tolerated during reconciliation.
    fn position_reconciliation_tolerance(&self) -> Decimal {
        DEFAULT_POSITION_RECONCILIATION_TOLERANCE
    }

    /// Returns whether this client can execute orders for the given instrument venue.
    ///
    /// Single-venue clients should use the default behavior. Routing brokers can
    /// override this when their client venue identifies the broker rather than
    /// the instrument's exchange venue.
    fn handles_order_venue(&self, venue: Venue) -> bool {
        self.venue() == venue
    }

    /// Generates and publishes the account state event.
    ///
    /// Implementations may publish synchronously. Callers must release shared state borrows,
    /// including clock and cache borrows, before calling this method because subscribers may
    /// access the same state.
    ///
    /// # Errors
    ///
    /// Returns an error if generating the account state fails.
    fn generate_account_state(
        &self,
        balances: Vec<AccountBalance>,
        margins: Vec<MarginBalance>,
        reported: bool,
        ts_event: UnixNanos,
    ) -> anyhow::Result<()>;

    /// Starts the execution client.
    ///
    /// # Errors
    ///
    /// Returns an error if the client fails to start.
    fn start(&mut self) -> anyhow::Result<()>;

    /// Stops the execution client.
    ///
    /// Implementations must be idempotent: the engine and node teardown paths
    /// (e.g. backtest `end` -> `reset` -> `dispose`) may call `stop()` more
    /// than once per run. Guard with an internal `is_stopped` check or
    /// equivalent so repeated calls are safe.
    ///
    /// # Errors
    ///
    /// Returns an error if the client fails to stop.
    fn stop(&mut self) -> anyhow::Result<()>;

    /// Resets the execution client to its initial state.
    ///
    /// The default implementation is a no-op. Adapters with reconnectable state
    /// (caches, sequence counters, in-flight orders) should override this.
    ///
    /// # Errors
    ///
    /// Returns an error if the client fails to reset.
    fn reset(&mut self) -> anyhow::Result<()> {
        Ok(())
    }

    /// Disposes of client resources and cleans up.
    ///
    /// The default implementation is a no-op. Adapters that hold async tasks,
    /// background threads, or external handles should override this.
    ///
    /// # Errors
    ///
    /// Returns an error if the client fails to dispose.
    fn dispose(&mut self) -> anyhow::Result<()> {
        Ok(())
    }

    /// Connects the client to the execution venue.
    ///
    /// # Errors
    ///
    /// Returns an error if connection fails.
    async fn connect(&mut self) -> anyhow::Result<()> {
        Ok(())
    }

    /// Disconnects the client from the execution venue.
    ///
    /// # Errors
    ///
    /// Returns an error if disconnection fails.
    async fn disconnect(&mut self) -> anyhow::Result<()> {
        Ok(())
    }

    /// Submits a single order command to the execution venue.
    ///
    /// # Errors
    ///
    /// Returns an error if submission fails.
    fn submit_order(&self, cmd: SubmitOrder) -> anyhow::Result<()> {
        log_not_implemented(&cmd);
        Ok(())
    }

    /// Submits a list of orders to the execution venue.
    ///
    /// # Errors
    ///
    /// Returns an error if submission fails.
    fn submit_order_list(&self, cmd: SubmitOrderList) -> anyhow::Result<()> {
        log_not_implemented(&cmd);
        Ok(())
    }

    /// Modifies an existing order.
    ///
    /// # Errors
    ///
    /// Returns an error if modification fails.
    fn modify_order(&self, cmd: ModifyOrder) -> anyhow::Result<()> {
        log_not_implemented(&cmd);
        Ok(())
    }

    /// Modifies a batch of orders.
    ///
    /// The default implementation fans out to [`Self::modify_order`] so existing execution
    /// clients remain compatible until they add native batch support.
    ///
    /// # Errors
    ///
    /// Returns an error if any child modification fails.
    fn batch_modify_orders(&self, cmd: BatchModifyOrders) -> anyhow::Result<()> {
        for modify in cmd.modifies {
            self.modify_order(modify)?;
        }
        Ok(())
    }

    /// Cancels a specific order.
    ///
    /// # Errors
    ///
    /// Returns an error if cancellation fails.
    fn cancel_order(&self, cmd: CancelOrder) -> anyhow::Result<()> {
        log_not_implemented(&cmd);
        Ok(())
    }

    /// Cancels all orders.
    ///
    /// # Errors
    ///
    /// Returns an error if cancellation fails.
    fn cancel_all_orders(&self, cmd: CancelAllOrders) -> anyhow::Result<()> {
        log_not_implemented(&cmd);
        Ok(())
    }

    /// Cancels a batch of orders.
    ///
    /// # Errors
    ///
    /// Returns an error if batch cancellation fails.
    fn batch_cancel_orders(&self, cmd: BatchCancelOrders) -> anyhow::Result<()> {
        log_not_implemented(&cmd);
        Ok(())
    }

    /// Queries the status of an account.
    ///
    /// # Errors
    ///
    /// Returns an error if the query fails.
    fn query_account(&self, cmd: QueryAccount) -> anyhow::Result<()> {
        log_not_implemented(&cmd);
        Ok(())
    }

    /// Queries the status of an order.
    ///
    /// # Errors
    ///
    /// Returns an error if the query fails.
    fn query_order(&self, cmd: QueryOrder) -> anyhow::Result<()> {
        log_not_implemented(&cmd);
        Ok(())
    }

    /// Generates a single order status report.
    ///
    /// # Errors
    ///
    /// Returns an error if report generation fails.
    async fn generate_order_status_report(
        &self,
        cmd: &GenerateOrderStatusReport,
    ) -> anyhow::Result<Option<OrderStatusReport>> {
        log_not_implemented(cmd);
        Ok(None)
    }

    /// Generates multiple order status reports.
    ///
    /// # Errors
    ///
    /// Returns an error if report generation fails.
    async fn generate_order_status_reports(
        &self,
        cmd: &GenerateOrderStatusReports,
    ) -> anyhow::Result<Vec<OrderStatusReport>> {
        log_not_implemented(cmd);
        Ok(Vec::new())
    }

    /// Generates fill reports based on execution results.
    ///
    /// # Errors
    ///
    /// Returns an error if fill report generation fails.
    async fn generate_fill_reports(
        &self,
        cmd: GenerateFillReports,
    ) -> anyhow::Result<Vec<FillReport>> {
        log_not_implemented(&cmd);
        Ok(Vec::new())
    }

    /// Generates position status reports.
    ///
    /// # Errors
    ///
    /// Returns an error if generation fails.
    async fn generate_position_status_reports(
        &self,
        cmd: &GeneratePositionStatusReports,
    ) -> anyhow::Result<Vec<PositionStatusReport>> {
        log_not_implemented(cmd);
        Ok(Vec::new())
    }

    /// Generates mass status for executions.
    ///
    /// The default composes the granular report generators using the realtime atomic clock.
    /// This is clock-correct only for live/realtime clients; clients using a mocked or backtest
    /// clock must override this method to compose reports with their own clock.
    ///
    /// # Errors
    ///
    /// Returns an error if status generation fails.
    async fn generate_mass_status(
        &self,
        lookback_mins: Option<u64>,
    ) -> anyhow::Result<Option<ExecutionMassStatus>> {
        let ts_init = get_atomic_clock_realtime().get_time_ns();
        let start = lookback_mins
            .map(|mins| {
                checked_mins_to_nanos(mins)
                    .map(|lookback_ns| {
                        UnixNanos::from(ts_init.as_u64().saturating_sub(lookback_ns))
                    })
                    .ok_or_else(|| anyhow::anyhow!("lookback minutes overflow nanoseconds: {mins}"))
            })
            .transpose()?;

        let order_cmd = GenerateOrderStatusReportsBuilder::default()
            .ts_init(ts_init)
            .open_only(false)
            .start(start)
            .build()
            .context("failed to build order status reports command")?;
        let fill_cmd = GenerateFillReportsBuilder::default()
            .ts_init(ts_init)
            .start(start)
            .build()
            .context("failed to build fill reports command")?;
        let position_cmd = GeneratePositionStatusReportsBuilder::default()
            .ts_init(ts_init)
            .start(start)
            .build()
            .context("failed to build position status reports command")?;

        let (order_reports, fill_reports, position_reports) = futures::try_join!(
            async {
                self.generate_order_status_reports(&order_cmd)
                    .await
                    .context("failed to generate order status reports")
            },
            async {
                self.generate_fill_reports(fill_cmd)
                    .await
                    .context("failed to generate fill reports")
            },
            async {
                self.generate_position_status_reports(&position_cmd)
                    .await
                    .context("failed to generate position status reports")
            },
        )?;

        let mut mass_status = ExecutionMassStatus::new(
            self.client_id(),
            self.account_id(),
            self.venue(),
            ts_init,
            None,
        );
        mass_status.add_order_reports(order_reports);
        mass_status.add_fill_reports(fill_reports);
        mass_status.add_position_reports(position_reports);

        Ok(Some(mass_status))
    }

    /// Registers an external order for tracking by the execution client.
    ///
    /// This is called after reconciliation creates an external order, allowing the
    /// execution client to track it for subsequent events (e.g., cancellations).
    fn register_external_order(
        &self,
        _client_order_id: ClientOrderId,
        _venue_order_id: VenueOrderId,
        _instrument_id: InstrumentId,
        _strategy_id: StrategyId,
        _ts_init: UnixNanos,
    ) {
        // Default no-op implementation
    }

    /// Handles an instrument update received via the message bus.
    ///
    /// Exec clients that need live instrument updates (e.g. for internal maps)
    /// can override this to process instruments for their venue.
    fn on_instrument(&mut self, _instrument: InstrumentAny) {
        // Default no-op
    }

    /// Calculates the commission for a reconciliation fill.
    ///
    /// Override this method to provide venue-specific commission logic
    /// for inferred fills generated during reconciliation.
    ///
    /// Returns `None` by default, signaling callers to use their own
    /// generic commission formula.
    #[expect(unused_variables)]
    fn calculate_commission(
        &self,
        instrument: &InstrumentAny,
        last_qty: Quantity,
        last_px: Price,
        liquidity_side: LiquiditySide,
    ) -> Option<Money> {
        None
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use nautilus_core::UUID4;
    use nautilus_model::{
        enums::{
            LiquiditySide, OmsType, OrderSide, OrderStatus, OrderType, PositionSideSpecified,
            TimeInForce,
        },
        identifiers::{PositionId, TradeId, TraderId, Venue},
        types::Currency,
    };
    use rstest::rstest;

    use super::*;

    struct RecordingExecutionClient {
        modified_order_ids: Rc<RefCell<Vec<ClientOrderId>>>,
    }

    impl RecordingExecutionClient {
        fn new(modified_order_ids: Rc<RefCell<Vec<ClientOrderId>>>) -> Self {
            Self { modified_order_ids }
        }
    }

    #[async_trait(?Send)]
    impl ExecutionClient for RecordingExecutionClient {
        fn is_connected(&self) -> bool {
            true
        }

        fn client_id(&self) -> ClientId {
            ClientId::from("TEST")
        }

        fn account_id(&self) -> AccountId {
            AccountId::from("TEST-001")
        }

        fn venue(&self) -> Venue {
            Venue::from("SIM")
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
        ) -> anyhow::Result<()> {
            Ok(())
        }

        fn start(&mut self) -> anyhow::Result<()> {
            Ok(())
        }

        fn stop(&mut self) -> anyhow::Result<()> {
            Ok(())
        }

        fn modify_order(&self, cmd: ModifyOrder) -> anyhow::Result<()> {
            self.modified_order_ids
                .borrow_mut()
                .push(cmd.client_order_id);

            Ok(())
        }
    }

    struct MassStatusExecutionClient {
        order_commands: RefCell<Vec<GenerateOrderStatusReports>>,
        fill_requests: RefCell<Vec<GenerateFillReports>>,
        position_queries: RefCell<Vec<GeneratePositionStatusReports>>,
        fail_fill: bool,
    }

    impl MassStatusExecutionClient {
        fn new(fail_fill: bool) -> Self {
            Self {
                order_commands: RefCell::new(Vec::new()),
                fill_requests: RefCell::new(Vec::new()),
                position_queries: RefCell::new(Vec::new()),
                fail_fill,
            }
        }
    }

    #[async_trait(?Send)]
    impl ExecutionClient for MassStatusExecutionClient {
        fn is_connected(&self) -> bool {
            true
        }

        fn client_id(&self) -> ClientId {
            ClientId::from("MASS-STATUS")
        }

        fn account_id(&self) -> AccountId {
            AccountId::from("MASS-STATUS-001")
        }

        fn venue(&self) -> Venue {
            Venue::from("SIM")
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
        ) -> anyhow::Result<()> {
            Ok(())
        }

        fn start(&mut self) -> anyhow::Result<()> {
            Ok(())
        }

        fn stop(&mut self) -> anyhow::Result<()> {
            Ok(())
        }

        async fn generate_order_status_reports(
            &self,
            cmd: &GenerateOrderStatusReports,
        ) -> anyhow::Result<Vec<OrderStatusReport>> {
            self.order_commands.borrow_mut().push(cmd.clone());
            Ok(vec![test_order_report()])
        }

        async fn generate_fill_reports(
            &self,
            cmd: GenerateFillReports,
        ) -> anyhow::Result<Vec<FillReport>> {
            self.fill_requests.borrow_mut().push(cmd);

            if self.fail_fill {
                anyhow::bail!("sentinel fill report failure");
            }
            Ok(vec![test_fill_report()])
        }

        async fn generate_position_status_reports(
            &self,
            cmd: &GeneratePositionStatusReports,
        ) -> anyhow::Result<Vec<PositionStatusReport>> {
            self.position_queries.borrow_mut().push(cmd.clone());
            Ok(vec![test_position_report()])
        }
    }

    fn test_order_report() -> OrderStatusReport {
        OrderStatusReport::new(
            AccountId::from("MASS-STATUS-001"),
            InstrumentId::from("AUD/USD.SIM"),
            None,
            VenueOrderId::from("ORDER-001"),
            OrderSide::Buy,
            OrderType::Limit,
            TimeInForce::Gtc,
            OrderStatus::Accepted,
            Quantity::from("10"),
            Quantity::from("0"),
            UnixNanos::from(1_000_000_000),
            UnixNanos::from(2_000_000_000),
            UnixNanos::from(3_000_000_000),
            None,
        )
    }

    fn test_fill_report() -> FillReport {
        FillReport::new(
            AccountId::from("MASS-STATUS-001"),
            InstrumentId::from("AUD/USD.SIM"),
            VenueOrderId::from("ORDER-001"),
            TradeId::from("TRADE-001"),
            OrderSide::Buy,
            Quantity::from("5"),
            Price::from("1.00010"),
            Money::new(1.0, Currency::USD()),
            LiquiditySide::Taker,
            None,
            None,
            UnixNanos::from(4_000_000_000),
            UnixNanos::from(5_000_000_000),
            None,
        )
    }

    fn test_position_report() -> PositionStatusReport {
        PositionStatusReport::new(
            AccountId::from("MASS-STATUS-001"),
            InstrumentId::from("AUD/USD.SIM"),
            PositionSideSpecified::Long,
            Quantity::from("5"),
            UnixNanos::from(6_000_000_000),
            UnixNanos::from(7_000_000_000),
            None,
            Some(PositionId::from("POSITION-001")),
            None,
        )
    }

    #[rstest]
    fn batch_modify_orders_default_fans_out_to_modify_order() {
        let modified_order_ids = Rc::new(RefCell::new(Vec::new()));
        let client = RecordingExecutionClient::new(modified_order_ids.clone());
        let instrument_id = InstrumentId::from("AUD/USD.SIM");
        let order1 = ClientOrderId::from("O-DEFAULT-BATCH-001");
        let order2 = ClientOrderId::from("O-DEFAULT-BATCH-002");
        let command = BatchModifyOrders::new(
            TraderId::from("TRADER-001"),
            Some(ClientId::from("TEST")),
            StrategyId::from("S-001"),
            instrument_id,
            vec![
                ModifyOrder::new(
                    TraderId::from("TRADER-001"),
                    Some(ClientId::from("TEST")),
                    StrategyId::from("S-001"),
                    instrument_id,
                    order1,
                    None,
                    Some(Quantity::from("10")),
                    Some(Price::from("1.00010")),
                    None,
                    UUID4::new(),
                    UnixNanos::default(),
                    None,
                    None,
                ),
                ModifyOrder::new(
                    TraderId::from("TRADER-001"),
                    Some(ClientId::from("TEST")),
                    StrategyId::from("S-001"),
                    instrument_id,
                    order2,
                    None,
                    Some(Quantity::from("20")),
                    Some(Price::from("1.00020")),
                    None,
                    UUID4::new(),
                    UnixNanos::default(),
                    None,
                    None,
                ),
            ],
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        );

        client.batch_modify_orders(command).unwrap();

        assert_eq!(modified_order_ids.borrow().as_slice(), &[order1, order2]);
    }

    #[rstest]
    fn generate_mass_status_default_composes_granular_reports() {
        let client = MassStatusExecutionClient::new(false);

        let mass_status = futures::executor::block_on(client.generate_mass_status(Some(5)))
            .unwrap()
            .unwrap();

        assert_eq!(mass_status.client_id, ClientId::from("MASS-STATUS"));
        assert_eq!(mass_status.account_id, AccountId::from("MASS-STATUS-001"));
        assert_eq!(mass_status.venue, Venue::from("SIM"));

        let order_reports = mass_status.order_reports();
        let fill_reports = mass_status.fill_reports();
        let position_reports = mass_status.position_reports();
        let order_report = order_reports.get(&VenueOrderId::from("ORDER-001")).unwrap();
        let fill_report = &fill_reports.get(&VenueOrderId::from("ORDER-001")).unwrap()[0];
        let position_report = &position_reports
            .get(&InstrumentId::from("AUD/USD.SIM"))
            .unwrap()[0];
        assert_eq!(order_reports.len(), 1);
        assert_eq!(fill_reports.len(), 1);
        assert_eq!(position_reports.len(), 1);
        assert_eq!(
            order_report.instrument_id,
            InstrumentId::from("AUD/USD.SIM")
        );
        assert_eq!(fill_report.trade_id, TradeId::from("TRADE-001"));
        assert_eq!(
            position_report.venue_position_id,
            Some(PositionId::from("POSITION-001")),
        );

        let order_commands = client.order_commands.borrow();
        let fill_requests = client.fill_requests.borrow();
        let position_queries = client.position_queries.borrow();
        assert_eq!(order_commands.len(), 1);
        assert_eq!(fill_requests.len(), 1);
        assert_eq!(position_queries.len(), 1);

        let order_cmd = &order_commands[0];
        let fill_cmd = &fill_requests[0];
        let position_cmd = &position_queries[0];
        assert_eq!(order_cmd.ts_init, mass_status.ts_init);
        assert_eq!(fill_cmd.ts_init, mass_status.ts_init);
        assert_eq!(position_cmd.ts_init, mass_status.ts_init);
        assert_ne!(test_order_report().ts_init, mass_status.ts_init);
        assert_ne!(test_fill_report().ts_init, mass_status.ts_init);
        assert_ne!(test_position_report().ts_init, mass_status.ts_init);

        let expected_start = UnixNanos::from(
            mass_status
                .ts_init
                .as_u64()
                .saturating_sub(checked_mins_to_nanos(5).unwrap()),
        );
        assert_eq!(order_cmd.start, Some(expected_start));
        assert_eq!(fill_cmd.start, Some(expected_start));
        assert_eq!(position_cmd.start, Some(expected_start));
        assert!(!order_cmd.open_only);
        assert!(order_cmd.instrument_id.is_none());
        assert!(order_cmd.end.is_none());
        assert!(order_cmd.params.is_none());
        assert!(fill_cmd.instrument_id.is_none());
        assert!(fill_cmd.venue_order_id.is_none());
        assert!(fill_cmd.end.is_none());
        assert!(fill_cmd.params.is_none());
        assert!(position_cmd.instrument_id.is_none());
        assert!(position_cmd.end.is_none());
        assert!(position_cmd.params.is_none());
    }

    #[rstest]
    fn generate_mass_status_default_propagates_granular_error() {
        let client = MassStatusExecutionClient::new(true);

        let error = futures::executor::block_on(client.generate_mass_status(Some(5))).unwrap_err();

        let error_chain = format!("{error:#}");
        assert!(error_chain.contains("failed to generate fill reports"));
        assert!(error_chain.contains("sentinel fill report failure"));
    }
}
