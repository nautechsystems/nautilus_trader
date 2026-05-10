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

//! Core execution client implementation for Interactive Brokers.

#[path = "core_helpers.rs"]
mod core_helpers;
#[path = "core_orders.rs"]
mod core_orders;
#[path = "core_updates.rs"]
mod core_updates;
#[cfg(test)]
#[path = "core_tests.rs"]
mod tests;

#[cfg(feature = "python")]
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    rc::Rc,
};
use std::{
    collections::VecDeque,
    fmt::Debug,
    str::FromStr,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use ahash::AHashMap;
use anyhow::Context;
// removed unused async_trait
use ibapi::{
    accounts::PositionUpdate,
    client::Client,
    orders::{
        ExecutionData, ExecutionFilter, Executions, OcaType, OrderStatus as IBOrderStatus,
        OrderUpdate, Orders,
    },
};
use nautilus_common::{
    cache::Cache,
    clients::ExecutionClient,
    enums::LogLevel,
    factories::OrderEventFactory,
    live::{get_runtime, runner::get_exec_event_sender},
    messages::{
        ExecutionEvent,
        execution::{
            BatchCancelOrders, CancelAllOrders, CancelOrder, ExecutionReport, GenerateFillReports,
            GenerateFillReportsBuilder, GenerateOrderStatusReport, GenerateOrderStatusReports,
            GenerateOrderStatusReportsBuilder, GeneratePositionStatusReports,
            GeneratePositionStatusReportsBuilder, ModifyOrder, QueryAccount, QueryOrder,
            SubmitOrder, SubmitOrderList,
        },
    },
    msgbus::{send_account_state, switchboard::MessagingSwitchboard},
};
use nautilus_core::{
    UUID4, UnixNanos,
    time::{AtomicTime, get_atomic_clock_realtime},
};
use nautilus_live::ExecutionClientCore;
#[cfg(feature = "python")]
use nautilus_model::events::{OrderAcceptedBatch, OrderCanceledBatch, OrderSubmittedBatch};
#[cfg(feature = "python")]
use nautilus_model::identifiers::{ExecAlgorithmId, OrderListId, PositionId};
#[cfg(feature = "python")]
use nautilus_model::orders::OrderList;
#[cfg(feature = "python")]
use nautilus_model::python::events::order::order_event_to_pyobject;
use nautilus_model::{
    accounts::AccountAny,
    enums::{
        LiquiditySide, OmsType, OrderSide, OrderType, PositionSideSpecified, TrailingOffsetType,
    },
    events::{
        AccountState, OrderAccepted, OrderCanceled, OrderEventAny, OrderPendingCancel,
        OrderRejected, OrderSubmitted,
    },
    identifiers::{
        AccountId, ClientId, ClientOrderId, InstrumentId, StrategyId, TradeId, TraderId, Venue,
        VenueOrderId,
    },
    instruments::Instrument,
    orders::{Order, any::OrderAny},
    reports::{ExecutionMassStatus, FillReport, OrderStatusReport, PositionStatusReport},
    types::{AccountBalance, Currency, MarginBalance, Money, Price, Quantity},
};
#[cfg(feature = "python")]
use nautilus_model::{enums::AccountType, events::OrderInitialized};
#[cfg(feature = "python")]
use pyo3::{IntoPyObjectExt, prelude::*};
use rust_decimal::Decimal;
use tokio::task::JoinHandle;
use ustr::Ustr;

use super::{
    account::{PositionTracker, create_position_tracker},
    parse::{parse_execution_time, parse_execution_to_fill_report, parse_order_status_to_report},
    transform::nautilus_order_to_ib_order,
};
#[cfg(feature = "python")]
use crate::common::consts::IB_VENUE;
use crate::{
    common::{
        parse::{ib_contract_to_instrument_id_simple, is_spread_instrument_id},
        shared_client::SharedClientHandle,
    },
    config::InteractiveBrokersExecClientConfig,
    providers::instruments::InteractiveBrokersInstrumentProvider,
};

/// Interactive Brokers execution client.
///
/// This client provides order execution functionality using the `rust-ibapi` library.
/// It manages order submission, modification, cancellation, and execution reporting.
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.interactive_brokers",
        unsendable
    )
)]
pub struct InteractiveBrokersExecutionClient {
    /// Core execution client functionality.
    core: ExecutionClientCore,
    /// Configuration for the client.
    config: InteractiveBrokersExecClientConfig,
    /// Instrument provider.
    instrument_provider: Arc<InteractiveBrokersInstrumentProvider>,
    /// Connection state.
    is_connected: AtomicBool,
    /// IB API client (shared per host/port/client_id when both data and execution connect).
    ib_client: Option<SharedClientHandle>,
    /// Active task handles.
    pending_tasks: Mutex<Vec<JoinHandle<()>>>,
    /// Order ID counter.
    next_order_id: Arc<Mutex<i32>>,
    /// Order update subscription handle.
    order_update_handle: Mutex<Option<JoinHandle<()>>>,
    /// Client order ID to venue order ID mapping.
    order_id_map: Arc<Mutex<AHashMap<ClientOrderId, i32>>>,
    /// Venue order ID to client order ID mapping.
    venue_order_id_map: Arc<Mutex<AHashMap<i32, ClientOrderId>>>,
    /// Commission cache by execution ID (to merge with fill reports).
    commission_cache: Arc<Mutex<AHashMap<String, (f64, String)>>>,
    /// Instrument ID mapping by venue order ID (for order status tracking).
    instrument_id_map: Arc<Mutex<AHashMap<i32, InstrumentId>>>,
    /// Trader ID mapping by venue order ID.
    trader_id_map: Arc<Mutex<AHashMap<i32, TraderId>>>,
    /// Strategy ID mapping by venue order ID.
    strategy_id_map: Arc<Mutex<AHashMap<i32, StrategyId>>>,
    /// Spread fill tracking to avoid duplicate processing.
    /// Maps client_order_id to set of trade_ids that have been processed.
    spread_fill_tracking: Arc<Mutex<AHashMap<ClientOrderId, ahash::AHashSet<String>>>>,
    /// Position tracker for detecting external position changes (e.g., option exercises).
    position_tracker: PositionTracker,
    /// Average fill price tracking by client order ID.
    /// Stores average fill prices from IB order status updates for use in fill reports.
    order_avg_prices: Arc<Mutex<AHashMap<ClientOrderId, Price>>>,
    /// Pending spread combo fills waiting for their matching avg fill price chunk.
    pending_combo_fills: Arc<Mutex<AHashMap<ClientOrderId, VecDeque<PendingComboFill>>>>,
    /// Pending average-price chunks derived from cumulative order status updates.
    pending_combo_fill_avgs: Arc<Mutex<AHashMap<ClientOrderId, VecDeque<(Decimal, Price)>>>>,
    /// Tracks cumulative filled quantity and notional for deriving incremental avg fill chunks.
    order_fill_progress: Arc<Mutex<AHashMap<ClientOrderId, (Decimal, Decimal)>>>,
    /// Set of client order IDs that have already emitted an OrderAccepted event.
    accepted_orders: Arc<Mutex<ahash::AHashSet<ClientOrderId>>>,
    /// Set of client order IDs that have already emitted an OrderPendingCancel event.
    pending_cancel_orders: Arc<Mutex<ahash::AHashSet<ClientOrderId>>>,
}

#[derive(Clone, Debug)]
struct PendingComboFill {
    account_id: AccountId,
    instrument_id: InstrumentId,
    venue_order_id: VenueOrderId,
    trade_id: TradeId,
    order_side: OrderSide,
    last_qty: Quantity,
    last_px: Price,
    commission: Money,
    liquidity_side: LiquiditySide,
    client_order_id: ClientOrderId,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
}

#[cfg(feature = "python")]
static EXEC_EVENT_CALLBACK: std::sync::OnceLock<std::sync::Mutex<Option<Py<PyAny>>>> =
    std::sync::OnceLock::new();

#[cfg(feature = "python")]
thread_local! {
    static EXEC_EVENT_BRIDGE_INITIALIZED: Cell<bool> = const { Cell::new(false) };
}

#[cfg(feature = "python")]
fn exec_event_callback() -> &'static std::sync::Mutex<Option<Py<PyAny>>> {
    EXEC_EVENT_CALLBACK.get_or_init(|| std::sync::Mutex::new(None))
}

#[cfg(feature = "python")]
fn string_hash_map_to_params(
    params: Option<HashMap<String, String>>,
) -> Option<nautilus_core::Params> {
    params.map(|items| {
        let mut mapped = nautilus_core::Params::new();
        for (key, value) in items {
            mapped.insert(key, serde_json::Value::String(value));
        }
        mapped
    })
}

