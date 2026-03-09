//! Python URL helper functions for Deribit.

use pyo3::prelude::*;

use crate::common::consts::{
    DERIBIT_HTTP_URL, DERIBIT_TESTNET_HTTP_URL, DERIBIT_TESTNET_WS_URL, DERIBIT_WS_URL,
};

#[pyfunction]
#[pyo3(name = "get_deribit_http_base_url")]
#[must_use]
pub fn py_get_deribit_http_base_url(is_testnet: bool) -> String {
    if is_testnet {
        DERIBIT_TESTNET_HTTP_URL.to_string()
    } else {
        DERIBIT_HTTP_URL.to_string()
    }
}

#[pyfunction]
#[pyo3(name = "get_deribit_ws_url")]
#[must_use]
pub fn py_get_deribit_ws_url(is_testnet: bool) -> String {
    if is_testnet {
        DERIBIT_TESTNET_WS_URL.to_string()
    } else {
        DERIBIT_WS_URL.to_string()
    }
}
