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

//! Typed wire models for Massive REST API responses.
//!
//! Prices and sizes deserialize into [`Decimal`] via the workspace
//! `serde-with-float` feature, which preserves the shortest exact decimal
//! representation of each JSON number (e.g. `309.4037` keeps scale 4).

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use ustr::Ustr;

/// Generic paginated response envelope shared by Massive v2/v3 endpoints.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(bound(deserialize = "T: serde::Deserialize<'de>"))]
pub struct MassiveResponse<T> {
    /// Response status (e.g. "OK", "DELAYED", "ERROR").
    #[serde(default)]
    pub status: Option<String>,
    /// Server-assigned request identifier.
    #[serde(default)]
    pub request_id: Option<String>,
    /// Number of results in this page.
    #[serde(default)]
    pub count: Option<u64>,
    /// URL of the next page of results, when more data is available.
    #[serde(default)]
    pub next_url: Option<String>,
    /// Error or informational message (populated on failures).
    #[serde(default)]
    pub message: Option<String>,
    /// The page of results (absent when the query matched nothing).
    #[serde(default)]
    pub results: Option<T>,
}

/// A reference ticker record from `/v3/reference/tickers`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MassiveTickerInfo {
    /// The exchange ticker symbol (e.g. "AAPL", "BRK.A").
    pub ticker: Ustr,
    /// The registered name of the security.
    #[serde(default)]
    pub name: Option<String>,
    /// The market type (expected "stocks").
    #[serde(default)]
    pub market: Option<String>,
    /// The locale of the security (expected "us").
    #[serde(default)]
    pub locale: Option<String>,
    /// ISO code of the primary listing exchange (e.g. "XNAS").
    #[serde(default)]
    pub primary_exchange: Option<Ustr>,
    /// The security type code (e.g. "CS", "ETF", "ADRC").
    #[serde(default, rename = "type")]
    pub ticker_type: Option<String>,
    /// Whether the ticker is actively traded.
    #[serde(default)]
    pub active: Option<bool>,
    /// The trading currency (e.g. "usd").
    #[serde(default)]
    pub currency_name: Option<String>,
    /// The Central Index Key (SEC identifier).
    #[serde(default)]
    pub cik: Option<String>,
    /// The composite OpenFIGI identifier.
    #[serde(default)]
    pub composite_figi: Option<String>,
    /// The share class OpenFIGI identifier.
    #[serde(default)]
    pub share_class_figi: Option<String>,
    /// Round lot size (from ticker details).
    #[serde(default)]
    pub round_lot: Option<Decimal>,
    /// When this record was last updated (RFC 3339).
    #[serde(default)]
    pub last_updated_utc: Option<String>,
}

/// An OHLCV aggregate window from the `/v2/aggs` endpoints.
///
/// Timestamps mark the start of the aggregate window in Unix milliseconds.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MassiveAggBar {
    /// The open price for the window.
    pub o: Decimal,
    /// The highest price for the window.
    pub h: Decimal,
    /// The lowest price for the window.
    pub l: Decimal,
    /// The close price for the window.
    pub c: Decimal,
    /// The trading volume for the window.
    pub v: Decimal,
    /// The volume weighted average price.
    #[serde(default)]
    pub vw: Option<Decimal>,
    /// The start of the aggregate window (Unix milliseconds).
    pub t: i64,
    /// The number of transactions in the window.
    #[serde(default)]
    pub n: Option<u64>,
    /// Whether the window contains only OTC trades.
    #[serde(default)]
    pub otc: Option<bool>,
}

/// A historical trade record from `/v3/trades/{ticker}`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MassiveTrade {
    /// The trade ID, unique per exchange and tape.
    #[serde(default)]
    pub id: Option<String>,
    /// The trade price.
    pub price: Decimal,
    /// The trade size in shares (whole shares).
    #[serde(default)]
    pub size: Option<Decimal>,
    /// The exact fractional trade size, when the trade was fractional.
    #[serde(default)]
    pub decimal_size: Option<Decimal>,
    /// The SIP timestamp (Unix nanoseconds); the canonical event time.
    pub sip_timestamp: i64,
    /// The participant/exchange timestamp (Unix nanoseconds).
    #[serde(default)]
    pub participant_timestamp: Option<i64>,
    /// The exchange ID where the trade occurred.
    #[serde(default)]
    pub exchange: Option<i64>,
    /// The trade condition codes.
    #[serde(default)]
    pub conditions: Option<Vec<i64>>,
    /// The sequence number of the trade on its tape.
    #[serde(default)]
    pub sequence_number: Option<i64>,
    /// The tape identifier (1 = NYSE, 2 = NYSE Arca/Amex, 3 = Nasdaq).
    #[serde(default)]
    pub tape: Option<i64>,
    /// The correction indicator, when the trade was corrected.
    #[serde(default)]
    pub correction: Option<i64>,
}

