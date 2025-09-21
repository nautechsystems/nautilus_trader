// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

use std::{fmt::Debug, path::PathBuf};

use nautilus_core::python::to_pyvalue_err;
use nautilus_model::{
    data::{FundingRateUpdate, OrderBookDelta, OrderBookDepth10, QuoteTick, TradeTick},
    identifiers::InstrumentId,
};
use pyo3::prelude::*;

use crate::csv::{
    load::{
        load_deltas, load_depth10_from_snapshot5, load_depth10_from_snapshot25, load_funding_rates,
        load_quotes, load_trades,
    },
    stream::{
        stream_batched_deltas, stream_deltas, stream_depth10_from_snapshot5,
        stream_depth10_from_snapshot25, stream_funding_rates, stream_quotes, stream_trades,
    },
};

macro_rules! impl_tardis_stream_iterator {
    ($struct_name:ident, $data_type:ty, $type_name:expr) => {
        #[pyclass(unsendable)]
        pub struct $struct_name {
            stream: Box<dyn Iterator<Item = anyhow::Result<Vec<$data_type>>>>,
        }

        impl Debug for $struct_name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{} {{ stream: ... }}", $type_name)
            }
        }

        #[pymethods]
        impl $struct_name {
            const fn __iter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
                slf
            }

            fn __next__(&mut self) -> PyResult<Option<Vec<$data_type>>> {
                match self.stream.next() {
                    Some(Ok(chunk)) => Ok(Some(chunk)),
                    Some(Err(e)) => Err(to_pyvalue_err(e)),
                    None => Ok(None),
                }
            }
        }
    };
}

/// # Errors
///
/// Returns a Python error if loading or parsing the CSV file fails.
#[pyfunction(name = "load_tardis_deltas")]
#[pyo3(signature = (filepath, price_precision=None, size_precision=None, instrument_id=None, limit=None))]
pub fn py_load_tardis_deltas(
    filepath: PathBuf,
    price_precision: Option<u8>,
    size_precision: Option<u8>,
    instrument_id: Option<InstrumentId>,
    limit: Option<usize>,
) -> PyResult<Vec<OrderBookDelta>> {
    load_deltas(
        filepath,
        price_precision,
        size_precision,
        instrument_id,
        limit,
    )
    .map_err(to_pyvalue_err)
}

/// # Errors
///
/// Returns a Python error if loading or parsing the CSV file fails.
#[pyfunction(name = "load_tardis_depth10_from_snapshot5")]
#[pyo3(signature = (filepath, price_precision=None, size_precision=None, instrument_id=None, limit=None))]
pub fn py_load_tardis_depth10_from_snapshot5(
    filepath: PathBuf,
    price_precision: Option<u8>,
    size_precision: Option<u8>,
    instrument_id: Option<InstrumentId>,
    limit: Option<usize>,
) -> PyResult<Vec<OrderBookDepth10>> {
    load_depth10_from_snapshot5(
        filepath,
        price_precision,
        size_precision,
        instrument_id,
        limit,
    )
    .map_err(to_pyvalue_err)
}

/// # Errors
///
/// Returns a Python error if loading or parsing the CSV file fails.
#[pyfunction(name = "load_tardis_depth10_from_snapshot25")]
#[pyo3(signature = (filepath, price_precision=None, size_precision=None, instrument_id=None, limit=None))]
pub fn py_load_tardis_depth10_from_snapshot25(
    filepath: PathBuf,
    price_precision: Option<u8>,
    size_precision: Option<u8>,
    instrument_id: Option<InstrumentId>,
    limit: Option<usize>,
) -> PyResult<Vec<OrderBookDepth10>> {
    load_depth10_from_snapshot25(
        filepath,
        price_precision,
        size_precision,
        instrument_id,
        limit,
    )
    .map_err(to_pyvalue_err)
}

/// # Errors
///
/// Returns a Python error if loading or parsing the CSV file fails.
#[pyfunction(name = "load_tardis_quotes")]
#[pyo3(signature = (filepath, price_precision=None, size_precision=None, instrument_id=None, limit=None))]
pub fn py_load_tardis_quotes(
    filepath: PathBuf,
    price_precision: Option<u8>,
    size_precision: Option<u8>,
    instrument_id: Option<InstrumentId>,
    limit: Option<usize>,
) -> PyResult<Vec<QuoteTick>> {
    load_quotes(
        filepath,
        price_precision,
        size_precision,
        instrument_id,
        limit,
    )
    .map_err(to_pyvalue_err)
}

