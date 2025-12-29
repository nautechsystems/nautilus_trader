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

//! Python bindings for Strategy with complete order and position management.

use std::{
    any::Any,
    cell::{RefCell, UnsafeCell},
    fmt::Debug,
    num::NonZeroUsize,
    ops::{Deref, DerefMut},
    rc::Rc,
};

use indexmap::IndexMap;
use nautilus_common::{
    actor::{
        Actor, DataActor,
        data_actor::DataActorCore,
        registry::{get_actor_registry, try_get_actor_unchecked},
    },
    cache::Cache,
    clock::Clock,
    component::{Component, get_component_registry},
    enums::ComponentState,
    python::{cache::PyCache, clock::PyClock, logging::PyLogger},
    signal::Signal,
    timer::{TimeEvent, TimeEventCallback},
};
use nautilus_core::{
    nanos::UnixNanos,
    python::{IntoPyObjectNautilusExt, to_pyruntime_err, to_pyvalue_err},
};
use nautilus_model::{
    data::{
        Bar, BarType, DataType, FundingRateUpdate, IndexPriceUpdate, InstrumentStatus,
        MarkPriceUpdate, OrderBookDeltas, QuoteTick, TradeTick, close::InstrumentClose,
    },
    enums::{BookType, OmsType, OrderSide, PositionSide, TimeInForce},
    events::{
        OrderAccepted, OrderCancelRejected, OrderDenied, OrderEmulated, OrderExpired,
        OrderInitialized, OrderModifyRejected, OrderPendingCancel, OrderPendingUpdate,
        OrderRejected, OrderReleased, OrderSubmitted, OrderTriggered, OrderUpdated,
        PositionChanged, PositionClosed, PositionOpened,
    },
    identifiers::{AccountId, ClientId, InstrumentId, PositionId, StrategyId, TraderId, Venue},
    instruments::InstrumentAny,
    orderbook::OrderBook,
    orders::OrderAny,
    position::Position,
    python::{instruments::instrument_any_to_pyobject, orders::pyobject_to_order_any},
    types::{Price, Quantity},
};
use nautilus_portfolio::portfolio::Portfolio;
use pyo3::{exceptions::PyValueError, prelude::*};
use ustr::Ustr;

use crate::strategy::{Strategy, StrategyConfig, StrategyCore};

#[pyo3::pymethods]
impl StrategyConfig {
    #[new]
    #[pyo3(signature = (
        strategy_id=None,
        order_id_tag=None,
        oms_type=None,
        external_order_claims=None,
        manage_contingent_orders=false,
        manage_gtd_expiry=false,
        use_uuid_client_order_ids=false,
        use_hyphens_in_client_order_ids=true,
        log_events=true,
        log_commands=true,
        log_rejected_due_post_only_as_warning=true
    ))]
    #[allow(clippy::too_many_arguments)]
    fn py_new(
        strategy_id: Option<StrategyId>,
        order_id_tag: Option<String>,
        oms_type: Option<OmsType>,
        external_order_claims: Option<Vec<InstrumentId>>,
        manage_contingent_orders: bool,
        manage_gtd_expiry: bool,
        use_uuid_client_order_ids: bool,
        use_hyphens_in_client_order_ids: bool,
        log_events: bool,
        log_commands: bool,
        log_rejected_due_post_only_as_warning: bool,
    ) -> Self {
        Self {
            strategy_id,
            order_id_tag,
            oms_type,
            external_order_claims,
            manage_contingent_orders,
            manage_gtd_expiry,
            use_uuid_client_order_ids,
            use_hyphens_in_client_order_ids,
            log_events,
            log_commands,
            log_rejected_due_post_only_as_warning,
        }
    }

    #[getter]
    fn strategy_id(&self) -> Option<StrategyId> {
        self.strategy_id
    }

    #[getter]
    fn order_id_tag(&self) -> Option<&String> {
        self.order_id_tag.as_ref()
    }

    #[getter]
    fn oms_type(&self) -> Option<OmsType> {
        self.oms_type
    }

    #[getter]
    fn manage_contingent_orders(&self) -> bool {
        self.manage_contingent_orders
    }

    #[getter]
    fn manage_gtd_expiry(&self) -> bool {
        self.manage_gtd_expiry
    }

    #[getter]
    fn use_uuid_client_order_ids(&self) -> bool {
        self.use_uuid_client_order_ids
    }

    #[getter]
    fn use_hyphens_in_client_order_ids(&self) -> bool {
        self.use_hyphens_in_client_order_ids
    }

    #[getter]
    fn log_events(&self) -> bool {
        self.log_events
    }

    #[getter]
    fn log_commands(&self) -> bool {
        self.log_commands
    }

    #[getter]
    fn log_rejected_due_post_only_as_warning(&self) -> bool {
        self.log_rejected_due_post_only_as_warning
    }
}

/// Inner state of PyStrategy, shared between Python wrapper and Rust registries.
pub struct PyStrategyInner {
    core: StrategyCore,
    py_self: Option<Py<PyAny>>,
    clock: PyClock,
    logger: PyLogger,
}

impl Debug for PyStrategyInner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PyStrategyInner")
            .field("core", &self.core)
            .field("py_self", &self.py_self.as_ref().map(|_| "<Py<PyAny>>"))
            .field("clock", &self.clock)
            .field("logger", &self.logger)
            .finish()
    }
}

impl PyStrategyInner {
    fn dispatch_on_start(&self) -> PyResult<()> {
        if let Some(ref py_self) = self.py_self {
            Python::attach(|py| py_self.call_method0(py, "on_start"))?;
        }
        Ok(())
    }

    fn dispatch_on_stop(&self) -> PyResult<()> {
        if let Some(ref py_self) = self.py_self {
            Python::attach(|py| py_self.call_method0(py, "on_stop"))?;
        }
        Ok(())
    }

    fn dispatch_on_resume(&self) -> PyResult<()> {
        if let Some(ref py_self) = self.py_self {
            Python::attach(|py| py_self.call_method0(py, "on_resume"))?;
        }
        Ok(())
    }

    fn dispatch_on_reset(&self) -> PyResult<()> {
        if let Some(ref py_self) = self.py_self {
            Python::attach(|py| py_self.call_method0(py, "on_reset"))?;
        }
        Ok(())
    }

    fn dispatch_on_dispose(&self) -> PyResult<()> {
        if let Some(ref py_self) = self.py_self {
            Python::attach(|py| py_self.call_method0(py, "on_dispose"))?;
        }
        Ok(())
    }

    fn dispatch_on_degrade(&self) -> PyResult<()> {
        if let Some(ref py_self) = self.py_self {
            Python::attach(|py| py_self.call_method0(py, "on_degrade"))?;
        }
        Ok(())
    }

    fn dispatch_on_fault(&self) -> PyResult<()> {
        if let Some(ref py_self) = self.py_self {
            Python::attach(|py| py_self.call_method0(py, "on_fault"))?;
        }
        Ok(())
    }