/// A historical NBBO quote record from `/v3/quotes/{ticker}`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MassiveQuote {
    /// The best bid price.
    #[serde(default)]
    pub bid_price: Option<Decimal>,
    /// The best bid size in shares.
    #[serde(default)]
    pub bid_size: Option<Decimal>,
    /// The exchange ID posting the best bid.
    #[serde(default)]
    pub bid_exchange: Option<i64>,
    /// The best ask price.
    #[serde(default)]
    pub ask_price: Option<Decimal>,
    /// The best ask size in shares.
    #[serde(default)]
    pub ask_size: Option<Decimal>,
    /// The exchange ID posting the best ask.
    #[serde(default)]
    pub ask_exchange: Option<i64>,
    /// The SIP timestamp (Unix nanoseconds); the canonical event time.
    pub sip_timestamp: i64,
    /// The participant/exchange timestamp (Unix nanoseconds).
    #[serde(default)]
    pub participant_timestamp: Option<i64>,
    /// The quote condition codes.
    #[serde(default)]
    pub conditions: Option<Vec<i64>>,
    /// The quote indicator codes.
    #[serde(default)]
    pub indicators: Option<Vec<i64>>,
    /// The sequence number of the quote on its tape.
    #[serde(default)]
    pub sequence_number: Option<i64>,
    /// The tape identifier.
    #[serde(default)]
    pub tape: Option<i64>,
}

/// Response type for `/v3/reference/tickers` (paginated list).
pub type MassiveTickersResponse = MassiveResponse<Vec<MassiveTickerInfo>>;

/// Response type for `/v3/reference/tickers/{ticker}` (single record).
pub type MassiveTickerDetailsResponse = MassiveResponse<MassiveTickerInfo>;

/// Response type for `/v2/aggs/ticker/{ticker}/range/...`.
pub type MassiveAggsResponse = MassiveResponse<Vec<MassiveAggBar>>;

/// Response type for `/v3/trades/{ticker}`.
pub type MassiveTradesResponse = MassiveResponse<Vec<MassiveTrade>>;

/// Response type for `/v3/quotes/{ticker}`.
pub type MassiveQuotesResponse = MassiveResponse<Vec<MassiveQuote>>;

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use rust_decimal_macros::dec;

    use super::*;
    use crate::common::testing::load_test_fixture;

    #[rstest]
    fn test_deserialize_tickers_response() {
        let json = load_test_fixture("http_tickers.json");
        let response: MassiveTickersResponse = serde_json::from_str(&json).unwrap();

        assert_eq!(response.status.as_deref(), Some("OK"));
        assert!(response.next_url.is_some());

        let results = response.results.unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].ticker.as_str(), "AAPL");
        assert_eq!(results[0].ticker_type.as_deref(), Some("CS"));
        assert_eq!(results[0].active, Some(true));
        assert_eq!(results[0].currency_name.as_deref(), Some("usd"));
        assert_eq!(results[1].ticker.as_str(), "BRK.A");
    }

    #[rstest]
    fn test_deserialize_aggs_response() {
        let json = load_test_fixture("http_aggs.json");
        let response: MassiveAggsResponse = serde_json::from_str(&json).unwrap();

        assert_eq!(response.status.as_deref(), Some("OK"));

        let bars = response.results.unwrap();
        assert_eq!(bars.len(), 2);
        assert_eq!(bars[0].o, dec!(74.06));
        assert_eq!(bars[0].c, dec!(75.0875));
        assert_eq!(bars[0].v, dec!(135647456));
        assert_eq!(bars[0].t, 1_577_941_200_000);
        assert_eq!(bars[1].vw, Some(dec!(74.5399)));
    }

    #[rstest]
    fn test_deserialize_aggs_empty_results() {
        let json = r#"{"ticker":"AAPL","queryCount":0,"resultsCount":0,"adjusted":true,"status":"OK","request_id":"abc"}"#;
        let response: MassiveAggsResponse = serde_json::from_str(json).unwrap();
        assert!(response.results.is_none());
    }

    #[rstest]
    fn test_deserialize_trades_response() {
        let json = load_test_fixture("http_trades.json");
        let response: MassiveTradesResponse = serde_json::from_str(&json).unwrap();

        let trades = response.results.unwrap();
        assert_eq!(trades.len(), 2);
        assert_eq!(trades[0].price, dec!(171.55));
        assert_eq!(trades[0].size, Some(dec!(100)));
        assert_eq!(trades[0].sip_timestamp, 1_517_562_000_016_036_600);
        assert_eq!(trades[1].decimal_size, Some(dec!(0.0406)));
    }

    #[rstest]
    fn test_deserialize_quotes_response() {
        let json = load_test_fixture("http_quotes.json");
        let response: MassiveQuotesResponse = serde_json::from_str(&json).unwrap();

        let quotes = response.results.unwrap();
        assert_eq!(quotes.len(), 2);
        assert_eq!(quotes[0].bid_price, Some(dec!(102.7)));
        assert_eq!(quotes[0].ask_price, Some(dec!(102.71)));
        assert_eq!(quotes[0].bid_size, Some(dec!(60)));
        assert_eq!(quotes[1].ask_price, Some(dec!(120.0048)));
    }

    #[rstest]
    fn test_deserialize_error_response() {
        let json = r#"{"status":"ERROR","request_id":"abc","error":"Unknown API Key","message":"Unknown API Key"}"#;
        let response: MassiveTickersResponse = serde_json::from_str(json).unwrap();
        assert_eq!(response.status.as_deref(), Some("ERROR"));
        assert_eq!(response.message.as_deref(), Some("Unknown API Key"));
        assert!(response.results.is_none());
    }
}
