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

#[cfg(test)]
mod tests;

pub(super) use std::{
    fmt::Debug,
    str::FromStr,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

pub(super) use ahash::{AHashMap, AHashSet};
pub(super) use anyhow::Context;
pub(super) use ibapi::{
    Notice, NoticeCategory,
    accounts::PositionUpdate,
    client::Client,
    contracts::Contract,
    orders::{
        ExecutionData, ExecutionFilter, Executions, OrderStatus as IBOrderStatus, OrderStatusKind,
        OrderUpdate, Orders,
    },
    prelude::{StreamExt, SubscriptionItemStreamExt},
};
use nautilus_common::messages::execution::QUERY_INCLUDE_FILLS;
pub(super) use nautilus_common::{
    cache::{Cache, fifo::FifoCacheMap},
    clients::ExecutionClient,
    enums::LogLevel,
    factories::OrderEventFactory,
    live::{
        runner::{get_data_event_sender, get_exec_event_sender},
        sender::EventSender,
    },
    messages::{
        DataEvent, ExecutionEvent,
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
pub(super) use nautilus_core::{
    DurationNanos, Params, UUID4, UnixNanos,
    time::{AtomicTime, get_atomic_clock_realtime},
};
pub(super) use nautilus_live::{
    ExecutionClientCore,
    execution::failure::CommandFailure,
    task::{TaskGroup, TaskGroupGuard, TaskSpawner},
};
pub(super) use nautilus_model::{
    accounts::AccountAny,
    enums::{
        LiquiditySide, OmsType, OrderSide, OrderStatus, OrderType, PositionSide, TrailingOffsetType,
    },
    events::{
        AccountState, OrderAccepted, OrderCancelRejected, OrderCanceled, OrderDenied,
        OrderDeniedReason, OrderEventAny, OrderFilled, OrderModifyRejected, OrderPendingCancel,
        OrderRejected, OrderSubmitted, OrderUpdated,
    },
    identifiers::{
        AccountId, ClientId, ClientOrderId, InstrumentId, StrategyId, TradeId, TraderId, Venue,
        VenueOrderId,
    },
    instruments::{Instrument, InstrumentAny},
    orders::{Order, any::OrderAny},
    reports::{ExecutionMassStatus, FillReport, OrderStatusReport, PositionStatusReport},
    types::{AccountBalance, Currency, MarginBalance, Money, Price, Quantity},
};
pub(super) use parking_lot::Mutex;
pub(super) use rust_decimal::Decimal;
pub(super) use tokio::sync::Mutex as AsyncMutex;
pub(super) use tokio_util::sync::CancellationToken;
pub(super) use ustr::Ustr;

pub(super) use super::{
    account::{
        PositionTracker, create_position_tracker, ib_account_code, initialize_position_tracking,
        record_own_fill, subscribe_account_summary, subscribe_positions,
    },
    parse,
    parse::{
        ib_venue_order_id, parse_execution_time, parse_execution_to_fill_report,
        parse_order_status_to_report,
    },
    transform::nautilus_order_to_ib_order,
};
use super::{incarnations::OrderIncarnationGroup, parse::parse_order_data_to_report};
pub(super) use crate::{
    common::shared_client::{SharedClientHandle, get_or_connect},
    config::InteractiveBrokersExecutionClientConfig,
    providers::instruments::InteractiveBrokersInstrumentProvider,
};

pub(super) const DENIAL_CLIENT_NOT_READY: &str = "IB_CLIENT_NOT_READY";
pub(super) const DENIAL_ORDER_LIST_INVALID: &str = "ORDER_LIST_INVALID";
pub(super) const DENIAL_ORDER_LIST_SIBLING_SUBMIT_FAILED: &str = "ORDER_LIST_SIBLING_SUBMIT_FAILED";
pub(super) const DENIAL_POST_ONLY_UNSUPPORTED: &str = "UNSUPPORTED_POST_ONLY";
pub(super) const DENIAL_QUOTE_QUANTITY_UNSUPPORTED: &str = "UNSUPPORTED_QUOTE_QUANTITY";
pub(super) const DENIAL_TRAILING_OFFSET_TYPE_UNSUPPORTED: &str = "UNSUPPORTED_TRAILING_OFFSET_TYPE";
const ORDER_ID_PARTITION_SIZE: i32 = 1_000_000;

pub(super) fn coded_denial_reason(code: &str, detail: &str) -> String {
    format!("{code}: {detail}")
}

/// Interactive Brokers execution client.
///
/// This client provides order execution functionality using the `rust-ibapi` library.
/// It manages order submission, modification, cancellation, and execution reporting.
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.adapters.interactive_brokers", unsendable)
)]
pub struct InteractiveBrokersExecutionClient {
    /// Core execution client functionality.
    pub(super) core: ExecutionClientCore,
    /// Configuration for the client.
    pub(super) config: InteractiveBrokersExecutionClientConfig,
    /// Raw IB account code sent to TWS, derived once from the configured `account_id`.
    pub(super) ib_account: Ustr,
    /// Instrument provider.
    pub(super) instrument_provider: Arc<InteractiveBrokersInstrumentProvider>,
    /// Connection state.
    pub(super) is_connected: Arc<AtomicBool>,
    /// IB API client (shared per host/port/client_id when both data and execution connect).
    pub(super) ib_client: Option<SharedClientHandle>,
    /// Tasks serving execution commands.
    pub(super) pending_tasks: TaskGroup,
    /// Tasks serving the active connection session.
    pub(super) session_tasks: TaskGroup,
    /// Order ID counter.
    pub(super) next_order_id: Arc<Mutex<i32>>,
    /// Serializes order submissions so TWS receives monotonically increasing order IDs.
    pub(super) order_submit_lock: Arc<AsyncMutex<()>>,
    /// Per-order state and indexes.
    pub(super) orders: OrderTracker,
    /// Commission cache by execution ID (to merge with fill reports).
    pub(super) commission_cache: Arc<Mutex<CommissionCache>>,
    /// Execution cache by execution ID while awaiting commission reports.
    pub(super) pending_execution_cache: Arc<Mutex<PendingExecutionCache>>,
    /// Position tracker for detecting external position changes (e.g., option exercises).
    pub(super) position_tracker: PositionTracker,
}

pub(super) type CommissionCache = FifoCacheMap<String, (f64, String), 10_000>;
pub(super) type PendingExecutionCache =
    FifoCacheMap<String, (tokio::time::Instant, ExecutionData), 10_000>;

#[derive(Clone)]
pub(super) struct OrderTracker {
    pub(super) state: Arc<Mutex<OrderTrackerState>>,
}

pub(super) struct OrderTrackerState {
    pub(super) groups: AHashMap<ClientOrderId, OrderIncarnationGroup>,
    pub(super) group_history: FifoCacheMap<ClientOrderId, OrderIncarnationGroup, 10_000>,
    pub(super) incarnation_parents: AHashMap<i64, ClientOrderId>,
    pub(super) order_snapshots: FifoCacheMap<i64, ibapi::orders::OrderData, 10_000>,
    pub(super) auxiliary_orders: AHashMap<ClientOrderId, TrackedOrder>,
    pub(super) client_id: i32,
    pub(super) order_id_map: AHashMap<ClientOrderId, i32>,
    pub(super) venue_order_id_map: AHashMap<i32, ClientOrderId>,
    pub(super) active_orders: AHashMap<i32, TrackedOrder>,
    pub(super) terminal_orders: FifoCacheMap<i32, TrackedOrder, 10_000>,
}

impl OrderTrackerState {
    pub(super) fn order(&self, order_id: i32) -> Option<&TrackedOrder> {
        self.active_orders
            .get(&order_id)
            .or_else(|| self.terminal_orders.get(&order_id))
    }

    pub(super) fn order_mut(&mut self, order_id: i32) -> Option<&mut TrackedOrder> {
        if self.active_orders.contains_key(&order_id) {
            self.active_orders.get_mut(&order_id)
        } else {
            self.terminal_orders.get_mut(&order_id)
        }
    }

    pub(super) fn order_by_client(&self, client_order_id: ClientOrderId) -> Option<&TrackedOrder> {
        if let Some(order) = self.auxiliary_orders.get(&client_order_id) {
            return Some(order);
        }
        self.order_id_map
            .get(&client_order_id)
            .and_then(|order_id| self.order(*order_id))
    }

    pub(super) fn order_by_client_mut(
        &mut self,
        client_order_id: ClientOrderId,
    ) -> Option<&mut TrackedOrder> {
        if self.auxiliary_orders.contains_key(&client_order_id) {
            return self.auxiliary_orders.get_mut(&client_order_id);
        }
        let order_id = self.order_id_map.get(&client_order_id).copied()?;
        self.order_mut(order_id)
    }

    pub(super) fn cancel_selector(
        &self,
        client_order_id: ClientOrderId,
        venue_order_id: Option<&VenueOrderId>,
    ) -> anyhow::Result<Option<IbOrderSelector>> {
        // A duplicate member shares its raw order ID with another physical order, so
        // only its permanent ID identifies it
        if let Some(order) = self.auxiliary_orders.get(&client_order_id)
            && order.perm_id > 0
        {
            return Ok(Some(IbOrderSelector::PermId(order.perm_id)));
        }

        if let Some(order_id) = self.order_id_map.get(&client_order_id) {
            return Ok(Some(IbOrderSelector::OrderId(*order_id)));
        }

        venue_order_id
            .map(IbOrderSelector::from_venue_order_id)
            .transpose()
    }
}

impl OrderTracker {
    pub(super) fn new(client_id: i32) -> Self {
        Self {
            state: Arc::new(Mutex::new(OrderTrackerState {
                groups: AHashMap::new(),
                group_history: FifoCacheMap::new(),
                incarnation_parents: AHashMap::new(),
                order_snapshots: FifoCacheMap::new(),
                auxiliary_orders: AHashMap::new(),
                client_id,
                order_id_map: AHashMap::new(),
                venue_order_id_map: AHashMap::new(),
                active_orders: AHashMap::new(),
                terminal_orders: FifoCacheMap::new(),
            })),
        }
    }

    #[expect(
        clippy::unnecessary_wraps,
        reason = "Retain the fallible tracker interface used throughout execution callbacks"
    )]
    pub(super) fn lock(&self) -> anyhow::Result<parking_lot::MutexGuard<'_, OrderTrackerState>> {
        Ok(self.state.lock())
    }
}

#[derive(Clone, Copy)]
pub(super) struct SpreadFillContext<'a> {
    pub(super) client_order_id: ClientOrderId,
    pub(super) spread_instrument_id: InstrumentId,
    pub(super) leg_instrument_id: InstrumentId,
    pub(super) commission: f64,
    pub(super) commission_currency: &'a str,
    pub(super) ts_init: UnixNanos,
    pub(super) account_id: AccountId,
    pub(super) avg_px: Option<Price>,
}

