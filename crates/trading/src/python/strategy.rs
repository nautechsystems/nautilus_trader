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

//! Python bindings for Strategy with complete order and position management.

use std::{
    any::Any,
    cell::{RefCell, UnsafeCell},
    collections::HashMap,
    fmt::Debug,
    num::NonZeroUsize,
    ops::{Deref, DerefMut},
    rc::Rc,
};

use chrono::{DateTime, Utc};
use indexmap::IndexMap;
use nautilus_common::{
    actor::{
        Actor, DataActor,
        data_actor::DataActorCore,
        registry::{try_get_actor_unchecked, with_actor_registry},
    },
    cache::Cache,
    clock::Clock,
    component::{Component, with_component_registry},
    enums::ComponentState,
    python::{
        cache::PyCache,
        clock::PyClock,
        indicators::{registered_python_indicators, wrap_python_indicator},
        logging::PyLogger,
        order_factory::PyOrderFactory,
    },
    signal::Signal,
    timer::{TimeEvent, TimeEventCallback},
};
use nautilus_core::{
    Params, from_pydict,
    python::{IntoPyObjectNautilusExt, to_pyruntime_err, to_pyvalue_err},
};
use nautilus_model::{
    data::{
        Bar, BarType, CustomData, DataType, FundingRateUpdate, IndexPriceUpdate, InstrumentStatus,
        MarkPriceUpdate, OrderBookDeltas, QuoteTick, TradeTick,
        close::InstrumentClose,
        option_chain::{OptionChainSlice, OptionGreeks},
    },
    enums::{BookType, OmsType, OrderSide, PositionSide, TimeInForce},
    events::{
        OrderAccepted, OrderCancelRejected, OrderCanceled, OrderDenied, OrderEmulated,
        OrderEventAny, OrderExpired, OrderFilled, OrderInitialized, OrderModifyRejected,
        OrderPendingCancel, OrderPendingUpdate, OrderRejected, OrderReleased, OrderSubmitted,
        OrderTriggered, OrderUpdated, PositionChanged, PositionClosed, PositionEvent,
        PositionOpened,
    },
    identifiers::{
        AccountId, ClientId, ClientOrderId, InstrumentId, OptionSeriesId, PositionId, StrategyId,
        TraderId, Venue,
    },
    instruments::InstrumentAny,
    orderbook::OrderBook,
    orders::{Order, OrderAny},
    position::Position,
    python::{
        data::option_chain::PyStrikeRange, events::order::order_event_to_pyobject,
        instruments::instrument_any_to_pyobject, orders::pyobject_to_order_any,
    },
    types::{Price, Quantity},
};
use nautilus_portfolio::{portfolio::Portfolio, python::PyPortfolio};
use pyo3::{
    prelude::*,
    types::{PyBytes, PyDict, PyList},
};
use ustr::Ustr;

use crate::strategy::{
    BatchModifyOrder, ImportableStrategyConfig, Strategy, StrategyConfig, StrategyCore,
};

#[pyo3::pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl StrategyConfig {
    /// The base model for all trading strategy configurations.
    #[new]
    #[pyo3(signature = (
        strategy_id=None,
        order_id_tag=None,
        oms_type=None,
        external_order_claims=None,
        manage_contingent_orders=false,
        manage_gtd_expiry=false,
        manage_stop=false,
        market_exit_interval_ms=100,
        market_exit_max_attempts=100,
        market_exit_time_in_force=TimeInForce::Gtc,
        market_exit_reduce_only=true,
        use_uuid_client_order_ids=false,
        use_hyphens_in_client_order_ids=true,
        log_events=true,
        log_commands=true,
        log_rejected_due_post_only_as_warning=true,
        **_kwargs
    ))]
    #[expect(
        clippy::fn_params_excessive_bools,
        clippy::too_many_arguments,
        reason = "constructor mirrors the existing Python keyword API"
    )]
    fn py_new(
        strategy_id: Option<StrategyId>,
        order_id_tag: Option<String>,
        oms_type: Option<OmsType>,
        external_order_claims: Option<Vec<InstrumentId>>,
        manage_contingent_orders: bool,
        manage_gtd_expiry: bool,
        manage_stop: bool,
        market_exit_interval_ms: u64,
        market_exit_max_attempts: u64,
        market_exit_time_in_force: TimeInForce,
        market_exit_reduce_only: bool,
        use_uuid_client_order_ids: bool,
        use_hyphens_in_client_order_ids: bool,
        log_events: bool,
        log_commands: bool,
        log_rejected_due_post_only_as_warning: bool,
        _kwargs: Option<&Bound<'_, PyDict>>,
    ) -> Self {
        Self {
            strategy_id,
            order_id_tag,
            use_uuid_client_order_ids,
            use_hyphens_in_client_order_ids,
            oms_type,
            external_order_claims,
            manage_contingent_orders,
            manage_gtd_expiry,
            manage_stop,
            market_exit_interval_ms,
            market_exit_max_attempts,
            market_exit_time_in_force,
            market_exit_reduce_only,
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

#[pyo3::pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl ImportableStrategyConfig {
    /// Configuration for creating strategies from importable paths.
    #[new]
    #[expect(clippy::needless_pass_by_value)]
    fn py_new(strategy_path: String, config_path: String, config: Py<PyDict>) -> PyResult<Self> {
        let json_config = Python::attach(|py| -> PyResult<HashMap<String, serde_json::Value>> {
            let kwargs = PyDict::new(py);
            kwargs.set_item("default", py.eval(pyo3::ffi::c_str!("str"), None, None)?)?;
            let json_str: String = PyModule::import(py, "json")?
                .call_method("dumps", (config.bind(py),), Some(&kwargs))?
                .extract()?;

            let json_value: serde_json::Value =
                serde_json::from_str(&json_str).map_err(to_pyvalue_err)?;

            if let serde_json::Value::Object(map) = json_value {
                Ok(map.into_iter().collect())
            } else {
                Err(to_pyvalue_err("Config must be a dictionary"))
            }
        })?;

        Ok(Self {
            strategy_path,
            config_path,
            config: json_config,
        })
    }

    #[getter]
    fn strategy_path(&self) -> &String {
        &self.strategy_path
    }

    #[getter]
    fn config_path(&self) -> &String {
        &self.config_path
    }

    #[getter]
    fn config(&self, py: Python<'_>) -> PyResult<Py<PyDict>> {
        let py_dict = PyDict::new(py);

        for (key, value) in &self.config {
            let json_str = serde_json::to_string(value).map_err(to_pyvalue_err)?;
            let py_value = PyModule::import(py, "json")?.call_method("loads", (json_str,), None)?;
            py_dict.set_item(key, py_value)?;
        }
        Ok(py_dict.unbind())
    }
}

/// Inner state of `PyStrategy`, shared between Python wrapper and Rust registries.
pub struct PyStrategyInner {
    core: StrategyCore,
    py_self: Option<Py<PyAny>>,
    clock: PyClock,
    logger: PyLogger,
}

impl Debug for PyStrategyInner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(PyStrategyInner))
            .field("core", &self.core)
            .field("py_self", &self.py_self.as_ref().map(|_| "<Py<PyAny>>"))
            .field("clock", &self.clock)
            .field("logger", &self.logger)
            .finish()
    }
}