#[cfg(feature = "python")]
fn dispatch_python_exec_event(
    py: Python<'_>,
    callback: &Py<PyAny>,
    event: ExecutionEvent,
) -> PyResult<()> {
    let (kind, payload) = match event {
        ExecutionEvent::Order(order_event) => {
            ("order_event", order_event_to_pyobject(py, order_event)?)
        }
        ExecutionEvent::OrderSubmittedBatch(batch) => (
            "order_submitted_batch",
            order_submitted_batch_to_pyobject(py, batch)?,
        ),
        ExecutionEvent::OrderAcceptedBatch(batch) => (
            "order_accepted_batch",
            order_accepted_batch_to_pyobject(py, batch)?,
        ),
        ExecutionEvent::OrderCanceledBatch(batch) => (
            "order_canceled_batch",
            order_canceled_batch_to_pyobject(py, batch)?,
        ),
        ExecutionEvent::Report(report) => match report {
            ExecutionReport::Order(report) => ("order_report", (*report).into_py_any(py)?),
            ExecutionReport::Fill(report) => ("fill_report", (*report).into_py_any(py)?),
            ExecutionReport::Position(report) => ("position_report", (*report).into_py_any(py)?),
            ExecutionReport::MassStatus(report) => {
                ("mass_status_report", (*report).into_py_any(py)?)
            }
            // The IB adapter never emits OrderWithFills; this arm exists only to
            // keep the match exhaustive against the shared ExecutionReport enum.
            ExecutionReport::OrderWithFills(..) => return Ok(()),
        },
        ExecutionEvent::Account(account_state) => ("account_state", account_state.into_py_any(py)?),
    };

    callback.call1(py, (kind, payload))?;
    Ok(())
}

#[cfg(feature = "python")]
fn order_accepted_batch_to_pyobject(
    py: Python<'_>,
    batch: OrderAcceptedBatch,
) -> PyResult<Py<PyAny>> {
    batch
        .into_iter()
        .map(|event| order_event_to_pyobject(py, OrderEventAny::Accepted(event)))
        .collect::<PyResult<Vec<Py<PyAny>>>>()?
        .into_py_any(py)
}

#[cfg(feature = "python")]
fn order_submitted_batch_to_pyobject(
    py: Python<'_>,
    batch: OrderSubmittedBatch,
) -> PyResult<Py<PyAny>> {
    batch
        .into_iter()
        .map(|event| order_event_to_pyobject(py, OrderEventAny::Submitted(event)))
        .collect::<PyResult<Vec<Py<PyAny>>>>()?
        .into_py_any(py)
}

#[cfg(feature = "python")]
fn order_canceled_batch_to_pyobject(
    py: Python<'_>,
    batch: OrderCanceledBatch,
) -> PyResult<Py<PyAny>> {
    batch
        .into_iter()
        .map(|event| order_event_to_pyobject(py, OrderEventAny::Canceled(event)))
        .collect::<PyResult<Vec<Py<PyAny>>>>()?
        .into_py_any(py)
}

impl Debug for InteractiveBrokersExecutionClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(InteractiveBrokersExecutionClient))
            .field("core", &self.core)
            .field("config", &self.config)
            .field("instrument_provider", &self.instrument_provider)
            .field("is_connected", &self.is_connected.load(Ordering::Relaxed))
            .field("ib_client", &self.ib_client.is_some())
            .finish_non_exhaustive()
    }
}

impl InteractiveBrokersExecutionClient {
    /// Creates a new [`InteractiveBrokersExecutionClient`].
    ///
    /// # Arguments
    ///
    /// * `core` - Core execution client functionality
    /// * `config` - Configuration for the client
    /// * `instrument_provider` - Instrument provider
    ///
    /// # Errors
    ///
    /// Returns an error if client creation fails.
    pub fn new(
        mut core: ExecutionClientCore,
        config: InteractiveBrokersExecClientConfig,
        instrument_provider: Arc<InteractiveBrokersInstrumentProvider>,
    ) -> anyhow::Result<Self> {
        // If account_id is provided in config, use it
        if let Some(account_id) = &config.account_id {
            core.account_id = AccountId::from(account_id.clone());
        }

        Ok(Self {
            core,
            config,
            instrument_provider,
            is_connected: AtomicBool::new(false),
            ib_client: None,
            pending_tasks: Mutex::new(Vec::new()),
            next_order_id: Arc::new(Mutex::new(0)),
            order_update_handle: Mutex::new(None),
            order_id_map: Arc::new(Mutex::new(AHashMap::new())),
            venue_order_id_map: Arc::new(Mutex::new(AHashMap::new())),
            commission_cache: Arc::new(Mutex::new(AHashMap::new())),
            instrument_id_map: Arc::new(Mutex::new(AHashMap::new())),
            trader_id_map: Arc::new(Mutex::new(AHashMap::new())),
            strategy_id_map: Arc::new(Mutex::new(AHashMap::new())),
            spread_fill_tracking: Arc::new(Mutex::new(AHashMap::new())),
            position_tracker: create_position_tracker(),
            order_avg_prices: Arc::new(Mutex::new(AHashMap::new())),
            pending_combo_fills: Arc::new(Mutex::new(AHashMap::new())),
            pending_combo_fill_avgs: Arc::new(Mutex::new(AHashMap::new())),
            order_fill_progress: Arc::new(Mutex::new(AHashMap::new())),
            accepted_orders: Arc::new(Mutex::new(ahash::AHashSet::new())),
            pending_cancel_orders: Arc::new(Mutex::new(ahash::AHashSet::new())),
        })
    }

    #[cfg(feature = "python")]
    pub(crate) fn new_for_python(
        mut config: InteractiveBrokersExecClientConfig,
        instrument_provider: crate::providers::instruments::InteractiveBrokersInstrumentProvider,
    ) -> anyhow::Result<Self> {
        Self::ensure_python_event_bridge();

        let account_id_value = config
            .account_id
            .clone()
            .unwrap_or_else(|| "UNKNOWN".to_string());
        let normalized_account_id = if account_id_value.starts_with("IB-") {
            account_id_value
        } else {
            format!("IB-{account_id_value}")
        };

        config.account_id = Some(normalized_account_id.clone());

        let core = ExecutionClientCore::new(
            TraderId::from("TRADER-001"),
            ClientId::from("IB"),
            *IB_VENUE,
            OmsType::Netting,
            AccountId::from(normalized_account_id),
            AccountType::Margin,
            None,
            Rc::new(RefCell::new(Cache::default())),
        );

        Self::new(core, config, Arc::new(instrument_provider))
    }

    #[cfg(feature = "python")]
    pub(crate) fn register_python_event_callback(&self, callback: Py<PyAny>) {
        *exec_event_callback()
            .lock()
            .expect("execution event callback mutex poisoned") = Some(callback);
    }

    #[cfg(feature = "python")]
    fn ensure_python_event_bridge() {
        if nautilus_common::live::runner::try_get_exec_event_sender().is_some() {
            return;
        }

        EXEC_EVENT_BRIDGE_INITIALIZED.with(|initialized| {
            if initialized.replace(true) {
                return;
            }

            let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel::<ExecutionEvent>();
            nautilus_common::live::runner::set_exec_event_sender(sender);

            get_runtime().spawn(async move {
                while let Some(event) = receiver.recv().await {
                    Python::attach(|py| {
                        let callback_guard = exec_event_callback()
                            .lock()
                            .expect("execution event callback mutex poisoned");

                        let Some(callback) = callback_guard.as_ref() else {
                            return;
                        };

                        if let Err(e) = dispatch_python_exec_event(py, callback, event) {
                            tracing::error!("Failed to dispatch IB execution event to Python: {e}");
                        }
                    });
                }
            });
        });
    }

    #[cfg(feature = "python")]
    #[allow(clippy::needless_pass_by_value)]
    pub(crate) fn submit_order_for_python(
        &self,
        trader_id: TraderId,
        order: OrderAny,
        instrument_id: InstrumentId,
        strategy_id: StrategyId,
        exec_algorithm_id: Option<ExecAlgorithmId>,
        position_id: Option<PositionId>,
        params: Option<HashMap<String, String>>,
    ) -> anyhow::Result<()> {
        self.cache_order_for_python(order.clone(), position_id)?;

        let cmd = SubmitOrder {
            trader_id,
            client_id: Some(self.client_id()),
            strategy_id,
            instrument_id,
            client_order_id: order.client_order_id(),
            order_init: order.init_event().clone(),
            exec_algorithm_id,
            position_id,
            params: string_hash_map_to_params(params),
            command_id: UUID4::new(),
            ts_init: get_atomic_clock_realtime().get_time_ns(),
        };

        ExecutionClient::submit_order(self, cmd)
    }