#[derive(Debug)]
pub(super) enum OrderCorrelation {
    Tracked {
        order_id: i32,
        context: TrackedOrder,
    },
    Duplicate {
        order_id: i32,
        context: TrackedOrder,
    },
    Untracked {
        client_order_id: Option<ClientOrderId>,
    },
}

#[derive(Clone, Debug)]
pub(super) struct TrackedOrder {
    pub(super) client_order_id: ClientOrderId,
    pub(super) trader_id: TraderId,
    pub(super) strategy_id: StrategyId,
    pub(super) instrument_id: InstrumentId,
    pub(super) order_side: OrderSide,
    pub(super) order_type: OrderType,
    pub(super) accepted: bool,
    pub(super) avg_px: Option<Price>,
    pub(super) pending_cancel: bool,
    pub(super) pending_modify: Option<PendingModifyValues>,
    pub(super) perm_id: i64,
    pub(super) spread_fill_ids: ahash::AHashSet<String>,
    /// Quantity, price, and trigger price of the last `OrderUpdated` sent from an openOrder.
    pub(super) last_update: Option<(Quantity, Option<Price>, Option<Price>)>,
}

/// Requested modify values in IB units, kept until an openOrder reflects them or the
/// modify is rejected, so unrelated openOrder refreshes cannot clear the pending flag.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct PendingModifyValues {
    pub(super) total_quantity: f64,
    pub(super) limit_price: Option<f64>,
    pub(super) aux_price: Option<f64>,
    /// Requested trailing stop trigger; `None` leaves the venue-moved trigger unchecked.
    pub(super) trail_stop_price: Option<f64>,
}

