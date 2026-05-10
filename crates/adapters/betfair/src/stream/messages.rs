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

//! Betfair Exchange Stream API message definitions.
//!
//! The stream protocol uses newline-delimited JSON with an `op` field to
//! discriminate message types. Field names are abbreviated for bandwidth
//! efficiency (e.g. `pt` for publish time, `mc` for market changes).
//!
//! # References
//!
//! <https://docs.developer.betfair.com/display/1smk3cen4v3lu3yomq5qye0ni/Exchange+Stream+API>

use std::str::FromStr;

use ahash::AHashMap;
use nautilus_core::serialization::{deserialize_decimal, deserialize_optional_decimal};
use rust_decimal::Decimal;
use serde::{Deserialize, Deserializer, Serialize, de::Visitor};
use ustr::Ustr;

use crate::common::{
    consts::{STREAM_OP_AUTHENTICATION, STREAM_OP_HEARTBEAT, STREAM_OP_RACE_SUBSCRIPTION},
    enums::{
        ChangeType, LapseStatusReasonCode, MarketBettingType, MarketDataFilterField, MarketStatus,
        PriceLadderType, RunnerStatus, SegmentType, StatusErrorCode, StreamingOrderStatus,
        StreamingOrderType, StreamingPersistenceType, StreamingSide,
    },
    types::{
        Handicap, MarketId, SelectionId, deserialize_optional_string_lenient,
        deserialize_selection_id,
    },
};

/// Top-level streaming message, discriminated by the `op` field.
///
/// Deserializing a raw JSON line into this enum replaces the Python
/// `stream_decode()` function from `betfair_parser`.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "op")]
pub enum StreamMessage {
    #[serde(rename = "connection")]
    Connection(Connection),
    #[serde(rename = "status")]
    Status(Status),
    #[serde(rename = "mcm")]
    MarketChange(MCM),
    #[serde(rename = "ocm")]
    OrderChange(OCM),
    #[serde(rename = "rcm")]
    RaceChange(RCM),
}

/// Connection confirmation sent on stream connect.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Connection {
    pub id: Option<u64>,
    pub connection_id: String,
}

/// Status response for errors or informational messages.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub id: Option<u64>,
    pub connection_closed: bool,
    pub connection_id: Option<String>,
    pub connections_available: Option<u32>,
    pub error_code: Option<StatusErrorCode>,
    pub error_message: Option<String>,
    pub status_code: Option<String>,
}

/// Market Change Message (MCM) - price/market data updates.
#[derive(Debug, Clone, Deserialize)]
pub struct MCM {
    pub id: Option<u64>,
    /// Publish time (epoch millis).
    pub pt: u64,
    /// Token used for resubscription.
    pub clk: Option<String>,
    /// Initial clock token (sent on first image).
    #[serde(rename = "initialClk")]
    pub initial_clk: Option<String>,
    /// Market changes (None on heartbeat).
    pub mc: Option<Vec<MarketChange>>,
    /// Change type.
    pub ct: Option<ChangeType>,
    /// Conflation interval in milliseconds.
    #[serde(rename = "conflateMs")]
    pub conflate_ms: Option<u64>,
    /// Heartbeat interval in milliseconds.
    #[serde(rename = "heartbeatMs")]
    pub heartbeat_ms: Option<u64>,
    /// Segment type for large messages.
    #[serde(rename = "segmentType")]
    pub segment_type: Option<SegmentType>,
    pub status: Option<i32>,
}

impl MCM {
    #[must_use]
    pub fn is_heartbeat(&self) -> bool {
        self.ct == Some(ChangeType::Heartbeat)
    }
}

/// Order Change Message (OCM) - order/position updates.
#[derive(Debug, Clone, Deserialize)]
pub struct OCM {
    pub id: Option<u64>,
    /// Publish time (epoch millis).
    pub pt: u64,
    pub clk: Option<String>,
    #[serde(rename = "initialClk")]
    pub initial_clk: Option<String>,
    /// Order market changes (None on heartbeat).
    pub oc: Option<Vec<OrderMarketChange>>,
    pub ct: Option<ChangeType>,
    #[serde(rename = "conflateMs")]
    pub conflate_ms: Option<u64>,
    #[serde(rename = "heartbeatMs")]
    pub heartbeat_ms: Option<u64>,
    #[serde(rename = "segmentType")]
    pub segment_type: Option<SegmentType>,
    pub status: Option<i32>,
}

