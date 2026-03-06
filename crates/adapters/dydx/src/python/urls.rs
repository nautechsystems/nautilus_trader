//! Python wrapper functions for dYdX URL helpers.

use pyo3::prelude::*;

use crate::common::consts::{
    DYDX_GRPC_URL, DYDX_GRPC_URLS, DYDX_HTTP_URL, DYDX_TESTNET_GRPC_URL, DYDX_TESTNET_GRPC_URLS,
    DYDX_TESTNET_HTTP_URL, DYDX_TESTNET_WS_URL, DYDX_WS_URL,
};

#[pyfunction]
#[pyo3(name = "get_dydx_grpc_urls")]
#[must_use]
pub fn py_get_dydx_grpc_urls(is_testnet: bool) -> Vec<String> {
    if is_testnet {
        DYDX_TESTNET_GRPC_URLS
            .iter()
            .map(|s| (*s).to_string())
            .collect()
    } else {
        DYDX_GRPC_URLS.iter().map(|s| (*s).to_string()).collect()
    }
}

#[pyfunction]
#[pyo3(name = "get_dydx_grpc_url")]
#[must_use]
pub fn py_get_dydx_grpc_url(is_testnet: bool) -> String {
    if is_testnet {
        DYDX_TESTNET_GRPC_URL.to_string()
    } else {
        DYDX_GRPC_URL.to_string()
    }
}

#[pyfunction]
#[pyo3(name = "get_dydx_http_url")]
#[must_use]
pub fn py_get_dydx_http_url(is_testnet: bool) -> String {
    if is_testnet {
        DYDX_TESTNET_HTTP_URL.to_string()
    } else {
        DYDX_HTTP_URL.to_string()
    }
}

#[pyfunction]
#[pyo3(name = "get_dydx_ws_url")]
#[must_use]
pub fn py_get_dydx_ws_url(is_testnet: bool) -> String {
    if is_testnet {
        DYDX_TESTNET_WS_URL.to_string()
    } else {
        DYDX_WS_URL.to_string()
    }
}
