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

use nautilus_core::python::to_pyvalue_err;
use pyo3::{IntoPyObjectExt, Py, PyAny, PyResult, Python};
use rust_decimal::Decimal;

use crate::{
    enums::{LiquiditySide, OrderType},
    identifiers::{AccountId, PositionId, TradeId, VenueOrderId},
    orders::{
        LimitIfTouchedOrder, LimitOrder, MarketIfTouchedOrder, MarketOrder, MarketToLimitOrder,
        Order, OrderAny, StopLimitOrder, StopMarketOrder, TrailingStopLimitOrder,
        TrailingStopMarketOrder,
    },
    python::events::order::order_event_to_pyobject,
    types::Quantity,
};

pub mod limit;
pub mod limit_if_touched;
pub mod list;
pub mod market;
pub mod market_if_touched;
pub mod market_to_limit;
pub mod stop_limit;
pub mod stop_market;
pub mod trailing_stop_limit;
pub mod trailing_stop_market;

/// Converts a Python order object into an [`OrderAny`] enum.
///
/// # Errors
///
/// Returns a `PyErr` if extraction fails or the order type is unsupported.
#[expect(clippy::needless_pass_by_value)]
pub fn pyobject_to_order_any(py: Python, order: Py<PyAny>) -> PyResult<OrderAny> {
    let order_type = order.getattr(py, "order_type")?.extract::<OrderType>(py)?;
    if order_type == OrderType::Limit {
        let limit = order.extract::<LimitOrder>(py)?;
        Ok(OrderAny::Limit(limit))
    } else if order_type == OrderType::Market {
        let market = order.extract::<MarketOrder>(py)?;
        Ok(OrderAny::Market(market))
    } else if order_type == OrderType::StopLimit {
        let stop_limit = order.extract::<StopLimitOrder>(py)?;
        Ok(OrderAny::StopLimit(stop_limit))
    } else if order_type == OrderType::LimitIfTouched {
        let limit_if_touched = order.extract::<LimitIfTouchedOrder>(py)?;
        Ok(OrderAny::LimitIfTouched(limit_if_touched))
    } else if order_type == OrderType::MarketIfTouched {
        let market_if_touched = order.extract::<MarketIfTouchedOrder>(py)?;
        Ok(OrderAny::MarketIfTouched(market_if_touched))
    } else if order_type == OrderType::MarketToLimit {
        let market_to_limit = order.extract::<MarketToLimitOrder>(py)?;
        Ok(OrderAny::MarketToLimit(market_to_limit))
    } else if order_type == OrderType::StopMarket {
        let stop_market = order.extract::<StopMarketOrder>(py)?;
        Ok(OrderAny::StopMarket(stop_market))
    } else if order_type == OrderType::TrailingStopMarket {
        let trailing_stop_market = order.extract::<TrailingStopMarketOrder>(py)?;
        Ok(OrderAny::TrailingStopMarket(trailing_stop_market))
    } else if order_type == OrderType::TrailingStopLimit {
        let trailing_stop_limit = order.extract::<TrailingStopLimitOrder>(py)?;
        Ok(OrderAny::TrailingStopLimit(trailing_stop_limit))
    } else {
        Err(to_pyvalue_err("Unsupported order type"))
    }
}

/// Converts an [`OrderAny`] enum into a Python object.
///
/// # Errors
///
/// Returns a `PyErr` if conversion to a Python object fails.
pub fn order_any_to_pyobject(py: Python, order: OrderAny) -> PyResult<Py<PyAny>> {
    match order {
        OrderAny::Limit(limit_order) => limit_order.into_py_any(py),
        OrderAny::LimitIfTouched(limit_if_touched_order) => limit_if_touched_order.into_py_any(py),
        OrderAny::Market(market_order) => market_order.into_py_any(py),
        OrderAny::MarketIfTouched(market_if_touched_order) => {
            market_if_touched_order.into_py_any(py)
        }
        OrderAny::MarketToLimit(market_to_limit_order) => market_to_limit_order.into_py_any(py),
        OrderAny::StopLimit(stop_limit_order) => stop_limit_order.into_py_any(py),
        OrderAny::StopMarket(stop_market_order) => stop_market_order.into_py_any(py),
        OrderAny::TrailingStopLimit(trailing_stop_limit_order) => {
            trailing_stop_limit_order.into_py_any(py)
        }
        OrderAny::TrailingStopMarket(trailing_stop_market_order) => {
            trailing_stop_market_order.into_py_any(py)
        }
    }
}

