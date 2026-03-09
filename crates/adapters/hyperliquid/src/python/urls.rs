//! Python bindings for Hyperliquid URL helper functions.

use pyo3::prelude::*;

use crate::common::consts::{info_url, ws_url};

/// Get the HTTP base URL for Hyperliquid API (info endpoint).
///
/// # Returns
///
/// The HTTP base URL string.
#[pyfunction]
#[pyo3(name = "get_hyperliquid_http_base_url")]
pub fn py_get_hyperliquid_http_base_url(is_testnet: bool) -> String {
    info_url(is_testnet).to_string()
}

/// Get the WebSocket URL for Hyperliquid API.
///
/// # Returns
///
/// The WebSocket URL string.
#[pyfunction]
#[pyo3(name = "get_hyperliquid_ws_url")]
pub fn py_get_hyperliquid_ws_url(is_testnet: bool) -> String {
    ws_url(is_testnet).to_string()
}
