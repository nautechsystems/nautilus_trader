// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
//
//  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
//  You may not use this file except in compliance with the License.
//  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
// -------------------------------------------------------------------------------------------------

//! Arrow helpers for Python-defined Hyperliquid custom data streams.

use nautilus_core::python::to_pyvalue_err;
use nautilus_serialization::{
    arrow::EncodeToRecordBatch, python::arrow::arrow_record_batch_to_pybytes,
};
use pyo3::{prelude::*, types::PyBytes};

use crate::data_types::HyperliquidPublicTrade;

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl HyperliquidPublicTrade {
    /// Encodes public Hyperliquid trades into Arrow IPC bytes for streaming persistence.
    ///
    /// # Errors
    ///
    /// Returns an error if no data is provided or Arrow encoding fails.
    #[staticmethod]
    #[expect(clippy::needless_pass_by_value)]
    fn to_arrow_record_batch_bytes(py: Python<'_>, data: Vec<Self>) -> PyResult<Py<PyBytes>> {
        let first = data
            .first()
            .ok_or_else(|| to_pyvalue_err("Cannot encode an empty HyperliquidPublicTrade batch"))?;
        let metadata = Self::metadata(first);
        let batch = Self::encode_batch(&metadata, &data).map_err(to_pyvalue_err)?;
        arrow_record_batch_to_pybytes(py, &batch)
    }
}
