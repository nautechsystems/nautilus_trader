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

//! Python bindings for the [`OrderFactory`].

use std::{cell::RefCell, rc::Rc};

use indexmap::IndexMap;
use nautilus_core::{UnixNanos, python::to_pyvalue_err};
use nautilus_model::{
    enums::{ContingencyType, OrderSide, OrderType, TimeInForce, TrailingOffsetType, TriggerType},
    identifiers::{
        ClientOrderId, ExecAlgorithmId, InstrumentId, OrderListId, StrategyId, TraderId,
    },
    orders::{OrderAny, str_indexmap_to_ustr},
    python::orders::order_any_to_pyobject,
    types::{Price, Quantity},
};
use pyo3::{prelude::*, types::PyList};
use rust_decimal::Decimal;
use ustr::Ustr;

use crate::{factories::OrderFactory, python::clock::PyClock};

/// Wrapper providing shared access to [`OrderFactory`] from Python.
#[allow(non_camel_case_types)]
#[pyo3::pyclass(
    module = "nautilus_trader.core.nautilus_pyo3.common",
    name = "OrderFactory",
    unsendable,
    from_py_object
)]
#[pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.common")]
#[derive(Debug, Clone)]
pub struct PyOrderFactory(Rc<RefCell<OrderFactory>>);

impl PyOrderFactory {
    /// Creates a `PyOrderFactory` from an `Rc<RefCell<OrderFactory>>`.
    #[must_use]
    pub fn from_rc(rc: Rc<RefCell<OrderFactory>>) -> Self {
        Self(rc)
    }

