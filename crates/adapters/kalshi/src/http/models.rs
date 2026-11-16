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

//! Kalshi REST API models.

use std::fmt::Debug;

use jiff::Timestamp;
use nautilus_core::serialization::decimal;
use rust_decimal::Decimal;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use strum::EnumIter;

use crate::common::enums::{
    ApiUsageTier, CollateralReturnType, ExchangeInstance, FeeType, MarketResult, MarketStatus,
    MarketType, OrderType, PriceLevelStructure, SettlementBoundsType, UsageGrantSource,
};

/// A fixed-point wire string, retaining its exact decimal value and scale.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FixedPoint(pub Decimal);

/// An exact JSON number decoded from its source text without a binary float conversion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DecimalNumber(pub Decimal);

impl Serialize for DecimalNumber {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let raw = serde_json::value::RawValue::from_string(self.0.to_string())
            .map_err(serde::ser::Error::custom)?;
        raw.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for DecimalNumber {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = Box::<serde_json::value::RawValue>::deserialize(deserializer)?;
        decimal::parse(raw.get())
            .map(Self)
            .map_err(|_| serde::de::Error::custom("invalid exact decimal number"))
    }
}

impl Serialize for FixedPoint {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0.to_string())
    }
}

impl<'de> Deserialize<'de> for FixedPoint {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        let digits = value.strip_prefix('-').unwrap_or(&value);
        let valid = !digits.is_empty()
            && digits.bytes().all(|c| c.is_ascii_digit() || c == b'.')
            && !digits.starts_with('.')
            && !digits.ends_with('.');

        if !valid {
            return Err(serde::de::Error::custom("invalid fixed-point string"));
        }

        Decimal::from_str_exact(&value)
            .map(Self)
            .map_err(|_| serde::de::Error::custom("fixed-point value exceeds decimal precision"))
    }
}

/// A wire field whose absence differs from explicit null and from a value.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum Field<T> {
    #[default]
    Missing,
    Null,
    Value(T),
}

impl<T> Field<T> {
    /// Returns whether the field is absent from the payload.
    #[must_use]
    pub const fn is_missing(&self) -> bool {
        matches!(self, Self::Missing)
    }
}

impl<T: Serialize> Serialize for Field<T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Missing | Self::Null => serializer.serialize_none(),
            Self::Value(value) => value.serialize(serializer),
        }
    }
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for Field<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Option::<T>::deserialize(deserializer).map(|value| value.map_or(Self::Null, Self::Value))
    }
}

/// The YES-price order-book side.
#[derive(Debug, Clone, Copy, PartialEq, Eq, EnumIter, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BookSide {
    Bid,
    Ask,
}

/// The contract outcome side.
#[derive(Debug, Clone, Copy, PartialEq, Eq, EnumIter, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OutcomeSide {
    Yes,
    No,
}

/// A venue order state; unknown states require explicit domain rejection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, EnumIter, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OrderStatus {
    Resting,
    Canceled,
    Executed,
    #[serde(other)]
    Unknown,
}

/// Self-trade prevention policy accepted by V2 orders.
#[derive(Debug, Clone, Copy, PartialEq, Eq, EnumIter, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SelfTradePrevention {
    TakerAtCross,
    Maker,
}

/// V2 time in force; expiration uses good-till-canceled with a timestamp.
#[derive(Debug, Clone, Copy, PartialEq, Eq, EnumIter, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TimeInForce {
    FillOrKill,
    GoodTillCanceled,
    ImmediateOrCancel,
}

/// Public exchange availability, including per-shard state when supplied.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExchangeStatus {
    pub exchange_active: bool,
    pub trading_active: bool,
    #[serde(default, skip_serializing_if = "Field::is_missing")]
    pub intra_exchange_transfers_active: Field<bool>,
    #[serde(default, skip_serializing_if = "Field::is_missing")]
    pub exchange_estimated_resume_time: Field<Timestamp>,
    #[serde(default, skip_serializing_if = "Field::is_missing")]
    pub exchange_index_statuses: Field<Vec<ExchangeIndexStatus>>,
}

/// Availability of one exchange shard.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExchangeIndexStatus {
    pub exchange_index: i32,
    pub description: String,
    pub exchange_active: bool,
    pub trading_active: bool,
    pub intra_exchange_transfers_active: bool,
}