    fn dispatch_on_time_event(&self, event: &TimeEvent) -> PyResult<()> {
        if let Some(ref py_self) = self.py_self {
            Python::attach(|py| {
                py_self.call_method1(py, "on_time_event", (event.clone().into_py_any_unwrap(py),))
            })?;
        }
        Ok(())
    }

    fn dispatch_on_order_initialized(&self, event: OrderInitialized) -> PyResult<()> {
        if let Some(ref py_self) = self.py_self {
            Python::attach(|py| {
                py_self.call_method1(py, "on_order_initialized", (event.into_py_any_unwrap(py),))
            })?;
        }
        Ok(())
    }

    fn dispatch_on_order_denied(&self, event: OrderDenied) -> PyResult<()> {
        if let Some(ref py_self) = self.py_self {
            Python::attach(|py| {
                py_self.call_method1(py, "on_order_denied", (event.into_py_any_unwrap(py),))
            })?;
        }
        Ok(())
    }

    fn dispatch_on_order_emulated(&self, event: OrderEmulated) -> PyResult<()> {
        if let Some(ref py_self) = self.py_self {
            Python::attach(|py| {
                py_self.call_method1(py, "on_order_emulated", (event.into_py_any_unwrap(py),))
            })?;
        }
        Ok(())
    }

    fn dispatch_on_order_released(&self, event: OrderReleased) -> PyResult<()> {
        if let Some(ref py_self) = self.py_self {
            Python::attach(|py| {
                py_self.call_method1(py, "on_order_released", (event.into_py_any_unwrap(py),))
            })?;
        }
        Ok(())
    }

    fn dispatch_on_order_submitted(&self, event: OrderSubmitted) -> PyResult<()> {
        if let Some(ref py_self) = self.py_self {
            Python::attach(|py| {
                py_self.call_method1(py, "on_order_submitted", (event.into_py_any_unwrap(py),))
            })?;
        }
        Ok(())
    }

    fn dispatch_on_order_rejected(&self, event: OrderRejected) -> PyResult<()> {
        if let Some(ref py_self) = self.py_self {
            Python::attach(|py| {
                py_self.call_method1(py, "on_order_rejected", (event.into_py_any_unwrap(py),))
            })?;
        }
        Ok(())
    }

    fn dispatch_on_order_accepted(&self, event: OrderAccepted) -> PyResult<()> {
        if let Some(ref py_self) = self.py_self {
            Python::attach(|py| {
                py_self.call_method1(py, "on_order_accepted", (event.into_py_any_unwrap(py),))
            })?;
        }
        Ok(())
    }

    fn dispatch_on_order_expired(&self, event: OrderExpired) -> PyResult<()> {
        if let Some(ref py_self) = self.py_self {
            Python::attach(|py| {
                py_self.call_method1(py, "on_order_expired", (event.into_py_any_unwrap(py),))
            })?;
        }
        Ok(())
    }

    fn dispatch_on_order_triggered(&self, event: OrderTriggered) -> PyResult<()> {
        if let Some(ref py_self) = self.py_self {
            Python::attach(|py| {
                py_self.call_method1(py, "on_order_triggered", (event.into_py_any_unwrap(py),))
            })?;
        }
        Ok(())
    }

    fn dispatch_on_order_pending_update(&self, event: OrderPendingUpdate) -> PyResult<()> {
        if let Some(ref py_self) = self.py_self {
            Python::attach(|py| {
                py_self.call_method1(
                    py,
                    "on_order_pending_update",
                    (event.into_py_any_unwrap(py),),
                )
            })?;
        }
        Ok(())
    }

    fn dispatch_on_order_pending_cancel(&self, event: OrderPendingCancel) -> PyResult<()> {
        if let Some(ref py_self) = self.py_self {
            Python::attach(|py| {
                py_self.call_method1(
                    py,
                    "on_order_pending_cancel",
                    (event.into_py_any_unwrap(py),),
                )
            })?;
        }
        Ok(())
    }

    fn dispatch_on_order_modify_rejected(&self, event: OrderModifyRejected) -> PyResult<()> {
        if let Some(ref py_self) = self.py_self {
            Python::attach(|py| {
                py_self.call_method1(
                    py,
                    "on_order_modify_rejected",
                    (event.into_py_any_unwrap(py),),
                )
            })?;
        }
        Ok(())
    }

    fn dispatch_on_order_cancel_rejected(&self, event: OrderCancelRejected) -> PyResult<()> {
        if let Some(ref py_self) = self.py_self {
            Python::attach(|py| {
                py_self.call_method1(
                    py,
                    "on_order_cancel_rejected",
                    (event.into_py_any_unwrap(py),),
                )
            })?;
        }
        Ok(())
    }

    fn dispatch_on_order_updated(&self, event: OrderUpdated) -> PyResult<()> {
        if let Some(ref py_self) = self.py_self {
            Python::attach(|py| {
                py_self.call_method1(py, "on_order_updated", (event.into_py_any_unwrap(py),))
            })?;
        }
        Ok(())
    }

    // TODO: Position events don't have PyO3 bindings yet, so these are stubbed
    fn dispatch_on_position_opened(&self, _event: PositionOpened) -> PyResult<()> {
        Ok(())
    }

    fn dispatch_on_position_changed(&self, _event: PositionChanged) -> PyResult<()> {
        Ok(())
    }

    fn dispatch_on_position_closed(&self, _event: PositionClosed) -> PyResult<()> {
        Ok(())
    }

    fn dispatch_on_data(&mut self, data: Py<PyAny>) -> PyResult<()> {
        if let Some(ref py_self) = self.py_self {
            Python::attach(|py| py_self.call_method1(py, "on_data", (data,)))?;
        }
        Ok(())
    }

    fn dispatch_on_signal(&mut self, signal: &Signal) -> PyResult<()> {
        if let Some(ref py_self) = self.py_self {
            Python::attach(|py| {
                py_self.call_method1(py, "on_signal", (signal.clone().into_py_any_unwrap(py),))
            })?;
        }
        Ok(())
    }

    fn dispatch_on_instrument(&mut self, instrument: Py<PyAny>) -> PyResult<()> {
        if let Some(ref py_self) = self.py_self {
            Python::attach(|py| py_self.call_method1(py, "on_instrument", (instrument,)))?;
        }
        Ok(())
    }

    fn dispatch_on_quote(&mut self, quote: QuoteTick) -> PyResult<()> {
        if let Some(ref py_self) = self.py_self {
            Python::attach(|py| {
                py_self.call_method1(py, "on_quote", (quote.into_py_any_unwrap(py),))
            })?;
        }
        Ok(())
    }

    fn dispatch_on_trade(&mut self, trade: TradeTick) -> PyResult<()> {
        if let Some(ref py_self) = self.py_self {
            Python::attach(|py| {
                py_self.call_method1(py, "on_trade", (trade.into_py_any_unwrap(py),))
            })?;
        }
        Ok(())
    }

    fn dispatch_on_bar(&mut self, bar: Bar) -> PyResult<()> {
        if let Some(ref py_self) = self.py_self {
            Python::attach(|py| py_self.call_method1(py, "on_bar", (bar.into_py_any_unwrap(py),)))?;
        }
        Ok(())
    }

