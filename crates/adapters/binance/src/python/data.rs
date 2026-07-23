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

use std::path::{Path, PathBuf};

use anyhow::Context;
use nautilus_core::python::to_pyruntime_err;
use nautilus_model::enums::RecordFlag;
use pyo3::{
    prelude::*,
    types::{PyAny, PyDict},
};
use serde::Deserialize;

/// Loads Binance order book CSV data for wrangling into Nautilus deltas.
///
/// # Errors
///
/// Returns an error if the file cannot be read, a row cannot be parsed, or a side is invalid.
#[pyfunction(name = "load_binance_order_book_deltas", signature = (file_path, nrows=None))]
#[pyo3_stub_gen::derive::gen_stub_pyfunction(module = "nautilus_trader.adapters.binance")]
#[gen_stub(override_return_type(type_repr = "pd.DataFrame", imports = ("pandas as pd",)))]
#[expect(
    clippy::needless_pass_by_value,
    reason = "PyO3 extracts Python path-like values into an owned PathBuf"
)]
pub(super) fn py_load_binance_order_book_deltas<'py>(
    py: Python<'py>,
    file_path: PathBuf,
    nrows: Option<usize>,
) -> PyResult<Bound<'py, PyAny>> {
    let rows = load_rows(&file_path, nrows).map_err(to_pyruntime_err)?;
    rows_to_dataframe(py, &rows)
}

#[derive(Deserialize)]
struct BinanceOrderBookDeltaCsvRow {
    symbol: String,
    timestamp: i64,
    last_update_id: i64,
    side: String,
    update_type: String,
    price: f64,
    qty: f64,
}

struct BinanceOrderBookDeltaRow {
    timestamp: i64,
    instrument_id: String,
    action: &'static str,
    side: &'static str,
    price: f64,
    size: f64,
    order_id: i64,
    flags: i64,
    sequence: i64,
}

fn load_rows(
    file_path: &Path,
    nrows: Option<usize>,
) -> anyhow::Result<Vec<BinanceOrderBookDeltaRow>> {
    let mut reader = csv::Reader::from_path(file_path).with_context(|| {
        format!(
            "failed to open Binance order book CSV {}",
            file_path.display(),
        )
    })?;

    reader
        .deserialize::<BinanceOrderBookDeltaCsvRow>()
        .take(nrows.unwrap_or(usize::MAX))
        .map(|row| map_row(&row?))
        .collect()
}

fn map_row(row: &BinanceOrderBookDeltaCsvRow) -> anyhow::Result<BinanceOrderBookDeltaRow> {
    let side = match row.side.to_ascii_lowercase().as_str() {
        "b" => "BUY",
        "a" => "SELL",
        side => anyhow::bail!("unrecognized side '{side}'"),
    };
    let is_snapshot = row.update_type == "snap";
    let action = if is_snapshot {
        "ADD"
    } else if row.qty == 0.0 {
        "DELETE"
    } else {
        "UPDATE"
    };

    Ok(BinanceOrderBookDeltaRow {
        timestamp: row.timestamp,
        instrument_id: format!("{}.BINANCE", row.symbol),
        action,
        side,
        price: row.price,
        size: row.qty,
        order_id: 0,
        flags: if is_snapshot {
            RecordFlag::F_SNAPSHOT as i64
        } else {
            0
        },
        sequence: row.last_update_id,
    })
}

fn rows_to_dataframe<'py>(
    py: Python<'py>,
    rows: &[BinanceOrderBookDeltaRow],
) -> PyResult<Bound<'py, PyAny>> {
    let pandas = py.import("pandas")?;
    let timestamps = rows.iter().map(|row| row.timestamp).collect::<Vec<_>>();
    let datetime_kwargs = PyDict::new(py);
    datetime_kwargs.set_item("unit", "ms")?;
    datetime_kwargs.set_item("utc", true)?;
    let timestamps = pandas.call_method("to_datetime", (timestamps,), Some(&datetime_kwargs))?;

    let data = PyDict::new(py);
    data.set_item("timestamp", timestamps)?;
    data.set_item(
        "instrument_id",
        rows.iter()
            .map(|row| row.instrument_id.as_str())
            .collect::<Vec<_>>(),
    )?;
    data.set_item(
        "action",
        rows.iter().map(|row| row.action).collect::<Vec<_>>(),
    )?;
    data.set_item("side", rows.iter().map(|row| row.side).collect::<Vec<_>>())?;
    data.set_item(
        "price",
        rows.iter().map(|row| row.price).collect::<Vec<_>>(),
    )?;
    data.set_item("size", rows.iter().map(|row| row.size).collect::<Vec<_>>())?;
    data.set_item(
        "order_id",
        rows.iter().map(|row| row.order_id).collect::<Vec<_>>(),
    )?;
    data.set_item(
        "flags",
        rows.iter().map(|row| row.flags).collect::<Vec<_>>(),
    )?;
    data.set_item(
        "sequence",
        rows.iter().map(|row| row.sequence).collect::<Vec<_>>(),
    )?;

    pandas
        .getattr("DataFrame")?
        .call1((data,))?
        .call_method1("set_index", ("timestamp",))
}
