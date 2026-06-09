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

//! Request and response payload definitions for Lighter REST endpoints.

use std::{collections::BTreeMap, str::FromStr};

use nautilus_core::serialization::{
    deserialize_decimal, deserialize_decimal_from_str, deserialize_decimal_or_zero,
    deserialize_optional_decimal,
};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize, de};
use ustr::Ustr;

use crate::common::enums::{
    LighterCandleResolution, LighterFundingResolution, LighterMarketStatus, LighterOrderKind,
    LighterOrderSide, LighterOrderStatus, LighterOrderTimeInForce, LighterPositionMarginMode,
    LighterProductType, LighterTradeType, LighterTriggerStatus,
};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct LighterResultCode {
    pub code: i32,
    pub message: Option<String>,
}

/// Response payload of `GET /api/v1/nextNonce`.
///
/// The venue reports the next sequence number it expects on `sendTx` for the
/// `(account_index, api_key_index)` pair. Pass this to
/// [`crate::signing::nonce::NonceManager::refresh`] before the first signed
/// transaction.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct LighterNextNonce {
    pub code: i32,
    pub message: Option<String>,
    pub nonce: i64,
}

/// One account row from `GET /api/v1/account`.
///
/// Models only the fields the adapter consumes; the venue response carries
/// many more, which are ignored on deserialization.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct LighterAccountDetail {
    pub account_index: u64,
    pub account_type: u8,
    pub status: i32,
}

/// Response payload of `GET /api/v1/account`.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct LighterAccountsResponse {
    pub code: i32,
    pub message: Option<String>,
    pub total: i64,
    pub accounts: Vec<LighterAccountDetail>,
}

/// Response payload of `GET /api/v1/getMakerOnlyApiKeys`.
///
/// Lighter restricts maker-only keys to the 0ms speed-bump lane (PostOnly
/// creates, modifies on ALO orders, cancel / cancel-all). Any tx kind outside
/// that allowlist — for example `ApproveIntegrator` (tx_type 45) — is rejected
/// with venue code `62007`. The adapter pre-flights this endpoint before
/// submitting the integrator auto-approval so it can skip the doomed tx with
/// a clear log line instead of swallowing the misleading 62007.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct LighterMakerOnlyApiKeys {
    pub code: i32,
    pub message: Option<String>,
    #[serde(default)]
    pub api_key_indexes: Vec<i64>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct LighterSendTxRequest {
    pub tx_type: u8,
    pub tx_info: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub price_protection: Option<bool>,
}

impl LighterSendTxRequest {
    #[must_use]
    pub fn new(tx_type: u8, tx_info: impl Into<String>) -> Self {
        Self {
            tx_type,
            tx_info: tx_info.into(),
            price_protection: None,
        }
    }

    #[must_use]
    pub const fn with_price_protection(mut self, price_protection: bool) -> Self {
        self.price_protection = Some(price_protection);
        self
    }

