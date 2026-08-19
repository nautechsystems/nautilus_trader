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
    parse::{
        deserialize_decimal_from_json_number, deserialize_decimal_from_str,
        deserialize_optional_decimal_from_json_number, deserialize_optional_polymarket_game_id,
        serialize_decimal_as_json_number, serialize_decimal_as_str,
        serialize_optional_decimal_as_json_number,
    },
};

/// A signed limit order for submission to the CLOB V2 exchange.
///
/// References: <https://docs.polymarket.com/v2-migration>,
/// <https://docs.polymarket.com/api-reference/trade/post-a-new-order>
///
/// `expiration` is part of the wire body but NOT part of the EIP-712 signed
/// struct in V2 (the protocol enforces it server-side). `"0"` means no
/// expiration. All other fields appear inside the signed struct.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PolymarketOrder {
    pub salt: u64,
    pub maker: String,
    pub signer: String,
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
    pub side: PolymarketOrderSide,
    pub signature_type: SignatureType,
    /// Unix seconds timestamp when a GTD order auto-expires. `"0"` for non-GTD.
    /// Not included in the EIP-712 signed hash; protocol enforces this value.
    pub expiration: String,
    /// Order creation time in milliseconds. Replaces `nonce` from V1 for
    /// per-address uniqueness (not an expiration).
    pub timestamp: String,
    /// Generic bytes32 metadata field. Zero bytes when unused.
    pub metadata: String,
    /// Builder code (`bytes32`). Zero bytes when unset.
    pub builder: String,
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
#[derive(Clone, Debug, Deserialize, Serialize)]
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
    /// Time when the market closed.
    pub closed_time: Option<String>,
    /// UMA resolution state reported by Gamma.
    pub uma_resolution_status: Option<String>,
    /// JSON-encoded UMA resolution states reported by Gamma.
    pub uma_resolution_statuses: Option<String>,
    /// Source used to resolve the market.
    pub resolution_source: Option<String>,
    /// Whether CLOB is accepting orders.
    pub accepting_orders: Option<bool>,
    /// Whether order book trading is enabled.
    pub enable_order_book: Option<bool>,
    /// Minimum price increment.
    #[serde(
        default,
        deserialize_with = "deserialize_optional_decimal_from_json_number",
        serialize_with = "serialize_optional_decimal_as_json_number"
    )]
    pub order_price_min_tick_size: Option<Decimal>,
    /// Minimum order size.
    #[serde(
        default,
        deserialize_with = "deserialize_optional_decimal_from_json_number",
        serialize_with = "serialize_optional_decimal_as_json_number"
    )]
    pub order_min_size: Option<Decimal>,
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
    /// Whether fees are enabled for this market.
    pub fees_enabled: Option<bool>,
    /// Fee schedule for this market.
    pub fee_schedule: Option<FeeSchedule>,
    /// Game ID for sport markets, kept verbatim because Gamma emits both
    /// numeric and composite `<uuid>:<away>:<home>` forms. `null` and `-1`
    /// both mean "no game" and surface as `None`. Reference shape:
    /// <https://github.com/Polymarket/rs-clob-client/blob/main/src/gamma/types/response.rs>.
    #[serde(default, deserialize_with = "deserialize_optional_polymarket_game_id")]
    pub game_id: Option<String>,
    /// Events linked to this gamma market.
    pub events: Option<Vec<GammaEvent>>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FeeSchedule {
    #[serde(
        serialize_with = "serialize_decimal_as_json_number",
        deserialize_with = "deserialize_decimal_from_json_number"
    )]
    pub exponent: Decimal,
    #[serde(
        serialize_with = "serialize_decimal_as_json_number",
        deserialize_with = "deserialize_decimal_from_json_number"
    )]
    pub rate: Decimal,
    pub taker_only: bool,
    #[serde(
        serialize_with = "serialize_decimal_as_json_number",
        deserialize_with = "deserialize_decimal_from_json_number"
    )]
    pub rebate_rate: Decimal,
}

