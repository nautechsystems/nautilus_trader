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

//! HTTP REST model types for the Polymarket CLOB API.

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use ustr::Ustr;

use crate::common::{
    enums::{
        PolymarketLiquiditySide, PolymarketOrderSide, PolymarketOrderStatus, PolymarketOrderType,
        PolymarketOutcome, PolymarketTradeStatus, SignatureType,
    },
    models::PolymarketMakerOrder,
    parse::{deserialize_decimal_from_str, serialize_decimal_as_str},
};

/// A signed limit order for submission to the CLOB exchange.
///
/// References: <https://docs.polymarket.com/#create-and-place-an-order>
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PolymarketOrder {
    pub salt: u64,
    pub maker: String,
    pub signer: String,
    pub taker: String,
    pub token_id: Ustr,
    #[serde(
        serialize_with = "serialize_decimal_as_str",
        deserialize_with = "deserialize_decimal_from_str"
    )]
    pub maker_amount: Decimal,
    #[serde(
        serialize_with = "serialize_decimal_as_str",
        deserialize_with = "deserialize_decimal_from_str"
    )]
    pub taker_amount: Decimal,
    pub expiration: String,
    pub nonce: String,
    #[serde(
        serialize_with = "serialize_decimal_as_str",
        deserialize_with = "deserialize_decimal_from_str"
    )]
    pub fee_rate_bps: Decimal,
    pub side: PolymarketOrderSide,
    pub signature_type: SignatureType,
    pub signature: String,
}

/// An active order returned by REST GET /orders.
///
/// References: <https://docs.polymarket.com/#get-orders>
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolymarketOpenOrder {
    pub associate_trades: Option<Vec<String>>,
    pub id: String,
    pub status: PolymarketOrderStatus,
    pub market: Ustr,
    #[serde(
        serialize_with = "serialize_decimal_as_str",
        deserialize_with = "deserialize_decimal_from_str"
    )]
    pub original_size: Decimal,
    pub outcome: PolymarketOutcome,
    pub maker_address: String,
    pub owner: String,
    #[serde(
        serialize_with = "serialize_decimal_as_str",
        deserialize_with = "deserialize_decimal_from_str"
    )]
    pub price: Decimal,
    pub side: PolymarketOrderSide,
    #[serde(
        serialize_with = "serialize_decimal_as_str",
        deserialize_with = "deserialize_decimal_from_str"
    )]
    pub size_matched: Decimal,
    pub asset_id: Ustr,
    pub expiration: Option<String>,
    pub order_type: PolymarketOrderType,
    pub created_at: u64,
}

/// A trade report returned by REST GET /trades.
///
/// References: <https://docs.polymarket.com/#get-trades>
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolymarketTradeReport {
    pub id: String,
    pub taker_order_id: String,
    pub market: Ustr,
    pub asset_id: Ustr,
    pub side: PolymarketOrderSide,
    #[serde(
        serialize_with = "serialize_decimal_as_str",
        deserialize_with = "deserialize_decimal_from_str"
    )]
    pub size: Decimal,
    #[serde(
        serialize_with = "serialize_decimal_as_str",
        deserialize_with = "deserialize_decimal_from_str"
    )]
    pub fee_rate_bps: Decimal,
    #[serde(
        serialize_with = "serialize_decimal_as_str",
        deserialize_with = "deserialize_decimal_from_str"
    )]
    pub price: Decimal,
    pub status: PolymarketTradeStatus,
    pub match_time: String,
    pub last_update: String,
    pub outcome: PolymarketOutcome,
    pub bucket_index: u64,
    pub owner: String,
    pub maker_address: String,
    pub transaction_hash: String,
    pub maker_orders: Vec<PolymarketMakerOrder>,
    pub trader_side: PolymarketLiquiditySide,
}

