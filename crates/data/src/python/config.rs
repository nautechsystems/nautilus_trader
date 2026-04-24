// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
//
//  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
//  You may not use this file except in compliance with the License.
//  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
//
//  Unless required by applicable law or agreed to in writing, software
//  distributed under the License is distributed on an "AS IS" BASIS,
//  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
//  See the License for the specific language governing permissions and
//  limitations under the License.
// -------------------------------------------------------------------------------------------------

//! Python bindings for data engine configuration.

use std::{collections::HashMap, time::Duration};

use nautilus_core::python::to_pyvalue_err;
use nautilus_model::{
    enums::{BarAggregation, BarIntervalType},
    identifiers::ClientId,
};
use pyo3::{Py, PyAny, PyResult, Python, prelude::PyAnyMethods, pymethods};

use crate::engine::config::DataEngineConfig;

// Coerces a PyO3 input into `BarIntervalType`, accepting both the enum (modern
// Rust surface) and the legacy Python v1 string form
// (`"left-open"` / `"right-open"`), matching `LiveDataEngineConfig`.
fn coerce_bar_interval_type(value: &Py<PyAny>) -> PyResult<BarIntervalType> {
    Python::attach(|py| {
        let bound = value.bind(py);
        if let Ok(variant) = bound.extract::<BarIntervalType>() {
            return Ok(variant);
        }

        let raw = bound.extract::<String>().map_err(|_| {
            to_pyvalue_err("`time_bars_interval_type` must be a string or BarIntervalType")
        })?;

        match raw.to_ascii_uppercase().replace('-', "_").as_str() {
            "LEFT_OPEN" => Ok(BarIntervalType::LeftOpen),
            "RIGHT_OPEN" => Ok(BarIntervalType::RightOpen),
            _ => Err(to_pyvalue_err(format!(
                "invalid `time_bars_interval_type`: {raw:?} (expected 'left-open' or 'right-open')"
            ))),
        }
    })
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl DataEngineConfig {
    /// Configuration for `DataEngine` instances.
    #[new]
    #[expect(clippy::too_many_arguments)]
    #[pyo3(signature = (
        time_bars_build_with_no_updates = None,
        time_bars_timestamp_on_close = None,
        time_bars_skip_first_non_full_bar = None,
        time_bars_interval_type = None,
        time_bars_build_delay = None,
        time_bars_origins = None,
        validate_data_sequence = None,
        buffer_deltas = None,
        emit_quotes_from_book = None,
        emit_quotes_from_book_depths = None,
        external_clients = None,
        debug = None,
    ))]
    fn py_new(
        time_bars_build_with_no_updates: Option<bool>,
        time_bars_timestamp_on_close: Option<bool>,
        time_bars_skip_first_non_full_bar: Option<bool>,
        time_bars_interval_type: Option<Py<PyAny>>,
        time_bars_build_delay: Option<u64>,
        time_bars_origins: Option<HashMap<BarAggregation, u64>>,
        validate_data_sequence: Option<bool>,
        buffer_deltas: Option<bool>,
        emit_quotes_from_book: Option<bool>,
        emit_quotes_from_book_depths: Option<bool>,
        external_clients: Option<Vec<ClientId>>,
        debug: Option<bool>,
    ) -> PyResult<Self> {
        let time_bars_interval_type = match time_bars_interval_type {
            Some(value) => Some(coerce_bar_interval_type(&value)?),
            None => None,
        };
        let time_bars_origins = time_bars_origins.map(|map| {
            map.into_iter()
                .map(|(agg, nanos)| (agg, Duration::from_nanos(nanos)))
                .collect()
        });
        Ok(Self::builder()
            .maybe_time_bars_build_with_no_updates(time_bars_build_with_no_updates)
            .maybe_time_bars_timestamp_on_close(time_bars_timestamp_on_close)
            .maybe_time_bars_skip_first_non_full_bar(time_bars_skip_first_non_full_bar)
            .maybe_time_bars_interval_type(time_bars_interval_type)
            .maybe_time_bars_build_delay(time_bars_build_delay)
            .maybe_time_bars_origins(time_bars_origins)
            .maybe_validate_data_sequence(validate_data_sequence)
            .maybe_buffer_deltas(buffer_deltas)
            .maybe_emit_quotes_from_book(emit_quotes_from_book)
            .maybe_emit_quotes_from_book_depths(emit_quotes_from_book_depths)
            .maybe_external_clients(external_clients)
            .maybe_debug(debug)
            .build())
    }

    #[getter]
    #[pyo3(name = "time_bars_build_with_no_updates")]
    const fn py_time_bars_build_with_no_updates(&self) -> bool {
        self.time_bars_build_with_no_updates
    }

    #[getter]
    #[pyo3(name = "time_bars_timestamp_on_close")]
    const fn py_time_bars_timestamp_on_close(&self) -> bool {
        self.time_bars_timestamp_on_close
    }

    #[getter]
    #[pyo3(name = "time_bars_skip_first_non_full_bar")]
    const fn py_time_bars_skip_first_non_full_bar(&self) -> bool {
        self.time_bars_skip_first_non_full_bar
    }

    #[getter]
    #[pyo3(name = "time_bars_interval_type")]
    const fn py_time_bars_interval_type(&self) -> BarIntervalType {
        self.time_bars_interval_type
    }

    #[getter]
    #[pyo3(name = "time_bars_build_delay")]
    const fn py_time_bars_build_delay(&self) -> u64 {
        self.time_bars_build_delay
    }

    #[getter]
    #[pyo3(name = "validate_data_sequence")]
    const fn py_validate_data_sequence(&self) -> bool {
        self.validate_data_sequence
    }

    #[getter]
    #[pyo3(name = "buffer_deltas")]
    const fn py_buffer_deltas(&self) -> bool {
        self.buffer_deltas
    }

    #[getter]
    #[pyo3(name = "emit_quotes_from_book")]
    const fn py_emit_quotes_from_book(&self) -> bool {
        self.emit_quotes_from_book
    }

    #[getter]
    #[pyo3(name = "emit_quotes_from_book_depths")]
    const fn py_emit_quotes_from_book_depths(&self) -> bool {
        self.emit_quotes_from_book_depths
    }

    #[getter]
    #[pyo3(name = "debug")]
    const fn py_debug(&self) -> bool {
        self.debug
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }

    fn __str__(&self) -> String {
        format!("{self:?}")
    }
}