/// An event response from the Gamma API `GET /events`.
///
/// Events are parent containers grouping related markets (e.g., an election
/// event contains multiple outcome markets). Each event's `markets` array
/// contains full [`GammaMarket`] objects.
#[derive(Clone, Debug, Deserialize, Serialize)]
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
    /// Game ID for sport markets, kept verbatim because Gamma emits both
    /// numeric and composite `<uuid>:<away>:<home>` forms. `null` and `-1`
    /// both mean "no game" and surface as `None`. Reference shape:
    /// <https://github.com/Polymarket/rs-clob-client/blob/main/src/gamma/types/response.rs>.
    #[serde(default, deserialize_with = "deserialize_optional_polymarket_game_id")]
    pub game_id: Option<String>,
}

/// A tag from the Gamma API `GET /tags`.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct GammaTag {
    /// Tag identifier.
    pub id: String,
    /// Human-readable label.
    pub label: Option<String>,
    /// URL slug.
    pub slug: Option<String>,
}

/// Response from the Gamma API `GET /public-search`.
#[derive(Clone, Debug, Deserialize, Serialize)]
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
    #[serde(deserialize_with = "deserialize_decimal_from_json_number")]
    pub minimum_tick_size: Decimal,
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

/// A single outcome token in a CLOB market response.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ClobMarketToken {
    pub token_id: String,
    pub outcome: String,
    #[serde(
        default,
        deserialize_with = "deserialize_optional_decimal_from_json_number",
        serialize_with = "serialize_optional_decimal_as_json_number"
    )]
    pub price: Option<Decimal>,
    pub winner: bool,
}

/// A daily reward rate in a CLOB market response.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ClobMarketRewardRate {
    pub asset_address: String,
    #[serde(
        deserialize_with = "deserialize_decimal_from_json_number",
        serialize_with = "serialize_decimal_as_json_number"
    )]
    pub rewards_daily_rate: Decimal,
}

/// Reward configuration in a CLOB market response.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ClobMarketRewards {
    pub rates: Option<Vec<ClobMarketRewardRate>>,
    #[serde(
        default,
        deserialize_with = "deserialize_optional_decimal_from_json_number",
        serialize_with = "serialize_optional_decimal_as_json_number"
    )]
    pub min_size: Option<Decimal>,
    #[serde(
        default,
        deserialize_with = "deserialize_optional_decimal_from_json_number",
        serialize_with = "serialize_optional_decimal_as_json_number"
    )]
    pub max_spread: Option<Decimal>,
}

/// Response from CLOB `GET /markets/{condition_id}`.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ClobMarketResponse {
    pub enable_order_book: Option<bool>,
    pub active: Option<bool>,
    pub condition_id: String,
    pub closed: bool,
    pub archived: Option<bool>,
    pub accepting_orders: Option<bool>,
    pub accepting_order_timestamp: Option<String>,
    #[serde(
        default,
        deserialize_with = "deserialize_optional_decimal_from_json_number",
        serialize_with = "serialize_optional_decimal_as_json_number"
    )]
    pub minimum_order_size: Option<Decimal>,
    #[serde(
        default,
        deserialize_with = "deserialize_optional_decimal_from_json_number",
        serialize_with = "serialize_optional_decimal_as_json_number"
    )]
    pub minimum_tick_size: Option<Decimal>,
    pub question_id: Option<String>,
    pub question: Option<String>,
    pub description: Option<String>,
    pub market_slug: Option<String>,
    pub end_date_iso: Option<String>,
    pub game_start_time: Option<String>,
    pub seconds_delay: Option<i64>,
    pub fpmm: Option<String>,
    pub maker_base_fee: Option<i64>,
    pub taker_base_fee: Option<i64>,
    pub notifications_enabled: Option<bool>,
    pub neg_risk: Option<bool>,
    pub neg_risk_market_id: Option<String>,
    pub neg_risk_request_id: Option<String>,
    pub icon: Option<String>,
    pub image: Option<String>,
    pub rewards: Option<ClobMarketRewards>,
    pub is_50_50_outcome: Option<bool>,
    pub tokens: Vec<ClobMarketToken>,
    pub tags: Option<Vec<String>>,
}

/// A position from the Polymarket Data API `GET /positions` endpoint.
#[derive(Clone, Debug, Deserialize)]
pub struct DataApiPosition {
    pub asset: String,
    #[serde(alias = "conditionId", alias = "condition_id")]
    pub condition_id: String,
    #[serde(deserialize_with = "deserialize_decimal_from_json_number")]
    pub size: Decimal,
    #[serde(
        default,
        alias = "avgPrice",
        alias = "avg_price",
        deserialize_with = "deserialize_optional_decimal_from_json_number"
    )]
    pub avg_price: Option<Decimal>,
}

