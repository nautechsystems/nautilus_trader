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

use std::path::PathBuf;

use nautilus_core::python::to_pyvalue_err;
use nautilus_model::{
    data::{OrderBookDelta, OrderBookDepth10, QuoteTick, TradeTick},
    identifiers::InstrumentId,
};
use pyo3::prelude::*;

use crate::csv::{
    load_deltas, load_depth10_from_snapshot5, load_depth10_from_snapshot25, load_quote_ticks,
    load_trade_ticks,
};

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

#[pyfunction(name = "load_tardis_quotes")]
#[pyo3(signature = (filepath, price_precision=None, size_precision=None, instrument_id=None, limit=None))]
pub fn py_load_tardis_quotes(
    filepath: PathBuf,
    price_precision: Option<u8>,
    size_precision: Option<u8>,
    instrument_id: Option<InstrumentId>,
    limit: Option<usize>,
) -> PyResult<Vec<QuoteTick>> {
    load_quote_ticks(
        filepath,
        price_precision,
        size_precision,
        instrument_id,
        limit,
    )
    .map_err(to_pyvalue_err)
}

#[pyfunction(name = "load_tardis_trades")]
#[pyo3(signature = (filepath, price_precision=None, size_precision=None, instrument_id=None, limit=None))]
pub fn py_load_tardis_trades(
    filepath: PathBuf,
    price_precision: Option<u8>,
    size_precision: Option<u8>,
    instrument_id: Option<InstrumentId>,
    limit: Option<usize>,
) -> PyResult<Vec<TradeTick>> {
    load_trade_ticks(
        filepath,
        price_precision,
        size_precision,
        instrument_id,
        limit,
    )
    .map_err(to_pyvalue_err)
}
