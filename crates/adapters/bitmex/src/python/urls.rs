//! Python wrapper functions for BitMEX URL helpers.

use pyo3::prelude::*;

use crate::common::urls;

/// Gets the BitMEX HTTP base URL.
#[pyfunction]
pub fn get_bitmex_http_base_url(testnet: bool) -> String {
    urls::get_http_base_url(testnet)
}

/// Gets the BitMEX WebSocket URL.
#[pyfunction]
pub fn get_bitmex_ws_url(testnet: bool) -> String {
    urls::get_ws_url(testnet)
}
