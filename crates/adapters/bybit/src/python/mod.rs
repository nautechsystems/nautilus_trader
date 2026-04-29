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

//! Python bindings from `pyo3`.

pub mod config;
pub mod enums;
pub mod factories;
pub mod http;
pub mod params;
pub mod types;
pub mod urls;
pub mod websocket;

use nautilus_common::factories::{ClientConfig, DataClientFactory, ExecutionClientFactory};
use nautilus_core::python::{to_pyruntime_err, to_pyvalue_err};
use nautilus_model::enums::{BarAggregation, OrderSide};
use nautilus_system::get_global_pyo3_registry;
use pyo3::prelude::*;

use crate::{
    common::{
        consts::BYBIT_NAUTILUS_BROKER_ID,
        enums::{BybitOrderSide, BybitPositionIdx, BybitPositionMode},
        parse::{bar_spec_to_bybit_interval, extract_raw_symbol},
        symbol::BybitSymbol,
    },
    config::{BybitDataClientConfig, BybitExecClientConfig},
    execution::resolve_position_idx,
    factories::{BybitDataClientFactory, BybitExecutionClientFactory},
};

/// Extracts the raw symbol from a Bybit symbol by removing the product type suffix.
///
/// # Examples
/// - `"ETHUSDT-LINEAR"` → `"ETHUSDT"`
/// - `"BTCUSDT-SPOT"` → `"BTCUSDT"`
/// - `"ETHUSDT"` → `"ETHUSDT"` (no suffix)
#[pyfunction]
#[pyo3_stub_gen::derive::gen_stub_pyfunction(module = "nautilus_trader.bybit")]
#[pyo3(name = "bybit_extract_raw_symbol")]
fn py_bybit_extract_raw_symbol(symbol: &str) -> &str {
    extract_raw_symbol(symbol)
}

/// Converts a Nautilus bar aggregation and step to a Bybit kline interval string.
///
/// # Errors
///
/// Returns an error if the aggregation type or step is not supported by Bybit.
#[pyfunction]
#[pyo3_stub_gen::derive::gen_stub_pyfunction(module = "nautilus_trader.bybit")]
#[pyo3(name = "bybit_bar_spec_to_interval")]
fn py_bybit_bar_spec_to_interval(aggregation: u8, step: u64) -> PyResult<String> {
    let aggregation = BarAggregation::from_repr(aggregation as usize)
        .ok_or_else(|| to_pyvalue_err(format!("Invalid BarAggregation value: {aggregation}")))?;
    let interval = bar_spec_to_bybit_interval(aggregation, step).map_err(to_pyvalue_err)?;
    Ok(interval.to_string())
}

/// Extracts the product type from a Bybit symbol.
///
/// # Examples
/// - `"ETHUSDT-LINEAR"` → `BybitProductType.LINEAR`
/// - `"BTCUSDT-SPOT"` → `BybitProductType.SPOT`
/// - `"BTCUSD-INVERSE"` → `BybitProductType.INVERSE`
/// - `"ETH-26JUN26-16000-P-OPTION"` → `BybitProductType.OPTION`
///
/// # Errors
///
/// Returns an error if the symbol does not contain a valid Bybit product type suffix.
#[pyfunction]
#[pyo3_stub_gen::derive::gen_stub_pyfunction(module = "nautilus_trader.bybit")]
#[pyo3(name = "bybit_product_type_from_symbol")]
fn py_bybit_product_type_from_symbol(
    symbol: &str,
) -> PyResult<crate::common::enums::BybitProductType> {
    let bybit_symbol = BybitSymbol::new(symbol).map_err(to_pyvalue_err)?;
    Ok(bybit_symbol.product_type())
}

