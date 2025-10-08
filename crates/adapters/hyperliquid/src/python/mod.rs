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

//! Python bindings from `pyo3`.

pub mod enums;
pub mod http;
pub mod urls;
pub mod websocket;

use nautilus_core::python::to_pyvalue_err;
use pyo3::prelude::*;
use serde_json::to_string;

/// Loaded as `nautilus_pyo3.hyperliquid`.
#[pymodule]
pub fn hyperliquid(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<crate::http::HyperliquidHttpClient>()?;

    // Register conditional order enums
    m.add_class::<crate::common::enums::HyperliquidTpSl>()?;
    m.add_class::<crate::common::enums::HyperliquidTriggerPriceType>()?;
    m.add_class::<crate::common::enums::HyperliquidConditionalOrderType>()?;
    m.add_class::<crate::common::enums::HyperliquidTrailingOffsetType>()?;

    // Add order conversion functions
    m.add_function(wrap_pyfunction!(order_to_json, m)?)?;
    m.add_function(wrap_pyfunction!(orders_to_json, m)?)?;

    Ok(())
}

/// Convert a Nautilus OrderAny (from nautilus_pyo3) to Hyperliquid order format JSON.
///
/// Parameters
/// ----------
/// order : OrderAny
///     The Nautilus order from nautilus_pyo3.
///
/// Returns
/// -------
/// str
///     JSON string representing the Hyperliquid order request.
#[pyfunction]
#[pyo3(name = "order_to_json")]
fn order_to_json(order: &nautilus_model::orders::OrderAny) -> PyResult<String> {
    let order_request = crate::common::parse::order_to_hyperliquid_request(order)
        .map_err(to_pyvalue_err)?;
    to_string(&order_request).map_err(to_pyvalue_err)
}

/// Convert multiple Nautilus OrderAny objects to Hyperliquid orders array JSON.
///
/// Parameters
/// ----------
/// orders : list[OrderAny]
///     List of Nautilus orders from nautilus_pyo3.
///
/// Returns
/// -------
/// str
///     JSON string representing the Hyperliquid orders array.
#[pyfunction]
#[pyo3(name = "orders_to_json")]
fn orders_to_json(orders: Vec<&nautilus_model::orders::OrderAny>) -> PyResult<String> {
    let orders_value = crate::common::parse::orders_to_hyperliquid_action_value(&orders)
        .map_err(to_pyvalue_err)?;
    to_string(&orders_value).map_err(to_pyvalue_err)
}