/// A market response from the Gamma API `GET /markets`.
///
/// References: <https://docs.polymarket.com/developers/gamma-markets-api/get-markets>
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GammaMarket {
    /// Internal Gamma market ID.
    pub id: String,
    /// On-chain condition ID for the CTF contracts.
    pub condition_id: String,
    /// Hash used for resolution.
    #[serde(rename = "questionID")]
    pub question_id: Option<String>,
    /// JSON-encoded array of two CLOB token IDs (Yes, No).
    #[serde(default)]
    pub clob_token_ids: String,
    /// JSON-encoded outcome labels (e.g. `["Yes", "No"]`).
    #[serde(default)]
    pub outcomes: String,
    /// Market question/title.
    pub question: String,
    /// Detailed description.
    pub description: Option<String>,
    /// Market start date (ISO 8601).
    pub start_date: Option<String>,
    /// Market end date (ISO 8601).
    pub end_date: Option<String>,
    /// Whether market is active.
    pub active: Option<bool>,
    /// Whether market is closed.
    pub closed: Option<bool>,
    /// Whether CLOB is accepting orders.
    pub accepting_orders: Option<bool>,
    /// Whether order book trading is enabled.
    pub enable_order_book: Option<bool>,
    /// Minimum price increment.
    pub order_price_min_tick_size: Option<f64>,
    /// Minimum order size.
    pub order_min_size: Option<f64>,
    /// Maker fee in basis points.
    pub maker_base_fee: Option<i64>,
    /// Taker fee in basis points.
    pub taker_base_fee: Option<i64>,
    /// URL slug.
    #[serde(rename = "slug")]
    pub market_slug: Option<String>,
    /// Whether the market uses neg-risk CTF exchange.
    #[serde(rename = "negRisk")]
    pub neg_risk: Option<bool>,
    /// Numeric liquidity value for sorting.
    pub liquidity_num: Option<f64>,
    /// Numeric volume value for sorting.
    pub volume_num: Option<f64>,
    /// 24-hour trading volume.
    #[serde(rename = "volume24hr")]
    pub volume_24hr: Option<f64>,
    /// JSON-encoded outcome prices (e.g. `["0.60", "0.40"]`).
    pub outcome_prices: Option<String>,
    /// Best bid price.
    pub best_bid: Option<f64>,
    /// Best ask price.
    pub best_ask: Option<f64>,
    /// Bid-ask spread.
    pub spread: Option<f64>,
    /// Last trade price.
    pub last_trade_price: Option<f64>,
    /// 1-day price change.
    pub one_day_price_change: Option<f64>,
    /// 1-week price change.
    pub one_week_price_change: Option<f64>,
    /// 1-week volume.
    #[serde(rename = "volume1wk")]
    pub volume_1wk: Option<f64>,
    /// 1-month volume.
    #[serde(rename = "volume1mo")]
    pub volume_1mo: Option<f64>,
    /// 1-year volume.
    #[serde(rename = "volume1yr")]
    pub volume_1yr: Option<f64>,
    /// Minimum size for rewards eligibility.
    pub rewards_min_size: Option<f64>,
    /// Maximum spread for rewards eligibility.
    pub rewards_max_spread: Option<f64>,
    /// Competitiveness score.
    pub competitive: Option<f64>,
    /// Market category.
    pub category: Option<String>,
    /// Neg-risk market ID for CTF exchange interaction.
    #[serde(rename = "negRiskMarketID")]
    pub neg_risk_market_id: Option<String>,
}

/// An event response from the Gamma API `GET /events`.
///
/// Events are parent containers grouping related markets (e.g., an election
/// event contains multiple outcome markets). Each event's `markets` array
/// contains full [`GammaMarket`] objects.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GammaEvent {
    pub id: String,
    pub slug: Option<String>,
    pub title: Option<String>,
    pub description: Option<String>,
    pub start_date: Option<String>,
    pub end_date: Option<String>,
    pub active: Option<bool>,
    pub closed: Option<bool>,
    pub archived: Option<bool>,
    #[serde(default)]
    pub markets: Vec<GammaMarket>,
    /// Event-level liquidity.
    pub liquidity: Option<f64>,
    /// Event-level volume.
    pub volume: Option<f64>,
    /// Event-level open interest.
    pub open_interest: Option<f64>,
    /// 24-hour event volume.
    #[serde(rename = "volume24hr")]
    pub volume_24hr: Option<f64>,
    /// Event category.
    pub category: Option<String>,
    /// Whether event uses neg-risk.
    pub neg_risk: Option<bool>,
    /// Neg-risk market ID.
    #[serde(rename = "negRiskMarketID")]
    pub neg_risk_market_id: Option<String>,
    /// Whether event is featured.
    pub featured: Option<bool>,
}