    /// Gets the inner `Rc<RefCell<OrderFactory>>` for use in Rust code.
    #[must_use]
    pub fn order_factory_rc(&self) -> Rc<RefCell<OrderFactory>> {
        self.0.clone()
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
#[expect(
    clippy::too_many_arguments,
    reason = "factory methods mirror the Python order factory API"
)]
impl PyOrderFactory {
    #[new]
    #[pyo3(signature = (
        trader_id,
        strategy_id,
        clock,
        use_uuid_client_order_ids=false,
        use_hyphens_in_client_order_ids=true
    ))]
    #[allow(
        clippy::needless_pass_by_value,
        reason = "PyO3 extracts pyclass constructor arguments by value"
    )]
    fn py_new(
        trader_id: TraderId,
        strategy_id: StrategyId,
        clock: PyClock,
        use_uuid_client_order_ids: bool,
        use_hyphens_in_client_order_ids: bool,
    ) -> Self {
        Self(Rc::new(RefCell::new(OrderFactory::new(
            trader_id,
            strategy_id,
            None,
            None,
            clock.clock_rc(),
            use_uuid_client_order_ids,
            use_hyphens_in_client_order_ids,
        ))))
    }

    #[pyo3(name = "get_client_order_id_count")]
    fn py_get_client_order_id_count(&self) -> usize {
        self.0.borrow().client_order_id_count()
    }

    #[pyo3(name = "get_order_list_id_count")]
    fn py_get_order_list_id_count(&self) -> usize {
        self.0.borrow().order_list_id_count()
    }

    #[pyo3(name = "generate_client_order_id")]
    fn py_generate_client_order_id(&mut self) -> ClientOrderId {
        self.0.borrow_mut().generate_client_order_id()
    }

    #[pyo3(name = "generate_order_list_id")]
    fn py_generate_order_list_id(&mut self) -> OrderListId {
        self.0.borrow_mut().generate_order_list_id()
    }

    #[pyo3(name = "reset")]
    fn py_reset(&mut self) {
        self.0.borrow_mut().reset_factory();
    }

    #[pyo3(name = "market", signature = (
        instrument_id,
        order_side,
        quantity,
        time_in_force=None,
        reduce_only=None,
        quote_quantity=None,
        exec_algorithm_id=None,
        exec_algorithm_params=None,
        tags=None,
        client_order_id=None
    ))]
    fn py_market(
        &mut self,
        py: Python<'_>,
        instrument_id: InstrumentId,
        order_side: OrderSide,
        quantity: Quantity,
        time_in_force: Option<TimeInForce>,
        reduce_only: Option<bool>,
        quote_quantity: Option<bool>,
        exec_algorithm_id: Option<ExecAlgorithmId>,
        exec_algorithm_params: Option<IndexMap<String, String>>,
        tags: Option<Vec<String>>,
        client_order_id: Option<ClientOrderId>,
    ) -> PyResult<Py<PyAny>> {
        py_order_from_factory(py, || {
            self.0.borrow_mut().try_market(
                instrument_id,
                order_side,
                quantity,
                time_in_force,
                reduce_only,
                quote_quantity,
                exec_algorithm_id,
                exec_algorithm_params.map(str_indexmap_to_ustr),
                tags.map(strings_to_ustrs),
                client_order_id,
            )
        })
    }

    #[pyo3(name = "limit", signature = (
        instrument_id,
        order_side,
        quantity,
        price,
        time_in_force=None,
        expire_time=None,
        post_only=None,
        reduce_only=None,
        quote_quantity=None,
        display_qty=None,
        emulation_trigger=None,
        trigger_instrument_id=None,
        exec_algorithm_id=None,
        exec_algorithm_params=None,
        tags=None,
        client_order_id=None
    ))]
    fn py_limit(
        &mut self,
        py: Python<'_>,
        instrument_id: InstrumentId,
        order_side: OrderSide,
        quantity: Quantity,
        price: Price,
        time_in_force: Option<TimeInForce>,
        expire_time: Option<u64>,
        post_only: Option<bool>,
        reduce_only: Option<bool>,
        quote_quantity: Option<bool>,
        display_qty: Option<Quantity>,
        emulation_trigger: Option<TriggerType>,
        trigger_instrument_id: Option<InstrumentId>,
        exec_algorithm_id: Option<ExecAlgorithmId>,
        exec_algorithm_params: Option<IndexMap<String, String>>,
        tags: Option<Vec<String>>,
        client_order_id: Option<ClientOrderId>,
    ) -> PyResult<Py<PyAny>> {
        py_order_from_factory(py, || {
            self.0.borrow_mut().try_limit(
                instrument_id,
                order_side,
                quantity,
                price,
                time_in_force,
                expire_time.map(UnixNanos::from),
                post_only,
                reduce_only,
                quote_quantity,
                display_qty,
                emulation_trigger,
                trigger_instrument_id,
                exec_algorithm_id,
                exec_algorithm_params.map(str_indexmap_to_ustr),
                tags.map(strings_to_ustrs),
                client_order_id,
            )
        })
    }

    #[pyo3(name = "stop_market", signature = (
        instrument_id,
        order_side,
        quantity,
        trigger_price,
        trigger_type=None,
        time_in_force=None,
        expire_time=None,
        reduce_only=None,
        quote_quantity=None,
        display_qty=None,
        emulation_trigger=None,
        trigger_instrument_id=None,
        exec_algorithm_id=None,
        exec_algorithm_params=None,
        tags=None,
        client_order_id=None
    ))]
    fn py_stop_market(
        &mut self,
        py: Python<'_>,
        instrument_id: InstrumentId,
        order_side: OrderSide,
        quantity: Quantity,
        trigger_price: Price,
        trigger_type: Option<TriggerType>,
        time_in_force: Option<TimeInForce>,
        expire_time: Option<u64>,
        reduce_only: Option<bool>,
        quote_quantity: Option<bool>,
        display_qty: Option<Quantity>,
        emulation_trigger: Option<TriggerType>,
        trigger_instrument_id: Option<InstrumentId>,
        exec_algorithm_id: Option<ExecAlgorithmId>,
        exec_algorithm_params: Option<IndexMap<String, String>>,
        tags: Option<Vec<String>>,
        client_order_id: Option<ClientOrderId>,
    ) -> PyResult<Py<PyAny>> {
        py_order_from_factory(py, || {
            self.0.borrow_mut().try_stop_market(
                instrument_id,
                order_side,
                quantity,
                trigger_price,
                trigger_type,
                time_in_force,
                expire_time.map(UnixNanos::from),
                reduce_only,
                quote_quantity,
                display_qty,
                emulation_trigger,
                trigger_instrument_id,
                exec_algorithm_id,
                exec_algorithm_params.map(str_indexmap_to_ustr),
                tags.map(strings_to_ustrs),
                client_order_id,
            )
        })
    }

    #[pyo3(name = "stop_limit", signature = (
        instrument_id,
        order_side,
        quantity,
        price,
        trigger_price,
        trigger_type=None,
        time_in_force=None,
        expire_time=None,
        post_only=None,
        reduce_only=None,
        quote_quantity=None,
        display_qty=None,
        emulation_trigger=None,
        trigger_instrument_id=None,
        exec_algorithm_id=None,
        exec_algorithm_params=None,
        tags=None,
        client_order_id=None
    ))]
    fn py_stop_limit(
        &mut self,
        py: Python<'_>,
        instrument_id: InstrumentId,
        order_side: OrderSide,
        quantity: Quantity,
        price: Price,
        trigger_price: Price,
        trigger_type: Option<TriggerType>,
        time_in_force: Option<TimeInForce>,
        expire_time: Option<u64>,
        post_only: Option<bool>,
        reduce_only: Option<bool>,
        quote_quantity: Option<bool>,
        display_qty: Option<Quantity>,
        emulation_trigger: Option<TriggerType>,
        trigger_instrument_id: Option<InstrumentId>,
        exec_algorithm_id: Option<ExecAlgorithmId>,
        exec_algorithm_params: Option<IndexMap<String, String>>,
        tags: Option<Vec<String>>,
        client_order_id: Option<ClientOrderId>,
    ) -> PyResult<Py<PyAny>> {
        py_order_from_factory(py, || {
            self.0.borrow_mut().try_stop_limit(
                instrument_id,
                order_side,
                quantity,
                price,
                trigger_price,
                trigger_type,
                time_in_force,
                expire_time.map(UnixNanos::from),
                post_only,
                reduce_only,
                quote_quantity,
                display_qty,
                emulation_trigger,
                trigger_instrument_id,
                exec_algorithm_id,
                exec_algorithm_params.map(str_indexmap_to_ustr),
                tags.map(strings_to_ustrs),
                client_order_id,
            )
        })
    }

    #[pyo3(name = "market_to_limit", signature = (
        instrument_id,
        order_side,
        quantity,
        time_in_force=None,
        expire_time=None,
        reduce_only=None,
        quote_quantity=None,
        display_qty=None,
        exec_algorithm_id=None,
        exec_algorithm_params=None,
        tags=None,
        client_order_id=None
    ))]
    fn py_market_to_limit(
        &mut self,
        py: Python<'_>,
        instrument_id: InstrumentId,
        order_side: OrderSide,
        quantity: Quantity,
        time_in_force: Option<TimeInForce>,
        expire_time: Option<u64>,
        reduce_only: Option<bool>,
        quote_quantity: Option<bool>,
        display_qty: Option<Quantity>,
        exec_algorithm_id: Option<ExecAlgorithmId>,
        exec_algorithm_params: Option<IndexMap<String, String>>,
        tags: Option<Vec<String>>,
        client_order_id: Option<ClientOrderId>,
    ) -> PyResult<Py<PyAny>> {
        py_order_from_factory(py, || {
            self.0.borrow_mut().try_market_to_limit(
                instrument_id,
                order_side,
                quantity,
                time_in_force,
                expire_time.map(UnixNanos::from),
                reduce_only,
                quote_quantity,
                display_qty,
                exec_algorithm_id,
                exec_algorithm_params.map(str_indexmap_to_ustr),
                tags.map(strings_to_ustrs),
                client_order_id,
            )
        })
    }

    #[pyo3(name = "market_if_touched", signature = (
        instrument_id,
        order_side,
        quantity,
        trigger_price,
        trigger_type=None,
        time_in_force=None,
        expire_time=None,
        reduce_only=None,
        quote_quantity=None,
        emulation_trigger=None,
        trigger_instrument_id=None,
        exec_algorithm_id=None,
        exec_algorithm_params=None,
        tags=None,
        client_order_id=None
    ))]
    fn py_market_if_touched(
        &mut self,
        py: Python<'_>,
        instrument_id: InstrumentId,
        order_side: OrderSide,
        quantity: Quantity,
        trigger_price: Price,
        trigger_type: Option<TriggerType>,
        time_in_force: Option<TimeInForce>,
        expire_time: Option<u64>,
        reduce_only: Option<bool>,
        quote_quantity: Option<bool>,
        emulation_trigger: Option<TriggerType>,
        trigger_instrument_id: Option<InstrumentId>,
        exec_algorithm_id: Option<ExecAlgorithmId>,
        exec_algorithm_params: Option<IndexMap<String, String>>,
        tags: Option<Vec<String>>,
        client_order_id: Option<ClientOrderId>,
    ) -> PyResult<Py<PyAny>> {
        py_order_from_factory(py, || {
            self.0.borrow_mut().try_market_if_touched(
                instrument_id,
                order_side,
                quantity,
                trigger_price,
                trigger_type,
                time_in_force,
                expire_time.map(UnixNanos::from),
                reduce_only,
                quote_quantity,
                emulation_trigger,
                trigger_instrument_id,
                exec_algorithm_id,
                exec_algorithm_params.map(str_indexmap_to_ustr),
                tags.map(strings_to_ustrs),
                client_order_id,
            )
        })
    }

    #[pyo3(name = "limit_if_touched", signature = (
        instrument_id,
        order_side,
        quantity,
        price,
        trigger_price,
        trigger_type=None,
        time_in_force=None,
        expire_time=None,
        post_only=None,
        reduce_only=None,
        quote_quantity=None,
        display_qty=None,
        emulation_trigger=None,
        trigger_instrument_id=None,
        exec_algorithm_id=None,
        exec_algorithm_params=None,
        tags=None,
        client_order_id=None
    ))]
    fn py_limit_if_touched(
        &mut self,
        py: Python<'_>,
        instrument_id: InstrumentId,
        order_side: OrderSide,
        quantity: Quantity,
        price: Price,
        trigger_price: Price,
        trigger_type: Option<TriggerType>,
        time_in_force: Option<TimeInForce>,
        expire_time: Option<u64>,
        post_only: Option<bool>,
        reduce_only: Option<bool>,
        quote_quantity: Option<bool>,
        display_qty: Option<Quantity>,
        emulation_trigger: Option<TriggerType>,
        trigger_instrument_id: Option<InstrumentId>,
        exec_algorithm_id: Option<ExecAlgorithmId>,
        exec_algorithm_params: Option<IndexMap<String, String>>,
        tags: Option<Vec<String>>,
        client_order_id: Option<ClientOrderId>,
    ) -> PyResult<Py<PyAny>> {
        py_order_from_factory(py, || {
            self.0.borrow_mut().try_limit_if_touched(
                instrument_id,
                order_side,
                quantity,
                price,
                trigger_price,
                trigger_type,
                time_in_force,
                expire_time.map(UnixNanos::from),
                post_only,
                reduce_only,
                quote_quantity,
                display_qty,
                emulation_trigger,
                trigger_instrument_id,
                exec_algorithm_id,
                exec_algorithm_params.map(str_indexmap_to_ustr),
                tags.map(strings_to_ustrs),
                client_order_id,
            )
        })
    }

    #[pyo3(name = "trailing_stop_market", signature = (
        instrument_id,
        order_side,
        quantity,
        trailing_offset,
        trailing_offset_type=None,
        activation_price=None,
        trigger_price=None,
        trigger_type=None,
        time_in_force=None,
        expire_time=None,
        reduce_only=None,
        quote_quantity=None,
        display_qty=None,
        emulation_trigger=None,
        trigger_instrument_id=None,
        exec_algorithm_id=None,
        exec_algorithm_params=None,
        tags=None,
        client_order_id=None
    ))]
    fn py_trailing_stop_market(
        &mut self,
        py: Python<'_>,
        instrument_id: InstrumentId,
        order_side: OrderSide,
        quantity: Quantity,
        trailing_offset: Decimal,
        trailing_offset_type: Option<TrailingOffsetType>,
        activation_price: Option<Price>,
        trigger_price: Option<Price>,
        trigger_type: Option<TriggerType>,
        time_in_force: Option<TimeInForce>,
        expire_time: Option<u64>,
        reduce_only: Option<bool>,
        quote_quantity: Option<bool>,
        display_qty: Option<Quantity>,
        emulation_trigger: Option<TriggerType>,
        trigger_instrument_id: Option<InstrumentId>,
        exec_algorithm_id: Option<ExecAlgorithmId>,
        exec_algorithm_params: Option<IndexMap<String, String>>,
        tags: Option<Vec<String>>,
        client_order_id: Option<ClientOrderId>,
    ) -> PyResult<Py<PyAny>> {
        py_order_from_factory(py, || {
            self.0.borrow_mut().try_trailing_stop_market(
                instrument_id,
                order_side,
                quantity,
                trailing_offset,
                trailing_offset_type,
                activation_price,
                trigger_price,
                trigger_type,
                time_in_force,
                expire_time.map(UnixNanos::from),
                reduce_only,
                quote_quantity,
                display_qty,
                emulation_trigger,
                trigger_instrument_id,
                exec_algorithm_id,
                exec_algorithm_params.map(str_indexmap_to_ustr),
                tags.map(strings_to_ustrs),
                client_order_id,
            )
        })
    }

    #[pyo3(name = "trailing_stop_limit", signature = (
        instrument_id,
        order_side,
        quantity,
        price,
        limit_offset,
        trailing_offset,
        trailing_offset_type=None,
        activation_price=None,
        trigger_price=None,
        trigger_type=None,
        time_in_force=None,
        expire_time=None,
        post_only=None,
        reduce_only=None,
        quote_quantity=None,
        display_qty=None,
        emulation_trigger=None,
        trigger_instrument_id=None,
        exec_algorithm_id=None,
        exec_algorithm_params=None,
        tags=None,
        client_order_id=None
    ))]
    fn py_trailing_stop_limit(
        &mut self,
        py: Python<'_>,
        instrument_id: InstrumentId,
        order_side: OrderSide,
        quantity: Quantity,
        price: Price,
        limit_offset: Decimal,
        trailing_offset: Decimal,
        trailing_offset_type: Option<TrailingOffsetType>,
        activation_price: Option<Price>,
        trigger_price: Option<Price>,
        trigger_type: Option<TriggerType>,
        time_in_force: Option<TimeInForce>,
        expire_time: Option<u64>,
        post_only: Option<bool>,
        reduce_only: Option<bool>,
        quote_quantity: Option<bool>,
        display_qty: Option<Quantity>,
        emulation_trigger: Option<TriggerType>,
        trigger_instrument_id: Option<InstrumentId>,
        exec_algorithm_id: Option<ExecAlgorithmId>,
        exec_algorithm_params: Option<IndexMap<String, String>>,
        tags: Option<Vec<String>>,
        client_order_id: Option<ClientOrderId>,
    ) -> PyResult<Py<PyAny>> {
        py_order_from_factory(py, || {
            self.0.borrow_mut().try_trailing_stop_limit(
                instrument_id,
                order_side,
                quantity,
                price,
                limit_offset,
                trailing_offset,
                trailing_offset_type,
                activation_price,
                trigger_price,
                trigger_type,
                time_in_force,
                expire_time.map(UnixNanos::from),
                post_only,
                reduce_only,
                quote_quantity,
                display_qty,
                emulation_trigger,
                trigger_instrument_id,
                exec_algorithm_id,
                exec_algorithm_params.map(str_indexmap_to_ustr),
                tags.map(strings_to_ustrs),
                client_order_id,
            )
        })
    }

    #[pyo3(name = "bracket", signature = (
        instrument_id,
        order_side,
        quantity,
        quote_quantity=false,
        emulation_trigger=None,
        trigger_instrument_id=None,
        contingency_type=ContingencyType::Ouo,
        entry_order_type=OrderType::Market,
        entry_price=None,
        entry_trigger_price=None,
        expire_time=None,
        time_in_force=TimeInForce::Gtc,
        entry_post_only=false,
        entry_exec_algorithm_id=None,
        entry_exec_algorithm_params=None,
        entry_tags=None,
        entry_client_order_id=None,
        tp_order_type=OrderType::Limit,
        tp_price=None,
        tp_trigger_price=None,
        tp_trigger_type=TriggerType::Default,
        tp_activation_price=None,
        tp_trailing_offset=None,
        tp_trailing_offset_type=TrailingOffsetType::Price,
        tp_limit_offset=None,
        tp_time_in_force=TimeInForce::Gtc,
        tp_post_only=true,
        tp_exec_algorithm_id=None,
        tp_exec_algorithm_params=None,
        tp_tags=None,
        tp_client_order_id=None,
        sl_order_type=OrderType::StopMarket,
        sl_trigger_price=None,
        sl_trigger_type=TriggerType::Default,
        sl_activation_price=None,
        sl_trailing_offset=None,
        sl_trailing_offset_type=TrailingOffsetType::Price,
        sl_time_in_force=TimeInForce::Gtc,
        sl_exec_algorithm_id=None,
        sl_exec_algorithm_params=None,
        sl_tags=None,
        sl_client_order_id=None
    ))]
    fn py_bracket(
        &mut self,
        py: Python<'_>,
        instrument_id: InstrumentId,
        order_side: OrderSide,
        quantity: Quantity,
        quote_quantity: bool,
        emulation_trigger: Option<TriggerType>,
        trigger_instrument_id: Option<InstrumentId>,
        contingency_type: ContingencyType,
        entry_order_type: OrderType,
        entry_price: Option<Price>,
        entry_trigger_price: Option<Price>,
        expire_time: Option<u64>,
        time_in_force: TimeInForce,
        entry_post_only: bool,
        entry_exec_algorithm_id: Option<ExecAlgorithmId>,
        entry_exec_algorithm_params: Option<IndexMap<String, String>>,
        entry_tags: Option<Vec<String>>,
        entry_client_order_id: Option<ClientOrderId>,
        tp_order_type: OrderType,
        tp_price: Option<Price>,
        tp_trigger_price: Option<Price>,
        tp_trigger_type: TriggerType,
        tp_activation_price: Option<Price>,
        tp_trailing_offset: Option<Decimal>,
        tp_trailing_offset_type: TrailingOffsetType,
        tp_limit_offset: Option<Decimal>,
        tp_time_in_force: TimeInForce,
        tp_post_only: bool,
        tp_exec_algorithm_id: Option<ExecAlgorithmId>,
        tp_exec_algorithm_params: Option<IndexMap<String, String>>,
        tp_tags: Option<Vec<String>>,
        tp_client_order_id: Option<ClientOrderId>,
        sl_order_type: OrderType,
        sl_trigger_price: Option<Price>,
        sl_trigger_type: TriggerType,
        sl_activation_price: Option<Price>,
        sl_trailing_offset: Option<Decimal>,
        sl_trailing_offset_type: TrailingOffsetType,
        sl_time_in_force: TimeInForce,
        sl_exec_algorithm_id: Option<ExecAlgorithmId>,
        sl_exec_algorithm_params: Option<IndexMap<String, String>>,
        sl_tags: Option<Vec<String>>,
        sl_client_order_id: Option<ClientOrderId>,
    ) -> PyResult<Py<PyList>> {
        let orders = py_orders_from_factory(|| {
            self.0
                .borrow_mut()
                .try_bracket()
                .instrument_id(instrument_id)
                .order_side(order_side)
                .quantity(quantity)
                .quote_quantity(quote_quantity)
                .maybe_emulation_trigger(emulation_trigger)
                .maybe_trigger_instrument_id(trigger_instrument_id)
                .contingency_type(contingency_type)
                .entry_order_type(entry_order_type)
                .maybe_entry_price(entry_price)
                .maybe_entry_trigger_price(entry_trigger_price)
                .maybe_expire_time(expire_time.map(UnixNanos::from))
                .time_in_force(time_in_force)
                .entry_post_only(entry_post_only)
                .maybe_entry_exec_algorithm_id(entry_exec_algorithm_id)
                .maybe_entry_exec_algorithm_params(
                    entry_exec_algorithm_params.map(str_indexmap_to_ustr),
                )
                .maybe_entry_tags(entry_tags.map(strings_to_ustrs))
                .maybe_entry_client_order_id(entry_client_order_id)
                .tp_order_type(tp_order_type)
                .maybe_tp_price(tp_price)
                .maybe_tp_trigger_price(tp_trigger_price)
                .tp_trigger_type(tp_trigger_type)
                .maybe_tp_activation_price(tp_activation_price)
                .maybe_tp_trailing_offset(tp_trailing_offset)
                .tp_trailing_offset_type(tp_trailing_offset_type)
                .maybe_tp_limit_offset(tp_limit_offset)
                .tp_time_in_force(tp_time_in_force)
                .tp_post_only(tp_post_only)
                .maybe_tp_exec_algorithm_id(tp_exec_algorithm_id)
                .maybe_tp_exec_algorithm_params(tp_exec_algorithm_params.map(str_indexmap_to_ustr))
                .maybe_tp_tags(tp_tags.map(strings_to_ustrs))
                .maybe_tp_client_order_id(tp_client_order_id)
                .sl_order_type(sl_order_type)
                .maybe_sl_trigger_price(sl_trigger_price)
                .sl_trigger_type(sl_trigger_type)
                .maybe_sl_activation_price(sl_activation_price)
                .maybe_sl_trailing_offset(sl_trailing_offset)
                .sl_trailing_offset_type(sl_trailing_offset_type)
                .sl_time_in_force(sl_time_in_force)
                .maybe_sl_exec_algorithm_id(sl_exec_algorithm_id)
                .maybe_sl_exec_algorithm_params(sl_exec_algorithm_params.map(str_indexmap_to_ustr))
                .maybe_sl_tags(sl_tags.map(strings_to_ustrs))
                .maybe_sl_client_order_id(sl_client_order_id)
                .call()
        })?;
        let py_orders = orders
            .into_iter()
            .map(|order| order_any_to_pyobject(py, order))
            .collect::<PyResult<Vec<_>>>()?;
        Ok(PyList::new(py, py_orders)?.unbind())
    }
}

fn py_order_from_factory<F>(py: Python<'_>, create: F) -> PyResult<Py<PyAny>>
where
    F: FnOnce() -> anyhow::Result<OrderAny>,
{
    let order = create().map_err(to_pyvalue_err)?;
    order_any_to_pyobject(py, order)
}

fn py_orders_from_factory<F>(create: F) -> PyResult<Vec<OrderAny>>
where
    F: FnOnce() -> anyhow::Result<Vec<OrderAny>>,
{
    create().map_err(to_pyvalue_err)
}

fn strings_to_ustrs(values: Vec<String>) -> Vec<Ustr> {
    values
        .into_iter()
        .map(|value| Ustr::from(value.as_str()))
        .collect()
}