/// Resolves the Bybit `positionIdx` for an outgoing order.
///
/// Returns `None` when no position mode is configured. Otherwise returns the
/// hedge-mode index for the position being affected (long or short), accounting
/// for `is_reduce_only`. A `manual_override` always wins.
#[pyfunction]
#[pyo3_stub_gen::derive::gen_stub_pyfunction(module = "nautilus_trader.bybit")]
#[pyo3(name = "bybit_resolve_position_idx")]
#[pyo3(signature = (position_mode, order_side, is_reduce_only, manual_override=None))]
fn py_bybit_resolve_position_idx(
    position_mode: Option<BybitPositionMode>,
    order_side: OrderSide,
    is_reduce_only: bool,
    manual_override: Option<BybitPositionIdx>,
) -> PyResult<Option<BybitPositionIdx>> {
    let bybit_side = BybitOrderSide::try_from(order_side).map_err(to_pyvalue_err)?;
    Ok(resolve_position_idx(
        position_mode,
        bybit_side,
        is_reduce_only,
        manual_override,
    ))
}

#[expect(clippy::needless_pass_by_value)]
fn extract_bybit_data_factory(
    py: Python<'_>,
    factory: Py<PyAny>,
) -> PyResult<Box<dyn DataClientFactory>> {
    match factory.extract::<BybitDataClientFactory>(py) {
        Ok(f) => Ok(Box::new(f)),
        Err(e) => Err(to_pyvalue_err(format!(
            "Failed to extract BybitDataClientFactory: {e}"
        ))),
    }
}

#[expect(clippy::needless_pass_by_value)]
fn extract_bybit_exec_factory(
    py: Python<'_>,
    factory: Py<PyAny>,
) -> PyResult<Box<dyn ExecutionClientFactory>> {
    match factory.extract::<BybitExecutionClientFactory>(py) {
        Ok(f) => Ok(Box::new(f)),
        Err(e) => Err(to_pyvalue_err(format!(
            "Failed to extract BybitExecutionClientFactory: {e}"
        ))),
    }
}

#[expect(clippy::needless_pass_by_value)]
fn extract_bybit_data_config(py: Python<'_>, config: Py<PyAny>) -> PyResult<Box<dyn ClientConfig>> {
    match config.extract::<BybitDataClientConfig>(py) {
        Ok(c) => Ok(Box::new(c)),
        Err(e) => Err(to_pyvalue_err(format!(
            "Failed to extract BybitDataClientConfig: {e}"
        ))),
    }
}

#[expect(clippy::needless_pass_by_value)]
fn extract_bybit_exec_config(py: Python<'_>, config: Py<PyAny>) -> PyResult<Box<dyn ClientConfig>> {
    match config.extract::<BybitExecClientConfig>(py) {
        Ok(c) => Ok(Box::new(c)),
        Err(e) => Err(to_pyvalue_err(format!(
            "Failed to extract BybitExecClientConfig: {e}"
        ))),
    }
}