/// # Errors
///
/// Returns a Python error if loading or parsing the CSV file fails.
#[pyfunction(name = "load_tardis_trades")]
#[pyo3(signature = (filepath, price_precision=None, size_precision=None, instrument_id=None, limit=None))]
pub fn py_load_tardis_trades(
    filepath: PathBuf,
    price_precision: Option<u8>,
    size_precision: Option<u8>,
    instrument_id: Option<InstrumentId>,
    limit: Option<usize>,
) -> PyResult<Vec<TradeTick>> {
    load_trades(
        filepath,
        price_precision,
        size_precision,
        instrument_id,
        limit,
    )
    .map_err(to_pyvalue_err)
}

/// # Errors
///
/// Returns a Python error if loading or parsing the CSV file fails.
#[pyfunction(name = "load_tardis_funding_rates")]
#[pyo3(signature = (filepath, instrument_id=None, limit=None))]
pub fn py_load_tardis_funding_rates(
    filepath: PathBuf,
    instrument_id: Option<InstrumentId>,
    limit: Option<usize>,
) -> PyResult<Vec<FundingRateUpdate>> {
    load_funding_rates(filepath, instrument_id, limit).map_err(to_pyvalue_err)
}

impl_tardis_stream_iterator!(
    TardisDeltaStreamIterator,
    OrderBookDelta,
    "TardisDeltasStreamIterator"
);

/// Streams order book deltas from a Tardis CSV file.
///
/// # Errors
///
/// Returns a Python error if loading or parsing the CSV file fails.
#[pyfunction(name = "stream_tardis_deltas")]
#[pyo3(signature = (filepath, chunk_size=100_000, price_precision=None, size_precision=None, instrument_id=None, limit=None))]
pub fn py_stream_tardis_deltas(
    filepath: PathBuf,
    chunk_size: usize,
    price_precision: Option<u8>,
    size_precision: Option<u8>,
    instrument_id: Option<InstrumentId>,
    limit: Option<usize>,
) -> PyResult<TardisDeltaStreamIterator> {
    let stream = stream_deltas(
        filepath,
        chunk_size,
        price_precision,
        size_precision,
        instrument_id,
        limit,
    )
    .map_err(to_pyvalue_err)?;

    Ok(TardisDeltaStreamIterator {
        stream: Box::new(stream),
    })
}

#[pyclass(unsendable)]
pub struct TardisBatchedDeltasStreamIterator {
    stream: Box<dyn Iterator<Item = anyhow::Result<Vec<Py<PyAny>>>>>,
}

impl Debug for TardisBatchedDeltasStreamIterator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "TardisBatchedDeltasStreamIterator {{ stream: ... }}")
    }
}

#[pymethods]
impl TardisBatchedDeltasStreamIterator {
    const fn __iter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }

    fn __next__(&mut self) -> PyResult<Option<Vec<Py<PyAny>>>> {
        match self.stream.next() {
            Some(Ok(batch)) => Ok(Some(batch)),
            Some(Err(e)) => Err(to_pyvalue_err(e)),
            None => Ok(None),
        }
    }
}

/// Streams batched order book deltas from a Tardis CSV file.
///
/// # Errors
///
/// Returns a Python error if loading or parsing the CSV file fails.
#[pyfunction(name = "stream_tardis_batched_deltas")]
#[pyo3(signature = (filepath, chunk_size=100_000, price_precision=None, size_precision=None, instrument_id=None, limit=None))]
pub fn py_stream_tardis_batched_deltas(
    filepath: PathBuf,
    chunk_size: usize,
    price_precision: Option<u8>,
    size_precision: Option<u8>,
    instrument_id: Option<InstrumentId>,
    limit: Option<usize>,
) -> PyResult<TardisBatchedDeltasStreamIterator> {
    let stream = stream_batched_deltas(
        filepath,
        chunk_size,
        price_precision,
        size_precision,
        instrument_id,
        limit,
    )
    .map_err(to_pyvalue_err)?;

    Ok(TardisBatchedDeltasStreamIterator {
        stream: Box::new(stream),
    })
}

impl_tardis_stream_iterator!(
    TardisQuoteStreamIterator,
    QuoteTick,
    "TardisQuoteStreamIterator"
);

/// Streams quote ticks from a Tardis CSV file.
///
/// # Errors
///
/// Returns a Python error if loading or parsing the CSV file fails.
#[pyfunction(name = "stream_tardis_quotes")]
#[pyo3(signature = (filepath, chunk_size=100_000, price_precision=None, size_precision=None, instrument_id=None, limit=None))]
pub fn py_stream_tardis_quotes(
    filepath: PathBuf,
    chunk_size: usize,
    price_precision: Option<u8>,
    size_precision: Option<u8>,
    instrument_id: Option<InstrumentId>,
    limit: Option<usize>,
) -> PyResult<TardisQuoteStreamIterator> {
    let stream = stream_quotes(
        filepath,
        chunk_size,
        price_precision,
        size_precision,
        instrument_id,
        limit,
    )
    .map_err(to_pyvalue_err)?;

    Ok(TardisQuoteStreamIterator {
        stream: Box::new(stream),
    })
}

