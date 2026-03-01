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

//! HTTP REST model types for the Kalshi API.

use serde::{Deserialize, Serialize};
use ustr::Ustr;

use crate::common::enums::{KalshiMarketStatus, KalshiMarketType, KalshiTakerSide};

// ---------------------------------------------------------------------------
// Instrument discovery
// ---------------------------------------------------------------------------

/// A Kalshi market (binary contract) as returned by `GET /markets`.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct KalshiMarket {
    pub ticker: Ustr,
    pub event_ticker: Ustr,
    pub market_type: KalshiMarketType,
    pub title: String,
    #[serde(default)]
    pub subtitle: String,
    pub yes_sub_title: Option<String>,
    pub no_sub_title: Option<String>,
    /// ISO 8601 open time.
    pub open_time: Option<String>,
    /// ISO 8601 close time.
    pub close_time: Option<String>,
    pub latest_expiration_time: Option<String>,
    pub status: KalshiMarketStatus,
    /// Best YES bid as dollar string (e.g. `"0.4200"`).
    pub yes_bid_dollars: Option<String>,
    pub yes_ask_dollars: Option<String>,
    pub no_bid_dollars: Option<String>,
    pub no_ask_dollars: Option<String>,
    pub last_price_dollars: Option<String>,
    pub volume_fp: Option<String>,
    pub open_interest_fp: Option<String>,
    pub notional_value_dollars: Option<String>,
    pub rules_primary: Option<String>,
}

/// Paginated response from `GET /markets`.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct KalshiMarketsResponse {
    pub markets: Vec<KalshiMarket>,
    pub cursor: Option<String>,
}

// ---------------------------------------------------------------------------
// Orderbook
// ---------------------------------------------------------------------------

/// A single price level: `(price_dollars, count_fp)`.
///
/// Arrays are sorted ascending by price; the **best bid is the last element**.
pub type KalshiPriceLevel = (String, String);

/// Fixed-point orderbook with dollar-string prices.
///
/// Levels are sorted ascending; best bid/offer is at the end of each vec.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct KalshiOrderbookFp {
    /// YES bids sorted ascending.
    pub yes_dollars: Vec<KalshiPriceLevel>,
    /// NO bids sorted ascending.
    pub no_dollars: Vec<KalshiPriceLevel>,
}

/// Response from `GET /markets/{ticker}/orderbook`.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct KalshiOrderbookResponse {
    pub orderbook_fp: KalshiOrderbookFp,
}

// ---------------------------------------------------------------------------
// Trades
// ---------------------------------------------------------------------------

/// A single trade as returned by `GET /markets/trades`.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct KalshiTrade {
    pub trade_id: String,
    pub ticker: Ustr,
    /// YES execution price as dollar string.
    pub yes_price_dollars: String,
    /// NO execution price as dollar string.
    pub no_price_dollars: String,
    /// Contract count with 2 decimal places (e.g. `"136.00"`).
    pub count_fp: String,
    pub taker_side: KalshiTakerSide,
    /// ISO 8601 creation time.
    pub created_time: String,
}

/// Paginated response from `GET /markets/trades`.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct KalshiTradesResponse {
    pub trades: Vec<KalshiTrade>,
    pub cursor: Option<String>,
}

// ---------------------------------------------------------------------------
// Candlesticks (OHLCV)
// ---------------------------------------------------------------------------

/// OHLC price data for one candle side (bid or ask).
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct KalshiOhlc {
    pub open: String,
    pub high: String,
    pub low: String,
    pub close: String,
}

/// OHLC + mean + previous for trade prices.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct KalshiPriceOhlc {
    pub open: Option<String>,
    pub high: Option<String>,
    pub low: Option<String>,
    pub close: Option<String>,
    pub mean: Option<String>,
    pub previous: Option<String>,
}

/// One OHLCV candlestick from `GET /historical/markets/{ticker}/candlesticks`.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct KalshiCandlestick {
    /// Unix timestamp of the period end (seconds).
    pub end_period_ts: u64,
    /// YES bid OHLC.
    pub yes_bid: KalshiOhlc,
    /// YES ask OHLC.
    pub yes_ask: KalshiOhlc,
    /// Trade price OHLC + statistics.
    pub price: KalshiPriceOhlc,
    /// Total contracts traded in this period (2 decimal places).
    pub volume: String,
    pub open_interest: String,
}

/// Response from `GET /historical/markets/{ticker}/candlesticks`.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct KalshiCandlesticksResponse {
    pub candlesticks: Vec<KalshiCandlestick>,
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn load_fixture(name: &str) -> String {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("test_data")
            .join(name);
        std::fs::read_to_string(&path)
            .unwrap_or_else(|_| panic!("missing fixture: {}", path.display()))
    }

    #[test]
    fn test_parse_markets_response() {
        let json = load_fixture("http_markets.json");
        let resp: KalshiMarketsResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(resp.markets.len(), 1);
        let m = &resp.markets[0];
        assert_eq!(m.ticker.as_str(), "KXBTC-25MAR15-B100000");
        assert_eq!(m.event_ticker.as_str(), "KXBTC-25MAR15");
        assert_eq!(m.status, KalshiMarketStatus::Active);
        assert_eq!(m.yes_bid_dollars.as_deref(), Some("0.4200"));
    }

    #[test]
    fn test_parse_orderbook_response() {
        let json = load_fixture("http_orderbook.json");
        let resp: KalshiOrderbookResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(resp.orderbook_fp.yes_dollars.len(), 2);
        // Prices are ascending; best bid is last.
        assert_eq!(resp.orderbook_fp.yes_dollars[1].0, "0.4200");
        assert_eq!(resp.orderbook_fp.yes_dollars[1].1, "13.00");
        assert_eq!(resp.orderbook_fp.no_dollars.len(), 2);
    }

    #[test]
    fn test_parse_trades_response() {
        let json = load_fixture("http_trades.json");
        let resp: KalshiTradesResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(resp.trades.len(), 1);
        let t = &resp.trades[0];
        assert_eq!(t.ticker.as_str(), "KXBTC-25MAR15-B100000");
        assert_eq!(t.yes_price_dollars, "0.3600");
        assert_eq!(t.taker_side, KalshiTakerSide::No);
    }

    #[test]
    fn test_parse_candlesticks_response() {
        let json = load_fixture("http_candlesticks.json");
        let resp: KalshiCandlesticksResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(resp.candlesticks.len(), 1);
        let c = &resp.candlesticks[0];
        assert_eq!(c.end_period_ts, 1741046400);
        assert_eq!(c.price.close.as_deref(), Some("0.4200"));
        assert_eq!(c.volume, "1250.00");
    }
}
