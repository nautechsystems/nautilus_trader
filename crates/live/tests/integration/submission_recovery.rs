// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
//
//  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
//  you may not use this file except in compliance with the License.
//  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
//
//  Unless required by applicable law or agreed to in writing, software
//  distributed under the License is distributed on an "AS IS" BASIS,
//  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
//  See the License for the specific language governing permissions and
//  limitations under the License.
// -------------------------------------------------------------------------------------------------

//! Running-node coverage of submissions whose acknowledgement and query responses are lost.

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    time::Duration,
};

use async_trait::async_trait;
use nautilus_common::{
    actor::DataActor,
    cache::CacheView,
    clients::{DataClient, ExecutionClient},
    clock::Clock,
    factories::{ClientConfig, DataClientFactory, ExecutionClientFactory},
    live::runner::get_exec_event_sender,
    logging::logger::LoggerConfig,
    messages::{
        ExecutionEvent,
        execution::{
            CancelOrder, ExecutionReport, GenerateFillReports, GenerateOrderStatusReport,
            GenerateOrderStatusReports, QueryOrder, SubmitOrder, SubmitOrderList, TradingCommand,
        },
    },
    msgbus::{
        self, MessagingSwitchboard, stubs::get_any_saving_handler, switchboard::get_trades_topic,
    },
    testing::wait_until_async,
};
use nautilus_core::{Params, UUID4, UnixNanos};
use nautilus_live::{
    builder::LiveNodeBuilder,
    config::{
        LiveExecutionEngineConfig, LiveNodeConfig, LiveRiskEngineConfig,
        SubmittedOrderExhaustionPolicy,
    },
    execution::submission::{SubmissionRecoveryExhausted, SubmissionRecoverySource},
    node::{LiveNodeHandle, NodeState},
};
use nautilus_model::{
    accounts::{AccountAny, MarginAccount},
    data::TradeTick,
    enums::{
        AccountType, AggressorSide, LiquiditySide, OmsType, OrderSide, OrderStatus, OrderType,
        TriggerType,
    },
    events::{
        AccountState, OrderAcceptedBatch, OrderCanceledBatch, OrderEventAny,
        order::spec::{
            OrderExpiredSpec, OrderFillVoidedSpec, OrderPendingCancelSpec, OrderPendingUpdateSpec,
            OrderReleasedSpec, OrderTriggeredSpec, OrderUpdatedSpec,
        },
    },
    identifiers::{
        AccountId, ClientId, ClientOrderId, OrderListId, StrategyId, TradeId, TraderId, Venue,
        VenueOrderId,
    },
    instruments::{
        Instrument, InstrumentAny,
        stubs::{crypto_perpetual_ethusdt, currency_pair_btcusdt},
    },
    orders::{Order, OrderAny, OrderList, OrderTestBuilder, stubs::TestOrderEventStubs},
    reports::{ExecutionMassStatus, FillReport, OrderStatusReport},
    types::{AccountBalance, Currency, MarginBalance, Money, Price, Quantity},
};
use nautilus_trading::{
    nautilus_strategy,
    strategy::{Strategy, StrategyConfig, StrategyCore},
};
use rstest::rstest;

#[derive(Debug, Default)]
struct SubmissionState {
    stop_on_submit: RefCell<Option<LiveNodeHandle>>,
    submitted: RefCell<Option<OrderAny>>,
    queries: RefCell<Vec<ClientOrderId>>,
    submitted_ids: RefCell<Vec<ClientOrderId>>,
    report: RefCell<Option<OrderStatusReport>>,
    fills: RefCell<Vec<FillReport>>,
    bulk_queries: Cell<usize>,
    fill_queries: Cell<usize>,
    query_failure: QueryFailure,
    seed_venue_id: bool,
    disconnect_evidence: bool,
    connection_failure: Option<bool>,
}

#[derive(Debug)]
struct SubmissionClientConfig;

impl ClientConfig for SubmissionClientConfig {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[derive(Debug)]
struct SubmissionClientFactory(Rc<SubmissionState>);

impl ExecutionClientFactory for SubmissionClientFactory {
    fn create(
        &self,
        _trader_id: TraderId,
        _name: &str,
        _config: &dyn ClientConfig,
        cache: CacheView,
        _clock: Rc<RefCell<dyn Clock>>,
    ) -> anyhow::Result<Box<dyn ExecutionClient>> {
        Ok(Box::new(SubmissionClient {
            connected: Cell::new(false),
            cache,
            state: self.0.clone(),
        }))
    }
    fn name(&self) -> &'static str {
        "SubmissionClientFactory"
    }
    fn config_type(&self) -> &'static str {
        "SubmissionClientConfig"
    }
}

struct SubmissionClient {
    connected: Cell<bool>,
    cache: CacheView,
    state: Rc<SubmissionState>,
}

