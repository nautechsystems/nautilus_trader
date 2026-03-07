// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
// -------------------------------------------------------------------------------------------------

use pyo3::prelude::*;

use crate::common::{enums::BitgetEnvironment, urls};

#[pyfunction]
#[pyo3(name = "get_bitget_http_base_url")]
pub fn py_get_bitget_http_base_url(environment: BitgetEnvironment) -> String {
    urls::get_http_base_url(environment).to_string()
}

#[pyfunction]
#[pyo3(name = "get_bitget_ws_public_url")]
pub fn py_get_bitget_ws_public_url(environment: BitgetEnvironment) -> String {
    urls::get_ws_public_url(environment).to_string()
}

#[pyfunction]
#[pyo3(name = "get_bitget_ws_private_url")]
pub fn py_get_bitget_ws_private_url(environment: BitgetEnvironment) -> String {
    urls::get_ws_private_url(environment).to_string()
}