impl OCM {
    #[must_use]
    pub fn is_heartbeat(&self) -> bool {
        self.ct == Some(ChangeType::Heartbeat)
    }
}

/// Delta update for a single market.
#[derive(Debug, Clone, Deserialize)]
pub struct MarketChange {
    /// Market identifier.
    pub id: MarketId,
    /// Runner changes.
    pub rc: Option<Vec<RunnerChange>>,
    /// Whether there was a conflation.
    pub con: Option<bool>,
    /// Whether this is a full image (vs delta).
    #[serde(default)]
    pub img: bool,
    /// Full market definition (sent on subscription or change).
    #[serde(rename = "marketDefinition")]
    pub market_definition: Option<MarketDefinition>,
    /// Total volume matched on this market.
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub tv: Option<Decimal>,
}

/// Delta update for a single runner (selection).
#[derive(Debug, Clone, Deserialize)]
pub struct RunnerChange {
    /// Selection identifier.
    #[serde(deserialize_with = "deserialize_selection_id")]
    pub id: SelectionId,
    /// Handicap value.
    pub hc: Option<Handicap>,
    /// Available to back.
    pub atb: Option<Vec<PV>>,
    /// Available to lay.
    pub atl: Option<Vec<PV>>,
    /// Best available to back (depth).
    pub batb: Option<Vec<LPV>>,
    /// Best available to lay (depth).
    pub batl: Option<Vec<LPV>>,
    /// Best display available to back.
    pub bdatb: Option<Vec<LPV>>,
    /// Best display available to lay.
    pub bdatl: Option<Vec<LPV>>,
    /// Starting price back.
    pub spb: Option<Vec<PV>>,
    /// Starting price lay.
    pub spl: Option<Vec<PV>>,
    /// Starting price near (projected SP).
    #[serde(default, deserialize_with = "deserialize_optional_decimal_lenient")]
    pub spn: Option<Decimal>,
    /// Starting price far (actual BSP).
    #[serde(default, deserialize_with = "deserialize_optional_decimal_lenient")]
    pub spf: Option<Decimal>,
    /// Traded volume by price level.
    pub trd: Option<Vec<PV>>,
    /// Last traded price.
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub ltp: Option<Decimal>,
    /// Total volume matched on this runner.
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub tv: Option<Decimal>,
}

fn deserialize_optional_decimal_lenient<'de, D>(
    deserializer: D,
) -> Result<Option<Decimal>, D::Error>
where
    D: Deserializer<'de>,
{
    struct LenientOptionalDecimalVisitor;

    impl Visitor<'_> for LenientOptionalDecimalVisitor {
        type Value = Option<Decimal>;

        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("null or a decimal number as string, integer, or float")
        }

        fn visit_str<E: serde::de::Error>(self, value: &str) -> Result<Self::Value, E> {
            Ok(parse_optional_decimal_lenient(value))
        }

        fn visit_string<E: serde::de::Error>(self, value: String) -> Result<Self::Value, E> {
            self.visit_str(&value)
        }

        fn visit_i64<E: serde::de::Error>(self, value: i64) -> Result<Self::Value, E> {
            Ok(Some(Decimal::from(value)))
        }

        fn visit_u64<E: serde::de::Error>(self, value: u64) -> Result<Self::Value, E> {
            Ok(Some(Decimal::from(value)))
        }

        fn visit_i128<E: serde::de::Error>(self, value: i128) -> Result<Self::Value, E> {
            Ok(Some(Decimal::from(value)))
        }

        fn visit_u128<E: serde::de::Error>(self, value: u128) -> Result<Self::Value, E> {
            Ok(Some(Decimal::from(value)))
        }

        fn visit_f64<E: serde::de::Error>(self, value: f64) -> Result<Self::Value, E> {
            Ok(Decimal::try_from(value).ok())
        }

        fn visit_unit<E: serde::de::Error>(self) -> Result<Self::Value, E> {
            Ok(None)
        }

        fn visit_none<E: serde::de::Error>(self) -> Result<Self::Value, E> {
            Ok(None)
        }
    }

    deserializer.deserialize_any(LenientOptionalDecimalVisitor)
}

