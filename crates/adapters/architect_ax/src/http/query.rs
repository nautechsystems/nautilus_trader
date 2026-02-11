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

//! Request parameter structures for the Ax REST API.
//!
//! Each struct corresponds to an Ax REST endpoint and is annotated
//! using `serde` so that it can be serialized directly into the query string
//! or request body expected by the exchange.
//!
//! Parameter structs are built using the builder pattern and then passed to
//! `AxRawHttpClient` methods where they are automatically serialized.

use serde::{Deserialize, Serialize};
use ustr::Ustr;

use crate::common::enums::{AxCandleWidth, AxOrderStatus};

/// Parameters for the GET /ticker endpoint.
///
/// # References
/// - <https://docs.architect.exchange/api-reference/marketdata/get-ticker>
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct GetTickerParams {
    /// Instrument symbol, e.g. "GBPUSD-PERP", "EURUSD-PERP".
    pub symbol: Ustr,
}

impl GetTickerParams {
    /// Creates a new [`GetTickerParams`] with the given symbol.
    #[must_use]
    pub fn new(symbol: Ustr) -> Self {
        Self { symbol }
    }
}

/// Parameters for the GET /instrument endpoint.
///
/// # References
/// - <https://docs.architect.exchange/api-reference/symbols-instruments/get-instrument>
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct GetInstrumentParams {
    /// Instrument symbol, e.g. "GBPUSD-PERP", "EURUSD-PERP".
    pub symbol: Ustr,
}

impl GetInstrumentParams {
    /// Creates a new [`GetInstrumentParams`] with the given symbol.
    #[must_use]
    pub fn new(symbol: Ustr) -> Self {
        Self { symbol }
    }
}

/// Parameters for the GET /candles endpoint.
///
/// # References
/// - <https://docs.architect.exchange/api-reference/marketdata/get-candles>
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct GetCandlesParams {
    /// Instrument symbol.
    pub symbol: Ustr,
    /// Start timestamp in nanoseconds.
    pub start_timestamp_ns: i64,
    /// End timestamp in nanoseconds.
    pub end_timestamp_ns: i64,
    /// Candle width/interval.
    pub candle_width: AxCandleWidth,
}

impl GetCandlesParams {
    /// Creates a new [`GetCandlesParams`].
    #[must_use]
    pub fn new(
        symbol: Ustr,
        start_timestamp_ns: i64,
        end_timestamp_ns: i64,
        candle_width: AxCandleWidth,
    ) -> Self {
        Self {
            symbol,
            start_timestamp_ns,
            end_timestamp_ns,
            candle_width,
        }
    }
}

/// Parameters for the GET /candles/current and GET /candles/last endpoints.
///
/// # References
/// - <https://docs.architect.exchange/api-reference/marketdata/get-current-candle>
/// - <https://docs.architect.exchange/api-reference/marketdata/get-last-candle>
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct GetCandleParams {
    /// Instrument symbol.
    pub symbol: Ustr,
    /// Candle width/interval.
    pub candle_width: AxCandleWidth,
}

impl GetCandleParams {
    /// Creates a new [`GetCandleParams`].
    #[must_use]
    pub fn new(symbol: Ustr, candle_width: AxCandleWidth) -> Self {
        Self {
            symbol,
            candle_width,
        }
    }
}

/// Parameters for the GET /funding-rates endpoint.
///
/// # References
/// - <https://docs.architect.exchange/api-reference/marketdata/get-funding-rates>
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct GetFundingRatesParams {
    /// Instrument symbol.
    pub symbol: Ustr,
    /// Start timestamp in nanoseconds.
    pub start_timestamp_ns: i64,
    /// End timestamp in nanoseconds.
    pub end_timestamp_ns: i64,
}

impl GetFundingRatesParams {
    /// Creates a new [`GetFundingRatesParams`].
    #[must_use]
    pub fn new(symbol: Ustr, start_timestamp_ns: i64, end_timestamp_ns: i64) -> Self {
        Self {
            symbol,
            start_timestamp_ns,
            end_timestamp_ns,
        }
    }
}

/// Parameters for the GET /transactions endpoint.
///
/// # References
/// - <https://docs.architect.exchange/api-reference/portfolio-management/get-transactions>
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct GetTransactionsParams {
    /// Transaction types to filter by.
    pub transaction_types: Vec<String>,
}

impl GetTransactionsParams {
    /// Creates a new [`GetTransactionsParams`].
    #[must_use]
    pub fn new(transaction_types: Vec<String>) -> Self {
        Self { transaction_types }
    }
}

/// Parameters for the GET /trades endpoint.
///
/// # References
/// - <https://docs.architect.exchange/api-reference/market-data/get-trades>
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct GetTradesParams {
    /// Instrument symbol, e.g. "BTC-PERP".
    pub symbol: Ustr,
    /// Maximum number of trades to return (max 100, default 10).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<i32>,
}

impl GetTradesParams {
    /// Creates a new [`GetTradesParams`].
    #[must_use]
    pub fn new(symbol: Ustr, limit: Option<i32>) -> Self {
        Self { symbol, limit }
    }
}

/// Parameters for the GET /book endpoint.
///
/// # References
/// - <https://docs.architect.exchange/api-reference/market-data/get-book>
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct GetBookParams {
    /// Instrument symbol, e.g. "BTC-PERP".
    pub symbol: Ustr,
    /// Book depth level: 2 (aggregated) or 3 (individual orders). Defaults to 2.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub level: Option<i32>,
}