macro_rules! impl_order_common_pymethods {
    ($type:ty) => {
        #[pyo3_stub_gen::derive::gen_stub_pymethods]
        #[pyo3::pymethods]
        impl $type {
            #[getter]
            #[pyo3(name = "avg_px")]
            fn py_avg_px(&self) -> Option<Decimal> {
                self.avg_px()
            }

            #[getter]
            #[pyo3(name = "event_count")]
            fn py_event_count(&self) -> usize {
                self.event_count()
            }

            #[getter]
            #[pyo3(name = "is_buy")]
            fn py_is_buy(&self) -> bool {
                self.is_buy()
            }

            #[getter]
            #[pyo3(name = "is_canceled")]
            fn py_is_canceled(&self) -> bool {
                self.is_canceled()
            }

            #[getter]
            #[pyo3(name = "is_inflight")]
            fn py_is_inflight(&self) -> bool {
                self.is_inflight()
            }

            #[getter]
            #[pyo3(name = "is_pending_cancel")]
            fn py_is_pending_cancel(&self) -> bool {
                self.is_pending_cancel()
            }

            #[getter]
            #[pyo3(name = "is_pending_update")]
            fn py_is_pending_update(&self) -> bool {
                self.is_pending_update()
            }

            #[getter]
            #[pyo3(name = "is_sell")]
            fn py_is_sell(&self) -> bool {
                self.is_sell()
            }

            #[getter]
            #[pyo3(name = "last_event")]
            fn py_last_event(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
                order_event_to_pyobject(py, self.last_event().clone())
            }

            #[getter]
            #[pyo3(name = "leaves_qty")]
            fn py_leaves_qty(&self) -> Quantity {
                self.leaves_qty()
            }

            #[getter]
            #[pyo3(name = "overfill_qty")]
            fn py_overfill_qty(&self) -> Quantity {
                self.overfill_qty()
            }

            #[getter]
            #[pyo3(name = "slippage")]
            fn py_slippage(&self) -> Option<Decimal> {
                self.slippage()
            }

            #[getter]
            #[pyo3(name = "trade_ids")]
            fn py_trade_ids(&self) -> Vec<TradeId> {
                self.trade_ids().into_iter().copied().collect()
            }

            #[getter]
            #[pyo3(name = "ts_accepted")]
            fn py_ts_accepted(&self) -> Option<u64> {
                self.ts_accepted().map(|ts| ts.as_u64())
            }

            #[getter]
            #[pyo3(name = "ts_closed")]
            fn py_ts_closed(&self) -> Option<u64> {
                self.ts_closed().map(|ts| ts.as_u64())
            }

            #[getter]
            #[pyo3(name = "ts_submitted")]
            fn py_ts_submitted(&self) -> Option<u64> {
                self.ts_submitted().map(|ts| ts.as_u64())
            }

            #[getter]
            #[pyo3(name = "venue_order_ids")]
            fn py_venue_order_ids(&self) -> Vec<VenueOrderId> {
                self.venue_order_ids().into_iter().copied().collect()
            }
        }
    };
}

macro_rules! impl_order_runtime_state_pymethods {
    ($type:ty) => {
        #[pyo3_stub_gen::derive::gen_stub_pymethods]
        #[pyo3::pymethods]
        impl $type {
            #[getter]
            #[pyo3(name = "filled_qty")]
            fn py_filled_qty(&self) -> Quantity {
                self.filled_qty()
            }

            #[getter]
            #[pyo3(name = "is_active_local")]
            fn py_is_active_local(&self) -> bool {
                self.is_active_local()
            }

            #[getter]
            #[pyo3(name = "is_emulated")]
            fn py_is_emulated(&self) -> bool {
                self.is_emulated()
            }

            #[getter]
            #[pyo3(name = "is_primary")]
            fn py_is_primary(&self) -> bool {
                self.is_primary()
            }

            #[getter]
            #[pyo3(name = "is_spawned")]
            fn py_is_spawned(&self) -> bool {
                self.is_spawned()
            }

            #[getter]
            #[pyo3(name = "last_trade_id")]
            fn py_last_trade_id(&self) -> Option<TradeId> {
                self.last_trade_id()
            }

            #[getter]
            #[pyo3(name = "liquidity_side")]
            fn py_liquidity_side(&self) -> Option<LiquiditySide> {
                self.liquidity_side()
            }

            #[getter]
            #[pyo3(name = "position_id")]
            fn py_position_id(&self) -> Option<PositionId> {
                self.position_id()
            }

            #[getter]
            #[pyo3(name = "ts_last")]
            fn py_ts_last(&self) -> u64 {
                self.ts_last().as_u64()
            }

            #[getter]
            #[pyo3(name = "venue_order_id")]
            fn py_venue_order_id(&self) -> Option<VenueOrderId> {
                self.venue_order_id()
            }
        }
    };
}