#[async_trait(?Send)]
impl ExecutionClient for SubmissionClient {
    fn is_connected(&self) -> bool {
        self.connected.get()
    }
    fn client_id(&self) -> ClientId {
        ClientId::from("BINANCE")
    }
    fn account_id(&self) -> AccountId {
        AccountId::from("BINANCE-001")
    }
    fn venue(&self) -> Venue {
        Venue::from("BINANCE")
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
    async fn connect(&mut self) -> anyhow::Result<()> {
        if let Some(wait_forever) = self.state.connection_failure {
            if wait_forever {
                std::future::pending::<()>().await;
            }
            return Ok(());
        }
        self.connected.set(true);
        Ok(())
    }
    async fn disconnect(&mut self) -> anyhow::Result<()> {
        if self.state.disconnect_evidence {
            let order = self.state.submitted.borrow().clone().unwrap();
            let (report, _) =
                venue_evidence(&order, OrderStatus::Accepted, Quantity::from("0.000"));
            get_exec_event_sender().send(ExecutionEvent::Report(ExecutionReport::Order(
                Box::new(report),
            )))?;
        }
        self.connected.set(false);
        Ok(())
    }

    fn submit_order(&self, command: SubmitOrder) -> anyhow::Result<()> {
        let order = self
            .cache
            .borrow()
            .order_owned(&command.client_order_id)
            .unwrap();
        let mut submitted = TestOrderEventStubs::submitted(&order, self.account_id());
        if let OrderEventAny::Submitted(event) = &mut submitted {
            event.ts_event = order.ts_init();
            event.ts_init = order.ts_init();
        }
        self.state
            .submitted_ids
            .borrow_mut()
            .push(order.client_order_id());
        if self.state.submitted.borrow().is_none() {
            self.state.submitted.replace(Some(order.clone()));
        }
        get_exec_event_sender().send(ExecutionEvent::Order(submitted))?;

        if self.state.seed_venue_id {
            let updated = OrderUpdatedSpec::builder()
                .trader_id(order.trader_id())
                .strategy_id(order.strategy_id())
                .instrument_id(order.instrument_id())
                .client_order_id(order.client_order_id())
                .account_id(self.account_id())
                .venue_order_id(VenueOrderId::new(format!("V-{}", order.client_order_id())))
                .quantity(order.quantity())
                .price(Price::from("100.00"))
                .ts_event(order.ts_init())
                .ts_init(order.ts_init())
                .build();
            get_exec_event_sender().send(ExecutionEvent::Order(OrderEventAny::Updated(updated)))?;
        }

        if order.order_type() == OrderType::Market && order.order_side() == OrderSide::Sell {
            let (report, fills) = venue_evidence(&order, OrderStatus::Filled, order.quantity());
            get_exec_event_sender().send(ExecutionEvent::Report(
                ExecutionReport::OrderWithFills(Box::new(report), fills),
            ))?;
        }

        if let Some(handle) = self.state.stop_on_submit.borrow().as_ref() {
            handle.stop();
        }
        Ok(())
    }

    fn submit_order_list(&self, command: SubmitOrderList) -> anyhow::Result<()> {
        for init in command.order_inits {
            let order = self
                .cache
                .borrow()
                .order_owned(&init.client_order_id)
                .unwrap();
            self.submit_order(SubmitOrder::from_order(
                &order,
                command.trader_id,
                command.client_id,
                None,
                UUID4::new(),
                command.ts_init,
            ))?;
        }
        Ok(())
    }

    fn query_order(&self, command: QueryOrder) -> anyhow::Result<()> {
        self.state
            .queries
            .borrow_mut()
            .push(command.client_order_id);
        // Unknown submissions receive no acknowledgement or query result
        if let Some(report) = self.state.report.borrow().clone() {
            get_exec_event_sender().send(ExecutionEvent::Report(ExecutionReport::Order(
                Box::new(report),
            )))?;
        }
        Ok(())
    }

    fn cancel_order(&self, _command: CancelOrder) -> anyhow::Result<()> {
        Ok(())
    }

    async fn generate_order_status_reports(
        &self,
        _command: &GenerateOrderStatusReports,
    ) -> anyhow::Result<Vec<OrderStatusReport>> {
        self.state
            .bulk_queries
            .set(self.state.bulk_queries.get() + 1);
        if matches!(self.state.query_failure, QueryFailure::BulkFillTimeout)
            && let Some(order) = self.state.submitted.borrow().as_ref()
        {
            let (report, _) = venue_evidence(order, OrderStatus::Canceled, Quantity::from("0.500"));
            return Ok(vec![report]);
        }
        Ok(self.state.report.borrow().iter().cloned().collect())
    }

    async fn generate_order_status_report(
        &self,
        command: &GenerateOrderStatusReport,
    ) -> anyhow::Result<Option<OrderStatusReport>> {
        self.state
            .queries
            .borrow_mut()
            .push(command.client_order_id.unwrap());

        match self.state.query_failure {
            QueryFailure::Error => anyhow::bail!("query transport failed"),
            QueryFailure::Timeout => std::future::pending().await,
            QueryFailure::PendingCancel | QueryFailure::PendingUpdate => {
                let order = self.state.submitted.borrow().clone().unwrap();
                let event = if matches!(self.state.query_failure, QueryFailure::PendingCancel) {
                    OrderEventAny::PendingCancel(
                        OrderPendingCancelSpec::builder()
                            .trader_id(order.trader_id())
                            .strategy_id(order.strategy_id())
                            .instrument_id(order.instrument_id())
                            .client_order_id(order.client_order_id())
                            .account_id(self.account_id())
                            .build(),
                    )
                } else {
                    OrderEventAny::PendingUpdate(
                        OrderPendingUpdateSpec::builder()
                            .trader_id(order.trader_id())
                            .strategy_id(order.strategy_id())
                            .instrument_id(order.instrument_id())
                            .client_order_id(order.client_order_id())
                            .account_id(self.account_id())
                            .build(),
                    )
                };
                get_exec_event_sender().send(ExecutionEvent::Order(event))?;
                tokio::time::sleep(Duration::from_millis(20)).await;
                Ok(None)
            }
            QueryFailure::Submitted => {
                let order = self.state.submitted.borrow().clone().unwrap();
                let (report, _) =
                    venue_evidence(&order, OrderStatus::Submitted, Quantity::from("0.000"));
                Ok(Some(report))
            }
            QueryFailure::Accepted | QueryFailure::Triggered | QueryFailure::Expired => {
                let order = self.state.submitted.borrow().clone().unwrap();
                let status = match self.state.query_failure {
                    QueryFailure::Triggered => OrderStatus::Triggered,
                    QueryFailure::Expired => OrderStatus::Expired,
                    _ => OrderStatus::Accepted,
                };
                let (report, _) = venue_evidence(&order, status, Quantity::from("0.000"));
                self.state.report.replace(Some(report.clone()));
                Ok(Some(report))
            }
            QueryFailure::CanceledFills | QueryFailure::ExpiredFills => {
                let order = self.state.submitted.borrow().clone().unwrap();
                let status = if matches!(self.state.query_failure, QueryFailure::CanceledFills) {
                    OrderStatus::Canceled
                } else {
                    OrderStatus::Expired
                };
                let (report, _) = venue_evidence(&order, status, Quantity::from("0.500"));
                Ok(Some(report))
            }
            QueryFailure::Mismatch => {
                let order = self.state.submitted.borrow().clone().unwrap();
                let (mut report, _) =
                    venue_evidence(&order, OrderStatus::Accepted, Quantity::from("0.000"));
                report.client_order_id = Some(ClientOrderId::from("WRONG-ID"));
                Ok(Some(report))
            }
            QueryFailure::None | QueryFailure::BulkFillTimeout => {
                Ok(self.state.report.borrow().clone())
            }
        }
    }

    async fn generate_fill_reports(
        &self,
        _command: GenerateFillReports,
    ) -> anyhow::Result<Vec<FillReport>> {
        self.state
            .fill_queries
            .set(self.state.fill_queries.get() + 1);
        if matches!(self.state.query_failure, QueryFailure::BulkFillTimeout) {
            return std::future::pending().await;
        }
        Ok(self.state.fills.borrow().clone())
    }
}

#[derive(Debug)]
struct SubmissionStrategy {
    core: StrategyCore,
    order: OrderAny,
    origin: SubmissionOrigin,
}

impl DataActor for SubmissionStrategy {
    fn on_start(&mut self) -> anyhow::Result<()> {
        if self.origin == SubmissionOrigin::FailStart {
            anyhow::bail!("strategy startup failure");
        }

        if matches!(
            self.origin,
            SubmissionOrigin::Startup
                | SubmissionOrigin::RestoredSubmitted
                | SubmissionOrigin::RestoredReleased
                | SubmissionOrigin::RestoredPendingCancel
                | SubmissionOrigin::RestoredPendingUpdate
                | SubmissionOrigin::NativeIngress
        ) {
            return Ok(());
        }

        if matches!(
            self.origin,
            SubmissionOrigin::Strategy | SubmissionOrigin::Emulator
        ) {
            return self.submit_order(self.order.clone(), None, None, None);
        }
        // Exercise native command ingress: the engine reconstructs this uncached order
        let order = &self.order;
        let command = if matches!(
            self.origin,
            SubmissionOrigin::Command
                | SubmissionOrigin::UnroutableCommand
                | SubmissionOrigin::MissingInstrumentCommand
                | SubmissionOrigin::CachedMissingInstrumentCommand
                | SubmissionOrigin::PurgedUnroutableCommand
        ) {
            TradingCommand::SubmitOrder(SubmitOrder::from_order(
                order,
                order.trader_id(),
                None,
                None,
                UUID4::new(),
                order.ts_init(),
            ))
        } else {
            let list = OrderList::new(
                order.order_list_id().unwrap(),
                order.instrument_id(),
                order.strategy_id(),
                vec![order.client_order_id()],
                order.ts_init(),
            );
            TradingCommand::SubmitOrderList(SubmitOrderList::new(
                order.trader_id(),
                None,
                order.strategy_id(),
                list,
                vec![order.init_event().clone()],
                None,
                None,
                None,
                UUID4::new(),
                order.ts_init(),
                None,
            ))
        };
        msgbus::send_trading_command(MessagingSwitchboard::exec_engine_queue_execute(), command);
        Ok(())
    }
}

nautilus_strategy!(SubmissionStrategy);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SubmissionOrigin {
    Strategy,
    Emulator,
    Command,
    OrderList,
    Startup,
    RestoredSubmitted,
    RestoredReleased,
    RestoredPendingCancel,
    RestoredPendingUpdate,
    NativeIngress,
    UnroutableCommand,
    UnroutableList,
    MissingInstrumentCommand,
    MissingInstrumentList,
    CachedMissingInstrumentCommand,
    CachedMissingInstrumentList,
    PurgedUnroutableCommand,
    PurgedUnroutableList,
    FailStart,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RecoveryPath {
    Inflight,
    MissingOrder,
    MissingError,
    MissingMismatch,
    MissingTimeout,
    MissingCanceledFills,
    MissingExpiredFills,
    BulkFillTimeout,
    MissingRecovered,
    MissingTriggered,
    MissingExpired,
    MissingSubmittedReport,
    MissingPendingCancel,
    MissingPendingUpdate,
    Both,
}

#[derive(Clone, Copy, Debug, Default)]
enum QueryFailure {
    #[default]
    None,
    Error,
    Mismatch,
    Timeout,
    BulkFillTimeout,
    CanceledFills,
    ExpiredFills,
    PendingCancel,
    PendingUpdate,
    Accepted,
    Triggered,
    Expired,
    Submitted,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FillIngress {
    Standalone,
    Bundled,
    Direct,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FillMismatch {
    None,
    Instrument,
    Side,
    Account,
    Venue,
    Type,
    Zero,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Evidence {
    RawFill(FillIngress, FillMismatch),
    None,
    DirectAccepted,
    DirectFilled,
    DirectTriggered,
    DirectExpired,
    DirectVoid(FillMismatch),
    InvalidDirectAccepted,
    InvalidDirectInstrument,
    InvalidDirectTrader,
    InvalidDirectFilled,
    InvalidInstrumentReport,
    InvalidInstrumentBareReport,
    InvalidAccountReport,
    InvalidAccountBareReport,
    InvalidOwnedAccepted,
    InvalidOwnedReport,
    Accepted,
    MassAccepted,
    MassFilled,
    FallbackAccepted,
    FallbackEventAccepted,
    FallbackBatchAccepted,
    FallbackBatchCanceled,
    FillVoided,
    DisconnectAccepted,
    QueryAccepted,
    QueryAcceptedWithInflight,
    QueryTriggered,
    QueryExpired,
    BulkAccepted,
    BulkTriggered,
    BulkExpired,
    Rejected,
    Canceled,
    Filled,
    PartialCanceled,
    ShutdownFill,
    SubmittedBareReport,
    SubmittedReport,
    PendingCancelReport,
    PendingUpdateReport,
    MassSubmittedReport,
    PendingReportWithFills,
    PendingReportWithLaggingQuantity,
}

fn venue_evidence(
    order: &OrderAny,
    status: OrderStatus,
    filled: Quantity,
) -> (OrderStatusReport, Vec<FillReport>) {
    let ts = UnixNanos::from(order.ts_init().as_u64() + 1_000_000_000);
    let venue_id = VenueOrderId::new(format!("V-{}", order.client_order_id()));
    let report = OrderStatusReport::new(
        AccountId::from("BINANCE-001"),
        order.instrument_id(),
        Some(order.client_order_id()),
        venue_id,
        Some(order.order_side()),
        order.order_type(),
        order.time_in_force(),
        status,
        order.quantity(),
        filled,
        ts,
        ts,
        ts,
        None,
    )
    .with_price(Price::from("100.00"));
    let fills = if filled.is_zero() {
        vec![]
    } else {
        vec![FillReport::new(
            AccountId::from("BINANCE-001"),
            order.instrument_id(),
            venue_id,
            TradeId::new(format!("T-{}", order.client_order_id())),
            order.order_side(),
            filled,
            Price::from("100.00"),
            Money::from("0 USDT"),
            LiquiditySide::Taker,
            Some(order.client_order_id()),
            None,
            ts,
            ts,
            None,
        )]
    };
    (report, fills)
}

#[rstest]
#[case::default_inflight(false, RecoveryPath::Inflight, Evidence::None)]
#[case::default_missing(false, RecoveryPath::MissingOrder, Evidence::None)]
#[case::retain_inflight(true, RecoveryPath::Inflight, Evidence::None)]
#[case::retain_missing(true, RecoveryPath::MissingOrder, Evidence::None)]
#[case::missing_error(true, RecoveryPath::MissingError, Evidence::None)]
#[case::missing_mismatch(true, RecoveryPath::MissingMismatch, Evidence::None)]
#[case::missing_timeout(true, RecoveryPath::MissingTimeout, Evidence::None)]
#[case::missing_canceled_fills(true, RecoveryPath::MissingCanceledFills, Evidence::None)]
#[case::missing_expired_fills(true, RecoveryPath::MissingExpiredFills, Evidence::None)]
#[case::missing_pending_cancel(true, RecoveryPath::MissingPendingCancel, Evidence::None)]
#[case::missing_pending_update(true, RecoveryPath::MissingPendingUpdate, Evidence::None)]
#[case::fallback_batch_accepted(true, RecoveryPath::Inflight, Evidence::FallbackBatchAccepted)]
#[case::fallback_batch_canceled(true, RecoveryPath::Inflight, Evidence::FallbackBatchCanceled)]
#[case::fill_voided(true, RecoveryPath::Inflight, Evidence::FillVoided)]
#[case::bulk_fill_timeout(true, RecoveryPath::BulkFillTimeout, Evidence::None)]
#[case::missing_submitted_report(true, RecoveryPath::MissingSubmittedReport, Evidence::None)]
#[case::submitted_bare_report(true, RecoveryPath::Inflight, Evidence::SubmittedBareReport)]
#[case::submitted_report(true, RecoveryPath::Inflight, Evidence::SubmittedReport)]
#[case::pending_cancel_report(true, RecoveryPath::Inflight, Evidence::PendingCancelReport)]
#[case::pending_update_report(true, RecoveryPath::Inflight, Evidence::PendingUpdateReport)]
#[case::mass_submitted_report(true, RecoveryPath::Inflight, Evidence::MassSubmittedReport)]
#[case::pending_report_with_fills(true, RecoveryPath::Inflight, Evidence::PendingReportWithFills)]
#[case::pending_report_with_lagging_quantity(
    true,
    RecoveryPath::Inflight,
    Evidence::PendingReportWithLaggingQuantity
)]
#[case::missing_recovered(true, RecoveryPath::MissingRecovered, Evidence::QueryAccepted)]
#[case::fallback_report(true, RecoveryPath::Inflight, Evidence::FallbackAccepted)]
#[case::fallback_event(true, RecoveryPath::Inflight, Evidence::FallbackEventAccepted)]
#[case::disconnect_evidence(true, RecoveryPath::Inflight, Evidence::DisconnectAccepted)]
#[case::retain_both(true, RecoveryPath::Both, Evidence::None)]
#[case::late_accepted(true, RecoveryPath::Inflight, Evidence::Accepted)]
#[case::mass_accepted(true, RecoveryPath::Inflight, Evidence::MassAccepted)]
#[case::mass_filled(true, RecoveryPath::Inflight, Evidence::MassFilled)]
#[case::late_rejected(true, RecoveryPath::Inflight, Evidence::Rejected)]
#[case::late_canceled(true, RecoveryPath::Inflight, Evidence::Canceled)]
#[case::late_fill(true, RecoveryPath::Inflight, Evidence::Filled)]
#[case::partial_then_cancel(true, RecoveryPath::Inflight, Evidence::PartialCanceled)]
#[case::missing_then_fill(true, RecoveryPath::MissingOrder, Evidence::Filled)]
#[case::shutdown_fill(true, RecoveryPath::Inflight, Evidence::ShutdownFill)]
#[tokio::test]
async fn submitted_exhaustion_runs_through_native_node(
    #[case] retain: bool,
    #[case] path: RecoveryPath,
    #[case] evidence: Evidence,
    #[values(
        SubmissionOrigin::Strategy,
        SubmissionOrigin::Emulator,
        SubmissionOrigin::Command,
        SubmissionOrigin::OrderList,
        SubmissionOrigin::Startup
    )]
    origin: SubmissionOrigin,
) {
    run_submission_case(retain, path, evidence, origin, false).await;
}

#[rstest]
#[tokio::test]
async fn startup_stop_preserves_submission_uncertainty(
    #[values(false, true)] retain: bool,
    #[values(Evidence::None, Evidence::DisconnectAccepted)] evidence: Evidence,
) {
    run_submission_case(
        retain,
        RecoveryPath::Inflight,
        evidence,
        SubmissionOrigin::Startup,
        true,
    )
    .await;
}

#[rstest]
#[tokio::test]
async fn cached_and_native_submission_ingress_is_recovered(
    #[values(
        SubmissionOrigin::RestoredSubmitted,
        SubmissionOrigin::RestoredPendingCancel,
        SubmissionOrigin::RestoredPendingUpdate,
        SubmissionOrigin::NativeIngress
    )]
    origin: SubmissionOrigin,
    #[values(
        Evidence::None,
        Evidence::Accepted,
        Evidence::Filled,
        Evidence::DisconnectAccepted
    )]
    evidence: Evidence,
) {
    run_submission_case(true, RecoveryPath::Inflight, evidence, origin, false).await;
}