    #[cfg(feature = "python")]
    #[allow(clippy::needless_pass_by_value)]
    pub(crate) fn submit_order_list_for_python(
        &self,
        trader_id: TraderId,
        strategy_id: StrategyId,
        orders: Vec<OrderAny>,
        exec_algorithm_id: Option<ExecAlgorithmId>,
        position_id: Option<PositionId>,
        params: Option<HashMap<String, String>>,
    ) -> anyhow::Result<()> {
        if orders.is_empty() {
            anyhow::bail!("Order list cannot be empty");
        }

        for order in &orders {
            self.cache_order_for_python(order.clone(), position_id)?;
        }

        let ts_init = get_atomic_clock_realtime().get_time_ns();
        let instrument_id = orders[0].instrument_id();
        let client_order_ids: Vec<ClientOrderId> =
            orders.iter().map(|o| o.client_order_id()).collect();
        let order_list_id = OrderListId::from(UUID4::new().to_string());
        let order_list = OrderList::new(
            order_list_id,
            instrument_id,
            strategy_id,
            client_order_ids,
            ts_init,
        );
        let order_inits: Vec<OrderInitialized> =
            orders.iter().map(|o| o.init_event().clone()).collect();

        let cmd = SubmitOrderList::new(
            trader_id,
            Some(self.client_id()),
            strategy_id,
            order_list,
            order_inits,
            exec_algorithm_id,
            position_id,
            string_hash_map_to_params(params),
            UUID4::new(),
            ts_init,
        );

        ExecutionClient::submit_order_list(self, cmd)
    }

    #[cfg(feature = "python")]
    pub(crate) fn modify_order_for_python(
        &self,
        trader_id: TraderId,
        strategy_id: StrategyId,
        client_order_id: ClientOrderId,
        venue_order_id: Option<VenueOrderId>,
        instrument_id: InstrumentId,
        quantity: Option<Quantity>,
        price: Option<Price>,
        trigger_price: Option<Price>,
        params: Option<HashMap<String, String>>,
    ) -> anyhow::Result<()> {
        let cmd = ModifyOrder {
            trader_id,
            client_id: Some(self.client_id()),
            strategy_id,
            instrument_id,
            client_order_id,
            venue_order_id,
            quantity,
            price,
            trigger_price,
            params: string_hash_map_to_params(params),
            command_id: UUID4::new(),
            ts_init: get_atomic_clock_realtime().get_time_ns(),
        };

        ExecutionClient::modify_order(self, cmd)
    }

    #[cfg(feature = "python")]
    pub(crate) fn cancel_order_for_python(
        &self,
        trader_id: TraderId,
        strategy_id: StrategyId,
        client_order_id: ClientOrderId,
        venue_order_id: Option<VenueOrderId>,
        instrument_id: InstrumentId,
        params: Option<HashMap<String, String>>,
    ) -> anyhow::Result<()> {
        let cmd = CancelOrder {
            trader_id,
            client_id: Some(self.client_id()),
            strategy_id,
            instrument_id,
            client_order_id,
            venue_order_id,
            params: string_hash_map_to_params(params),
            command_id: UUID4::new(),
            ts_init: get_atomic_clock_realtime().get_time_ns(),
        };

        ExecutionClient::cancel_order(self, cmd)
    }

    #[cfg(feature = "python")]
    pub(crate) fn cancel_all_orders_for_python(
        &self,
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        order_side: OrderSide,
        params: Option<HashMap<String, String>>,
    ) -> anyhow::Result<()> {
        let cmd = CancelAllOrders {
            trader_id,
            client_id: Some(self.client_id()),
            strategy_id,
            instrument_id,
            order_side,
            command_id: UUID4::new(),
            ts_init: get_atomic_clock_realtime().get_time_ns(),
            params: string_hash_map_to_params(params),
        };

        ExecutionClient::cancel_all_orders(self, cmd)
    }

    #[cfg(feature = "python")]
    pub(crate) fn batch_cancel_orders_for_python(
        &self,
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_ids: Vec<ClientOrderId>,
        params: Option<HashMap<String, String>>,
    ) -> anyhow::Result<()> {
        let ts_init = get_atomic_clock_realtime().get_time_ns();
        let cancels = client_order_ids
            .into_iter()
            .map(|client_order_id| CancelOrder {
                trader_id,
                client_id: Some(self.client_id()),
                strategy_id,
                instrument_id,
                client_order_id,
                venue_order_id: None,
                command_id: UUID4::new(),
                ts_init,
                params: None,
            })
            .collect();

        let cmd = BatchCancelOrders {
            trader_id,
            client_id: Some(self.client_id()),
            strategy_id,
            instrument_id,
            cancels,
            command_id: UUID4::new(),
            ts_init,
            params: string_hash_map_to_params(params),
        };

        ExecutionClient::batch_cancel_orders(self, cmd)
    }

    #[cfg(feature = "python")]
    pub(crate) fn query_account_for_python(&self, trader_id: TraderId) -> anyhow::Result<()> {
        let cmd = QueryAccount {
            trader_id,
            client_id: Some(self.client_id()),
            account_id: ExecutionClient::account_id(self),
            command_id: UUID4::new(),
            ts_init: get_atomic_clock_realtime().get_time_ns(),
            params: None,
        };

        ExecutionClient::query_account(self, cmd)
    }

    #[cfg(feature = "python")]
    pub(crate) fn query_order_for_python(
        &self,
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        venue_order_id: Option<VenueOrderId>,
    ) -> anyhow::Result<()> {
        let cmd = QueryOrder {
            trader_id,
            client_id: Some(self.client_id()),
            strategy_id,
            instrument_id,
            client_order_id,
            venue_order_id,
            command_id: UUID4::new(),
            ts_init: get_atomic_clock_realtime().get_time_ns(),
            params: None,
        };

        ExecutionClient::query_order(self, cmd)
    }

    #[cfg(feature = "python")]
    pub(crate) async fn generate_order_status_report_for_python(
        &self,
        instrument_id: Option<InstrumentId>,
        client_order_id: Option<ClientOrderId>,
        venue_order_id: Option<VenueOrderId>,
    ) -> anyhow::Result<Option<OrderStatusReport>> {
        let cmd = GenerateOrderStatusReport {
            command_id: UUID4::new(),
            ts_init: get_atomic_clock_realtime().get_time_ns(),
            instrument_id,
            client_order_id,
            venue_order_id,
            params: None,
            correlation_id: None,
        };

        self.generate_order_status_report(&cmd).await
    }

    #[cfg(feature = "python")]
    pub(crate) async fn generate_order_status_reports_for_python(
        &self,
        open_only: bool,
        instrument_id: Option<InstrumentId>,
        start: Option<u64>,
        end: Option<u64>,
    ) -> anyhow::Result<Vec<OrderStatusReport>> {
        let start_ns = start.map(nautilus_core::UnixNanos::from);
        let end_ns = end.map(nautilus_core::UnixNanos::from);

        let cmd = GenerateOrderStatusReports {
            command_id: UUID4::new(),
            ts_init: get_atomic_clock_realtime().get_time_ns(),
            open_only,
            instrument_id,
            start: start_ns,
            end: end_ns,
            params: None,
            log_receipt_level: LogLevel::Info,
            correlation_id: None,
        };

        let mut reports = self.generate_order_status_reports(&cmd).await?;

        if open_only {
            use nautilus_model::enums::OrderStatus;
            reports.retain(|report| {
                matches!(
                    report.order_status,
                    OrderStatus::Initialized
                        | OrderStatus::Submitted
                        | OrderStatus::Accepted
                        | OrderStatus::Triggered
                        | OrderStatus::PendingUpdate
                        | OrderStatus::PendingCancel
                )
            });
        }

        if start_ns.is_some() || end_ns.is_some() {
            reports.retain(|report| {
                let ts = report.ts_last;

                if let Some(start) = start_ns
                    && ts < start
                {
                    return false;
                }

                if let Some(end) = end_ns
                    && ts > end
                {
                    return false;
                }

                true
            });
        }

        Ok(reports)
    }

    #[cfg(feature = "python")]
    pub(crate) async fn generate_fill_reports_for_python(
        &self,
        instrument_id: Option<InstrumentId>,
        venue_order_id: Option<VenueOrderId>,
        start: Option<u64>,
        end: Option<u64>,
    ) -> anyhow::Result<Vec<FillReport>> {
        let cmd = GenerateFillReports {
            command_id: UUID4::new(),
            ts_init: get_atomic_clock_realtime().get_time_ns(),
            instrument_id,
            venue_order_id,
            start: start.map(nautilus_core::UnixNanos::from),
            end: end.map(nautilus_core::UnixNanos::from),
            params: None,
            log_receipt_level: LogLevel::Info,
            correlation_id: None,
        };

        self.generate_fill_reports(cmd).await
    }

    #[cfg(feature = "python")]
    pub(crate) async fn generate_position_status_reports_for_python(
        &self,
        instrument_id: Option<InstrumentId>,
        start: Option<u64>,
        end: Option<u64>,
    ) -> anyhow::Result<Vec<PositionStatusReport>> {
        let cmd = GeneratePositionStatusReports {
            command_id: UUID4::new(),
            ts_init: get_atomic_clock_realtime().get_time_ns(),
            instrument_id,
            start: start.map(nautilus_core::UnixNanos::from),
            end: end.map(nautilus_core::UnixNanos::from),
            params: None,
            log_receipt_level: LogLevel::Info,
            correlation_id: None,
        };

        self.generate_position_status_reports(&cmd).await
    }