    fn dispatch_on_book_deltas(&mut self, deltas: OrderBookDeltas) -> PyResult<()> {
        if let Some(ref py_self) = self.py_self {
            Python::attach(|py| {
                py_self.call_method1(py, "on_book_deltas", (deltas.into_py_any_unwrap(py),))
            })?;
        }
        Ok(())
    }

    fn dispatch_on_book(&mut self, book: &OrderBook) -> PyResult<()> {
        if let Some(ref py_self) = self.py_self {
            Python::attach(|py| {
                py_self.call_method1(py, "on_book", (book.clone().into_py_any_unwrap(py),))
            })?;
        }
        Ok(())
    }

    fn dispatch_on_mark_price(&mut self, mark_price: MarkPriceUpdate) -> PyResult<()> {
        if let Some(ref py_self) = self.py_self {
            Python::attach(|py| {
                py_self.call_method1(py, "on_mark_price", (mark_price.into_py_any_unwrap(py),))
            })?;
        }
        Ok(())
    }

    fn dispatch_on_index_price(&mut self, index_price: IndexPriceUpdate) -> PyResult<()> {
        if let Some(ref py_self) = self.py_self {
            Python::attach(|py| {
                py_self.call_method1(py, "on_index_price", (index_price.into_py_any_unwrap(py),))
            })?;
        }
        Ok(())
    }

    fn dispatch_on_funding_rate(&mut self, funding_rate: FundingRateUpdate) -> PyResult<()> {
        if let Some(ref py_self) = self.py_self {
            Python::attach(|py| {
                py_self.call_method1(
                    py,
                    "on_funding_rate",
                    (funding_rate.into_py_any_unwrap(py),),
                )
            })?;
        }
        Ok(())
    }

    fn dispatch_on_instrument_status(&mut self, data: InstrumentStatus) -> PyResult<()> {
        if let Some(ref py_self) = self.py_self {
            Python::attach(|py| {
                py_self.call_method1(py, "on_instrument_status", (data.into_py_any_unwrap(py),))
            })?;
        }
        Ok(())
    }

    fn dispatch_on_instrument_close(&mut self, update: InstrumentClose) -> PyResult<()> {
        if let Some(ref py_self) = self.py_self {
            Python::attach(|py| {
                py_self.call_method1(py, "on_instrument_close", (update.into_py_any_unwrap(py),))
            })?;
        }
        Ok(())
    }
}

impl Deref for PyStrategyInner {
    type Target = DataActorCore;

    fn deref(&self) -> &Self::Target {
        &self.core.actor
    }
}

impl DerefMut for PyStrategyInner {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.core.actor
    }
}

impl Strategy for PyStrategyInner {
    fn core_mut(&mut self) -> &mut StrategyCore {
        &mut self.core
    }

    fn on_order_initialized(&mut self, event: OrderInitialized) {
        let _ = self.dispatch_on_order_initialized(event);
    }

    fn on_order_denied(&mut self, event: OrderDenied) {
        let _ = self.dispatch_on_order_denied(event);
    }

    fn on_order_emulated(&mut self, event: OrderEmulated) {
        let _ = self.dispatch_on_order_emulated(event);
    }

    fn on_order_released(&mut self, event: OrderReleased) {
        let _ = self.dispatch_on_order_released(event);
    }

    fn on_order_submitted(&mut self, event: OrderSubmitted) {
        let _ = self.dispatch_on_order_submitted(event);
    }

    fn on_order_rejected(&mut self, event: OrderRejected) {
        let _ = self.dispatch_on_order_rejected(event);
    }

    fn on_order_accepted(&mut self, event: OrderAccepted) {
        let _ = self.dispatch_on_order_accepted(event);
    }

    fn on_order_expired(&mut self, event: OrderExpired) {
        let _ = self.dispatch_on_order_expired(event);
    }

    fn on_order_triggered(&mut self, event: OrderTriggered) {
        let _ = self.dispatch_on_order_triggered(event);
    }

    fn on_order_pending_update(&mut self, event: OrderPendingUpdate) {
        let _ = self.dispatch_on_order_pending_update(event);
    }

    fn on_order_pending_cancel(&mut self, event: OrderPendingCancel) {
        let _ = self.dispatch_on_order_pending_cancel(event);
    }

    fn on_order_modify_rejected(&mut self, event: OrderModifyRejected) {
        let _ = self.dispatch_on_order_modify_rejected(event);
    }

    fn on_order_cancel_rejected(&mut self, event: OrderCancelRejected) {
        let _ = self.dispatch_on_order_cancel_rejected(event);
    }

    fn on_order_updated(&mut self, event: OrderUpdated) {
        let _ = self.dispatch_on_order_updated(event);
    }

    fn on_position_opened(&mut self, event: PositionOpened) {
        let _ = self.dispatch_on_position_opened(event);
    }

    fn on_position_changed(&mut self, event: PositionChanged) {
        let _ = self.dispatch_on_position_changed(event);
    }

    fn on_position_closed(&mut self, event: PositionClosed) {
        let _ = self.dispatch_on_position_closed(event);
    }
}

impl DataActor for PyStrategyInner {
    fn on_start(&mut self) -> anyhow::Result<()> {
        Strategy::on_start(self)?;
        self.dispatch_on_start()
            .map_err(|e| anyhow::anyhow!("Python on_start failed: {e}"))
    }

    fn on_stop(&mut self) -> anyhow::Result<()> {
        self.dispatch_on_stop()
            .map_err(|e| anyhow::anyhow!("Python on_stop failed: {e}"))
    }

    fn on_resume(&mut self) -> anyhow::Result<()> {
        self.dispatch_on_resume()
            .map_err(|e| anyhow::anyhow!("Python on_resume failed: {e}"))
    }

    fn on_reset(&mut self) -> anyhow::Result<()> {
        self.dispatch_on_reset()
            .map_err(|e| anyhow::anyhow!("Python on_reset failed: {e}"))
    }

    fn on_dispose(&mut self) -> anyhow::Result<()> {
        self.dispatch_on_dispose()
            .map_err(|e| anyhow::anyhow!("Python on_dispose failed: {e}"))
    }

    fn on_degrade(&mut self) -> anyhow::Result<()> {
        self.dispatch_on_degrade()
            .map_err(|e| anyhow::anyhow!("Python on_degrade failed: {e}"))
    }

    fn on_fault(&mut self) -> anyhow::Result<()> {
        self.dispatch_on_fault()
            .map_err(|e| anyhow::anyhow!("Python on_fault failed: {e}"))
    }

    fn on_time_event(&mut self, event: &TimeEvent) -> anyhow::Result<()> {
        Strategy::on_time_event(self, event)?;
        self.dispatch_on_time_event(event)
            .map_err(|e| anyhow::anyhow!("Python on_time_event failed: {e}"))
    }

    #[allow(unused_variables)]
    fn on_data(&mut self, data: &dyn Any) -> anyhow::Result<()> {
        Python::attach(|py| {
            let py_data = py.None();
            self.dispatch_on_data(py_data)
                .map_err(|e| anyhow::anyhow!("Python on_data failed: {e}"))
        })
    }