/// A tag from the Gamma API `GET /tags`.
#[derive(Clone, Debug, Deserialize)]
pub struct GammaTag {
    /// Tag identifier.
    pub id: String,
    /// Human-readable label.
    pub label: Option<String>,
    /// URL slug.
    pub slug: Option<String>,
}

/// Response from the Gamma API `GET /public-search`.
#[derive(Clone, Debug, Deserialize)]
pub struct SearchResponse {
    /// Matching markets.
    #[serde(default)]
    pub markets: Option<Vec<GammaMarket>>,
    /// Matching events.
    #[serde(default)]
    pub events: Option<Vec<GammaEvent>>,
}

/// Tick size response from CLOB `GET /tick-size`.
///
/// References: <https://docs.polymarket.com/api-reference/market-data/get-tick-size>
#[derive(Clone, Debug, Deserialize)]
pub struct TickSizeResponse {
    /// Minimum tick size (price increment) for a token.
    pub minimum_tick_size: f64,
}

/// Fee rate response from CLOB `GET /fee-rate`.
///
/// Returns the taker fee rate in basis points for a given token.
#[derive(Clone, Debug, Deserialize)]
pub struct FeeRateResponse {
    /// Fee rate in basis points.
    pub base_fee: Decimal,
}

/// A single price level from the CLOB order book.
#[derive(Clone, Debug, Deserialize)]
pub struct ClobBookLevel {
    pub price: String,
    pub size: String,
}

/// Response from the CLOB `GET /book` endpoint.
///
/// Extra fields (`market`, `asset_id`, `hash`, `timestamp`) are silently ignored.
#[derive(Clone, Debug, Deserialize)]
pub struct ClobBookResponse {
    pub bids: Vec<ClobBookLevel>,
    pub asks: Vec<ClobBookLevel>,
}

/// A position from the Polymarket Data API `GET /positions` endpoint.
#[derive(Clone, Debug, Deserialize)]
pub struct DataApiPosition {
    pub asset: String,
    #[serde(alias = "conditionId", alias = "condition_id")]
    pub condition_id: String,
    pub size: f64,
    #[serde(alias = "avgPrice", alias = "avg_price")]
    pub avg_price: Option<f64>,
}