    #[cfg(feature = "python")]
    pub(crate) fn cache_order_for_python(
        &self,
        order: OrderAny,
        position_id: Option<PositionId>,
    ) -> anyhow::Result<()> {
        self.core
            .cache_mut()
            .add_order(order, position_id, Some(self.client_id()), true)
    }

    fn reserve_next_local_order_id(next_order_id: &Arc<Mutex<i32>>) -> anyhow::Result<i32> {
        let mut guard = next_order_id
            .lock()
            .map_err(|_| anyhow::anyhow!("Failed to lock next order ID"))?;
        anyhow::ensure!(
            *guard > 0,
            "No valid Interactive Brokers order ID available"
        );
        let order_id = *guard;
        *guard += 1;
        Ok(order_id)
    }

    /// Gets the next valid order ID from IB.
    ///
    /// # Errors
    ///
    /// Returns an error if getting the next order ID fails.
    async fn get_next_order_id(&self) -> anyhow::Result<i32> {
        let client = self.ib_client.as_ref().context("IB client not connected")?;

        let timeout_dur = Duration::from_secs(self.config.request_timeout);
        let order_id = tokio::time::timeout(timeout_dur, client.next_valid_order_id())
            .await
            .context("Timeout getting next order ID")??;
        Ok(order_id)
    }

    async fn get_highest_open_order_id(&self, client: &Client) -> anyhow::Result<Option<i32>> {
        let timeout_dur = Duration::from_secs(self.config.request_timeout);
        let mut subscription = tokio::time::timeout(timeout_dur, client.all_open_orders())
            .await
            .context("Timeout requesting open orders for next order ID initialization")??;
        let mut highest_order_id = None;

        while let Some(order_result) = subscription.next().await {
            match order_result {
                Ok(Orders::OrderData(data)) => {
                    highest_order_id = Some(
                        highest_order_id
                            .map_or(data.order_id, |current: i32| current.max(data.order_id)),
                    );
                }
                Ok(_) => {}
                Err(e) => {
                    tracing::debug!(
                        "Ignoring open-order event while initializing next order ID: {e}"
                    );
                }
            }
        }

        Ok(highest_order_id)
    }

    /// Aborts all pending tasks.
    fn abort_pending_tasks(&self) {
        let mut tasks = self.pending_tasks.lock().expect(MUTEX_POISONED);
        for task in tasks.drain(..) {
            task.abort();
        }

        if let Some(handle) = self
            .order_update_handle
            .lock()
            .expect(MUTEX_POISONED)
            .take()
        {
            handle.abort();
        }
    }
}

// Implementation of ExecutionClient trait
#[async_trait::async_trait(?Send)]
impl ExecutionClient for InteractiveBrokersExecutionClient {
    fn is_connected(&self) -> bool {
        self.is_connected.load(Ordering::Relaxed)
    }

    fn client_id(&self) -> ClientId {
        self.core.client_id
    }

    fn account_id(&self) -> AccountId {
        self.core.account_id
    }

    fn venue(&self) -> Venue {
        self.core.venue
    }

    fn oms_type(&self) -> OmsType {
        self.core.oms_type
    }

    fn get_account(&self) -> Option<AccountAny> {
        self.core.cache().account(&self.core.account_id).cloned()
    }

    fn generate_account_state(
        &self,
        balances: Vec<AccountBalance>,
        margins: Vec<MarginBalance>,
        reported: bool,
        ts_event: UnixNanos,
    ) -> anyhow::Result<()> {
        let factory = OrderEventFactory::new(
            self.core.trader_id,
            self.core.account_id,
            self.core.account_type,
            self.core.base_currency,
        );
        let state = factory.generate_account_state(
            balances,
            margins,
            reported,
            ts_event,
            get_atomic_clock_realtime().get_time_ns(),
        );
        get_exec_event_sender()
            .send(ExecutionEvent::Account(state))
            .map_err(|e| anyhow::anyhow!("Failed to send account state: {e}"))
    }

    fn start(&mut self) -> anyhow::Result<()> {
        // Start is handled by connect() for live clients
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        self.abort_pending_tasks();
        Ok(())
    }

    fn submit_order(&self, cmd: SubmitOrder) -> anyhow::Result<()> {
        let client = self.ib_client.as_ref().context("IB client not connected")?;

        let order_id_map = Arc::clone(&self.order_id_map);
        let venue_order_id_map = Arc::clone(&self.venue_order_id_map);
        let instrument_id_map = Arc::clone(&self.instrument_id_map);
        let trader_id_map = Arc::clone(&self.trader_id_map);
        let strategy_id_map = Arc::clone(&self.strategy_id_map);
        let next_order_id = Arc::clone(&self.next_order_id);
        let instrument_provider = Arc::clone(&self.instrument_provider);
        let exec_sender = get_exec_event_sender();
        let clock = get_atomic_clock_realtime();
        let accepted_orders = Arc::clone(&self.accepted_orders);

        let client_clone = client.as_arc().clone();

        let account_id = self.core.account_id;

        let handle = get_runtime().spawn(async move {
            if let Err(e) = Self::handle_submit_order_async(
                &cmd,
                &client_clone,
                &order_id_map,
                &venue_order_id_map,
                &instrument_id_map,
                &trader_id_map,
                &strategy_id_map,
                &next_order_id,
                &instrument_provider,
                &exec_sender,
                clock,
                account_id,
                &accepted_orders,
            )
            .await
            {
                tracing::error!("Error submitting order: {e}");
            }
        });

        self.pending_tasks
            .lock()
            .map_err(|_| anyhow::anyhow!("Failed to lock pending tasks"))?
            .push(handle);

        Ok(())
    }

    async fn connect(&mut self) -> anyhow::Result<()> {
        if self.is_connected.load(Ordering::Relaxed) {
            log::debug!("Interactive Brokers execution client already connected");
            return Ok(());
        }

        tracing::info!("Connecting Interactive Brokers execution client...");
        log::debug!(
            "Execution client config host={} port={} client_id={} account_id={:?} request_timeout={} connection_timeout={} fetch_all_open_orders={} track_option_exercise_from_position_update={}",
            self.config.host,
            self.config.port,
            self.config.client_id,
            self.config.account_id,
            self.config.request_timeout,
            self.config.connection_timeout,
            self.config.fetch_all_open_orders,
            self.config.track_option_exercise_from_position_update
        );

        let handle = crate::common::shared_client::get_or_connect(
            &self.config.host,
            self.config.port,
            self.config.client_id,
            self.config.connection_timeout,
        )
        .await
        .context("Failed to connect to IB Gateway/TWS")?;

        tracing::info!(
            "Connected to IB Gateway/TWS at {}:{} (client_id: {})",
            self.config.host,
            self.config.port,
            self.config.client_id
        );

        self.ib_client = Some(handle);

        // Initialize provider and load instruments from cache if configured
        log::debug!("Initializing IB execution instrument provider");
        if let Err(e) = self.instrument_provider.initialize().await {
            tracing::warn!("Failed to initialize instrument provider: {}", e);
        }

        // Load instruments from config
        log::debug!("Loading configured IB execution instruments");

        if let Err(e) = self
            .instrument_provider
            .load_all_async(
                self.ib_client.as_ref().unwrap().as_arc().as_ref(),
                None,
                None,
                false,
            )
            .await
        {
            tracing::warn!("Failed to load instruments on startup: {}", e);
        }

        let client = self.ib_client.as_ref().unwrap().as_arc();
        log::debug!("Preloading cached spread instruments for execution client");
        self.preload_cached_spread_instruments(client.as_ref())
            .await?;

        // Get initial next order ID (uses self.ib_client internally)
        log::debug!("Requesting next valid IB order ID");
        let next_id = self.get_next_order_id().await?;
        log::debug!("Requesting highest open IB order ID");
        let highest_open_order_id = self.get_highest_open_order_id(client.as_ref()).await?;
        let starting_order_id = highest_open_order_id
            .map(|order_id| next_id.max(order_id.saturating_add(1)))
            .unwrap_or(next_id);
        if starting_order_id != next_id {
            tracing::info!(
                "Adjusted next Interactive Brokers order ID from {} to {} based on existing open orders",
                next_id,
                starting_order_id
            );
        } else {
            tracing::info!(
                "Initialized next Interactive Brokers order ID to {}",
                starting_order_id
            );
        }
        {
            let mut id = self
                .next_order_id
                .lock()
                .map_err(|_| anyhow::anyhow!("Failed to lock next order ID"))?;
            *id = starting_order_id;
        }

        // Start order update subscription (uses self.ib_client internally)
        log::debug!("Starting IB order update stream");
        self.start_order_updates().await?;

        // Subscribe to account summary and generate initial account state
        // Wait for initial account summary to load before proceeding
        let client_for_account = Arc::clone(client);
        let account_id = self.core.account_id;
        let _exec_client_core = self.core.clone(); // Clone core to generate account state
        log::debug!("Subscribing to IB account summary for {}", account_id);
        match crate::execution::account::subscribe_account_summary(&client_for_account, account_id)
            .await
        {
            Ok((balances, margins)) => {
                tracing::info!(
                    "Received account summary: {} balances, {} margins",
                    balances.len(),
                    margins.len()
                );
                // Generate account state event like Python version
                let ts_event = get_atomic_clock_realtime().get_time_ns();

                if let Err(e) = ExecutionClient::generate_account_state(
                    self, balances, margins, true, // reported
                    ts_event,
                ) {
                    tracing::warn!("Failed to generate account state: {}", e);
                }
            }
            Err(e) => {
                tracing::warn!("Failed to subscribe to account summary: {}", e);
            }
        }

        // Initialize position tracking with existing positions
        // This avoids processing duplicates from execDetails
        let client_for_positions_init = Arc::clone(client);
        let position_tracker_init = Arc::clone(&self.position_tracker);

        log::debug!("Initializing IB execution position tracking");
        if let Err(e) = crate::execution::account::initialize_position_tracking(
            &client_for_positions_init,
            self.core.account_id,
            position_tracker_init,
        )
        .await
        {
            tracing::warn!("Failed to initialize position tracking: {}", e);
        }

        // Subscribe to PnL updates
        let client_for_pnl = Arc::clone(client); // Clone Arc

        log::debug!("Subscribing to IB PnL updates");

        if let Err(e) =
            crate::execution::account::subscribe_pnl(&client_for_pnl, self.core.account_id).await
        {
            tracing::warn!("Failed to subscribe to PnL: {}", e);
        }

        // Subscribe to position updates for option exercise tracking if enabled
        if self.config.track_option_exercise_from_position_update {
            let client_for_positions = Arc::clone(client);
            let position_tracker_clone = Arc::clone(&self.position_tracker);
            let instrument_provider_clone = Arc::clone(&self.instrument_provider);

            log::debug!("Subscribing to IB position updates for option exercise tracking");

            if let Err(e) = crate::execution::account::subscribe_positions(
                &client_for_positions,
                self.core.account_id,
                position_tracker_clone,
                instrument_provider_clone,
            )
            .await
            {
                tracing::warn!("Failed to subscribe to positions: {}", e);
            }
        }

        self.is_connected.store(true, Ordering::Relaxed);
        self.core.set_connected();

        tracing::info!("Connected Interactive Brokers execution client");
        Ok(())
    }