    pub(crate) fn form_fields(&self) -> Vec<(&'static str, String)> {
        let mut fields = vec![
            ("tx_type", self.tx_type.to_string()),
            ("tx_info", self.tx_info.clone()),
        ];

        if let Some(price_protection) = self.price_protection {
            fields.push(("price_protection", price_protection.to_string()));
        }
        fields
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct LighterSendTxBatchRequest {
    pub tx_types: String,
    pub tx_infos: String,
}

impl LighterSendTxBatchRequest {
    #[must_use]
    pub fn new(tx_types: impl Into<String>, tx_infos: impl Into<String>) -> Self {
        Self {
            tx_types: tx_types.into(),
            tx_infos: tx_infos.into(),
        }
    }

    pub(crate) fn form_fields(&self) -> Vec<(&'static str, String)> {
        vec![
            ("tx_types", self.tx_types.clone()),
            ("tx_infos", self.tx_infos.clone()),
        ]
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct LighterSendTxResponse {
    pub code: i32,
    pub message: Option<String>,
    pub tx_hash: String,
    pub predicted_execution_time_ms: i64,
    pub volume_quota_remaining: Option<i64>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct LighterSendTxBatchResponse {
    pub code: i32,
    pub message: Option<String>,
    pub tx_hash: Vec<String>,
    pub predicted_execution_time_ms: i64,
    pub volume_quota_remaining: Option<i64>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct LighterOrderBooks {
    pub code: i32,
    pub message: Option<String>,
    #[serde(default, deserialize_with = "deserialize_null_vec")]
    pub order_books: Vec<LighterOrderBook>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct LighterOrderBook {
    pub symbol: Ustr,
    pub market_id: i16,
    pub market_type: LighterProductType,
    pub base_asset_id: i16,
    pub quote_asset_id: i16,
    pub status: LighterMarketStatus,
    #[serde(deserialize_with = "deserialize_decimal_from_str")]
    pub taker_fee: Decimal,
    #[serde(deserialize_with = "deserialize_decimal_from_str")]
    pub maker_fee: Decimal,
    #[serde(deserialize_with = "deserialize_decimal_from_str")]
    pub liquidation_fee: Decimal,
    #[serde(deserialize_with = "deserialize_decimal_from_str")]
    pub min_base_amount: Decimal,
    #[serde(deserialize_with = "deserialize_decimal_from_str")]
    pub min_quote_amount: Decimal,
    #[serde(deserialize_with = "deserialize_decimal_from_str")]
    pub order_quote_limit: Decimal,
    pub supported_size_decimals: u8,
    pub supported_price_decimals: u8,
    pub supported_quote_decimals: u8,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct LighterOrderBookDetails {
    pub code: i32,
    pub message: Option<String>,
    #[serde(default, deserialize_with = "deserialize_null_vec")]
    pub order_book_details: Vec<LighterPerpOrderBookDetail>,
    #[serde(default, deserialize_with = "deserialize_null_vec")]
    pub spot_order_book_details: Vec<LighterSpotOrderBookDetail>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct LighterPerpOrderBookDetail {
    #[serde(flatten)]
    pub order_book: LighterOrderBook,
    pub size_decimals: u8,
    pub price_decimals: u8,
    pub quote_multiplier: i64,
    pub default_initial_margin_fraction: u16,
    pub min_initial_margin_fraction: u16,
    pub maintenance_margin_fraction: u16,
    pub closeout_margin_fraction: u16,
    #[serde(deserialize_with = "deserialize_decimal")]
    pub last_trade_price: Decimal,
    pub daily_trades_count: i64,
    #[serde(deserialize_with = "deserialize_decimal")]
    pub daily_base_token_volume: Decimal,
    #[serde(deserialize_with = "deserialize_decimal")]
    pub daily_quote_token_volume: Decimal,
    #[serde(deserialize_with = "deserialize_decimal")]
    pub daily_price_low: Decimal,
    #[serde(deserialize_with = "deserialize_decimal")]
    pub daily_price_high: Decimal,
    #[serde(deserialize_with = "deserialize_decimal")]
    pub daily_price_change: Decimal,
    #[serde(deserialize_with = "deserialize_decimal")]
    pub open_interest: Decimal,
    #[serde(deserialize_with = "deserialize_decimal_btree_map")]
    pub daily_chart: BTreeMap<String, Decimal>,
    pub market_config: LighterMarketConfig,
    pub strategy_index: u8,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct LighterSpotOrderBookDetail {
    #[serde(flatten)]
    pub order_book: LighterOrderBook,
    pub size_decimals: u8,
    pub price_decimals: u8,
    #[serde(deserialize_with = "deserialize_decimal")]
    pub last_trade_price: Decimal,
    pub daily_trades_count: i64,
    #[serde(deserialize_with = "deserialize_decimal")]
    pub daily_base_token_volume: Decimal,
    #[serde(deserialize_with = "deserialize_decimal")]
    pub daily_quote_token_volume: Decimal,
    #[serde(deserialize_with = "deserialize_decimal")]
    pub daily_price_low: Decimal,
    #[serde(deserialize_with = "deserialize_decimal")]
    pub daily_price_high: Decimal,
    #[serde(deserialize_with = "deserialize_decimal")]
    pub daily_price_change: Decimal,
    #[serde(deserialize_with = "deserialize_decimal_btree_map")]
    pub daily_chart: BTreeMap<String, Decimal>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct LighterMarketConfig {
    pub market_margin_mode: LighterPositionMarginMode,
    pub insurance_fund_account_index: i64,
    pub liquidation_mode: i32,
    pub force_reduce_only: bool,
    pub trading_hours: String,
    pub funding_fee_discounts_enabled: bool,
    pub hidden: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct LighterOrderBookOrders {
    pub code: i32,
    pub message: Option<String>,
    pub total_asks: i64,
    #[serde(default, deserialize_with = "deserialize_null_vec")]
    pub asks: Vec<LighterSimpleOrder>,
    pub total_bids: i64,
    #[serde(default, deserialize_with = "deserialize_null_vec")]
    pub bids: Vec<LighterSimpleOrder>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct LighterSimpleOrder {
    pub order_index: i64,
    pub order_id: String,
    pub owner_account_index: i64,
    #[serde(deserialize_with = "deserialize_decimal_from_str")]
    pub initial_base_amount: Decimal,
    #[serde(deserialize_with = "deserialize_decimal_from_str")]
    pub remaining_base_amount: Decimal,
    #[serde(deserialize_with = "deserialize_decimal_from_str")]
    pub price: Decimal,
    pub order_expiry: i64,
    pub transaction_time: i64,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct LighterOrderBookDepth {
    pub code: i32,
    pub message: Option<String>,
    #[serde(default, deserialize_with = "deserialize_null_vec")]
    pub asks: Vec<LighterPriceLevel>,
    #[serde(default, deserialize_with = "deserialize_null_vec")]
    pub bids: Vec<LighterPriceLevel>,
    pub offset: i64,
    pub nonce: i64,
}

/// A single book level returned by the order-book REST snapshot and the
/// WebSocket ticker / order_book / order_book_depth10 frames.
///
/// Lighter sends empty strings on the bid or ask side of a ticker frame when
/// that side currently has no resting orders; [`deserialize_decimal_or_zero`]
/// maps those to [`Decimal::ZERO`]. Downstream parsers must treat a
/// zero-priced (or zero-sized) level as an uninhabited side, not a literal
/// zero quote.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct LighterPriceLevel {
    #[serde(deserialize_with = "deserialize_decimal_or_zero")]
    pub price: Decimal,
    #[serde(deserialize_with = "deserialize_decimal_or_zero")]
    pub size: Decimal,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct LighterTrades {
    pub code: i32,
    pub message: Option<String>,
    pub next_cursor: Option<String>,
    #[serde(default, deserialize_with = "deserialize_null_vec")]
    pub trades: Vec<LighterTrade>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct LighterCandles {
    pub code: i32,
    pub message: Option<String>,
    #[serde(rename = "r")]
    pub resolution: LighterCandleResolution,
    #[serde(rename = "c", default, deserialize_with = "deserialize_null_vec")]
    pub candles: Vec<LighterCandle>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct LighterCandle {
    #[serde(rename = "t")]
    pub timestamp: i64,
    #[serde(rename = "o", default, deserialize_with = "deserialize_decimal")]
    pub open: Decimal,
    #[serde(rename = "h", default, deserialize_with = "deserialize_decimal")]
    pub high: Decimal,
    #[serde(rename = "l", default, deserialize_with = "deserialize_decimal")]
    pub low: Decimal,
    #[serde(rename = "c", default, deserialize_with = "deserialize_decimal")]
    pub close: Decimal,
    #[serde(rename = "v", default, deserialize_with = "deserialize_decimal")]
    pub volume_base: Decimal,
    #[serde(rename = "V", default, deserialize_with = "deserialize_decimal")]
    pub volume_quote: Decimal,
    #[serde(rename = "i")]
    pub last_trade_id: i64,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct LighterFundings {
    pub code: i32,
    pub message: Option<String>,
    pub resolution: LighterFundingResolution,
    #[serde(default, deserialize_with = "deserialize_null_vec")]
    pub fundings: Vec<LighterFunding>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct LighterFunding {
    pub timestamp: i64,
    #[serde(deserialize_with = "deserialize_decimal")]
    pub value: Decimal,
    #[serde(deserialize_with = "deserialize_decimal")]
    pub rate: Decimal,
    pub direction: LighterFundingDirection,
}

#[derive(Copy, Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum LighterFundingDirection {
    Long,
    Short,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct LighterTrade {
    pub trade_id: i64,
    pub trade_id_str: Option<String>,
    pub tx_hash: String,
    #[serde(rename = "type")]
    pub trade_type: LighterTradeType,
    pub market_id: i16,
    #[serde(deserialize_with = "deserialize_decimal_from_str")]
    pub size: Decimal,
    #[serde(deserialize_with = "deserialize_decimal_from_str")]
    pub price: Decimal,
    #[serde(deserialize_with = "deserialize_decimal_from_str")]
    pub usd_amount: Decimal,
    pub ask_id: i64,
    pub ask_id_str: Option<String>,
    pub bid_id: i64,
    pub bid_id_str: Option<String>,
    pub ask_client_id: i64,
    pub ask_client_id_str: Option<String>,
    pub bid_client_id: i64,
    pub bid_client_id_str: Option<String>,
    pub ask_account_id: i64,
    pub bid_account_id: i64,
    pub is_maker_ask: bool,
    pub block_height: i64,
    pub timestamp: i64,
    pub taker_fee: Option<i32>,
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub taker_position_size_before: Option<Decimal>,
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub taker_entry_quote_before: Option<Decimal>,
    pub taker_initial_margin_fraction_before: Option<u16>,
    pub taker_position_sign_changed: Option<bool>,
    pub maker_fee: Option<i32>,
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub maker_position_size_before: Option<Decimal>,
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub maker_entry_quote_before: Option<Decimal>,
    pub maker_initial_margin_fraction_before: Option<u16>,
    pub maker_position_sign_changed: Option<bool>,
    pub transaction_time: i64,
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub ask_account_pnl: Option<Decimal>,
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub bid_account_pnl: Option<Decimal>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct LighterOrders {
    pub code: i32,
    pub message: Option<String>,
    pub next_cursor: Option<String>,
    #[serde(default, deserialize_with = "deserialize_null_vec")]
    pub orders: Vec<LighterOrder>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct LighterOrder {
    pub order_index: i64,
    pub client_order_index: i64,
    pub order_id: String,
    pub client_order_id: String,
    pub market_index: i16,
    pub owner_account_index: i64,
    #[serde(deserialize_with = "deserialize_decimal_from_str")]
    pub initial_base_amount: Decimal,
    #[serde(deserialize_with = "deserialize_decimal_from_str")]
    pub price: Decimal,
    pub nonce: i64,
    #[serde(deserialize_with = "deserialize_decimal_from_str")]
    pub remaining_base_amount: Decimal,
    pub is_ask: bool,
    pub base_size: i64,
    pub base_price: i32,
    #[serde(deserialize_with = "deserialize_decimal_from_str")]
    pub filled_base_amount: Decimal,
    #[serde(deserialize_with = "deserialize_decimal_from_str")]
    pub filled_quote_amount: Decimal,
    #[serde(default, deserialize_with = "deserialize_order_side")]
    pub side: Option<LighterOrderSide>,
    #[serde(rename = "type")]
    pub order_type: LighterOrderKind,
    pub time_in_force: LighterOrderTimeInForce,
    pub reduce_only: bool,
    // Lighter sends an empty string when no trigger price is set.
    #[serde(deserialize_with = "deserialize_decimal_or_zero")]
    pub trigger_price: Decimal,
    pub order_expiry: i64,
    pub status: LighterOrderStatus,
    pub trigger_status: LighterTriggerStatus,
    pub trigger_time: i64,
    pub parent_order_index: i64,
    pub parent_order_id: String,
    pub to_trigger_order_id_0: String,
    pub to_trigger_order_id_1: String,
    pub to_cancel_order_id_0: String,
    pub integrator_fee_collector_index: String,
    #[serde(deserialize_with = "deserialize_decimal_or_zero")]
    pub integrator_taker_fee: Decimal,
    #[serde(deserialize_with = "deserialize_decimal_or_zero")]
    pub integrator_maker_fee: Decimal,
    pub block_height: i64,
    pub timestamp: i64,
    pub created_at: i64,
    pub updated_at: i64,
    pub transaction_time: i64,
}

#[derive(Deserialize)]
struct DecimalFromAny(#[serde(deserialize_with = "deserialize_decimal")] Decimal);

fn deserialize_decimal_btree_map<'de, D>(
    deserializer: D,
) -> Result<BTreeMap<String, Decimal>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    BTreeMap::<String, DecimalFromAny>::deserialize(deserializer).map(|values| {
        values
            .into_iter()
            .map(|(key, decimal)| (key, decimal.0))
            .collect()
    })
}

fn deserialize_null_vec<'de, D, T>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<Vec<T>>::deserialize(deserializer).map(Option::unwrap_or_default)
}

fn deserialize_order_side<'de, D>(deserializer: D) -> Result<Option<LighterOrderSide>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Option::<String>::deserialize(deserializer)?;
    match value.as_deref() {
        None | Some("") => Ok(None),
        Some(s) => LighterOrderSide::from_str(s)
            .map(Some)
            .map_err(|_| de::Error::unknown_variant(s, &["buy", "sell", ""])),
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    const HTTP_ORDER_BOOK_DETAILS: &str =
        include_str!("../../test_data/http_order_book_details.json");
    const HTTP_RECENT_TRADES: &str = include_str!("../../test_data/http_recent_trades.json");
    const HTTP_RECENT_TRADES_MISSING: &str =
        include_str!("../../test_data/http_recent_trades_missing.json");
    const HTTP_RECENT_TRADES_NULL: &str =
        include_str!("../../test_data/http_recent_trades_null.json");
    const HTTP_ORDER_BOOKS: &str = include_str!("../../test_data/http_order_books.json");
    const HTTP_ORDER_BOOK_ORDERS: &str =
        include_str!("../../test_data/http_order_book_orders.json");
    const HTTP_ORDER_BOOK_DEPTH: &str = include_str!("../../test_data/http_order_book_depth.json");
    const HTTP_ORDER_BOOK_DEPTH_NULL: &str =
        include_str!("../../test_data/http_order_book_depth_null.json");
    const HTTP_ORDERS: &str = include_str!("../../test_data/http_orders.json");
    const HTTP_CANDLES: &str = include_str!("../../test_data/http_candles.json");
    const HTTP_CANDLES_NULL: &str = include_str!("../../test_data/http_candles_null.json");
    const HTTP_FUNDINGS: &str = include_str!("../../test_data/http_fundings.json");
    const HTTP_ACCOUNT: &str = include_str!("../../test_data/http_account.json");

    #[rstest]
    fn test_account_response_deserializes_live_shape() {
        let response: LighterAccountsResponse = serde_json::from_str(HTTP_ACCOUNT).unwrap();

        assert_eq!(response.code, 200);
        assert_eq!(response.total, 1);
        assert_eq!(response.accounts.len(), 1);
        let account = &response.accounts[0];
        assert_eq!(account.account_index, 123_456);
        assert_eq!(account.account_type, 0);
        assert_eq!(account.status, 1);
    }

    #[rstest]
    fn test_order_book_details_deserializes_live_shape() {
        let details: LighterOrderBookDetails =
            serde_json::from_str(HTTP_ORDER_BOOK_DETAILS).unwrap();

        assert_eq!(details.code, 200);
        assert_eq!(details.order_book_details.len(), 1);
        assert_eq!(
            details.order_book_details[0].order_book.market_type,
            LighterProductType::Perp,
        );
        assert_eq!(details.order_book_details[0].price_decimals, 2);
        assert_eq!(
            details.order_book_details[0].last_trade_price,
            Decimal::new(236_131, 2),
        );
        assert_eq!(
            details.order_book_details[0].daily_base_token_volume,
            Decimal::new(1_953_991_521, 4),
        );
        assert_eq!(
            details.order_book_details[0]
                .market_config
                .market_margin_mode,
            LighterPositionMarginMode::Cross,
        );
        assert!(details.spot_order_book_details.is_empty());
    }

    #[rstest]
    fn test_recent_trades_allow_omitted_fee_fields() {
        let trades: LighterTrades = serde_json::from_str(HTTP_RECENT_TRADES).unwrap();

        assert_eq!(trades.trades.len(), 1);
        assert_eq!(
            trades.trades[0].trade_id_str.as_deref(),
            Some("19211490282")
        );
        assert_eq!(trades.trades[0].taker_fee, None);
        assert_eq!(trades.trades[0].maker_fee, Some(28));
    }

    #[rstest]
    fn test_recent_trades_deserializes_null_trades_as_empty() {
        let trades: LighterTrades = serde_json::from_str(HTTP_RECENT_TRADES_NULL).unwrap();

        assert_eq!(trades.code, 200);
        assert!(trades.trades.is_empty());
    }

    #[rstest]
    fn test_recent_trades_deserializes_missing_trades_as_empty() {
        let trades: LighterTrades = serde_json::from_str(HTTP_RECENT_TRADES_MISSING).unwrap();

        assert_eq!(trades.code, 200);
        assert!(trades.trades.is_empty());
    }

    #[rstest]
    fn test_candles_deserializes_live_shape_with_omitted_raw_fields() {
        let candles: LighterCandles = serde_json::from_str(HTTP_CANDLES).unwrap();

        assert_eq!(candles.code, 200);
        assert_eq!(candles.resolution, LighterCandleResolution::OneMinute);
        assert_eq!(candles.candles.len(), 2);
        assert_eq!(candles.candles[0].timestamp, 1_700_000_000_000);
        assert_eq!(candles.candles[0].open, Decimal::new(236_111, 2));
        assert_eq!(candles.candles[0].high, Decimal::new(236_222, 2));
        assert_eq!(candles.candles[0].low, Decimal::new(236_000, 2));
        assert_eq!(candles.candles[0].close, Decimal::new(236_131, 2));
        assert_eq!(candles.candles[0].volume_base, Decimal::new(12_345, 4));
        assert_eq!(candles.candles[0].last_trade_id, 19_211_490_282);
    }

    #[rstest]
    fn test_candles_deserializes_null_candles_as_empty() {
        let candles: LighterCandles = serde_json::from_str(HTTP_CANDLES_NULL).unwrap();

        assert_eq!(candles.code, 200);
        assert_eq!(candles.resolution, LighterCandleResolution::OneMinute);
        assert!(candles.candles.is_empty());
    }

    #[rstest]
    fn test_fundings_deserializes_live_shape() {
        let fundings: LighterFundings = serde_json::from_str(HTTP_FUNDINGS).unwrap();

        assert_eq!(fundings.code, 200);
        assert_eq!(fundings.resolution, LighterFundingResolution::OneHour);
        assert_eq!(fundings.fundings.len(), 2);
        assert_eq!(fundings.fundings[0].timestamp, 1_778_702_400);
        assert_eq!(fundings.fundings[0].rate, Decimal::new(12, 4));
        assert_eq!(
            fundings.fundings[0].direction,
            LighterFundingDirection::Long
        );
        assert_eq!(
            fundings.fundings[1].direction,
            LighterFundingDirection::Short
        );
    }

    #[rstest]
    fn test_order_books_deserializes_live_shape() {
        let order_books: LighterOrderBooks = serde_json::from_str(HTTP_ORDER_BOOKS).unwrap();

        assert_eq!(order_books.code, 200);
        assert_eq!(order_books.order_books.len(), 1);
        assert_eq!(order_books.order_books[0].symbol, Ustr::from("ETH"));
        assert_eq!(
            order_books.order_books[0].market_type,
            LighterProductType::Perp
        );
        assert_eq!(
            order_books.order_books[0].status,
            LighterMarketStatus::Active
        );
        assert_eq!(order_books.order_books[0].supported_price_decimals, 2);
    }

    #[rstest]
    fn test_order_book_orders_deserializes_live_shape() {
        let book: LighterOrderBookOrders = serde_json::from_str(HTTP_ORDER_BOOK_ORDERS).unwrap();

        assert_eq!(book.total_asks, 1);
        assert_eq!(book.asks[0].order_id, "281476929689581");
        assert_eq!(book.asks[0].price, Decimal::from_str("2361.32").unwrap());
        assert_eq!(book.total_bids, 1);
        assert_eq!(
            book.bids[0].remaining_base_amount,
            Decimal::from_str("3.4125").unwrap(),
        );
    }

    #[rstest]
    fn test_order_book_depth_deserializes_live_shape() {
        let depth: LighterOrderBookDepth = serde_json::from_str(HTTP_ORDER_BOOK_DEPTH).unwrap();

        assert_eq!(depth.code, 200);
        assert_eq!(depth.asks[0].price, Decimal::from_str("2352.74").unwrap());
        assert_eq!(depth.bids[0].size, Decimal::from_str("0.2125").unwrap());
        assert_eq!(depth.offset, 1_558_300);
        assert_eq!(depth.nonce, 9_182_390_020);
    }

    #[rstest]
    fn test_order_book_depth_deserializes_null_sides_as_empty() {
        let depth: LighterOrderBookDepth =
            serde_json::from_str(HTTP_ORDER_BOOK_DEPTH_NULL).unwrap();

        assert_eq!(depth.code, 200);
        assert!(depth.asks.is_empty());
        assert!(depth.bids.is_empty());
        assert_eq!(depth.offset, 1);
        assert_eq!(depth.nonce, 0);
    }

    #[rstest]
    fn test_orders_deserializes_live_shape() {
        let orders: LighterOrders = serde_json::from_str(HTTP_ORDERS).unwrap();

        assert_eq!(orders.next_cursor.as_deref(), Some("cursor-1"));
        assert_eq!(orders.orders.len(), 1);
        assert_eq!(orders.orders[0].order_type, LighterOrderKind::Limit);
        assert_eq!(
            orders.orders[0].time_in_force,
            LighterOrderTimeInForce::GoodTillTime,
        );
        assert_eq!(orders.orders[0].status, LighterOrderStatus::Open);
        assert_eq!(orders.orders[0].trigger_status, LighterTriggerStatus::Na);
        assert_eq!(orders.orders[0].side, Some(LighterOrderSide::Sell));
        assert!(orders.orders[0].is_ask);
    }

    #[rstest]
    fn test_orders_allows_empty_side_with_is_ask() {
        let mut value: serde_json::Value = serde_json::from_str(HTTP_ORDERS).unwrap();
        value["orders"][0]["side"] = serde_json::Value::String(String::new());

        let orders: LighterOrders = serde_json::from_value(value).unwrap();

        assert_eq!(orders.orders[0].side, None);
        assert!(orders.orders[0].is_ask);
    }
}