/// A fixed-point market price-grid interval.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PriceRange {
    pub start: FixedPoint,
    pub end: FixedPoint,
    pub step: FixedPoint,
}

/// Raw market metadata and exact quote, volume, and settlement values.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Market {
    pub ticker: String,
    pub event_ticker: String,
    pub market_type: MarketType,
    pub yes_sub_title: String,
    pub no_sub_title: String,
    pub created_time: Timestamp,
    #[serde(default, skip_serializing_if = "Field::is_missing")]
    pub updated_time: Field<Timestamp>,
    pub open_time: Timestamp,
    pub close_time: Timestamp,
    pub latest_expiration_time: Timestamp,
    pub settlement_timer_seconds: i64,
    pub status: MarketStatus,
    pub yes_bid_dollars: FixedPoint,
    pub yes_bid_size_fp: FixedPoint,
    pub yes_ask_dollars: FixedPoint,
    pub yes_ask_size_fp: FixedPoint,
    pub no_bid_dollars: FixedPoint,
    pub no_ask_dollars: FixedPoint,
    pub last_price_dollars: FixedPoint,
    pub volume_fp: FixedPoint,
    pub volume_24h_fp: FixedPoint,
    pub result: MarketResult,
    pub can_close_early: bool,
    pub open_interest_fp: FixedPoint,
    pub notional_value_dollars: FixedPoint,
    pub previous_yes_bid_dollars: FixedPoint,
    pub previous_yes_ask_dollars: FixedPoint,
    pub previous_price_dollars: FixedPoint,
    pub settlement_bounds_type: SettlementBoundsType,
    pub expiration_value: String,
    pub rules_primary: String,
    pub rules_secondary: String,
    pub price_level_structure: PriceLevelStructure,
    pub price_ranges: Vec<PriceRange>,
    #[serde(default, skip_serializing_if = "Field::is_missing")]
    pub exchange_index: Field<i32>,
    #[serde(default, skip_serializing_if = "Field::is_missing")]
    pub settlement_value_dollars: Field<FixedPoint>,
    #[serde(default, skip_serializing_if = "Field::is_missing")]
    pub settlement_floor_dollars: Field<FixedPoint>,
    #[serde(default, skip_serializing_if = "Field::is_missing")]
    pub settlement_ts: Field<Timestamp>,
    #[serde(default, skip_serializing_if = "Field::is_missing")]
    pub expected_expiration_time: Field<Timestamp>,
    #[serde(default, skip_serializing_if = "Field::is_missing")]
    pub mve_collection_ticker: Field<String>,
    #[serde(default, skip_serializing_if = "Field::is_missing")]
    pub mve_selected_legs: Field<Vec<serde_json::Value>>,
}

/// Event identity and collateral metadata, with optional nested markets.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Event {
    pub event_ticker: String,
    pub series_ticker: String,
    pub sub_title: String,
    pub title: String,
    pub collateral_return_type: CollateralReturnType,
    pub mutually_exclusive: bool,
    #[serde(deserialize_with = "Option::deserialize")]
    pub settlement_sources: Option<Vec<SettlementSource>>,
    #[serde(default, skip_serializing_if = "Field::is_missing")]
    pub markets: Field<Vec<Market>>,
    #[serde(default, skip_serializing_if = "Field::is_missing")]
    pub exchange_index: Field<i32>,
    #[serde(default, skip_serializing_if = "Field::is_missing")]
    pub last_updated_ts: Field<Timestamp>,
    #[serde(default, skip_serializing_if = "Field::is_missing")]
    pub strike_date: Field<Timestamp>,
    #[serde(default, skip_serializing_if = "Field::is_missing")]
    pub strike_period: Field<String>,
    #[serde(default, skip_serializing_if = "Field::is_missing")]
    pub fee_type_override: Field<FeeType>,
    #[serde(default, skip_serializing_if = "Field::is_missing")]
    pub fee_multiplier_override: Field<DecimalNumber>,
}

/// Provider settlement-source reference.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SettlementSource {
    #[serde(default, skip_serializing_if = "Field::is_missing")]
    pub name: Field<String>,
    #[serde(default, skip_serializing_if = "Field::is_missing")]
    pub url: Field<String>,
}