    async fn disconnect(&mut self) -> anyhow::Result<()> {
        if !self.is_connected.load(Ordering::Relaxed) {
            log::debug!("Interactive Brokers execution client already disconnected");
            return Ok(());
        }

        tracing::info!("Disconnecting Interactive Brokers execution client...");

        // Abort pending tasks
        self.abort_pending_tasks();

        // Disconnect IB client if connected
        // The rust-ibapi Client doesn't have an explicit disconnect method
        // Connection will be closed when the Arc is dropped
        if self.ib_client.is_some() {
            tracing::debug!("Dropping IB client connection");
        }

        self.ib_client = None;
        self.is_connected.store(false, Ordering::Relaxed);
        self.core.set_disconnected();

        tracing::info!("Disconnected Interactive Brokers execution client");
        Ok(())
    }

    async fn generate_order_status_report(
        &self,
        cmd: &GenerateOrderStatusReport,
    ) -> anyhow::Result<Option<OrderStatusReport>> {
        let plural_cmd = GenerateOrderStatusReports {
            command_id: cmd.command_id,
            ts_init: cmd.ts_init,
            open_only: false,
            instrument_id: cmd.instrument_id,
            start: None,
            end: None,
            params: cmd.params.clone(),
            log_receipt_level: LogLevel::Info,
            correlation_id: cmd.correlation_id,
        };

        let reports = self.generate_order_status_reports(&plural_cmd).await?;

        // Filter by client_order_id and venue_order_id
        let report = reports.into_iter().find(|r| {
            let matches_client = if let Some(filter_client_id) = cmd.client_order_id {
                r.client_order_id == Some(filter_client_id)
            } else {
                true
            };
            let matches_venue = if let Some(filter_venue_id) = cmd.venue_order_id {
                r.venue_order_id == filter_venue_id
            } else {
                true
            };
            matches_client && matches_venue
        });

        Ok(report)
    }

    async fn generate_order_status_reports(
        &self,
        cmd: &GenerateOrderStatusReports,
    ) -> anyhow::Result<Vec<OrderStatusReport>> {
        let client = self.ib_client.as_ref().context("IB client not connected")?;

        let timeout_dur = Duration::from_secs(self.config.request_timeout);
        let mut subscription = tokio::time::timeout(timeout_dur, client.all_open_orders())
            .await
            .context("Timeout requesting open orders")??;
        let mut reports = Vec::new();
        let ts_init = get_atomic_clock_realtime().get_time_ns();

        while let Some(order_result) = subscription.next().await {
            match order_result {
                Ok(Orders::OrderData(data)) => {
                    // Convert IB contract to instrument ID
                    let instrument_id = ib_contract_to_instrument_id_simple(&data.contract)
                        .context("Failed to convert contract to instrument ID")?;

                    // Filter by instrument_id if specified
                    if let Some(filter_id) = cmd.instrument_id {
                        if instrument_id != filter_id {
                            continue;
                        }
                    }

                    // Parse to order status report using minimal OrderStatus
                    // Note: OrderState doesn't have filled/average_fill_price, so we use defaults
                    match parse_order_status_to_report(
                        &IBOrderStatus {
                            order_id: data.order_id,
                            status: data.order_state.status.clone(),
                            filled: 0.0,             // Not available in OrderState
                            remaining: 0.0,          // Not available in OrderState
                            average_fill_price: 0.0, // Not available in OrderState
                            perm_id: data.order.perm_id,
                            parent_id: 0,         // Not available in OrderState
                            last_fill_price: 0.0, // Not available in OrderState
                            client_id: data.order.client_id,
                            why_held: String::new(), // Not available in OrderState
                            market_cap_price: 0.0,   // Not available in OrderState
                        },
                        Some(&data.order),
                        instrument_id,
                        self.core.account_id,
                        &self.instrument_provider,
                        ts_init,
                    ) {
                        Ok(report) => reports.push(report),
                        Err(e) => {
                            tracing::warn!("Failed to parse order status report: {e}");
                        }
                    }
                }
                Ok(_) => {
                    // Ignore other order types
                }
                Err(e) => {
                    tracing::warn!("Error receiving order data: {e}");
                }
            }
        }

        Ok(reports)
    }

    async fn generate_fill_reports(
        &self,
        cmd: GenerateFillReports,
    ) -> anyhow::Result<Vec<FillReport>> {
        let client = self.ib_client.as_ref().context("IB client not connected")?;

        // Get account code from account ID
        let account_code = self.core.account_id.to_string();

        // Build time filter from start/end if provided
        let time_filter = if let (Some(start), Some(end)) = (cmd.start, cmd.end) {
            // Format: YYYYMMDD
            // Convert UnixNanos to DateTime<Utc> then format
            let start_dt = start.to_datetime_utc();
            let end_dt = end.to_datetime_utc();
            format!("{} {}", start_dt.format("%Y%m%d"), end_dt.format("%Y%m%d"))
        } else {
            String::new()
        };

        let filter = ExecutionFilter {
            client_id: None,
            account_code,
            time: time_filter,
            symbol: String::new(),
            security_type: String::new(),
            exchange: String::new(),
            side: String::new(),
            last_n_days: 0,
            specific_dates: Vec::new(),
        };

        let timeout_dur = Duration::from_secs(self.config.request_timeout);
        let mut subscription = tokio::time::timeout(timeout_dur, client.executions(filter))
            .await
            .context("Timeout requesting executions")??;
        let mut reports = Vec::new();
        let ts_init = get_atomic_clock_realtime().get_time_ns();
        let mut current_exec_data: Option<ExecutionData> = None;

        while let Some(exec_result) = subscription.next().await {
            match exec_result {
                Ok(Executions::ExecutionData(exec_data)) => {
                    current_exec_data = Some(exec_data);
                }
                Ok(Executions::CommissionReport(commission)) => {
                    if let Some(exec_data) = current_exec_data.take() {
                        // Convert IB contract to instrument ID
                        let instrument_id =
                            ib_contract_to_instrument_id_simple(&exec_data.contract)
                                .context("Failed to convert contract to instrument ID")?;

                        // Filter by instrument_id if specified
                        if let Some(filter_id) = cmd.instrument_id
                            && instrument_id != filter_id
                        {
                            continue;
                        }

                        // Parse to fill report
                        match parse_execution_to_fill_report(
                            &exec_data.execution,
                            &exec_data.contract,
                            commission.commission,
                            &commission.currency,
                            instrument_id,
                            self.core.account_id,
                            &self.instrument_provider,
                            ts_init,
                            None, // avg_px (not available in historical fills)
                        ) {
                            Ok(report) => reports.push(report),
                            Err(e) => {
                                tracing::warn!("Failed to parse fill report: {e}");
                            }
                        }
                    }
                }
                Ok(_) => {
                    // Ignore other message types (Notice, etc.)
                }
                Err(e) => {
                    tracing::warn!("Error receiving execution data: {e}");
                }
            }
        }

        Ok(reports)
    }