/// Loaded as `nautilus_pyo3.bybit`.
///
/// # Errors
///
/// Returns an error if any bindings fail to register with the Python module.
#[pymodule]
#[rustfmt::skip]
pub fn bybit(_: Python<'_>, m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add(stringify!(BYBIT_NAUTILUS_BROKER_ID), BYBIT_NAUTILUS_BROKER_ID)?;
    m.add_class::<crate::common::enums::BybitAccountType>()?;
    m.add_class::<crate::common::enums::BybitCancelType>()?;
    m.add_class::<crate::common::enums::BybitEnvironment>()?;
    m.add_class::<crate::common::enums::BybitMarginAction>()?;
    m.add_class::<crate::common::enums::BybitMarginMode>()?;
    m.add_class::<crate::common::enums::BybitOpenOnly>()?;
    m.add_class::<crate::common::enums::BybitOrderFilter>()?;
    m.add_class::<crate::common::enums::BybitOrderSide>()?;
    m.add_class::<crate::common::enums::BybitOrderStatus>()?;
    m.add_class::<crate::common::enums::BybitOrderType>()?;
    m.add_class::<crate::common::enums::BybitPositionIdx>()?;
    m.add_class::<crate::common::enums::BybitPositionMode>()?;
    m.add_class::<crate::common::enums::BybitProductType>()?;
    m.add_class::<crate::common::enums::BybitStopOrderType>()?;
    m.add_class::<crate::common::enums::BybitTimeInForce>()?;
    m.add_class::<crate::common::enums::BybitTpSlMode>()?;
    m.add_class::<crate::common::enums::BybitTriggerDirection>()?;
    m.add_class::<crate::common::enums::BybitTriggerType>()?;
    m.add_class::<crate::http::client::BybitHttpClient>()?;
    m.add_class::<crate::http::client::BybitRawHttpClient>()?;
    m.add_class::<crate::http::models::BybitServerTime>()?;
    m.add_class::<crate::http::models::BybitOrder>()?;
    m.add_class::<crate::http::models::BybitOrderCursorList>()?;
    m.add_class::<crate::http::models::BybitTickerData>()?;
    m.add_class::<crate::common::types::BybitMarginBorrowResult>()?;
    m.add_class::<crate::common::types::BybitMarginRepayResult>()?;
    m.add_class::<crate::common::types::BybitMarginStatusResult>()?;
    m.add_class::<crate::websocket::client::BybitWebSocketClient>()?;
    m.add_class::<crate::websocket::messages::BybitWebSocketError>()?;
    m.add_class::<params::BybitWsPlaceOrderParams>()?;
    m.add_class::<params::BybitWsAmendOrderParams>()?;
    m.add_class::<params::BybitWsCancelOrderParams>()?;
    m.add_class::<params::BybitTickersParams>()?;
    m.add_class::<BybitDataClientConfig>()?;
    m.add_class::<BybitExecClientConfig>()?;
    m.add_class::<BybitDataClientFactory>()?;
    m.add_class::<BybitExecutionClientFactory>()?;
    m.add_function(wrap_pyfunction!(urls::py_get_bybit_http_base_url, m)?)?;
    m.add_function(wrap_pyfunction!(urls::py_get_bybit_ws_url_public, m)?)?;
    m.add_function(wrap_pyfunction!(urls::py_get_bybit_ws_url_private, m)?)?;
    m.add_function(wrap_pyfunction!(urls::py_get_bybit_ws_url_trade, m)?)?;
    m.add_function(wrap_pyfunction!(py_bybit_extract_raw_symbol, m)?)?;
    m.add_function(wrap_pyfunction!(py_bybit_bar_spec_to_interval, m)?)?;
    m.add_function(wrap_pyfunction!(py_bybit_product_type_from_symbol, m)?)?;
    m.add_function(wrap_pyfunction!(py_bybit_resolve_position_idx, m)?)?;

    let registry = get_global_pyo3_registry();

    if let Err(e) =
        registry.register_factory_extractor("BYBIT".to_string(), extract_bybit_data_factory)
    {
        return Err(to_pyruntime_err(format!(
            "Failed to register Bybit data factory extractor: {e}"
        )));
    }

    if let Err(e) = registry
        .register_exec_factory_extractor("BYBIT".to_string(), extract_bybit_exec_factory)
    {
        return Err(to_pyruntime_err(format!(
            "Failed to register Bybit exec factory extractor: {e}"
        )));
    }

    if let Err(e) = registry.register_config_extractor(
        "BybitDataClientConfig".to_string(),
        extract_bybit_data_config,
    ) {
        return Err(to_pyruntime_err(format!(
            "Failed to register Bybit data config extractor: {e}"
        )));
    }

    if let Err(e) = registry.register_config_extractor(
        "BybitExecClientConfig".to_string(),
        extract_bybit_exec_config,
    ) {
        return Err(to_pyruntime_err(format!(
            "Failed to register Bybit exec config extractor: {e}"
        )));
    }

    Ok(())
}