impl_tardis_stream_iterator!(
    TardisTradeStreamIterator,
    TradeTick,
    "TardisTradeStreamIterator"
);

/// Streams trade ticks from a Tardis CSV file.
///
/// # Errors
///
/// Returns a Python error if loading or parsing the CSV file fails.
#[pyfunction(name = "stream_tardis_trades")]
#[pyo3(signature = (filepath, chunk_size=100_000, price_precision=None, size_precision=None, instrument_id=None, limit=None))]
pub fn py_stream_tardis_trades(
    filepath: PathBuf,
    chunk_size: usize,
    price_precision: Option<u8>,
    size_precision: Option<u8>,
    instrument_id: Option<InstrumentId>,
    limit: Option<usize>,
) -> PyResult<TardisTradeStreamIterator> {
    let stream = stream_trades(
        filepath,
        chunk_size,
        price_precision,
        size_precision,
        instrument_id,
        limit,
    )
    .map_err(to_pyvalue_err)?;

    Ok(TardisTradeStreamIterator {
        stream: Box::new(stream),
    })
}

impl_tardis_stream_iterator!(
    TardisDepth10StreamIterator,
    OrderBookDepth10,
    "TardisDepth10StreamIterator"
);

/// Streams order book depth10 from a Tardis snapshot5 CSV file.
///
/// # Errors
///
/// Returns a Python error if loading or parsing the CSV file fails.
#[pyfunction(name = "stream_tardis_depth10_from_snapshot5")]
#[pyo3(signature = (filepath, chunk_size=100_000, price_precision=None, size_precision=None, instrument_id=None, limit=None))]
pub fn py_stream_tardis_depth10_from_snapshot5(
    filepath: PathBuf,
    chunk_size: usize,
    price_precision: Option<u8>,
    size_precision: Option<u8>,
    instrument_id: Option<InstrumentId>,
    limit: Option<usize>,
) -> PyResult<TardisDepth10StreamIterator> {
    let stream = stream_depth10_from_snapshot5(
        filepath,
        chunk_size,
        price_precision,
        size_precision,
        instrument_id,
        limit,
    )
    .map_err(to_pyvalue_err)?;

    Ok(TardisDepth10StreamIterator {
        stream: Box::new(stream),
    })
}

/// Streams order book depth10 from a Tardis snapshot25 CSV file.
///
/// # Errors
///
/// Returns a Python error if loading or parsing the CSV file fails.
#[pyfunction(name = "stream_tardis_depth10_from_snapshot25")]
#[pyo3(signature = (filepath, chunk_size=100_000, price_precision=None, size_precision=None, instrument_id=None, limit=None))]
pub fn py_stream_tardis_depth10_from_snapshot25(
    filepath: PathBuf,
    chunk_size: usize,
    price_precision: Option<u8>,
    size_precision: Option<u8>,
    instrument_id: Option<InstrumentId>,
    limit: Option<usize>,
) -> PyResult<TardisDepth10StreamIterator> {
    let stream = stream_depth10_from_snapshot25(
        filepath,
        chunk_size,
        price_precision,
        size_precision,
        instrument_id,
        limit,
    )
    .map_err(to_pyvalue_err)?;

    Ok(TardisDepth10StreamIterator {
        stream: Box::new(stream),
    })
}

impl_tardis_stream_iterator!(
    TardisFundingRateStreamIterator,
    FundingRateUpdate,
    "TardisFundingRateStreamIterator"
);

/// Streams funding rate updates from a Tardis derivative ticker CSV file.
///
/// # Errors
///
/// Returns a Python error if loading or parsing the CSV file fails.
#[pyfunction(name = "stream_tardis_funding_rates")]
#[pyo3(signature = (filepath, chunk_size=100_000, instrument_id=None, limit=None))]
pub fn py_stream_tardis_funding_rates(
    filepath: PathBuf,
    chunk_size: usize,
    instrument_id: Option<InstrumentId>,
    limit: Option<usize>,
) -> PyResult<TardisFundingRateStreamIterator> {
    let stream =
        stream_funding_rates(filepath, chunk_size, instrument_id, limit).map_err(to_pyvalue_err)?;

    Ok(TardisFundingRateStreamIterator {
        stream: Box::new(stream),
    })
}