fn parse_optional_decimal_lenient(value: &str) -> Option<Decimal> {
    let trimmed = value.trim();
    if trimmed.is_empty() || is_non_finite_decimal(trimmed) {
        return None;
    }

    if trimmed.contains('e') || trimmed.contains('E') {
        Decimal::from_scientific(trimmed).ok()
    } else {
        Decimal::from_str(trimmed).ok()
    }
}

fn is_non_finite_decimal(value: &str) -> bool {
    value.eq_ignore_ascii_case("nan")
        || value.eq_ignore_ascii_case("inf")
        || value.eq_ignore_ascii_case("+inf")
        || value.eq_ignore_ascii_case("-inf")
        || value.eq_ignore_ascii_case("infinity")
        || value.eq_ignore_ascii_case("+infinity")
        || value.eq_ignore_ascii_case("-infinity")
}

/// Full market definition snapshot.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MarketDefinition {
    pub bet_delay: Option<i32>,
    pub betting_type: Option<MarketBettingType>,
    pub bsp_market: Option<bool>,
    pub bsp_reconciled: Option<bool>,
    pub competition_id: Option<String>,
    pub competition_name: Option<String>,
    pub complete: Option<bool>,
    pub country_code: Option<Ustr>,
    pub cross_matching: Option<bool>,
    pub discount_allowed: Option<bool>,
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub each_way_divisor: Option<Decimal>,
    pub event_id: Option<String>,
    pub event_name: Option<String>,
    #[serde(default, deserialize_with = "deserialize_optional_string_lenient")]
    pub event_type_id: Option<String>,
    pub event_type_name: Option<Ustr>,
    pub in_play: Option<bool>,
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub line_interval: Option<Decimal>,
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub line_max_unit: Option<Decimal>,
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub line_min_unit: Option<Decimal>,
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub market_base_rate: Option<Decimal>,
    pub market_id: Option<MarketId>,
    pub market_name: Option<String>,
    pub market_time: Option<String>,
    pub market_type: Option<Ustr>,
    pub number_of_active_runners: Option<u32>,
    pub number_of_winners: Option<u32>,
    pub open_date: Option<String>,
    pub persistence_enabled: Option<bool>,
    pub price_ladder_definition: Option<PriceLadderDefinition>,
    pub race_type: Option<Ustr>,
    pub regulators: Option<Vec<Ustr>>,
    pub runners: Option<Vec<RunnerDefinition>>,
    pub runners_voidable: Option<bool>,
    pub settled_time: Option<String>,
    pub status: Option<MarketStatus>,
    pub suspend_time: Option<String>,
    pub timezone: Option<Ustr>,
    pub turn_in_play_enabled: Option<bool>,
    pub venue: Option<Ustr>,
    pub version: Option<u64>,
}

/// Runner (selection) definition within a market definition.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunnerDefinition {
    #[serde(deserialize_with = "deserialize_selection_id")]
    pub id: SelectionId,
    pub hc: Option<Handicap>,
    pub sort_priority: Option<u32>,
    pub name: Option<String>,
    pub status: Option<RunnerStatus>,
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub adjustment_factor: Option<Decimal>,
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub bsp: Option<Decimal>,
    pub removal_date: Option<String>,
}

/// Price ladder definition within a market definition.
#[derive(Debug, Clone, Deserialize)]
pub struct PriceLadderDefinition {
    #[serde(rename = "type")]
    pub ladder_type: Option<PriceLadderType>,
}

// Betfair encodes price-volume types as JSON arrays: [price, volume] and
// [level, price, volume] respectively.

/// Price-volume pair, serialized as a JSON array `[price, volume]`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PV {
    pub price: Decimal,
    pub volume: Decimal,
}

impl<'de> Deserialize<'de> for PV {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        // Handles both `[price, volume]` and `[level, price, volume]` (RESUB_DELTA)
        let arr: Vec<Decimal> = Deserialize::deserialize(deserializer)?;
        match arr.len() {
            2 => Ok(Self {
                price: arr[0],
                volume: arr[1],
            }),
            3 => Ok(Self {
                price: arr[1],
                volume: arr[2],
            }),
            n => Err(serde::de::Error::invalid_length(n, &"2 or 3 elements")),
        }
    }
}