impl GetBookParams {
    /// Creates a new [`GetBookParams`].
    #[must_use]
    pub fn new(symbol: Ustr, level: Option<i32>) -> Self {
        Self { symbol, level }
    }
}

/// Parameters for the GET /order-status endpoint.
///
/// Exactly one of `order_id` or `client_order_id` must be provided.
///
/// # References
/// - <https://docs.architect.exchange/api-reference/order-management/get-order-status>
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct GetOrderStatusParams {
    /// Order ID (e.g. "O-01ARZ3NDEKTSV4RRFFQ69G5FAV").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub order_id: Option<String>,
    /// Client order ID (64-bit integer).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_order_id: Option<u64>,
}

impl GetOrderStatusParams {
    /// Creates params to look up by venue order ID.
    #[must_use]
    pub fn by_order_id(order_id: impl Into<String>) -> Self {
        Self {
            order_id: Some(order_id.into()),
            client_order_id: None,
        }
    }

    /// Creates params to look up by client order ID.
    #[must_use]
    pub fn by_client_order_id(cid: u64) -> Self {
        Self {
            order_id: None,
            client_order_id: Some(cid),
        }
    }
}

/// Parameters for the GET /orders endpoint.
///
/// # References
/// - <https://docs.architect.exchange/api-reference/order-management/get-orders>
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct GetOrdersParams {
    /// Filter by trading symbol.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbol: Option<Ustr>,
    /// Beginning of time range (ISO 8601).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_time: Option<String>,
    /// End of time range (ISO 8601).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_time: Option<String>,
    /// Maximum results returned.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<i32>,
    /// Pagination offset.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub offset: Option<i32>,
    /// Filter by order state.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub order_state: Option<AxOrderStatus>,
}

impl GetOrdersParams {
    /// Creates a new empty [`GetOrdersParams`].
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use ustr::Ustr;

    use super::*;

    #[rstest]
    fn test_get_ticker_params_serialization() {
        let params = GetTickerParams::new(Ustr::from("GBPUSD-PERP"));
        let qs = serde_urlencoded::to_string(&params).unwrap();
        assert_eq!(qs, "symbol=GBPUSD-PERP");
    }

    #[rstest]
    fn test_get_instrument_params_serialization() {
        let params = GetInstrumentParams::new(Ustr::from("EURUSD-PERP"));
        let qs = serde_urlencoded::to_string(&params).unwrap();
        assert_eq!(qs, "symbol=EURUSD-PERP");
    }

    #[rstest]
    fn test_get_candles_params_serialization() {
        let params = GetCandlesParams::new(
            Ustr::from("GBPUSD-PERP"),
            1000000000,
            2000000000,
            AxCandleWidth::Minutes1,
        );
        let qs = serde_urlencoded::to_string(&params).unwrap();
        assert!(qs.contains("symbol=GBPUSD-PERP"));
        assert!(qs.contains("start_timestamp_ns=1000000000"));
        assert!(qs.contains("end_timestamp_ns=2000000000"));
        assert!(qs.contains("candle_width=1m"));
    }

    #[rstest]
    fn test_get_candle_params_serialization() {
        let params = GetCandleParams::new(Ustr::from("GBPUSD-PERP"), AxCandleWidth::Hours1);
        let qs = serde_urlencoded::to_string(&params).unwrap();
        assert!(qs.contains("symbol=GBPUSD-PERP"));
        assert!(qs.contains("candle_width=1h"));
    }

    #[rstest]
    fn test_get_funding_rates_params_serialization() {
        let params = GetFundingRatesParams::new(Ustr::from("GBPUSD-PERP"), 1000000000, 2000000000);
        let qs = serde_urlencoded::to_string(&params).unwrap();
        assert!(qs.contains("symbol=GBPUSD-PERP"));
        assert!(qs.contains("start_timestamp_ns=1000000000"));
        assert!(qs.contains("end_timestamp_ns=2000000000"));
    }

    #[rstest]
    fn test_get_trades_params_serialization() {
        let params = GetTradesParams::new(Ustr::from("BTC-PERP"), Some(50));
        let qs = serde_urlencoded::to_string(&params).unwrap();
        assert!(qs.contains("symbol=BTC-PERP"));
        assert!(qs.contains("limit=50"));
    }

    #[rstest]
    fn test_get_trades_params_serialization_no_limit() {
        let params = GetTradesParams::new(Ustr::from("BTC-PERP"), None);
        let qs = serde_urlencoded::to_string(&params).unwrap();
        assert_eq!(qs, "symbol=BTC-PERP");
    }

    #[rstest]
    fn test_get_book_params_serialization() {
        let params = GetBookParams::new(Ustr::from("EURUSD-PERP"), Some(3));
        let qs = serde_urlencoded::to_string(&params).unwrap();
        assert!(qs.contains("symbol=EURUSD-PERP"));
        assert!(qs.contains("level=3"));
    }

    #[rstest]
    fn test_get_order_status_by_order_id_serialization() {
        let params = GetOrderStatusParams::by_order_id("O-01ARZ3NDEKTSV4RRFFQ69G5FAV");
        let qs = serde_urlencoded::to_string(&params).unwrap();
        assert!(qs.contains("order_id=O-01ARZ3NDEKTSV4RRFFQ69G5FAV"));
        assert!(!qs.contains("client_order_id"));
    }

    #[rstest]
    fn test_get_order_status_by_client_order_id_serialization() {
        let params = GetOrderStatusParams::by_client_order_id(12345);
        let qs = serde_urlencoded::to_string(&params).unwrap();
        assert_eq!(qs, "client_order_id=12345");
    }
}