impl PendingModifyValues {
    pub(super) fn matches(&self, order: &ibapi::orders::Order) -> bool {
        const EPS: f64 = 1e-9;
        let approx = |a: f64, b: f64| (a - b).abs() <= EPS;
        let approx_opt = |a: Option<f64>, b: Option<f64>| match (a, b) {
            (Some(a), Some(b)) => approx(a, b),
            (None, None) => true,
            _ => false,
        };
        approx(self.total_quantity, order.total_quantity)
            && approx_opt(self.limit_price, order.limit_price)
            && approx_opt(self.aux_price, order.aux_price)
            && self.trail_stop_price.is_none_or(|requested| {
                order
                    .trail_stop_price
                    .is_some_and(|current| approx(requested, current))
            })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum IbOrderSelector {
    OrderId(i32),
    PermId(i64),
}

impl IbOrderSelector {
    pub(super) fn from_venue_order_id(venue_order_id: &VenueOrderId) -> anyhow::Result<Self> {
        let raw = venue_order_id.as_str();
        if let Some(perm_id) = raw.strip_prefix("PERM-") {
            return Ok(Self::PermId(perm_id.parse::<i64>().with_context(|| {
                format!("Failed to parse venue_order_id {raw:?} as IB perm_id")
            })?));
        }

        Ok(Self::OrderId(raw.parse::<i32>().with_context(|| {
            format!("Failed to parse venue_order_id {raw:?} as IB order_id")
        })?))
    }

    pub(super) fn matches(self, order_id: i32, perm_id: i64) -> bool {
        match self {
            Self::OrderId(target_order_id) => order_id == target_order_id,
            Self::PermId(target_perm_id) => perm_id == target_perm_id,
        }
    }

    pub(super) fn venue_order_id(self) -> VenueOrderId {
        match self {
            Self::OrderId(order_id) => VenueOrderId::from(order_id.to_string()),
            Self::PermId(perm_id) => VenueOrderId::from(format!("PERM-{perm_id}")),
        }
    }

    pub(super) fn label(self) -> String {
        self.venue_order_id().to_string()
    }
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
    /// # Errors
    ///
    /// Returns an error if client creation fails.
    pub fn new(
        core: ExecutionClientCore,
        config: InteractiveBrokersExecutionClientConfig,
        instrument_provider: Arc<InteractiveBrokersInstrumentProvider>,
    ) -> anyhow::Result<Self> {
        anyhow::ensure!(
            !config.client_id.unsigned_abs().is_multiple_of(1000),
            "Interactive Brokers execution client_id must not be a multiple of 1000 because order ID partitioning uses client_id % 1000, was {}",
            config.client_id
        );

        let ib_account = ib_account_code(config.account_id.as_deref(), core.account_id);
        let pending_tasks = TaskGroup::new();
        let session_tasks = TaskGroup::new();

        let orders = OrderTracker::new(config.client_id);
        Ok(Self {
            core,
            config,
            ib_account,
            instrument_provider,
            is_connected: Arc::new(AtomicBool::new(false)),
            ib_client: None,
            pending_tasks,
            session_tasks,
            next_order_id: Arc::new(Mutex::new(0)),
            order_submit_lock: Arc::new(AsyncMutex::new(())),
            orders,
            commission_cache: Arc::new(Mutex::new(CommissionCache::new())),
            pending_execution_cache: Arc::new(Mutex::new(PendingExecutionCache::new())),
            position_tracker: create_position_tracker(),
        })
    }

    async fn order_details_with_fills(
        client: &Client,
        target: IbOrderSelector,
        instrument_id: InstrumentId,
        account_id: AccountId,
        ib_account: Ustr,
        provider: &InteractiveBrokersInstrumentProvider,
        ts_init: UnixNanos,
        orders: &OrderTracker,
    ) -> anyhow::Result<Option<(OrderStatusReport, Vec<FillReport>)>> {
        let mut found = None;
        let mut found_completed = false;

        for completed in [false, true] {
            let subscription = if completed {
                client.completed_orders(false).await?
            } else {
                client.all_open_orders().await?
            };
            let mut subscription = subscription.filter_data();
            while let Some(item) = subscription.next().await {
                let Orders::OrderData(data) = item? else {
                    continue;
                };

                if data.order.account != ib_account
                    || !target.matches(data.order_id, data.order.perm_id)
                {
                    continue;
                }

                if let Some(previous) = &found {
                    let previous: &ibapi::orders::OrderData = previous;
                    anyhow::ensure!(
                        previous.order.perm_id == data.order.perm_id,
                        "IB API order ID identifies multiple permanent IDs"
                    );
                }
                found_completed = completed;
                found = Some(data);
            }

            if found.is_some() {
                break;
            }
        }
        let Some(data) = found else {
            return Ok(None);
        };
        let resolved = provider.resolve_instrument_id_for_contract(&data.contract)?;
        anyhow::ensure!(
            resolved == instrument_id,
            "IB order query resolved an unexpected instrument {resolved}"
        );
        orders
            .lock()?
            .observe_order_data(&data, account_id, found_completed)?;
        let mut report =
            parse_order_data_to_report(&data, instrument_id, account_id, provider, ts_init)?;
        let mut subscription = client
            .executions(Self::execution_filter(ib_account, None))
            .await?
            .filter_data();
        let mut executions = AHashMap::new();
        let mut commissions = AHashMap::new();

        while let Some(item) = subscription.next().await {
            match item? {
                Executions::ExecutionData(data)
                    if data.execution.account_number == ib_account
                        && target.matches(data.execution.order_id, data.execution.perm_id) =>
                {
                    executions.insert(data.execution.execution_id.clone(), data);
                }
                Executions::CommissionReport(commission) => {
                    commissions.insert(
                        commission.execution_id,
                        (commission.commission, commission.currency),
                    );
                }
                _ => {}
            }
        }
        let mut fills = Vec::with_capacity(executions.len());
        for (id, data) in executions {
            let (commission, currency) = commissions
                .remove(&id)
                .with_context(|| format!("IB execution {id} has no commission report"))?;
            fills.push(parse_execution_to_fill_report(
                &data.execution,
                &data.contract,
                commission,
                &currency,
                instrument_id,
                account_id,
                provider,
                ts_init,
                None,
            )?);
        }
        fills.sort_by_key(|fill| (fill.ts_event, fill.trade_id));
        let filled = fills.iter().try_fold(Decimal::ZERO, |sum, fill| {
            sum.checked_add(fill.last_qty.as_decimal())
                .context("IB execution quantity overflow")
        })?;
        let filled = Quantity::from_decimal_dp(filled, report.quantity.precision)?;
        report.filled_qty = report.filled_qty.max(filled);
        if report.quantity.is_positive() && report.filled_qty >= report.quantity {
            report.order_status = OrderStatus::Filled;
        }
        Ok(Some((report, fills)))
    }

    fn submit_order_list_with_orders(
        &self,
        cmd: SubmitOrderList,
        orders: Vec<OrderAny>,
    ) -> anyhow::Result<()> {
        let client = self.ib_client.as_ref().context("IB client not connected")?;

        let order_tracker = self.orders.clone();
        let next_order_id = Arc::clone(&self.next_order_id);
        let instrument_provider = Arc::clone(&self.instrument_provider);
        let exec_sender = get_exec_event_sender();
        let clock = get_atomic_clock_realtime();
        let account_id = self.core.account_id;
        let ib_account = self.ib_account;
        let strategy_id = cmd.strategy_id;
        let client_clone = client.as_arc().clone();
        let order_submit_lock = Arc::clone(&self.order_submit_lock);

        let future = async move {
            if let Err(e) = Self::handle_submit_order_list_async(
                &cmd,
                &orders,
                &client_clone,
                &order_tracker,
                &next_order_id,
                &instrument_provider,
                &exec_sender,
                clock,
                account_id,
                ib_account,
                strategy_id,
                &order_submit_lock,
            )
            .await
            {
                tracing::error!("Error submitting order list: {e}");
            }
        };

        self.pending_tasks
            .spawn(future)
            .context("failed to register IB execution command task")?;

        Ok(())
    }

    pub(super) fn reserve_next_local_order_id(
        next_order_id: &Arc<Mutex<i32>>,
    ) -> anyhow::Result<i32> {
        let mut guard = next_order_id.lock();
        anyhow::ensure!(
            *guard > 0,
            "No valid Interactive Brokers order ID available"
        );
        let order_id = *guard;
        *guard += 1;
        Ok(order_id)
    }

    fn apply_client_order_id_floor(next_id: i32, client_id: i32) -> i32 {
        let client_slot = client_id.unsigned_abs() % 1000;
        if client_slot == 0 {
            return next_id;
        }

        let order_id_floor = (client_slot as i32) * ORDER_ID_PARTITION_SIZE;
        let order_id_ceiling = order_id_floor + ORDER_ID_PARTITION_SIZE;
        if next_id > order_id_floor && next_id < order_id_ceiling {
            next_id
        } else {
            order_id_floor + next_id.rem_euclid(ORDER_ID_PARTITION_SIZE).max(1)
        }
    }

    fn is_order_id_in_client_partition(order_id: i32, client_id: i32) -> bool {
        let client_slot = client_id.unsigned_abs() % 1000;
        if client_slot == 0 {
            return true;
        }

        let order_id_floor = (client_slot as i32) * ORDER_ID_PARTITION_SIZE;
        order_id > order_id_floor && order_id < order_id_floor + ORDER_ID_PARTITION_SIZE
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
        let subscription = tokio::time::timeout(timeout_dur, client.all_open_orders())
            .await
            .context("Timeout requesting open orders for next order ID initialization")??;
        let mut subscription = subscription.filter_data();
        let mut highest_order_id = None;
        let mut snapshots = Vec::new();

        while let Some(order_result) = subscription.next().await {
            match order_result {
                Ok(Orders::OrderData(data)) => {
                    if data.order.account == self.ib_account {
                        snapshots.push(data.clone());
                    }

                    if data.order.client_id != self.config.client_id
                        || !Self::is_order_id_in_client_partition(
                            data.order_id,
                            self.config.client_id,
                        )
                    {
                        continue;
                    }
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

        {
            let cache = self.core.cache();
            let mut state = self.orders.lock()?;

            for data in &snapshots {
                let venue_id = parse::ib_venue_order_id(data.order_id, data.order.perm_id);
                let Some(id) = cache.client_order_id(&venue_id) else {
                    continue;
                };
                let Some(order) = cache.order(id) else {
                    continue;
                };

                if cache.client_id(id) != Some(&self.core.client_id)
                    || order.account_id() != Some(self.core.account_id)
                    || data.order.client_id != self.config.client_id
                {
                    continue;
                }

                if let Some(existing) = state.venue_order_id_map.get(&data.order_id) {
                    if *existing != *id {
                        continue;
                    }
                }
                state.order_id_map.insert(*id, data.order_id);
                state.venue_order_id_map.insert(data.order_id, *id);
                state
                    .active_orders
                    .entry(data.order_id)
                    .or_insert_with(|| TrackedOrder {
                        client_order_id: *id,
                        trader_id: order.trader_id(),
                        strategy_id: order.strategy_id(),
                        instrument_id: order.instrument_id(),
                        order_side: order.order_side(),
                        order_type: order.order_type(),
                        accepted: true,
                        avg_px: None,
                        pending_cancel: false,
                        pending_modify: None,
                        perm_id: data.order.perm_id,
                        spread_fill_ids: AHashSet::new(),
                        last_update: Some((order.quantity(), order.price(), order.trigger_price())),
                    });
            }

            for data in &snapshots {
                state.observe_order_data(data, self.core.account_id, false)?;
            }
        }
        Ok(highest_order_id)
    }

    fn begin_task_shutdown(&self) {
        self.pending_tasks.begin_shutdown();
        self.session_tasks.begin_shutdown();
        self.is_connected.store(false, Ordering::Release);
        self.core.set_disconnected();
    }

    async fn finish_tasks(&self) -> anyhow::Result<()> {
        let (session_result, pending_result) = tokio::join!(
            self.session_tasks
                .finish_shutdown(Duration::from_secs(1), Duration::from_secs(2)),
            self.pending_tasks
                .finish_shutdown(Duration::from_secs(2), Duration::from_secs(2)),
        );
        session_result.context("failed to finish IB execution session tasks")?;
        pending_result.context("failed to finish IB execution command tasks")?;
        Ok(())
    }

    async fn prepare_task_groups(&mut self) -> anyhow::Result<()> {
        if !self.session_tasks.is_open() || !self.pending_tasks.is_open() {
            self.begin_task_shutdown();
            self.finish_tasks().await?;
            self.ib_client = None;
            self.session_tasks
                .start_generation()
                .context("failed to start IB execution session task generation")?;
            self.pending_tasks
                .start_generation()
                .context("failed to start IB execution command task generation")?;
        }
        Ok(())
    }

    async fn teardown_partial_connect(&mut self) -> anyhow::Result<()> {
        self.begin_task_shutdown();
        self.ib_client = None;
        let tasks_result = self.finish_tasks().await;
        self.is_connected.store(false, Ordering::Release);
        self.core.set_disconnected();
        tasks_result
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

    // IB uses a broker venue for the client while routing exchange-MIC instruments;
    // contract transformation remains the authority for actual venue support.
    fn handles_order_venue(&self, _venue: Venue) -> bool {
        true
    }

    fn has_distinct_order_identity(&self, previous: VenueOrderId, reported: VenueOrderId) -> bool {
        matches!((IbOrderSelector::from_venue_order_id(&previous), IbOrderSelector::from_venue_order_id(&reported)),
            (Ok(IbOrderSelector::PermId(first)), Ok(IbOrderSelector::PermId(second))) if first > 0 && second > 0 && first != second)
    }

    fn requires_order_status_for_fill(&self) -> bool {
        true
    }

    fn order_status_query_timeout(&self) -> DurationNanos {
        DurationNanos::try_from_secs(self.config.request_timeout)
            .unwrap_or(DurationNanos::new(u64::MAX))
    }

    fn oms_type(&self) -> OmsType {
        self.core.oms_type
    }

    fn get_account(&self) -> Option<AccountAny> {
        self.core.cache().account_owned(&self.core.account_id)
    }

    fn generate_account_state(
        &self,
        balances: Vec<AccountBalance>,
        margins: Vec<MarginBalance>,
        reported: bool,
        ts_event: UnixNanos,
        info: Option<Params>,
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
            info,
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
        self.begin_task_shutdown();
        Ok(())
    }

    fn submit_order(&self, cmd: SubmitOrder) -> anyhow::Result<()> {
        let order = self.core.get_order(&cmd.client_order_id)?;
        if let Err(reason) = validate_order(&order) {
            let reason = reason.to_string();
            Self::send_order_denied(
                cmd.order_init.trader_id,
                cmd.strategy_id,
                cmd.instrument_id,
                cmd.order_init.client_order_id,
                &reason,
            )?;
            return Ok(());
        }

        if let Err(reason) = self.ensure_client_ready_for_order_request("submit order") {
            self.deny_submit_order_not_ready(&cmd, &reason)?;
            return Ok(());
        }

        let client = self.ib_client.as_ref().context("IB client not connected")?;

        let orders = self.orders.clone();
        let next_order_id = Arc::clone(&self.next_order_id);
        let instrument_provider = Arc::clone(&self.instrument_provider);
        let exec_sender = get_exec_event_sender();
        let clock = get_atomic_clock_realtime();
        let order_submit_lock = Arc::clone(&self.order_submit_lock);

        let client_clone = client.as_arc().clone();

        let account_id = self.core.account_id;
        let ib_account = self.ib_account;

        let future = async move {
            if let Err(e) = Self::handle_submit_order_async(
                &cmd,
                &client_clone,
                &orders,
                &next_order_id,
                &instrument_provider,
                &exec_sender,
                clock,
                account_id,
                ib_account,
                &order_submit_lock,
            )
            .await
            {
                tracing::error!("Error submitting order: {e}");
            }
        };

        self.pending_tasks
            .spawn(future)
            .context("failed to register IB execution command task")?;

        Ok(())
    }

    async fn connect(&mut self) -> anyhow::Result<()> {
        if self.is_connected.load(Ordering::Relaxed)
            && self.session_tasks.is_open()
            && self.pending_tasks.is_open()
        {
            log::debug!("Interactive Brokers execution client already connected");
            return Ok(());
        }

        self.prepare_task_groups().await?;
        let setup_guard =
            TaskGroupGuard::new(&[&self.session_tasks, &self.pending_tasks], move || {});

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

        let handle = get_or_connect(
            &self.config.host,
            self.config.port,
            self.config.client_id,
            self.config.connection_timeout,
        )
        .await
        .context("Failed to connect to IB Gateway/TWS")?;
        let client = Arc::clone(handle.as_arc());

        tracing::info!(
            "Connected to IB Gateway/TWS at {}:{} (client_id: {})",
            self.config.host,
            self.config.port,
            self.config.client_id
        );

        // Initialize provider and load instruments from cache/config if configured
        log::debug!("Initializing IB execution instrument provider");

        if let Err(e) = self
            .instrument_provider
            .initialize_with_client(client.as_ref())
            .await
        {
            if !self.config.instrument_provider.load_ids.is_empty()
                || !self.config.instrument_provider.load_contracts.is_empty()
            {
                return Err(e).context("Failed to load configured IB instruments on startup");
            }

            tracing::warn!("Failed to load instruments on startup: {}", e);
        }

        self.ib_client = Some(handle);

        let session_result = async {

        log::debug!("Preloading cached instruments for execution client");
        self.preload_cached_instruments(client.as_ref()).await;

        // Get initial next order ID (uses self.ib_client internally)
        log::debug!("Requesting next valid IB order ID");
        let next_id = self.get_next_order_id().await?;
        log::debug!("Requesting highest open IB order ID");
        let highest_open_order_id = self.get_highest_open_order_id(client.as_ref()).await?;
        let client_scoped_next_id =
            Self::apply_client_order_id_floor(next_id, self.config.client_id);
        let starting_order_id = highest_open_order_id.map_or(client_scoped_next_id, |order_id| {
            client_scoped_next_id.max(order_id.saturating_add(1))
        });

        if starting_order_id == next_id {
            tracing::debug!(
                "Initialized next Interactive Brokers order ID to {}",
                starting_order_id
            );
        } else {
            tracing::debug!(
                "Adjusted next Interactive Brokers order ID from {} to {} based on client ID/open orders",
                next_id,
                starting_order_id
            );
        }
        {
            let mut id = self
                .next_order_id
                .lock();
            *id = starting_order_id;
        }

        // Start order update subscription (uses self.ib_client internally)
        log::debug!("Starting IB order update stream");
        self.start_order_updates().await?;

        // Subscribe to account summary and generate initial account state
        // Wait for initial account summary to load before proceeding
        let client_for_account = Arc::clone(&client);
        let account_id = self.core.account_id;
        let _exec_client_core = self.core.clone(); // Clone core to generate account state
        log::debug!("Subscribing to IB account summary for {account_id}");

        match subscribe_account_summary(&client_for_account, self.ib_account).await {
            Ok((balances, margins, info)) => {
                tracing::debug!(
                    "Received account summary: {} balances, {} margins",
                    balances.len(),
                    margins.len()
                );
                // Generate account state event like Python version
                let ts_event = get_atomic_clock_realtime().get_time_ns();

                if let Err(e) = ExecutionClient::generate_account_state(
                    self, balances, margins, true, // reported
                    ts_event, info,
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
        let client_for_positions_init = Arc::clone(&client);
        let position_tracker_init = Arc::clone(&self.position_tracker);

        log::debug!("Initializing IB execution position tracking");

        match initialize_position_tracking(
            &client_for_positions_init,
            self.ib_account,
            position_tracker_init,
        )
        .await
        {
            Ok(contracts) => {
                self.publish_position_instruments(client.as_ref(), contracts)
                    .await;
            }
            Err(e) => tracing::warn!("Failed to initialize position tracking: {}", e),
        }

        // Subscribe to PnL updates
        let client_for_pnl = Arc::clone(&client); // Clone Arc

        log::debug!("Subscribing to IB PnL updates");

        if let Err(e) = crate::execution::account::subscribe_pnl(
            &client_for_pnl,
            self.ib_account,
            &self.session_tasks,
        )
        .await
        {
            tracing::warn!("Failed to subscribe to PnL: {}", e);
        }

        // Subscribe to position updates for option exercise tracking if enabled
        if self.config.track_option_exercise_from_position_update {
            let client_for_positions = Arc::clone(&client);
            let position_tracker_clone = Arc::clone(&self.position_tracker);
            let instrument_provider_clone = Arc::clone(&self.instrument_provider);

            log::debug!("Subscribing to IB position updates for option exercise tracking");

            if let Err(e) = subscribe_positions(
                &client_for_positions,
                self.core.account_id,
                self.ib_account,
                position_tracker_clone,
                instrument_provider_clone,
                &self.session_tasks,
            )
            .await
            {
                tracing::warn!("Failed to subscribe to positions: {}", e);
            }
        }

        Ok::<(), anyhow::Error>(())
        }
        .await;

        if let Err(e) = session_result {
            if let Err(teardown_error) = self.teardown_partial_connect().await {
                return Err(e.context(format!(
                    "IB execution startup teardown failed: {teardown_error}"
                )));
            }
            return Err(e);
        }

        self.is_connected.store(true, Ordering::Relaxed);
        self.core.set_connected();
        setup_guard.disarm();

        tracing::info!("Connected Interactive Brokers execution client");
        Ok(())
    }

    async fn disconnect(&mut self) -> anyhow::Result<()> {
        if !self.is_connected.load(Ordering::Relaxed)
            && self.ib_client.is_none()
            && self.session_tasks.is_open()
            && self.session_tasks.is_empty()
            && self.pending_tasks.is_open()
            && self.pending_tasks.is_empty()
        {
            log::debug!("Interactive Brokers execution client already disconnected");
            return Ok(());
        }

        tracing::info!("Disconnecting Interactive Brokers execution client...");

        self.teardown_partial_connect().await?;

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
            causation_id: cmd.causation_id,
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
        let subscription = tokio::time::timeout(timeout_dur, client.all_open_orders())
            .await
            .context("Timeout requesting open orders")??;
        let mut subscription = subscription.filter_data();
        let mut reports = Vec::new();
        let mut ib_orders = Vec::new();
        let ts_init = get_atomic_clock_realtime().get_time_ns();
        let ib_account = self.ib_account;

        while let Some(order_result) = subscription.next().await {
            match order_result {
                Ok(Orders::OrderData(data)) => {
                    if !data.order.account.is_empty() && data.order.account != ib_account {
                        continue;
                    }

                    // Convert IB contract to instrument ID
                    let instrument_id = self
                        .resolve_report_contract_instrument_id(&data.contract)
                        .with_context(|| {
                            format!(
                                "Failed to resolve IB order {} contract ID {} ({:?})",
                                data.order_id,
                                data.contract.contract_id,
                                data.contract.security_type
                            )
                        })?;

                    // Filter by instrument_id if specified
                    if let Some(filter_id) = cmd.instrument_id
                        && instrument_id != filter_id
                    {
                        continue;
                    }

                    // Parse to order status report using minimal OrderStatus
                    // Note: OrderState doesn't have filled/average_fill_price, so we use defaults
                    let report = parse_order_data_to_report(
                        &data,
                        instrument_id,
                        self.core.account_id,
                        &self.instrument_provider,
                        ts_init,
                    )
                    .with_context(|| format!("Failed to parse IB order {}", data.order_id))?;
                    reports.push(report);
                    ib_orders.push(data.order);
                }
                Ok(_) => {
                    // Ignore other order types
                }
                Err(e) => return Err(e.into()),
            }
        }

        if !cmd.open_only {
            let completed = tokio::time::timeout(timeout_dur, client.completed_orders(false))
                .await
                .context("Timeout requesting completed orders")??;
            let mut completed = completed.filter_data();

            while let Some(order_result) = completed.next().await {
                let Orders::OrderData(data) = order_result? else {
                    continue;
                };

                if !data.order.account.is_empty() && data.order.account != ib_account {
                    continue;
                }

                // An OCA group that reduces its members on a fill cancels an unfilled member
                // by reducing its quantity to zero, leaving nothing to reconcile
                if data.order.total_quantity == 0.0 && data.order.filled_quantity == 0.0 {
                    tracing::debug!(
                        "Skipping completed IB order {} with zero quantity",
                        data.order.perm_id
                    );
                    continue;
                }

                let instrument_id = self
                    .resolve_report_contract_instrument_id(&data.contract)
                    .with_context(|| {
                        format!(
                            "Failed to resolve completed IB order contract ID {} ({:?})",
                            data.contract.contract_id, data.contract.security_type
                        )
                    })?;

                if cmd
                    .instrument_id
                    .is_some_and(|filter_id| instrument_id != filter_id)
                {
                    continue;
                }

                let report = parse_order_data_to_report(
                    &data,
                    instrument_id,
                    self.core.account_id,
                    &self.instrument_provider,
                    ts_init,
                )
                .context("Failed to parse completed IB order")?;

                if !reports
                    .iter()
                    .any(|existing| existing.venue_order_id == report.venue_order_id)
                {
                    reports.push(report);
                    ib_orders.push(data.order);
                }
            }
        }

        parse::link_order_contingencies(&mut reports, &ib_orders);

        Ok(reports)
    }

    async fn generate_fill_reports(
        &self,
        cmd: GenerateFillReports,
    ) -> anyhow::Result<Vec<FillReport>> {
        let client = self.ib_client.as_ref().context("IB client not connected")?;

        let filter = Self::execution_filter(self.ib_account, cmd.start);

        let timeout_dur = Duration::from_secs(self.config.request_timeout);
        let subscription = tokio::time::timeout(timeout_dur, client.executions(filter))
            .await
            .context("Timeout requesting executions")??;
        let mut subscription = subscription.filter_data();
        let mut reports = Vec::new();
        let ts_init = get_atomic_clock_realtime().get_time_ns();
        let mut pending_exec_data: AHashMap<String, ExecutionData> = AHashMap::new();
        let mut pending_commissions: AHashMap<String, (f64, String)> = AHashMap::new();

        while let Some(exec_result) = subscription.next().await {
            match exec_result {
                Ok(Executions::ExecutionData(exec_data)) => {
                    let execution_id = exec_data.execution.execution_id.clone();
                    if let Some((commission, commission_currency)) =
                        pending_commissions.remove(&execution_id)
                    {
                        if let Some(report) = self.parse_historical_fill_report(
                            &cmd,
                            &exec_data,
                            commission,
                            &commission_currency,
                            ts_init,
                        ) {
                            reports.push(report);
                        }
                    } else {
                        pending_exec_data.insert(execution_id, exec_data);
                    }
                }
                Ok(Executions::CommissionReport(commission)) => {
                    if let Some(exec_data) = pending_exec_data.remove(&commission.execution_id) {
                        if let Some(report) = self.parse_historical_fill_report(
                            &cmd,
                            &exec_data,
                            commission.commission,
                            &commission.currency,
                            ts_init,
                        ) {
                            reports.push(report);
                        }
                    } else {
                        pending_commissions.insert(
                            commission.execution_id,
                            (commission.commission, commission.currency),
                        );
                    }
                }
                Err(e) => {
                    tracing::warn!("Error receiving execution data: {e}");
                }
            }
        }

        anyhow::ensure!(
            pending_exec_data.is_empty(),
            "IB did not provide commission reports for execution IDs: {}",
            pending_exec_data
                .keys()
                .map(String::as_str)
                .collect::<Vec<_>>()
                .join(", ")
        );

        Ok(reports)
    }

    async fn generate_position_status_reports(
        &self,
        cmd: &GeneratePositionStatusReports,
    ) -> anyhow::Result<Vec<PositionStatusReport>> {
        let client = self.ib_client.as_ref().context("IB client not connected")?;

        let timeout_dur = Duration::from_secs(self.config.request_timeout);
        let subscription = tokio::time::timeout(timeout_dur, client.positions())
            .await
            .context("Timeout requesting positions")??;
        let mut subscription = subscription.filter_data();
        let mut reports = Vec::new();
        let ts_init = get_atomic_clock_realtime().get_time_ns();
        let ib_account = self.ib_account;

        // Process positions until PositionEnd; return empty list when none (reconciliation parity:
        // never return None/missing for "no positions").
        while let Some(position_result) = subscription.next().await {
            match position_result {
                Ok(PositionUpdate::Position(position)) => {
                    // Filter for the specific account
                    if position.account != ib_account {
                        continue;
                    }

                    let instrument = match self
                        .instrument_provider
                        .get_instrument(client.as_arc().as_ref(), &position.contract)
                        .await
                    {
                        Ok(Some(instrument)) => instrument,
                        Ok(None) => anyhow::bail!(
                            "Cannot resolve position instrument for IB contract ID {} ({:?})",
                            position.contract.contract_id,
                            position.contract.security_type
                        ),
                        Err(e) => return Err(e).context(format!(
                            "Failed to resolve position instrument for IB contract ID {} ({:?})",
                            position.contract.contract_id, position.contract.security_type
                        )),
                    };
                    let instrument_id = instrument.id();

                    // Filter by instrument_id if specified
                    if let Some(filter_id) = cmd.instrument_id
                        && instrument_id != filter_id
                    {
                        continue;
                    }

                    // Determine position side
                    let position_side = if position.position == 0.0 {
                        PositionSide::Flat
                    } else if position.position > 0.0 {
                        PositionSide::Long
                    } else {
                        PositionSide::Short
                    };

                    let quantity =
                        Quantity::new(position.position.abs(), instrument.size_precision());

                    // Convert IB avg_cost to Nautilus Price, accounting for price magnifier and multiplier
                    // Python: converted_avg_cost = avg_cost / (multiplier * price_magnifier)
                    let avg_px_open = self.position_avg_px_open(
                        &instrument_id,
                        &instrument,
                        position.average_cost,
                    );

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
                Err(e) => return Err(e.into()),
            }
        }

        if reports.is_empty()
            && let Some(instrument_id) = cmd.instrument_id
        {
            let precision = self
                .instrument_provider
                .find(&instrument_id)
                .map_or(0, |instrument| instrument.size_precision());
            reports.push(PositionStatusReport::new(
                self.core.account_id,
                instrument_id,
                PositionSide::Flat,
                Quantity::zero(precision),
                ts_init,
                ts_init,
                None,
                None,
                None,
            ));
        }

        Ok(reports)
    }

    async fn generate_mass_status(
        &self,
        lookback_mins: Option<u64>,
    ) -> anyhow::Result<Option<ExecutionMassStatus>> {
        let ts_now = get_atomic_clock_realtime().get_time_ns();
        let start = lookback_mins
            .map(DurationNanos::try_from_mins)
            .transpose()?
            .map(|lookback| ts_now.saturating_sub(lookback));

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
        // IB `reqExecutions` and completed orders are session-scoped, so without a
        // configured lookback the actual venue bound is unknown and the report set
        // cannot be claimed complete.
        mass_status.set_report_window(start, start.is_some());

        mass_status.add_order_reports(order_reports);
        mass_status.add_fill_reports(fill_reports);
        mass_status.add_position_reports(position_reports);

        Ok(Some(mass_status))
    }

    fn query_account(&self, _cmd: QueryAccount) -> anyhow::Result<()> {
        let client = self.ib_client.as_ref().context("IB client not connected")?;

        let client_clone = client.as_arc().clone();
        let account_id = self.core.account_id;
        let ib_account = self.ib_account;
        let account_type = self.core.account_type;
        let base_currency = self.core.base_currency;
        let clock = get_atomic_clock_realtime();
        let request_timeout_secs = self.config.request_timeout;

        let future = async move {
            let timeout_dur = Duration::from_secs(request_timeout_secs);
            let result = tokio::time::timeout(
                timeout_dur,
                subscribe_account_summary(&client_clone, ib_account),
            )
            .await;

            match result {
                Ok(Ok((balances, margins, info))) => {
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
                    )
                    .with_info(info);

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
        };

        self.pending_tasks
            .spawn(future)
            .context("failed to register IB execution command task")?;

        Ok(())
    }

    fn query_order(&self, cmd: QueryOrder) -> anyhow::Result<()> {
        let client = self.ib_client.as_ref().context("IB client not connected")?;
        let include_fills = cmd
            .params
            .as_ref()
            .and_then(|params| params.get_bool(QUERY_INCLUDE_FILLS))
            .unwrap_or(false);
        let client_order_id = cmd.client_order_id;
        let trader_id = cmd.trader_id;
        let strategy_id = cmd.strategy_id;
        let instrument_id = cmd.instrument_id;

        let target_order = if let Some(venue_order_id) = &cmd.venue_order_id {
            IbOrderSelector::from_venue_order_id(venue_order_id)?
        } else {
            let state = self.orders.lock()?;
            IbOrderSelector::OrderId(
                *state
                    .order_id_map
                    .get(&cmd.client_order_id)
                    .context("No venue order id for client_order_id")?,
            )
        };

        let order_quantity = self
            .core
            .cache()
            .order(&cmd.client_order_id)
            .map(|order| order.quantity().as_decimal());

        let client_clone = client.as_arc().clone();
        let orders = self.orders.clone();
        let instrument_provider = Arc::clone(&self.instrument_provider);
        let account_id = self.core.account_id;
        let exec_sender = get_exec_event_sender();
        let ts_init = get_atomic_clock_realtime().get_time_ns();
        let request_timeout_secs = self.config.request_timeout;
        let ib_account = self.ib_account;

        let future = async move {
            if include_fills {
                let result = tokio::time::timeout(
                    Duration::from_secs(request_timeout_secs),
                    Self::order_details_with_fills(
                        client_clone.as_ref(),
                        target_order,
                        instrument_id,
                        account_id,
                        ib_account,
                        &instrument_provider,
                        ts_init,
                        &orders,
                    ),
                )
                .await;

                match result {
                    Ok(Ok(Some((report, fills)))) => {
                        if let Err(e) = exec_sender.send(ExecutionEvent::Report(
                            ExecutionReport::OrderWithFills(Box::new(report), fills),
                        )) {
                            tracing::error!("Failed to deliver IB order details: {e}");
                        }
                    }
                    Ok(Ok(None)) => tracing::warn!(
                        "IB returned no authoritative order details for {client_order_id}; fills remain unresolved"
                    ),
                    Ok(Err(e)) => {
                        tracing::error!("Failed to obtain IB order details and executions: {e:#}");
                    }
                    Err(e) => {
                        tracing::error!("Timed out obtaining IB order details and executions: {e}");
                    }
                }
                return;
            }
            let timeout_dur = Duration::from_secs(request_timeout_secs);
            let subscription =
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
            let mut subscription = subscription.filter_data();

            while let Some(order_result) = subscription.next().await {
                if let Ok(Orders::OrderData(data)) = order_result {
                    if !data.order.account.is_empty() && data.order.account != ib_account {
                        continue;
                    }

                    if !target_order.matches(data.order_id, data.order.perm_id) {
                        continue;
                    }

                    let instrument_id = match orders.lock() {
                        Ok(state) => state.order(data.order_id).map(|order| order.instrument_id),
                        Err(_) => None,
                    };
                    let instrument_id = match instrument_id {
                        Some(id) => id,
                        None => match instrument_provider
                            .resolve_instrument_id_for_contract(&data.contract)
                        {
                            Ok(id) => id,
                            Err(e) => {
                                tracing::warn!("query_order: failed to convert contract: {e}");
                                return;
                            }
                        },
                    };

                    let report = match parse_order_data_to_report(
                        &data,
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

            let is_pending_cancel = orders.lock().is_ok_and(|state| {
                state
                    .order_by_client(client_order_id)
                    .is_some_and(|order| order.pending_cancel)
            });

            if is_pending_cancel {
                let filter = Self::execution_filter(ib_account, None);
                let executions = match tokio::time::timeout(
                    timeout_dur,
                    client_clone.executions(filter),
                )
                .await
                {
                    Ok(Ok(subscription)) => subscription,
                    Ok(Err(e)) => {
                        tracing::error!(
                            "query_order: failed to request executions before inferring cancel: {e}"
                        );
                        return;
                    }
                    Err(_) => {
                        tracing::error!(
                            "query_order: timeout requesting executions before inferring cancel"
                        );
                        return;
                    }
                };
                let mut executions = executions.filter_data();
                let mut execution_data = AHashMap::new();
                let mut commissions = AHashMap::new();

                while let Some(result) = executions.next().await {
                    match result {
                        Ok(Executions::ExecutionData(data))
                            if target_order
                                .matches(data.execution.order_id, data.execution.perm_id) =>
                        {
                            execution_data.insert(data.execution.execution_id.clone(), data);
                        }
                        Ok(Executions::CommissionReport(commission)) => {
                            commissions.insert(
                                commission.execution_id,
                                (commission.commission, commission.currency),
                            );
                        }
                        Ok(_) => {}
                        Err(e) => {
                            tracing::error!(
                                "query_order: execution query failed before cancel resolution: {e}"
                            );
                            return;
                        }
                    }
                }

                if !execution_data.is_empty() {
                    let cumulative_filled = execution_data
                        .values()
                        .map(|data| data.execution.cumulative_quantity)
                        .fold(0.0_f64, f64::max);

                    for (execution_id, data) in execution_data {
                        let Some((commission, currency)) = commissions.remove(&execution_id) else {
                            tracing::error!(
                                "query_order: execution {execution_id} has no commission report; cancel remains unresolved"
                            );
                            return;
                        };
                        let report = match parse_execution_to_fill_report(
                            &data.execution,
                            &data.contract,
                            commission,
                            &currency,
                            instrument_id,
                            account_id,
                            &instrument_provider,
                            ts_init,
                            None,
                        ) {
                            Ok(report) => report,
                            Err(e) => {
                                tracing::error!(
                                    "query_order: failed to parse execution {execution_id}: {e}"
                                );
                                return;
                            }
                        };
                        let context =
                            Self::get_tracked_order_context(data.execution.order_id, &orders)
                                .ok()
                                .flatten();
                        let event = if let Some(context) = context {
                            let quote_currency = instrument_provider
                                .find(&context.instrument_id)
                                .map_or(report.commission.currency, |instrument| {
                                    instrument.quote_currency()
                                });
                            ExecutionEvent::Order(OrderEventAny::Filled(OrderFilled::new(
                                context.trader_id,
                                context.strategy_id,
                                context.instrument_id,
                                context.client_order_id,
                                report.venue_order_id,
                                report.account_id,
                                report.trade_id,
                                context.order_side,
                                context.order_type,
                                report.last_qty,
                                report.last_px,
                                quote_currency,
                                report.liquidity_side,
                                UUID4::new(),
                                report.ts_event,
                                report.ts_init,
                                false,
                                report.venue_position_id,
                                Some(report.commission),
                                None,
                            )))
                        } else {
                            ExecutionEvent::Report(ExecutionReport::Fill(Box::new(report)))
                        };

                        if exec_sender.send(event).is_err() {
                            tracing::error!(
                                "query_order: failed to send fill resolved from execution history"
                            );
                            return;
                        }
                    }

                    if let Ok(mut state) = orders.lock()
                        && let Some(order) = state.order_by_client_mut(client_order_id)
                    {
                        order.pending_cancel = false;
                    }

                    // The order is absent from open orders, so any unfilled remainder
                    // was cancelled at the venue. Quantize to the instrument's size
                    // precision so float dust from the f64 cumulative quantity cannot
                    // register as a phantom remainder after a full fill.
                    let size_precision = instrument_provider
                        .find(&instrument_id)
                        .map(|instrument| u32::from(instrument.size_precision()));
                    let remainder = order_quantity.and_then(|total| {
                        Decimal::from_f64_retain(cumulative_filled).map(|filled| {
                            let remainder = total - filled;
                            size_precision.map_or(remainder, |dp| remainder.round_dp(dp))
                        })
                    });

                    match remainder {
                        Some(remainder) if remainder > Decimal::ZERO => {
                            Self::send_inferred_order_canceled(
                                trader_id,
                                strategy_id,
                                instrument_id,
                                client_order_id,
                                target_order.venue_order_id(),
                                account_id,
                                ts_init,
                                &exec_sender,
                                "cancelled remainder after partial-fill recovery",
                            );
                        }
                        Some(_) => {}
                        None => {
                            tracing::debug!(
                                "query_order: cannot determine cancelled remainder for {} (order not in cache)",
                                client_order_id
                            );
                        }
                    }
                    return;
                }
            }

            let was_pending_cancel = orders.lock().is_ok_and(|mut state| {
                state
                    .order_by_client_mut(client_order_id)
                    .is_some_and(|order| std::mem::replace(&mut order.pending_cancel, false))
            });

            if was_pending_cancel {
                Self::send_inferred_order_canceled(
                    trader_id,
                    strategy_id,
                    instrument_id,
                    client_order_id,
                    target_order.venue_order_id(),
                    account_id,
                    ts_init,
                    &exec_sender,
                    "missing open order",
                );
                return;
            }

            tracing::debug!(
                "query_order: order {} not found in open orders (may be filled or canceled)",
                target_order.label()
            );
        };

        self.pending_tasks
            .spawn(future)
            .context("failed to register IB execution command task")?;

        Ok(())
    }

    fn submit_order_list(&self, cmd: SubmitOrderList) -> anyhow::Result<()> {
        let orders = self.core.get_orders_for_list(&cmd.order_list)?;
        if let Some(reason) = orders.iter().find_map(|order| validate_order(order).err()) {
            self.deny_submit_order_list(&cmd, &reason.to_string())?;
            return Ok(());
        }

        if let Err(reason) = self.ensure_client_ready_for_order_request("submit order list") {
            let reason = coded_denial_reason(DENIAL_CLIENT_NOT_READY, &reason);
            self.deny_submit_order_list(&cmd, &reason)?;
            return Ok(());
        }

        self.submit_order_list_with_orders(cmd, orders)
    }

    fn modify_order(&self, cmd: ModifyOrder) -> anyhow::Result<()> {
        if self
            .orders
            .lock()?
            .group_for_client(cmd.client_order_id)
            .is_some()
        {
            Self::send_order_modify_rejected(
                &cmd,
                "Cannot modify an order with unresolved duplicate broker identities; cancel and reconcile it first",
                &get_exec_event_sender(),
                get_atomic_clock_realtime().get_time_ns(),
                self.core.account_id,
            )?;
            return Ok(());
        }

        if let Err(reason) = self.ensure_client_ready_for_order_request("modify order") {
            Self::send_order_modify_rejected(
                &cmd,
                &reason,
                &get_exec_event_sender(),
                get_atomic_clock_realtime().get_time_ns(),
                self.core.account_id,
            )?;
            return Ok(());
        }

        let client = self.ib_client.as_ref().context("IB client not connected")?;

        let orders = self.orders.clone();
        let instrument_provider = Arc::clone(&self.instrument_provider);
        let exec_sender = get_exec_event_sender();
        let clock = get_atomic_clock_realtime();
        let account_id = self.core.account_id;
        let ib_account = self.ib_account;
        let client_clone = client.as_arc().clone();
        let request_timeout_secs = self.config.request_timeout;
        let future = async move {
            if let Err(e) = Self::handle_modify_order_async(
                &cmd,
                &client_clone,
                &orders,
                &instrument_provider,
                ib_account,
                request_timeout_secs,
            )
            .await
            {
                let reason = format!("Failed to route modify order to IB: {e:#}");

                if let Err(send_error) = Self::send_order_modify_rejected(
                    &cmd,
                    &reason,
                    &exec_sender,
                    clock.get_time_ns(),
                    account_id,
                ) {
                    tracing::error!("{reason}; failed to emit OrderModifyRejected: {send_error}");
                }
            }
        };

        self.pending_tasks
            .spawn(future)
            .context("failed to register IB execution command task")?;

        Ok(())
    }

    fn cancel_order(&self, cmd: CancelOrder) -> anyhow::Result<()> {
        let target_order = Arc::new(self.core.get_order(&cmd.client_order_id)?);
        let exec_sender = get_exec_event_sender();
        let clock = get_atomic_clock_realtime();
        let account_id = self.core.account_id;

        if let Err(e) = Self::validate_cancel_order_target(&cmd, &target_order) {
            let reason = format!("Failed to resolve cancel order target: {e:#}");
            Self::send_order_cancel_rejected(
                &target_order,
                &reason,
                &exec_sender,
                clock.get_time_ns(),
                account_id,
            )?;
            return Ok(());
        }

        if let Err(reason) = self.ensure_client_ready_for_order_request("cancel order") {
            Self::send_order_cancel_rejected(
                &target_order,
                &reason,
                &exec_sender,
                clock.get_time_ns(),
                account_id,
            )?;
            return Ok(());
        }

        let client = self.ib_client.as_ref().context("IB client not connected")?;

        let orders = self.orders.clone();
        let instrument_provider = Arc::clone(&self.instrument_provider);
        let exec_sender = get_exec_event_sender();
        let clock = get_atomic_clock_realtime();
        let account_id = self.core.account_id;
        let ib_account = self.ib_account;
        let client_clone = client.as_arc().clone();
        let request_timeout_secs = self.config.request_timeout;

        let future = async move {
            if let Err(e) = Self::handle_cancel_order_async(
                &cmd,
                &target_order,
                &client_clone,
                &orders,
                &instrument_provider,
                &exec_sender,
                clock.get_time_ns(),
                account_id,
                ib_account,
                request_timeout_secs,
            )
            .await
            {
                let reason = format!("Failed to route cancel order to IB: {e:#}");

                if let Err(send_error) = Self::send_order_cancel_rejected(
                    &target_order,
                    &reason,
                    &exec_sender,
                    clock.get_time_ns(),
                    account_id,
                ) {
                    tracing::error!("{reason}; failed to emit OrderCancelRejected: {send_error}");
                }
            }
        };

        self.pending_tasks
            .spawn(future)
            .context("failed to register IB execution command task")?;

        Ok(())
    }

    fn cancel_all_orders(&self, cmd: CancelAllOrders) -> anyhow::Result<()> {
        if self
            .ensure_client_ready_for_order_request("cancel orders")
            .is_err()
        {
            return Ok(());
        }
        let client = self.ib_client.as_ref().context("IB client not connected")?;
        let orders_to_cancel = self.cancel_all_targets(&cmd)?;

        if orders_to_cancel.is_empty() {
            tracing::debug!("No open orders to cancel");
            return Ok(());
        }

        tracing::debug!(
            "Canceling {} open order(s) for instrument {}",
            orders_to_cancel.len(),
            cmd.instrument_id
        );

        let client_clone = client.as_arc().clone();
        let orders = self.orders.clone();
        let exec_sender = get_exec_event_sender();
        let clock = get_atomic_clock_realtime();
        let account_id = self.core.account_id;
        let ib_account = self.ib_account;
        let request_timeout_secs = self.config.request_timeout;

        let provider = Arc::clone(&self.instrument_provider);
        let future = async move {
            if let Err(e) = Self::handle_cancel_all_orders_async(
                &client_clone,
                &orders,
                &exec_sender,
                clock.get_time_ns(),
                account_id,
                ib_account,
                request_timeout_secs,
                orders_to_cancel,
                &provider,
                cmd.order_side,
            )
            .await
            {
                tracing::error!("Error canceling all orders: {e}");
            }
        };

        self.pending_tasks
            .spawn(future)
            .context("failed to register IB execution command task")?;

        Ok(())
    }

    fn batch_cancel_orders(&self, cmd: BatchCancelOrders) -> anyhow::Result<()> {
        // Cancel each order in the batch
        for cancel_cmd in cmd.cancels {
            self.cancel_order(cancel_cmd)?;
        }
        Ok(())
    }

    // Reconciliation creates orders a previous session left working at IB; without tracking,
    // their IB status updates are dropped as untracked.
    fn register_external_order(
        &self,
        client_order_id: ClientOrderId,
        venue_order_id: VenueOrderId,
        _instrument_id: InstrumentId,
        _strategy_id: StrategyId,
        _ts_init: UnixNanos,
    ) {
        let order = match self.core.get_order(&client_order_id) {
            Ok(order) if order.is_open() => order,
            Ok(_) => return,
            Err(e) => {
                tracing::warn!("Cannot track external IB order {client_order_id}: {e}");
                return;
            }
        };
        let order_selector = match IbOrderSelector::from_venue_order_id(&venue_order_id) {
            Ok(order_selector) => order_selector,
            Err(e) => {
                tracing::warn!("Cannot track external IB order {client_order_id}: {e}");
                return;
            }
        };
        let Some(client) = self.ib_client.as_ref() else {
            tracing::warn!(
                "Cannot track external IB order {client_order_id}: IB client not connected"
            );
            return;
        };

        let client = client.as_arc().clone();
        let orders = self.orders.clone();
        let ib_account = self.ib_account;
        let request_timeout_secs = self.config.request_timeout;

        let future = async move {
            let result = Self::resolve_ib_order_id(
                &client,
                order_selector,
                ib_account,
                request_timeout_secs,
            )
            .await
            .and_then(|ib_order_id| {
                Self::cache_recovered_order_tracking(ib_order_id, &order, &orders)
            });

            // A filled order awaiting its deferred fills is still open in the cache but no
            // longer listed by IB; cancels resolve `PERM-` IDs on demand either way
            if let Err(e) = result {
                tracing::debug!("Cannot track external IB order {client_order_id}: {e:#}");
            }
        };

        if let Err(e) = self.pending_tasks.spawn(future) {
            tracing::warn!("Cannot track external IB order {client_order_id}: {e}");
        }
    }
}

fn validate_order(order: &impl Order) -> Result<(), OrderDeniedReason> {
    if order.is_reduce_only() {
        return Err(OrderDeniedReason::UnsupportedReduceOnly);
    }

    Ok(())
}

impl InteractiveBrokersExecutionClient {
    pub(super) fn execution_filter(ib_account: Ustr, start: Option<UnixNanos>) -> ExecutionFilter {
        let time = start.map_or_else(String::new, |start| {
            start
                .to_datetime_utc()
                .strftime("%Y%m%d-%H:%M:%S")
                .to_string()
        });

        ExecutionFilter {
            client_id: None,
            account_code: ib_account.to_string(),
            time,
            symbol: String::new(),
            security_type: String::new(),
            exchange: String::new(),
            side: None,
            last_n_days: 0,
            specific_dates: Vec::new(),
        }
    }

    fn is_ready_for_order_request(&self) -> bool {
        if !self.is_connected.load(Ordering::Relaxed) {
            return false;
        }

        if !self
            .ib_client
            .as_ref()
            .is_some_and(|client| client.is_connected())
        {
            return false;
        }

        *self.next_order_id.lock() > 0
    }

    // Selects open orders for the instrument and account from the cache, the tracker, and
    // incarnation groups, keeping only the requested side when one is set
    fn cancel_all_targets(
        &self,
        cmd: &CancelAllOrders,
    ) -> anyhow::Result<Vec<(ClientOrderId, Option<VenueOrderId>)>> {
        let cache = self.core.cache();
        let mut selected: Vec<_> = cache
            .orders_open(
                None,
                Some(&cmd.instrument_id),
                None,
                Some(&self.core.account_id),
                cmd.order_side,
            )
            .iter()
            .map(|order| (order.client_order_id(), order.venue_order_id()))
            .collect();
        let state = self.orders.lock()?;
        selected.extend(state.active_orders.iter().filter_map(|(order_id, order)| {
            (order.instrument_id == cmd.instrument_id
                && cmd.order_side.is_none_or(|side| side == order.order_side))
            .then_some((
                order.client_order_id,
                Some(parse::ib_venue_order_id(*order_id, order.perm_id)),
            ))
        }));
        selected.extend(
            state
                .group_cancel_candidates(cmd.instrument_id, self.core.account_id, cmd.order_side)
                .map(|parent| (parent, None)),
        );
        selected.sort_by_key(|(id, _)| id.to_string());
        selected.dedup_by_key(|(id, _)| *id);
        Ok(selected)
    }

    fn ensure_client_ready_for_order_request(&self, request: &str) -> Result<(), String> {
        if self.is_ready_for_order_request() {
            return Ok(());
        }

        let reason = format!("Interactive Brokers client is not ready; refusing to {request}");
        tracing::warn!("{reason}");
        Err(reason)
    }

    fn deny_submit_order_not_ready(&self, cmd: &SubmitOrder, reason: &str) -> anyhow::Result<()> {
        let reason = coded_denial_reason(DENIAL_CLIENT_NOT_READY, reason);
        Self::send_order_denied(
            cmd.order_init.trader_id,
            cmd.strategy_id,
            cmd.instrument_id,
            cmd.order_init.client_order_id,
            &reason,
        )
    }

    fn deny_submit_order_list(&self, cmd: &SubmitOrderList, reason: &str) -> anyhow::Result<()> {
        for order_init in &cmd.order_inits {
            Self::send_order_denied(
                order_init.trader_id,
                cmd.strategy_id,
                cmd.instrument_id,
                order_init.client_order_id,
                reason,
            )?;
        }

        Ok(())
    }

    #[allow(clippy::too_many_arguments)] // Event construction preserves the query context.
    fn send_inferred_order_canceled(
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        venue_order_id: VenueOrderId,
        account_id: AccountId,
        ts_init: UnixNanos,
        exec_sender: &EventSender<ExecutionEvent>,
        context: &str,
    ) {
        let event = OrderCanceled::new(
            trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            UUID4::new(),
            ts_init,
            ts_init,
            false,
            Some(venue_order_id),
            Some(account_id),
            None,
        );

        if exec_sender
            .send(ExecutionEvent::Order(OrderEventAny::Canceled(event)))
            .is_err()
        {
            tracing::error!("query_order: failed to send inferred order canceled event");
        } else {
            tracing::debug!(
                "query_order: inferred cancel for {} ({})",
                client_order_id,
                context
            );
        }
    }

    fn send_order_denied(
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        reason: &str,
    ) -> anyhow::Result<()> {
        let ts_event = get_atomic_clock_realtime().get_time_ns();
        let exec_sender = get_exec_event_sender();
        Self::send_order_denied_to(
            trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            reason,
            &exec_sender,
            ts_event,
        )
    }

    pub(super) fn send_order_denied_to(
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        reason: &str,
        exec_sender: &EventSender<ExecutionEvent>,
        ts_event: UnixNanos,
    ) -> anyhow::Result<()> {
        let event = OrderDenied::new(
            trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            Ustr::from(reason),
            UUID4::new(),
            ts_event,
            ts_event,
        );

        exec_sender
            .send(ExecutionEvent::Order(OrderEventAny::Denied(event)))
            .map_err(|e| anyhow::anyhow!("Failed to send order denied event: {e}"))
    }

    fn send_order_modify_rejected(
        cmd: &ModifyOrder,
        reason: &str,
        exec_sender: &EventSender<ExecutionEvent>,
        ts_event: UnixNanos,
        account_id: AccountId,
    ) -> anyhow::Result<()> {
        let event = OrderModifyRejected::new(
            cmd.trader_id,
            cmd.strategy_id,
            cmd.instrument_id,
            cmd.client_order_id,
            Ustr::from(reason),
            UUID4::new(),
            ts_event,
            ts_event,
            false,
            cmd.venue_order_id,
            Some(account_id),
        );
        exec_sender
            .send(ExecutionEvent::Order(OrderEventAny::ModifyRejected(event)))
            .map_err(|e| anyhow::anyhow!("Failed to send order modify rejected event: {e}"))
    }

    fn send_order_cancel_rejected(
        target_order: &OrderAny,
        reason: &str,
        exec_sender: &EventSender<ExecutionEvent>,
        ts_event: UnixNanos,
        account_id: AccountId,
    ) -> anyhow::Result<()> {
        let event = OrderCancelRejected::new(
            target_order.trader_id(),
            target_order.strategy_id(),
            target_order.instrument_id(),
            target_order.client_order_id(),
            Ustr::from(reason),
            UUID4::new(),
            ts_event,
            ts_event,
            false,
            target_order.venue_order_id(),
            Some(account_id),
        );
        exec_sender
            .send(ExecutionEvent::Order(OrderEventAny::CancelRejected(event)))
            .map_err(|e| anyhow::anyhow!("Failed to send order cancel rejected event: {e}"))
    }
}

impl InteractiveBrokersExecutionClient {
    fn parse_historical_fill_report(
        &self,
        cmd: &GenerateFillReports,
        exec_data: &ExecutionData,
        commission: f64,
        commission_currency: &str,
        ts_init: UnixNanos,
    ) -> Option<FillReport> {
        let instrument_id = match self.resolve_historical_execution_instrument_id(exec_data) {
            Ok(instrument_id) => instrument_id,
            Err(e) => {
                Self::warn_historical_fill_report_parse_error(exec_data, &e);
                return None;
            }
        };

        if let Some(filter_id) = cmd.instrument_id
            && instrument_id != filter_id
        {
            return None;
        }

        if let Some(filter_venue_order_id) = cmd.venue_order_id
            && ib_venue_order_id(exec_data.execution.order_id, exec_data.execution.perm_id)
                != filter_venue_order_id
        {
            return None;
        }

        if let Some(end) = cmd.end {
            match parse_execution_time(&exec_data.execution.time) {
                Ok(ts_event) if ts_event > end => return None,
                Ok(_) => {}
                Err(e) => {
                    Self::warn_historical_fill_report_parse_error(exec_data, &e);
                    return None;
                }
            }
        }

        match parse_execution_to_fill_report(
            &exec_data.execution,
            &exec_data.contract,
            commission,
            commission_currency,
            instrument_id,
            self.core.account_id,
            &self.instrument_provider,
            ts_init,
            None, // avg_px (not available in historical fills)
        ) {
            Ok(report) => Some(report),
            Err(e) => {
                Self::warn_historical_fill_report_parse_error(exec_data, &e);
                None
            }
        }
    }

    fn resolve_historical_execution_instrument_id(
        &self,
        exec_data: &ExecutionData,
    ) -> anyhow::Result<InstrumentId> {
        self.resolve_report_contract_instrument_id(&exec_data.contract)
    }

    fn resolve_report_contract_instrument_id(
        &self,
        contract: &Contract,
    ) -> anyhow::Result<InstrumentId> {
        self.instrument_provider
            .resolve_instrument_id_for_contract(contract)
            .context("Failed to resolve IBKR report contract to instrument ID")
    }

    fn position_avg_px_open(
        &self,
        instrument_id: &InstrumentId,
        instrument: &InstrumentAny,
        average_cost: f64,
    ) -> Option<Decimal> {
        if average_cost <= 0.0 {
            return None;
        }

        let price_magnifier = self.instrument_provider.get_price_magnifier(instrument_id) as f64;
        let multiplier = instrument.multiplier().as_f64();
        let converted_avg_cost = average_cost / (multiplier * price_magnifier);
        Decimal::from_f64_retain(converted_avg_cost)
            .map(|price| price.round_dp(instrument.price_precision() as u32))
    }

    fn warn_historical_fill_report_parse_error(exec_data: &ExecutionData, error: &anyhow::Error) {
        tracing::warn!(
            symbol = exec_data.contract.symbol.as_str(),
            sec_type = ?exec_data.contract.security_type,
            exchange = exec_data.contract.exchange.as_str(),
            primary_exchange = exec_data.contract.primary_exchange.as_str(),
            local_symbol = exec_data.contract.local_symbol.as_str(),
            con_id = exec_data.contract.contract_id,
            order_id = exec_data.execution.order_id,
            order_ref = exec_data.execution.order_reference.as_str(),
            execution_id = exec_data.execution.execution_id.as_str(),
            error = %error,
            "Failed to parse IBKR historical fill report",
        );
    }

    fn validate_cancel_order_target(
        cmd: &CancelOrder,
        target_order: &OrderAny,
    ) -> anyhow::Result<()> {
        anyhow::ensure!(
            cmd.client_order_id == target_order.client_order_id(),
            "command client order ID {} does not match cached order {}",
            cmd.client_order_id,
            target_order.client_order_id()
        );
        anyhow::ensure!(
            cmd.instrument_id == target_order.instrument_id(),
            "command instrument ID {} does not match cached order {}",
            cmd.instrument_id,
            target_order.instrument_id()
        );

        // Command actor IDs identify the requester and are not ownership evidence.
        if let (Some(command_venue_order_id), Some(target_venue_order_id)) =
            (cmd.venue_order_id.as_ref(), target_order.venue_order_id())
        {
            anyhow::ensure!(
                command_venue_order_id == &target_venue_order_id,
                "command venue order ID {command_venue_order_id} does not match cached order {target_venue_order_id}"
            );
        }

        Ok(())
    }

    /// Handles a cancel order asynchronously.
    ///
    /// # Errors
    ///
    /// Returns an error if broker order resolution or identity caching fails.
    #[allow(
        clippy::too_many_arguments,
        reason = "cancel routing requires the request, target order, venue client, and event context"
    )]
    async fn handle_cancel_order_async(
        cmd: &CancelOrder,
        target_order: &OrderAny,
        client: &Arc<Client>,
        orders: &OrderTracker,
        instrument_provider: &Arc<InteractiveBrokersInstrumentProvider>,
        exec_sender: &EventSender<ExecutionEvent>,
        ts_init: UnixNanos,
        account_id: AccountId,
        ib_account: Ustr,
        request_timeout_secs: u64,
    ) -> anyhow::Result<()> {
        if orders.lock()?.groups.contains_key(&cmd.client_order_id) {
            return Self::cancel_incarnation_group(
                cmd.client_order_id,
                client,
                orders,
                instrument_provider,
                exec_sender,
                ts_init,
                account_id,
                ib_account,
                request_timeout_secs,
                None,
            )
            .await;
        }
        let order_selector = if let Some(venue_order_id) = &cmd.venue_order_id {
            IbOrderSelector::from_venue_order_id(venue_order_id)?
        } else {
            let state = orders.lock()?;
            IbOrderSelector::OrderId(
                *state
                    .order_id_map
                    .get(&cmd.client_order_id)
                    .context("No IB order ID mapping found for client order ID")?,
            )
        };
        let ib_order_id =
            Self::resolve_ib_order_id(client, order_selector, ib_account, request_timeout_secs)
                .await?;
        Self::cache_recovered_order_tracking(ib_order_id, target_order, orders)?;
        let venue_order_id = target_order
            .venue_order_id()
            .unwrap_or_else(|| VenueOrderId::from(ib_order_id.to_string()));

        let timeout_dur = Duration::from_secs(request_timeout_secs);
        let send_error =
            match tokio::time::timeout(timeout_dur, client.cancel_order(ib_order_id, "")).await {
                Ok(Ok(_subscription)) => None,
                Ok(Err(e)) => match Self::classify_order_submit_error(&e) {
                    CommandFailure::NotSent(reason) | CommandFailure::VenueRejected(reason) => {
                        anyhow::bail!(reason)
                    }
                    CommandFailure::Ambiguous(reason) => Some(reason),
                },
                Err(_) => Some(format!(
                    "cancel request timed out after {request_timeout_secs} seconds"
                )),
            };

        if let Some(send_error) = send_error {
            tracing::warn!(
                "Cancel outcome is unknown after attempting to send order {} to IB: {}; querying venue state",
                cmd.client_order_id,
                send_error
            );
            return Self::resolve_ambiguous_cancel(
                target_order,
                client,
                order_selector,
                account_id,
                ib_account,
                request_timeout_secs,
                &send_error,
                exec_sender,
                ts_init,
            )
            .await;
        }

        if let Err(e) = Self::emit_order_pending_cancel(
            ib_order_id,
            cmd.client_order_id,
            venue_order_id,
            orders,
            exec_sender,
            ts_init,
            account_id,
        ) {
            tracing::error!(
                "Cancel request for order {} was sent, but OrderPendingCancel emission failed: {e}",
                cmd.client_order_id
            );
        }

        Ok(())
    }

    #[allow(clippy::too_many_arguments)] // Resolution needs the original cancel context.
    async fn resolve_ambiguous_cancel(
        target_order: &OrderAny,
        client: &Arc<Client>,
        order_selector: IbOrderSelector,
        account_id: AccountId,
        ib_account: Ustr,
        request_timeout_secs: u64,
        send_error: &str,
        exec_sender: &EventSender<ExecutionEvent>,
        ts_init: UnixNanos,
    ) -> anyhow::Result<()> {
        let timeout_dur = Duration::from_secs(request_timeout_secs);
        let remains_open = tokio::time::timeout(timeout_dur, async {
            let mut subscription = client.all_open_orders().await?.filter_data();
            while let Some(result) = subscription.next().await {
                let Orders::OrderData(data) = result? else {
                    continue;
                };

                if !data.order.account.is_empty() && data.order.account != ib_account {
                    continue;
                }

                if order_selector.matches(data.order_id, data.order.perm_id) {
                    return Ok::<bool, ibapi::Error>(true);
                }
            }
            Ok(false)
        })
        .await
        .context("Timed out querying open orders after ambiguous cancel")??;

        if remains_open {
            let reason = format!(
                "Cancel was not confirmed after transport error; order remains open at IB: {send_error}"
            );
            return Self::send_order_cancel_rejected(
                target_order,
                &reason,
                exec_sender,
                ts_init,
                account_id,
            );
        }

        let completed_status = tokio::time::timeout(timeout_dur, async {
            let mut subscription = client.completed_orders(false).await?.filter_data();
            while let Some(result) = subscription.next().await {
                let Orders::OrderData(data) = result? else {
                    continue;
                };

                if !data.order.account.is_empty() && data.order.account != ib_account {
                    continue;
                }
                let matches_order = order_selector.matches(data.order_id, data.order.perm_id);
                if matches_order {
                    return Ok::<Option<OrderStatusKind>, ibapi::Error>(Some(
                        data.order_state.status,
                    ));
                }
            }
            Ok(None)
        })
        .await
        .context("Timed out querying completed orders after ambiguous cancel")??;

        if completed_status.as_ref().is_some_and(|status| {
            matches!(
                status,
                OrderStatusKind::Cancelled | OrderStatusKind::ApiCancelled
            )
        }) {
            let event = OrderCanceled::new(
                target_order.trader_id(),
                target_order.strategy_id(),
                target_order.instrument_id(),
                target_order.client_order_id(),
                UUID4::new(),
                ts_init,
                ts_init,
                false,
                Some(order_selector.venue_order_id()),
                Some(account_id),
                None,
            );
            exec_sender
                .send(ExecutionEvent::Order(OrderEventAny::Canceled(event)))
                .map_err(|e| {
                    anyhow::anyhow!("Failed to send resolved order canceled event: {e}")
                })?;
            return Ok(());
        }

        let reason = completed_status.map_or_else(
            || {
                format!(
                    "Cancel outcome could not be resolved from IB open or completed orders: {send_error}"
                )
            },
            |status| {
                format!(
                    "Cancel did not complete; IB reports order status {}",
                    status.as_str()
                )
            },
        );
        Self::send_order_cancel_rejected(target_order, &reason, exec_sender, ts_init, account_id)
    }

    pub(super) async fn resolve_ib_order_id(
        client: &Arc<Client>,
        order_selector: IbOrderSelector,
        ib_account: Ustr,
        request_timeout_secs: u64,
    ) -> anyhow::Result<i32> {
        let target_perm_id = match order_selector {
            IbOrderSelector::OrderId(order_id) => return Ok(order_id),
            IbOrderSelector::PermId(perm_id) => perm_id,
        };

        let timeout_dur = Duration::from_secs(request_timeout_secs);
        tokio::time::timeout(timeout_dur, async {
            let mut subscription = client.all_open_orders().await?.filter_data();
            let mut routes = AHashMap::<(i32, i32), AHashSet<i64>>::new();
            let mut target_routes = AHashSet::new();

            while let Some(result) = subscription.next().await {
                let Orders::OrderData(data) = result? else { continue; };
                if data.order.account != ib_account || !Self::is_active_open_order(&data.order) { continue; }
                let route = (data.order.client_id, data.order_id);
                routes.entry(route).or_default().insert(data.order.perm_id);
                if data.order.perm_id == target_perm_id { target_routes.insert(route); }
            }
            anyhow::ensure!(target_routes.len() == 1,
                "Cannot resolve PERM-{target_perm_id}: expected one broker route, found {}", target_routes.len());
            let route = *target_routes.iter().next().expect("one route was checked");
            anyhow::ensure!(route.0 == client.client_id() && route.1 != 0,
                "Cannot resolve PERM-{target_perm_id}: order is not bound to this API client");
            anyhow::ensure!(routes.get(&route).is_some_and(|ids| ids.len() == 1),
                "Cannot resolve PERM-{target_perm_id}: broker route is shared by distinct permanent IDs");
            Ok(route.1)
        }).await.context("timed out resolving IB permanent order identity")?
    }

    pub(super) fn is_active_open_order(order: &ibapi::orders::Order) -> bool {
        !order.deactivate
    }

    pub(super) fn is_definitive_order_submit_error(error: &ibapi::Error) -> bool {
        matches!(
            error,
            ibapi::Error::InvalidArgument(_) | ibapi::Error::ServerVersion(_, _, _)
        )
    }

    pub(super) fn classify_order_submit_error(error: &ibapi::Error) -> CommandFailure {
        let reason = error.to_string();

        if Self::is_definitive_order_submit_error(error) {
            CommandFailure::not_sent(reason)
        } else if matches!(
            error,
            ibapi::Error::Notice(notice)
                if notice.category() == ibapi::NoticeCategory::OrderRejection
        ) {
            CommandFailure::venue_rejected(reason)
        } else {
            CommandFailure::ambiguous(reason)
        }
    }

    async fn handle_cancel_all_orders_async(
        client: &Arc<Client>,
        orders: &OrderTracker,
        exec_sender: &EventSender<ExecutionEvent>,
        ts_init: UnixNanos,
        account_id: AccountId,
        ib_account: Ustr,
        request_timeout_secs: u64,
        orders_to_cancel: Vec<(ClientOrderId, Option<VenueOrderId>)>,
        provider: &Arc<InteractiveBrokersInstrumentProvider>,
        order_side: Option<OrderSide>,
    ) -> anyhow::Result<()> {
        let mut handled = AHashSet::new();
        let mut individual = Vec::new();

        for (id, venue_id) in orders_to_cancel {
            let parent = orders.lock()?.group_for_client(id);
            if let Some(parent) = parent {
                if handled.insert(parent) {
                    Self::cancel_incarnation_group(
                        parent,
                        client,
                        orders,
                        provider,
                        exec_sender,
                        ts_init,
                        account_id,
                        ib_account,
                        request_timeout_secs,
                        order_side,
                    )
                    .await?;
                }
            } else {
                individual.push((id, venue_id));
            }
        }
        let orders_to_cancel = individual;
        // Get all IB order selectors first, then drop the guard before awaiting
        let order_selectors: Vec<(ClientOrderId, IbOrderSelector, Option<VenueOrderId>)> = {
            let state = orders.lock()?;

            orders_to_cancel
                .into_iter()
                .filter_map(|(client_order_id, venue_order_id)| {
                    match state.cancel_selector(client_order_id, venue_order_id.as_ref()) {
                        Ok(Some(order_selector)) => {
                            Some((client_order_id, order_selector, venue_order_id))
                        }
                        Ok(None) => None,
                        Err(e) => {
                            tracing::error!(
                                "Failed to resolve cancel-all order {client_order_id}: {e}"
                            );
                            None
                        }
                    }
                })
                .collect()
        };

        // Now cancel each order (guard is dropped, so we can await)
        for (client_order_id, order_selector, venue_order_id) in order_selectors {
            let ib_order_id = match Self::resolve_ib_order_id(
                client,
                order_selector,
                ib_account,
                request_timeout_secs,
            )
            .await
            {
                Ok(ib_order_id) => ib_order_id,
                Err(e) => {
                    tracing::error!("Failed resolve cancel-all order {client_order_id}: {e}");
                    continue;
                }
            };
            let venue_order_id =
                venue_order_id.unwrap_or_else(|| VenueOrderId::from(ib_order_id.to_string()));

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
                    venue_order_id,
                    orders,
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

        tracing::debug!("Finished canceling all orders");

        Ok(())
    }

    pub(super) fn emit_order_pending_cancel(
        _order_id: i32,
        client_order_id: ClientOrderId,
        venue_order_id: VenueOrderId,
        orders: &OrderTracker,
        exec_sender: &EventSender<ExecutionEvent>,
        ts_init: UnixNanos,
        account_id: AccountId,
    ) -> anyhow::Result<()> {
        let mut state = orders.lock()?;
        let order = state
            .order_by_client_mut(client_order_id)
            .context("Tracked state not found for pending cancel order")?;
        if order.pending_cancel {
            return Ok(());
        }
        order.pending_cancel = true;
        let instrument_id = order.instrument_id;
        let trader_id = order.trader_id;
        let strategy_id = order.strategy_id;
        drop(state);

        let event = OrderPendingCancel::new(
            trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            Some(account_id),
            UUID4::new(),
            ts_init,
            ts_init,
            false,
            Some(venue_order_id),
        );

        exec_sender
            .send(ExecutionEvent::Order(OrderEventAny::PendingCancel(event)))
            .map_err(|e| anyhow::anyhow!("Failed to send order pending cancel event: {e}"))?;

        Ok(())
    }
}