/// Series metadata used by discovery and fee qualification.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Series {
    pub ticker: String,
    pub frequency: String,
    pub title: String,
    pub category: String,
    pub categories: Vec<String>,
    #[serde(deserialize_with = "Option::deserialize")]
    pub tags: Option<Vec<String>>,
    #[serde(deserialize_with = "Option::deserialize")]
    pub settlement_sources: Option<Vec<SettlementSource>>,
    pub contract_url: String,
    pub contract_terms_url: String,
    pub fee_type: FeeType,
    pub fee_multiplier: DecimalNumber,
    #[serde(deserialize_with = "Option::deserialize")]
    pub additional_prohibitions: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Field::is_missing")]
    pub volume_fp: Field<FixedPoint>,
    #[serde(default, skip_serializing_if = "Field::is_missing")]
    pub last_updated_ts: Field<Timestamp>,
    #[serde(default, skip_serializing_if = "Field::is_missing")]
    pub exchange_index: Field<i32>,
}

/// Fixed-point YES and NO bid levels, each encoded as price and count.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Orderbook {
    pub yes_dollars: Vec<[FixedPoint; 2]>,
    pub no_dollars: Vec<[FixedPoint; 2]>,
}

/// Public trade with canonical taker-side fields.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Trade {
    pub trade_id: String,
    pub ticker: String,
    pub count_fp: FixedPoint,
    pub yes_price_dollars: FixedPoint,
    pub no_price_dollars: FixedPoint,
    pub taker_outcome_side: OutcomeSide,
    pub taker_book_side: BookSide,
    pub created_time: Timestamp,
    pub is_block_trade: bool,
}

/// Bid or ask candle prices.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BidAskDistribution {
    pub open_dollars: FixedPoint,
    pub low_dollars: FixedPoint,
    pub high_dollars: FixedPoint,
    pub close_dollars: FixedPoint,
}

/// Trade candle prices; missing and null prices remain distinct.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PriceDistribution {
    #[serde(default, skip_serializing_if = "Field::is_missing")]
    pub open_dollars: Field<FixedPoint>,
    #[serde(default, skip_serializing_if = "Field::is_missing")]
    pub low_dollars: Field<FixedPoint>,
    #[serde(default, skip_serializing_if = "Field::is_missing")]
    pub high_dollars: Field<FixedPoint>,
    #[serde(default, skip_serializing_if = "Field::is_missing")]
    pub close_dollars: Field<FixedPoint>,
    #[serde(default, skip_serializing_if = "Field::is_missing")]
    pub mean_dollars: Field<FixedPoint>,
    #[serde(default, skip_serializing_if = "Field::is_missing")]
    pub previous_dollars: Field<FixedPoint>,
    #[serde(default, skip_serializing_if = "Field::is_missing")]
    pub min_dollars: Field<FixedPoint>,
    #[serde(default, skip_serializing_if = "Field::is_missing")]
    pub max_dollars: Field<FixedPoint>,
}

/// One candle; the period end is Unix time in seconds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Candlestick {
    pub end_period_ts: i64,
    pub yes_bid: BidAskDistribution,
    pub yes_ask: BidAskDistribution,
    pub price: PriceDistribution,
    pub volume_fp: FixedPoint,
    pub open_interest_fp: FixedPoint,
}

/// Archived bid/ask prices use unsuffixed dollar fields.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoricalBidAskDistribution {
    pub open: FixedPoint,
    pub low: FixedPoint,
    pub high: FixedPoint,
    pub close: FixedPoint,
}

/// Archived trade prices are required nullable dollar fields.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoricalPriceDistribution {
    #[serde(deserialize_with = "Option::deserialize")]
    pub open: Option<FixedPoint>,
    #[serde(deserialize_with = "Option::deserialize")]
    pub low: Option<FixedPoint>,
    #[serde(deserialize_with = "Option::deserialize")]
    pub high: Option<FixedPoint>,
    #[serde(deserialize_with = "Option::deserialize")]
    pub close: Option<FixedPoint>,
    #[serde(deserialize_with = "Option::deserialize")]
    pub mean: Option<FixedPoint>,
    #[serde(deserialize_with = "Option::deserialize")]
    pub previous: Option<FixedPoint>,
}

