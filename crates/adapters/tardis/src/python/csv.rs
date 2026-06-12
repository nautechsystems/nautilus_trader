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

use std::{fmt::Debug, path::PathBuf, time::Duration};

use nautilus_core::python::to_pyvalue_err;
use nautilus_model::{
    data::{Data, FundingRateUpdate, OrderBookDelta, OrderBookDepth10, QuoteTick, TradeTick},
    identifiers::InstrumentId,
};
use pyo3::prelude::*;

use crate::csv::{
    convert::{TardisOptionsChainCSVConverterConfig, convert_options_chain_csv},
    load::{
        load_deltas, load_depth10_from_snapshot5, load_depth10_from_snapshot25, load_funding_rates,
        load_options_chain, load_quotes, load_trades,
    },
    stream::{
        stream_batched_deltas, stream_deltas, stream_depth10_from_snapshot5,
        stream_depth10_from_snapshot25, stream_funding_rates, stream_options_chain, stream_quotes,
        stream_trades,
    },
};

macro_rules! impl_tardis_stream_iterator {
    ($struct_name:ident, $data_type:ty, $type_name:expr) => {
        #[pyclass(unsendable)]
        #[pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.adapters.tardis")]
        pub struct $struct_name {
            stream: Box<dyn Iterator<Item = anyhow::Result<Vec<$data_type>>>>,
        }

        impl Debug for $struct_name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{} {{ stream: ... }}", $type_name)
            }
        }

        #[pymethods]
        #[pyo3_stub_gen::derive::gen_stub_pymethods]
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

fn options_chain_data_to_pyobject(py: Python<'_>, data: Data) -> PyResult<Py<PyAny>> {
    match data {
        Data::Quote(quote) => Py::new(py, quote).map(|value| value.into_any()),
        Data::OptionGreeks(greeks) => Py::new(py, greeks).map(|value| value.into_any()),
        data => Err(to_pyvalue_err(format!(
            "Unsupported options_chain data type: {data:?}"
        ))),
    }
}

/// # Errors
///
/// Returns a Python error if loading or parsing the CSV file fails.
#[pyfunction(name = "load_tardis_deltas")]
#[pyo3_stub_gen::derive::gen_stub_pyfunction(module = "nautilus_trader.adapters.tardis")]
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
#[pyo3_stub_gen::derive::gen_stub_pyfunction(module = "nautilus_trader.adapters.tardis")]
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
#[pyo3_stub_gen::derive::gen_stub_pyfunction(module = "nautilus_trader.adapters.tardis")]
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
#[pyo3_stub_gen::derive::gen_stub_pyfunction(module = "nautilus_trader.adapters.tardis")]
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
#[pyo3_stub_gen::derive::gen_stub_pyfunction(module = "nautilus_trader.adapters.tardis")]
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
#[pyo3_stub_gen::derive::gen_stub_pyfunction(module = "nautilus_trader.adapters.tardis")]
#[pyo3(signature = (filepath, instrument_id=None, limit=None))]
pub fn py_load_tardis_funding_rates(
    filepath: PathBuf,
    instrument_id: Option<InstrumentId>,
    limit: Option<usize>,
) -> PyResult<Vec<FundingRateUpdate>> {
    load_funding_rates(filepath, instrument_id, limit).map_err(to_pyvalue_err)
}

/// # Errors
///
/// Returns a Python error if loading or parsing the CSV file fails.
#[pyfunction(name = "load_tardis_options_chain")]
#[pyo3_stub_gen::derive::gen_stub_pyfunction(module = "nautilus_trader.adapters.tardis")]
#[pyo3(signature = (filepath, underlyings=None, price_precision=None, size_precision=None, limit=None))]
pub fn py_load_tardis_options_chain(
    py: Python<'_>,
    filepath: PathBuf,
    underlyings: Option<Vec<String>>,
    price_precision: Option<u8>,
    size_precision: Option<u8>,
    limit: Option<usize>,
) -> PyResult<Vec<Py<PyAny>>> {
    load_options_chain(
        filepath,
        underlyings,
        price_precision,
        size_precision,
        limit,
    )
    .map_err(to_pyvalue_err)?
    .into_iter()
    .map(|data| options_chain_data_to_pyobject(py, data))
    .collect()
}

