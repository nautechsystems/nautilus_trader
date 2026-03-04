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
    pub clob_token_ids: String,
    /// JSON-encoded outcome labels (e.g. `["Yes", "No"]`).
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
}

/// Tick size response from CLOB `GET /tick-size`.
///
/// References: <https://docs.polymarket.com/api-reference/market-data/get-tick-size>
#[derive(Clone, Debug, Deserialize)]
pub struct TickSizeResponse {
    /// Minimum tick size (price increment) for a token.
    pub minimum_tick_size: f64,
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
        assert_eq!(order.outcome, PolymarketOutcome::Yes);
        assert_eq!(order.original_size, dec!(100.0000));
        assert_eq!(order.price, dec!(0.5000));
        assert_eq!(order.size_matched, dec!(25.0000));
        assert_eq!(order.created_at, 1703875200000);
        assert!(order.expiration.is_none());
        assert_eq!(order.associate_trades, Some(vec!["0xabc001".to_string()]));
    }

    #[rstest]
    fn test_open_order_matched_sell_fok() {
        let order: PolymarketOpenOrder = load("http_open_order_sell_fok.json");

        assert_eq!(order.status, PolymarketOrderStatus::Matched);
        assert_eq!(order.side, PolymarketOrderSide::Sell);
        assert_eq!(order.order_type, PolymarketOrderType::FOK);
        assert_eq!(order.outcome, PolymarketOutcome::No);
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
        assert_eq!(trade.outcome, PolymarketOutcome::Yes);
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
        assert_eq!(first.outcome, PolymarketOutcome::Yes);

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
}