#[rstest]
#[tokio::test]
async fn restored_released_submission_uses_native_recovery(
    #[values(RecoveryPath::Inflight, RecoveryPath::MissingOrder)] path: RecoveryPath,
    #[values(
        Evidence::None,
        Evidence::Accepted,
        Evidence::Filled,
        Evidence::MassAccepted,
        Evidence::MassFilled,
        Evidence::PendingReportWithLaggingQuantity,
        Evidence::DirectAccepted,
        Evidence::DirectFilled,
        Evidence::DirectTriggered,
        Evidence::DirectExpired,
        Evidence::InvalidDirectAccepted,
        Evidence::InvalidDirectInstrument,
        Evidence::InvalidDirectTrader,
        Evidence::InvalidDirectFilled,
        Evidence::InvalidInstrumentReport,
        Evidence::InvalidInstrumentBareReport,
        Evidence::InvalidOwnedAccepted,
        Evidence::InvalidOwnedReport
    )]
    evidence: Evidence,
) {
    run_submission_case(
        true,
        path,
        evidence,
        SubmissionOrigin::RestoredReleased,
        false,
    )
    .await;
}

#[rstest]
#[case::bulk_triggered(RecoveryPath::MissingOrder, Evidence::BulkTriggered)]
#[case::bulk_expired(RecoveryPath::MissingOrder, Evidence::BulkExpired)]
#[case::query_triggered(RecoveryPath::MissingTriggered, Evidence::QueryTriggered)]
#[case::query_expired(RecoveryPath::MissingExpired, Evidence::QueryExpired)]
#[case::direct_triggered(RecoveryPath::Inflight, Evidence::DirectTriggered)]
#[case::direct_expired(RecoveryPath::Inflight, Evidence::DirectExpired)]
#[tokio::test]
async fn retained_submission_recovers_from_terminal_and_triggered_evidence(
    #[case] path: RecoveryPath,
    #[case] evidence: Evidence,
    #[values(
        SubmissionOrigin::RestoredSubmitted,
        SubmissionOrigin::RestoredReleased,
        SubmissionOrigin::RestoredPendingCancel,
        SubmissionOrigin::RestoredPendingUpdate
    )]
    origin: SubmissionOrigin,
) {
    run_submission_case(true, path, evidence, origin, false).await;
}

#[rstest]
#[tokio::test]
async fn retained_submission_validates_report_identity(
    #[values(RecoveryPath::Inflight, RecoveryPath::MissingOrder)] path: RecoveryPath,
    #[values(
        SubmissionOrigin::RestoredSubmitted,
        SubmissionOrigin::RestoredPendingCancel,
        SubmissionOrigin::RestoredPendingUpdate
    )]
    origin: SubmissionOrigin,
    #[values(
        Evidence::InvalidInstrumentReport,
        Evidence::InvalidInstrumentBareReport,
        Evidence::InvalidAccountReport,
        Evidence::InvalidAccountBareReport
    )]
    evidence: Evidence,
) {
    run_submission_case(true, path, evidence, origin, false).await;
}

#[rstest]
#[tokio::test]
async fn retained_submission_recovers_from_direct_terminal_void(
    #[values(RecoveryPath::Inflight, RecoveryPath::MissingOrder)] path: RecoveryPath,
    #[values(
        SubmissionOrigin::RestoredSubmitted,
        SubmissionOrigin::RestoredReleased
    )]
    origin: SubmissionOrigin,
    #[values(
        FillMismatch::None,
        FillMismatch::Instrument,
        FillMismatch::Side,
        FillMismatch::Type,
        FillMismatch::Zero
    )]
    mismatch: FillMismatch,
) {
    run_submission_case(true, path, Evidence::DirectVoid(mismatch), origin, false).await;
}

#[rstest]
#[case::report(RecoveryPath::Inflight, Evidence::Accepted)]
#[case::direct(RecoveryPath::Inflight, Evidence::DirectAccepted)]
#[case::bulk(RecoveryPath::Both, Evidence::BulkAccepted)]
#[case::final_query(RecoveryPath::MissingRecovered, Evidence::QueryAcceptedWithInflight)]
#[tokio::test]
async fn late_acceptance_preserves_pre_acknowledgement_command(
    #[case] path: RecoveryPath,
    #[case] evidence: Evidence,
    #[values(
        SubmissionOrigin::RestoredPendingCancel,
        SubmissionOrigin::RestoredPendingUpdate
    )]
    origin: SubmissionOrigin,
) {
    run_submission_case(true, path, evidence, origin, false).await;
}

#[rstest]
#[tokio::test]
async fn pretransport_failures_preserve_defaults_and_resolve_opt_in_tracking(
    #[values(false, true)] retain: bool,
    #[values(
        SubmissionOrigin::UnroutableCommand,
        SubmissionOrigin::UnroutableList,
        SubmissionOrigin::MissingInstrumentCommand,
        SubmissionOrigin::MissingInstrumentList,
        SubmissionOrigin::CachedMissingInstrumentCommand,
        SubmissionOrigin::CachedMissingInstrumentList,
        SubmissionOrigin::PurgedUnroutableCommand,
        SubmissionOrigin::PurgedUnroutableList
    )]
    origin: SubmissionOrigin,
) {
    run_submission_case(
        retain,
        RecoveryPath::Inflight,
        Evidence::None,
        origin,
        false,
    )
    .await;
}