    async fn generate_position_status_reports(
        &self,
        cmd: &GeneratePositionStatusReports,
    ) -> anyhow::Result<Vec<PositionStatusReport>> {
        let client = self.ib_client.as_ref().context("IB client not connected")?;

        let timeout_dur = Duration::from_secs(self.config.request_timeout);
        let mut subscription = tokio::time::timeout(timeout_dur, client.positions())
            .await
            .context("Timeout requesting positions")??;
        let mut reports = Vec::new();
        let ts_init = get_atomic_clock_realtime().get_time_ns();

        // Process positions until PositionEnd; return empty list when none (reconciliation parity:
        // never return None/missing for "no positions").
        while let Some(position_result) = subscription.next().await {
            match position_result {
                Ok(PositionUpdate::Position(position)) => {
                    // Filter for the specific account
                    if position.account != self.core.account_id.to_string() {
                        continue;
                    }

                    // Convert IB contract to instrument ID
                    let instrument_id = ib_contract_to_instrument_id_simple(&position.contract)
                        .context("Failed to convert contract to instrument ID")?;

                    // Filter by instrument_id if specified
                    if let Some(filter_id) = cmd.instrument_id
                        && instrument_id != filter_id
                    {
                        continue;
                    }

                    // Get instrument for precision
                    let instrument = self
                        .instrument_provider
                        .find(&instrument_id)
                        .context("Instrument not found")?;

                    // Determine position side
                    let position_side = if position.position == 0.0 {
                        PositionSideSpecified::Flat
                    } else if position.position > 0.0 {
                        PositionSideSpecified::Long
                    } else {
                        PositionSideSpecified::Short
                    };

                    let quantity =
                        Quantity::new(position.position.abs(), instrument.size_precision());

                    // Convert IB avg_cost to Nautilus Price, accounting for price magnifier and multiplier
                    // Python: converted_avg_cost = avg_cost / (multiplier * price_magnifier)
                    let avg_px_open = if position.average_cost > 0.0 {
                        let price_magnifier =
                            self.instrument_provider.get_price_magnifier(&instrument_id) as f64;
                        let multiplier = instrument.multiplier().as_f64();
                        let converted_avg_cost =
                            position.average_cost / (multiplier * price_magnifier);
                        let price_precision = instrument.price_precision();
                        Some(
                            rust_decimal::Decimal::from_f64_retain(converted_avg_cost)
                                .and_then(|d| {
                                    // Round to price precision
                                    let rounded = d.round_dp(price_precision as u32);
                                    Some(rounded)
                                })
                                .unwrap_or_default(),
                        )
                    } else {
                        None
                    };

                    let report = PositionStatusReport::new(
                        self.core.account_id,
                        instrument_id,
                        position_side,
                        quantity,
                        ts_init, // ts_last
                        ts_init, // ts_init
                        None,    // report_id: auto-generated
                        None,    // venue_position_id
                        avg_px_open,
                    );

                    reports.push(report);
                }
                Ok(PositionUpdate::PositionEnd) => {
                    // End of position list
                    break;
                }
                Err(e) => {
                    tracing::warn!("Error receiving position data: {e}");
                }
            }
        }

        Ok(reports)
    }

    async fn generate_mass_status(
        &self,
        lookback_mins: Option<u64>,
    ) -> anyhow::Result<Option<ExecutionMassStatus>> {
        let ts_now = get_atomic_clock_realtime().get_time_ns();
        let start = lookback_mins.map(|mins| {
            let lookback_ns = mins * 60 * 1_000_000_000;
            UnixNanos::from(ts_now.as_u64().saturating_sub(lookback_ns))
        });

        let order_cmd = GenerateOrderStatusReportsBuilder::default()
            .ts_init(ts_now)
            .open_only(false)
            .start(start)
            .build()
            .map_err(|e| anyhow::anyhow!("{e}"))?;

        let fill_cmd = GenerateFillReportsBuilder::default()
            .ts_init(ts_now)
            .start(start)
            .build()
            .map_err(|e| anyhow::anyhow!("{e}"))?;

        let position_cmd = GeneratePositionStatusReportsBuilder::default()
            .ts_init(ts_now)
            .start(start)
            .build()
            .map_err(|e| anyhow::anyhow!("{e}"))?;

        let (order_reports, fill_reports, position_reports) = tokio::try_join!(
            self.generate_order_status_reports(&order_cmd),
            self.generate_fill_reports(fill_cmd),
            self.generate_position_status_reports(&position_cmd),
        )?;

        tracing::info!(
            "generate_mass_status: {} order reports, {} fill reports, {} position reports",
            order_reports.len(),
            fill_reports.len(),
            position_reports.len()
        );

        let mut mass_status = ExecutionMassStatus::new(
            self.core.client_id,
            self.core.account_id,
            self.core.venue,
            ts_now,
            Some(UUID4::new()),
        );

        mass_status.add_order_reports(order_reports);
        mass_status.add_fill_reports(fill_reports);
        mass_status.add_position_reports(position_reports);

        Ok(Some(mass_status))
    }

    fn query_account(&self, _cmd: QueryAccount) -> anyhow::Result<()> {
        let client = self.ib_client.as_ref().context("IB client not connected")?;

        let client_clone = client.as_arc().clone();
        let account_id = self.core.account_id;
        let account_type = self.core.account_type;
        let base_currency = self.core.base_currency;
        let clock = get_atomic_clock_realtime();
        let request_timeout_secs = self.config.request_timeout;

        let handle = get_runtime().spawn(async move {
            let timeout_dur = Duration::from_secs(request_timeout_secs);
            let result = tokio::time::timeout(
                timeout_dur,
                crate::execution::account::subscribe_account_summary(&client_clone, account_id),
            )
            .await;

            match result {
                Ok(Ok((balances, margins))) => {
                    let ts_event = clock.get_time_ns();
                    let ts_now = clock.get_time_ns();

                    let account_state = AccountState::new(
                        account_id,
                        account_type,
                        balances,
                        margins,
                        true,
                        UUID4::new(),
                        ts_event,
                        ts_now,
                        base_currency,
                    );

                    let endpoint = MessagingSwitchboard::portfolio_update_account();
                    send_account_state(endpoint, &account_state);
                }
                Ok(Err(e)) => {
                    tracing::error!("Failed to query account state: {e}");
                }
                Err(_) => {
                    tracing::error!("Timeout waiting for account summary");
                }
            }
        });

        self.pending_tasks
            .lock()
            .map_err(|_| anyhow::anyhow!("Failed to lock pending tasks"))?
            .push(handle);

        Ok(())
    }