/// An archived candle with exact unsuffixed count strings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoricalCandlestick {
    pub end_period_ts: i64,
    pub yes_bid: HistoricalBidAskDistribution,
    pub yes_ask: HistoricalBidAskDistribution,
    pub price: HistoricalPriceDistribution,
    pub volume: FixedPoint,
    pub open_interest: FixedPoint,
}

/// Archive partition cutoffs as exact timestamps.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoricalCutoff {
    pub market_settled_ts: Timestamp,
    pub trades_created_ts: Timestamp,
    pub orders_updated_ts: Timestamp,
    #[serde(default, skip_serializing_if = "Field::is_missing")]
    pub market_positions_last_updated_ts: Field<Timestamp>,
}

/// Subaccount collateral; integer amounts use venue cents.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Balance {
    pub balance: i64,
    pub balance_dollars: FixedPoint,
    pub portfolio_value: i64,
    pub updated_ts: i64,
    #[serde(default, skip_serializing_if = "Field::is_missing")]
    pub balance_breakdown: Field<Vec<IndexedBalance>>,
}

/// Dollar collateral for one exchange shard.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexedBalance {
    pub exchange_index: i32,
    pub balance: FixedPoint,
}

/// Signed market position and exact economics, without an inferred entry price.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MarketPosition {
    pub ticker: String,
    pub exchange_index: i32,
    pub total_traded_dollars: FixedPoint,
    pub position_fp: FixedPoint,
    pub market_exposure_dollars: FixedPoint,
    pub realized_pnl_dollars: FixedPoint,
    pub fees_paid_dollars: FixedPoint,
    pub last_updated_ts: Timestamp,
}

/// Event-level position economics.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventPosition {
    pub event_ticker: String,
    pub total_cost_dollars: FixedPoint,
    pub total_cost_shares_fp: FixedPoint,
    pub event_exposure_dollars: FixedPoint,
    pub realized_pnl_dollars: FixedPoint,
    pub fees_paid_dollars: FixedPoint,
}

/// A settlement record still available in the live tier.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Settlement {
    pub ticker: String,
    pub exchange_index: i32,
    pub event_ticker: String,
    pub market_result: MarketResult,
    pub yes_count_fp: FixedPoint,
    pub yes_total_cost_dollars: FixedPoint,
    pub no_count_fp: FixedPoint,
    pub no_total_cost_dollars: FixedPoint,
    pub revenue: i64,
    pub settled_time: Timestamp,
    pub fee_cost: FixedPoint,
    #[serde(default, skip_serializing_if = "Field::is_missing")]
    pub value: Field<i64>,
}

/// A private fill with stable fill, trade, and order identities.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Fill {
    pub fill_id: String,
    pub exchange_index: i32,
    pub trade_id: String,
    pub order_id: String,
    pub ticker: String,
    pub market_ticker: String,
    pub outcome_side: OutcomeSide,
    pub book_side: BookSide,
    pub count_fp: FixedPoint,
    pub yes_price_dollars: FixedPoint,
    pub no_price_dollars: FixedPoint,
    pub is_taker: bool,
    pub fee_cost: FixedPoint,
    #[serde(default, skip_serializing_if = "Field::is_missing")]
    pub created_time: Field<Timestamp>,
    #[serde(default, skip_serializing_if = "Field::is_missing")]
    pub subaccount_number: Field<i32>,
    #[serde(default, skip_serializing_if = "Field::is_missing")]
    pub ts: Field<i64>,
}