    fn on_signal(&mut self, signal: &Signal) -> anyhow::Result<()> {
        self.dispatch_on_signal(signal)
            .map_err(|e| anyhow::anyhow!("Python on_signal failed: {e}"))
    }

    fn on_instrument(&mut self, instrument: &InstrumentAny) -> anyhow::Result<()> {
        Python::attach(|py| {
            let py_instrument = instrument_any_to_pyobject(py, instrument.clone())
                .map_err(|e| anyhow::anyhow!("Failed to convert InstrumentAny to Python: {e}"))?;
            self.dispatch_on_instrument(py_instrument)
                .map_err(|e| anyhow::anyhow!("Python on_instrument failed: {e}"))
        })
    }

    fn on_quote(&mut self, quote: &QuoteTick) -> anyhow::Result<()> {
        self.dispatch_on_quote(*quote)
            .map_err(|e| anyhow::anyhow!("Python on_quote failed: {e}"))
    }

    fn on_trade(&mut self, tick: &TradeTick) -> anyhow::Result<()> {
        self.dispatch_on_trade(*tick)
            .map_err(|e| anyhow::anyhow!("Python on_trade failed: {e}"))
    }

    fn on_bar(&mut self, bar: &Bar) -> anyhow::Result<()> {
        self.dispatch_on_bar(*bar)
            .map_err(|e| anyhow::anyhow!("Python on_bar failed: {e}"))
    }

    fn on_book_deltas(&mut self, deltas: &OrderBookDeltas) -> anyhow::Result<()> {
        self.dispatch_on_book_deltas(deltas.clone())
            .map_err(|e| anyhow::anyhow!("Python on_book_deltas failed: {e}"))
    }

    fn on_book(&mut self, order_book: &OrderBook) -> anyhow::Result<()> {
        self.dispatch_on_book(order_book)
            .map_err(|e| anyhow::anyhow!("Python on_book failed: {e}"))
    }

    fn on_mark_price(&mut self, mark_price: &MarkPriceUpdate) -> anyhow::Result<()> {
        self.dispatch_on_mark_price(*mark_price)
            .map_err(|e| anyhow::anyhow!("Python on_mark_price failed: {e}"))
    }

    fn on_index_price(&mut self, index_price: &IndexPriceUpdate) -> anyhow::Result<()> {
        self.dispatch_on_index_price(*index_price)
            .map_err(|e| anyhow::anyhow!("Python on_index_price failed: {e}"))
    }

    fn on_funding_rate(&mut self, funding_rate: &FundingRateUpdate) -> anyhow::Result<()> {
        self.dispatch_on_funding_rate(*funding_rate)
            .map_err(|e| anyhow::anyhow!("Python on_funding_rate failed: {e}"))
    }

    fn on_instrument_status(&mut self, data: &InstrumentStatus) -> anyhow::Result<()> {
        self.dispatch_on_instrument_status(*data)
            .map_err(|e| anyhow::anyhow!("Python on_instrument_status failed: {e}"))
    }

    fn on_instrument_close(&mut self, update: &InstrumentClose) -> anyhow::Result<()> {
        self.dispatch_on_instrument_close(*update)
            .map_err(|e| anyhow::anyhow!("Python on_instrument_close failed: {e}"))
    }
}

/// Python-facing wrapper for Strategy.
#[allow(non_camel_case_types)]
#[pyo3::pyclass(
    module = "nautilus_trader.trading",
    name = "Strategy",
    unsendable,
    subclass
)]
pub struct PyStrategy {
    inner: Rc<UnsafeCell<PyStrategyInner>>,
}

impl Debug for PyStrategy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PyStrategy")
            .field("inner", &self.inner())
            .finish()
    }
}

impl PyStrategy {
    #[inline]
    #[allow(unsafe_code)]
    pub(crate) fn inner(&self) -> &PyStrategyInner {
        unsafe { &*self.inner.get() }
    }

    #[inline]
    #[allow(unsafe_code, clippy::mut_from_ref)]
    pub(crate) fn inner_mut(&self) -> &mut PyStrategyInner {
        unsafe { &mut *self.inner.get() }
    }
}

impl PyStrategy {
    /// Creates a new PyStrategy instance.
    pub fn new(config: Option<StrategyConfig>) -> Self {
        let config = config.unwrap_or_default();
        let core = StrategyCore::new(config);
        let clock = PyClock::new_test();
        let logger = PyLogger::new(core.actor.actor_id.as_str());

        let inner = PyStrategyInner {
            core,
            py_self: None,
            clock,
            logger,
        };

        Self {
            inner: Rc::new(UnsafeCell::new(inner)),
        }
    }

    /// Sets the Python instance reference for method dispatch.
    pub fn set_python_instance(&mut self, py_obj: Py<PyAny>) {
        self.inner_mut().py_self = Some(py_obj);
    }

    /// Returns a value indicating whether the strategy has been registered with a trader.
    pub fn is_registered(&self) -> bool {
        self.inner().core.actor.is_registered()
    }

    /// Register the strategy with a trader.
    ///
    /// # Errors
    ///
    /// Returns an error if registration fails.
    pub fn register(
        &mut self,
        trader_id: TraderId,
        clock: Rc<RefCell<dyn Clock>>,
        cache: Rc<RefCell<Cache>>,
        portfolio: Rc<RefCell<Portfolio>>,
    ) -> anyhow::Result<()> {
        let inner = self.inner_mut();
        inner.core.register(trader_id, clock, cache, portfolio)?;

        inner.clock = PyClock::from_rc(inner.core.actor.clock_rc());

        let actor_id = inner.core.actor.actor_id.inner();
        let callback = TimeEventCallback::from(move |event: TimeEvent| {
            if let Some(mut strategy) = try_get_actor_unchecked::<PyStrategyInner>(&actor_id) {
                if let Err(e) = DataActor::on_time_event(&mut *strategy, &event) {
                    log::error!("Python time event handler failed for strategy {actor_id}: {e}");
                }
            } else {
                log::error!("Strategy {actor_id} not found for time event handling");
            }
        });

        inner.clock.inner_mut().register_default_handler(callback);

        Component::initialize(inner)
    }

    /// Registers this strategy in the global component and actor registries.
    pub fn register_in_global_registries(&self) {
        let inner = self.inner();
        let component_id = Component::component_id(inner).inner();
        let actor_id = Actor::id(inner);

        let inner_ref: Rc<UnsafeCell<PyStrategyInner>> = self.inner.clone();

        let component_trait_ref: Rc<UnsafeCell<dyn Component>> = inner_ref.clone();
        get_component_registry().insert(component_id, component_trait_ref);

        let actor_trait_ref: Rc<UnsafeCell<dyn Actor>> = inner_ref;
        get_actor_registry().insert(actor_id, actor_trait_ref);
    }
}

#[pyo3::pymethods]
impl PyStrategy {
    #[new]
    #[pyo3(signature = (config=None))]
    fn py_new(config: Option<StrategyConfig>) -> Self {
        Self::new(config)
    }

