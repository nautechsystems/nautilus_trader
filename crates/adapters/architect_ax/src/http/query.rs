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

use crate::common::enums::AxCandleWidth;

/// Parameters for the GET /ticker endpoint.
///
/// # References
/// - <https://docs.sandbox.x.architect.co/api-reference/marketdata/get-ticker>
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct GetTickerParams {
    /// Instrument symbol, e.g. "GBPUSD-PERP", "EURUSD-PERP".
    pub symbol: String,
}

impl GetTickerParams {
    /// Creates a new [`GetTickerParams`] with the given symbol.
    #[must_use]
    pub fn new(symbol: impl Into<String>) -> Self {
        Self {
            symbol: symbol.into(),
        }
    }
}

/// Parameters for the GET /instrument endpoint.
///
/// # References
/// - <https://docs.sandbox.x.architect.co/api-reference/symbols-instruments/get-instrument>
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct GetInstrumentParams {
    /// Instrument symbol, e.g. "GBPUSD-PERP", "EURUSD-PERP".
    pub symbol: String,
}

impl GetInstrumentParams {
    /// Creates a new [`GetInstrumentParams`] with the given symbol.
    #[must_use]
    pub fn new(symbol: impl Into<String>) -> Self {
        Self {
            symbol: symbol.into(),
        }
    }
}

/// Parameters for the GET /candles endpoint.
///
/// # References
/// - <https://docs.sandbox.x.architect.co/api-reference/marketdata/get-candles>
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct GetCandlesParams {
    /// Instrument symbol.
    pub symbol: String,
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
        symbol: impl Into<String>,
        start_timestamp_ns: i64,
        end_timestamp_ns: i64,
        candle_width: AxCandleWidth,
    ) -> Self {
        Self {
            symbol: symbol.into(),
            start_timestamp_ns,
            end_timestamp_ns,
            candle_width,
        }
    }
}

/// Parameters for the GET /candles/current and GET /candles/last endpoints.
///
/// # References
/// - <https://docs.sandbox.x.architect.co/api-reference/marketdata/get-current-candle>
/// - <https://docs.sandbox.x.architect.co/api-reference/marketdata/get-last-candle>
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct GetCandleParams {
    /// Instrument symbol.
    pub symbol: String,
    /// Candle width/interval.
    pub candle_width: AxCandleWidth,
}

impl GetCandleParams {
    /// Creates a new [`GetCandleParams`].
    #[must_use]
    pub fn new(symbol: impl Into<String>, candle_width: AxCandleWidth) -> Self {
        Self {
            symbol: symbol.into(),
            candle_width,
        }
    }
}

/// Parameters for the GET /funding-rates endpoint.
///
/// # References
/// - <https://docs.sandbox.x.architect.co/api-reference/marketdata/get-funding-rates>
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct GetFundingRatesParams {
    /// Instrument symbol.
    pub symbol: String,
    /// Start timestamp in nanoseconds.
    pub start_timestamp_ns: i64,
    /// End timestamp in nanoseconds.
    pub end_timestamp_ns: i64,
}

impl GetFundingRatesParams {
    /// Creates a new [`GetFundingRatesParams`].
    #[must_use]
    pub fn new(symbol: impl Into<String>, start_timestamp_ns: i64, end_timestamp_ns: i64) -> Self {
        Self {
            symbol: symbol.into(),
            start_timestamp_ns,
            end_timestamp_ns,
        }
    }
}

/// Parameters for the GET /transactions endpoint.
///
/// # References
/// - <https://docs.sandbox.x.architect.co/api-reference/portfolio-management/get-transactions>
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

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_get_ticker_params_serialization() {
        let params = GetTickerParams::new("GBPUSD-PERP");
        let qs = serde_urlencoded::to_string(&params).unwrap();
        assert_eq!(qs, "symbol=GBPUSD-PERP");
    }

    #[rstest]
    fn test_get_instrument_params_serialization() {
        let params = GetInstrumentParams::new("EURUSD-PERP");
        let qs = serde_urlencoded::to_string(&params).unwrap();
        assert_eq!(qs, "symbol=EURUSD-PERP");
    }

    #[rstest]
    fn test_get_candles_params_serialization() {
        let params = GetCandlesParams::new(
            "GBPUSD-PERP",
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
        let params = GetCandleParams::new("GBPUSD-PERP", AxCandleWidth::Hours1);
        let qs = serde_urlencoded::to_string(&params).unwrap();
        assert!(qs.contains("symbol=GBPUSD-PERP"));
        assert!(qs.contains("candle_width=1h"));
    }

    #[rstest]
    fn test_get_funding_rates_params_serialization() {
        let params = GetFundingRatesParams::new("GBPUSD-PERP", 1000000000, 2000000000);
        let qs = serde_urlencoded::to_string(&params).unwrap();
        assert!(qs.contains("symbol=GBPUSD-PERP"));
        assert!(qs.contains("start_timestamp_ns=1000000000"));
        assert!(qs.contains("end_timestamp_ns=2000000000"));
    }
}