/// A raw order report retaining exact costs, fees, and canonical sides.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Order {
    pub order_id: String,
    pub user_id: String,
    pub client_order_id: String,
    pub ticker: String,
    pub outcome_side: OutcomeSide,
    pub book_side: BookSide,
    #[serde(rename = "type")]
    pub order_type: OrderType,
    pub status: OrderStatus,
    pub yes_price_dollars: FixedPoint,
    pub no_price_dollars: FixedPoint,
    pub fill_count_fp: FixedPoint,
    pub remaining_count_fp: FixedPoint,
    pub initial_count_fp: FixedPoint,
    pub taker_fill_cost_dollars: FixedPoint,
    pub maker_fill_cost_dollars: FixedPoint,
    pub taker_fees_dollars: FixedPoint,
    pub maker_fees_dollars: FixedPoint,
    #[serde(default, skip_serializing_if = "Field::is_missing")]
    pub expiration_time: Field<Timestamp>,
    #[serde(default, skip_serializing_if = "Field::is_missing")]
    pub created_time: Field<Timestamp>,
    #[serde(default, skip_serializing_if = "Field::is_missing")]
    pub last_update_time: Field<Timestamp>,
    #[serde(default, skip_serializing_if = "Field::is_missing")]
    pub self_trade_prevention_type: Field<SelfTradePrevention>,
    #[serde(default, skip_serializing_if = "Field::is_missing")]
    pub order_group_id: Field<String>,
    #[serde(default, skip_serializing_if = "Field::is_missing")]
    pub cancel_order_on_pause: Field<bool>,
    #[serde(default, skip_serializing_if = "Field::is_missing")]
    pub subaccount_number: Field<i32>,
    #[serde(default, skip_serializing_if = "Field::is_missing")]
    pub exchange_index: Field<i32>,
}

/// Provider error fields. Diagnostics must redact their potentially private values.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VenueError {
    #[serde(default, skip_serializing_if = "Field::is_missing")]
    pub code: Field<String>,
    #[serde(default, skip_serializing_if = "Field::is_missing")]
    pub message: Field<String>,
    #[serde(default, skip_serializing_if = "Field::is_missing")]
    pub details: Field<String>,
}

impl Debug for VenueError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(VenueError))
            .finish_non_exhaustive()
    }
}

/// A V2 create acknowledgement; absent average prices do not imply zero.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateOrderResponse {
    pub order_id: String,
    pub fill_count: FixedPoint,
    pub remaining_count: FixedPoint,
    pub ts_ms: i64,
    #[serde(default, skip_serializing_if = "Field::is_missing")]
    pub client_order_id: Field<String>,
    #[serde(default, skip_serializing_if = "Field::is_missing")]
    pub average_fill_price: Field<FixedPoint>,
    #[serde(default, skip_serializing_if = "Field::is_missing")]
    pub average_fee_paid: Field<FixedPoint>,
}

/// A V2 amend acknowledgement.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AmendOrderResponse {
    pub order_id: String,
    pub ts_ms: i64,
    #[serde(default, skip_serializing_if = "Field::is_missing")]
    pub client_order_id: Field<String>,
    #[serde(default, skip_serializing_if = "Field::is_missing")]
    pub remaining_count: Field<FixedPoint>,
    #[serde(default, skip_serializing_if = "Field::is_missing")]
    pub fill_count: Field<FixedPoint>,
    #[serde(default, skip_serializing_if = "Field::is_missing")]
    pub average_fill_price: Field<FixedPoint>,
    #[serde(default, skip_serializing_if = "Field::is_missing")]
    pub average_fee_paid: Field<FixedPoint>,
}

/// A V2 decrease acknowledgement.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DecreaseOrderResponse {
    pub order_id: String,
    pub remaining_count: FixedPoint,
    pub ts_ms: i64,
    #[serde(default, skip_serializing_if = "Field::is_missing")]
    pub client_order_id: Field<String>,
}

/// A V2 cancellation acknowledgement.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CancelOrderResponse {
    pub order_id: String,
    pub reduced_by: FixedPoint,
    pub ts_ms: i64,
    #[serde(default, skip_serializing_if = "Field::is_missing")]
    pub client_order_id: Field<String>,
}

/// One independent batch-create outcome, including venue rejection evidence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BatchCreateOrder {
    #[serde(default, skip_serializing_if = "Field::is_missing")]
    pub order_id: Field<String>,
    #[serde(default, skip_serializing_if = "Field::is_missing")]
    pub client_order_id: Field<String>,
    #[serde(default, skip_serializing_if = "Field::is_missing")]
    pub fill_count: Field<FixedPoint>,
    #[serde(default, skip_serializing_if = "Field::is_missing")]
    pub remaining_count: Field<FixedPoint>,
    #[serde(default, skip_serializing_if = "Field::is_missing")]
    pub average_fill_price: Field<FixedPoint>,
    #[serde(default, skip_serializing_if = "Field::is_missing")]
    pub average_fee_paid: Field<FixedPoint>,
    #[serde(default, skip_serializing_if = "Field::is_missing")]
    pub ts_ms: Field<i64>,
    #[serde(default, skip_serializing_if = "Field::is_missing")]
    pub error: Field<VenueError>,
}