    /// Captures the Python self reference for Rust→Python event dispatch.
    #[pyo3(signature = (config=None))]
    #[allow(unused_variables)]
    fn __init__(slf: &Bound<'_, Self>, config: Option<StrategyConfig>) {
        let py_self: Py<PyAny> = slf.clone().unbind().into_any();
        slf.borrow_mut().set_python_instance(py_self);
    }

    #[getter]
    #[pyo3(name = "trader_id")]
    fn py_trader_id(&self) -> Option<TraderId> {
        self.inner().core.trader_id()
    }

    #[getter]
    #[pyo3(name = "strategy_id")]
    fn py_strategy_id(&self) -> StrategyId {
        StrategyId::from(self.inner().core.actor.actor_id.inner().as_str())
    }

    #[getter]
    #[pyo3(name = "clock")]
    fn py_clock(&self) -> PyResult<PyClock> {
        let inner = self.inner();
        if !inner.core.actor.is_registered() {
            Err(PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(
                "Strategy must be registered with a trader before accessing clock",
            ))
        } else {
            Ok(inner.clock.clone())
        }
    }

    #[getter]
    #[pyo3(name = "cache")]
    fn py_cache(&self) -> PyResult<PyCache> {
        let inner = self.inner();
        if !inner.core.actor.is_registered() {
            Err(PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(
                "Strategy must be registered with a trader before accessing cache",
            ))
        } else {
            Ok(PyCache::from_rc(inner.core.actor.cache_rc()))
        }
    }

    #[getter]
    #[pyo3(name = "log")]
    fn py_log(&self) -> PyLogger {
        self.inner().logger.clone()
    }

    #[pyo3(name = "state")]
    fn py_state(&self) -> ComponentState {
        self.inner().core.actor.state()
    }

    #[pyo3(name = "is_ready")]
    fn py_is_ready(&self) -> bool {
        Component::is_ready(self.inner())
    }

    #[pyo3(name = "is_running")]
    fn py_is_running(&self) -> bool {
        Component::is_running(self.inner())
    }

    #[pyo3(name = "is_stopped")]
    fn py_is_stopped(&self) -> bool {
        Component::is_stopped(self.inner())
    }

    #[pyo3(name = "is_disposed")]
    fn py_is_disposed(&self) -> bool {
        Component::is_disposed(self.inner())
    }

    #[pyo3(name = "is_degraded")]
    fn py_is_degraded(&self) -> bool {
        Component::is_degraded(self.inner())
    }

    #[pyo3(name = "is_faulted")]
    fn py_is_faulted(&self) -> bool {
        Component::is_faulted(self.inner())
    }

    #[pyo3(name = "start")]
    fn py_start(&mut self) -> PyResult<()> {
        Component::start(self.inner_mut()).map_err(to_pyruntime_err)
    }

    #[pyo3(name = "stop")]
    fn py_stop(&mut self) -> PyResult<()> {
        Component::stop(self.inner_mut()).map_err(to_pyruntime_err)
    }

    #[pyo3(name = "resume")]
    fn py_resume(&mut self) -> PyResult<()> {
        Component::resume(self.inner_mut()).map_err(to_pyruntime_err)
    }

    #[pyo3(name = "reset")]
    fn py_reset(&mut self) -> PyResult<()> {
        Component::reset(self.inner_mut()).map_err(to_pyruntime_err)
    }

    #[pyo3(name = "dispose")]
    fn py_dispose(&mut self) -> PyResult<()> {
        Component::dispose(self.inner_mut()).map_err(to_pyruntime_err)
    }

    #[pyo3(name = "degrade")]
    fn py_degrade(&mut self) -> PyResult<()> {
        Component::degrade(self.inner_mut()).map_err(to_pyruntime_err)
    }

    #[pyo3(name = "fault")]
    fn py_fault(&mut self) -> PyResult<()> {
        Component::fault(self.inner_mut()).map_err(to_pyruntime_err)
    }

    #[pyo3(name = "submit_order")]
    #[pyo3(signature = (order, position_id=None, client_id=None, params=None))]
    fn py_submit_order(
        &mut self,
        py: Python<'_>,
        order: Py<PyAny>,
        position_id: Option<PositionId>,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) -> PyResult<()> {
        let order = pyobject_to_order_any(py, order)?;
        let inner = self.inner_mut();
        match params {
            Some(p) => Strategy::submit_order_with_params(inner, order, position_id, client_id, p),
            None => Strategy::submit_order(inner, order, position_id, client_id),
        }
        .map_err(to_pyruntime_err)
    }

    #[pyo3(name = "modify_order")]
    #[pyo3(signature = (order, quantity=None, price=None, trigger_price=None, client_id=None, params=None))]
    #[allow(clippy::too_many_arguments)]
    fn py_modify_order(
        &mut self,
        py: Python<'_>,
        order: Py<PyAny>,
        quantity: Option<Quantity>,
        price: Option<Price>,
        trigger_price: Option<Price>,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) -> PyResult<()> {
        let order = pyobject_to_order_any(py, order)?;
        let inner = self.inner_mut();
        match params {
            Some(p) => Strategy::modify_order_with_params(
                inner,
                order,
                quantity,
                price,
                trigger_price,
                client_id,
                p,
            ),
            None => Strategy::modify_order(inner, order, quantity, price, trigger_price, client_id),
        }
        .map_err(to_pyruntime_err)
    }

    #[pyo3(name = "cancel_order")]
    #[pyo3(signature = (order, client_id=None, params=None))]
    fn py_cancel_order(
        &mut self,
        py: Python<'_>,
        order: Py<PyAny>,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) -> PyResult<()> {
        let order = pyobject_to_order_any(py, order)?;
        let inner = self.inner_mut();
        match params {
            Some(p) => Strategy::cancel_order_with_params(inner, order, client_id, p),
            None => Strategy::cancel_order(inner, order, client_id),
        }
        .map_err(to_pyruntime_err)
    }

    #[pyo3(name = "cancel_orders")]
    #[pyo3(signature = (orders, client_id=None, params=None))]
    fn py_cancel_orders(
        &mut self,
        py: Python<'_>,
        orders: Vec<Py<PyAny>>,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) -> PyResult<()> {
        let orders: Vec<OrderAny> = orders
            .into_iter()
            .map(|o| pyobject_to_order_any(py, o))
            .collect::<PyResult<Vec<_>>>()?;
        Strategy::cancel_orders(self.inner_mut(), orders, client_id, params)
            .map_err(to_pyruntime_err)
    }

    #[pyo3(name = "cancel_all_orders")]
    #[pyo3(signature = (instrument_id, order_side=None, client_id=None, params=None))]
    fn py_cancel_all_orders(
        &mut self,
        instrument_id: InstrumentId,
        order_side: Option<OrderSide>,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) -> PyResult<()> {
        let inner = self.inner_mut();
        match params {
            Some(p) => Strategy::cancel_all_orders_with_params(
                inner,
                instrument_id,
                order_side,
                client_id,
                p,
            ),
            None => Strategy::cancel_all_orders(inner, instrument_id, order_side, client_id),
        }
        .map_err(to_pyruntime_err)
    }