impl Serialize for PV {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        (self.price, self.volume).serialize(serializer)
    }
}

/// Level-price-volume triple, serialized as a JSON array `[level, price, volume]`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LPV {
    pub level: u32,
    pub price: Decimal,
    pub volume: Decimal,
}

impl<'de> Deserialize<'de> for LPV {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let arr: (u32, Decimal, Decimal) = Deserialize::deserialize(deserializer)?;
        Ok(Self {
            level: arr.0,
            price: arr.1,
            volume: arr.2,
        })
    }
}

impl Serialize for LPV {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        (self.level, self.price, self.volume).serialize(serializer)
    }
}

/// Order changes for a single market.
#[derive(Debug, Clone, Deserialize)]
pub struct OrderMarketChange {
    /// Market identifier.
    pub id: MarketId,
    #[serde(rename = "accountId")]
    pub account_id: Option<u64>,
    pub closed: Option<bool>,
    #[serde(rename = "fullImage", default)]
    pub full_image: bool,
    /// Order runner changes.
    pub orc: Option<Vec<OrderRunnerChange>>,
}

/// Order changes for a single runner within a market.
#[derive(Debug, Clone, Deserialize)]
pub struct OrderRunnerChange {
    /// Selection identifier.
    #[serde(deserialize_with = "deserialize_selection_id")]
    pub id: SelectionId,
    #[serde(rename = "fullImage", default)]
    pub full_image: bool,
    /// Handicap.
    pub hc: Option<Handicap>,
    /// Matched backs.
    pub mb: Option<Vec<MatchedOrder>>,
    /// Matched lays.
    pub ml: Option<Vec<MatchedOrder>>,
    /// Strategy match changes, keyed by customer strategy ref.
    pub smc: Option<AHashMap<String, StrategyMatchChange>>,
    /// Unmatched orders.
    pub uo: Option<Vec<UnmatchedOrder>>,
}

/// Matched order (price-size pair), serialized as `[price, size]`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MatchedOrder {
    pub price: Decimal,
    pub size: Decimal,
}

impl<'de> Deserialize<'de> for MatchedOrder {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let arr: (Decimal, Decimal) = Deserialize::deserialize(deserializer)?;
        Ok(Self {
            price: arr.0,
            size: arr.1,
        })
    }
}

impl Serialize for MatchedOrder {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        (self.price, self.size).serialize(serializer)
    }
}

/// Strategy-level match changes.
#[derive(Debug, Clone, Deserialize)]
pub struct StrategyMatchChange {
    /// Matched backs.
    pub mb: Option<Vec<MatchedOrder>>,
    /// Matched lays.
    pub ml: Option<Vec<MatchedOrder>>,
}

/// Unmatched order on the streaming API.
#[derive(Debug, Clone, Deserialize)]
pub struct UnmatchedOrder {
    /// Bet identifier.
    pub id: String,
    /// Price.
    #[serde(deserialize_with = "deserialize_decimal")]
    pub p: Decimal,
    /// Size.
    #[serde(deserialize_with = "deserialize_decimal")]
    pub s: Decimal,
    /// Side (B=Back, L=Lay).
    pub side: StreamingSide,
    /// Order status (E=Executable, EC=ExecutionComplete).
    pub status: StreamingOrderStatus,
    /// Persistence type (L=Lapse, P=Persist, MOC=MarketOnClose).
    ///
    /// Betfair can omit this on some BSP market-on-close order updates.
    #[serde(default)]
    pub pt: Option<StreamingPersistenceType>,
    /// Order type (L=Limit, LOC=LimitOnClose, MOC=MarketOnClose).
    pub ot: StreamingOrderType,
    /// Placed date (epoch millis).
    pub pd: u64,
    /// BSP liability.
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub bsp: Option<Decimal>,
    /// Customer strategy reference.
    pub rfo: Option<String>,
    /// Regulator reference.
    pub rfs: Option<String>,
    /// Customer order reference.
    pub rc: Option<String>,
    /// Regulator auth code.
    pub rac: Option<String>,
    /// Matched date (epoch millis).
    pub md: Option<u64>,
    /// Cancelled date (epoch millis).
    pub cd: Option<u64>,
    /// Lapsed date (epoch millis).
    pub ld: Option<u64>,
    /// Average price matched.
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub avp: Option<Decimal>,
    /// Size matched.
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub sm: Option<Decimal>,
    /// Size remaining.
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub sr: Option<Decimal>,
    /// Size lapsed.
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub sl: Option<Decimal>,
    /// Size cancelled.
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub sc: Option<Decimal>,
    /// Size voided.
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub sv: Option<Decimal>,
    /// Lapse status reason code.
    pub lsrc: Option<LapseStatusReasonCode>,
}