/// One independent batch-cancel outcome.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BatchCancelOrder {
    pub order_id: String,
    pub reduced_by: FixedPoint,
    #[serde(default, skip_serializing_if = "Field::is_missing")]
    pub client_order_id: Field<String>,
    #[serde(default, skip_serializing_if = "Field::is_missing")]
    pub ts_ms: Field<i64>,
    #[serde(default, skip_serializing_if = "Field::is_missing")]
    pub error: Field<VenueError>,
}

/// Token-bucket refill and capacity advertised by the venue.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BucketLimit {
    pub refill_rate: i64,
    pub bucket_capacity: i64,
}

/// A provider usage-level grant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UsageGrant {
    pub exchange_instance: ExchangeInstance,
    pub level: ApiUsageTier,
    pub source: UsageGrantSource,
    #[serde(default, skip_serializing_if = "Field::is_missing")]
    pub expires_ts: Field<i64>,
}

/// Account token limits; enforcement belongs to the shared budget owner.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountLimits {
    pub usage_tier: ApiUsageTier,
    pub read: BucketLimit,
    pub write: BucketLimit,
    pub grants: Vec<UsageGrant>,
}

/// An endpoint's advertised token cost.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EndpointCost {
    pub method: String,
    pub path: String,
    pub cost: i64,
}

/// Default and endpoint-specific token costs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EndpointCosts {
    pub default_cost: i64,
    pub endpoint_costs: Vec<EndpointCost>,
}

/// A market response envelope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MarketResponse {
    pub market: Market,
}

/// A paginated markets response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MarketsResponse {
    pub markets: Vec<Market>,
    #[serde(deserialize_with = "Field::deserialize")]
    pub cursor: Field<String>,
}

/// An event response, including the separately returned markets.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventResponse {
    pub event: Event,
    pub markets: Vec<Market>,
}

/// A paginated events response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventsResponse {
    pub events: Vec<Event>,
    #[serde(deserialize_with = "Field::deserialize")]
    pub cursor: Field<String>,
}

/// A series response envelope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SeriesResponse {
    pub series: Series,
}

/// An unpaginated series response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SeriesListResponse {
    pub series: Vec<Series>,
}

/// A REST order-book envelope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrderbookResponse {
    pub orderbook_fp: Orderbook,
}

/// A paginated trades response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TradesResponse {
    pub trades: Vec<Trade>,
    #[serde(deserialize_with = "Field::deserialize")]
    pub cursor: Field<String>,
}

/// A live market's candles.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CandlesticksResponse {
    pub ticker: String,
    pub candlesticks: Vec<Candlestick>,
}

/// An archived market's candles.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoricalCandlesticksResponse {
    pub ticker: String,
    pub candlesticks: Vec<HistoricalCandlestick>,
}

/// Market and event positions from one live or archived page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PositionsResponse {
    pub market_positions: Vec<MarketPosition>,
    pub event_positions: Vec<EventPosition>,
    #[serde(default, skip_serializing_if = "Field::is_missing")]
    pub cursor: Field<String>,
}

/// Collected market/event positions and explicit pagination completeness.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PositionPages {
    pub market_positions: Vec<MarketPosition>,
    pub event_positions: Vec<EventPosition>,
    pub complete: bool,
    pub next_cursor: Option<String>,
}

/// A paginated settlements response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SettlementsResponse {
    pub settlements: Vec<Settlement>,
    #[serde(default, skip_serializing_if = "Field::is_missing")]
    pub cursor: Field<String>,
}

/// A paginated private fills response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FillsResponse {
    pub fills: Vec<Fill>,
    #[serde(deserialize_with = "Field::deserialize")]
    pub cursor: Field<String>,
}

/// A paginated orders response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrdersResponse {
    pub orders: Vec<Order>,
    #[serde(deserialize_with = "Field::deserialize")]
    pub cursor: Field<String>,
}

/// A single order report envelope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrderResponse {
    pub order: Order,
}

/// Independent batch-create results.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BatchCreateResponse {
    pub orders: Vec<BatchCreateOrder>,
}

/// Independent selected-ID batch-cancel results.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BatchCancelResponse {
    pub orders: Vec<BatchCancelOrder>,
}
