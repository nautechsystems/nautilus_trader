// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
// -------------------------------------------------------------------------------------------------

pub mod config;
pub mod enums;
pub mod factories;
pub mod http;
pub mod models;
pub mod urls;
pub mod websocket;

use pyo3::prelude::*;

#[pymodule]
#[rustfmt::skip]
pub fn bitget(_: Python<'_>, m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<crate::common::enums::BitgetEnvironment>()?;
    m.add_class::<crate::common::enums::BitgetProductType>()?;
    m.add_class::<crate::common::enums::BitgetInstrumentKind>()?;
    m.add_class::<crate::common::enums::BitgetOrderSide>()?;
    m.add_class::<crate::common::enums::BitgetOrderType>()?;
    m.add_class::<crate::common::enums::BitgetTimeInForce>()?;

    m.add_class::<crate::http::client::BitgetHttpClient>()?;
    m.add_class::<crate::websocket::client::BitgetWebSocketClient>()?;
    m.add_class::<crate::websocket::parse::BitgetBookState>()?;

    m.add_class::<crate::config::BitgetDataClientConfig>()?;
    m.add_class::<crate::config::BitgetExecClientConfig>()?;
    m.add_class::<crate::factories::BitgetDataClientFactory>()?;
    m.add_class::<crate::factories::BitgetExecutionClientFactory>()?;

    m.add_function(wrap_pyfunction!(urls::py_get_bitget_http_base_url, m)?)?;
    m.add_function(wrap_pyfunction!(urls::py_get_bitget_ws_public_url, m)?)?;
    m.add_function(wrap_pyfunction!(urls::py_get_bitget_ws_private_url, m)?)?;

    Ok(())
}
