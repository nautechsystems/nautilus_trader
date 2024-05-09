// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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
use pyo3::{IntoPy, PyObject, PyResult, Python};

use crate::{
    enums::OrderType,
    orders::{
        any::OrderAny, limit::LimitOrder, limit_if_touched::LimitIfTouchedOrder,
        market::MarketOrder, market_if_touched::MarketIfTouchedOrder,
        market_to_limit::MarketToLimitOrder, stop_limit::StopLimitOrder,
        stop_market::StopMarketOrder, trailing_stop_limit::TrailingStopLimitOrder,
        trailing_stop_market::TrailingStopMarketOrder,
    },
};

pub fn convert_pyobject_to_order_any(py: Python, order: PyObject) -> PyResult<OrderAny> {
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

pub fn convert_order_any_to_pyobject(py: Python, order: OrderAny) -> PyResult<PyObject> {
    match order {
        OrderAny::Limit(limit_order) => Ok(limit_order.into_py(py)),
        OrderAny::LimitIfTouched(limit_if_touched_order) => Ok(limit_if_touched_order.into_py(py)),
        OrderAny::Market(market_order) => Ok(market_order.into_py(py)),
        OrderAny::MarketIfTouched(market_if_touched_order) => {
            Ok(market_if_touched_order.into_py(py))
        }
        OrderAny::MarketToLimit(market_to_limit_order) => Ok(market_to_limit_order.into_py(py)),
        OrderAny::StopLimit(stop_limit_order) => Ok(stop_limit_order.into_py(py)),
        OrderAny::StopMarket(stop_market_order) => Ok(stop_market_order.into_py(py)),
        OrderAny::TrailingStopLimit(trailing_stop_limit_order) => {
            Ok(trailing_stop_limit_order.into_py(py))
        }
        OrderAny::TrailingStopMarket(trailing_stop_market_order) => {
            Ok(trailing_stop_market_order.into_py(py))
        }
    }
}

pub mod limit;
pub mod limit_if_touched;
pub mod market;
pub mod market_if_touched;
pub mod market_to_limit;
pub mod stop_limit;
pub mod stop_market;
pub mod trailing_stop_limit;
pub mod trailing_stop_market;