/// Authentication request sent on stream connect.
#[derive(Debug, Clone, Serialize)]
pub struct Authentication {
    pub op: String,
    pub id: Option<u64>,
    #[serde(rename = "appKey")]
    pub app_key: String,
    pub session: String,
}

impl Authentication {
    /// Creates a new authentication request.
    #[must_use]
    pub fn new(app_key: String, session: String) -> Self {
        Self {
            op: STREAM_OP_AUTHENTICATION.to_string(),
            id: None,
            app_key,
            session,
        }
    }
}

/// Market subscription request.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MarketSubscription {
    pub op: String,
    pub id: Option<u64>,
    pub market_filter: StreamMarketFilter,
    pub market_data_filter: MarketDataFilter,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub clk: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conflate_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub heartbeat_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub initial_clk: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub segmentation_enabled: Option<bool>,
}

/// Order subscription request.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OrderSubscription {
    pub op: String,
    pub id: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub order_filter: Option<OrderFilter>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub clk: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conflate_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub heartbeat_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub initial_clk: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub segmentation_enabled: Option<bool>,
}

/// Race stream subscription request.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RaceSubscription {
    pub op: String,
    pub id: Option<u64>,
}

impl RaceSubscription {
    #[must_use]
    pub fn new(id: u64) -> Self {
        Self {
            op: STREAM_OP_RACE_SUBSCRIPTION.to_string(),
            id: Some(id),
        }
    }
}

/// Heartbeat request to keep the connection alive.
#[derive(Debug, Clone, Serialize)]
pub struct StreamHeartbeat {
    pub op: String,
    pub id: Option<u64>,
}

impl StreamHeartbeat {
    #[must_use]
    pub fn new() -> Self {
        Self {
            op: STREAM_OP_HEARTBEAT.to_string(),
            id: None,
        }
    }
}

impl Default for StreamHeartbeat {
    fn default() -> Self {
        Self::new()
    }
}

/// Market filter for streaming subscriptions.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamMarketFilter {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub betting_types: Option<Vec<MarketBettingType>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bsp_market: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub country_codes: Option<Vec<Ustr>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event_ids: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event_type_ids: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub market_ids: Option<Vec<MarketId>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub market_types: Option<Vec<Ustr>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub race_types: Option<Vec<Ustr>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub turn_in_play_enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub venues: Option<Vec<Ustr>>,
}

/// Market data filter for streaming subscriptions.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MarketDataFilter {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fields: Option<Vec<MarketDataFilterField>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ladder_levels: Option<u32>,
}

/// Order filter for streaming subscriptions.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrderFilter {
    #[serde(default = "default_true")]
    pub include_overall_position: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub customer_strategy_refs: Option<Vec<String>>,
    #[serde(default)]
    pub partition_matched_by_strategy_ref: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub account_ids: Option<Vec<u64>>,
}

impl Default for OrderFilter {
    fn default() -> Self {
        Self {
            include_overall_position: true,
            customer_strategy_refs: None,
            partition_matched_by_strategy_ref: false,
            account_ids: None,
        }
    }
}

fn default_true() -> bool {
    true
}

/// Race Change Message (RCM) - live GPS tracking data (Total Performance Data).
#[derive(Debug, Clone, Deserialize)]
pub struct RCM {
    pub id: Option<u64>,
    /// Publish time (epoch millis).
    pub pt: u64,
    /// Clock token (may be integer or string depending on feed state).
    pub clk: Option<serde_json::Value>,
    /// Race changes (None on heartbeat).
    pub rc: Option<Vec<RaceChange>>,
}

