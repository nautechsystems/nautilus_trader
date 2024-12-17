use std::path::PathBuf;

use nautilus_model::data::bar::Bar;
use nautilus_model::data::delta::OrderBookDelta;
use nautilus_model::data::depth::OrderBookDepth10;
use nautilus_model::data::quote::QuoteTick;
use nautilus_model::data::trade::TradeTick;
use pyo3::prelude::*;

use crate::backend::catalog::ParquetDataCatalog;

/// A catalog for writing data to Parquet files.
#[pyclass(name = "ParquetCatalogV2")]
pub struct PyParquetDataCatalogV2 {
    inner: ParquetDataCatalog,
}

#[pymethods]
impl PyParquetDataCatalogV2 {
    /// Create a new ParquetCatalog with the given base path and optional batch size.
    #[new]
    #[pyo3(signature = (base_path, batch_size=None))]
    pub fn new(base_path: String, batch_size: Option<usize>) -> Self {
        Self {
            inner: ParquetDataCatalog::new(PathBuf::from(base_path), batch_size),
        }
    }

    // TODO: Cannot pass mixed data across pyo3 as a single type
    // pub fn write_data(mut slf: PyRefMut<'_, Self>, data_type: NautilusDataType, data: Vec<Data>) {}

    pub fn write_quote_ticks(&self, data: Vec<QuoteTick>) {
        self.inner.write_to_parquet(data);
    }

    pub fn write_trade_ticks(&self, data: Vec<TradeTick>) {
        self.inner.write_to_parquet(data);
    }

    pub fn write_order_book_deltas(&self, data: Vec<OrderBookDelta>) {
        self.inner.write_to_parquet(data);
    }

    pub fn write_bars(&self, data: Vec<Bar>) {
        self.inner.write_to_parquet(data);
    }

    pub fn write_order_book_depths(&self, data: Vec<OrderBookDepth10>) {
        self.inner.write_to_parquet(data);
    }
}