async fn run_submission_case(
    retain: bool,
    path: RecoveryPath,
    evidence: Evidence,
    origin: SubmissionOrigin,
    stop_during_connect: bool,
) {
    let emulated = origin == SubmissionOrigin::Emulator;
    let restored = matches!(
        origin,
        SubmissionOrigin::RestoredSubmitted
            | SubmissionOrigin::RestoredReleased
            | SubmissionOrigin::RestoredPendingCancel
            | SubmissionOrigin::RestoredPendingUpdate
    );
    let native_ingress = origin == SubmissionOrigin::NativeIngress;
    // The default startup drain bypasses manager observers, so inflight recovery is not registered.
    let default_startup_inflight =
        !retain && origin == SubmissionOrigin::Startup && path == RecoveryPath::Inflight;
    let no_route = matches!(
        origin,
        SubmissionOrigin::UnroutableCommand
            | SubmissionOrigin::UnroutableList
            | SubmissionOrigin::PurgedUnroutableCommand
            | SubmissionOrigin::PurgedUnroutableList
    );
    let cached_missing_instrument = matches!(
        origin,
        SubmissionOrigin::CachedMissingInstrumentCommand
            | SubmissionOrigin::CachedMissingInstrumentList
            | SubmissionOrigin::PurgedUnroutableList
    );
    let missing_instrument = cached_missing_instrument
        || matches!(
            origin,
            SubmissionOrigin::MissingInstrumentCommand | SubmissionOrigin::MissingInstrumentList
        );
    let definitive_denial = no_route || missing_instrument;
    let purge_denied = matches!(
        origin,
        SubmissionOrigin::PurgedUnroutableCommand | SubmissionOrigin::PurgedUnroutableList
    );
    let invalid_evidence = matches!(
        evidence,
        Evidence::InvalidDirectAccepted
            | Evidence::InvalidDirectInstrument
            | Evidence::InvalidDirectTrader
            | Evidence::InvalidDirectFilled
            | Evidence::InvalidInstrumentReport
            | Evidence::InvalidInstrumentBareReport
            | Evidence::InvalidAccountReport
            | Evidence::InvalidAccountBareReport
            | Evidence::InvalidOwnedAccepted
            | Evidence::InvalidOwnedReport
    ) || matches!(evidence, Evidence::RawFill(_, mismatch) | Evidence::DirectVoid(mismatch) if mismatch != FillMismatch::None);
    let pending_command = match origin {
        SubmissionOrigin::RestoredPendingCancel => Some(OrderStatus::PendingCancel),
        SubmissionOrigin::RestoredPendingUpdate => Some(OrderStatus::PendingUpdate),
        _ => None,
    };
    let query_outcome = match evidence {
        Evidence::QueryAccepted | Evidence::QueryAcceptedWithInflight => {
            Some(OrderStatus::Accepted)
        }
        Evidence::QueryTriggered => Some(OrderStatus::Triggered),
        Evidence::QueryExpired => Some(OrderStatus::Expired),
        _ => None,
    };
    let triggered_evidence = matches!(
        evidence,
        Evidence::DirectTriggered | Evidence::BulkTriggered | Evidence::QueryTriggered
    );
    let pending_report = invalid_evidence
        || matches!(
            evidence,
            Evidence::SubmittedBareReport
                | Evidence::SubmittedReport
                | Evidence::PendingCancelReport
                | Evidence::PendingUpdateReport
                | Evidence::MassSubmittedReport
        );
    let state = Rc::new(SubmissionState {
        query_failure: match path {
            RecoveryPath::MissingError => QueryFailure::Error,
            RecoveryPath::MissingMismatch => QueryFailure::Mismatch,
            RecoveryPath::MissingTimeout => QueryFailure::Timeout,
            RecoveryPath::BulkFillTimeout => QueryFailure::BulkFillTimeout,
            RecoveryPath::MissingCanceledFills => QueryFailure::CanceledFills,
            RecoveryPath::MissingExpiredFills => QueryFailure::ExpiredFills,
            RecoveryPath::MissingRecovered => QueryFailure::Accepted,
            RecoveryPath::MissingTriggered => QueryFailure::Triggered,
            RecoveryPath::MissingExpired => QueryFailure::Expired,
            RecoveryPath::MissingSubmittedReport => QueryFailure::Submitted,
            RecoveryPath::MissingPendingCancel => QueryFailure::PendingCancel,
            RecoveryPath::MissingPendingUpdate => QueryFailure::PendingUpdate,
            _ => QueryFailure::None,
        },
        seed_venue_id: matches!(
            evidence,
            Evidence::FallbackAccepted
                | Evidence::FallbackEventAccepted
                | Evidence::FallbackBatchAccepted
                | Evidence::FallbackBatchCanceled
                | Evidence::FillVoided
        ),
        disconnect_evidence: evidence == Evidence::DisconnectAccepted,
        ..Default::default()
    });
    let mut config = LiveNodeConfig {
        exec_engine: LiveExecutionEngineConfig {
            reconciliation: false,
            inflight_check_interval_ms: if !purge_denied
                && (matches!(path, RecoveryPath::Inflight | RecoveryPath::Both)
                    || evidence == Evidence::QueryAcceptedWithInflight
                    || pending_command.is_some() && triggered_evidence)
            {
                10
            } else {
                0
            },
            inflight_check_threshold_ms: if pending_command.is_some()
                && (triggered_evidence
                    || matches!(
                        evidence,
                        Evidence::Accepted
                            | Evidence::DirectAccepted
                            | Evidence::BulkAccepted
                            | Evidence::QueryAcceptedWithInflight
                    )) {
                100
            } else {
                10
            },
            inflight_check_retries: 3,
            open_check_interval_secs: if path == RecoveryPath::Inflight {
                None
            } else {
                Some(0.03)
            },
            open_check_threshold_ms: if matches!(
                path,
                RecoveryPath::MissingPendingCancel | RecoveryPath::MissingPendingUpdate
            ) {
                50
            } else {
                0
            },
            open_check_missing_retries: if path == RecoveryPath::BulkFillTimeout {
                3
            } else {
                2
            },
            open_check_open_only: false,
            open_check_lookback_mins: None,
            single_order_query_delay_ms: 0,
            ..Default::default()
        },
        risk_engine: LiveRiskEngineConfig {
            bypass: true,
            ..Default::default()
        },
        timeout_reconciliation: Duration::from_millis(100),
        delay_post_stop: if evidence == Evidence::ShutdownFill {
            Duration::from_secs(1)
        } else {
            Duration::ZERO
        },
        logging: LoggerConfig {
            bypass_logging: true,
            ..Default::default()
        },
        ..Default::default()
    };

    if retain {
        config.exec_engine.submitted_order_exhaustion_policy =
            SubmittedOrderExhaustionPolicy::RetainUnresolved;
    }

    let mut builder = LiveNodeBuilder::from_config(config)
        .unwrap()
        .with_name("SubmissionRecovery");

    if !no_route {
        builder = builder
            .add_exec_client(
                Some("BINANCE".into()),
                Box::new(SubmissionClientFactory(state.clone())),
                Box::new(SubmissionClientConfig),
            )
            .unwrap();
    }
    let mut node = builder.build().unwrap();
    let instrument = crypto_perpetual_ethusdt();
    let strategy_id = StrategyId::from("SUBMISSION-001");
    let mut order_builder = OrderTestBuilder::new(if emulated || triggered_evidence {
        OrderType::StopLimit
    } else {
        OrderType::Limit
    });

    if emulated || triggered_evidence {
        order_builder.trigger_price(Price::from("99.00"));
    }

    if emulated {
        order_builder.emulation_trigger(TriggerType::LastPrice);
    }

    if matches!(
        origin,
        SubmissionOrigin::OrderList
            | SubmissionOrigin::UnroutableList
            | SubmissionOrigin::MissingInstrumentList
            | SubmissionOrigin::CachedMissingInstrumentList
            | SubmissionOrigin::PurgedUnroutableList
    ) {
        order_builder.order_list_id(OrderListId::from("LIST-RECOVERY"));
    }
    let order = order_builder
        .trader_id(node.trader_id())
        .strategy_id(strategy_id)
        .ts_init(UnixNanos::from(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos() as u64,
        ))
        .instrument_id(instrument.id())
        .side(OrderSide::Buy)
        .quantity(Quantity::from("1.000"))
        .price(Price::from("100.00"))
        .build();
    let client_order_id = order.client_order_id();
    let cache = node.kernel().cache();
    cache
        .borrow_mut()
        .add_instrument(InstrumentAny::CurrencyPair(currency_pair_btcusdt()))
        .unwrap();

    if !missing_instrument {
        cache
            .borrow_mut()
            .add_instrument(InstrumentAny::CryptoPerpetual(instrument.clone()))
            .unwrap();
    }
    cache
        .borrow_mut()
        .add_account(AccountAny::Margin(MarginAccount::new(
            AccountState::new(
                AccountId::from("BINANCE-001"),
                AccountType::Margin,
                vec![AccountBalance::new(
                    Money::from("1000000 USDT"),
                    Money::from("0 USDT"),
                    Money::from("1000000 USDT"),
                )],
                vec![],
                true,
                UUID4::new(),
                UnixNanos::default(),
                UnixNanos::default(),
                Some(Currency::USDT()),
            ),
            true,
        )))
        .unwrap();

    if matches!(
        evidence,
        Evidence::InvalidOwnedAccepted | Evidence::InvalidOwnedReport
    ) {
        let mut owner = OrderTestBuilder::new(OrderType::Limit)
            .trader_id(node.trader_id())
            .strategy_id(strategy_id)
            .instrument_id(order.instrument_id())
            .client_order_id(ClientOrderId::from("O-OTHER-OWNER"))
            .quantity(order.quantity())
            .price(Price::from("100.00"))
            .build();
        owner
            .apply(TestOrderEventStubs::submitted(
                &owner,
                AccountId::from("BINANCE-001"),
            ))
            .unwrap();
        owner
            .apply(TestOrderEventStubs::accepted(
                &owner,
                AccountId::from("BINANCE-001"),
                VenueOrderId::new(format!("V-{client_order_id}")),
            ))
            .unwrap();
        owner
            .apply(TestOrderEventStubs::canceled(
                &owner,
                AccountId::from("BINANCE-001"),
                Some(VenueOrderId::new(format!("V-{client_order_id}"))),
            ))
            .unwrap();
        cache
            .borrow_mut()
            .add_order(owner, None, None, false)
            .unwrap();
        cache
            .borrow_mut()
            .add_venue_order_id(
                &ClientOrderId::from("O-OTHER-OWNER"),
                &VenueOrderId::new(format!("V-{client_order_id}")),
                false,
            )
            .unwrap();
        assert_eq!(
            cache
                .borrow()
                .client_order_id(&VenueOrderId::new(format!("V-{client_order_id}"))),
            Some(&ClientOrderId::from("O-OTHER-OWNER"))
        );
    }

    if cached_missing_instrument {
        cache
            .borrow_mut()
            .add_order(order.clone(), None, None, true)
            .unwrap();
    }

    if restored || native_ingress {
        let mut cached = order.clone();

        if restored {
            if origin == SubmissionOrigin::RestoredReleased {
                let released = OrderReleasedSpec::builder()
                    .trader_id(order.trader_id())
                    .strategy_id(order.strategy_id())
                    .instrument_id(order.instrument_id())
                    .client_order_id(client_order_id)
                    .released_price(Price::from("100.00"))
                    .build();
                cached.apply(OrderEventAny::Released(released)).unwrap();
            } else {
                cached
                    .apply(TestOrderEventStubs::submitted(
                        &cached,
                        AccountId::from("BINANCE-001"),
                    ))
                    .unwrap();
            }

            if matches!(
                origin,
                SubmissionOrigin::RestoredPendingCancel | SubmissionOrigin::RestoredPendingUpdate
            ) {
                let updated = OrderUpdatedSpec::builder()
                    .trader_id(cached.trader_id())
                    .strategy_id(cached.strategy_id())
                    .instrument_id(cached.instrument_id())
                    .client_order_id(client_order_id)
                    .account_id(AccountId::from("BINANCE-001"))
                    .venue_order_id(VenueOrderId::new(format!("V-{client_order_id}")))
                    .quantity(cached.quantity())
                    .price(Price::from("100.00"))
                    .build();
                cached.apply(OrderEventAny::Updated(updated)).unwrap();
                let event = if origin == SubmissionOrigin::RestoredPendingCancel {
                    OrderEventAny::PendingCancel(
                        OrderPendingCancelSpec::builder()
                            .trader_id(order.trader_id())
                            .strategy_id(order.strategy_id())
                            .instrument_id(order.instrument_id())
                            .client_order_id(client_order_id)
                            .account_id(AccountId::from("BINANCE-001"))
                            .build(),
                    )
                } else {
                    OrderEventAny::PendingUpdate(
                        OrderPendingUpdateSpec::builder()
                            .trader_id(order.trader_id())
                            .strategy_id(order.strategy_id())
                            .instrument_id(order.instrument_id())
                            .client_order_id(client_order_id)
                            .account_id(AccountId::from("BINANCE-001"))
                            .build(),
                    )
                };
                cached.apply(event).unwrap();
            }
        }
        cache
            .borrow_mut()
            .add_order(cached.clone(), None, Some(ClientId::from("BINANCE")), true)
            .unwrap();
        state.submitted.replace(Some(cached));
    }

    if stop_during_connect {
        state.stop_on_submit.replace(Some(node.handle()));
    }

    if origin == SubmissionOrigin::Startup {
        msgbus::send_trading_command(
            MessagingSwitchboard::exec_engine_queue_execute(),
            TradingCommand::SubmitOrder(SubmitOrder::from_order(
                &order,
                order.trader_id(),
                None,
                None,
                UUID4::new(),
                order.ts_init(),
            )),
        );
    }
    node.add_strategy(SubmissionStrategy {
        core: StrategyCore::new(StrategyConfig {
            strategy_id: Some(strategy_id),
            manage_stop: evidence == Evidence::ShutdownFill,
            ..Default::default()
        }),
        order,
        origin,
    })
    .unwrap();
    let (handler, exhausted) = get_any_saving_handler::<SubmissionRecoveryExhausted>(None);
    msgbus::subscribe_any(
        MessagingSwitchboard::submission_recovery_exhausted_topic().into(),
        handler,
        None,
    );
    let handle = node.handle();
    let execution_engine = Rc::clone(node.kernel().exec_engine());
    let driver = async {
        if native_ingress {
            wait_until_async(
                || async { handle.state() == NodeState::Running },
                Duration::from_secs(2),
            )
            .await;
            let cached = cache.borrow().order_owned(&client_order_id).unwrap();
            get_exec_event_sender()
                .send(ExecutionEvent::Order(TestOrderEventStubs::submitted(
                    &cached,
                    AccountId::from("BINANCE-001"),
                )))
                .unwrap();
        }

        if stop_during_connect {
            wait_until_async(
                || async { handle.state() == NodeState::Stopped },
                Duration::from_secs(5),
            )
            .await;
            return;
        }

        if definitive_denial && !retain {
            wait_until_async(
                || async { execution_engine.borrow().command_count() > 0 },
                Duration::from_secs(2),
            )
            .await;
            handle.stop();
            return;
        }

        if emulated {
            wait_until_async(
                || async {
                    cache
                        .borrow()
                        .order(&client_order_id)
                        .is_some_and(|order| order.status() == OrderStatus::Emulated)
                },
                Duration::from_secs(2),
            )
            .await;
            let order = cache.borrow().order_owned(&client_order_id).unwrap();
            let trade = TradeTick::new(
                order.instrument_id(),
                Price::from("100.00"),
                Quantity::from("1.000"),
                AggressorSide::Buy,
                TradeId::from("TRIGGER"),
                order.ts_init(),
                order.ts_init(),
            );
            msgbus::publish_trade(get_trades_topic(order.instrument_id()), &trade);
        }

        wait_until_async(
            || async {
                if definitive_denial {
                    cache
                        .borrow()
                        .order(&client_order_id)
                        .is_some_and(|order| order.status() == OrderStatus::Denied)
                } else if let Some(status) = query_outcome {
                    cache.borrow().order(&client_order_id).is_some_and(|order| {
                        if matches!(status, OrderStatus::Accepted | OrderStatus::Triggered)
                            && pending_command.is_some()
                        {
                            Some(order.status()) == pending_command
                                && order.events().iter().any(|event| {
                                    matches!(
                                        event,
                                        OrderEventAny::Accepted(_) | OrderEventAny::Triggered(_)
                                    )
                                })
                        } else {
                            order.status() == status
                        }
                    })
                } else if retain {
                    !exhausted.get_messages().is_empty()
                } else {
                    cache.borrow().order(&client_order_id).is_some_and(|order| {
                        order.status()
                            == if default_startup_inflight {
                                OrderStatus::Submitted
                            } else {
                                OrderStatus::Rejected
                            }
                    })
                }
            },
            Duration::from_secs(5),
        )
        .await;

        if purge_denied {
            let ts = cache.borrow().order(&client_order_id).unwrap().ts_last();
            cache
                .borrow_mut()
                .purge_closed_orders(UnixNanos::from(ts.as_u64() + 1), 0);
            assert!(cache.borrow().order(&client_order_id).is_none());
        }
        let queries_at_exhaustion = state.queries.borrow().len();
        let bulk_at_exhaustion = state.bulk_queries.get();
        // Allow many scheduler ticks to prove exhaustion does not restart per-order queries
        tokio::time::sleep(Duration::from_millis(150)).await;

        if evidence == Evidence::QueryAcceptedWithInflight
            || pending_command.is_some() && evidence == Evidence::QueryTriggered
        {
            wait_until_async(
                || async { state.queries.borrow().len() > queries_at_exhaustion },
                Duration::from_secs(2),
            )
            .await;
        } else if retain || !definitive_denial {
            assert_eq!(state.queries.borrow().len(), queries_at_exhaustion);
        }

        if retain && path != RecoveryPath::Inflight {
            assert!(state.bulk_queries.get() > bulk_at_exhaustion);
        }

        if evidence == Evidence::ShutdownFill {
            handle.stop();
            wait_until_async(
                || async { handle.state() == NodeState::ShuttingDown },
                Duration::from_secs(2),
            )
            .await;
        }

        if query_outcome.is_none()
            && !matches!(evidence, Evidence::None | Evidence::DisconnectAccepted)
        {
            let order = state.submitted.borrow().clone().unwrap();
            let status = match evidence {
                Evidence::Accepted
                | Evidence::BulkAccepted
                | Evidence::DirectAccepted
                | Evidence::InvalidDirectAccepted
                | Evidence::InvalidDirectInstrument
                | Evidence::InvalidDirectTrader
                | Evidence::InvalidOwnedAccepted
                | Evidence::InvalidOwnedReport
                | Evidence::MassAccepted
                | Evidence::FallbackAccepted
                | Evidence::FallbackEventAccepted
                | Evidence::FallbackBatchAccepted => OrderStatus::Accepted,
                Evidence::DirectTriggered | Evidence::BulkTriggered => OrderStatus::Triggered,
                Evidence::DirectExpired | Evidence::BulkExpired => OrderStatus::Expired,
                Evidence::FallbackBatchCanceled => OrderStatus::Canceled,
                Evidence::FillVoided | Evidence::DirectVoid(_) => OrderStatus::Voided,
                Evidence::SubmittedBareReport
                | Evidence::SubmittedReport
                | Evidence::MassSubmittedReport
                | Evidence::PendingReportWithFills
                | Evidence::PendingReportWithLaggingQuantity
                | Evidence::RawFill(FillIngress::Bundled, _) => OrderStatus::Submitted,
                Evidence::PendingCancelReport => OrderStatus::PendingCancel,
                Evidence::PendingUpdateReport => OrderStatus::PendingUpdate,
                Evidence::Rejected
                | Evidence::InvalidInstrumentReport
                | Evidence::InvalidInstrumentBareReport
                | Evidence::InvalidAccountReport
                | Evidence::InvalidAccountBareReport => OrderStatus::Rejected,
                Evidence::Canceled => OrderStatus::Canceled,
                Evidence::PartialCanceled => OrderStatus::PartiallyFilled,
                _ => OrderStatus::Filled,
            };
            let filled = match evidence {
                Evidence::Filled
                | Evidence::DirectFilled
                | Evidence::InvalidDirectFilled
                | Evidence::MassFilled
                | Evidence::ShutdownFill
                | Evidence::PendingReportWithFills
                | Evidence::PendingReportWithLaggingQuantity
                | Evidence::RawFill(_, _) => Quantity::from("1.000"),
                Evidence::PartialCanceled => Quantity::from("0.400"),
                _ => Quantity::from("0.000"),
            };
            let expected_status = if pending_report {
                cache.borrow().order(&client_order_id).unwrap().status()
            } else if matches!(status, OrderStatus::Accepted | OrderStatus::Triggered)
                && pending_command.is_some()
            {
                pending_command.unwrap()
            } else if matches!(
                evidence,
                Evidence::PendingReportWithFills
                    | Evidence::PendingReportWithLaggingQuantity
                    | Evidence::RawFill(_, _)
            ) {
                OrderStatus::Filled
            } else {
                status
            };
            let (mut report, mut fills) = venue_evidence(&order, status, filled);

            if matches!(
                evidence,
                Evidence::PendingReportWithLaggingQuantity
                    | Evidence::RawFill(FillIngress::Bundled, _)
            ) {
                report.filled_qty = Quantity::from("0.000");
            }

            if let Evidence::RawFill(_, mismatch) = evidence {
                match mismatch {
                    FillMismatch::Instrument => {
                        fills[0].instrument_id = currency_pair_btcusdt().id();
                    }
                    FillMismatch::Side => fills[0].order_side = OrderSide::Sell,
                    FillMismatch::Account => fills[0].account_id = AccountId::from("OTHER-001"),
                    FillMismatch::Venue => fills[0].venue_order_id = VenueOrderId::from("V-OTHER"),
                    FillMismatch::Zero => fills[0].last_qty = Quantity::from("0.000"),
                    FillMismatch::None | FillMismatch::Type => {}
                }
            }

            if matches!(
                evidence,
                Evidence::InvalidInstrumentReport | Evidence::InvalidInstrumentBareReport
            ) {
                report.instrument_id = "BTCUSDT.BINANCE".parse().unwrap();
            }

            if matches!(
                evidence,
                Evidence::InvalidAccountReport | Evidence::InvalidAccountBareReport
            ) {
                report.account_id = AccountId::from("BINANCE-OTHER");
            }

            if matches!(
                evidence,
                Evidence::FallbackAccepted
                    | Evidence::FallbackEventAccepted
                    | Evidence::FallbackBatchAccepted
                    | Evidence::FallbackBatchCanceled
            ) {
                report.client_order_id = Some(ClientOrderId::from("UNKNOWN-CLIENT-ID"));
            }

            if !invalid_evidence && !matches!(evidence, Evidence::DirectVoid(_)) {
                state.report.replace(Some(report.clone()));
                state.fills.replace(fills.clone());
            }
            let direct_fill = if matches!(
                evidence,
                Evidence::DirectFilled
                    | Evidence::InvalidDirectFilled
                    | Evidence::RawFill(FillIngress::Direct, _)
            ) {
                let fill = &fills[0];
                let mut event = TestOrderEventStubs::filled(
                    &order,
                    &InstrumentAny::from(instrument.clone()),
                    Some(fill.trade_id),
                    None,
                    Some(fill.last_px),
                    Some(fill.last_qty),
                    Some(fill.liquidity_side),
                    Some(fill.commission),
                    Some(fill.ts_event),
                    Some(fill.account_id),
                );

                if let OrderEventAny::Filled(event) = &mut event {
                    event.venue_order_id = fill.venue_order_id;
                    event.instrument_id = fill.instrument_id;
                    event.order_side = fill.order_side;
                    if evidence == Evidence::RawFill(FillIngress::Direct, FillMismatch::Type) {
                        event.order_type = OrderType::Market;
                    }

                    if evidence == Evidence::InvalidDirectFilled {
                        event.last_qty = Quantity::from("2.000");
                    }
                }
                Some(event)
            } else {
                None
            };
            let message = if matches!(evidence, Evidence::RawFill(FillIngress::Standalone, _)) {
                ExecutionReport::Fill(Box::new(fills[0].clone()))
            } else if matches!(
                evidence,
                Evidence::MassAccepted | Evidence::MassFilled | Evidence::MassSubmittedReport
            ) {
                let mut mass = ExecutionMassStatus::new(
                    ClientId::from("BINANCE"),
                    AccountId::from("BINANCE-001"),
                    Venue::from("BINANCE"),
                    report.ts_init,
                    None,
                );
                mass.add_order_reports(vec![report]);
                mass.add_fill_reports(fills);
                ExecutionReport::MassStatus(Box::new(mass))
            } else if matches!(
                evidence,
                Evidence::SubmittedBareReport
                    | Evidence::PendingCancelReport
                    | Evidence::InvalidInstrumentBareReport
                    | Evidence::InvalidAccountBareReport
            ) {
                ExecutionReport::Order(Box::new(report))
            } else {
                ExecutionReport::OrderWithFills(Box::new(report), fills)
            };

            if matches!(
                evidence,
                Evidence::DirectAccepted
                    | Evidence::InvalidDirectAccepted
                    | Evidence::InvalidDirectInstrument
                    | Evidence::InvalidDirectTrader
                    | Evidence::InvalidOwnedAccepted
                    | Evidence::FallbackEventAccepted
                    | Evidence::FallbackBatchAccepted
            ) {
                let mut event = TestOrderEventStubs::accepted(
                    &order,
                    AccountId::from("BINANCE-001"),
                    VenueOrderId::new(format!("V-{}", order.client_order_id())),
                );

                if let OrderEventAny::Accepted(accepted) = &mut event {
                    if evidence == Evidence::InvalidDirectAccepted {
                        accepted.strategy_id = StrategyId::from("INVALID-001");
                    } else if evidence == Evidence::InvalidDirectInstrument {
                        accepted.instrument_id = "BTCUSDT.BINANCE".parse().unwrap();
                    } else if evidence == Evidence::InvalidDirectTrader {
                        accepted.trader_id = TraderId::from("INVALID-001");
                    } else if !matches!(
                        evidence,
                        Evidence::DirectAccepted | Evidence::InvalidOwnedAccepted
                    ) {
                        accepted.client_order_id = ClientOrderId::from("UNKNOWN-CLIENT-ID");
                    }
                }

                if evidence == Evidence::DirectAccepted {
                    get_exec_event_sender()
                        .send(ExecutionEvent::Order(event.clone()))
                        .unwrap();
                }
                let event = if evidence == Evidence::FallbackBatchAccepted {
                    let OrderEventAny::Accepted(accepted) = event else {
                        unreachable!()
                    };
                    ExecutionEvent::OrderAcceptedBatch(OrderAcceptedBatch::new(vec![accepted]))
                } else {
                    ExecutionEvent::Order(event)
                };

                get_exec_event_sender().send(event).unwrap();
            } else if matches!(
                evidence,
                Evidence::DirectTriggered | Evidence::DirectExpired
            ) {
                let account_id = AccountId::from("BINANCE-001");
                let venue_order_id = VenueOrderId::new(format!("V-{}", order.client_order_id()));
                let event = if evidence == Evidence::DirectTriggered {
                    OrderEventAny::Triggered(
                        OrderTriggeredSpec::builder()
                            .trader_id(order.trader_id())
                            .strategy_id(order.strategy_id())
                            .instrument_id(order.instrument_id())
                            .client_order_id(order.client_order_id())
                            .account_id(account_id)
                            .venue_order_id(venue_order_id)
                            .build(),
                    )
                } else {
                    OrderEventAny::Expired(
                        OrderExpiredSpec::builder()
                            .trader_id(order.trader_id())
                            .strategy_id(order.strategy_id())
                            .instrument_id(order.instrument_id())
                            .client_order_id(order.client_order_id())
                            .account_id(account_id)
                            .venue_order_id(venue_order_id)
                            .build(),
                    )
                };

                if evidence == Evidence::DirectTriggered {
                    get_exec_event_sender()
                        .send(ExecutionEvent::Order(event.clone()))
                        .unwrap();
                }
                get_exec_event_sender()
                    .send(ExecutionEvent::Order(event))
                    .unwrap();
            } else if let Some(event) = direct_fill {
                get_exec_event_sender()
                    .send(ExecutionEvent::Order(event))
                    .unwrap();
            } else if evidence == Evidence::FallbackBatchCanceled {
                let event = TestOrderEventStubs::canceled(
                    &order,
                    AccountId::from("BINANCE-001"),
                    Some(VenueOrderId::new(format!("V-{}", order.client_order_id()))),
                );
                let OrderEventAny::Canceled(mut canceled) = event else {
                    unreachable!()
                };
                canceled.client_order_id = ClientOrderId::from("UNKNOWN-CLIENT-ID");
                get_exec_event_sender()
                    .send(ExecutionEvent::OrderCanceledBatch(OrderCanceledBatch::new(
                        vec![canceled],
                    )))
                    .unwrap();
            } else if matches!(evidence, Evidence::FillVoided | Evidence::DirectVoid(_)) {
                if evidence == Evidence::FillVoided {
                    let pending = OrderPendingUpdateSpec::builder()
                        .trader_id(order.trader_id())
                        .strategy_id(order.strategy_id())
                        .instrument_id(order.instrument_id())
                        .client_order_id(order.client_order_id())
                        .account_id(AccountId::from("BINANCE-001"))
                        .build();
                    get_exec_event_sender()
                        .send(ExecutionEvent::Order(OrderEventAny::PendingUpdate(pending)))
                        .unwrap();
                }
                let mut voided = OrderFillVoidedSpec::builder()
                    .trader_id(order.trader_id())
                    .strategy_id(order.strategy_id())
                    .instrument_id(order.instrument_id())
                    .client_order_id(order.client_order_id())
                    .venue_order_id(VenueOrderId::new(format!("V-{}", order.client_order_id())))
                    .account_id(AccountId::from("BINANCE-001"))
                    .order_side(order.order_side())
                    .order_type(order.order_type())
                    .voided_qty(Quantity::from("0.400"))
                    .last_px(Price::from("100.00"))
                    .currency(Currency::USDT())
                    .build();

                if let Evidence::DirectVoid(mismatch) = evidence {
                    match mismatch {
                        FillMismatch::Instrument => {
                            voided.instrument_id = currency_pair_btcusdt().id();
                        }
                        FillMismatch::Side => voided.order_side = OrderSide::Sell,
                        FillMismatch::Type => voided.order_type = OrderType::Market,
                        FillMismatch::Zero => voided.voided_qty = Quantity::from("0.000"),
                        _ => {}
                    }
                }

                for _ in 0..2 {
                    get_exec_event_sender()
                        .send(ExecutionEvent::Order(OrderEventAny::FillVoided(
                            voided.clone(),
                        )))
                        .unwrap();
                }
            } else if !matches!(
                evidence,
                Evidence::BulkAccepted | Evidence::BulkTriggered | Evidence::BulkExpired
            ) {
                get_exec_event_sender()
                    .send(ExecutionEvent::Report(message.clone()))
                    .unwrap();
            }
            wait_until_async(
                || async {
                    let cache = cache.borrow();
                    let order = cache.order(&client_order_id).unwrap();
                    order.status() == expected_status
                        && (invalid_evidence
                            || !matches!(status, OrderStatus::Accepted | OrderStatus::Triggered)
                            || pending_command.is_none()
                            || order.events().iter().any(|event| {
                                matches!(
                                    (status, event),
                                    (OrderStatus::Accepted, OrderEventAny::Accepted(_))
                                        | (OrderStatus::Triggered, OrderEventAny::Triggered(_))
                                )
                            }))
                },
                Duration::from_secs(2),
            )
            .await;

            if !invalid_evidence
                && !matches!(
                    evidence,
                    Evidence::FallbackBatchAccepted
                        | Evidence::FallbackBatchCanceled
                        | Evidence::FillVoided
                        | Evidence::DirectVoid(_)
                        | Evidence::BulkAccepted
                        | Evidence::BulkTriggered
                        | Evidence::BulkExpired
                )
            {
                get_exec_event_sender()
                    .send(ExecutionEvent::Report(message))
                    .unwrap();
            }

            if pending_command.is_some()
                && matches!(
                    evidence,
                    Evidence::BulkAccepted | Evidence::DirectAccepted | Evidence::DirectTriggered
                )
            {
                wait_until_async(
                    || async { state.queries.borrow().len() > queries_at_exhaustion },
                    Duration::from_secs(2),
                )
                .await;
            }

            if evidence == Evidence::PartialCanceled {
                let (report, fills) = venue_evidence(&order, OrderStatus::Canceled, filled);
                state.report.replace(Some(report.clone()));
                get_exec_event_sender()
                    .send(ExecutionEvent::Report(ExecutionReport::OrderWithFills(
                        Box::new(report),
                        fills,
                    )))
                    .unwrap();
                wait_until_async(
                    || async {
                        cache.borrow().order(&client_order_id).unwrap().status()
                            == OrderStatus::Canceled
                    },
                    Duration::from_secs(2),
                )
                .await;
            }

            if evidence == Evidence::ShutdownFill {
                wait_until_async(
                    || async {
                        cache
                            .borrow()
                            .positions_open(None, None, Some(&strategy_id), None, None)
                            .is_empty()
                            && state.submitted_ids.borrow().len() == 2
                    },
                    Duration::from_secs(2),
                )
                .await;
            }
        }
        handle.stop();
    };
    let (result, ()) = tokio::join!(node.run(), driver);

    if retain && (evidence == Evidence::None || pending_report) && !definitive_denial {
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Incomplete submission recovery")
        );
        assert_eq!(
            node.exec_manager().unresolved_submission_ids(),
            vec![client_order_id]
        );
        assert_eq!(
            node.exec_manager()
                .submission_recovery_exhaustion(&client_order_id)
                .is_some(),
            !stop_during_connect,
        );
    } else {
        result.unwrap();
        assert!(node.exec_manager().unresolved_submission_ids().is_empty());
        assert!(
            node.exec_manager()
                .submission_recovery_exhaustion(&client_order_id)
                .is_none()
        );
    }
    let notifications = exhausted.get_messages();
    let should_exhaust =
        retain && query_outcome.is_none() && !stop_during_connect && !definitive_denial;
    assert_eq!(notifications.len(), usize::from(should_exhaust));
    if should_exhaust {
        assert_eq!(notifications[0].client_order_id, client_order_id);
        if !matches!(path, RecoveryPath::Inflight | RecoveryPath::Both) {
            assert_eq!(
                notifications[0].source,
                SubmissionRecoverySource::MissingOrder
            );
        } else if path == RecoveryPath::Inflight {
            assert_eq!(notifications[0].source, SubmissionRecoverySource::Inflight);
            assert_eq!(notifications[0].retry_count, 3);
        }
    }
    assert_eq!(state.submitted.borrow().is_some(), !definitive_denial);
    if definitive_denial && !retain {
        let status = cache
            .borrow()
            .order(&client_order_id)
            .map(|order| order.status());
        assert_eq!(
            status,
            if no_route && cached_missing_instrument {
                Some(OrderStatus::Denied)
            } else if no_route {
                None
            } else {
                Some(OrderStatus::Initialized)
            },
        );
        assert!(state.submitted_ids.borrow().is_empty());
        return;
    }

    if purge_denied {
        assert!(cache.borrow().order(&client_order_id).is_none());
        assert!(state.submitted_ids.borrow().is_empty());
        return;
    }
    let final_order = cache.borrow().order_owned(&client_order_id).unwrap();

    if triggered_evidence && let Some(pending) = pending_command {
        assert_eq!(final_order.status(), pending);
        assert_eq!(final_order.previous_status(), Some(OrderStatus::Triggered));
        let venue_id = VenueOrderId::new(format!("V-{}", final_order.client_order_id()));
        assert_eq!(final_order.venue_order_id(), Some(venue_id));
        assert_eq!(
            cache
                .borrow()
                .venue_order_id(&final_order.client_order_id()),
            Some(&venue_id)
        );
        assert_eq!(
            cache.borrow().client_order_id(&venue_id),
            Some(&final_order.client_order_id())
        );
        assert_eq!(
            final_order
                .events()
                .iter()
                .filter(|event| matches!(event, OrderEventAny::Triggered(_)))
                .count(),
            1
        );
    }

    if matches!(
        evidence,
        Evidence::Accepted
            | Evidence::DirectAccepted
            | Evidence::BulkAccepted
            | Evidence::QueryAccepted
            | Evidence::QueryAcceptedWithInflight
    ) && let Some(pending) = pending_command
    {
        assert_eq!(final_order.status(), pending);
        assert_eq!(final_order.previous_status(), Some(OrderStatus::Accepted));
        assert_eq!(
            final_order
                .events()
                .iter()
                .filter(|event| matches!(event, OrderEventAny::Accepted(_)))
                .count(),
            1
        );
        assert!(final_order.events().iter().any(|event| match event {
            OrderEventAny::PendingCancel(event) =>
                event.reconciliation && event.causation_id.is_some(),
            OrderEventAny::PendingUpdate(event) =>
                event.reconciliation && event.causation_id.is_some(),
            _ => false,
        }));
    }

    if evidence == Evidence::DirectVoid(FillMismatch::None) {
        assert_eq!(final_order.status(), OrderStatus::Voided);
        assert_eq!(
            final_order
                .events()
                .iter()
                .filter(|event| matches!(event, OrderEventAny::FillVoided(_)))
                .count(),
            1
        );
        assert_eq!(
            final_order
                .events()
                .iter()
                .filter(|event| matches!(event, OrderEventAny::Accepted(_)))
                .count(),
            1
        );
        assert!(
            cache
                .borrow()
                .positions(None, None, None, None, None)
                .is_empty()
        );
    }

    if path == RecoveryPath::BulkFillTimeout {
        assert_eq!(state.fill_queries.get(), 3);
        assert_eq!(exhausted.get_messages()[0].retry_count, 3);
    }

    if invalid_evidence {
        assert!(
            cache
                .borrow()
                .positions(None, None, None, None, None)
                .is_empty()
        );
        assert_eq!(
            final_order.account_id(),
            (origin != SubmissionOrigin::RestoredReleased).then(|| AccountId::from("BINANCE-001"))
        );
        assert_eq!(
            final_order
                .events()
                .iter()
                .filter(|event| matches!(event, OrderEventAny::Submitted(_)))
                .count(),
            usize::from(origin != SubmissionOrigin::RestoredReleased)
        );
    }

    if evidence == Evidence::None || pending_report {
        assert_eq!(
            final_order.status(),
            if definitive_denial {
                OrderStatus::Denied
            } else if origin == SubmissionOrigin::RestoredReleased {
                OrderStatus::Released
            } else if origin == SubmissionOrigin::RestoredPendingCancel {
                OrderStatus::PendingCancel
            } else if origin == SubmissionOrigin::RestoredPendingUpdate {
                OrderStatus::PendingUpdate
            } else if retain || stop_during_connect || default_startup_inflight {
                match path {
                    RecoveryPath::MissingPendingCancel => OrderStatus::PendingCancel,
                    RecoveryPath::MissingPendingUpdate => OrderStatus::PendingUpdate,
                    _ => OrderStatus::Submitted,
                }
            } else {
                OrderStatus::Rejected
            }
        );
    }

    if matches!(
        evidence,
        Evidence::Filled
            | Evidence::DirectFilled
            | Evidence::MassFilled
            | Evidence::PartialCanceled
            | Evidence::ShutdownFill
            | Evidence::PendingReportWithFills
            | Evidence::PendingReportWithLaggingQuantity
            | Evidence::RawFill(_, FillMismatch::None)
    ) {
        assert_eq!(
            final_order
                .events()
                .iter()
                .filter(|event| matches!(event, OrderEventAny::Filled(_)))
                .count(),
            1
        );
        assert_eq!(
            final_order.filled_qty(),
            if evidence == Evidence::PartialCanceled {
                Quantity::from("0.400")
            } else {
                Quantity::from("1.000")
            }
        );
    }

    if path == RecoveryPath::Inflight && (retain || !definitive_denial) {
        let expected_queries =
            if stop_during_connect || definitive_denial || default_startup_inflight {
                0
            } else {
                2
            };

        if pending_command.is_some()
            && matches!(
                evidence,
                Evidence::Accepted | Evidence::DirectAccepted | Evidence::DirectTriggered
            )
        {
            // Applied acceptance re-enables ordinary recovery of the outstanding command.
            assert!(state.queries.borrow().len() >= expected_queries);
            assert!(
                state
                    .queries
                    .borrow()
                    .iter()
                    .all(|id| *id == client_order_id)
            );
        } else {
            assert_eq!(
                *state.queries.borrow(),
                vec![client_order_id; expected_queries]
            );
        }
    }

    if evidence == Evidence::ShutdownFill {
        assert_eq!(state.submitted_ids.borrow().len(), 2);
        assert!(
            cache
                .borrow()
                .positions_open(None, None, Some(&strategy_id), None, None)
                .is_empty()
        );
    } else {
        let expected = if restored || native_ingress || definitive_denial {
            vec![]
        } else {
            vec![client_order_id]
        };
        assert_eq!(*state.submitted_ids.borrow(), expected);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StartupFailure {
    DataTimeout,
    ExecutionTimeout,
    Readiness,
    Strategy,
}

#[rstest]
#[tokio::test]
async fn startup_failure_includes_unresolved_identity_after_drain(
    #[values(
        StartupFailure::DataTimeout,
        StartupFailure::ExecutionTimeout,
        StartupFailure::Readiness,
        StartupFailure::Strategy
    )]
    failure: StartupFailure,
    #[values(false, true)] standalone_start: bool,
    #[values(false, true)] retain: bool,
    #[values(false, true)] disconnect_evidence: bool,
) {
    let data_phase = failure == StartupFailure::DataTimeout;
    let state = Rc::new(SubmissionState {
        connection_failure: if failure == StartupFailure::Strategy {
            None
        } else {
            Some(failure != StartupFailure::Readiness)
        },
        disconnect_evidence,
        ..Default::default()
    });
    let mut builder = LiveNodeBuilder::from_config(LiveNodeConfig {
        exec_engine: LiveExecutionEngineConfig {
            reconciliation: false,
            submitted_order_exhaustion_policy: if retain {
                SubmittedOrderExhaustionPolicy::RetainUnresolved
            } else {
                SubmittedOrderExhaustionPolicy::ResolveLocally
            },
            ..Default::default()
        },
        timeout_connection: Duration::from_millis(20),
        delay_post_stop: Duration::ZERO,
        logging: LoggerConfig {
            bypass_logging: true,
            ..Default::default()
        },
        ..Default::default()
    })
    .unwrap()
    .add_exec_client(
        Some("BINANCE".into()),
        Box::new(SubmissionClientFactory(state.clone())),
        Box::new(SubmissionClientConfig),
    )
    .unwrap();

    if data_phase {
        builder = builder
            .add_data_client(
                Some("BINANCE".into()),
                Box::new(SubmissionDataClientFactory(state.clone())),
                Box::new(SubmissionClientConfig),
            )
            .unwrap();
    }
    let mut node = builder.build().unwrap();
    let instrument = crypto_perpetual_ethusdt();
    let mut order = OrderTestBuilder::new(OrderType::Limit)
        .trader_id(node.trader_id())
        .instrument_id(instrument.id())
        .side(OrderSide::Buy)
        .quantity(Quantity::from("1.000"))
        .price(Price::from("100.00"))
        .build();
    order
        .apply(TestOrderEventStubs::submitted(
            &order,
            AccountId::from("BINANCE-001"),
        ))
        .unwrap();
    let id = order.client_order_id();
    state.submitted.replace(Some(order.clone()));
    node.kernel()
        .cache()
        .borrow_mut()
        .add_instrument(instrument.into())
        .unwrap();
    node.kernel()
        .cache()
        .borrow_mut()
        .add_order(order, None, None, false)
        .unwrap();

    if failure == StartupFailure::Strategy {
        node.add_strategy(SubmissionStrategy {
            core: StrategyCore::new(StrategyConfig {
                strategy_id: Some(StrategyId::from("FAIL-001")),
                ..Default::default()
            }),
            order: state.submitted.borrow().clone().unwrap(),
            origin: SubmissionOrigin::FailStart,
        })
        .unwrap();
    }
    let result = if standalone_start {
        node.start().await
    } else {
        node.run().await
    };
    let error = result.unwrap_err().to_string();
    assert!(
        error.contains(if failure == StartupFailure::Strategy {
            "strategy startup failure"
        } else {
            "timeout"
        }),
        "{error}"
    );
    assert_eq!(
        error.contains("Incomplete submission recovery"),
        retain && !disconnect_evidence,
        "{error}"
    );

    if retain && !disconnect_evidence {
        assert!(error.contains(id.as_str()));
        assert_eq!(node.exec_manager().unresolved_submission_ids(), vec![id]);
    } else {
        assert!(node.exec_manager().unresolved_submission_ids().is_empty());
    }
    assert_eq!(node.handle().state(), NodeState::Stopped);
}