/// Delta update for a single race within an RCM.
#[derive(Debug, Clone, Deserialize)]
pub struct RaceChange {
    /// Race identifier (e.g. "28587288.1650").
    pub id: Option<String>,
    /// Betfair market identifier.
    pub mid: Option<String>,
    /// Individual runner GPS data changes.
    pub rrc: Option<Vec<RaceRunnerChange>>,
    /// Overall race progress summary.
    pub rpc: Option<RaceProgressChange>,
}

/// GPS tracking data for a single runner.
#[derive(Debug, Clone, Deserialize)]
pub struct RaceRunnerChange {
    /// Feed time (epoch millis).
    pub ft: Option<u64>,
    /// Selection identifier.
    pub id: Option<i64>,
    /// Latitude (GPS coordinate).
    pub lat: Option<f64>,
    /// Longitude (GPS coordinate).
    #[serde(rename = "long")]
    pub lng: Option<f64>,
    /// Speed in m/s (Doppler-derived).
    pub spd: Option<f64>,
    /// Distance to finish in meters.
    pub prg: Option<f64>,
    /// Stride frequency in Hz.
    pub sfq: Option<f64>,
}

/// Race-level progress summary.
#[derive(Debug, Clone, Deserialize)]
pub struct RaceProgressChange {
    /// Feed time (epoch millis).
    pub ft: Option<u64>,
    /// Gate/sectional name (e.g. "1f", "2f", "Finish").
    pub g: Option<String>,
    /// Sectional time in seconds.
    pub st: Option<f64>,
    /// Running time since race start in seconds.
    pub rt: Option<f64>,
    /// Speed of lead horse in m/s.
    pub spd: Option<f64>,
    /// Distance to finish for leading horse in meters.
    pub prg: Option<f64>,
    /// Runner order by selection ID (current race position).
    pub ord: Option<Vec<i64>>,
    /// Obstacle data for jump races.
    #[serde(rename = "J")]
    pub jumps: Option<Vec<Jump>>,
}

/// Jump obstacle location data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Jump {
    /// Jump number.
    #[serde(rename = "J")]
    pub number: i32,
    /// Distance from finish line in meters.
    #[serde(rename = "L")]
    pub distance: f64,
}