    #[pyo3(name = "close_position")]
    #[pyo3(signature = (position, client_id=None, tags=None, time_in_force=None, reduce_only=None, quote_quantity=None))]
    fn py_close_position(
        &mut self,
        position: &Position,
        client_id: Option<ClientId>,
        tags: Option<Vec<String>>,
        time_in_force: Option<TimeInForce>,
        reduce_only: Option<bool>,
        quote_quantity: Option<bool>,
    ) -> PyResult<()> {
        let tags = tags.map(|t| t.into_iter().map(|s| Ustr::from(&s)).collect());
        Strategy::close_position(
            self.inner_mut(),
            position,
            client_id,
            tags,
            time_in_force,
            reduce_only,
            quote_quantity,
        )
        .map_err(to_pyruntime_err)
    }

    #[pyo3(name = "close_all_positions")]
    #[pyo3(signature = (instrument_id, position_side=None, client_id=None, tags=None, time_in_force=None, reduce_only=None, quote_quantity=None))]
    #[allow(clippy::too_many_arguments)]
    fn py_close_all_positions(
        &mut self,
        instrument_id: InstrumentId,
        position_side: Option<PositionSide>,
        client_id: Option<ClientId>,
        tags: Option<Vec<String>>,
        time_in_force: Option<TimeInForce>,
        reduce_only: Option<bool>,
        quote_quantity: Option<bool>,
    ) -> PyResult<()> {
        let tags = tags.map(|t| t.into_iter().map(|s| Ustr::from(&s)).collect());
        Strategy::close_all_positions(
            self.inner_mut(),
            instrument_id,
            position_side,
            client_id,
            tags,
            time_in_force,
            reduce_only,
            quote_quantity,
        )
        .map_err(to_pyruntime_err)
    }

    #[pyo3(name = "query_account")]
    #[pyo3(signature = (account_id, client_id=None))]
    fn py_query_account(
        &mut self,
        account_id: AccountId,
        client_id: Option<ClientId>,
    ) -> PyResult<()> {
        Strategy::query_account(self.inner_mut(), account_id, client_id).map_err(to_pyruntime_err)
    }

    #[pyo3(name = "query_order")]
    #[pyo3(signature = (order, client_id=None))]
    fn py_query_order(
        &mut self,
        py: Python<'_>,
        order: Py<PyAny>,
        client_id: Option<ClientId>,
    ) -> PyResult<()> {
        let order = pyobject_to_order_any(py, order)?;
        Strategy::query_order(self.inner_mut(), &order, client_id).map_err(to_pyruntime_err)
    }

    #[pyo3(name = "on_start")]
    fn py_on_start(&mut self) -> PyResult<()> {
        self.inner_mut().dispatch_on_start()
    }

    #[pyo3(name = "on_stop")]
    fn py_on_stop(&mut self) -> PyResult<()> {
        self.inner_mut().dispatch_on_stop()
    }

    #[pyo3(name = "on_resume")]
    fn py_on_resume(&mut self) -> PyResult<()> {
        self.inner_mut().dispatch_on_resume()
    }

    #[pyo3(name = "on_reset")]
    fn py_on_reset(&mut self) -> PyResult<()> {
        self.inner_mut().dispatch_on_reset()
    }

    #[pyo3(name = "on_dispose")]
    fn py_on_dispose(&mut self) -> PyResult<()> {
        self.inner_mut().dispatch_on_dispose()
    }

    #[pyo3(name = "on_degrade")]
    fn py_on_degrade(&mut self) -> PyResult<()> {
        self.inner_mut().dispatch_on_degrade()
    }

    #[pyo3(name = "on_fault")]
    fn py_on_fault(&mut self) -> PyResult<()> {
        self.inner_mut().dispatch_on_fault()
    }

    #[pyo3(name = "on_data")]
    fn py_on_data(&mut self, data: Py<PyAny>) -> PyResult<()> {
        self.inner_mut().dispatch_on_data(data)
    }

    #[pyo3(name = "on_signal")]
    fn py_on_signal(&mut self, signal: &Signal) -> PyResult<()> {
        self.inner_mut().dispatch_on_signal(signal)
    }

    #[pyo3(name = "on_instrument")]
    fn py_on_instrument(&mut self, instrument: Py<PyAny>) -> PyResult<()> {
        self.inner_mut().dispatch_on_instrument(instrument)
    }

    #[pyo3(name = "on_quote")]
    fn py_on_quote(&mut self, quote: QuoteTick) -> PyResult<()> {
        self.inner_mut().dispatch_on_quote(quote)
    }

    #[pyo3(name = "on_trade")]
    fn py_on_trade(&mut self, trade: TradeTick) -> PyResult<()> {
        self.inner_mut().dispatch_on_trade(trade)
    }

    #[pyo3(name = "on_bar")]
    fn py_on_bar(&mut self, bar: Bar) -> PyResult<()> {
        self.inner_mut().dispatch_on_bar(bar)
    }

    #[pyo3(name = "on_book_deltas")]
    fn py_on_book_deltas(&mut self, deltas: OrderBookDeltas) -> PyResult<()> {
        self.inner_mut().dispatch_on_book_deltas(deltas)
    }

    #[pyo3(name = "on_book")]
    fn py_on_book(&mut self, book: &OrderBook) -> PyResult<()> {
        self.inner_mut().dispatch_on_book(book)
    }

    #[pyo3(name = "on_mark_price")]
    fn py_on_mark_price(&mut self, mark_price: MarkPriceUpdate) -> PyResult<()> {
        self.inner_mut().dispatch_on_mark_price(mark_price)
    }

    #[pyo3(name = "on_index_price")]
    fn py_on_index_price(&mut self, index_price: IndexPriceUpdate) -> PyResult<()> {
        self.inner_mut().dispatch_on_index_price(index_price)
    }

    #[pyo3(name = "on_funding_rate")]
    fn py_on_funding_rate(&mut self, funding_rate: FundingRateUpdate) -> PyResult<()> {
        self.inner_mut().dispatch_on_funding_rate(funding_rate)
    }

    #[pyo3(name = "on_instrument_status")]
    fn py_on_instrument_status(&mut self, status: InstrumentStatus) -> PyResult<()> {
        self.inner_mut().dispatch_on_instrument_status(status)
    }

    #[pyo3(name = "on_instrument_close")]
    fn py_on_instrument_close(&mut self, close: InstrumentClose) -> PyResult<()> {
        self.inner_mut().dispatch_on_instrument_close(close)
    }

    #[pyo3(name = "subscribe_data")]
    #[pyo3(signature = (data_type, client_id=None, params=None))]
    fn py_subscribe_data(
        &mut self,
        data_type: DataType,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) -> PyResult<()> {
        DataActor::subscribe_data(self.inner_mut(), data_type, client_id, params);
        Ok(())
    }

    #[pyo3(name = "subscribe_instruments")]
    #[pyo3(signature = (venue, client_id=None, params=None))]
    fn py_subscribe_instruments(
        &mut self,
        venue: Venue,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) -> PyResult<()> {
        DataActor::subscribe_instruments(self.inner_mut(), venue, client_id, params);
        Ok(())
    }