struct SubmissionDataClient(SubmissionClient);
#[derive(Debug)]
struct SubmissionDataClientFactory(Rc<SubmissionState>);
impl DataClientFactory for SubmissionDataClientFactory {
    fn create(
        &self,
        _name: &str,
        _config: &dyn ClientConfig,
        cache: CacheView,
        _clock: Rc<RefCell<dyn Clock>>,
    ) -> anyhow::Result<Box<dyn DataClient>> {
        Ok(Box::new(SubmissionDataClient(SubmissionClient {
            connected: Cell::new(false),
            cache,
            state: self.0.clone(),
        })))
    }
    fn name(&self) -> &'static str {
        "SubmissionDataClientFactory"
    }
    fn config_type(&self) -> &'static str {
        "SubmissionClientConfig"
    }
}
#[async_trait(?Send)]
impl DataClient for SubmissionDataClient {
    fn client_id(&self) -> ClientId {
        ExecutionClient::client_id(&self.0)
    }
    fn venue(&self) -> Option<Venue> {
        Some(ExecutionClient::venue(&self.0))
    }
    fn start(&mut self) -> anyhow::Result<()> {
        ExecutionClient::start(&mut self.0)
    }
    fn stop(&mut self) -> anyhow::Result<()> {
        ExecutionClient::stop(&mut self.0)
    }
    fn reset(&mut self) -> anyhow::Result<()> {
        Ok(())
    }
    fn dispose(&mut self) -> anyhow::Result<()> {
        Ok(())
    }
    fn is_connected(&self) -> bool {
        self.0.connected.get()
    }
    fn is_disconnected(&self) -> bool {
        !self.0.connected.get()
    }
    async fn connect(&mut self) -> anyhow::Result<()> {
        ExecutionClient::connect(&mut self.0).await
    }
    async fn disconnect(&mut self) -> anyhow::Result<()> {
        ExecutionClient::disconnect(&mut self.0).await
    }
}

