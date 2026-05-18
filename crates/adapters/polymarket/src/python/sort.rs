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

//! Python wrappers for Polymarket trade-parsing helpers.

use pyo3::{prelude::*, types::PyDict};

use crate::http::data_api::build_polymarket_trade_id;

/// Returns a deterministic sort key tuple for a Polymarket Data API trade dict.
///
/// The Polymarket Data API timestamps are second-resolution and the public
/// endpoint does not guarantee a stable order across pages. Pass this as the
/// `key` argument to `list.sort` so concatenated pages produce a deterministic
/// chronological stream.
///
/// The returned tuple is `(timestamp, transactionHash, asset, side, price, size)`,
/// with all string-typed fields stringified to match Python's behaviour for
/// dictionaries with mixed-type values. Missing keys default to empty strings,
/// matching `dict.get(key, "")`.
#[pyfunction]
#[pyo3(name = "polymarket_trade_sort_key")]
pub fn py_polymarket_trade_sort_key(
    trade: &Bound<'_, PyDict>,
) -> PyResult<(i64, String, String, String, String, String)> {
    fn extract_string(trade: &Bound<'_, PyDict>, key: &str) -> PyResult<String> {
        match trade.get_item(key)? {
            Some(value) => Ok(value.str()?.extract::<String>()?),
            None => Ok(String::new()),
        }
    }

    let timestamp: i64 = match trade.get_item("timestamp")? {
        Some(value) => value.extract()?,
        None => 0,
    };
    let transaction_hash = extract_string(trade, "transactionHash")?;
    let asset = extract_string(trade, "asset")?;
    let side = extract_string(trade, "side")?;
    let price = extract_string(trade, "price")?;
    let size = extract_string(trade, "size")?;

    Ok((timestamp, transaction_hash, asset, side, price, size))
}

/// Returns the composite Polymarket TradeId for a fill.
///
/// Polygon transactions can settle multiple fills sharing the same
/// `transactionHash`. Using only the last 36 chars collapses them to a single
/// TradeId and downstream catalog readers silently drop duplicates. The id
/// composes a hash suffix, an asset suffix, and a per-(tx, asset) sequence so
/// every fill is preserved.
#[pyfunction]
#[pyo3(name = "polymarket_trade_id")]
pub fn py_polymarket_trade_id(transaction_hash: &str, asset: &str, seq: u32) -> String {
    build_polymarket_trade_id(transaction_hash, asset, seq)
}