    #[pyo3(name = "subscribe_instrument")]
    #[pyo3(signature = (instrument_id, client_id=None, params=None))]
    fn py_subscribe_instrument(
        &mut self,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) -> PyResult<()> {
        DataActor::subscribe_instrument(self.inner_mut(), instrument_id, client_id, params);
        Ok(())
    }

    #[pyo3(name = "subscribe_book_deltas")]
    #[pyo3(signature = (instrument_id, book_type, depth=None, client_id=None, managed=false, params=None))]
    fn py_subscribe_book_deltas(
        &mut self,
        instrument_id: InstrumentId,
        book_type: BookType,
        depth: Option<usize>,
        client_id: Option<ClientId>,
        managed: bool,
        params: Option<IndexMap<String, String>>,
    ) -> PyResult<()> {
        let depth = depth.and_then(NonZeroUsize::new);
        DataActor::subscribe_book_deltas(
            self.inner_mut(),
            instrument_id,
            book_type,
            depth,
            client_id,
            managed,
            params,
        );
        Ok(())
    }

    #[pyo3(name = "subscribe_book_at_interval")]
    #[pyo3(signature = (instrument_id, book_type, interval_ms, depth=None, client_id=None, params=None))]
    fn py_subscribe_book_at_interval(
        &mut self,
        instrument_id: InstrumentId,
        book_type: BookType,
        interval_ms: usize,
        depth: Option<usize>,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) -> PyResult<()> {
        let depth = depth.and_then(NonZeroUsize::new);
        let interval_ms = NonZeroUsize::new(interval_ms)
            .ok_or_else(|| PyErr::new::<PyValueError, _>("interval_ms must be > 0"))?;

        DataActor::subscribe_book_at_interval(
            self.inner_mut(),
            instrument_id,
            book_type,
            depth,
            interval_ms,
            client_id,
            params,
        );
        Ok(())
    }

    #[pyo3(name = "subscribe_quotes")]
    #[pyo3(signature = (instrument_id, client_id=None, params=None))]
    fn py_subscribe_quotes(
        &mut self,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) -> PyResult<()> {
        DataActor::subscribe_quotes(self.inner_mut(), instrument_id, client_id, params);
        Ok(())
    }

    #[pyo3(name = "subscribe_trades")]
    #[pyo3(signature = (instrument_id, client_id=None, params=None))]
    fn py_subscribe_trades(
        &mut self,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) -> PyResult<()> {
        DataActor::subscribe_trades(self.inner_mut(), instrument_id, client_id, params);
        Ok(())
    }

    #[pyo3(name = "subscribe_bars")]
    #[pyo3(signature = (bar_type, client_id=None, params=None))]
    fn py_subscribe_bars(
        &mut self,
        bar_type: BarType,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) -> PyResult<()> {
        DataActor::subscribe_bars(self.inner_mut(), bar_type, client_id, params);
        Ok(())
    }

    #[pyo3(name = "subscribe_mark_prices")]
    #[pyo3(signature = (instrument_id, client_id=None, params=None))]
    fn py_subscribe_mark_prices(
        &mut self,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) -> PyResult<()> {
        DataActor::subscribe_mark_prices(self.inner_mut(), instrument_id, client_id, params);
        Ok(())
    }

    #[pyo3(name = "subscribe_index_prices")]
    #[pyo3(signature = (instrument_id, client_id=None, params=None))]
    fn py_subscribe_index_prices(
        &mut self,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) -> PyResult<()> {
        DataActor::subscribe_index_prices(self.inner_mut(), instrument_id, client_id, params);
        Ok(())
    }

    #[pyo3(name = "subscribe_instrument_status")]
    #[pyo3(signature = (instrument_id, client_id=None, params=None))]
    fn py_subscribe_instrument_status(
        &mut self,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) -> PyResult<()> {
        DataActor::subscribe_instrument_status(self.inner_mut(), instrument_id, client_id, params);
        Ok(())
    }

    #[pyo3(name = "subscribe_instrument_close")]
    #[pyo3(signature = (instrument_id, client_id=None, params=None))]
    fn py_subscribe_instrument_close(
        &mut self,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) -> PyResult<()> {
        DataActor::subscribe_instrument_close(self.inner_mut(), instrument_id, client_id, params);
        Ok(())
    }

    #[pyo3(name = "request_data")]
    #[pyo3(signature = (data_type, client_id, start=None, end=None, limit=None, params=None))]
    fn py_request_data(
        &mut self,
        data_type: DataType,
        client_id: ClientId,
        start: Option<u64>,
        end: Option<u64>,
        limit: Option<usize>,
        params: Option<IndexMap<String, String>>,
    ) -> PyResult<String> {
        let limit = limit.and_then(NonZeroUsize::new);
        let start = start.map(|ts| UnixNanos::from(ts).to_datetime_utc());
        let end = end.map(|ts| UnixNanos::from(ts).to_datetime_utc());

        let request_id = DataActor::request_data(
            self.inner_mut(),
            data_type,
            client_id,
            start,
            end,
            limit,
            params,
        )
        .map_err(to_pyvalue_err)?;
        Ok(request_id.to_string())
    }

    #[pyo3(name = "request_instrument")]
    #[pyo3(signature = (instrument_id, start=None, end=None, client_id=None, params=None))]
    fn py_request_instrument(
        &mut self,
        instrument_id: InstrumentId,
        start: Option<u64>,
        end: Option<u64>,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) -> PyResult<String> {
        let start = start.map(|ts| UnixNanos::from(ts).to_datetime_utc());
        let end = end.map(|ts| UnixNanos::from(ts).to_datetime_utc());

        let request_id = DataActor::request_instrument(
            self.inner_mut(),
            instrument_id,
            start,
            end,
            client_id,
            params,
        )
        .map_err(to_pyvalue_err)?;
        Ok(request_id.to_string())
    }

    #[pyo3(name = "request_instruments")]
    #[pyo3(signature = (venue=None, start=None, end=None, client_id=None, params=None))]
    fn py_request_instruments(
        &mut self,
        venue: Option<Venue>,
        start: Option<u64>,
        end: Option<u64>,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) -> PyResult<String> {
        let start = start.map(|ts| UnixNanos::from(ts).to_datetime_utc());
        let end = end.map(|ts| UnixNanos::from(ts).to_datetime_utc());

        let request_id =
            DataActor::request_instruments(self.inner_mut(), venue, start, end, client_id, params)
                .map_err(to_pyvalue_err)?;
        Ok(request_id.to_string())
    }

    #[pyo3(name = "request_book_snapshot")]
    #[pyo3(signature = (instrument_id, depth=None, client_id=None, params=None))]
    fn py_request_book_snapshot(
        &mut self,
        instrument_id: InstrumentId,
        depth: Option<usize>,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) -> PyResult<String> {
        let depth = depth.and_then(NonZeroUsize::new);

        let request_id = DataActor::request_book_snapshot(
            self.inner_mut(),
            instrument_id,
            depth,
            client_id,
            params,
        )
        .map_err(to_pyvalue_err)?;
        Ok(request_id.to_string())
    }