#[rstest]
#[case::standalone_valid(FillIngress::Standalone, FillMismatch::None)]
#[case::standalone_instrument(FillIngress::Standalone, FillMismatch::Instrument)]
#[case::standalone_side(FillIngress::Standalone, FillMismatch::Side)]
#[case::bundled_valid(FillIngress::Bundled, FillMismatch::None)]
#[case::bundled_instrument(FillIngress::Bundled, FillMismatch::Instrument)]
#[case::bundled_side(FillIngress::Bundled, FillMismatch::Side)]
#[case::bundled_account(FillIngress::Bundled, FillMismatch::Account)]
#[case::bundled_venue(FillIngress::Bundled, FillMismatch::Venue)]
#[case::direct_instrument(FillIngress::Direct, FillMismatch::Instrument)]
#[case::direct_side(FillIngress::Direct, FillMismatch::Side)]
#[case::direct_type(FillIngress::Direct, FillMismatch::Type)]
#[case::direct_zero(FillIngress::Direct, FillMismatch::Zero)]
#[tokio::test]
async fn retained_fill_recovery_validates_original_evidence(
    #[values(RecoveryPath::Inflight, RecoveryPath::MissingOrder)] path: RecoveryPath,
    #[case] ingress: FillIngress,
    #[case] mismatch: FillMismatch,
    #[values(
        SubmissionOrigin::RestoredReleased,
        SubmissionOrigin::RestoredSubmitted,
        SubmissionOrigin::RestoredPendingCancel,
        SubmissionOrigin::RestoredPendingUpdate
    )]
    origin: SubmissionOrigin,
) {
    run_submission_case(
        true,
        path,
        Evidence::RawFill(ingress, mismatch),
        origin,
        false,
    )
    .await;
}
