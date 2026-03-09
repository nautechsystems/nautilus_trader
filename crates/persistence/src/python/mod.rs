//! Python bindings from [PyO3](https://pyo3.rs).

pub mod backend;
pub mod catalog;
pub mod feather;
pub mod wranglers;

use pyo3::prelude::*;

/// Loaded as `nautilus_pyo3.persistence`.
///
/// # Errors
///
/// Returns a `PyErr` if registering any module components fails.
#[pymodule]
pub fn persistence(_: Python<'_>, m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<crate::backend::session::DataBackendSession>()?;
    m.add_class::<crate::backend::session::DataQueryResult>()?;
    m.add_class::<backend::session::NautilusDataType>()?;
    m.add_class::<catalog::ParquetDataCatalogV2>()?;
    m.add_class::<feather::StreamingFeatherWriterV2>()?;
    m.add_class::<wranglers::bar::BarDataWrangler>()?;
    m.add_class::<wranglers::delta::OrderBookDeltaDataWrangler>()?;
    m.add_class::<wranglers::depth::OrderBookDepth10DataWrangler>()?;
    m.add_class::<wranglers::quote::QuoteTickDataWrangler>()?;
    m.add_class::<wranglers::trade::TradeTickDataWrangler>()?;
    Ok(())
}