#[expect(
    clippy::needless_pass_by_ref_mut,
    reason = "dispatch methods share receiver shape with mutable DataActor hooks"
)]
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

    fn dispatch_on_save(&self) -> PyResult<IndexMap<String, Vec<u8>>> {
        if let Some(ref py_self) = self.py_self {
            Python::attach(|py| {
                let py_state = py_self.call_method0(py, "on_save")?;
                let py_state: &Bound<'_, PyDict> = py_state.cast_bound::<PyDict>(py)?;
                pydict_to_state(py_state)
            })
        } else {
            Ok(IndexMap::new())
        }
    }

    fn dispatch_on_load(&self, state: &IndexMap<String, Vec<u8>>) -> PyResult<()> {
        if let Some(ref py_self) = self.py_self {
            Python::attach(|py| -> PyResult<()> {
                let py_state = state_to_pydict(py, state)?;
                py_self.call_method1(py, "on_load", (py_state,))?;
                Ok(())
            })?;
        }
        Ok(())
    }

    fn dispatch_on_market_exit(&self) -> PyResult<()> {
        if let Some(ref py_self) = self.py_self {
            Python::attach(|py| py_self.call_method0(py, "on_market_exit"))?;
        }
        Ok(())
    }

    fn dispatch_post_market_exit(&self) -> PyResult<()> {
        if let Some(ref py_self) = self.py_self {
            Python::attach(|py| py_self.call_method0(py, "post_market_exit"))?;
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

    fn dispatch_on_order_event(&self, event: OrderEventAny) -> PyResult<()> {
        if let Some(ref py_self) = self.py_self {
            Python::attach(|py| {
                let py_event = order_event_to_pyobject(py, event)?;
                py_self.call_method1(py, "on_order_event", (py_event,))
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

    fn dispatch_on_order_updated(&self, event: &OrderUpdated) -> PyResult<()> {
        if let Some(ref py_self) = self.py_self {
            Python::attach(|py| {
                py_self.call_method1(py, "on_order_updated", ((*event).into_py_any_unwrap(py),))
            })?;
        }
        Ok(())
    }

    fn dispatch_on_order_canceled(&self, event: OrderCanceled) -> PyResult<()> {
        if let Some(ref py_self) = self.py_self {
            Python::attach(|py| {
                py_self.call_method1(py, "on_order_canceled", (event.into_py_any_unwrap(py),))
            })?;
        }
        Ok(())
    }

    fn dispatch_on_order_filled(&self, event: &OrderFilled) -> PyResult<()> {
        if let Some(ref py_self) = self.py_self {
            Python::attach(|py| {
                py_self.call_method1(py, "on_order_filled", ((*event).into_py_any_unwrap(py),))
            })?;
        }
        Ok(())
    }

    fn dispatch_on_position_opened(&self, event: PositionOpened) -> PyResult<()> {
        if let Some(ref py_self) = self.py_self {
            Python::attach(|py| {
                py_self.call_method1(py, "on_position_opened", (event.into_py_any_unwrap(py),))
            })?;
        }
        Ok(())
    }

    fn dispatch_on_position_event(&self, event: PositionEvent) -> PyResult<()> {
        if let Some(ref py_self) = self.py_self {
            Python::attach(|py| {
                let py_event = match event {
                    PositionEvent::PositionOpened(event) => event.into_py_any_unwrap(py),
                    PositionEvent::PositionChanged(event) => event.into_py_any_unwrap(py),
                    PositionEvent::PositionClosed(event) => event.into_py_any_unwrap(py),
                    PositionEvent::PositionAdjusted(event) => event.into_py_any_unwrap(py),
                };
                py_self.call_method1(py, "on_position_event", (py_event,))
            })?;
        }
        Ok(())
    }

    fn dispatch_on_position_changed(&self, event: PositionChanged) -> PyResult<()> {
        if let Some(ref py_self) = self.py_self {
            Python::attach(|py| {
                py_self.call_method1(py, "on_position_changed", (event.into_py_any_unwrap(py),))
            })?;
        }
        Ok(())
    }

    fn dispatch_on_position_closed(&self, event: PositionClosed) -> PyResult<()> {
        if let Some(ref py_self) = self.py_self {
            Python::attach(|py| {
                py_self.call_method1(py, "on_position_closed", (event.into_py_any_unwrap(py),))
            })?;
        }
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

    fn dispatch_on_book_deltas(&mut self, deltas: &OrderBookDeltas) -> PyResult<()> {
        if let Some(ref py_self) = self.py_self {
            Python::attach(|py| {
                py_self.call_method1(
                    py,
                    "on_book_deltas",
                    (deltas.clone().into_py_any_unwrap(py),),
                )
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

    fn dispatch_on_option_greeks(&mut self, greeks: OptionGreeks) -> PyResult<()> {
        if let Some(ref py_self) = self.py_self {
            Python::attach(|py| {
                py_self.call_method1(py, "on_option_greeks", (greeks.into_py_any_unwrap(py),))
            })?;
        }
        Ok(())
    }

    fn dispatch_on_option_chain(&mut self, slice: &OptionChainSlice) -> PyResult<()> {
        if let Some(ref py_self) = self.py_self {
            Python::attach(|py| {
                py_self.call_method1(
                    py,
                    "on_option_chain",
                    (slice.clone().into_py_any_unwrap(py),),
                )
            })?;
        }
        Ok(())
    }

    fn dispatch_on_historical_data(&mut self, data: Py<PyAny>) -> PyResult<()> {
        if let Some(ref py_self) = self.py_self {
            Python::attach(|py| py_self.call_method1(py, "on_historical_data", (data,)))?;
        }
        Ok(())
    }

    fn dispatch_on_historical_quotes(&mut self, quotes: Vec<QuoteTick>) -> PyResult<()> {
        if let Some(ref py_self) = self.py_self {
            Python::attach(|py| {
                let py_quotes: Vec<_> = quotes
                    .into_iter()
                    .map(|quote| quote.into_py_any_unwrap(py))
                    .collect();
                py_self.call_method1(py, "on_historical_quotes", (py_quotes,))
            })?;
        }
        Ok(())
    }

    fn dispatch_on_historical_trades(&mut self, trades: Vec<TradeTick>) -> PyResult<()> {
        if let Some(ref py_self) = self.py_self {
            Python::attach(|py| {
                let py_trades: Vec<_> = trades
                    .into_iter()
                    .map(|trade| trade.into_py_any_unwrap(py))
                    .collect();
                py_self.call_method1(py, "on_historical_trades", (py_trades,))
            })?;
        }
        Ok(())
    }

    fn dispatch_on_historical_funding_rates(
        &mut self,
        funding_rates: Vec<FundingRateUpdate>,
    ) -> PyResult<()> {
        if let Some(ref py_self) = self.py_self {
            Python::attach(|py| {
                let py_funding_rates: Vec<_> = funding_rates
                    .into_iter()
                    .map(|rate| rate.into_py_any_unwrap(py))
                    .collect();
                py_self.call_method1(py, "on_historical_funding_rates", (py_funding_rates,))
            })?;
        }
        Ok(())
    }

    fn dispatch_on_historical_bars(&mut self, bars: Vec<Bar>) -> PyResult<()> {
        if let Some(ref py_self) = self.py_self {
            Python::attach(|py| {
                let py_bars: Vec<_> = bars
                    .into_iter()
                    .map(|bar| bar.into_py_any_unwrap(py))
                    .collect();
                py_self.call_method1(py, "on_historical_bars", (py_bars,))
            })?;
        }
        Ok(())
    }

    fn dispatch_on_historical_mark_prices(
        &mut self,
        mark_prices: Vec<MarkPriceUpdate>,
    ) -> PyResult<()> {
        if let Some(ref py_self) = self.py_self {
            Python::attach(|py| {
                let py_mark_prices: Vec<_> = mark_prices
                    .into_iter()
                    .map(|price| price.into_py_any_unwrap(py))
                    .collect();
                py_self.call_method1(py, "on_historical_mark_prices", (py_mark_prices,))
            })?;
        }
        Ok(())
    }

    fn dispatch_on_historical_index_prices(
        &mut self,
        index_prices: Vec<IndexPriceUpdate>,
    ) -> PyResult<()> {
        if let Some(ref py_self) = self.py_self {
            Python::attach(|py| {
                let py_index_prices: Vec<_> = index_prices
                    .into_iter()
                    .map(|price| price.into_py_any_unwrap(py))
                    .collect();
                py_self.call_method1(py, "on_historical_index_prices", (py_index_prices,))
            })?;
        }
        Ok(())
    }
}

impl Deref for PyStrategyInner {
    type Target = DataActorCore;

    fn deref(&self) -> &Self::Target {
        &self.core
    }
}

impl DerefMut for PyStrategyInner {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.core
    }
}

impl Strategy for PyStrategyInner {
    fn core(&self) -> &StrategyCore {
        &self.core
    }

    fn core_mut(&mut self) -> &mut StrategyCore {
        &mut self.core
    }

    fn external_order_claims(&self) -> Option<Vec<InstrumentId>> {
        self.core.config.external_order_claims.clone()
    }

    fn on_market_exit(&mut self) {
        let _ = self.dispatch_on_market_exit();
    }

    fn post_market_exit(&mut self) {
        let _ = self.dispatch_post_market_exit();
    }

    fn on_order_initialized(&mut self, event: OrderInitialized) {
        let _ = self.dispatch_on_order_initialized(event);
    }

    fn on_order_event(&mut self, event: OrderEventAny) {
        let _ = self.dispatch_on_order_event(event);
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
        let _ = self.dispatch_on_order_updated(&event);
    }

    fn on_position_opened(&mut self, event: PositionOpened) {
        let _ = self.dispatch_on_position_opened(event);
    }

    fn on_position_event(&mut self, event: PositionEvent) {
        let _ = self.dispatch_on_position_event(event);
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

    fn on_save(&self) -> anyhow::Result<IndexMap<String, Vec<u8>>> {
        self.dispatch_on_save()
            .map_err(|e| anyhow::anyhow!("Python on_save failed: {e}"))
    }

    fn on_load(&mut self, state: IndexMap<String, Vec<u8>>) -> anyhow::Result<()> {
        self.dispatch_on_load(&state)
            .map_err(|e| anyhow::anyhow!("Python on_load failed: {e}"))
    }

    fn on_time_event(&mut self, event: &TimeEvent) -> anyhow::Result<()> {
        Strategy::on_time_event(self, event)?;
        self.dispatch_on_time_event(event)
            .map_err(|e| anyhow::anyhow!("Python on_time_event failed: {e}"))
    }

    #[allow(unused_variables)]
    fn on_data(&mut self, data: &CustomData) -> anyhow::Result<()> {
        Python::attach(|py| {
            let py_data: Py<PyAny> = Py::new(py, data.clone())?.into_any();
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
        self.dispatch_on_book_deltas(deltas)
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

    fn on_option_greeks(&mut self, greeks: &OptionGreeks) -> anyhow::Result<()> {
        self.dispatch_on_option_greeks(*greeks)
            .map_err(|e| anyhow::anyhow!("Python on_option_greeks failed: {e}"))
    }

    fn on_option_chain(&mut self, slice: &OptionChainSlice) -> anyhow::Result<()> {
        self.dispatch_on_option_chain(slice)
            .map_err(|e| anyhow::anyhow!("Python on_option_chain failed: {e}"))
    }

    fn on_historical_data(&mut self, data: &dyn Any) -> anyhow::Result<()> {
        Python::attach(|py| {
            let py_data: Py<PyAny> = if let Some(custom_data) = data.downcast_ref::<CustomData>() {
                Py::new(py, custom_data.clone())?.into_any()
            } else {
                anyhow::bail!("Failed to convert historical data to Python: unsupported type");
            };
            self.dispatch_on_historical_data(py_data)
                .map_err(|e| anyhow::anyhow!("Python on_historical_data failed: {e}"))
        })
    }

    fn on_historical_quotes(&mut self, quotes: &[QuoteTick]) -> anyhow::Result<()> {
        self.dispatch_on_historical_quotes(quotes.to_vec())
            .map_err(|e| anyhow::anyhow!("Python on_historical_quotes failed: {e}"))
    }

    fn on_historical_trades(&mut self, trades: &[TradeTick]) -> anyhow::Result<()> {
        self.dispatch_on_historical_trades(trades.to_vec())
            .map_err(|e| anyhow::anyhow!("Python on_historical_trades failed: {e}"))
    }

    fn on_historical_funding_rates(
        &mut self,
        funding_rates: &[FundingRateUpdate],
    ) -> anyhow::Result<()> {
        self.dispatch_on_historical_funding_rates(funding_rates.to_vec())
            .map_err(|e| anyhow::anyhow!("Python on_historical_funding_rates failed: {e}"))
    }

    fn on_historical_bars(&mut self, bars: &[Bar]) -> anyhow::Result<()> {
        self.dispatch_on_historical_bars(bars.to_vec())
            .map_err(|e| anyhow::anyhow!("Python on_historical_bars failed: {e}"))
    }

    fn on_historical_mark_prices(&mut self, mark_prices: &[MarkPriceUpdate]) -> anyhow::Result<()> {
        self.dispatch_on_historical_mark_prices(mark_prices.to_vec())
            .map_err(|e| anyhow::anyhow!("Python on_historical_mark_prices failed: {e}"))
    }

    fn on_historical_index_prices(
        &mut self,
        index_prices: &[IndexPriceUpdate],
    ) -> anyhow::Result<()> {
        self.dispatch_on_historical_index_prices(index_prices.to_vec())
            .map_err(|e| anyhow::anyhow!("Python on_historical_index_prices failed: {e}"))
    }

    fn on_order_filled(&mut self, event: &OrderFilled) -> anyhow::Result<()> {
        self.dispatch_on_order_filled(event)
            .map_err(|e| anyhow::anyhow!("Python on_order_filled failed: {e}"))
    }

    fn on_order_canceled(&mut self, event: &OrderCanceled) -> anyhow::Result<()> {
        self.dispatch_on_order_canceled(*event)
            .map_err(|e| anyhow::anyhow!("Python on_order_canceled failed: {e}"))
    }
}

fn state_to_pydict(py: Python<'_>, state: &IndexMap<String, Vec<u8>>) -> PyResult<Py<PyDict>> {
    let py_state = PyDict::new(py);
    for (key, value) in state {
        py_state.set_item(key, PyBytes::new(py, value))?;
    }
    Ok(py_state.unbind())
}

fn pydict_to_state(state: &Bound<'_, PyDict>) -> PyResult<IndexMap<String, Vec<u8>>> {
    let mut rust_state = IndexMap::with_capacity(state.len());
    for (key, value) in state.iter() {
        rust_state.insert(key.extract()?, value.extract()?);
    }
    Ok(rust_state)
}

/// Python-facing wrapper for Strategy.
#[allow(non_camel_case_types)]
#[pyo3::pyclass(
    module = "nautilus_trader.trading",
    name = "Strategy",
    unsendable,
    subclass
)]
#[pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.trading")]
pub struct PyStrategy {
    inner: Rc<UnsafeCell<PyStrategyInner>>,
}

impl Debug for PyStrategy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(PyStrategy))
            .field("inner", &self.inner())
            .finish()
    }
}

impl PyStrategy {
    #[inline]
    #[allow(unsafe_code)]
    pub(crate) fn inner(&self) -> &PyStrategyInner {
        // SAFETY: `PyStrategy` is `unsendable` so access is single-threaded, and
        // callers never hold a mutable and shared reference simultaneously.
        unsafe { &*self.inner.get() }
    }

    #[inline]
    #[allow(unsafe_code, clippy::mut_from_ref)]
    pub(crate) fn inner_mut(&self) -> &mut PyStrategyInner {
        // SAFETY: `PyStrategy` is `unsendable` so access is single-threaded, and
        // callers never hold a mutable and shared reference simultaneously.
        unsafe { &mut *self.inner.get() }
    }
}

impl PyStrategy {
    /// Creates a new `PyStrategy` instance.
    #[must_use]
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

    /// Updates configured external order claim instrument IDs before registration.
    pub fn set_external_order_claims(&mut self, external_order_claims: Option<Vec<InstrumentId>>) {
        self.inner_mut().core.config.external_order_claims = external_order_claims;
    }

    /// Returns the configured external order claim instrument IDs.
    #[must_use]
    pub fn external_order_claims(&self) -> Option<Vec<InstrumentId>> {
        self.inner().external_order_claims()
    }

    /// Updates the runtime strategy ID.
    ///
    /// Must only be called before registration. See `PyDataActor::set_actor_id`.
    pub fn set_strategy_id(&mut self, strategy_id: StrategyId) -> anyhow::Result<()> {
        let inner = self.inner_mut();
        inner.core.change_id(strategy_id);
        Ok(())
    }

    /// Updates the runtime order ID tag.
    pub fn set_order_id_tag(&mut self, order_id_tag: &str) -> anyhow::Result<()> {
        let inner = self.inner_mut();
        inner.core.change_order_id_tag(order_id_tag);
        Ok(())
    }

    /// Updates the runtime `log_events` setting.
    pub fn set_log_events(&mut self, log_events: bool) {
        let inner = self.inner_mut();
        inner.core.actor.config.log_events = log_events;
    }

    /// Updates the runtime `log_commands` setting.
    pub fn set_log_commands(&mut self, log_commands: bool) {
        let inner = self.inner_mut();
        inner.core.actor.config.log_commands = log_commands;
    }

    /// Returns the strategy ID.
    #[must_use]
    pub fn strategy_id(&self) -> StrategyId {
        StrategyId::from(self.inner().core.actor.actor_id.inner().as_str())
    }

    /// Returns a value indicating whether the strategy has been registered with a trader.
    #[must_use]
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
        with_component_registry(|registry| registry.insert(component_id, component_trait_ref));

        let actor_trait_ref: Rc<UnsafeCell<dyn Actor>> = inner_ref;
        with_actor_registry(|registry| registry.insert(actor_id, actor_trait_ref));
    }
}

#[pyo3::pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
#[expect(
    clippy::large_types_passed_by_value,
    clippy::unused_self,
    reason = "default PyO3 callbacks must remain instance methods and accept Python-owned event values"
)]
impl PyStrategy {
    /// Creates a new [`PyStrategy`] instance.
    ///
    /// Accepts `None` or any Python object. If the object is a [`StrategyConfig`]
    /// (or can be extracted as one via `from_py_object`), its values are used;
    /// otherwise the strategy falls back to [`StrategyConfig::default()`].
    ///
    /// This permissive signature is required so that Python subclasses can pass
    /// a **custom** config dataclass to their `__init__`; the Rust
    /// `add_strategy_from_config` then extracts `strategy_id`, `log_events`, etc.
    /// via `getattr` and calls the corresponding setters separately.
    #[new]
    #[pyo3(signature = (config=None))]
    fn py_new(config: Option<Py<PyAny>>) -> Self {
        let strategy_config =
            config.and_then(|obj| Python::attach(|py| obj.extract::<StrategyConfig>(py).ok()));
        Self::new(strategy_config)
    }

    /// Captures the Python self reference for Rust→Python event dispatch.
    #[pyo3(signature = (config=None))]
    #[allow(unused_variables, clippy::needless_pass_by_value)]
    fn __init__(slf: &Bound<'_, Self>, config: Option<Py<PyAny>>) {
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
        if inner.core.actor.is_registered() {
            Ok(inner.clock.clone())
        } else {
            Err(to_pyruntime_err(
                "Strategy must be registered with a trader before accessing clock",
            ))
        }
    }

    #[getter]
    #[pyo3(name = "cache")]
    fn py_cache(&self) -> PyResult<PyCache> {
        let inner = self.inner();
        if inner.core.actor.is_registered() {
            Ok(PyCache::from_rc(inner.core.actor.cache_rc()))
        } else {
            Err(to_pyruntime_err(
                "Strategy must be registered with a trader before accessing cache",
            ))
        }
    }

    #[getter]
    #[pyo3(name = "portfolio")]
    fn py_portfolio(&self) -> PyResult<PyPortfolio> {
        let inner = self.inner();
        if inner.core.actor.is_registered() {
            Ok(PyPortfolio::from_rc(inner.core.portfolio().clone()))
        } else {
            Err(to_pyruntime_err(
                "Strategy must be registered with a trader before accessing portfolio",
            ))
        }
    }

    #[getter]
    #[pyo3(name = "order_factory")]
    fn py_order_factory(&self) -> PyResult<PyOrderFactory> {
        let inner = self.inner();
        if inner.core.actor.is_registered() {
            Ok(PyOrderFactory::from_rc(inner.core.order_factory_rc()))
        } else {
            Err(to_pyruntime_err(
                "Strategy must be registered with a trader before accessing order_factory",
            ))
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
        let inner = self.inner_mut();
        if Strategy::stop(inner) {
            Component::stop(inner).map_err(to_pyruntime_err)
        } else {
            Ok(())
        }
    }

    #[pyo3(name = "market_exit")]
    fn py_market_exit(&mut self) -> PyResult<()> {
        Strategy::market_exit(self.inner_mut()).map_err(to_pyruntime_err)
    }

    #[pyo3(name = "is_exiting")]
    fn py_is_exiting(&self) -> bool {
        Strategy::is_exiting(self.inner())
    }

    #[pyo3(name = "save")]
    fn py_save(&self, py: Python<'_>) -> PyResult<Py<PyDict>> {
        let state = DataActor::on_save(self.inner()).map_err(to_pyruntime_err)?;
        state_to_pydict(py, &state)
    }

    #[pyo3(name = "load")]
    fn py_load(&mut self, state: &Bound<'_, PyDict>) -> PyResult<()> {
        let state = pydict_to_state(state)?;
        DataActor::on_load(self.inner_mut(), state).map_err(to_pyruntime_err)
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

    #[getter]
    #[pyo3(name = "registered_indicators")]
    fn py_registered_indicators(&self, py: Python<'_>) -> PyResult<Py<PyList>> {
        registered_python_indicators(py, self.inner().core.registered_indicators())
    }

    #[pyo3(name = "indicators_initialized")]
    fn py_indicators_initialized(&self, _py: Python<'_>) -> PyResult<bool> {
        self.inner()
            .core
            .indicators_initialized()
            .map_err(to_pyruntime_err)
    }

    #[pyo3(name = "register_indicator_for_quote_ticks")]
    fn py_register_indicator_for_quote_ticks(
        &mut self,
        py: Python<'_>,
        instrument_id: InstrumentId,
        indicator: Py<PyAny>,
    ) {
        let indicator = wrap_python_indicator(py, indicator);
        self.inner_mut()
            .core
            .register_indicator_for_quote_ticks(instrument_id, indicator);
    }

    #[pyo3(name = "register_indicator_for_trade_ticks")]
    fn py_register_indicator_for_trade_ticks(
        &mut self,
        py: Python<'_>,
        instrument_id: InstrumentId,
        indicator: Py<PyAny>,
    ) {
        let indicator = wrap_python_indicator(py, indicator);
        self.inner_mut()
            .core
            .register_indicator_for_trade_ticks(instrument_id, indicator);
    }

    #[pyo3(name = "register_indicator_for_bars")]
    fn py_register_indicator_for_bars(
        &mut self,
        py: Python<'_>,
        bar_type: BarType,
        indicator: Py<PyAny>,
    ) {
        let indicator = wrap_python_indicator(py, indicator);
        self.inner_mut()
            .core
            .register_indicator_for_bars(bar_type, indicator);
    }

    #[pyo3(name = "submit_order")]
    #[pyo3(signature = (order, position_id=None, client_id=None, params=None))]
    fn py_submit_order(
        &mut self,
        py: Python<'_>,
        order: Py<PyAny>,
        position_id: Option<PositionId>,
        client_id: Option<ClientId>,
        params: Option<Py<PyDict>>,
    ) -> PyResult<()> {
        let order = pyobject_to_order_any(py, order)?;
        let params_map = Python::attach(|py| -> PyResult<Option<Params>> {
            match params {
                Some(dict) => from_pydict(py, &dict),
                None => Ok(None),
            }
        })?;
        let inner = self.inner_mut();

        Strategy::submit_order(inner, order, position_id, client_id, params_map)
            .map_err(to_pyruntime_err)
    }

    #[pyo3(name = "submit_order_list")]
    #[pyo3(signature = (order_list, position_id=None, client_id=None, params=None))]
    #[expect(
        clippy::needless_pass_by_value,
        reason = "PyO3 owns extracted method arguments before Rust conversion"
    )]
    fn py_submit_order_list(
        &mut self,
        py: Python<'_>,
        order_list: Py<PyAny>,
        position_id: Option<PositionId>,
        client_id: Option<ClientId>,
        params: Option<Py<PyDict>>,
    ) -> PyResult<()> {
        let orders = py_order_list_to_orders(py, &order_list)?;
        let params_map = Python::attach(|py| -> PyResult<Option<Params>> {
            match params {
                Some(dict) => from_pydict(py, &dict),
                None => Ok(None),
            }
        })?;
        let inner = self.inner_mut();

        Strategy::submit_order_list(inner, orders, position_id, client_id, params_map)
            .map_err(to_pyruntime_err)
    }

    #[pyo3(name = "modify_order")]
    #[pyo3(signature = (client_order_id, quantity=None, price=None, trigger_price=None, client_id=None, params=None))]
    fn py_modify_order(
        &mut self,
        client_order_id: ClientOrderId,
        quantity: Option<Quantity>,
        price: Option<Price>,
        trigger_price: Option<Price>,
        client_id: Option<ClientId>,
        params: Option<Py<PyDict>>,
    ) -> PyResult<()> {
        let params_map = Python::attach(|py| -> PyResult<Option<Params>> {
            match params {
                Some(dict) => from_pydict(py, &dict),
                None => Ok(None),
            }
        })?;
        let inner = self.inner_mut();

        Strategy::modify_order(
            inner,
            client_order_id,
            quantity,
            price,
            trigger_price,
            client_id,
            params_map,
        )
        .map_err(to_pyruntime_err)
    }

    #[pyo3(name = "modify_orders")]
    #[pyo3(signature = (updates, client_id=None, params=None))]
    fn py_modify_orders(
        &mut self,
        updates: Vec<BatchModifyOrder>,
        client_id: Option<ClientId>,
        params: Option<Py<PyDict>>,
    ) -> PyResult<()> {
        let params_map = Python::attach(|py| -> PyResult<Option<Params>> {
            match params {
                Some(dict) => from_pydict(py, &dict),
                None => Ok(None),
            }
        })?;

        Strategy::modify_orders(self.inner_mut(), updates, client_id, params_map)
            .map_err(to_pyruntime_err)
    }

    #[pyo3(name = "cancel_order")]
    #[pyo3(signature = (client_order_id, client_id=None, params=None))]
    fn py_cancel_order(
        &mut self,
        client_order_id: ClientOrderId,
        client_id: Option<ClientId>,
        params: Option<Py<PyDict>>,
    ) -> PyResult<()> {
        let params_map = Python::attach(|py| -> PyResult<Option<Params>> {
            match params {
                Some(dict) => from_pydict(py, &dict),
                None => Ok(None),
            }
        })?;
        let inner = self.inner_mut();

        Strategy::cancel_order(inner, client_order_id, client_id, params_map)
            .map_err(to_pyruntime_err)
    }

    /// Cancels the managed GTD expiry for the given order.
    #[pyo3(name = "cancel_gtd_expiry")]
    #[pyo3(signature = (order))]
    fn py_cancel_gtd_expiry(&mut self, py: Python<'_>, order: Py<PyAny>) -> PyResult<()> {
        let order = pyobject_to_order_any(py, order)?;

        Strategy::cancel_gtd_expiry(self.inner_mut(), &order.client_order_id());
        Ok(())
    }

    #[pyo3(name = "cancel_orders")]
    #[pyo3(signature = (client_order_ids, client_id=None, params=None))]
    fn py_cancel_orders(
        &mut self,
        client_order_ids: Vec<ClientOrderId>,
        client_id: Option<ClientId>,
        params: Option<Py<PyDict>>,
    ) -> PyResult<()> {
        let params_map = Python::attach(|py| -> PyResult<Option<Params>> {
            match params {
                Some(dict) => from_pydict(py, &dict),
                None => Ok(None),
            }
        })?;

        Strategy::cancel_orders(self.inner_mut(), client_order_ids, client_id, params_map)
            .map_err(to_pyruntime_err)
    }

    #[pyo3(name = "cancel_all_orders")]
    #[pyo3(signature = (instrument_id, order_side=None, client_id=None, params=None))]
    fn py_cancel_all_orders(
        &mut self,
        instrument_id: InstrumentId,
        order_side: Option<OrderSide>,
        client_id: Option<ClientId>,
        params: Option<Py<PyDict>>,
    ) -> PyResult<()> {
        let params_map = Python::attach(|py| -> PyResult<Option<Params>> {
            match params {
                Some(dict) => from_pydict(py, &dict),
                None => Ok(None),
            }
        })?;
        Strategy::cancel_all_orders(
            self.inner_mut(),
            instrument_id,
            order_side,
            client_id,
            params_map,
        )
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
    #[expect(clippy::too_many_arguments)]
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
    #[pyo3(signature = (account_id, client_id=None, params=None))]
    fn py_query_account(
        &mut self,
        py: Python<'_>,
        account_id: AccountId,
        client_id: Option<ClientId>,
        params: Option<Py<PyDict>>,
    ) -> PyResult<()> {
        let params_map = match params {
            Some(dict) => from_pydict(py, &dict)?,
            None => None,
        };
        Strategy::query_account(self.inner_mut(), account_id, client_id, params_map)
            .map_err(to_pyruntime_err)
    }

    #[pyo3(name = "query_order")]
    #[pyo3(signature = (order, client_id=None, params=None))]
    fn py_query_order(
        &mut self,
        py: Python<'_>,
        order: Py<PyAny>,
        client_id: Option<ClientId>,
        params: Option<Py<PyDict>>,
    ) -> PyResult<()> {
        let order = pyobject_to_order_any(py, order)?;
        let params_map = match params {
            Some(dict) => from_pydict(py, &dict)?,
            None => None,
        };
        Strategy::query_order(self.inner_mut(), &order, client_id, params_map)
            .map_err(to_pyruntime_err)
    }

    #[pyo3(name = "on_start")]
    fn py_on_start(&mut self) {}

    #[pyo3(name = "on_stop")]
    fn py_on_stop(&mut self) {}

    #[pyo3(name = "on_resume")]
    fn py_on_resume(&mut self) {}

    #[pyo3(name = "on_reset")]
    fn py_on_reset(&mut self) {}

    #[pyo3(name = "on_dispose")]
    fn py_on_dispose(&mut self) {}

    #[pyo3(name = "on_degrade")]
    fn py_on_degrade(&mut self) {}

    #[pyo3(name = "on_fault")]
    fn py_on_fault(&mut self) {}

    #[pyo3(name = "on_save")]
    fn py_on_save(&self, py: Python<'_>) -> Py<PyDict> {
        PyDict::new(py).unbind()
    }

    #[allow(unused_variables)]
    #[pyo3(name = "on_load")]
    fn py_on_load(&mut self, state: &Bound<'_, PyDict>) {}

    #[allow(unused_variables, clippy::needless_pass_by_value)]
    #[pyo3(name = "on_time_event")]
    fn py_on_time_event(&mut self, event: TimeEvent) {}

    #[allow(unused_variables, clippy::needless_pass_by_value)]
    #[pyo3(name = "on_data")]
    fn py_on_data(&mut self, data: Py<PyAny>) {}

    #[allow(unused_variables)]
    #[pyo3(name = "on_signal")]
    fn py_on_signal(&mut self, signal: &Signal) {}

    #[allow(unused_variables, clippy::needless_pass_by_value)]
    #[pyo3(name = "on_instrument")]
    fn py_on_instrument(&mut self, instrument: Py<PyAny>) {}

    #[allow(unused_variables)]
    #[pyo3(name = "on_quote")]
    fn py_on_quote(&mut self, quote: QuoteTick) {}

    #[allow(unused_variables)]
    #[pyo3(name = "on_trade")]
    fn py_on_trade(&mut self, trade: TradeTick) {}

    #[allow(unused_variables)]
    #[pyo3(name = "on_bar")]
    fn py_on_bar(&mut self, bar: Bar) {}

    #[allow(unused_variables, clippy::needless_pass_by_value)]
    #[pyo3(name = "on_book_deltas")]
    fn py_on_book_deltas(&mut self, deltas: OrderBookDeltas) {}

    #[allow(unused_variables)]
    #[pyo3(name = "on_book")]
    fn py_on_book(&mut self, book: &OrderBook) {}

    #[allow(unused_variables)]
    #[pyo3(name = "on_mark_price")]
    fn py_on_mark_price(&mut self, mark_price: MarkPriceUpdate) {}

    #[allow(unused_variables)]
    #[pyo3(name = "on_index_price")]
    fn py_on_index_price(&mut self, index_price: IndexPriceUpdate) {}

    #[allow(unused_variables)]
    #[pyo3(name = "on_funding_rate")]
    fn py_on_funding_rate(&mut self, funding_rate: FundingRateUpdate) {}

    #[allow(unused_variables)]
    #[pyo3(name = "on_instrument_status")]
    fn py_on_instrument_status(&mut self, status: InstrumentStatus) {}

    #[allow(unused_variables)]
    #[pyo3(name = "on_instrument_close")]
    fn py_on_instrument_close(&mut self, close: InstrumentClose) {}

    #[allow(unused_variables)]
    #[pyo3(name = "on_option_greeks")]
    fn py_on_option_greeks(&mut self, greeks: OptionGreeks) {}

    #[allow(unused_variables, clippy::needless_pass_by_value)]
    #[pyo3(name = "on_option_chain")]
    fn py_on_option_chain(&mut self, slice: OptionChainSlice) {}

    #[pyo3(name = "on_market_exit")]
    fn py_on_market_exit(&mut self) {}

    #[pyo3(name = "post_market_exit")]
    fn py_post_market_exit(&mut self) {}

    #[allow(unused_variables, clippy::needless_pass_by_value)]
    #[pyo3(name = "on_order_initialized")]
    fn py_on_order_initialized(&mut self, event: OrderInitialized) {}

    #[allow(unused_variables, clippy::needless_pass_by_value)]
    #[pyo3(name = "on_order_event")]
    fn py_on_order_event(&mut self, event: Py<PyAny>) {}

    #[allow(unused_variables)]
    #[pyo3(name = "on_order_denied")]
    fn py_on_order_denied(&mut self, event: OrderDenied) {}

    #[allow(unused_variables)]
    #[pyo3(name = "on_order_emulated")]
    fn py_on_order_emulated(&mut self, event: OrderEmulated) {}

    #[allow(unused_variables)]
    #[pyo3(name = "on_order_released")]
    fn py_on_order_released(&mut self, event: OrderReleased) {}

    #[allow(unused_variables)]
    #[pyo3(name = "on_order_submitted")]
    fn py_on_order_submitted(&mut self, event: OrderSubmitted) {}

    #[allow(unused_variables)]
    #[pyo3(name = "on_order_rejected")]
    fn py_on_order_rejected(&mut self, event: OrderRejected) {}

    #[allow(unused_variables)]
    #[pyo3(name = "on_order_accepted")]
    fn py_on_order_accepted(&mut self, event: OrderAccepted) {}

    #[allow(unused_variables)]
    #[pyo3(name = "on_order_expired")]
    fn py_on_order_expired(&mut self, event: OrderExpired) {}

    #[allow(unused_variables)]
    #[pyo3(name = "on_order_triggered")]
    fn py_on_order_triggered(&mut self, event: OrderTriggered) {}

    #[allow(unused_variables)]
    #[pyo3(name = "on_order_pending_update")]
    fn py_on_order_pending_update(&mut self, event: OrderPendingUpdate) {}

    #[allow(unused_variables)]
    #[pyo3(name = "on_order_pending_cancel")]
    fn py_on_order_pending_cancel(&mut self, event: OrderPendingCancel) {}

    #[allow(unused_variables)]
    #[pyo3(name = "on_order_modify_rejected")]
    fn py_on_order_modify_rejected(&mut self, event: OrderModifyRejected) {}

    #[allow(unused_variables)]
    #[pyo3(name = "on_order_cancel_rejected")]
    fn py_on_order_cancel_rejected(&mut self, event: OrderCancelRejected) {}

    #[allow(unused_variables)]
    #[pyo3(name = "on_order_updated")]
    fn py_on_order_updated(&mut self, event: OrderUpdated) {}

    #[allow(unused_variables)]
    #[pyo3(name = "on_order_canceled")]
    fn py_on_order_canceled(&mut self, event: OrderCanceled) {}

    #[allow(unused_variables)]
    #[pyo3(name = "on_order_filled")]
    fn py_on_order_filled(&mut self, event: OrderFilled) {}

    #[allow(unused_variables, clippy::needless_pass_by_value)]
    #[pyo3(name = "on_position_opened")]
    fn py_on_position_opened(&mut self, event: PositionOpened) {}

    #[allow(unused_variables, clippy::needless_pass_by_value)]
    #[pyo3(name = "on_position_event")]
    fn py_on_position_event(&mut self, event: Py<PyAny>) {}

    #[allow(unused_variables, clippy::needless_pass_by_value)]
    #[pyo3(name = "on_position_changed")]
    fn py_on_position_changed(&mut self, event: PositionChanged) {}

    #[allow(unused_variables, clippy::needless_pass_by_value)]
    #[pyo3(name = "on_position_closed")]
    fn py_on_position_closed(&mut self, event: PositionClosed) {}

    #[allow(unused_variables, clippy::needless_pass_by_value)]
    #[pyo3(name = "on_historical_data")]
    fn py_on_historical_data(&mut self, data: Py<PyAny>) {
        // Default implementation - can be overridden in Python subclasses
    }

    #[allow(unused_variables, clippy::needless_pass_by_value)]
    #[pyo3(name = "on_historical_quotes")]
    fn py_on_historical_quotes(&mut self, quotes: Vec<QuoteTick>) {
        // Default implementation - can be overridden in Python subclasses
    }

    #[allow(unused_variables, clippy::needless_pass_by_value)]
    #[pyo3(name = "on_historical_trades")]
    fn py_on_historical_trades(&mut self, trades: Vec<TradeTick>) {
        // Default implementation - can be overridden in Python subclasses
    }

    #[allow(unused_variables, clippy::needless_pass_by_value)]
    #[pyo3(name = "on_historical_funding_rates")]
    fn py_on_historical_funding_rates(&mut self, funding_rates: Vec<FundingRateUpdate>) {
        // Default implementation - can be overridden in Python subclasses
    }

    #[allow(unused_variables, clippy::needless_pass_by_value)]
    #[pyo3(name = "on_historical_bars")]
    fn py_on_historical_bars(&mut self, bars: Vec<Bar>) {
        // Default implementation - can be overridden in Python subclasses
    }

    #[allow(unused_variables, clippy::needless_pass_by_value)]
    #[pyo3(name = "on_historical_mark_prices")]
    fn py_on_historical_mark_prices(&mut self, mark_prices: Vec<MarkPriceUpdate>) {
        // Default implementation - can be overridden in Python subclasses
    }

    #[allow(unused_variables, clippy::needless_pass_by_value)]
    #[pyo3(name = "on_historical_index_prices")]
    fn py_on_historical_index_prices(&mut self, index_prices: Vec<IndexPriceUpdate>) {
        // Default implementation - can be overridden in Python subclasses
    }

    #[pyo3(name = "subscribe_data")]
    #[pyo3(signature = (data_type, client_id=None, params=None))]
    fn py_subscribe_data(
        &mut self,
        data_type: DataType,
        client_id: Option<ClientId>,
        params: Option<Py<PyDict>>,
    ) -> PyResult<()> {
        let params_map = Python::attach(|py| -> PyResult<Option<Params>> {
            match params {
                Some(dict) => from_pydict(py, &dict),
                None => Ok(None),
            }
        })?;
        DataActor::subscribe_data(self.inner_mut(), data_type, client_id, params_map);
        Ok(())
    }

    #[pyo3(name = "subscribe_instruments")]
    #[pyo3(signature = (venue, client_id=None, params=None))]
    fn py_subscribe_instruments(
        &mut self,
        venue: Venue,
        client_id: Option<ClientId>,
        params: Option<Py<PyDict>>,
    ) -> PyResult<()> {
        let params_map = Python::attach(|py| -> PyResult<Option<Params>> {
            match params {
                Some(dict) => from_pydict(py, &dict),
                None => Ok(None),
            }
        })?;
        DataActor::subscribe_instruments(self.inner_mut(), venue, client_id, params_map);
        Ok(())
    }

    #[pyo3(name = "subscribe_instrument")]
    #[pyo3(signature = (instrument_id, client_id=None, params=None))]
    fn py_subscribe_instrument(
        &mut self,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<Py<PyDict>>,
    ) -> PyResult<()> {
        let params_map = Python::attach(|py| -> PyResult<Option<Params>> {
            match params {
                Some(dict) => from_pydict(py, &dict),
                None => Ok(None),
            }
        })?;
        DataActor::subscribe_instrument(self.inner_mut(), instrument_id, client_id, params_map);
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
        params: Option<Py<PyDict>>,
    ) -> PyResult<()> {
        let params_map = Python::attach(|py| -> PyResult<Option<Params>> {
            match params {
                Some(dict) => from_pydict(py, &dict),
                None => Ok(None),
            }
        })?;
        let depth = depth.and_then(NonZeroUsize::new);
        DataActor::subscribe_book_deltas(
            self.inner_mut(),
            instrument_id,
            book_type,
            depth,
            client_id,
            managed,
            params_map,
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
        params: Option<Py<PyDict>>,
    ) -> PyResult<()> {
        let params_map = Python::attach(|py| -> PyResult<Option<Params>> {
            match params {
                Some(dict) => from_pydict(py, &dict),
                None => Ok(None),
            }
        })?;
        let depth = depth.and_then(NonZeroUsize::new);
        let interval_ms = NonZeroUsize::new(interval_ms)
            .ok_or_else(|| to_pyvalue_err("interval_ms must be > 0"))?;

        DataActor::subscribe_book_at_interval(
            self.inner_mut(),
            instrument_id,
            book_type,
            depth,
            interval_ms,
            client_id,
            params_map,
        );
        Ok(())
    }

    #[pyo3(name = "subscribe_quotes")]
    #[pyo3(signature = (instrument_id, client_id=None, params=None))]
    fn py_subscribe_quotes(
        &mut self,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<Py<PyDict>>,
    ) -> PyResult<()> {
        let params_map = Python::attach(|py| -> PyResult<Option<Params>> {
            match params {
                Some(dict) => from_pydict(py, &dict),
                None => Ok(None),
            }
        })?;
        DataActor::subscribe_quotes(self.inner_mut(), instrument_id, client_id, params_map);
        Ok(())
    }

    #[pyo3(name = "subscribe_trades")]
    #[pyo3(signature = (instrument_id, client_id=None, params=None))]
    fn py_subscribe_trades(
        &mut self,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<Py<PyDict>>,
    ) -> PyResult<()> {
        let params_map = Python::attach(|py| -> PyResult<Option<Params>> {
            match params {
                Some(dict) => from_pydict(py, &dict),
                None => Ok(None),
            }
        })?;
        DataActor::subscribe_trades(self.inner_mut(), instrument_id, client_id, params_map);
        Ok(())
    }

    #[pyo3(name = "subscribe_bars")]
    #[pyo3(signature = (bar_type, client_id=None, params=None))]
    fn py_subscribe_bars(
        &mut self,
        bar_type: BarType,
        client_id: Option<ClientId>,
        params: Option<Py<PyDict>>,
    ) -> PyResult<()> {
        let params_map = Python::attach(|py| -> PyResult<Option<Params>> {
            match params {
                Some(dict) => from_pydict(py, &dict),
                None => Ok(None),
            }
        })?;
        DataActor::subscribe_bars(self.inner_mut(), bar_type, client_id, params_map);
        Ok(())
    }

    #[pyo3(name = "subscribe_mark_prices")]
    #[pyo3(signature = (instrument_id, client_id=None, params=None))]
    fn py_subscribe_mark_prices(
        &mut self,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<Py<PyDict>>,
    ) -> PyResult<()> {
        let params_map = Python::attach(|py| -> PyResult<Option<Params>> {
            match params {
                Some(dict) => from_pydict(py, &dict),
                None => Ok(None),
            }
        })?;
        DataActor::subscribe_mark_prices(self.inner_mut(), instrument_id, client_id, params_map);
        Ok(())
    }

    #[pyo3(name = "subscribe_index_prices")]
    #[pyo3(signature = (instrument_id, client_id=None, params=None))]
    fn py_subscribe_index_prices(
        &mut self,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<Py<PyDict>>,
    ) -> PyResult<()> {
        let params_map = Python::attach(|py| -> PyResult<Option<Params>> {
            match params {
                Some(dict) => from_pydict(py, &dict),
                None => Ok(None),
            }
        })?;
        DataActor::subscribe_index_prices(self.inner_mut(), instrument_id, client_id, params_map);
        Ok(())
    }

    #[pyo3(name = "subscribe_funding_rates")]
    #[pyo3(signature = (instrument_id, client_id=None, params=None))]
    fn py_subscribe_funding_rates(
        &mut self,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<Py<PyDict>>,
    ) -> PyResult<()> {
        let params_map = Python::attach(|py| -> PyResult<Option<Params>> {
            match params {
                Some(dict) => from_pydict(py, &dict),
                None => Ok(None),
            }
        })?;
        DataActor::subscribe_funding_rates(self.inner_mut(), instrument_id, client_id, params_map);
        Ok(())
    }

    #[pyo3(name = "subscribe_option_greeks")]
    #[pyo3(signature = (instrument_id, client_id=None, params=None))]
    fn py_subscribe_option_greeks(
        &mut self,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<Py<PyDict>>,
    ) -> PyResult<()> {
        let params_map = Python::attach(|py| -> PyResult<Option<Params>> {
            match params {
                Some(dict) => from_pydict(py, &dict),
                None => Ok(None),
            }
        })?;
        DataActor::subscribe_option_greeks(self.inner_mut(), instrument_id, client_id, params_map);
        Ok(())
    }

    #[pyo3(name = "subscribe_instrument_status")]
    #[pyo3(signature = (instrument_id, client_id=None, params=None))]
    fn py_subscribe_instrument_status(
        &mut self,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<Py<PyDict>>,
    ) -> PyResult<()> {
        let params_map = Python::attach(|py| -> PyResult<Option<Params>> {
            match params {
                Some(dict) => from_pydict(py, &dict),
                None => Ok(None),
            }
        })?;
        DataActor::subscribe_instrument_status(
            self.inner_mut(),
            instrument_id,
            client_id,
            params_map,
        );
        Ok(())
    }

    #[pyo3(name = "subscribe_instrument_close")]
    #[pyo3(signature = (instrument_id, client_id=None, params=None))]
    fn py_subscribe_instrument_close(
        &mut self,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<Py<PyDict>>,
    ) -> PyResult<()> {
        let params_map = Python::attach(|py| -> PyResult<Option<Params>> {
            match params {
                Some(dict) => from_pydict(py, &dict),
                None => Ok(None),
            }
        })?;
        DataActor::subscribe_instrument_close(
            self.inner_mut(),
            instrument_id,
            client_id,
            params_map,
        );
        Ok(())
    }

    #[pyo3(name = "subscribe_option_chain")]
    #[pyo3(signature = (series_id, strike_range, snapshot_interval_ms=None, client_id=None, params=None))]
    fn py_subscribe_option_chain(
        &mut self,
        py: Python<'_>,
        series_id: OptionSeriesId,
        strike_range: PyStrikeRange,
        snapshot_interval_ms: Option<u64>,
        client_id: Option<ClientId>,
        params: Option<Py<PyDict>>,
    ) -> PyResult<()> {
        let params_map = match params {
            Some(dict) => from_pydict(py, &dict)?,
            None => None,
        };
        DataActor::subscribe_option_chain(
            self.inner_mut(),
            series_id,
            strike_range.inner,
            snapshot_interval_ms,
            client_id,
            params_map,
        );
        Ok(())
    }

    #[pyo3(name = "subscribe_order_fills")]
    #[pyo3(signature = (instrument_id))]
    fn py_subscribe_order_fills(&mut self, instrument_id: InstrumentId) {
        DataActor::subscribe_order_fills(self.inner_mut(), instrument_id);
    }

    #[pyo3(name = "subscribe_order_cancels")]
    #[pyo3(signature = (instrument_id))]
    fn py_subscribe_order_cancels(&mut self, instrument_id: InstrumentId) {
        DataActor::subscribe_order_cancels(self.inner_mut(), instrument_id);
    }

    #[pyo3(name = "unsubscribe_data")]
    #[pyo3(signature = (data_type, client_id=None, params=None))]
    fn py_unsubscribe_data(
        &mut self,
        data_type: DataType,
        client_id: Option<ClientId>,
        params: Option<Py<PyDict>>,
    ) -> PyResult<()> {
        let params_map = Python::attach(|py| -> PyResult<Option<Params>> {
            match params {
                Some(dict) => from_pydict(py, &dict),
                None => Ok(None),
            }
        })?;
        DataActor::unsubscribe_data(self.inner_mut(), data_type, client_id, params_map);
        Ok(())
    }

    #[pyo3(name = "unsubscribe_instruments")]
    #[pyo3(signature = (venue, client_id=None, params=None))]
    fn py_unsubscribe_instruments(
        &mut self,
        venue: Venue,
        client_id: Option<ClientId>,
        params: Option<Py<PyDict>>,
    ) -> PyResult<()> {
        let params_map = Python::attach(|py| -> PyResult<Option<Params>> {
            match params {
                Some(dict) => from_pydict(py, &dict),
                None => Ok(None),
            }
        })?;
        DataActor::unsubscribe_instruments(self.inner_mut(), venue, client_id, params_map);
        Ok(())
    }

    #[pyo3(name = "unsubscribe_instrument")]
    #[pyo3(signature = (instrument_id, client_id=None, params=None))]
    fn py_unsubscribe_instrument(
        &mut self,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<Py<PyDict>>,
    ) -> PyResult<()> {
        let params_map = Python::attach(|py| -> PyResult<Option<Params>> {
            match params {
                Some(dict) => from_pydict(py, &dict),
                None => Ok(None),
            }
        })?;
        DataActor::unsubscribe_instrument(self.inner_mut(), instrument_id, client_id, params_map);
        Ok(())
    }

    #[pyo3(name = "unsubscribe_book_deltas")]
    #[pyo3(signature = (instrument_id, client_id=None, params=None))]
    fn py_unsubscribe_book_deltas(
        &mut self,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<Py<PyDict>>,
    ) -> PyResult<()> {
        let params_map = Python::attach(|py| -> PyResult<Option<Params>> {
            match params {
                Some(dict) => from_pydict(py, &dict),
                None => Ok(None),
            }
        })?;
        DataActor::unsubscribe_book_deltas(self.inner_mut(), instrument_id, client_id, params_map);
        Ok(())
    }

    #[pyo3(name = "unsubscribe_book_at_interval")]
    #[pyo3(signature = (instrument_id, interval_ms, client_id=None, params=None))]
    fn py_unsubscribe_book_at_interval(
        &mut self,
        instrument_id: InstrumentId,
        interval_ms: usize,
        client_id: Option<ClientId>,
        params: Option<Py<PyDict>>,
    ) -> PyResult<()> {
        let params_map = Python::attach(|py| -> PyResult<Option<Params>> {
            match params {
                Some(dict) => from_pydict(py, &dict),
                None => Ok(None),
            }
        })?;
        let interval_ms = NonZeroUsize::new(interval_ms)
            .ok_or_else(|| to_pyvalue_err("interval_ms must be > 0"))?;

        DataActor::unsubscribe_book_at_interval(
            self.inner_mut(),
            instrument_id,
            interval_ms,
            client_id,
            params_map,
        );
        Ok(())
    }

    #[pyo3(name = "unsubscribe_quotes")]
    #[pyo3(signature = (instrument_id, client_id=None, params=None))]
    fn py_unsubscribe_quotes(
        &mut self,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<Py<PyDict>>,
    ) -> PyResult<()> {
        let params_map = Python::attach(|py| -> PyResult<Option<Params>> {
            match params {
                Some(dict) => from_pydict(py, &dict),
                None => Ok(None),
            }
        })?;
        DataActor::unsubscribe_quotes(self.inner_mut(), instrument_id, client_id, params_map);
        Ok(())
    }

    #[pyo3(name = "unsubscribe_trades")]
    #[pyo3(signature = (instrument_id, client_id=None, params=None))]
    fn py_unsubscribe_trades(
        &mut self,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<Py<PyDict>>,
    ) -> PyResult<()> {
        let params_map = Python::attach(|py| -> PyResult<Option<Params>> {
            match params {
                Some(dict) => from_pydict(py, &dict),
                None => Ok(None),
            }
        })?;
        DataActor::unsubscribe_trades(self.inner_mut(), instrument_id, client_id, params_map);
        Ok(())
    }

    #[pyo3(name = "unsubscribe_bars")]
    #[pyo3(signature = (bar_type, client_id=None, params=None))]
    fn py_unsubscribe_bars(
        &mut self,
        bar_type: BarType,
        client_id: Option<ClientId>,
        params: Option<Py<PyDict>>,
    ) -> PyResult<()> {
        let params_map = Python::attach(|py| -> PyResult<Option<Params>> {
            match params {
                Some(dict) => from_pydict(py, &dict),
                None => Ok(None),
            }
        })?;
        DataActor::unsubscribe_bars(self.inner_mut(), bar_type, client_id, params_map);
        Ok(())
    }

    #[pyo3(name = "unsubscribe_mark_prices")]
    #[pyo3(signature = (instrument_id, client_id=None, params=None))]
    fn py_unsubscribe_mark_prices(
        &mut self,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<Py<PyDict>>,
    ) -> PyResult<()> {
        let params_map = Python::attach(|py| -> PyResult<Option<Params>> {
            match params {
                Some(dict) => from_pydict(py, &dict),
                None => Ok(None),
            }
        })?;
        DataActor::unsubscribe_mark_prices(self.inner_mut(), instrument_id, client_id, params_map);
        Ok(())
    }

    #[pyo3(name = "unsubscribe_index_prices")]
    #[pyo3(signature = (instrument_id, client_id=None, params=None))]
    fn py_unsubscribe_index_prices(
        &mut self,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<Py<PyDict>>,
    ) -> PyResult<()> {
        let params_map = Python::attach(|py| -> PyResult<Option<Params>> {
            match params {
                Some(dict) => from_pydict(py, &dict),
                None => Ok(None),
            }
        })?;
        DataActor::unsubscribe_index_prices(self.inner_mut(), instrument_id, client_id, params_map);
        Ok(())
    }

    #[pyo3(name = "unsubscribe_funding_rates")]
    #[pyo3(signature = (instrument_id, client_id=None, params=None))]
    fn py_unsubscribe_funding_rates(
        &mut self,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<Py<PyDict>>,
    ) -> PyResult<()> {
        let params_map = Python::attach(|py| -> PyResult<Option<Params>> {
            match params {
                Some(dict) => from_pydict(py, &dict),
                None => Ok(None),
            }
        })?;
        DataActor::unsubscribe_funding_rates(
            self.inner_mut(),
            instrument_id,
            client_id,
            params_map,
        );
        Ok(())
    }

    #[pyo3(name = "unsubscribe_option_greeks")]
    #[pyo3(signature = (instrument_id, client_id=None, params=None))]
    fn py_unsubscribe_option_greeks(
        &mut self,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<Py<PyDict>>,
    ) -> PyResult<()> {
        let params_map = Python::attach(|py| -> PyResult<Option<Params>> {
            match params {
                Some(dict) => from_pydict(py, &dict),
                None => Ok(None),
            }
        })?;
        DataActor::unsubscribe_option_greeks(
            self.inner_mut(),
            instrument_id,
            client_id,
            params_map,
        );
        Ok(())
    }

    #[pyo3(name = "unsubscribe_instrument_status")]
    #[pyo3(signature = (instrument_id, client_id=None, params=None))]
    fn py_unsubscribe_instrument_status(
        &mut self,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<Py<PyDict>>,
    ) -> PyResult<()> {
        let params_map = Python::attach(|py| -> PyResult<Option<Params>> {
            match params {
                Some(dict) => from_pydict(py, &dict),
                None => Ok(None),
            }
        })?;
        DataActor::unsubscribe_instrument_status(
            self.inner_mut(),
            instrument_id,
            client_id,
            params_map,
        );
        Ok(())
    }

    #[pyo3(name = "unsubscribe_instrument_close")]
    #[pyo3(signature = (instrument_id, client_id=None, params=None))]
    fn py_unsubscribe_instrument_close(
        &mut self,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<Py<PyDict>>,
    ) -> PyResult<()> {
        let params_map = Python::attach(|py| -> PyResult<Option<Params>> {
            match params {
                Some(dict) => from_pydict(py, &dict),
                None => Ok(None),
            }
        })?;
        DataActor::unsubscribe_instrument_close(
            self.inner_mut(),
            instrument_id,
            client_id,
            params_map,
        );
        Ok(())
    }

    #[pyo3(name = "unsubscribe_option_chain")]
    #[pyo3(signature = (series_id, client_id=None))]
    fn py_unsubscribe_option_chain(
        &mut self,
        series_id: OptionSeriesId,
        client_id: Option<ClientId>,
    ) {
        DataActor::unsubscribe_option_chain(self.inner_mut(), series_id, client_id);
    }

    #[pyo3(name = "unsubscribe_order_fills")]
    #[pyo3(signature = (instrument_id))]
    fn py_unsubscribe_order_fills(&mut self, instrument_id: InstrumentId) {
        DataActor::unsubscribe_order_fills(self.inner_mut(), instrument_id);
    }

    #[pyo3(name = "unsubscribe_order_cancels")]
    #[pyo3(signature = (instrument_id))]
    fn py_unsubscribe_order_cancels(&mut self, instrument_id: InstrumentId) {
        DataActor::unsubscribe_order_cancels(self.inner_mut(), instrument_id);
    }

    #[pyo3(name = "request_data")]
    #[pyo3(signature = (data_type, client_id, start=None, end=None, limit=None, params=None))]
    fn py_request_data(
        &mut self,
        data_type: DataType,
        client_id: ClientId,
        start: Option<DateTime<Utc>>,
        end: Option<DateTime<Utc>>,
        limit: Option<usize>,
        params: Option<Py<PyDict>>,
    ) -> PyResult<String> {
        let params_map = Python::attach(|py| -> PyResult<Option<Params>> {
            match params {
                Some(dict) => from_pydict(py, &dict),
                None => Ok(None),
            }
        })?;
        let limit = limit.and_then(NonZeroUsize::new);
        let request_id = DataActor::request_data(
            self.inner_mut(),
            data_type,
            client_id,
            start,
            end,
            limit,
            params_map,
        )
        .map_err(to_pyvalue_err)?;
        Ok(request_id.to_string())
    }

    #[pyo3(name = "request_instrument")]
    #[pyo3(signature = (instrument_id, start=None, end=None, client_id=None, params=None))]
    fn py_request_instrument(
        &mut self,
        instrument_id: InstrumentId,
        start: Option<DateTime<Utc>>,
        end: Option<DateTime<Utc>>,
        client_id: Option<ClientId>,
        params: Option<Py<PyDict>>,
    ) -> PyResult<String> {
        let params_map = Python::attach(|py| -> PyResult<Option<Params>> {
            match params {
                Some(dict) => from_pydict(py, &dict),
                None => Ok(None),
            }
        })?;
        let request_id = DataActor::request_instrument(
            self.inner_mut(),
            instrument_id,
            start,
            end,
            client_id,
            params_map,
        )
        .map_err(to_pyvalue_err)?;
        Ok(request_id.to_string())
    }

    #[pyo3(name = "request_instruments")]
    #[pyo3(signature = (venue=None, start=None, end=None, client_id=None, params=None))]
    fn py_request_instruments(
        &mut self,
        venue: Option<Venue>,
        start: Option<DateTime<Utc>>,
        end: Option<DateTime<Utc>>,
        client_id: Option<ClientId>,
        params: Option<Py<PyDict>>,
    ) -> PyResult<String> {
        let params_map = Python::attach(|py| -> PyResult<Option<Params>> {
            match params {
                Some(dict) => from_pydict(py, &dict),
                None => Ok(None),
            }
        })?;
        let request_id = DataActor::request_instruments(
            self.inner_mut(),
            venue,
            start,
            end,
            client_id,
            params_map,
        )
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
        params: Option<Py<PyDict>>,
    ) -> PyResult<String> {
        let params_map = Python::attach(|py| -> PyResult<Option<Params>> {
            match params {
                Some(dict) => from_pydict(py, &dict),
                None => Ok(None),
            }
        })?;
        let depth = depth.and_then(NonZeroUsize::new);

        let request_id = DataActor::request_book_snapshot(
            self.inner_mut(),
            instrument_id,
            depth,
            client_id,
            params_map,
        )
        .map_err(to_pyvalue_err)?;
        Ok(request_id.to_string())
    }

    #[pyo3(name = "request_book_deltas")]
    #[pyo3(signature = (instrument_id, start=None, end=None, limit=None, client_id=None, params=None))]
    fn py_request_book_deltas(
        &mut self,
        instrument_id: InstrumentId,
        start: Option<DateTime<Utc>>,
        end: Option<DateTime<Utc>>,
        limit: Option<usize>,
        client_id: Option<ClientId>,
        params: Option<Py<PyDict>>,
    ) -> PyResult<String> {
        let params_map = Python::attach(|py| -> PyResult<Option<Params>> {
            match params {
                Some(dict) => from_pydict(py, &dict),
                None => Ok(None),
            }
        })?;
        let limit = limit.and_then(NonZeroUsize::new);
        let request_id = DataActor::request_book_deltas(
            self.inner_mut(),
            instrument_id,
            start,
            end,
            limit,
            client_id,
            params_map,
        )
        .map_err(to_pyvalue_err)?;
        Ok(request_id.to_string())
    }

    #[pyo3(name = "request_book_depth")]
    #[pyo3(signature = (instrument_id, start=None, end=None, limit=None, depth=None, client_id=None, params=None))]
    #[expect(clippy::too_many_arguments)]
    fn py_request_book_depth(
        &mut self,
        instrument_id: InstrumentId,
        start: Option<DateTime<Utc>>,
        end: Option<DateTime<Utc>>,
        limit: Option<usize>,
        depth: Option<usize>,
        client_id: Option<ClientId>,
        params: Option<Py<PyDict>>,
    ) -> PyResult<String> {
        let params_map = Python::attach(|py| -> PyResult<Option<Params>> {
            match params {
                Some(dict) => from_pydict(py, &dict),
                None => Ok(None),
            }
        })?;
        let limit = limit.and_then(NonZeroUsize::new);
        let depth = depth.and_then(NonZeroUsize::new);
        let request_id = DataActor::request_book_depth(
            self.inner_mut(),
            instrument_id,
            start,
            end,
            limit,
            depth,
            client_id,
            params_map,
        )
        .map_err(to_pyvalue_err)?;
        Ok(request_id.to_string())
    }

    #[pyo3(name = "request_quotes")]
    #[pyo3(signature = (instrument_id, start=None, end=None, limit=None, client_id=None, params=None))]
    fn py_request_quotes(
        &mut self,
        instrument_id: InstrumentId,
        start: Option<DateTime<Utc>>,
        end: Option<DateTime<Utc>>,
        limit: Option<usize>,
        client_id: Option<ClientId>,
        params: Option<Py<PyDict>>,
    ) -> PyResult<String> {
        let params_map = Python::attach(|py| -> PyResult<Option<Params>> {
            match params {
                Some(dict) => from_pydict(py, &dict),
                None => Ok(None),
            }
        })?;
        let limit = limit.and_then(NonZeroUsize::new);
        let request_id = DataActor::request_quotes(
            self.inner_mut(),
            instrument_id,
            start,
            end,
            limit,
            client_id,
            params_map,
        )
        .map_err(to_pyvalue_err)?;
        Ok(request_id.to_string())
    }

    #[pyo3(name = "request_trades")]
    #[pyo3(signature = (instrument_id, start=None, end=None, limit=None, client_id=None, params=None))]
    fn py_request_trades(
        &mut self,
        instrument_id: InstrumentId,
        start: Option<DateTime<Utc>>,
        end: Option<DateTime<Utc>>,
        limit: Option<usize>,
        client_id: Option<ClientId>,
        params: Option<Py<PyDict>>,
    ) -> PyResult<String> {
        let params_map = Python::attach(|py| -> PyResult<Option<Params>> {
            match params {
                Some(dict) => from_pydict(py, &dict),
                None => Ok(None),
            }
        })?;
        let limit = limit.and_then(NonZeroUsize::new);
        let request_id = DataActor::request_trades(
            self.inner_mut(),
            instrument_id,
            start,
            end,
            limit,
            client_id,
            params_map,
        )
        .map_err(to_pyvalue_err)?;
        Ok(request_id.to_string())
    }

    #[pyo3(name = "request_funding_rates")]
    #[pyo3(signature = (instrument_id, start=None, end=None, limit=None, client_id=None, params=None))]
    fn py_request_funding_rates(
        &mut self,
        instrument_id: InstrumentId,
        start: Option<DateTime<Utc>>,
        end: Option<DateTime<Utc>>,
        limit: Option<usize>,
        client_id: Option<ClientId>,
        params: Option<Py<PyDict>>,
    ) -> PyResult<String> {
        let params_map = Python::attach(|py| -> PyResult<Option<Params>> {
            match params {
                Some(dict) => from_pydict(py, &dict),
                None => Ok(None),
            }
        })?;
        let limit = limit.and_then(NonZeroUsize::new);
        let request_id = DataActor::request_funding_rates(
            self.inner_mut(),
            instrument_id,
            start,
            end,
            limit,
            client_id,
            params_map,
        )
        .map_err(to_pyvalue_err)?;
        Ok(request_id.to_string())
    }

    #[pyo3(name = "request_bars")]
    #[pyo3(signature = (bar_type, start=None, end=None, limit=None, client_id=None, params=None))]
    fn py_request_bars(
        &mut self,
        bar_type: BarType,
        start: Option<DateTime<Utc>>,
        end: Option<DateTime<Utc>>,
        limit: Option<usize>,
        client_id: Option<ClientId>,
        params: Option<Py<PyDict>>,
    ) -> PyResult<String> {
        let params_map = Python::attach(|py| -> PyResult<Option<Params>> {
            match params {
                Some(dict) => from_pydict(py, &dict),
                None => Ok(None),
            }
        })?;
        let limit = limit.and_then(NonZeroUsize::new);
        let request_id = DataActor::request_bars(
            self.inner_mut(),
            bar_type,
            start,
            end,
            limit,
            client_id,
            params_map,
        )
        .map_err(to_pyvalue_err)?;
        Ok(request_id.to_string())
    }
}

fn py_order_list_to_orders(py: Python<'_>, order_list: &Py<PyAny>) -> PyResult<Vec<OrderAny>> {
    let order_objects = match order_list.getattr(py, "orders") {
        Ok(orders) => orders.extract::<Vec<Py<PyAny>>>(py)?,
        Err(e) if e.is_instance_of::<pyo3::exceptions::PyAttributeError>(py) => {
            order_list.extract::<Vec<Py<PyAny>>>(py)?
        }
        Err(e) => return Err(e),
    };

    order_objects
        .into_iter()
        .map(|order| pyobject_to_order_any(py, order))
        .collect()
}

#[cfg(test)]
mod tests {
    use std::{
        cell::RefCell,
        collections::{BTreeMap, HashMap},
        rc::Rc,
        str::FromStr,
    };

    use indexmap::IndexMap;
    use nautilus_common::{
        actor::DataActor,
        cache::Cache,
        clock::{Clock, TestClock},
        component::Component,
        messages::{
            data::{BarsResponse, QuotesResponse, TradesResponse},
            execution::TradingCommand,
        },
        msgbus::{
            self, MessagingSwitchboard,
            stubs::{TypedIntoMessageSavingHandler, get_typed_into_message_saving_handler},
        },
        signal::Signal,
        timer::TimeEvent,
    };
    use nautilus_core::{UUID4, UnixNanos};
    use nautilus_model::{
        data::{
            Bar, BarType, CustomData, FundingRateUpdate, IndexPriceUpdate, InstrumentStatus,
            MarkPriceUpdate, OrderBookDelta, OrderBookDeltas, QuoteTick, TradeTick,
            close::InstrumentClose,
            greeks::OptionGreekValues,
            option_chain::{OptionChainSlice, OptionGreeks},
            stubs::stub_custom_data,
        },
        enums::{
            AggressorSide, BookType, GreeksConvention, InstrumentCloseType, MarketStatusAction,
            OrderSide, OrderType, PositionSide, TimeInForce,
        },
        events::{
            OrderAccepted, OrderCancelRejected, OrderCanceled, OrderDenied, OrderEmulated,
            OrderEventAny, OrderExpired, OrderInitialized, OrderModifyRejected, OrderPendingCancel,
            OrderPendingUpdate, OrderRejected, OrderReleased, OrderSubmitted, OrderTriggered,
            OrderUpdated, PositionChanged, PositionClosed, PositionEvent, PositionOpened,
            order::spec::OrderFilledSpec,
        },
        identifiers::{
            AccountId, ClientId, ClientOrderId, InstrumentId, OptionSeriesId, PositionId,
            StrategyId, TradeId, TraderId, Venue,
        },
        instruments::{CurrencyPair, InstrumentAny, stubs::audusd_sim},
        orderbook::OrderBook,
        orders::{Order, OrderTestBuilder},
        python::orders::order_any_to_pyobject,
        types::{Currency, Money, Price, Quantity},
    };
    use nautilus_portfolio::portfolio::Portfolio;
    use pyo3::{
        Bound, Py, PyAny, PyResult, Python,
        ffi::c_str,
        types::{PyAnyMethods, PyBytes, PyDict, PyList},
    };
    use serde_json::Value;
    use ustr::Ustr;

    use super::PyStrategy;
    use crate::strategy::{Strategy, StrategyConfig};

    const TRACKING_STRATEGY_CODE: &std::ffi::CStr = c_str!(
        r#"
class TrackingStrategy:
    TRACKED_METHODS = {
        "on_start",
        "on_stop",
        "on_resume",
        "on_reset",
        "on_dispose",
        "on_degrade",
        "on_fault",
        "on_save",
        "on_load",
        "on_time_event",
        "on_data",
        "on_signal",
        "on_instrument",
        "on_quote",
        "on_trade",
        "on_bar",
        "on_book_deltas",
        "on_book",
        "on_mark_price",
        "on_index_price",
        "on_funding_rate",
        "on_instrument_status",
        "on_instrument_close",
        "on_option_greeks",
        "on_option_chain",
        "on_historical_data",
        "on_historical_quotes",
        "on_historical_trades",
        "on_historical_funding_rates",
        "on_historical_bars",
        "on_historical_mark_prices",
        "on_historical_index_prices",
        "on_market_exit",
        "post_market_exit",
        "on_order_initialized",
        "on_order_event",
        "on_order_denied",
        "on_order_emulated",
        "on_order_released",
        "on_order_submitted",
        "on_order_rejected",
        "on_order_accepted",
        "on_order_expired",
        "on_order_triggered",
        "on_order_pending_update",
        "on_order_pending_cancel",
        "on_order_modify_rejected",
        "on_order_cancel_rejected",
        "on_order_updated",
        "on_order_canceled",
        "on_order_filled",
        "on_position_opened",
        "on_position_event",
        "on_position_changed",
        "on_position_closed",
    }

    def __init__(self):
        self.calls = []

    def _record(self, method_name, *args):
        self.calls.append((method_name, args))

    def was_called(self, method_name):
        return any(call[0] == method_name for call in self.calls)

    def call_count(self, method_name):
        return sum(1 for call in self.calls if call[0] == method_name)

    def call_names(self):
        return [call[0] for call in self.calls]

    def last_loaded_state(self):

        for method_name, args in reversed(self.calls):
            if method_name == "on_load":
                return args[0]
        return None

    def on_save(self):
        self._record("on_save")
        return {"strategy": b"saved"}

    def on_load(self, state):
        self._record("on_load", dict(state))

    def __getattr__(self, name):
        if name in self.TRACKED_METHODS:
            return lambda *args: self._record(name, *args)
        raise AttributeError(name)
"#
    );

    fn create_tracking_python_strategy(py: Python<'_>) -> PyResult<Py<PyAny>> {
        py.run(TRACKING_STRATEGY_CODE, None, None)?;
        let tracking_strategy_class = py.eval(c_str!("TrackingStrategy"), None, None)?;
        let instance = tracking_strategy_class.call0()?;
        Ok(instance.unbind())
    }

    fn python_method_was_called(
        py_strategy: &Py<PyAny>,
        py: Python<'_>,
        method_name: &str,
    ) -> bool {
        py_strategy
            .call_method1(py, "was_called", (method_name,))
            .and_then(|result| result.extract::<bool>(py))
            .unwrap_or(false)
    }

    fn python_method_call_count(py_strategy: &Py<PyAny>, py: Python<'_>, method_name: &str) -> i32 {
        py_strategy
            .call_method1(py, "call_count", (method_name,))
            .and_then(|result| result.extract::<i32>(py))
            .unwrap_or(0)
    }

    fn python_method_call_names(py_strategy: &Py<PyAny>, py: Python<'_>) -> Vec<String> {
        py_strategy
            .call_method0(py, "call_names")
            .and_then(|result| result.extract::<Vec<String>>(py))
            .unwrap_or_default()
    }

    fn python_last_loaded_state(
        py_strategy: &Py<PyAny>,
        py: Python<'_>,
    ) -> Option<HashMap<String, Vec<u8>>> {
        py_strategy
            .call_method0(py, "last_loaded_state")
            .and_then(|result| result.extract::<Option<HashMap<String, Vec<u8>>>>(py))
            .unwrap_or(None)
    }

    const TRACKING_INDICATOR_CODE: &std::ffi::CStr = c_str!(
        r#"
class TrackingIndicator:
    def __init__(self, events=None):
        self.initialized = False
        self.calls = []
        self.events = events

    def handle_quote_tick(self, quote):
        self.calls.append("quote")
        if self.events is not None:
            self.events.append("indicator:quote")

    def handle_trade_tick(self, trade):
        self.calls.append("trade")
        if self.events is not None:
            self.events.append("indicator:trade")

    def handle_bar(self, bar):
        self.calls.append("bar")
        if self.events is not None:
            self.events.append("indicator:bar")

    def call_count(self, name):
        return self.calls.count(name)

class IndicatorEventStrategy:
    def __init__(self, events):
        self.events = events

    def on_start(self):
        pass

    def on_quote(self, quote):
        self.events.append("strategy:quote")

    def on_trade(self, trade):
        self.events.append("strategy:trade")

    def on_bar(self, bar):
        self.events.append("strategy:bar")
"#
    );

    fn create_tracking_python_indicator(py: Python<'_>) -> PyResult<Py<PyAny>> {
        py.run(TRACKING_INDICATOR_CODE, None, None)?;
        let indicator_class = py.eval(c_str!("TrackingIndicator"), None, None)?;
        Ok(indicator_class.call0()?.unbind())
    }

    fn create_event_tracking_python_indicator(
        py: Python<'_>,
        events: &Bound<'_, PyList>,
    ) -> PyResult<Py<PyAny>> {
        py.run(TRACKING_INDICATOR_CODE, None, None)?;
        let indicator_class = py.eval(c_str!("TrackingIndicator"), None, None)?;
        Ok(indicator_class.call1((events,))?.unbind())
    }

    fn create_indicator_event_strategy(
        py: Python<'_>,
        events: &Bound<'_, PyList>,
    ) -> PyResult<Py<PyAny>> {
        py.run(TRACKING_INDICATOR_CODE, None, None)?;
        let strategy_class = py.eval(c_str!("IndicatorEventStrategy"), None, None)?;
        Ok(strategy_class.call1((events,))?.unbind())
    }

    fn python_indicator_call_count(
        indicator: &Py<PyAny>,
        py: Python<'_>,
        method_name: &str,
    ) -> i32 {
        indicator
            .call_method1(py, "call_count", (method_name,))
            .and_then(|result| result.extract::<i32>(py))
            .unwrap_or(0)
    }

    fn sample_instrument() -> CurrencyPair {
        audusd_sim()
    }

    fn sample_time_event() -> TimeEvent {
        TimeEvent::new(
            Ustr::from("test_timer"),
            UUID4::new(),
            UnixNanos::default(),
            UnixNanos::default(),
        )
    }

    fn sample_data() -> CustomData {
        stub_custom_data(1, 42, None, None)
    }

    fn sample_signal() -> Signal {
        Signal::new(
            Ustr::from("test_signal"),
            "1.0".to_string(),
            UnixNanos::default(),
            UnixNanos::default(),
        )
    }

    fn sample_quote() -> QuoteTick {
        let instrument = sample_instrument();
        QuoteTick::new(
            instrument.id,
            Price::from("1.00000"),
            Price::from("1.00001"),
            Quantity::from(100_000),
            Quantity::from(100_000),
            UnixNanos::default(),
            UnixNanos::default(),
        )
    }

    fn sample_trade() -> TradeTick {
        let instrument = sample_instrument();
        TradeTick::new(
            instrument.id,
            Price::from("1.00000"),
            Quantity::from(100_000),
            AggressorSide::Buyer,
            TradeId::new("123456"),
            UnixNanos::default(),
            UnixNanos::default(),
        )
    }

    fn sample_bar() -> Bar {
        let instrument = sample_instrument();
        let bar_type =
            BarType::from_str(&format!("{}-1-MINUTE-LAST-INTERNAL", instrument.id)).unwrap();
        Bar::new(
            bar_type,
            Price::from("1.00000"),
            Price::from("1.00010"),
            Price::from("0.99990"),
            Price::from("1.00005"),
            Quantity::from(100_000),
            UnixNanos::default(),
            UnixNanos::default(),
        )
    }

    fn sample_book() -> OrderBook {
        OrderBook::new(sample_instrument().id, BookType::L2_MBP)
    }

    fn sample_book_deltas() -> OrderBookDeltas {
        let instrument = sample_instrument();
        let delta =
            OrderBookDelta::clear(instrument.id, 0, UnixNanos::default(), UnixNanos::default());
        OrderBookDeltas::new(instrument.id, vec![delta])
    }

    fn sample_mark_price() -> MarkPriceUpdate {
        MarkPriceUpdate::new(
            sample_instrument().id,
            Price::from("1.00000"),
            UnixNanos::default(),
            UnixNanos::default(),
        )
    }

    fn sample_index_price() -> IndexPriceUpdate {
        IndexPriceUpdate::new(
            sample_instrument().id,
            Price::from("1.00000"),
            UnixNanos::default(),
            UnixNanos::default(),
        )
    }

    fn sample_funding_rate() -> FundingRateUpdate {
        FundingRateUpdate::new(
            sample_instrument().id,
            "0.0001".parse().unwrap(),
            None,
            None,
            UnixNanos::default(),
            UnixNanos::default(),
        )
    }

    fn sample_instrument_status() -> InstrumentStatus {
        InstrumentStatus::new(
            sample_instrument().id,
            MarketStatusAction::Trading,
            UnixNanos::default(),
            UnixNanos::default(),
            None,
            None,
            None,
            None,
            None,
        )
    }

    fn sample_instrument_close() -> InstrumentClose {
        InstrumentClose::new(
            sample_instrument().id,
            Price::from("1.00000"),
            InstrumentCloseType::EndOfSession,
            UnixNanos::default(),
            UnixNanos::default(),
        )
    }

    fn sample_option_greeks() -> OptionGreeks {
        OptionGreeks {
            instrument_id: InstrumentId::from("AUD/USD.SIM"),
            convention: GreeksConvention::BlackScholes,
            greeks: OptionGreekValues {
                delta: 0.55,
                gamma: 0.03,
                vega: 0.12,
                theta: -0.05,
                rho: 0.01,
            },
            mark_iv: Some(0.25),
            bid_iv: None,
            ask_iv: None,
            underlying_price: None,
            open_interest: None,
            ts_event: UnixNanos::default(),
            ts_init: UnixNanos::default(),
        }
    }

    fn sample_option_chain() -> OptionChainSlice {
        OptionChainSlice {
            series_id: OptionSeriesId::new(
                Venue::from("SIM"),
                Ustr::from("AUD"),
                Ustr::from("USD"),
                UnixNanos::from(1_711_036_800_000_000_000),
            ),
            atm_strike: None,
            calls: BTreeMap::default(),
            puts: BTreeMap::default(),
            ts_event: UnixNanos::default(),
            ts_init: UnixNanos::default(),
        }
    }

    fn sample_position_opened() -> PositionOpened {
        PositionOpened {
            trader_id: TraderId::from("TRADER-001"),
            strategy_id: StrategyId::from("TEST-001"),
            instrument_id: InstrumentId::from("BTCUSDT.BINANCE"),
            position_id: PositionId::from("P-001"),
            account_id: AccountId::from("ACC-001"),
            opening_order_id: ClientOrderId::from("O-001"),
            entry: OrderSide::Buy,
            side: PositionSide::Long,
            signed_qty: 1.0,
            quantity: Quantity::from(1),
            last_qty: Quantity::from(1),
            last_px: Price::from("1.00000"),
            currency: Currency::from("USD"),
            avg_px_open: 1.0,
            event_id: UUID4::new(),
            ts_event: UnixNanos::default(),
            ts_init: UnixNanos::default(),
        }
    }

    fn sample_position_changed() -> PositionChanged {
        PositionChanged {
            trader_id: TraderId::from("TRADER-001"),
            strategy_id: StrategyId::from("TEST-001"),
            instrument_id: InstrumentId::from("BTCUSDT.BINANCE"),
            position_id: PositionId::from("P-001"),
            account_id: AccountId::from("ACC-001"),
            opening_order_id: ClientOrderId::from("O-001"),
            entry: OrderSide::Buy,
            side: PositionSide::Long,
            signed_qty: 2.0,
            quantity: Quantity::from(2),
            peak_quantity: Quantity::from(2),
            last_qty: Quantity::from(1),
            last_px: Price::from("1.10000"),
            currency: Currency::from("USD"),
            avg_px_open: 1.05,
            avg_px_close: None,
            realized_return: 0.0,
            realized_pnl: None,
            unrealized_pnl: Money::new(0.0, Currency::USD()),
            event_id: UUID4::new(),
            ts_opened: UnixNanos::default(),
            ts_event: UnixNanos::default(),
            ts_init: UnixNanos::default(),
        }
    }

    fn sample_position_closed() -> PositionClosed {
        PositionClosed {
            trader_id: TraderId::from("TRADER-001"),
            strategy_id: StrategyId::from("TEST-001"),
            instrument_id: InstrumentId::from("BTCUSDT.BINANCE"),
            position_id: PositionId::from("P-001"),
            account_id: AccountId::from("ACC-001"),
            opening_order_id: ClientOrderId::from("O-001"),
            closing_order_id: Some(ClientOrderId::from("O-002")),
            entry: OrderSide::Buy,
            side: PositionSide::Flat,
            signed_qty: 0.0,
            quantity: Quantity::from(0),
            peak_quantity: Quantity::from(2),
            last_qty: Quantity::from(2),
            last_px: Price::from("1.20000"),
            currency: Currency::from("USD"),
            avg_px_open: 1.05,
            avg_px_close: Some(1.20),
            realized_return: 0.1,
            realized_pnl: Some(Money::new(0.1, Currency::USD())),
            unrealized_pnl: Money::new(0.0, Currency::USD()),
            duration: 1,
            event_id: UUID4::new(),
            ts_opened: UnixNanos::default(),
            ts_closed: Some(UnixNanos::default()),
            ts_event: UnixNanos::default(),
            ts_init: UnixNanos::default(),
        }
    }

    fn sample_python_market_order(
        py: Python<'_>,
        strategy_id: StrategyId,
        client_order_id: ClientOrderId,
    ) -> PyResult<Py<PyAny>> {
        let order = OrderTestBuilder::new(OrderType::Market)
            .trader_id(TraderId::from("TRADER-001"))
            .strategy_id(strategy_id)
            .instrument_id(sample_instrument().id)
            .client_order_id(client_order_id)
            .quantity(Quantity::from(100_000))
            .build();

        order_any_to_pyobject(py, order)
    }

    fn create_registered_tracking_strategy_with_config(
        py: Python<'_>,
        config: Option<StrategyConfig>,
    ) -> (Py<PyAny>, PyStrategy) {
        let py_strategy = create_tracking_python_strategy(py).unwrap();
        let mut rust_strategy = PyStrategy::new(config);
        rust_strategy.set_python_instance(py_strategy.clone_ref(py));

        let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(TestClock::new()));
        let cache = Rc::new(RefCell::new(Cache::new(None, None)));
        let portfolio = Rc::new(RefCell::new(Portfolio::new(
            cache.clone(),
            clock.clone(),
            None,
        )));

        rust_strategy
            .register(TraderId::from("TRADER-001"), clock, cache, portfolio)
            .unwrap();

        (py_strategy, rust_strategy)
    }

    fn create_registered_tracking_strategy(py: Python<'_>) -> (Py<PyAny>, PyStrategy) {
        create_registered_tracking_strategy_with_config(py, None)
    }

    #[rstest::rstest]
    fn test_external_order_claims_returns_configured_instruments() {
        let claims = vec![
            InstrumentId::from("AUDUSD.SIM"),
            InstrumentId::from("BTCUSDT.BINANCE"),
        ];
        let strategy = PyStrategy::new(Some(StrategyConfig {
            external_order_claims: Some(claims.clone()),
            ..Default::default()
        }));

        assert_eq!(strategy.external_order_claims(), Some(claims));
    }

    #[rstest::rstest]
    fn test_python_aggregate_event_handlers_exist() {
        pyo3::Python::initialize();
        Python::attach(|py| {
            let strategy = Py::new(py, PyStrategy::new(None)).unwrap();
            let strategy = strategy.bind(py);

            assert!(strategy.hasattr("on_order_event").unwrap());
            assert!(strategy.hasattr("on_position_event").unwrap());
        });
    }

    #[rstest::rstest]
    fn test_indicator_registration_exposes_readiness_and_registered_view() {
        pyo3::Python::initialize();
        Python::attach(|py| {
            let mut rust_strategy = PyStrategy::new(None);
            let indicator = create_tracking_python_indicator(py).unwrap();
            let instrument_id = sample_instrument().id;
            let bar_type = sample_bar().bar_type;

            assert_eq!(
                rust_strategy
                    .py_registered_indicators(py)
                    .unwrap()
                    .bind(py)
                    .len()
                    .unwrap(),
                0
            );
            assert!(!rust_strategy.py_indicators_initialized(py).unwrap());

            rust_strategy.py_register_indicator_for_quote_ticks(
                py,
                instrument_id,
                indicator.clone_ref(py),
            );
            rust_strategy.py_register_indicator_for_trade_ticks(
                py,
                instrument_id,
                indicator.clone_ref(py),
            );
            rust_strategy.py_register_indicator_for_bars(py, bar_type, indicator.clone_ref(py));

            let registered = rust_strategy.py_registered_indicators(py).unwrap();
            let registered = registered.bind(py);

            assert_eq!(registered.len().unwrap(), 1);
            assert_eq!(
                registered.get_item(0).unwrap().as_ptr(),
                indicator.bind(py).as_ptr()
            );
            assert!(!rust_strategy.py_indicators_initialized(py).unwrap());

            indicator.bind(py).setattr("initialized", true).unwrap();

            assert!(rust_strategy.py_indicators_initialized(py).unwrap());
        });
    }

    #[rstest::rstest]
    fn test_registered_indicators_receive_quote_trade_and_bar_before_strategy_callbacks() {
        pyo3::Python::initialize();
        Python::attach(|py| {
            let events = PyList::empty(py);
            let py_strategy = create_indicator_event_strategy(py, &events).unwrap();
            let indicator = create_event_tracking_python_indicator(py, &events).unwrap();

            let mut rust_strategy = PyStrategy::new(None);
            rust_strategy.set_python_instance(py_strategy.clone_ref(py));

            let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(TestClock::new()));
            let cache = Rc::new(RefCell::new(Cache::new(None, None)));
            let portfolio = Rc::new(RefCell::new(Portfolio::new(
                cache.clone(),
                clock.clone(),
                None,
            )));

            rust_strategy
                .register(TraderId::from("TRADER-001"), clock, cache, portfolio)
                .unwrap();
            Component::start(rust_strategy.inner_mut()).unwrap();

            let quote = sample_quote();
            let trade = sample_trade();
            let bar = sample_bar();
            let external_bar_type = BarType::from_str(&format!(
                "{}-1-MINUTE-LAST-EXTERNAL",
                bar.bar_type.instrument_id()
            ))
            .unwrap();

            rust_strategy.py_register_indicator_for_quote_ticks(
                py,
                quote.instrument_id,
                indicator.clone_ref(py),
            );
            rust_strategy.py_register_indicator_for_trade_ticks(
                py,
                trade.instrument_id,
                indicator.clone_ref(py),
            );
            rust_strategy.py_register_indicator_for_bars(
                py,
                external_bar_type,
                indicator.clone_ref(py),
            );

            DataActor::handle_quote(rust_strategy.inner_mut(), &quote);
            DataActor::handle_trade(rust_strategy.inner_mut(), &trade);
            DataActor::handle_bar(rust_strategy.inner_mut(), &bar);

            let events = events.extract::<Vec<String>>().unwrap();

            assert_eq!(python_indicator_call_count(&indicator, py, "quote"), 1);
            assert_eq!(python_indicator_call_count(&indicator, py, "trade"), 1);
            assert_eq!(python_indicator_call_count(&indicator, py, "bar"), 1);
            assert_eq!(
                events,
                vec![
                    "indicator:quote",
                    "strategy:quote",
                    "indicator:trade",
                    "strategy:trade",
                    "indicator:bar",
                    "strategy:bar",
                ]
            );
        });
    }

    #[rstest::rstest]
    fn test_registered_indicators_receive_historical_quote_trade_and_bar_batches() {
        pyo3::Python::initialize();
        Python::attach(|py| {
            let mut rust_strategy = PyStrategy::new(None);
            let indicator = create_tracking_python_indicator(py).unwrap();
            let quote = sample_quote();
            let trade = sample_trade();
            let bar = sample_bar();
            let quotes = vec![quote];
            let trades = vec![trade];
            let bars = vec![bar];

            rust_strategy.py_register_indicator_for_quote_ticks(
                py,
                quote.instrument_id,
                indicator.clone_ref(py),
            );
            rust_strategy.py_register_indicator_for_trade_ticks(
                py,
                trade.instrument_id,
                indicator.clone_ref(py),
            );
            rust_strategy.py_register_indicator_for_bars(py, bar.bar_type, indicator.clone_ref(py));

            let client_id = ClientId::new("TEST");
            let quotes_response = QuotesResponse::new(
                UUID4::new(),
                client_id,
                quote.instrument_id,
                quotes,
                None,
                None,
                UnixNanos::default(),
                None,
            );
            let trades_response = TradesResponse::new(
                UUID4::new(),
                client_id,
                trade.instrument_id,
                trades,
                None,
                None,
                UnixNanos::default(),
                None,
            );
            let bars_response = BarsResponse::new(
                UUID4::new(),
                client_id,
                bar.bar_type,
                bars,
                None,
                None,
                UnixNanos::default(),
                None,
            );

            DataActor::handle_quotes_response(rust_strategy.inner_mut(), &quotes_response);
            DataActor::handle_trades_response(rust_strategy.inner_mut(), &trades_response);
            DataActor::handle_bars_response(rust_strategy.inner_mut(), &bars_response);

            assert_eq!(python_indicator_call_count(&indicator, py, "quote"), 1);
            assert_eq!(python_indicator_call_count(&indicator, py, "trade"), 1);
            assert_eq!(python_indicator_call_count(&indicator, py, "bar"), 1);
        });
    }

    fn assert_python_dispatch<F>(py: Python<'_>, method_name: &str, invoke: F)
    where
        F: FnOnce(&mut PyStrategy) -> anyhow::Result<()>,
    {
        let (py_strategy, mut rust_strategy) = create_registered_tracking_strategy(py);
        let result = invoke(&mut rust_strategy);

        assert!(result.is_ok());
        assert!(python_method_was_called(&py_strategy, py, method_name));
        assert_eq!(python_method_call_count(&py_strategy, py, method_name), 1);
    }

    #[rstest::rstest]
    #[case("on_start")]
    #[case("on_stop")]
    #[case("on_resume")]
    #[case("on_reset")]
    #[case("on_dispose")]
    #[case("on_degrade")]
    #[case("on_fault")]
    fn test_python_dispatch_lifecycle_matrix(#[case] method_name: &str) {
        pyo3::Python::initialize();
        Python::attach(|py| {
            assert_python_dispatch(py, method_name, |rust_strategy| match method_name {
                "on_start" => DataActor::on_start(rust_strategy.inner_mut()),
                "on_stop" => DataActor::on_stop(rust_strategy.inner_mut()),
                "on_resume" => DataActor::on_resume(rust_strategy.inner_mut()),
                "on_reset" => DataActor::on_reset(rust_strategy.inner_mut()),
                "on_dispose" => DataActor::on_dispose(rust_strategy.inner_mut()),
                "on_degrade" => DataActor::on_degrade(rust_strategy.inner_mut()),
                "on_fault" => DataActor::on_fault(rust_strategy.inner_mut()),
                _ => unreachable!("unhandled lifecycle case: {method_name}"),
            });
        });
    }

    #[rstest::rstest]
    #[case("on_save")]
    #[case("on_load")]
    fn test_python_dispatch_persistence_matrix(#[case] method_name: &str) {
        pyo3::Python::initialize();
        Python::attach(|py| {
            assert_python_dispatch(py, method_name, |rust_strategy| match method_name {
                "on_save" => {
                    let state = DataActor::on_save(rust_strategy.inner()).unwrap();
                    assert_eq!(
                        state.get("strategy").map(Vec::as_slice),
                        Some(b"saved".as_slice())
                    );
                    Ok(())
                }
                "on_load" => {
                    let mut state = IndexMap::new();
                    state.insert("strategy".to_string(), b"loaded".to_vec());
                    DataActor::on_load(rust_strategy.inner_mut(), state)
                }
                _ => unreachable!("unhandled persistence case: {method_name}"),
            });
        });
    }

    #[rstest::rstest]
    fn test_python_persistence_methods_convert_state() {
        pyo3::Python::initialize();
        Python::attach(|py| {
            let (py_strategy, mut rust_strategy) = create_registered_tracking_strategy(py);

            let saved = rust_strategy.py_save(py).unwrap();
            let saved_state = saved
                .bind(py)
                .extract::<HashMap<String, Vec<u8>>>()
                .unwrap();
            assert_eq!(
                saved_state.get("strategy").map(Vec::as_slice),
                Some(&b"saved"[..])
            );

            let load_state = PyDict::new(py);
            load_state
                .set_item("strategy", PyBytes::new(py, b"loaded-from-python"))
                .unwrap();

            rust_strategy.py_load(&load_state).unwrap();

            let loaded_state = python_last_loaded_state(&py_strategy, py).unwrap();
            assert_eq!(
                loaded_state.get("strategy").map(Vec::as_slice),
                Some(&b"loaded-from-python"[..])
            );
        });
    }

    #[rstest::rstest]
    fn test_python_stop_stops_immediately_when_manage_stop_disabled() {
        pyo3::Python::initialize();
        Python::attach(|py| {
            let config = StrategyConfig {
                strategy_id: Some(StrategyId::from("TEST-001")),
                order_id_tag: Some("001".to_string()),
                manage_stop: false,
                ..Default::default()
            };
            let (py_strategy, mut rust_strategy) =
                create_registered_tracking_strategy_with_config(py, Some(config));

            rust_strategy.py_start().unwrap();
            rust_strategy.py_stop().unwrap();

            assert!(rust_strategy.py_is_stopped());
            assert!(!rust_strategy.inner().core.pending_stop);
            assert!(!rust_strategy.inner().core.is_exiting);
            assert_eq!(python_method_call_count(&py_strategy, py, "on_stop"), 1);
        });
    }

    #[rstest::rstest]
    fn test_python_stop_defers_when_manage_stop_enabled() {
        pyo3::Python::initialize();
        Python::attach(|py| {
            let config = StrategyConfig {
                strategy_id: Some(StrategyId::from("TEST-001")),
                order_id_tag: Some("001".to_string()),
                manage_stop: true,
                ..Default::default()
            };
            let (py_strategy, mut rust_strategy) =
                create_registered_tracking_strategy_with_config(py, Some(config));

            rust_strategy.py_start().unwrap();
            rust_strategy.py_stop().unwrap();

            assert!(rust_strategy.py_is_running());
            assert!(rust_strategy.inner().core.pending_stop);
            assert!(rust_strategy.inner().core.is_exiting);
            assert_eq!(python_method_call_count(&py_strategy, py, "on_stop"), 0);
        });
    }

    #[rstest::rstest]
    fn test_python_market_exit_methods_update_state_and_dispatch_hooks() {
        pyo3::Python::initialize();
        Python::attach(|py| {
            let (py_strategy, mut rust_strategy) = create_registered_tracking_strategy(py);

            rust_strategy.py_start().unwrap();

            assert!(!rust_strategy.py_is_exiting());

            rust_strategy.py_market_exit().unwrap();

            assert!(rust_strategy.py_is_exiting());
            assert_eq!(
                python_method_call_count(&py_strategy, py, "on_market_exit"),
                1
            );

            rust_strategy.inner_mut().finalize_market_exit();

            assert!(!rust_strategy.py_is_exiting());
            assert_eq!(
                python_method_call_count(&py_strategy, py, "post_market_exit"),
                1
            );
        });
    }

    #[rstest::rstest]
    #[case::order_list_object(true)]
    #[case::raw_order_sequence(false)]
    fn test_python_submit_order_list_accepts_order_list_inputs(#[case] wrap_order_list: bool) {
        pyo3::Python::initialize();
        Python::attach(|py| {
            let (_, mut rust_strategy) = create_registered_tracking_strategy(py);
            let (risk_handler, risk_messages): (_, TypedIntoMessageSavingHandler<TradingCommand>) =
                get_typed_into_message_saving_handler(Some(Ustr::from("RiskEngine.queue_execute")));
            msgbus::register_trading_command_endpoint(
                MessagingSwitchboard::risk_engine_queue_execute(),
                risk_handler,
            );

            let strategy_id = rust_strategy.strategy_id();
            let client_order_id1 = ClientOrderId::from("O-PYO3-LIST-001");
            let client_order_id2 = ClientOrderId::from("O-PYO3-LIST-002");
            let orders = vec![
                sample_python_market_order(py, strategy_id, client_order_id1).unwrap(),
                sample_python_market_order(py, strategy_id, client_order_id2).unwrap(),
            ];
            let params = PyDict::new(py);

            params.set_item("routing_hint", "prefer_batch").unwrap();
            let order_list = if wrap_order_list {
                let order_list_type = py
                    .eval(c_str!("type('OrderListShim', (), {})"), None, None)
                    .unwrap();
                let order_list = order_list_type.call0().unwrap();

                order_list.setattr("orders", orders).unwrap();
                order_list.unbind()
            } else {
                PyList::new(py, orders).unwrap().into_any().unbind()
            };

            rust_strategy
                .py_submit_order_list(py, order_list, None, None, Some(params.unbind()))
                .unwrap();

            let cache = rust_strategy.inner().core.cache();
            let cached_order1 = cache.order(&client_order_id1).unwrap();
            let cached_order2 = cache.order(&client_order_id2).unwrap();
            let order_list_id = cached_order1.order_list_id().unwrap();
            let order_list = cache.order_list(&order_list_id).unwrap();

            assert_eq!(cached_order2.order_list_id(), Some(order_list_id));
            assert_eq!(
                order_list.client_order_ids.as_slice(),
                &[client_order_id1, client_order_id2]
            );

            let risk_messages = risk_messages.get_messages();
            assert_eq!(risk_messages.len(), 1);
            let Some(TradingCommand::SubmitOrderList(command)) = risk_messages.first() else {
                panic!("expected SubmitOrderList command");
            };
            assert_eq!(
                command
                    .params
                    .as_ref()
                    .and_then(|params| params.get("routing_hint")),
                Some(&Value::String("prefer_batch".to_string()))
            );
        });
    }

    #[rstest::rstest]
    fn test_python_cancel_gtd_expiry_accepts_order() {
        pyo3::Python::initialize();
        Python::attach(|py| {
            let (_, mut rust_strategy) = create_registered_tracking_strategy(py);
            let strategy_id = rust_strategy.strategy_id();
            let client_order_id = ClientOrderId::from("O-PYO3-GTD-001");
            let timer_name = format!("GTD-EXPIRY:{client_order_id}");
            let order = OrderTestBuilder::new(OrderType::Limit)
                .trader_id(TraderId::from("TRADER-001"))
                .strategy_id(strategy_id)
                .instrument_id(sample_instrument().id)
                .client_order_id(client_order_id)
                .quantity(Quantity::from(100_000))
                .price(Price::from("1.00000"))
                .time_in_force(TimeInForce::Gtd)
                .expire_time(UnixNanos::from(1))
                .build();
            let py_order = order_any_to_pyobject(py, order).unwrap();

            {
                let mut clock = rust_strategy.inner_mut().core.clock();
                clock
                    .set_time_alert_ns(&timer_name, UnixNanos::from(1), None, None)
                    .unwrap();
            }
            rust_strategy
                .inner_mut()
                .core
                .gtd_timers
                .insert(client_order_id, Ustr::from(&timer_name));

            rust_strategy
                .py_cancel_gtd_expiry(py, py_order)
                .expect("cancel_gtd_expiry should accept Python order");

            let clock_timer_exists = rust_strategy
                .inner_mut()
                .core
                .clock()
                .timer_names()
                .contains(&timer_name.as_str());

            assert!(
                !rust_strategy
                    .inner_mut()
                    .has_gtd_expiry_timer(&client_order_id)
            );
            assert!(!clock_timer_exists);
        });
    }

    #[rstest::rstest]
    #[case("on_time_event")]
    #[case("on_data")]
    #[case("on_signal")]
    #[case("on_instrument")]
    #[case("on_quote")]
    #[case("on_trade")]
    #[case("on_bar")]
    #[case("on_book_deltas")]
    #[case("on_book")]
    #[case("on_mark_price")]
    #[case("on_index_price")]
    #[case("on_funding_rate")]
    #[case("on_instrument_status")]
    #[case("on_instrument_close")]
    #[case("on_option_greeks")]
    #[case("on_option_chain")]
    #[case("on_historical_data")]
    #[case("on_historical_quotes")]
    #[case("on_historical_trades")]
    #[case("on_historical_funding_rates")]
    #[case("on_historical_bars")]
    #[case("on_historical_mark_prices")]
    #[case("on_historical_index_prices")]
    fn test_python_dispatch_data_callback_matrix(#[case] method_name: &str) {
        pyo3::Python::initialize();
        Python::attach(|py| {
            assert_python_dispatch(py, method_name, |rust_strategy| match method_name {
                "on_time_event" => {
                    let event = sample_time_event();
                    DataActor::on_time_event(rust_strategy.inner_mut(), &event)
                }
                "on_data" => {
                    let data = sample_data();
                    rust_strategy.inner_mut().on_data(&data)
                }
                "on_signal" => {
                    let signal = sample_signal();
                    rust_strategy.inner_mut().on_signal(&signal)
                }
                "on_instrument" => {
                    let instrument = InstrumentAny::CurrencyPair(sample_instrument());
                    rust_strategy.inner_mut().on_instrument(&instrument)
                }
                "on_quote" => {
                    let quote = sample_quote();
                    rust_strategy.inner_mut().on_quote(&quote)
                }
                "on_trade" => {
                    let trade = sample_trade();
                    rust_strategy.inner_mut().on_trade(&trade)
                }
                "on_bar" => {
                    let bar = sample_bar();
                    rust_strategy.inner_mut().on_bar(&bar)
                }
                "on_book_deltas" => {
                    let deltas = sample_book_deltas();
                    rust_strategy.inner_mut().on_book_deltas(&deltas)
                }
                "on_book" => {
                    let book = sample_book();
                    rust_strategy.inner_mut().on_book(&book)
                }
                "on_mark_price" => {
                    let mark_price = sample_mark_price();
                    rust_strategy.inner_mut().on_mark_price(&mark_price)
                }
                "on_index_price" => {
                    let index_price = sample_index_price();
                    rust_strategy.inner_mut().on_index_price(&index_price)
                }
                "on_funding_rate" => {
                    let funding_rate = sample_funding_rate();
                    rust_strategy.inner_mut().on_funding_rate(&funding_rate)
                }
                "on_instrument_status" => {
                    let status = sample_instrument_status();
                    rust_strategy.inner_mut().on_instrument_status(&status)
                }
                "on_instrument_close" => {
                    let close = sample_instrument_close();
                    rust_strategy.inner_mut().on_instrument_close(&close)
                }
                "on_option_greeks" => {
                    let greeks = sample_option_greeks();
                    DataActor::on_option_greeks(rust_strategy.inner_mut(), &greeks)
                }
                "on_option_chain" => {
                    let slice = sample_option_chain();
                    DataActor::on_option_chain(rust_strategy.inner_mut(), &slice)
                }
                "on_historical_data" => {
                    let data = sample_data();
                    rust_strategy.inner_mut().on_historical_data(&data)
                }
                "on_historical_quotes" => {
                    let quotes = vec![sample_quote()];
                    rust_strategy.inner_mut().on_historical_quotes(&quotes)
                }
                "on_historical_trades" => {
                    let trades = vec![sample_trade()];
                    rust_strategy.inner_mut().on_historical_trades(&trades)
                }
                "on_historical_funding_rates" => {
                    let funding_rates = vec![sample_funding_rate()];
                    rust_strategy
                        .inner_mut()
                        .on_historical_funding_rates(&funding_rates)
                }
                "on_historical_bars" => {
                    let bars = vec![sample_bar()];
                    rust_strategy.inner_mut().on_historical_bars(&bars)
                }
                "on_historical_mark_prices" => {
                    let mark_prices = vec![sample_mark_price()];
                    rust_strategy
                        .inner_mut()
                        .on_historical_mark_prices(&mark_prices)
                }
                "on_historical_index_prices" => {
                    let index_prices = vec![sample_index_price()];
                    rust_strategy
                        .inner_mut()
                        .on_historical_index_prices(&index_prices)
                }
                _ => unreachable!("unhandled data callback case: {method_name}"),
            });
        });
    }

    #[rstest::rstest]
    #[case("on_order_initialized")]
    #[case("on_order_event")]
    #[case("on_order_denied")]
    #[case("on_order_emulated")]
    #[case("on_order_released")]
    #[case("on_order_submitted")]
    #[case("on_order_rejected")]
    #[case("on_order_accepted")]
    #[case("on_order_expired")]
    #[case("on_order_triggered")]
    #[case("on_order_pending_update")]
    #[case("on_order_pending_cancel")]
    #[case("on_order_modify_rejected")]
    #[case("on_order_cancel_rejected")]
    #[case("on_order_updated")]
    #[case("on_order_canceled")]
    #[case("on_order_filled")]
    fn test_python_dispatch_order_callback_matrix(#[case] method_name: &str) {
        pyo3::Python::initialize();
        Python::attach(|py| {
            assert_python_dispatch(py, method_name, |rust_strategy| match method_name {
                "on_order_initialized" => {
                    Strategy::on_order_initialized(
                        rust_strategy.inner_mut(),
                        OrderInitialized::default(),
                    );
                    Ok(())
                }
                "on_order_event" => {
                    Strategy::on_order_event(
                        rust_strategy.inner_mut(),
                        OrderEventAny::Accepted(OrderAccepted::default()),
                    );
                    Ok(())
                }
                "on_order_denied" => {
                    Strategy::on_order_denied(rust_strategy.inner_mut(), OrderDenied::default());
                    Ok(())
                }
                "on_order_emulated" => {
                    Strategy::on_order_emulated(
                        rust_strategy.inner_mut(),
                        OrderEmulated::default(),
                    );
                    Ok(())
                }
                "on_order_released" => {
                    Strategy::on_order_released(
                        rust_strategy.inner_mut(),
                        OrderReleased::default(),
                    );
                    Ok(())
                }
                "on_order_submitted" => {
                    Strategy::on_order_submitted(
                        rust_strategy.inner_mut(),
                        OrderSubmitted::default(),
                    );
                    Ok(())
                }
                "on_order_rejected" => {
                    Strategy::on_order_rejected(
                        rust_strategy.inner_mut(),
                        OrderRejected::default(),
                    );
                    Ok(())
                }
                "on_order_accepted" => {
                    Strategy::on_order_accepted(
                        rust_strategy.inner_mut(),
                        OrderAccepted::default(),
                    );
                    Ok(())
                }
                "on_order_expired" => {
                    Strategy::on_order_expired(rust_strategy.inner_mut(), OrderExpired::default());
                    Ok(())
                }
                "on_order_triggered" => {
                    Strategy::on_order_triggered(
                        rust_strategy.inner_mut(),
                        OrderTriggered::default(),
                    );
                    Ok(())
                }
                "on_order_pending_update" => {
                    Strategy::on_order_pending_update(
                        rust_strategy.inner_mut(),
                        OrderPendingUpdate::default(),
                    );
                    Ok(())
                }
                "on_order_pending_cancel" => {
                    Strategy::on_order_pending_cancel(
                        rust_strategy.inner_mut(),
                        OrderPendingCancel::default(),
                    );
                    Ok(())
                }
                "on_order_modify_rejected" => {
                    Strategy::on_order_modify_rejected(
                        rust_strategy.inner_mut(),
                        OrderModifyRejected::default(),
                    );
                    Ok(())
                }
                "on_order_cancel_rejected" => {
                    Strategy::on_order_cancel_rejected(
                        rust_strategy.inner_mut(),
                        OrderCancelRejected::default(),
                    );
                    Ok(())
                }
                "on_order_updated" => {
                    Strategy::on_order_updated(rust_strategy.inner_mut(), OrderUpdated::default());
                    Ok(())
                }
                "on_order_canceled" => {
                    let event = OrderCanceled::default();
                    DataActor::on_order_canceled(rust_strategy.inner_mut(), &event)
                }
                "on_order_filled" => {
                    let event = OrderFilledSpec::builder().build();
                    DataActor::on_order_filled(rust_strategy.inner_mut(), &event)
                }
                _ => unreachable!("unhandled order callback case: {method_name}"),
            });
        });
    }

    #[rstest::rstest]
    fn test_python_handle_order_event_dispatches_specific_and_aggregate_callbacks() {
        pyo3::Python::initialize();
        Python::attach(|py| {
            let (py_strategy, mut rust_strategy) = create_registered_tracking_strategy(py);

            rust_strategy.py_start().unwrap();
            Strategy::handle_order_event(
                rust_strategy.inner_mut(),
                OrderEventAny::Accepted(OrderAccepted::default()),
            );

            assert_eq!(
                python_method_call_count(&py_strategy, py, "on_order_accepted"),
                1
            );
            assert_eq!(
                python_method_call_count(&py_strategy, py, "on_order_event"),
                1
            );
            let call_names = python_method_call_names(&py_strategy, py);
            assert_eq!(
                &call_names[call_names.len() - 2..],
                ["on_order_accepted", "on_order_event"],
            );
        });
    }

    #[rstest::rstest]
    #[case("on_position_event")]
    #[case("on_position_opened")]
    #[case("on_position_changed")]
    #[case("on_position_closed")]
    fn test_python_dispatch_position_callback_matrix(#[case] method_name: &str) {
        pyo3::Python::initialize();
        Python::attach(|py| {
            assert_python_dispatch(py, method_name, |rust_strategy| match method_name {
                "on_position_event" => {
                    Strategy::on_position_event(
                        rust_strategy.inner_mut(),
                        PositionEvent::PositionOpened(sample_position_opened()),
                    );
                    Ok(())
                }
                "on_position_opened" => {
                    Strategy::on_position_opened(
                        rust_strategy.inner_mut(),
                        sample_position_opened(),
                    );
                    Ok(())
                }
                "on_position_changed" => {
                    Strategy::on_position_changed(
                        rust_strategy.inner_mut(),
                        sample_position_changed(),
                    );
                    Ok(())
                }
                "on_position_closed" => {
                    Strategy::on_position_closed(
                        rust_strategy.inner_mut(),
                        sample_position_closed(),
                    );
                    Ok(())
                }
                _ => unreachable!("unhandled position callback case: {method_name}"),
            });
        });
    }

    #[rstest::rstest]
    fn test_python_handle_position_event_dispatches_specific_and_aggregate_callbacks() {
        pyo3::Python::initialize();
        Python::attach(|py| {
            let (py_strategy, mut rust_strategy) = create_registered_tracking_strategy(py);

            rust_strategy.py_start().unwrap();
            Strategy::handle_position_event(
                rust_strategy.inner_mut(),
                PositionEvent::PositionOpened(sample_position_opened()),
            );

            assert_eq!(
                python_method_call_count(&py_strategy, py, "on_position_opened"),
                1
            );
            assert_eq!(
                python_method_call_count(&py_strategy, py, "on_position_event"),
                1
            );
            let call_names = python_method_call_names(&py_strategy, py);
            assert_eq!(
                &call_names[call_names.len() - 2..],
                ["on_position_opened", "on_position_event"],
            );
        });
    }
}