/// A trade from the Polymarket Data API `GET /trades` endpoint.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DataApiTrade {
    pub proxy_wallet: Option<String>,
    pub asset: String,
    pub condition_id: String,
    pub side: PolymarketOrderSide,
    #[serde(deserialize_with = "deserialize_decimal_from_json_number")]
    pub price: Decimal,
    #[serde(deserialize_with = "deserialize_decimal_from_json_number")]
    pub size: Decimal,
    pub timestamp: i64,
    pub title: Option<String>,
    pub slug: Option<String>,
    pub icon: Option<String>,
    pub event_slug: Option<String>,
    pub outcome: Option<String>,
    pub outcome_index: Option<i64>,
    pub name: Option<String>,
    pub pseudonym: Option<String>,
    pub bio: Option<String>,
    pub profile_image: Option<String>,
    pub profile_image_optimized: Option<String>,
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
        assert_eq!(first.price, dec!(0.5000));
        assert_eq!(first.outcome, PolymarketOutcome::yes());

        let second = &trade.maker_orders[1];
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
        assert_eq!(order.maker_amount, dec!(100000000));
        assert_eq!(order.taker_amount, dec!(50000000));
        assert_eq!(order.expiration, "0");
        assert_eq!(order.timestamp, "1713398400000");
        assert_eq!(
            order.metadata,
            "0x0000000000000000000000000000000000000000000000000000000000000000"
        );
        assert_eq!(
            order.builder,
            "0x0000000000000000000000000000000000000000000000000000000000000000"
        );
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
        assert!(json.contains("\"signatureType\""));
        assert!(json.contains("\"expiration\""));
        assert!(json.contains("\"timestamp\""));
        assert!(json.contains("\"metadata\""));
        assert!(json.contains("\"builder\""));
    }

    #[rstest]
    fn test_signed_order_omits_v1_fields() {
        // V2 dropped `taker`, `nonce`, and `feeRateBps` from the order body.
        // A regression that re-introduces any of them would silently land V1
        // shape on a V2 endpoint, so we explicitly assert their absence.
        let order: PolymarketOrder = load("http_signed_order.json");
        let json = serde_json::to_string(&order).unwrap();

        assert!(
            !json.contains("\"taker\""),
            "wire body must not include `taker`: {json}"
        );
        assert!(
            !json.contains("\"nonce\""),
            "wire body must not include `nonce`: {json}"
        );
        assert!(
            !json.contains("\"feeRateBps\""),
            "wire body must not include `feeRateBps`: {json}"
        );
    }

    #[rstest]
    fn test_signed_order_v2_docs_example_roundtrips() {
        // POST /order body shape from <https://docs.polymarket.com/v2-migration>.
        // Round-tripping it ensures we accept the exact shape the docs publish.
        let docs_example = r#"{
            "salt": 12345,
            "maker": "0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266",
            "signer": "0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266",
            "tokenId": "102936",
            "makerAmount": "1000000",
            "takerAmount": "2000000",
            "side": "BUY",
            "signatureType": 1,
            "expiration": "0",
            "timestamp": "1713398400000",
            "metadata": "0x0000000000000000000000000000000000000000000000000000000000000000",
            "builder": "0x0000000000000000000000000000000000000000000000000000000000000000",
            "signature": "0xdeadbeef"
        }"#;

        let order: PolymarketOrder = serde_json::from_str(docs_example).unwrap();
        assert_eq!(order.salt, 12345);
        assert_eq!(order.token_id.as_str(), "102936");
        assert_eq!(order.maker_amount, dec!(1000000));
        assert_eq!(order.taker_amount, dec!(2000000));
        assert_eq!(order.side, PolymarketOrderSide::Buy);
        assert_eq!(order.signature_type, SignatureType::PolyProxy);
        assert_eq!(order.expiration, "0");
        assert_eq!(order.timestamp, "1713398400000");

        // Round-trip preserves field semantics.
        let json = serde_json::to_string(&order).unwrap();
        let order2: PolymarketOrder = serde_json::from_str(&json).unwrap();
        assert_eq!(order, order2);
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
    fn test_sports_market_are_weird() {
        let money_line: GammaMarket = load("gamma_market_sports_market_money_line.json");
        let map_handicap: GammaMarket = load("gamma_market_sports_market_map_handicap.json");

        // same event, same slug
        assert_eq!(
            money_line.events.as_ref().unwrap()[0].game_id,
            map_handicap.events.as_ref().unwrap()[0].game_id
        );

        // one market has no game_id
        assert!(map_handicap.game_id.is_none());
        assert_eq!(money_line.game_id.as_deref(), Some("1427074"));
    }

    #[rstest]
    fn test_gamma_event_composite_sports_game_id() {
        // Live Gamma record from issue #4771: the event carries a numeric
        // `gameId` while its first market carries a composite one.
        let events: Vec<GammaEvent> = load("gamma_event_sports_composite_game_id.json");

        assert_eq!(events.len(), 1);

        let event = &events[0];

        assert_eq!(event.id, "835109");
        assert_eq!(event.game_id.as_deref(), Some("287011684"));
        assert_eq!(event.markets.len(), 2);
        assert_eq!(event.markets[0].id, "3524358");
        assert_eq!(
            event.markets[0].game_id.as_deref(),
            Some("dd80aae9-52f9-4c7b-a1cf-7b4ab63cd281:STL:TEX")
        );
        assert_eq!(event.markets[1].id, "3554041");
        assert_eq!(event.markets[1].game_id, None);

        // Re-serialization feeds the Python loader, so the key stays a string
        // even where Gamma sent a number.
        let encoded = serde_json::to_value(event).unwrap();

        assert_eq!(encoded["gameId"], serde_json::json!("287011684"));
        assert_eq!(
            encoded["markets"][0]["gameId"],
            serde_json::json!("dd80aae9-52f9-4c7b-a1cf-7b4ab63cd281:STL:TEX")
        );
    }

    #[rstest]
    fn test_fee_schedule_decimal_fields() {
        let market: GammaMarket = load("gamma_market_sports_market_money_line.json");
        let schedule = market.fee_schedule.unwrap();

        assert_eq!(schedule.exponent, Decimal::ONE);
        assert_eq!(schedule.rate, dec!(0.03));
        assert!(schedule.taker_only);
        assert_eq!(schedule.rebate_rate, dec!(0.25));
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
        assert!(market.uma_resolution_status.is_none());
        assert_eq!(market.uma_resolution_statuses.as_deref(), Some("[]"));
        assert_eq!(
            market.outcome_prices.as_deref(),
            Some("[\"0.505\", \"0.495\"]")
        );
    }

    #[rstest]
    fn test_gamma_market_uma_resolution_statuses() {
        let market: GammaMarket = load("gamma_market.json");

        assert!(market.uma_resolution_status.is_none());
        assert_eq!(market.uma_resolution_statuses.as_deref(), Some("[]"));
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
        // Verify serde silently ignores fields from both V1 and V2 `/book`
        // responses. The live V2 endpoint adds `tick_size`, `min_order_size`,
        // `neg_risk`, and `last_trade_price` on top of the V1 fields; pinning
        // them here catches a future `#[serde(deny_unknown_fields)]` regression
        // before it breaks production parsing.
        let json = r#"{
            "market": "0xabc",
            "asset_id": "123",
            "hash": "0x1",
            "timestamp": "123",
            "bids": [],
            "asks": [],
            "tick_size": "0.01",
            "min_order_size": "5",
            "neg_risk": false,
            "last_trade_price": "0.55"
        }"#;
        let response: ClobBookResponse = serde_json::from_str(json).unwrap();
        assert!(response.bids.is_empty());
        assert!(response.asks.is_empty());
    }

    #[rstest]
    fn test_clob_market_response_captured_fields() {
        let response: ClobMarketResponse = load("clob_market_response.json");
        let raw: serde_json::Value = load("clob_market_response.json");

        assert_eq!(response.enable_order_book, Some(true));
        assert_eq!(response.active, Some(true));
        assert!(!response.closed);
        assert_eq!(response.archived, Some(false));
        assert_eq!(response.accepting_orders, Some(true));
        assert_eq!(
            response.accepting_order_timestamp.as_deref(),
            Some("2026-08-01T22:56:49Z")
        );
        assert_eq!(response.minimum_order_size, Some(dec!(5)));
        assert_eq!(response.minimum_tick_size, Some(dec!(0.01)));
        assert_eq!(
            response.condition_id,
            "0xcccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"
        );
        assert_eq!(
            response.question_id.as_deref(),
            Some("0xdddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd")
        );
        assert_eq!(
            response.question.as_deref(),
            Some("LoL: T1 vs Hanwha Life Esports (BO3) - LCK Round 3-4 Legend Group")
        );
        assert_eq!(response.description.as_deref(), raw["description"].as_str());
        assert_eq!(
            response.market_slug.as_deref(),
            Some("sanitized-clob-market")
        );
        assert_eq!(
            response.end_date_iso.as_deref(),
            Some("2026-08-08T00:00:00Z")
        );
        assert_eq!(
            response.game_start_time.as_deref(),
            Some("2026-08-08T08:00:00Z")
        );
        assert_eq!(response.seconds_delay, Some(1));
        assert_eq!(response.fpmm.as_deref(), Some(""));
        assert_eq!(response.maker_base_fee, Some(1000));
        assert_eq!(response.taker_base_fee, Some(1000));
        assert_eq!(response.notifications_enabled, Some(true));
        assert_eq!(response.neg_risk, Some(false));
        assert_eq!(response.neg_risk_market_id.as_deref(), Some(""));
        assert_eq!(response.neg_risk_request_id.as_deref(), Some(""));
        assert_eq!(
            response.icon.as_deref(),
            Some("https://example.com/sanitized-market.png")
        );
        assert_eq!(
            response.image.as_deref(),
            Some("https://example.com/sanitized-market.png")
        );
        let rewards = response.rewards.as_ref().expect("captured rewards");
        assert!(rewards.rates.is_none());
        assert_eq!(rewards.min_size, Some(dec!(50)));
        assert_eq!(rewards.max_spread, Some(dec!(4.5)));
        assert_eq!(response.is_50_50_outcome, Some(false));
        assert_eq!(response.tokens.len(), 2);
        assert_eq!(
            response.tokens[0].token_id,
            "10000000000000000000000000000000000000000000000000000000000000000000000000001"
        );
        assert_eq!(response.tokens[0].outcome, "T1");
        assert_eq!(response.tokens[0].price, Some(dec!(0.715)));
        assert!(!response.tokens[0].winner);
        assert_eq!(
            response.tokens[1].token_id,
            "10000000000000000000000000000000000000000000000000000000000000000000000000002"
        );
        assert_eq!(response.tokens[1].outcome, "Hanwha Life Esports");
        assert_eq!(response.tokens[1].price, Some(dec!(0.285)));
        assert!(!response.tokens[1].winner);
        assert_eq!(
            response.tags.as_deref(),
            Some(
                &[
                    "Sports".to_string(),
                    "Esports".to_string(),
                    "league of legends".to_string(),
                    "Games".to_string(),
                ][..]
            )
        );
    }

    #[rstest]
    fn test_clob_market_rewards_documented_rate_fields() {
        // Constructed from the documented Rewards schema because the capture has `rates: null`
        let json = r#"{
            "rates":[{"asset_address":"0x1111111111111111111111111111111111111111","rewards_daily_rate":12.5}],
            "min_size":25,
            "max_spread":3.5
        }"#;
        let rewards: ClobMarketRewards = serde_json::from_str(json).unwrap();

        let rates = rewards.rates.as_deref().expect("documented reward rate");
        assert_eq!(rates.len(), 1);
        assert_eq!(
            rates[0].asset_address,
            "0x1111111111111111111111111111111111111111"
        );
        assert_eq!(rates[0].rewards_daily_rate, dec!(12.5));
        assert_eq!(rewards.min_size, Some(dec!(25)));
        assert_eq!(rewards.max_spread, Some(dec!(3.5)));
    }

    #[rstest]
    fn test_clob_market_decimal_fields_preserve_precision() {
        let json = r#"{
            "condition_id":"0xcondition",
            "closed":false,
            "minimum_order_size":123456789.1234567890123456789,
            "minimum_tick_size":0.1234567890123456789012345678,
            "rewards":{
                "rates":[{
                    "asset_address":"0x1111111111111111111111111111111111111111",
                    "rewards_daily_rate":0.1234567890123456789012345678
                }],
                "min_size":123456789.1234567890123456789,
                "max_spread":0.1234567890123456789012345678
            },
            "tokens":[{
                "token_id":"token-1",
                "outcome":"Yes",
                "price":0.1234567890123456789012345678,
                "winner":false
            }]
        }"#;
        let market: ClobMarketResponse = serde_json::from_str(json).unwrap();
        let precise = Decimal::from_str_exact("0.1234567890123456789012345678").unwrap();
        let large = Decimal::from_str_exact("123456789.1234567890123456789").unwrap();

        assert_eq!(market.minimum_order_size, Some(large));
        assert_eq!(market.minimum_tick_size, Some(precise));
        let rewards = market.rewards.as_ref().unwrap();
        assert_eq!(
            rewards.rates.as_ref().unwrap()[0].rewards_daily_rate,
            precise
        );
        assert_eq!(rewards.min_size, Some(large));
        assert_eq!(rewards.max_spread, Some(precise));
        assert_eq!(market.tokens[0].price, Some(precise));
        let serialized = serde_json::to_string(&market).unwrap();
        assert!(serialized.contains("\"minimum_order_size\":123456789.1234567890123456789"));
        assert!(serialized.contains("\"minimum_tick_size\":0.1234567890123456789012345678"));
        assert!(serialized.contains("\"rewards_daily_rate\":0.1234567890123456789012345678"));
        assert!(serialized.contains("\"min_size\":123456789.1234567890123456789"));
        assert!(serialized.contains("\"max_spread\":0.1234567890123456789012345678"));
        assert!(serialized.contains("\"price\":0.1234567890123456789012345678"));
    }

    #[rstest]
    fn test_clob_market_response_deserialization_accepting_false() {
        let response: ClobMarketResponse = load("clob_market_closed_binary_accepting_false.json");
        assert_eq!(
            response.condition_id,
            "0x8ccc3f4951ff02c1d34b87988752b4444ad17228732780a6cf22afefe8478bb6"
        );
        assert!(response.closed);
        assert_eq!(response.tokens.len(), 2);
        assert_eq!(response.tokens[0].outcome, "Yes");
        assert!(!response.tokens[0].winner);
        assert_eq!(response.tokens[1].outcome, "No");
        assert!(response.tokens[1].winner);
    }

    #[rstest]
    fn test_clob_market_response_deserialization_accepting_true() {
        let response: ClobMarketResponse = load("clob_market_closed_binary_accepting_true.json");
        assert_eq!(
            response.condition_id,
            "0xd57eed0d44f5b8ca54925d8d6ff440b146b3e6e071da18136ee3ee572d34479e"
        );
        assert!(response.closed);
        assert_eq!(response.tokens.len(), 2);
        assert_eq!(response.tokens[0].outcome, "Yes");
        assert!(response.tokens[0].winner);
        assert_eq!(response.tokens[1].outcome, "No");
        assert!(!response.tokens[1].winner);
    }

    #[rstest]
    fn test_tick_size_response_preserves_json_number() {
        let response: TickSizeResponse =
            serde_json::from_str(r#"{"minimum_tick_size":0.1234567890123456789012345678}"#)
                .unwrap();
        let precise =
            rust_decimal::Decimal::from_str_exact("0.1234567890123456789012345678").unwrap();

        assert_eq!(response.minimum_tick_size, precise);
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
        assert_eq!(positions[0].size, dec!(150.5));
        assert_eq!(positions[0].avg_price, Some(dec!(0.55)));

        // Zero-size position
        assert_eq!(positions[1].size, dec!(0));
        assert_eq!(positions[1].avg_price, Some(dec!(0.45)));

        // Third position
        assert_eq!(
            positions[2].condition_id,
            "0xabc123def456789012345678901234567890abcdef1234567890abcdef123456"
        );
        assert_eq!(positions[2].size, dec!(42));
        assert_eq!(positions[2].avg_price, Some(dec!(0.3)));

        // Dust position (below DUST_POSITION_THRESHOLD)
        assert_eq!(positions[3].size, dec!(0.005));
        assert_eq!(positions[3].avg_price, Some(dec!(0.7)));
    }

    #[rstest]
    fn test_data_api_position_deserializes_exact_numeric_tokens() {
        let position: DataApiPosition = serde_json::from_str(
            r#"{
                "asset":"123",
                "conditionId":"0xabc",
                "size":1.000001,
                "avgPrice":0.123456789012345678
            }"#,
        )
        .unwrap();

        assert_eq!(position.size, dec!(1.000001));
        assert_eq!(position.avg_price, Some(dec!(0.123456789012345678)));
    }

    #[rstest]
    fn test_data_api_trade_deserialization() {
        let trades: Vec<DataApiTrade> = load("data_api_trades_captured_response.json");

        assert_eq!(trades.len(), 3);
        assert_eq!(
            trades[0].asset,
            "10000000000000000000000000000000000000000000000000000000000000000000000000001"
        );
        assert_eq!(
            trades[0].condition_id,
            "0xcccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"
        );
        assert_eq!(trades[0].side, PolymarketOrderSide::Sell);
        assert_eq!(trades[0].price, dec!(0.7));
        assert_eq!(trades[0].size, dec!(92.59));
        assert_eq!(trades[0].timestamp, 1786179735);
        assert_eq!(
            trades[0].transaction_hash,
            "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
        );

        assert_eq!(trades[1].asset, trades[0].asset);
        assert_eq!(trades[1].condition_id, trades[0].condition_id);
        assert_eq!(trades[1].side, PolymarketOrderSide::Buy);
        assert_eq!(trades[1].price, dec!(0.709999959));
        assert_eq!(trades[1].size, dec!(1.464786));
        assert_eq!(trades[1].timestamp, 1786179730);
        assert_eq!(
            trades[1].transaction_hash,
            "0xbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
        );
        assert_eq!(
            trades[2].asset,
            "10000000000000000000000000000000000000000000000000000000000000000000000000002"
        );
        assert_eq!(trades[2].condition_id, trades[0].condition_id);
        assert_eq!(trades[2].side, PolymarketOrderSide::Buy);
        assert_eq!(trades[2].price, dec!(0.2972581967));
        assert_eq!(trades[2].size, dec!(244));
        assert_eq!(trades[2].timestamp, 1786179726);
        assert_eq!(
            trades[2].transaction_hash,
            "0xcccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"
        );

        for (trade, outcome, outcome_index) in [
            (&trades[0], "T1", 0),
            (&trades[1], "T1", 0),
            (&trades[2], "Hanwha Life Esports", 1),
        ] {
            assert_eq!(
                trade.proxy_wallet.as_deref(),
                Some("0x1111111111111111111111111111111111111111")
            );
            assert_eq!(
                trade.title.as_deref(),
                Some("LoL: T1 vs Hanwha Life Esports (BO3) - LCK Round 3-4 Legend Group")
            );
            assert_eq!(trade.slug.as_deref(), Some("sanitized-market"));
            assert_eq!(
                trade.icon.as_deref(),
                Some("https://example.com/sanitized-market.png")
            );
            assert_eq!(trade.event_slug.as_deref(), Some("sanitized-event"));
            assert_eq!(trade.outcome.as_deref(), Some(outcome));
            assert_eq!(trade.outcome_index, Some(outcome_index));
            assert_eq!(trade.name.as_deref(), Some("Sanitized trader"));
            assert_eq!(trade.pseudonym.as_deref(), Some("sanitized-trader"));
            assert_eq!(trade.bio.as_deref(), Some("Sanitized profile"));
            assert_eq!(
                trade.profile_image.as_deref(),
                Some("https://example.com/sanitized-profile.png")
            );
            assert_eq!(
                trade.profile_image_optimized.as_deref(),
                Some("https://example.com/sanitized-profile-optimized.png")
            );
        }
    }

    #[rstest]
    fn test_data_api_trade_decimal_fields_preserve_precision() {
        let json = r#"{
            "asset":"token-1",
            "conditionId":"0xcondition",
            "side":"BUY",
            "price":0.1234567890123456789012345678,
            "size":123456789.1234567890123456789,
            "timestamp":1786179735,
            "transactionHash":"0xtransaction"
        }"#;
        let trade: DataApiTrade = serde_json::from_str(json).unwrap();

        assert_eq!(
            trade.price,
            Decimal::from_str_exact("0.1234567890123456789012345678").unwrap()
        );
        assert_eq!(
            trade.size,
            Decimal::from_str_exact("123456789.1234567890123456789").unwrap()
        );
    }
}