/// Converts Tardis `options_chain` CSV files into a Nautilus catalog.
///
/// # Errors
///
/// Returns a Python error if parsing, instrument derivation, or catalog writing fails.
#[pyfunction(name = "convert_tardis_options_chain_csv")]
#[pyo3_stub_gen::derive::gen_stub_pyfunction(module = "nautilus_trader.adapters.tardis")]
#[pyo3(signature = (filepaths, catalog_path, underlyings=None, snapshot_interval_ms=None, extract_bbo_as_quotes=true, write_instruments=true, price_precision=None, size_precision=None))]
#[allow(
    clippy::too_many_arguments,
    reason = "PyO3 exposes these keyword arguments as function parameters"
)]
pub fn py_convert_tardis_options_chain_csv(
    py: Python<'_>,
    filepaths: Vec<PathBuf>,
    catalog_path: PathBuf,
    underlyings: Option<Vec<String>>,
    snapshot_interval_ms: Option<u64>,
    extract_bbo_as_quotes: bool,
    write_instruments: bool,
    price_precision: Option<u8>,
    size_precision: Option<u8>,
) -> PyResult<()> {
    let config = TardisOptionsChainCSVConverterConfig {
        filepaths,
        catalog_path,
        underlyings,
        snapshot_interval: snapshot_interval_ms.map(Duration::from_millis),
        extract_bbo_as_quotes,
        write_instruments,
        price_precision,
        size_precision,
    };

    py.detach(|| convert_options_chain_csv(&config))
        .map_err(to_pyvalue_err)
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
#[pyo3_stub_gen::derive::gen_stub_pyfunction(module = "nautilus_trader.adapters.tardis")]
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
#[pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.adapters.tardis")]
pub struct TardisBatchedDeltasStreamIterator {
    stream: Box<dyn Iterator<Item = anyhow::Result<Vec<Py<PyAny>>>>>,
}

impl Debug for TardisBatchedDeltasStreamIterator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "TardisBatchedDeltasStreamIterator {{ stream: ... }}")
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
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
#[pyo3_stub_gen::derive::gen_stub_pyfunction(module = "nautilus_trader.adapters.tardis")]
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
#[pyo3_stub_gen::derive::gen_stub_pyfunction(module = "nautilus_trader.adapters.tardis")]
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

#[pyclass(unsendable)]
#[pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.adapters.tardis")]
pub struct TardisOptionsChainStreamIterator {
    stream: Box<dyn Iterator<Item = anyhow::Result<Vec<Data>>>>,
}

impl Debug for TardisOptionsChainStreamIterator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "TardisOptionsChainStreamIterator {{ stream: ... }}")
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl TardisOptionsChainStreamIterator {
    const fn __iter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }

    fn __next__(&mut self, py: Python<'_>) -> PyResult<Option<Vec<Py<PyAny>>>> {
        match self.stream.next() {
            Some(Ok(chunk)) => chunk
                .into_iter()
                .map(|data| options_chain_data_to_pyobject(py, data))
                .collect::<PyResult<Vec<_>>>()
                .map(Some),
            Some(Err(e)) => Err(to_pyvalue_err(e)),
            None => Ok(None),
        }
    }
}

/// Streams Tardis options chain rows from a CSV file.
///
/// # Errors
///
/// Returns a Python error if loading or parsing the CSV file fails.
#[pyfunction(name = "stream_tardis_options_chain")]
#[pyo3_stub_gen::derive::gen_stub_pyfunction(module = "nautilus_trader.adapters.tardis")]
#[pyo3(signature = (filepath, chunk_size=100_000, underlyings=None, price_precision=None, size_precision=None, limit=None))]
pub fn py_stream_tardis_options_chain(
    filepath: PathBuf,
    chunk_size: usize,
    underlyings: Option<Vec<String>>,
    price_precision: Option<u8>,
    size_precision: Option<u8>,
    limit: Option<usize>,
) -> PyResult<TardisOptionsChainStreamIterator> {
    let stream = stream_options_chain(
        filepath,
        chunk_size,
        underlyings,
        price_precision,
        size_precision,
        limit,
    )
    .map_err(to_pyvalue_err)?;

    Ok(TardisOptionsChainStreamIterator {
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
#[pyo3_stub_gen::derive::gen_stub_pyfunction(module = "nautilus_trader.adapters.tardis")]
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
#[pyo3_stub_gen::derive::gen_stub_pyfunction(module = "nautilus_trader.adapters.tardis")]
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
#[pyo3_stub_gen::derive::gen_stub_pyfunction(module = "nautilus_trader.adapters.tardis")]
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
#[pyo3_stub_gen::derive::gen_stub_pyfunction(module = "nautilus_trader.adapters.tardis")]
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
