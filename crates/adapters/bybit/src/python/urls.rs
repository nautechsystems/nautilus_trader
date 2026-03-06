//! Python wrapper functions for Bybit URL helpers.

use pyo3::prelude::*;

use crate::common::{
    enums::{BybitEnvironment, BybitProductType},
    urls,
};

/// Gets the Bybit HTTP base URL for the given environment.
#[pyfunction]
#[pyo3(name = "get_bybit_http_base_url")]
pub fn py_get_bybit_http_base_url(environment: BybitEnvironment) -> &'static str {
    urls::bybit_http_base_url(environment)
}

/// Gets the Bybit WebSocket URL for public data (market data).
#[pyfunction]
#[pyo3(name = "get_bybit_ws_url_public")]
pub fn py_get_bybit_ws_url_public(
    product_type: BybitProductType,
    environment: BybitEnvironment,
) -> String {
    urls::bybit_ws_public_url(product_type, environment)
}

/// Gets the Bybit WebSocket URL for private data (account/order management).
#[pyfunction]
#[pyo3(name = "get_bybit_ws_url_private")]
pub fn py_get_bybit_ws_url_private(environment: BybitEnvironment) -> &'static str {
    urls::bybit_ws_private_url(environment)
}

/// Gets the Bybit WebSocket URL for trade operations (order placement/modification).
#[pyfunction]
#[pyo3(name = "get_bybit_ws_url_trade")]
pub fn py_get_bybit_ws_url_trade(environment: BybitEnvironment) -> &'static str {
    urls::bybit_ws_trade_url(environment)
}