/// A trade from the Polymarket Data API `GET /trades` endpoint.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DataApiTrade {
    pub asset: String,
    pub condition_id: String,
    pub side: PolymarketOrderSide,
    pub price: f64,
    pub size: f64,
    pub timestamp: i64,
    pub transaction_hash: String,
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use rust_decimal_macros::dec;

    use super::*;
    use crate::common::enums::{PolymarketOrderStatus, PolymarketTradeStatus, SignatureType};

    fn load<T: serde::de::DeserializeOwned>(filename: &str) -> T {
        let path = format!("test_data/{filename}");
        let content = std::fs::read_to_string(path).expect("Failed to read test data");
        serde_json::from_str(&content).expect("Failed to parse test data")
    }

    #[rstest]
    fn test_open_order_live_buy_gtc() {
        let order: PolymarketOpenOrder = load("http_open_order.json");

        assert_eq!(
            order.id,
            "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef12"
        );
        assert_eq!(order.status, PolymarketOrderStatus::Live);
        assert_eq!(order.side, PolymarketOrderSide::Buy);
        assert_eq!(order.order_type, PolymarketOrderType::GTC);
        assert_eq!(order.outcome, PolymarketOutcome::yes());
        assert_eq!(order.original_size, dec!(100.0000));
        assert_eq!(order.price, dec!(0.5000));
        assert_eq!(order.size_matched, dec!(25.0000));
        assert_eq!(order.created_at, 1703875200);
        assert!(order.expiration.is_none());
        assert_eq!(order.associate_trades, Some(vec!["0xabc001".to_string()]));
    }

    #[rstest]
    fn test_open_order_matched_sell_fok() {
        let order: PolymarketOpenOrder = load("http_open_order_sell_fok.json");

        assert_eq!(order.status, PolymarketOrderStatus::Matched);
        assert_eq!(order.side, PolymarketOrderSide::Sell);
        assert_eq!(order.order_type, PolymarketOrderType::FOK);
        assert_eq!(order.outcome, PolymarketOutcome::no());
        assert_eq!(order.size_matched, dec!(50.0000));
        assert_eq!(order.expiration, Some("1735689600".to_string()));
        assert!(order.associate_trades.is_none());
    }

    #[rstest]
    fn test_open_order_roundtrip() {
        let order: PolymarketOpenOrder = load("http_open_order.json");
        let json = serde_json::to_string(&order).unwrap();
        let order2: PolymarketOpenOrder = serde_json::from_str(&json).unwrap();
        assert_eq!(order, order2);
    }

    #[rstest]
    fn test_trade_report_fields() {
        let trade: PolymarketTradeReport = load("http_trade_report.json");

        assert_eq!(trade.id, "trade-0xabcdef1234");
        assert_eq!(
            trade.taker_order_id,
            "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef12"
        );
        assert_eq!(trade.side, PolymarketOrderSide::Buy);
        assert_eq!(trade.size, dec!(25.0000));
        assert_eq!(trade.fee_rate_bps, dec!(0));
        assert_eq!(trade.price, dec!(0.5000));
        assert_eq!(trade.status, PolymarketTradeStatus::Confirmed);
        assert_eq!(trade.outcome, PolymarketOutcome::yes());
        assert_eq!(trade.bucket_index, 0);
        assert_eq!(trade.trader_side, PolymarketLiquiditySide::Taker);
        assert_eq!(trade.maker_orders.len(), 2);
    }

    #[rstest]
    fn test_trade_report_maker_orders() {
        let trade: PolymarketTradeReport = load("http_trade_report.json");

        let first = &trade.maker_orders[0];
        assert_eq!(first.matched_amount, dec!(25.0000));
        assert_eq!(first.fee_rate_bps, dec!(0));
        assert_eq!(first.price, dec!(0.5000));
        assert_eq!(first.outcome, PolymarketOutcome::yes());

        let second = &trade.maker_orders[1];
        assert_eq!(second.fee_rate_bps, dec!(10));
        assert_eq!(second.matched_amount, dec!(5.0000));
    }

    #[rstest]
    fn test_trade_report_roundtrip() {
        let trade: PolymarketTradeReport = load("http_trade_report.json");
        let json = serde_json::to_string(&trade).unwrap();
        let trade2: PolymarketTradeReport = serde_json::from_str(&json).unwrap();
        assert_eq!(trade, trade2);
    }

    #[rstest]
    fn test_signed_order_camel_case_fields() {
        let order: PolymarketOrder = load("http_signed_order.json");

        assert_eq!(order.salt, 123456789);
        assert_eq!(order.maker, "0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266");
        assert_eq!(order.taker, "0x0000000000000000000000000000000000000000");
        assert_eq!(order.maker_amount, dec!(100000000));
        assert_eq!(order.taker_amount, dec!(50000000));
        assert_eq!(order.fee_rate_bps, dec!(0));
        assert_eq!(order.expiration, "0");
        assert_eq!(order.nonce, "0");
        assert_eq!(order.side, PolymarketOrderSide::Buy);
        assert_eq!(order.signature_type, SignatureType::Eoa);
    }

    #[rstest]
    fn test_signed_order_roundtrip() {
        let order: PolymarketOrder = load("http_signed_order.json");
        let json = serde_json::to_string(&order).unwrap();
        let order2: PolymarketOrder = serde_json::from_str(&json).unwrap();
        assert_eq!(order, order2);
    }

    #[rstest]
    fn test_signed_order_serializes_camel_case() {
        let order: PolymarketOrder = load("http_signed_order.json");
        let json = serde_json::to_string(&order).unwrap();

        // Verify camelCase field names are present in serialized output
        assert!(json.contains("\"tokenId\""));
        assert!(json.contains("\"makerAmount\""));
        assert!(json.contains("\"takerAmount\""));
        assert!(json.contains("\"feeRateBps\""));
        assert!(json.contains("\"signatureType\""));
    }

    #[rstest]
    fn test_gamma_event_deserialization() {
        let events: Vec<GammaEvent> = load("gamma_event.json");

        assert_eq!(events.len(), 1);
        let event = &events[0];
        assert_eq!(event.id, "30829");
        assert_eq!(
            event.slug.as_deref(),
            Some("democratic-presidential-nominee-2028")
        );
        assert_eq!(
            event.title.as_deref(),
            Some("Democratic Presidential Nominee 2028")
        );
        assert_eq!(event.active, Some(true));
        assert_eq!(event.closed, Some(false));
        assert_eq!(event.archived, Some(false));
        assert_eq!(event.markets.len(), 2);
        assert_eq!(
            event.markets[0].condition_id,
            "0xc8f1cf5d4f26e0fd9c8fe89f2a7b3263b902cf14fde7bfccef525753bb492e47"
        );
        assert_eq!(
            event.markets[1].condition_id,
            "0xe39adea057926dc197fe30a441f57a340b2a232d5a687010f78bba9b6e02620f"
        );
    }

    #[rstest]
    fn test_gamma_event_empty_markets() {
        let json = r#"[{"id": "evt-002"}]"#;
        let events: Vec<GammaEvent> = serde_json::from_str(json).unwrap();

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].id, "evt-002");
        assert!(events[0].markets.is_empty());
        assert!(events[0].slug.is_none());
    }

    #[rstest]
    fn test_gamma_market_enriched_fields() {
        let market: GammaMarket = load("gamma_market.json");

        assert_eq!(market.best_bid, Some(0.5));
        assert_eq!(market.best_ask, Some(0.51));
        assert_eq!(market.spread, Some(0.009));
        assert_eq!(market.last_trade_price, Some(0.51));
        assert!(market.one_day_price_change.is_none());
        assert!(market.one_week_price_change.is_none());
        assert_eq!(market.volume_1wk, Some(9.999997));
        assert_eq!(market.volume_1mo, Some(9.999997));
        assert_eq!(market.volume_1yr, Some(9.999997));
        assert_eq!(market.rewards_min_size, Some(50.0));
        assert_eq!(market.rewards_max_spread, Some(4.5));
        assert_eq!(market.competitive, Some(0.9999750006249843));
        assert!(market.category.is_none());
        assert!(market.neg_risk_market_id.is_none());
        assert_eq!(
            market.outcome_prices.as_deref(),
            Some("[\"0.505\", \"0.495\"]")
        );
    }

    #[rstest]
    fn test_gamma_market_enriched_fields_default_to_none() {
        // Minimal market JSON: only required fields
        let json = r#"{"id": "m1", "conditionId": "0xcond", "clobTokenIds": "[]", "outcomes": "[]", "question": "Q?"}"#;
        let market: GammaMarket = serde_json::from_str(json).unwrap();

        assert!(market.best_bid.is_none());
        assert!(market.spread.is_none());
        assert!(market.volume_1wk.is_none());
        assert!(market.rewards_min_size.is_none());
        assert!(market.competitive.is_none());
        assert!(market.category.is_none());
        assert!(market.neg_risk_market_id.is_none());
    }

    #[rstest]
    fn test_gamma_event_enriched_fields() {
        let events: Vec<GammaEvent> = load("gamma_event.json");
        let event = &events[0];

        assert_eq!(event.liquidity, Some(43042905.16152));
        assert_eq!(event.volume, Some(799823812.487094));
        assert_eq!(event.open_interest, Some(0.0));
        assert_eq!(event.volume_24hr, Some(5669354.219446001));
        assert!(event.category.is_none());
        assert_eq!(event.neg_risk, Some(true));
        assert_eq!(
            event.neg_risk_market_id.as_deref(),
            Some("0x2c3d7e0eee6f058be3006baabf0d54a07da254ba47fe6e3e095e7990c7814700")
        );
        assert_eq!(event.featured, Some(false));
    }

    #[rstest]
    fn test_gamma_tag_deserialization() {
        let tags: Vec<GammaTag> = load("gamma_tags.json");

        assert_eq!(tags.len(), 5);
        assert_eq!(tags[0].id, "101259");
        assert_eq!(tags[0].label.as_deref(), Some("Health and Human Services"));
        assert_eq!(tags[0].slug.as_deref(), Some("health-and-human-services"));
        assert_eq!(tags[2].slug.as_deref(), Some("attorney-general"));
    }

    #[rstest]
    fn test_search_response_deserialization() {
        let response: SearchResponse = load("search_response.json");

        // Real API returns no top-level "markets" key
        assert!(response.markets.is_none());

        let events = response.events.as_ref().unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].slug.as_deref(), Some("bitcoin-above-on-march-11"));
        assert_eq!(events[0].markets.len(), 1);
    }

    #[rstest]
    fn test_search_response_empty_fields() {
        let json = "{}";
        let response: SearchResponse = serde_json::from_str(json).unwrap();
        assert!(response.markets.is_none());
        assert!(response.events.is_none());
    }

    #[rstest]
    fn test_clob_book_response_deserialization() {
        let response: ClobBookResponse = load("clob_book_response.json");

        assert_eq!(response.bids.len(), 3);
        assert_eq!(response.asks.len(), 3);

        assert_eq!(response.bids[0].price, "0.48");
        assert_eq!(response.bids[0].size, "100.00");
        assert_eq!(response.bids[2].price, "0.50");
        assert_eq!(response.bids[2].size, "150.00");

        assert_eq!(response.asks[0].price, "0.51");
        assert_eq!(response.asks[0].size, "120.00");
        assert_eq!(response.asks[2].price, "0.53");
        assert_eq!(response.asks[2].size, "90.00");
    }

    #[rstest]
    fn test_clob_book_response_ignores_extra_fields() {
        // Verify serde silently ignores extra fields from the API
        let json = r#"{"market": "0xabc", "asset_id": "123", "hash": "0x1", "timestamp": "123", "bids": [], "asks": []}"#;
        let response: ClobBookResponse = serde_json::from_str(json).unwrap();
        assert!(response.bids.is_empty());
        assert!(response.asks.is_empty());
    }

    #[rstest]
    fn test_fee_rate_response_zero() {
        let response: FeeRateResponse = load("clob_fee_rate_response_zero.json");
        assert_eq!(response.base_fee, dec!(0));
    }

    #[rstest]
    fn test_fee_rate_response_nonzero() {
        let response: FeeRateResponse = load("clob_fee_rate_response_nonzero.json");
        assert_eq!(response.base_fee, dec!(150));
    }

    #[rstest]
    fn test_data_api_position_deserialization() {
        let positions: Vec<DataApiPosition> = load("data_api_positions_response.json");

        assert_eq!(positions.len(), 4);
        assert_eq!(
            positions[0].asset,
            "71321045863084981365469005770620412523470745398083994982746259498689308907982"
        );
        assert_eq!(
            positions[0].condition_id,
            "0xc8f1cf5d4f26e0fd9c8fe89f2a7b3263b902cf14fde7bfccef525753bb492e47"
        );
        assert_eq!(positions[0].size, 150.5);
        assert_eq!(positions[0].avg_price, Some(0.55));

        // Zero-size position
        assert_eq!(positions[1].size, 0.0);
        assert_eq!(positions[1].avg_price, Some(0.45));

        // Third position
        assert_eq!(
            positions[2].condition_id,
            "0xabc123def456789012345678901234567890abcdef1234567890abcdef123456"
        );
        assert_eq!(positions[2].size, 42.0);
        assert_eq!(positions[2].avg_price, Some(0.3));

        // Dust position (below DUST_SNAP_THRESHOLD)
        assert_eq!(positions[3].size, 0.005);
        assert_eq!(positions[3].avg_price, Some(0.7));
    }

    #[rstest]
    fn test_data_api_trade_deserialization() {
        let trades: Vec<DataApiTrade> = load("data_api_trades_response.json");

        assert_eq!(trades.len(), 3);
        assert_eq!(
            trades[0].asset,
            "71321045863084981365469005770620412523470745398083994982746259498689308907982"
        );
        assert_eq!(
            trades[0].condition_id,
            "0xc8f1cf5d4f26e0fd9c8fe89f2a7b3263b902cf14fde7bfccef525753bb492e47"
        );
        assert_eq!(trades[0].side, PolymarketOrderSide::Buy);
        assert_eq!(trades[0].price, 0.55);
        assert_eq!(trades[0].size, 100.0);
        assert_eq!(trades[0].timestamp, 1710000000);
        assert_eq!(
            trades[0].transaction_hash,
            "0xabc123def456789012345678901234567890abcdef1234567890abcdef123456"
        );

        assert_eq!(trades[1].side, PolymarketOrderSide::Sell);
        assert_eq!(trades[1].price, 0.53);

        // Third trade has different asset (other outcome token)
        assert_eq!(
            trades[2].asset,
            "99999999999999999999999999999999999999999999999999999999999999999999999999999"
        );
    }
}
