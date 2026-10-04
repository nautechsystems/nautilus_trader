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

//! Python bindings for Polymarket trade parsing.

use pyo3::{prelude::*, types::PyDict};

use crate::http::data_api::build_polymarket_trade_id;

/// Returns a deterministic sort key tuple for a Polymarket Data API v2 trade
/// dict.
///
/// The Polymarket Data API timestamps are second-resolution and the public
/// endpoint does not guarantee a stable order across pages. Pass this as the
/// `key` argument to `list.sort` so concatenated pages produce a deterministic
/// chronological stream.
///
/// The returned tuple is `(timestamp, transaction_hash, token_id, side, price, size)`,
/// with all string-typed fields stringified to match Python's behavior for
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
    let transaction_hash = extract_string(trade, "transaction_hash")?;
    let asset = extract_string(trade, "token_id")?;
    let side = extract_string(trade, "side")?;
    let price = extract_string(trade, "price")?;
    let size = extract_string(trade, "size")?;

    Ok((timestamp, transaction_hash, asset, side, price, size))
}

/// Returns the composite Polymarket TradeId for a fill.
///
/// Polygon transactions can settle multiple fills sharing the same
/// `transaction_hash`. Using only the last 36 chars collapses them to a single
/// TradeId and downstream catalog readers silently drop duplicates. The id
/// composes a hash suffix, an asset suffix, and a per-(tx, asset) sequence so
/// every fill is preserved.
#[pyfunction]
#[pyo3(name = "polymarket_trade_id")]
pub fn py_polymarket_trade_id(transaction_hash: &str, asset: &str, seq: u32) -> String {
    build_polymarket_trade_id(transaction_hash, asset, seq)
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_trade_sort_key_reads_v2_fields() {
        Python::initialize();
        Python::attach(|py| {
            let trade = PyDict::new(py);
            trade.set_item("timestamp", 1_710_000_000).unwrap();
            trade.set_item("transaction_hash", "0xabc").unwrap();
            trade.set_item("token_id", "1234token").unwrap();
            trade.set_item("side", "BUY").unwrap();
            trade.set_item("price", "0.55").unwrap();
            trade.set_item("size", "10").unwrap();

            let key = py_polymarket_trade_sort_key(&trade).unwrap();

            assert_eq!(
                key,
                (
                    1_710_000_000,
                    "0xabc".to_string(),
                    "1234token".to_string(),
                    "BUY".to_string(),
                    "0.55".to_string(),
                    "10".to_string(),
                )
            );
        });
    }

    #[rstest]
    fn test_trade_sort_key_ignores_v1_field_names() {
        // v1 camelCase keys must no longer contribute sort components.
        Python::initialize();
        Python::attach(|py| {
            let trade = PyDict::new(py);
            trade.set_item("timestamp", 1).unwrap();
            trade.set_item("transactionHash", "0xabc").unwrap();
            trade.set_item("asset", "1234token").unwrap();

            let key = py_polymarket_trade_sort_key(&trade).unwrap();

            assert_eq!(key.1, String::new());
            assert_eq!(key.2, String::new());
        });
    }
}
