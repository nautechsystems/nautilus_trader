use pyo3::prelude::*;

pub mod enums;
pub mod http;
pub mod websocket;

#[pymodule]
pub fn asterdex2(_py: Python<'_>, m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<http::PyAsterdexHttpClient>()?;
    m.add_class::<websocket::PyAsterdexWebSocketClient>()?;
    m.add_class::<enums::PyAsterdexMarketType>()?;
    m.add_class::<enums::PyAsterdexOrderSide>()?;
    m.add_class::<enums::PyAsterdexOrderType>()?;
    Ok(())
}