/// Decode a single JSON stream line into a [`StreamMessage`].
///
/// # Errors
///
/// Returns an error if the JSON is malformed or the `op` field is missing/unknown.
pub fn stream_decode(data: &[u8]) -> Result<StreamMessage, serde_json::Error> {
    serde_json::from_slice(data)
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;
    use crate::common::testing::load_test_json;

    #[rstest]
    #[case("stream/ocm_NEW_FULL_IMAGE.json")]
    #[case("stream/ocm_FILLED.json")]
    #[case("stream/ocm_FULL_IMAGE.json")]
    #[case("stream/ocm_FULL_IMAGE_STRATEGY.json")]
    #[case("stream/ocm_CANCEL.json")]
    #[case("stream/ocm_UPDATE.json")]
    #[case("stream/ocm_SUB_IMAGE.json")]
    #[case("stream/ocm_MIXED.json")]
    #[case("stream/ocm_EMPTY_IMAGE.json")]
    #[case("stream/ocm_error_fill.json")]
    #[case("stream/ocm_filled_different_price.json")]
    #[case("stream/ocm_order_update.json")]
    fn test_stream_decode_ocm_fixtures(#[case] fixture: &str) {
        let data = load_test_json(fixture);
        let msg = stream_decode(data.as_bytes()).unwrap_or_else(|e| panic!("{fixture}: {e}"));
        assert!(matches!(msg, StreamMessage::OrderChange(_)), "{fixture}");
    }

    #[rstest]
    #[case("stream/mcm_SUB_IMAGE.json")]
    #[case("stream/mcm_SUB_IMAGE_no_market_def.json")]
    #[case("stream/mcm_UPDATE.json")]
    #[case("stream/mcm_UPDATE_md.json")]
    #[case("stream/mcm_UPDATE_tv.json")]
    #[case("stream/mcm_HEARTBEAT.json")]
    #[case("stream/mcm_RESUB_DELTA.json")]
    #[case("stream/mcm_live_IMAGE.json")]
    #[case("stream/mcm_live_UPDATE.json")]
    #[case("stream/mcm_latency.json")]
    #[case("stream/market_definition_racing.json")]
    #[case("stream/market_definition_runner_removed.json")]
    fn test_stream_decode_mcm_fixtures(#[case] fixture: &str) {
        let data = load_test_json(fixture);
        let msg = stream_decode(data.as_bytes()).unwrap_or_else(|e| panic!("{fixture}: {e}"));
        assert!(matches!(msg, StreamMessage::MarketChange(_)), "{fixture}");
    }

    /// Fixtures containing a JSON array of multiple MCM messages.
    #[rstest]
    #[case("stream/mcm_BSP.json")]
    #[case("stream/market_updates.json")]
    fn test_stream_decode_mcm_multi_fixtures(#[case] fixture: &str) {
        let data = load_test_json(fixture);
        let msgs: Vec<StreamMessage> =
            serde_json::from_str(&data).unwrap_or_else(|e| panic!("{fixture}: {e}"));
        assert!(!msgs.is_empty(), "{fixture}: empty array");
        for msg in &msgs {
            assert!(matches!(msg, StreamMessage::MarketChange(_)), "{fixture}");
        }
    }

    /// Fixtures containing a JSON array of multiple OCM messages.
    #[rstest]
    #[case("stream/ocm_multiple_fills.json")]
    #[case("stream/ocm_DUPLICATE_EXECUTION.json")]
    fn test_stream_decode_ocm_multi_fixtures(#[case] fixture: &str) {
        let data = load_test_json(fixture);
        let msgs: Vec<StreamMessage> =
            serde_json::from_str(&data).unwrap_or_else(|e| panic!("{fixture}: {e}"));
        assert!(!msgs.is_empty(), "{fixture}: empty array");
        for msg in &msgs {
            assert!(matches!(msg, StreamMessage::OrderChange(_)), "{fixture}");
        }
    }

    #[rstest]
    fn test_stream_decode_connection() {
        let data = load_test_json("stream/connection.json");
        let msg = stream_decode(data.as_bytes()).unwrap();
        match msg {
            StreamMessage::Connection(conn) => {
                assert_eq!(conn.connection_id, "002-051134157842-432409");
            }
            other => panic!("Expected Connection, was {other:?}"),
        }
    }

    #[rstest]
    fn test_stream_decode_status() {
        let data = load_test_json("stream/status.json");
        let msg = stream_decode(data.as_bytes()).unwrap();
        assert!(matches!(msg, StreamMessage::Status(_)));
    }

    #[rstest]
    fn test_stream_decode_lenient_sp_fields() {
        let data = r#"{
            "op":"mcm",
            "pt":1773304044929,
            "mc":[{
                "id":"1.255095842",
                "rc":[{
                    "id":96146807,
                    "spn":"Infinity",
                    "spf":"NaN",
                    "ltp":5.0,
                    "tv":10.63
                }]
            }]
        }"#;

        let msg = stream_decode(data.as_bytes()).unwrap();

        match msg {
            StreamMessage::MarketChange(mcm) => {
                let rc = &mcm.mc.as_ref().unwrap()[0].rc.as_ref().unwrap()[0];
                assert_eq!(rc.spn, None);
                assert_eq!(rc.spf, None);
                assert_eq!(rc.ltp, Some(Decimal::new(50, 1)));
                assert_eq!(rc.tv, Some(Decimal::new(1063, 2)));
            }
            other => panic!("Expected MarketChange, was {other:?}"),
        }
    }

    #[rstest]
    fn test_market_definition_standalone() {
        let data = load_test_json("stream/market_definition.json");
        let _def: MarketDefinition = serde_json::from_str(&data).unwrap();
    }

    #[rstest]
    #[case("rest/market_definition_open.json")]
    #[case("rest/market_definition_closed.json")]
    #[case("rest/market_definition_runner_removed.json")]
    fn test_market_definition_response_fixtures(#[case] fixture: &str) {
        let data = load_test_json(fixture);
        let _def: MarketDefinition = serde_json::from_str(&data).unwrap();
    }

    #[rstest]
    fn test_stream_decode_rcm_single() {
        let data = load_test_json("stream/rcm_single.json");
        let msg = stream_decode(data.as_bytes()).unwrap();
        match msg {
            StreamMessage::RaceChange(rcm) => {
                let rc = rcm.rc.as_ref().unwrap();
                assert_eq!(rc.len(), 1);

                let race = &rc[0];
                assert_eq!(race.id.as_deref(), Some("28587288.1650"));
                assert_eq!(race.mid.as_deref(), Some("1.1234567"));

                let runners = race.rrc.as_ref().unwrap();
                assert_eq!(runners.len(), 1);
                assert_eq!(runners[0].id, Some(7390417));
                assert!((runners[0].lat.unwrap() - 51.4189543).abs() < 1e-6);
                assert!((runners[0].spd.unwrap() - 17.8).abs() < 1e-6);
                assert!((runners[0].sfq.unwrap() - 2.07).abs() < 1e-6);

                let progress = race.rpc.as_ref().unwrap();
                assert_eq!(progress.g.as_deref(), Some("1f"));
                assert!((progress.st.unwrap() - 10.6).abs() < 1e-6);
                assert!((progress.rt.unwrap() - 46.7).abs() < 1e-6);

                let order = progress.ord.as_ref().unwrap();
                assert_eq!(order.len(), 5);
                assert_eq!(order[0], 7390417);

                let jumps = progress.jumps.as_ref().unwrap();
                assert_eq!(jumps.len(), 2);
                assert_eq!(jumps[0].number, 2);
                assert!((jumps[0].distance - 370.1).abs() < 1e-6);
            }
            other => panic!("Expected RaceChange, was {other:?}"),
        }
    }

    #[rstest]
    fn test_stream_decode_rcm_multi_runner() {
        let data = load_test_json("stream/rcm_multi_runner.json");
        let msg = stream_decode(data.as_bytes()).unwrap();
        match msg {
            StreamMessage::RaceChange(rcm) => {
                let rc = rcm.rc.as_ref().unwrap();
                let runners = rc[0].rrc.as_ref().unwrap();
                assert_eq!(runners.len(), 5);

                let ids: Vec<i64> = runners.iter().filter_map(|r| r.id).collect();
                assert_eq!(ids, vec![35467839, 24947967, 299569, 31422647, 41694785]);
            }
            other => panic!("Expected RaceChange, was {other:?}"),
        }
    }

    #[rstest]
    fn test_stream_decode_ocm_voided() {
        let data = load_test_json("stream/ocm_VOIDED.json");
        let msg = stream_decode(data.as_bytes()).unwrap();
        match msg {
            StreamMessage::OrderChange(ocm) => {
                let oc = ocm.oc.as_ref().unwrap();
                let orc = oc[0].orc.as_ref().unwrap();
                let uo = &orc[0].uo.as_ref().unwrap()[0];
                assert_eq!(uo.sv.unwrap(), rust_decimal::Decimal::from(50));
                assert_eq!(uo.sm.unwrap(), rust_decimal::Decimal::from(50));
                assert_eq!(uo.s, rust_decimal::Decimal::from(100));
            }
            other => panic!("Expected OrderChange, was {other:?}"),
        }
    }

    #[rstest]
    fn test_stream_decode_ocm_missing_persistence_type_for_market_on_close() {
        let data = r#"{
            "op":"ocm",
            "id":1,
            "pt":1775175455685,
            "clk":"clk-1",
            "oc":[{
                "id":"1.256134154",
                "orc":[{
                    "id":77465280,
                    "uo":[{
                        "id":"424009603606",
                        "p":1.01,
                        "s":2.00,
                        "side":"B",
                        "status":"E",
                        "ot":"MOC",
                        "pd":1775175455000,
                        "sr":2.00
                    }]
                }]
            }]
        }"#;

        let msg = stream_decode(data.as_bytes()).unwrap();

        match msg {
            StreamMessage::OrderChange(ocm) => {
                let oc = ocm.oc.as_ref().unwrap();
                let orc = oc[0].orc.as_ref().unwrap();
                let uo = &orc[0].uo.as_ref().unwrap()[0];
                assert_eq!(uo.pt, None);
                assert_eq!(
                    uo.ot,
                    crate::common::enums::StreamingOrderType::MarketOnClose
                );
            }
            other => panic!("Expected OrderChange, was {other:?}"),
        }
    }
}