    fn query_order(&self, cmd: QueryOrder) -> anyhow::Result<()> {
        let client = self.ib_client.as_ref().context("IB client not connected")?;
        let client_order_id = cmd.client_order_id;
        let trader_id = cmd.trader_id;
        let strategy_id = cmd.strategy_id;
        let instrument_id = cmd.instrument_id;

        let target_ib_order_id: i32 = if let Some(venue_order_id) = &cmd.venue_order_id {
            venue_order_id
                .as_str()
                .parse()
                .context("Failed to parse venue_order_id as IB order id")?
        } else {
            let map = self
                .order_id_map
                .lock()
                .map_err(|_| anyhow::anyhow!("Failed to lock order_id_map"))?;
            *map.get(&cmd.client_order_id)
                .context("No venue order id for client_order_id")?
        };

        let client_clone = client.as_arc().clone();
        let instrument_id_map = Arc::clone(&self.instrument_id_map);
        let instrument_provider = Arc::clone(&self.instrument_provider);
        let account_id = self.core.account_id;
        let exec_sender = get_exec_event_sender();
        let ts_init = get_atomic_clock_realtime().get_time_ns();
        let request_timeout_secs = self.config.request_timeout;
        let pending_cancel_orders = Arc::clone(&self.pending_cancel_orders);

        let handle = get_runtime().spawn(async move {
            let timeout_dur = Duration::from_secs(request_timeout_secs);
            let mut subscription =
                match tokio::time::timeout(timeout_dur, client_clone.all_open_orders()).await {
                    Ok(Ok(s)) => s,
                    Ok(Err(e)) => {
                        tracing::error!("query_order: failed to request open orders: {e}");
                        return;
                    }
                    Err(_) => {
                        tracing::error!("query_order: timeout requesting open orders");
                        return;
                    }
                };

            while let Some(order_result) = subscription.next().await {
                if let Ok(Orders::OrderData(data)) = order_result {
                    if data.order_id != target_ib_order_id {
                        continue;
                    }

                    let instrument_id = match instrument_id_map.lock() {
                        Ok(map) => map.get(&data.order_id).copied(),
                        Err(_) => None,
                    };
                    let instrument_id = match instrument_id {
                        Some(id) => id,
                        None => match ib_contract_to_instrument_id_simple(&data.contract) {
                            Ok(id) => id,
                            Err(e) => {
                                tracing::warn!("query_order: failed to convert contract: {e}");
                                return;
                            }
                        },
                    };

                    let report = match parse_order_status_to_report(
                        &IBOrderStatus {
                            order_id: data.order_id,
                            status: data.order_state.status.clone(),
                            filled: 0.0,
                            remaining: 0.0,
                            average_fill_price: 0.0,
                            perm_id: data.order.perm_id,
                            parent_id: 0,
                            last_fill_price: 0.0,
                            client_id: data.order.client_id,
                            why_held: String::new(),
                            market_cap_price: 0.0,
                        },
                        Some(&data.order),
                        instrument_id,
                        account_id,
                        &instrument_provider,
                        ts_init,
                    ) {
                        Ok(r) => r,
                        Err(e) => {
                            tracing::warn!("query_order: failed to parse order status: {e}");
                            return;
                        }
                    };

                    if exec_sender
                        .send(ExecutionEvent::Report(ExecutionReport::Order(Box::new(
                            report,
                        ))))
                        .is_err()
                    {
                        tracing::error!("query_order: failed to send order status report");
                    }
                    return;
                }
            }

            let was_pending_cancel = pending_cancel_orders
                .lock()
                .map(|mut pending| pending.remove(&client_order_id))
                .unwrap_or(false);

            if was_pending_cancel {
                let event = OrderCanceled::new(
                    trader_id,
                    strategy_id,
                    instrument_id,
                    client_order_id,
                    UUID4::new(),
                    ts_init,
                    ts_init,
                    false,
                    Some(VenueOrderId::from(target_ib_order_id.to_string())),
                    Some(account_id),
                );

                if exec_sender
                    .send(ExecutionEvent::Order(OrderEventAny::Canceled(event)))
                    .is_err()
                {
                    tracing::error!("query_order: failed to send inferred order canceled event");
                } else {
                    tracing::info!(
                        "query_order: inferred cancel for {} from missing open order {}",
                        client_order_id,
                        target_ib_order_id
                    );
                }
                return;
            }

            tracing::debug!(
                "query_order: order {} not found in open orders (may be filled or canceled)",
                target_ib_order_id
            );
        });

        self.pending_tasks
            .lock()
            .map_err(|_| anyhow::anyhow!("Failed to lock pending tasks"))?
            .push(handle);

        Ok(())
    }

    fn submit_order_list(&self, cmd: SubmitOrderList) -> anyhow::Result<()> {
        let client = self.ib_client.as_ref().context("IB client not connected")?;

        let orders = self.core.get_orders_for_list(&cmd.order_list)?;

        let order_id_map = Arc::clone(&self.order_id_map);
        let venue_order_id_map = Arc::clone(&self.venue_order_id_map);
        let instrument_id_map = Arc::clone(&self.instrument_id_map);
        let trader_id_map = Arc::clone(&self.trader_id_map);
        let strategy_id_map = Arc::clone(&self.strategy_id_map);
        let next_order_id = Arc::clone(&self.next_order_id);
        let instrument_provider = Arc::clone(&self.instrument_provider);
        let exec_sender = get_exec_event_sender();
        let clock = get_atomic_clock_realtime();
        let account_id = self.core.account_id;
        let strategy_id = cmd.strategy_id;
        let accepted_orders = Arc::clone(&self.accepted_orders);
        let client_clone = client.as_arc().clone();

        let handle = get_runtime().spawn(async move {
            if let Err(e) = Self::handle_submit_order_list_async(
                &cmd,
                &orders,
                &client_clone,
                &order_id_map,
                &venue_order_id_map,
                &instrument_id_map,
                &trader_id_map,
                &strategy_id_map,
                &next_order_id,
                &instrument_provider,
                &exec_sender,
                clock,
                account_id,
                strategy_id,
                &accepted_orders,
            )
            .await
            {
                tracing::error!("Error submitting order list: {e}");
            }
        });

        self.pending_tasks
            .lock()
            .map_err(|_| anyhow::anyhow!("Failed to lock pending tasks"))?
            .push(handle);

        Ok(())
    }

    fn modify_order(&self, cmd: ModifyOrder) -> anyhow::Result<()> {
        let client = self.ib_client.as_ref().context("IB client not connected")?;

        // Get order from cache before spawning async task (Rc doesn't work across async boundaries)
        let original_order = {
            let cache = self.core.cache();
            cache
                .order(&cmd.client_order_id)
                .cloned()
                .context("Order not found in cache")?
        };

        let order_id_map = Arc::clone(&self.order_id_map);
        let venue_order_id_map = Arc::clone(&self.venue_order_id_map);
        let instrument_provider = Arc::clone(&self.instrument_provider);
        let exec_sender = get_exec_event_sender();
        let clock = get_atomic_clock_realtime();
        let account_id = self.core.account_id;
        let client_clone = client.as_arc().clone();
        let original_order = Arc::new(original_order);

        let handle = get_runtime().spawn(async move {
            if let Err(e) = Self::handle_modify_order_async(
                &cmd,
                &client_clone,
                &order_id_map,
                &venue_order_id_map,
                &instrument_provider,
                &exec_sender,
                clock,
                account_id,
                &original_order,
            )
            .await
            {
                tracing::error!("Error modifying order: {e}");
            }
        });

        self.pending_tasks
            .lock()
            .map_err(|_| anyhow::anyhow!("Failed to lock pending tasks"))?
            .push(handle);

        Ok(())
    }

    fn cancel_order(&self, cmd: CancelOrder) -> anyhow::Result<()> {
        let client = self.ib_client.as_ref().context("IB client not connected")?;

        let order_id_map = Arc::clone(&self.order_id_map);
        let instrument_id_map = Arc::clone(&self.instrument_id_map);
        let trader_id_map = Arc::clone(&self.trader_id_map);
        let strategy_id_map = Arc::clone(&self.strategy_id_map);
        let pending_cancel_orders = Arc::clone(&self.pending_cancel_orders);
        let exec_sender = get_exec_event_sender();
        let clock = get_atomic_clock_realtime();
        let account_id = self.core.account_id;
        let client_clone = client.as_arc().clone();

        let handle = get_runtime().spawn(async move {
            if let Err(e) = Self::handle_cancel_order_async(
                &cmd,
                &client_clone,
                &order_id_map,
                &instrument_id_map,
                &trader_id_map,
                &strategy_id_map,
                &pending_cancel_orders,
                &exec_sender,
                clock.get_time_ns(),
                account_id,
            )
            .await
            {
                tracing::error!("Error canceling order: {e}");
            }
        });

        self.pending_tasks
            .lock()
            .map_err(|_| anyhow::anyhow!("Failed to lock pending tasks"))?
            .push(handle);

        Ok(())
    }