    #[pyo3(name = "request_quotes")]
    #[pyo3(signature = (instrument_id, start=None, end=None, limit=None, client_id=None, params=None))]
    fn py_request_quotes(
        &mut self,
        instrument_id: InstrumentId,
        start: Option<u64>,
        end: Option<u64>,
        limit: Option<usize>,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) -> PyResult<String> {
        let limit = limit.and_then(NonZeroUsize::new);
        let start = start.map(|ts| UnixNanos::from(ts).to_datetime_utc());
        let end = end.map(|ts| UnixNanos::from(ts).to_datetime_utc());

        let request_id = DataActor::request_quotes(
            self.inner_mut(),
            instrument_id,
            start,
            end,
            limit,
            client_id,
            params,
        )
        .map_err(to_pyvalue_err)?;
        Ok(request_id.to_string())
    }

    #[pyo3(name = "request_trades")]
    #[pyo3(signature = (instrument_id, start=None, end=None, limit=None, client_id=None, params=None))]
    fn py_request_trades(
        &mut self,
        instrument_id: InstrumentId,
        start: Option<u64>,
        end: Option<u64>,
        limit: Option<usize>,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) -> PyResult<String> {
        let limit = limit.and_then(NonZeroUsize::new);
        let start = start.map(|ts| UnixNanos::from(ts).to_datetime_utc());
        let end = end.map(|ts| UnixNanos::from(ts).to_datetime_utc());

        let request_id = DataActor::request_trades(
            self.inner_mut(),
            instrument_id,
            start,
            end,
            limit,
            client_id,
            params,
        )
        .map_err(to_pyvalue_err)?;
        Ok(request_id.to_string())
    }

    #[pyo3(name = "request_bars")]
    #[pyo3(signature = (bar_type, start=None, end=None, limit=None, client_id=None, params=None))]
    fn py_request_bars(
        &mut self,
        bar_type: BarType,
        start: Option<u64>,
        end: Option<u64>,
        limit: Option<usize>,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) -> PyResult<String> {
        let limit = limit.and_then(NonZeroUsize::new);
        let start = start.map(|ts| UnixNanos::from(ts).to_datetime_utc());
        let end = end.map(|ts| UnixNanos::from(ts).to_datetime_utc());

        let request_id = DataActor::request_bars(
            self.inner_mut(),
            bar_type,
            start,
            end,
            limit,
            client_id,
            params,
        )
        .map_err(to_pyvalue_err)?;
        Ok(request_id.to_string())
    }

    #[pyo3(name = "unsubscribe_data")]
    #[pyo3(signature = (data_type, client_id=None, params=None))]
    fn py_unsubscribe_data(
        &mut self,
        data_type: DataType,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) -> PyResult<()> {
        DataActor::unsubscribe_data(self.inner_mut(), data_type, client_id, params);
        Ok(())
    }

    #[pyo3(name = "unsubscribe_instruments")]
    #[pyo3(signature = (venue, client_id=None, params=None))]
    fn py_unsubscribe_instruments(
        &mut self,
        venue: Venue,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) -> PyResult<()> {
        DataActor::unsubscribe_instruments(self.inner_mut(), venue, client_id, params);
        Ok(())
    }

    #[pyo3(name = "unsubscribe_instrument")]
    #[pyo3(signature = (instrument_id, client_id=None, params=None))]
    fn py_unsubscribe_instrument(
        &mut self,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) -> PyResult<()> {
        DataActor::unsubscribe_instrument(self.inner_mut(), instrument_id, client_id, params);
        Ok(())
    }

    #[pyo3(name = "unsubscribe_book_deltas")]
    #[pyo3(signature = (instrument_id, client_id=None, params=None))]
    fn py_unsubscribe_book_deltas(
        &mut self,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) -> PyResult<()> {
        DataActor::unsubscribe_book_deltas(self.inner_mut(), instrument_id, client_id, params);
        Ok(())
    }

    #[pyo3(name = "unsubscribe_book_at_interval")]
    #[pyo3(signature = (instrument_id, interval_ms, client_id=None, params=None))]
    fn py_unsubscribe_book_at_interval(
        &mut self,
        instrument_id: InstrumentId,
        interval_ms: usize,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) -> PyResult<()> {
        let interval_ms = NonZeroUsize::new(interval_ms)
            .ok_or_else(|| PyErr::new::<PyValueError, _>("interval_ms must be > 0"))?;

        DataActor::unsubscribe_book_at_interval(
            self.inner_mut(),
            instrument_id,
            interval_ms,
            client_id,
            params,
        );
        Ok(())
    }

    #[pyo3(name = "unsubscribe_quotes")]
    #[pyo3(signature = (instrument_id, client_id=None, params=None))]
    fn py_unsubscribe_quotes(
        &mut self,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) -> PyResult<()> {
        DataActor::unsubscribe_quotes(self.inner_mut(), instrument_id, client_id, params);
        Ok(())
    }

    #[pyo3(name = "unsubscribe_trades")]
    #[pyo3(signature = (instrument_id, client_id=None, params=None))]
    fn py_unsubscribe_trades(
        &mut self,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) -> PyResult<()> {
        DataActor::unsubscribe_trades(self.inner_mut(), instrument_id, client_id, params);
        Ok(())
    }

    #[pyo3(name = "unsubscribe_bars")]
    #[pyo3(signature = (bar_type, client_id=None, params=None))]
    fn py_unsubscribe_bars(
        &mut self,
        bar_type: BarType,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) -> PyResult<()> {
        DataActor::unsubscribe_bars(self.inner_mut(), bar_type, client_id, params);
        Ok(())
    }

    #[pyo3(name = "unsubscribe_mark_prices")]
    #[pyo3(signature = (instrument_id, client_id=None, params=None))]
    fn py_unsubscribe_mark_prices(
        &mut self,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) -> PyResult<()> {
        DataActor::unsubscribe_mark_prices(self.inner_mut(), instrument_id, client_id, params);
        Ok(())
    }

    #[pyo3(name = "unsubscribe_index_prices")]
    #[pyo3(signature = (instrument_id, client_id=None, params=None))]
    fn py_unsubscribe_index_prices(
        &mut self,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) -> PyResult<()> {
        DataActor::unsubscribe_index_prices(self.inner_mut(), instrument_id, client_id, params);
        Ok(())
    }

    #[pyo3(name = "unsubscribe_instrument_status")]
    #[pyo3(signature = (instrument_id, client_id=None, params=None))]
    fn py_unsubscribe_instrument_status(
        &mut self,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) -> PyResult<()> {
        DataActor::unsubscribe_instrument_status(
            self.inner_mut(),
            instrument_id,
            client_id,
            params,
        );
        Ok(())
    }

    #[pyo3(name = "unsubscribe_instrument_close")]
    #[pyo3(signature = (instrument_id, client_id=None, params=None))]
    fn py_unsubscribe_instrument_close(
        &mut self,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) -> PyResult<()> {
        DataActor::unsubscribe_instrument_close(self.inner_mut(), instrument_id, client_id, params);
        Ok(())
    }
}