macro_rules! impl_order_account_id_pymethods {
    ($type:ty) => {
        #[pyo3_stub_gen::derive::gen_stub_pymethods]
        #[pyo3::pymethods]
        impl $type {
            #[getter]
            #[pyo3(name = "account_id")]
            fn py_account_id(&self) -> Option<AccountId> {
                self.account_id()
            }
        }
    };
}

macro_rules! impl_order_init_event_pymethods {
    ($type:ty) => {
        #[pyo3_stub_gen::derive::gen_stub_pymethods]
        #[pyo3::pymethods]
        impl $type {
            #[getter]
            #[pyo3(name = "init_event")]
            fn py_init_event(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
                match self.init_event() {
                    Some(event) => order_event_to_pyobject(py, event),
                    None => Ok(py.None()),
                }
            }
        }
    };
}

macro_rules! impl_order_open_state_pymethods {
    ($type:ty) => {
        #[pyo3_stub_gen::derive::gen_stub_pymethods]
        #[pyo3::pymethods]
        impl $type {
            #[getter]
            #[pyo3(name = "is_open")]
            fn py_is_open(&self) -> bool {
                self.is_open()
            }

            #[getter]
            #[pyo3(name = "is_closed")]
            fn py_is_closed(&self) -> bool {
                self.is_closed()
            }
        }
    };
}

macro_rules! impl_trigger_order_state_pymethods {
    ($type:ty) => {
        #[pyo3_stub_gen::derive::gen_stub_pymethods]
        #[pyo3::pymethods]
        impl $type {
            #[getter]
            #[pyo3(name = "is_triggered")]
            fn py_is_triggered(&self) -> bool {
                self.is_triggered
            }

            #[getter]
            #[pyo3(name = "ts_triggered")]
            fn py_ts_triggered(&self) -> Option<u64> {
                self.ts_triggered.map(|ts| ts.as_u64())
            }
        }
    };
}

macro_rules! impl_trailing_order_activation_pymethods {
    ($type:ty) => {
        #[pyo3_stub_gen::derive::gen_stub_pymethods]
        #[pyo3::pymethods]
        impl $type {
            #[getter]
            #[pyo3(name = "is_activated")]
            fn py_is_activated(&self) -> bool {
                self.is_activated
            }
        }
    };
}

impl_order_common_pymethods!(LimitOrder);
impl_order_common_pymethods!(LimitIfTouchedOrder);
impl_order_common_pymethods!(MarketOrder);
impl_order_common_pymethods!(MarketIfTouchedOrder);
impl_order_common_pymethods!(MarketToLimitOrder);
impl_order_common_pymethods!(StopLimitOrder);
impl_order_common_pymethods!(StopMarketOrder);
impl_order_common_pymethods!(TrailingStopLimitOrder);
impl_order_common_pymethods!(TrailingStopMarketOrder);

impl_order_runtime_state_pymethods!(LimitIfTouchedOrder);
impl_order_runtime_state_pymethods!(MarketOrder);
impl_order_runtime_state_pymethods!(MarketIfTouchedOrder);
impl_order_runtime_state_pymethods!(MarketToLimitOrder);
impl_order_runtime_state_pymethods!(StopLimitOrder);
impl_order_runtime_state_pymethods!(StopMarketOrder);
impl_order_runtime_state_pymethods!(TrailingStopLimitOrder);
impl_order_runtime_state_pymethods!(TrailingStopMarketOrder);

impl_order_account_id_pymethods!(MarketIfTouchedOrder);
impl_order_account_id_pymethods!(StopLimitOrder);

impl_order_init_event_pymethods!(LimitOrder);
impl_order_init_event_pymethods!(MarketOrder);
impl_order_open_state_pymethods!(MarketOrder);

impl_trigger_order_state_pymethods!(LimitIfTouchedOrder);
impl_trigger_order_state_pymethods!(MarketIfTouchedOrder);
impl_trigger_order_state_pymethods!(StopLimitOrder);
impl_trigger_order_state_pymethods!(StopMarketOrder);
impl_trigger_order_state_pymethods!(TrailingStopLimitOrder);
impl_trigger_order_state_pymethods!(TrailingStopMarketOrder);

impl_trailing_order_activation_pymethods!(TrailingStopLimitOrder);
impl_trailing_order_activation_pymethods!(TrailingStopMarketOrder);