    fn cancel_all_orders(&self, cmd: CancelAllOrders) -> anyhow::Result<()> {
        let client = self.ib_client.as_ref().context("IB client not connected")?;

        // Warn if order_side is specified (IB doesn't support side filtering)
        if cmd.order_side != OrderSide::NoOrderSide {
            tracing::warn!(
                "Interactive Brokers does not support order_side filtering for cancel all orders; \
                ignoring order_side={:?} and canceling all orders",
                cmd.order_side
            );
        }

        // Get open orders from cache before spawning async task (Rc doesn't work across async boundaries)
        // Note: In Rust, instrument_id is always required, so we always filter by it
        let orders_to_cancel: Vec<(ClientOrderId, Option<VenueOrderId>)> = {
            let cache = self.core.cache();
            let mut orders_to_cancel: Vec<(ClientOrderId, Option<VenueOrderId>)> = cache
                .orders_open(
                    None,                     // venue
                    Some(&cmd.instrument_id), // instrument_id (always filter by it in Rust)
                    None,                     // strategy_id
                    None,                     // account_id
                    None,                     // side (IB doesn't support side filtering)
                )
                .iter()
                .map(|order| (order.client_order_id(), order.venue_order_id()))
                .collect();

            if orders_to_cancel.is_empty() {
                let instrument_id_map = self
                    .instrument_id_map
                    .lock()
                    .map_err(|_| anyhow::anyhow!("Failed to lock instrument ID map"))?;

                let venue_map = self
                    .venue_order_id_map
                    .lock()
                    .map_err(|_| anyhow::anyhow!("Failed to lock venue order ID map"))?;

                orders_to_cancel.extend(instrument_id_map.iter().filter_map(
                    |(order_id, instrument_id)| {
                        (*instrument_id == cmd.instrument_id)
                            .then_some(*order_id)
                            .and_then(|ib_order_id| {
                                venue_map.get(&ib_order_id).copied().map(|client_order_id| {
                                    (
                                        client_order_id,
                                        Some(VenueOrderId::from(ib_order_id.to_string())),
                                    )
                                })
                            })
                    },
                ));
            }

            orders_to_cancel.sort_by_key(|(client_order_id, _)| client_order_id.to_string());
            orders_to_cancel.dedup_by_key(|(client_order_id, _)| *client_order_id);
            orders_to_cancel
        };

        if orders_to_cancel.is_empty() {
            tracing::info!("No open orders to cancel");
            return Ok(());
        }

        tracing::info!(
            "Canceling {} open order(s) for instrument {}",
            orders_to_cancel.len(),
            cmd.instrument_id
        );

        let client_clone = client.as_arc().clone();
        let order_id_map = Arc::clone(&self.order_id_map);
        let instrument_id_map = Arc::clone(&self.instrument_id_map);
        let trader_id_map = Arc::clone(&self.trader_id_map);
        let strategy_id_map = Arc::clone(&self.strategy_id_map);
        let pending_cancel_orders = Arc::clone(&self.pending_cancel_orders);
        let exec_sender = get_exec_event_sender();
        let clock = get_atomic_clock_realtime();
        let account_id = self.core.account_id;

        let handle = get_runtime().spawn(async move {
            if let Err(e) = Self::handle_cancel_all_orders_async(
                &client_clone,
                &order_id_map,
                &instrument_id_map,
                &trader_id_map,
                &strategy_id_map,
                &pending_cancel_orders,
                &exec_sender,
                clock.get_time_ns(),
                account_id,
                orders_to_cancel,
            )
            .await
            {
                tracing::error!("Error canceling all orders: {e}");
            }
        });

        self.pending_tasks
            .lock()
            .map_err(|_| anyhow::anyhow!("Failed to lock pending tasks"))?
            .push(handle);

        Ok(())
    }

    fn batch_cancel_orders(&self, cmd: BatchCancelOrders) -> anyhow::Result<()> {
        // Cancel each order in the batch
        for cancel_cmd in cmd.cancels {
            self.cancel_order(cancel_cmd)?;
        }
        Ok(())
    }
}

#[allow(dead_code)]
impl InteractiveBrokersExecutionClient {
    /// Handles cancel all orders asynchronously.
    ///
    /// # Errors
    ///
    /// Returns an error if the global cancel request fails.
    async fn handle_cancel_order_async(
        cmd: &CancelOrder,
        client: &Arc<Client>,
        order_id_map: &Arc<Mutex<AHashMap<ClientOrderId, i32>>>,
        instrument_id_map: &Arc<Mutex<AHashMap<i32, InstrumentId>>>,
        trader_id_map: &Arc<Mutex<AHashMap<i32, TraderId>>>,
        strategy_id_map: &Arc<Mutex<AHashMap<i32, StrategyId>>>,
        pending_cancel_orders: &Arc<Mutex<ahash::AHashSet<ClientOrderId>>>,
        exec_sender: &tokio::sync::mpsc::UnboundedSender<ExecutionEvent>,
        ts_init: UnixNanos,
        account_id: AccountId,
    ) -> anyhow::Result<()> {
        let ib_order_id = if let Some(venue_order_id) = &cmd.venue_order_id {
            // Use venue order ID directly if available
            venue_order_id
                .as_str()
                .parse::<i32>()
                .map_err(|e| anyhow::anyhow!("Failed to parse venue order ID: {e}"))?
        } else {
            // Otherwise look it up from mapping
            let map = order_id_map
                .lock()
                .map_err(|_| anyhow::anyhow!("Failed to lock order ID map"))?;
            *map.get(&cmd.client_order_id)
                .context("No IB order ID mapping found for client order ID")?
        };

        client
            .cancel_order(ib_order_id, "")
            .await
            .context("Failed to cancel order with IB")?;

        Self::emit_order_pending_cancel(
            ib_order_id,
            cmd.client_order_id,
            instrument_id_map,
            trader_id_map,
            strategy_id_map,
            pending_cancel_orders,
            exec_sender,
            ts_init,
            account_id,
        )?;

        Ok(())
    }

    async fn handle_cancel_all_orders_async(
        client: &Arc<Client>,
        order_id_map: &Arc<Mutex<AHashMap<ClientOrderId, i32>>>,
        instrument_id_map: &Arc<Mutex<AHashMap<i32, InstrumentId>>>,
        trader_id_map: &Arc<Mutex<AHashMap<i32, TraderId>>>,
        strategy_id_map: &Arc<Mutex<AHashMap<i32, StrategyId>>>,
        pending_cancel_orders: &Arc<Mutex<ahash::AHashSet<ClientOrderId>>>,
        exec_sender: &tokio::sync::mpsc::UnboundedSender<ExecutionEvent>,
        ts_init: UnixNanos,
        account_id: AccountId,
        orders_to_cancel: Vec<(ClientOrderId, Option<VenueOrderId>)>,
    ) -> anyhow::Result<()> {
        // Get all IB order IDs first, then drop the guard before awaiting
        let ib_order_ids: Vec<(ClientOrderId, i32)> = {
            let order_id_map_guard = order_id_map
                .lock()
                .map_err(|_| anyhow::anyhow!("Failed to lock order ID map"))?;

            orders_to_cancel
                .into_iter()
                .filter_map(|(client_order_id, venue_order_id)| {
                    if let Some(venue_order_id) = venue_order_id {
                        return venue_order_id
                            .as_str()
                            .parse::<i32>()
                            .ok()
                            .map(|ib_order_id| (client_order_id, ib_order_id));
                    }

                    order_id_map_guard
                        .get(&client_order_id)
                        .copied()
                        .map(|ib_order_id| (client_order_id, ib_order_id))
                })
                .collect()
        };

        // Now cancel each order (guard is dropped, so we can await)
        for (client_order_id, ib_order_id) in ib_order_ids {
            if let Err(e) = client.cancel_order(ib_order_id, "").await {
                tracing::error!(
                    "Failed to cancel order {} (IB order ID: {}): {e}",
                    client_order_id,
                    ib_order_id
                );
            } else {
                if let Err(e) = Self::emit_order_pending_cancel(
                    ib_order_id,
                    client_order_id,
                    instrument_id_map,
                    trader_id_map,
                    strategy_id_map,
                    pending_cancel_orders,
                    exec_sender,
                    ts_init,
                    account_id,
                ) {
                    tracing::error!(
                        "Failed to emit pending cancel for order {} (IB order ID: {}): {e}",
                        client_order_id,
                        ib_order_id
                    );
                }
                tracing::debug!(
                    "Canceled order {} (IB order ID: {})",
                    client_order_id,
                    ib_order_id
                );
            }
        }

        tracing::info!("Finished canceling all orders");

        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn emit_order_pending_cancel(
        order_id: i32,
        client_order_id: ClientOrderId,
        instrument_id_map: &Arc<Mutex<AHashMap<i32, InstrumentId>>>,
        trader_id_map: &Arc<Mutex<AHashMap<i32, TraderId>>>,
        strategy_id_map: &Arc<Mutex<AHashMap<i32, StrategyId>>>,
        pending_cancel_orders: &Arc<Mutex<ahash::AHashSet<ClientOrderId>>>,
        exec_sender: &tokio::sync::mpsc::UnboundedSender<ExecutionEvent>,
        ts_init: UnixNanos,
        account_id: AccountId,
    ) -> anyhow::Result<()> {
        let mut pending = pending_cancel_orders
            .lock()
            .map_err(|_| anyhow::anyhow!("Failed to lock pending cancel orders map"))?;
        if !pending.insert(client_order_id) {
            return Ok(());
        }
        drop(pending);

        let instrument_id = Self::get_mapped_instrument_id(order_id, instrument_id_map)?
            .context("Instrument ID not found for pending cancel order")?;
        let (trader_id, strategy_id) =
            Self::get_required_order_actor_ids(order_id, trader_id_map, strategy_id_map)?;

        let event = OrderPendingCancel::new(
            trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            account_id,
            UUID4::new(),
            ts_init,
            ts_init,
            false,
            Some(VenueOrderId::from(order_id.to_string())),
        );

        exec_sender
            .send(ExecutionEvent::Order(OrderEventAny::PendingCancel(event)))
            .map_err(|e| anyhow::anyhow!("Failed to send order pending cancel event: {e}"))?;

        Ok(())
    }
}

const MUTEX_POISONED: &str = "Mutex poisoned";
