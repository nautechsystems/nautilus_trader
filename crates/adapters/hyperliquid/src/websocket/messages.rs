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

use ahash::AHashMap;
pub use hypersdk::hypercore::{Bbo, BookLevel, Candle, L2Book, Trade};
use hypersdk::hypercore::{Subscription as SdkSubscription, api::Action};
use nautilus_core::serialization::{
    deserialize_decimal, deserialize_decimal_from_str, deserialize_optional_decimal_from_str,
};
use nautilus_model::{
    data::{
        Bar, Data, FundingRateUpdate, IndexPriceUpdate, MarkPriceUpdate, OrderBookDeltas,
        OrderBookDepth, QuoteTick, TradeTick,
    },
    identifiers::InstrumentId,
    reports::{FillReport, OrderStatusReport},
};
use rust_decimal::Decimal;
use serde::{Deserialize, Deserializer, Serialize};
use ustr::Ustr;

use crate::{
    common::enums::{
        HyperliquidFillDirection, HyperliquidLiquidationMethod,
        HyperliquidOrderStatus as HyperliquidOrderStatusEnum, HyperliquidSide,
        HyperliquidTimeInForce, HyperliquidTpSl, HyperliquidTwapStatus,
    },
    http::models::HyperliquidExchangeRequest,
};

/// Represents an outbound WebSocket message from client to Hyperliquid.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "method")]
#[serde(rename_all = "lowercase")]
pub enum HyperliquidWsRequest {
    /// Subscribe to a data feed.
    Subscribe {
        /// Subscription details.
        subscription: SubscriptionRequest,
    },
    /// Unsubscribe from a data feed.
    Unsubscribe {
        /// Subscription details to remove.
        subscription: SubscriptionRequest,
    },
    /// Post a request (info or action).
    Post {
        /// Request ID for tracking.
        id: u64,
        /// Request payload.
        request: PostRequest,
    },
    /// Ping for keepalive.
    Ping,
}

/// A venue subscription, including feeds not yet represented by the SDK.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum SubscriptionRequest {
    Extension(SubscriptionExtension),
    Sdk(SdkSubscription),
}

/// Subscription fields missing from hypersdk.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum SubscriptionExtension {
    WebData2 {
        user: String,
    },
    ActiveSpotAssetCtx {
        coin: String,
    },
    UserFills {
        user: String,
        #[serde(rename = "aggregateByTime")]
        aggregate_by_time: bool,
    },
}

/// Post request wrapper for info and action requests.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
#[serde(rename_all = "lowercase")]
pub enum PostRequest {
    /// Info request (no signature required).
    Info { payload: serde_json::Value },
    /// Action request (requires signature).
    Action {
        payload: Box<HyperliquidExchangeRequest<Action>>,
    },
}

/// Subscription response data wrapper.
#[derive(Debug, Clone, Deserialize)]
pub struct SubscriptionResponseData {
    pub method: String,
    pub subscription: SubscriptionRequest,
}

/// Inbound WebSocket message from Hyperliquid server.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "channel")]
#[serde(rename_all = "camelCase")]
pub enum HyperliquidWsMessage {
    /// Subscription confirmation.
    SubscriptionResponse { data: SubscriptionResponseData },
    /// Post request response.
    Post { data: PostResponse },
    /// All mid prices.
    AllMids { data: AllMidsData },
    /// Aggregate asset contexts across all perp dexes.
    AllDexsAssetCtxs { data: WsAllDexsAssetCtxsData },
    /// Notifications.
    Notification { data: NotificationData },
    /// Web data.
    WebData2 { data: serde_json::Value },
    /// Candlestick data.
    Candle {
        #[serde(deserialize_with = "deserialize_candle")]
        data: Candle,
    },
    /// Level 2 order book.
    L2Book {
        #[serde(deserialize_with = "deserialize_book")]
        data: L2Book,
    },
    /// Trade updates.
    Trades {
        #[serde(deserialize_with = "deserialize_trades")]
        data: Vec<Trade>,
    },
    /// Order updates.
    OrderUpdates { data: Vec<WsOrderData> },
    /// User events.
    UserEvents { data: WsUserEventData },
    /// Generic user channel (Hyperliquid sends fills/events on this channel).
    #[serde(rename = "user")]
    User { data: WsUserEventData },
    /// User fills.
    UserFills { data: WsUserFillsData },
    /// User funding payments.
    UserFundings { data: WsUserFundingsData },
    /// User ledger updates.
    UserNonFundingLedgerUpdates { data: serde_json::Value },
    /// Active asset context.
    ActiveAssetCtx { data: WsActiveAssetCtxData },
    /// Active spot asset context (same data as ActiveAssetCtx, different channel name).
    ActiveSpotAssetCtx { data: WsActiveAssetCtxData },
    /// Active asset data.
    ActiveAssetData { data: WsActiveAssetData },
    /// TWAP slice fills.
    UserTwapSliceFills { data: WsUserTwapSliceFillsData },
    /// TWAP history.
    UserTwapHistory { data: WsUserTwapHistoryData },
    /// Best bid/offer.
    Bbo {
        #[serde(deserialize_with = "deserialize_bbo")]
        data: Bbo,
    },
    /// Error response.
    Error { data: String },
    /// Pong response.
    Pong,
}

/// Post response data.
#[derive(Debug, Clone, Deserialize)]
pub struct PostResponse {
    pub id: u64,
    pub response: PostResponsePayload,
}

/// Post response payload.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
#[serde(rename_all = "lowercase")]
pub enum PostResponsePayload {
    Info { payload: serde_json::Value },
    Action { payload: serde_json::Value },
    Error { payload: String },
}

/// All mid prices data.
#[derive(Debug, Clone, Deserialize)]
pub struct AllMidsData {
    pub mids: AHashMap<Ustr, String>,
}

/// `allDexsAssetCtxs` data payload.
#[derive(Debug, Clone, Deserialize)]
pub struct WsAllDexsAssetCtxsData {
    pub ctxs: Vec<(String, Vec<PerpsAssetCtx>)>,
}

/// Notification data.
#[derive(Debug, Clone, Deserialize)]
pub struct NotificationData {
    pub notification: String,
}

/// Decode venue strings directly into the SDK candle without passing through floats.
fn deserialize_candle<'de, D>(deserializer: D) -> Result<Candle, D::Error>
where
    D: Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    require_decimal_strings::<D::Error>(&value, &["o", "h", "l", "c", "v"])?;
    let candle: Candle = serde_json::from_value(value).map_err(serde::de::Error::custom)?;
    u32::try_from(candle.num_trades).map_err(serde::de::Error::custom)?;
    Ok(candle)
}

fn require_decimal_strings<E: serde::de::Error>(
    value: &serde_json::Value,
    fields: &[&str],
) -> Result<(), E> {
    for field in fields {
        if !value.get(field).is_some_and(serde_json::Value::is_string) {
            return Err(E::custom(format!("{field} must be a decimal string")));
        }
    }
    Ok(())
}

fn validate_book_level<E: serde::de::Error>(value: &serde_json::Value) -> Result<(), E> {
    require_decimal_strings::<E>(value, &["px", "sz"])?;
    let count = value
        .get("n")
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| E::custom("n must be an unsigned integer"))?;
    u32::try_from(count).map_err(E::custom)?;
    Ok(())
}

fn deserialize_book<'de, D>(deserializer: D) -> Result<L2Book, D::Error>
where
    D: Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    if let Some(sides) = value.get("levels").and_then(serde_json::Value::as_array) {
        for levels in sides {
            if let Some(levels) = levels.as_array() {
                for level in levels {
                    validate_book_level::<D::Error>(level)?;
                }
            }
        }
    }
    serde_json::from_value(value).map_err(serde::de::Error::custom)
}

fn deserialize_bbo<'de, D>(deserializer: D) -> Result<Bbo, D::Error>
where
    D: Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    if let Some(levels) = value.get("bbo").and_then(serde_json::Value::as_array) {
        for level in levels.iter().filter(|level| !level.is_null()) {
            validate_book_level::<D::Error>(level)?;
        }
    }
    serde_json::from_value(value).map_err(serde::de::Error::custom)
}

pub(crate) fn deserialize_trades<'de, D>(deserializer: D) -> Result<Vec<Trade>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    if let Some(trades) = value.as_array() {
        for trade in trades {
            require_decimal_strings::<D::Error>(trade, &["px", "sz"])?;
            if !trade
                .get("users")
                .and_then(serde_json::Value::as_array)
                .is_some_and(|users| users.len() == 2)
            {
                return Err(serde::de::Error::custom(
                    "users must contain the buyer and seller addresses",
                ));
            }
        }
    }
    serde_json::from_value(value).map_err(serde::de::Error::custom)
}

/// WebSocket order data.
#[derive(Debug, Clone, Deserialize)]
pub struct WsOrderData {
    pub order: WsBasicOrderData,
    pub status: HyperliquidOrderStatusEnum,
    #[serde(rename = "statusTimestamp")]
    pub status_timestamp: u64,
}

/// Basic order data.
#[derive(Debug, Clone, Deserialize)]
pub struct WsBasicOrderData {
    pub coin: Ustr,
    pub side: HyperliquidSide,
    #[serde(rename = "limitPx", deserialize_with = "deserialize_decimal_from_str")]
    pub limit_px: Decimal,
    #[serde(deserialize_with = "deserialize_decimal_from_str")]
    pub sz: Decimal,
    pub oid: u64,
    pub timestamp: u64,
    #[serde(rename = "origSz", deserialize_with = "deserialize_decimal_from_str")]
    pub orig_sz: Decimal,
    pub cloid: Option<String>,
    pub tif: Option<HyperliquidTimeInForce>,
    #[serde(rename = "reduceOnly")]
    pub reduce_only: Option<bool>,
    /// Trigger price for conditional orders (stop/take-profit).
    #[serde(
        rename = "triggerPx",
        default,
        deserialize_with = "deserialize_optional_decimal_from_str"
    )]
    pub trigger_px: Option<Decimal>,
    /// Whether this is a market or limit trigger order.
    #[serde(rename = "isMarket")]
    pub is_market: Option<bool>,
    /// Take-profit or stop-loss indicator.
    pub tpsl: Option<HyperliquidTpSl>,
    /// Whether the trigger has been activated.
    #[serde(rename = "triggerActivated")]
    pub trigger_activated: Option<bool>,
    /// Trailing stop parameters if applicable.
    #[serde(rename = "trailingStop")]
    pub trailing_stop: Option<WsTrailingStopData>,
    /// Venue order type label (for example `"Stop Market"`), present on REST order rows
    /// such as `frontendOpenOrders`, which omit `tpsl` and `isMarket`.
    #[serde(rename = "orderType", default)]
    pub order_type: Option<String>,
}

/// Trailing stop offset type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum TrailingOffsetType {
    /// Price offset.
    Price,
    /// Percentage offset.
    Percentage,
    /// Basis points offset.
    BasisPoints,
}

impl TrailingOffsetType {
    /// Format the offset value with the appropriate unit.
    pub fn format_offset(&self, offset: &str) -> String {
        match self {
            Self::Price => offset.to_string(),
            Self::Percentage => format!("{offset}%"),
            Self::BasisPoints => format!("{offset} bps"),
        }
    }
}

/// Trailing stop data from WebSocket.
#[derive(Debug, Clone, Deserialize)]
pub struct WsTrailingStopData {
    /// Trailing offset value.
    #[serde(deserialize_with = "deserialize_decimal_from_str")]
    pub offset: Decimal,
    /// Offset type.
    #[serde(rename = "offsetType")]
    pub offset_type: TrailingOffsetType,
    /// Current callback price (highest/lowest price reached).
    #[serde(
        rename = "callbackPrice",
        default,
        deserialize_with = "deserialize_optional_decimal_from_str"
    )]
    pub callback_price: Option<Decimal>,
}

/// WebSocket user event data.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum WsUserEventData {
    Fills {
        fills: Vec<WsFillData>,
    },
    Funding {
        funding: WsUserFundingData,
    },
    Liquidation {
        liquidation: WsLiquidationData,
    },
    NonUserCancel {
        #[serde(rename = "nonUserCancel")]
        non_user_cancel: Vec<WsNonUserCancelData>,
    },
    /// Trigger order activated (moved from pending to active).
    TriggerActivated {
        #[serde(rename = "triggerActivated")]
        trigger_activated: WsTriggerActivatedData,
    },
    /// Trigger order executed (trigger price reached, order placed).
    TriggerTriggered {
        #[serde(rename = "triggerTriggered")]
        trigger_triggered: WsTriggerTriggeredData,
    },
}

/// WebSocket fill data.
#[derive(Debug, Clone, Deserialize)]
pub struct WsFillData {
    pub coin: Ustr,
    #[serde(deserialize_with = "deserialize_decimal_from_str")]
    pub px: Decimal,
    #[serde(deserialize_with = "deserialize_decimal_from_str")]
    pub sz: Decimal,
    pub side: HyperliquidSide,
    pub time: u64,
    #[serde(
        rename = "startPosition",
        deserialize_with = "deserialize_decimal_from_str"
    )]
    pub start_position: Decimal,
    pub dir: HyperliquidFillDirection,
    #[serde(
        rename = "closedPnl",
        deserialize_with = "deserialize_decimal_from_str"
    )]
    pub closed_pnl: Decimal,
    pub hash: String,
    pub oid: u64,
    pub crossed: bool,
    #[serde(deserialize_with = "deserialize_decimal_from_str")]
    pub fee: Decimal,
    pub tid: u64,
    #[serde(default)]
    pub liquidation: Option<FillLiquidationData>,
    #[serde(rename = "feeToken")]
    pub fee_token: Ustr,
    #[serde(
        rename = "builderFee",
        default,
        deserialize_with = "deserialize_optional_decimal_from_str"
    )]
    pub builder_fee: Option<Decimal>,
    /// Client order ID (hex string with 0x prefix).
    pub cloid: Option<String>,
    /// TWAP order ID if this fill is part of a TWAP order.
    #[serde(rename = "twapId")]
    pub twap_id: Option<serde_json::Value>,
}

/// Fill liquidation data.
#[derive(Debug, Clone, Deserialize)]
pub struct FillLiquidationData {
    #[serde(rename = "liquidatedUser")]
    pub liquidated_user: Option<String>,
    #[serde(rename = "markPx", deserialize_with = "deserialize_decimal_from_str")]
    pub mark_px: Decimal,
    pub method: HyperliquidLiquidationMethod,
}

/// WebSocket user funding data.
#[derive(Debug, Clone, Deserialize)]
pub struct WsUserFundingData {
    pub time: u64,
    pub coin: Ustr,
    #[serde(deserialize_with = "deserialize_decimal_from_str")]
    pub usdc: Decimal,
    #[serde(deserialize_with = "deserialize_decimal_from_str")]
    pub szi: Decimal,
    #[serde(
        rename = "fundingRate",
        deserialize_with = "deserialize_decimal_from_str"
    )]
    pub funding_rate: Decimal,
}

/// WebSocket liquidation data.
#[derive(Debug, Clone, Deserialize)]
pub struct WsLiquidationData {
    pub lid: u64,
    pub liquidator: String,
    pub liquidated_user: String,
    #[serde(deserialize_with = "deserialize_decimal_from_str")]
    pub liquidated_ntl_pos: Decimal,
    #[serde(deserialize_with = "deserialize_decimal_from_str")]
    pub liquidated_account_value: Decimal,
}

/// WebSocket non-user cancel data.
#[derive(Debug, Clone, Deserialize)]
pub struct WsNonUserCancelData {
    pub coin: Ustr,
    pub oid: u64,
}

/// Trigger order activated event data.
#[derive(Debug, Clone, Deserialize)]
pub struct WsTriggerActivatedData {
    pub coin: Ustr,
    pub oid: u64,
    pub time: u64,
    #[serde(
        rename = "triggerPx",
        deserialize_with = "deserialize_decimal_from_str"
    )]
    pub trigger_px: Decimal,
    pub tpsl: HyperliquidTpSl,
}

/// Trigger order triggered event data.
#[derive(Debug, Clone, Deserialize)]
pub struct WsTriggerTriggeredData {
    pub coin: Ustr,
    pub oid: u64,
    pub time: u64,
    #[serde(
        rename = "triggerPx",
        deserialize_with = "deserialize_decimal_from_str"
    )]
    pub trigger_px: Decimal,
    #[serde(rename = "marketPx", deserialize_with = "deserialize_decimal_from_str")]
    pub market_px: Decimal,
    pub tpsl: HyperliquidTpSl,
    /// Order ID of the resulting market/limit order after trigger.
    #[serde(rename = "resultingOid")]
    pub resulting_oid: Option<u64>,
}

/// WebSocket user fills data.
#[derive(Debug, Clone, Deserialize)]
pub struct WsUserFillsData {
    #[serde(rename = "isSnapshot")]
    pub is_snapshot: Option<bool>,
    pub user: String,
    pub fills: Vec<WsFillData>,
}

/// WebSocket user fundings data.
#[derive(Debug, Clone, Deserialize)]
pub struct WsUserFundingsData {
    #[serde(rename = "isSnapshot")]
    pub is_snapshot: Option<bool>,
    pub user: String,
    pub fundings: Vec<WsUserFundingData>,
}

/// WebSocket active asset context data.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum WsActiveAssetCtxData {
    Perp { coin: Ustr, ctx: PerpsAssetCtx },
    Spot { coin: Ustr, ctx: SpotAssetCtx },
}

/// Shared asset context fields.
#[derive(Debug, Clone, Deserialize)]
pub struct SharedAssetCtx {
    #[serde(
        rename = "dayNtlVlm",
        deserialize_with = "deserialize_decimal_from_str"
    )]
    pub day_ntl_vlm: Decimal,
    #[serde(
        rename = "prevDayPx",
        deserialize_with = "deserialize_decimal_from_str"
    )]
    pub prev_day_px: Decimal,
    #[serde(rename = "markPx", deserialize_with = "deserialize_decimal_from_str")]
    pub mark_px: Decimal,
    #[serde(
        rename = "midPx",
        default,
        deserialize_with = "deserialize_optional_decimal_from_str"
    )]
    pub mid_px: Option<Decimal>,
    #[serde(rename = "impactPxs")]
    pub impact_pxs: Option<Vec<String>>,
    #[serde(
        rename = "dayBaseVlm",
        default,
        deserialize_with = "deserialize_optional_decimal_from_str"
    )]
    pub day_base_vlm: Option<Decimal>,
}

/// Perps asset context.
#[derive(Debug, Clone, Deserialize)]
pub struct PerpsAssetCtx {
    #[serde(flatten)]
    pub shared: SharedAssetCtx,
    #[serde(deserialize_with = "deserialize_decimal_from_str")]
    pub funding: Decimal,
    #[serde(
        rename = "openInterest",
        deserialize_with = "deserialize_decimal_from_str"
    )]
    pub open_interest: Decimal,
    #[serde(rename = "oraclePx", deserialize_with = "deserialize_decimal_from_str")]
    pub oracle_px: Decimal,
    #[serde(default, deserialize_with = "deserialize_optional_decimal_from_str")]
    pub premium: Option<Decimal>,
}

/// Spot asset context.
#[derive(Debug, Clone, Deserialize)]
pub struct SpotAssetCtx {
    #[serde(flatten)]
    pub shared: SharedAssetCtx,
    #[serde(
        rename = "circulatingSupply",
        deserialize_with = "deserialize_decimal_from_str"
    )]
    pub circulating_supply: Decimal,
}

/// WebSocket active asset data.
#[derive(Debug, Clone, Deserialize)]
pub struct WsActiveAssetData {
    pub user: String,
    pub coin: Ustr,
    pub leverage: LeverageData,
    #[serde(rename = "maxTradeSzs")]
    pub max_trade_szs: [f64; 2],
    #[serde(rename = "availableToTrade")]
    pub available_to_trade: [f64; 2],
}

/// Leverage data.
#[derive(Debug, Clone, Deserialize)]
pub struct LeverageData {
    pub value: f64,
    pub type_: String,
}

/// WebSocket TWAP slice fills data.
#[derive(Debug, Clone, Deserialize)]
pub struct WsUserTwapSliceFillsData {
    #[serde(rename = "isSnapshot")]
    pub is_snapshot: Option<bool>,
    pub user: String,
    #[serde(rename = "twapSliceFills")]
    pub twap_slice_fills: Vec<WsTwapSliceFillData>,
}

/// TWAP slice fill data.
#[derive(Debug, Clone, Deserialize)]
pub struct WsTwapSliceFillData {
    pub fill: WsFillData,
    #[serde(rename = "twapId")]
    pub twap_id: u64,
}

/// WebSocket TWAP history data.
#[derive(Debug, Clone, Deserialize)]
pub struct WsUserTwapHistoryData {
    #[serde(rename = "isSnapshot")]
    pub is_snapshot: Option<bool>,
    pub user: String,
    pub history: Vec<WsTwapHistoryData>,
}

/// TWAP history data.
#[derive(Debug, Clone, Deserialize)]
pub struct WsTwapHistoryData {
    pub state: TwapStateData,
    pub status: TwapStatusData,
    pub time: u64,
    #[serde(default, rename = "twapId")]
    pub twap_id: Option<u64>,
}

/// TWAP state data.
#[derive(Debug, Clone, Deserialize)]
pub struct TwapStateData {
    pub coin: Ustr,
    pub user: String,
    pub side: HyperliquidSide,
    /// Venue may send a JSON string or number.
    #[serde(deserialize_with = "deserialize_decimal")]
    pub sz: Decimal,
    #[serde(rename = "executedSz", deserialize_with = "deserialize_decimal")]
    pub executed_sz: Decimal,
    #[serde(rename = "executedNtl", deserialize_with = "deserialize_decimal")]
    pub executed_ntl: Decimal,
    pub minutes: u32,
    #[serde(rename = "reduceOnly")]
    pub reduce_only: bool,
    pub randomize: bool,
    pub timestamp: u64,
}

/// TWAP status data.
#[derive(Debug, Clone, Deserialize)]
pub struct TwapStatusData {
    pub status: HyperliquidTwapStatus,
    /// Present when `status` is `error`; otherwise often omitted.
    #[serde(default)]
    pub description: String,
}

#[cfg(test)]
mod tests {
    use hypersdk::hypercore::Side;
    use rstest::rstest;
    use rust_decimal_macros::dec;
    use serde_json;

    use super::*;
    use crate::common::enums::HyperliquidBarInterval;

    #[rstest]
    fn test_subscription_request_serialization() {
        let sub = SubscriptionRequest::Sdk(SdkSubscription::L2Book {
            coin: "BTC".to_string(),
            n_sig_figs: Some(5),
            mantissa: None,

            fast: false,
        });

        let json = serde_json::to_string(&sub).unwrap();
        assert!(json.contains(r#""type":"l2Book""#));
        assert!(json.contains(r#""coin":"BTC""#));
    }

    #[rstest]
    fn test_hyperliquid_ws_request_serialization() {
        let req = HyperliquidWsRequest::Subscribe {
            subscription: SubscriptionRequest::Sdk(SdkSubscription::Trades {
                coin: "ETH".to_string(),
            }),
        };

        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains(r#""method":"subscribe""#));
        assert!(json.contains(r#""type":"trades""#));
    }

    #[rstest]
    #[case::all_mids(SubscriptionRequest::Sdk(SdkSubscription::AllMids { dex: None }), serde_json::json!({"type": "allMids"}))]
    #[case::dex_mids(SubscriptionRequest::Sdk(SdkSubscription::AllMids { dex: Some("testdex".to_owned()) }), serde_json::json!({"type": "allMids", "dex": "testdex"}))]
    #[case::book(SubscriptionRequest::Sdk(SdkSubscription::L2Book { coin: "testdex:BTC".to_string(), n_sig_figs: Some(5), mantissa: Some(2),
fast: false,
}), serde_json::json!({"type": "l2Book", "coin": "testdex:BTC", "nSigFigs": 5, "mantissa": 2}))]
    #[case::full_book(SubscriptionRequest::Sdk(SdkSubscription::L2Book { coin: "BTC".to_string(), n_sig_figs: None, mantissa: None,
fast: false,
}), serde_json::json!({"type": "l2Book", "coin": "BTC"}))]
    #[case::candle(SubscriptionRequest::Sdk(SdkSubscription::Candle { coin: "BTC".to_string(), interval: HyperliquidBarInterval::OneMonth.as_str().to_string() }), serde_json::json!({"type": "candle", "coin": "BTC", "interval": "1M"}))]
    #[case::trades(SubscriptionRequest::Sdk(SdkSubscription::Trades { coin: "#123".to_string() }), serde_json::json!({"type": "trades", "coin": "#123"}))]
    #[case::asset_context(SubscriptionRequest::Sdk(SdkSubscription::ActiveAssetCtx { coin: "BTC".to_string() }), serde_json::json!({"type": "activeAssetCtx", "coin": "BTC"}))]
    #[case::bbo(SubscriptionRequest::Sdk(SdkSubscription::Bbo { coin: "BTC".to_string() }), serde_json::json!({"type": "bbo", "coin": "BTC"}))]
    #[case::aggregate_user_fills(SubscriptionRequest::Extension(SubscriptionExtension::UserFills { user: "0xuser".to_owned(), aggregate_by_time: true }), serde_json::json!({"type": "userFills", "user": "0xuser", "aggregateByTime": true}))]
    fn test_sdk_subscription_preserves_wire_options(
        #[case] subscription: SubscriptionRequest,
        #[case] expected: serde_json::Value,
        #[values(false, true)] unsubscribe: bool,
    ) {
        let request = if unsubscribe {
            HyperliquidWsRequest::Unsubscribe { subscription }
        } else {
            HyperliquidWsRequest::Subscribe { subscription }
        };
        let wire = serde_json::to_value(request).unwrap();
        assert_eq!(wire["subscription"], expected);
        assert_eq!(
            wire["method"],
            if unsubscribe {
                "unsubscribe"
            } else {
                "subscribe"
            }
        );
    }

    #[rstest]
    fn test_sdk_subscription_rejects_overflowing_book_options() {
        let subscription = serde_json::json!({"type": "l2Book", "coin": "BTC", "nSigFigs": 256});
        assert!(serde_json::from_value::<SubscriptionRequest>(subscription).is_err());
    }

    #[rstest]
    #[case(false)]
    #[case(true)]
    fn test_aggregate_user_fills_subscription_response(#[case] aggregate_by_time: bool) {
        let subscription = serde_json::json!({
            "type": "userFills", "user": "0x1111111111111111111111111111111111111111",
            "aggregateByTime": aggregate_by_time,
        });
        let response: SubscriptionResponseData = serde_json::from_value(serde_json::json!({
            "method": "subscribe", "subscription": subscription,
        }))
        .unwrap();
        assert_eq!(
            serde_json::to_value(response.subscription).unwrap(),
            subscription
        );
    }

    #[rstest]
    fn test_order_request_serialization() {
        use hypersdk::hypercore::{Cloid as B128, OrderRequest, OrderTypePlacement, TimeInForce};
        let order = OrderRequest {
            asset: 0,
            is_buy: true,
            limit_px: dec!(50000),
            sz: dec!(0.1),
            reduce_only: false,
            order_type: OrderTypePlacement::Limit {
                tif: TimeInForce::Gtc,
            },
            cloid: B128::from([1; 16]),
        };
        let wire = serde_json::to_value(order).unwrap();
        assert_eq!(wire["a"], 0);
        assert_eq!(wire["b"], true);
        assert_eq!(wire["p"], "50000");
        assert_eq!(wire["s"], "0.1");
        assert_eq!(wire["t"], serde_json::json!({"limit": {"tif": "Gtc"}}));
    }

    #[rstest]
    #[case("px")]
    #[case("sz")]
    fn test_sdk_trade_rejects_numeric_decimals(#[case] field: &str) {
        let mut trade = serde_json::json!({
            "coin": "BTC", "side": "B", "px": "1", "sz": "2", "hash": "0xhash",
            "time": 1, "tid": 1,
            "users": ["0x1111111111111111111111111111111111111111", "0x2222222222222222222222222222222222222222"],
        });
        trade[field] = serde_json::json!(0.12345678901234567);
        assert!(
            serde_json::from_value::<HyperliquidWsMessage>(serde_json::json!({
                "channel": "trades", "data": [trade],
            }))
            .is_err()
        );
    }

    #[rstest]
    fn test_ws_trade_data_deserialization() {
        let json = r#"{
            "coin": "BTC",
            "side": "B",
            "px": "50000.0",
            "sz": "0.1",
            "hash": "0x123",
            "time": 1234567890,
            "tid": 12345,
            "users": ["0x1111111111111111111111111111111111111111", "0x2222222222222222222222222222222222222222"]
        }"#;

        let trade: Trade = serde_json::from_str(json).unwrap();
        assert_eq!(trade.coin, "BTC");
        assert_eq!(trade.side, Side::Bid);
        assert_eq!(trade.px, dec!(50000.0));
    }

    #[rstest]
    fn test_ws_book_data_deserialization() {
        let json = r#"{
            "coin": "ETH",
            "levels": [
                [{"px": "3000.0", "sz": "1.0", "n": 1}],
                [{"px": "3001.0", "sz": "2.0", "n": 2}]
            ],
            "time": 1234567890
        }"#;

        let book: L2Book = serde_json::from_str(json).unwrap();
        assert_eq!(book.coin, "ETH");
        assert_eq!(book.levels[0].len(), 1);
        assert_eq!(book.levels[1].len(), 1);
    }

    fn candle_payload() -> serde_json::Value {
        serde_json::json!({
            "t": 1_700_000_000_000_u64,
            "T": 1_700_000_059_999_u64,
            "s": "BTC",
            "i": "1m",
            "o": "0.1234567890123456789012345678",
            "c": "0.1234567890123456789012345679",
            "h": "0.1234567890123456789012345680",
            "l": "0.1234567890123456789012345677",
            "v": "1.0000000000000000000000000001",
            "n": 42
        })
    }

    #[rstest]
    fn test_sdk_candle_preserves_exact_decimals_and_metadata() {
        let candle: Candle = serde_json::from_value(candle_payload()).unwrap();
        assert_eq!(candle.open_time, 1_700_000_000_000);
        assert_eq!(candle.close_time, 1_700_000_059_999);
        assert_eq!(candle.coin, "BTC");
        assert_eq!(candle.interval, "1m");
        assert_eq!(candle.num_trades, 42);
        assert_eq!(candle.open, dec!(0.1234567890123456789012345678));
        assert_eq!(candle.close, dec!(0.1234567890123456789012345679));
        assert_eq!(candle.high, dec!(0.1234567890123456789012345680));
        assert_eq!(candle.low, dec!(0.1234567890123456789012345677));
        assert_eq!(candle.volume, dec!(1.0000000000000000000000000001));
    }

    #[rstest]
    fn test_sdk_book_level_preserves_exact_decimals_and_counts() {
        let level: BookLevel = serde_json::from_value(serde_json::json!({
            "px": "0.1234567890123456789012345678",
            "sz": "1.0000000000000000000000000001",
            "n": u32::MAX,
        }))
        .unwrap();
        assert_eq!(level.px, dec!(0.1234567890123456789012345678));
        assert_eq!(level.sz, dec!(1.0000000000000000000000000001));
        assert_eq!(level.n, u32::MAX as usize);
    }

    #[rstest]
    fn test_sdk_market_data_rejects_counts_above_adapter_bounds() {
        let overflow = u64::from(u32::MAX) + 1;
        let mut payload = candle_payload();
        payload["n"] = serde_json::json!(overflow);
        assert!(deserialize_candle(payload).is_err());
        assert!(
            validate_book_level::<serde_json::Error>(&serde_json::json!({
                "px": "1", "sz": "2", "n": overflow
            }))
            .is_err()
        );
    }

    #[rstest]
    #[case("o")]
    #[case("c")]
    #[case("h")]
    #[case("l")]
    #[case("v")]
    fn test_sdk_candle_rejects_numeric_decimals(#[case] field: &str) {
        let mut payload = candle_payload();
        payload[field] = serde_json::json!(0.1);
        assert!(deserialize_candle(payload).is_err());
    }

    #[rstest]
    #[case("px")]
    #[case("sz")]
    fn test_sdk_book_level_rejects_numeric_decimals(#[case] field: &str) {
        let mut payload = serde_json::json!({"px": "1", "sz": "2", "n": 1});
        payload[field] = serde_json::json!(0.1);
        assert!(validate_book_level::<serde_json::Error>(&payload).is_err());
    }

    #[rstest]
    fn test_ws_trailing_stop_data_deserialization() {
        let json = r#"{
            "offset": "100.0",
            "offsetType": "price",
            "callbackPrice": "50000.0"
        }"#;

        let data: WsTrailingStopData = serde_json::from_str(json).unwrap();
        assert_eq!(data.offset, dec!(100.0));
        assert_eq!(data.offset_type, TrailingOffsetType::Price);
        assert_eq!(data.callback_price.unwrap(), dec!(50000.0));
    }

    #[rstest]
    fn test_ws_trigger_activated_data_deserialization() {
        let json = r#"{
            "coin": "BTC",
            "oid": 12345,
            "time": 1704470400000,
            "triggerPx": "50000.0",
            "tpsl": "sl"
        }"#;

        let data: WsTriggerActivatedData = serde_json::from_str(json).unwrap();
        assert_eq!(data.coin, Ustr::from("BTC"));
        assert_eq!(data.oid, 12345);
        assert_eq!(data.trigger_px, dec!(50000.0));
        assert_eq!(data.tpsl, HyperliquidTpSl::Sl);
        assert_eq!(data.time, 1704470400000);
    }

    #[rstest]
    fn test_ws_trigger_triggered_data_deserialization() {
        let json = r#"{
            "coin": "ETH",
            "oid": 67890,
            "time": 1704470500000,
            "triggerPx": "3000.0",
            "marketPx": "3001.0",
            "tpsl": "tp",
            "resultingOid": 99999
        }"#;

        let data: WsTriggerTriggeredData = serde_json::from_str(json).unwrap();
        assert_eq!(data.coin, Ustr::from("ETH"));
        assert_eq!(data.oid, 67890);
        assert_eq!(data.trigger_px, dec!(3000.0));
        assert_eq!(data.market_px, dec!(3001.0));
        assert_eq!(data.tpsl, HyperliquidTpSl::Tp);
        assert_eq!(data.resulting_oid, Some(99999));
    }

    #[rstest]
    fn test_ws_fill_data_deserialization_with_cloid_and_twap() {
        let json = r#"{
            "coin": "@107",
            "px": "31.737",
            "sz": "0.31",
            "side": "B",
            "time": 1769920606068,
            "startPosition": "0.0",
            "dir": "Buy",
            "closedPnl": "0.0",
            "hash": "0xc731e7561e5334a0c8ab043472ce7d01d400ff3bb95653726afa92a8dd570e8b",
            "oid": 308086083674,
            "crossed": true,
            "fee": "0.00021699",
            "tid": 812806034449156,
            "cloid": "0xd211f1c27288259290850338d22132a0",
            "feeToken": "HYPE",
            "twapId": null
        }"#;

        let fill: WsFillData = serde_json::from_str(json).unwrap();
        assert_eq!(fill.coin, "@107");
        assert_eq!(fill.px, dec!(31.737));
        assert_eq!(fill.sz, dec!(0.31));
        assert_eq!(fill.side, HyperliquidSide::Buy);
        assert_eq!(fill.oid, 308086083674);
        assert!(fill.crossed);
        assert_eq!(fill.fee, dec!(0.00021699));
        assert_eq!(fill.fee_token, "HYPE");
        assert_eq!(
            fill.cloid,
            Some("0xd211f1c27288259290850338d22132a0".to_string())
        );
        assert!(fill.twap_id.is_none() || fill.twap_id == Some(serde_json::Value::Null));
    }

    #[rstest]
    fn test_ws_user_fills_message_deserialization() {
        let json = r#"{"channel":"user","data":{"fills":[{"coin":"@107","px":"31.737","sz":"0.31","side":"B","time":1769920606068,"startPosition":"0.0","dir":"Buy","closedPnl":"0.0","hash":"0xc731e7561e5334a0c8ab043472ce7d01d400ff3bb95653726afa92a8dd570e8b","oid":308086083674,"crossed":true,"fee":"0.00021699","tid":812806034449156,"cloid":"0xd211f1c27288259290850338d22132a0","feeToken":"HYPE","twapId":null}]}}"#;

        let msg: HyperliquidWsMessage = serde_json::from_str(json).unwrap();

        match msg {
            HyperliquidWsMessage::User { data } => match data {
                WsUserEventData::Fills { fills } => {
                    assert_eq!(fills.len(), 1);
                    let fill = &fills[0];
                    assert_eq!(fill.coin, "@107");
                    assert_eq!(fill.px, dec!(31.737));
                    assert_eq!(
                        fill.cloid,
                        Some("0xd211f1c27288259290850338d22132a0".to_string())
                    );
                }
                _ => panic!("Expected Fills variant"),
            },
            _ => panic!("Expected User channel message"),
        }
    }

    #[rstest]
    fn test_ws_user_fills_message_with_builder_fee() {
        // Real message from production that was failing
        let json = r#"{"channel":"user","data":{"fills":[{"coin":"BTC","px":"79146.0","sz":"0.001","side":"A","time":1769940855551,"startPosition":"0.00093","dir":"Long > Short","closedPnl":"0.046128","hash":"0x5f8b9c337a197c4061050434769793020e020019151c9b1203544786391d562b","oid":308254271324,"crossed":false,"fee":"0.019785","builderFee":"0.007914","tid":404237815023429,"cloid":"0x50663504b0f4fedea00080176229d94f","feeToken":"USDC","twapId":null}]}}"#;

        let msg: HyperliquidWsMessage = serde_json::from_str(json).unwrap();

        match msg {
            HyperliquidWsMessage::User { data } => match data {
                WsUserEventData::Fills { fills } => {
                    assert_eq!(fills.len(), 1);
                    let fill = &fills[0];
                    assert_eq!(fill.coin, "BTC");
                    assert_eq!(fill.px, dec!(79146.0));
                    assert_eq!(fill.side, HyperliquidSide::Sell);
                    assert_eq!(fill.builder_fee, Some(dec!(0.007914)));
                    assert_eq!(fill.fee_token, "USDC");
                }
                _ => panic!("Expected Fills variant"),
            },
            _ => panic!("Expected User channel message"),
        }
    }

    #[rstest]
    fn test_ws_user_fills_message_with_liquidation() {
        // Real message from production that failed to parse: the liquidation
        // block carries `markPx` as a quoted string like every other decimal.
        let json = include_str!("../../test_data/ws_user_fill_liquidation.json");

        let msg: HyperliquidWsMessage = serde_json::from_str(json).unwrap();

        match msg {
            HyperliquidWsMessage::User { data } => match data {
                WsUserEventData::Fills { fills } => {
                    assert_eq!(fills.len(), 1);
                    let fill = &fills[0];
                    let liquidation = fill.liquidation.as_ref().expect("expected liquidation");
                    assert_eq!(fill.coin, "BTC");
                    assert_eq!(fill.side, HyperliquidSide::Sell);
                    assert_eq!(liquidation.mark_px, dec!(66607.0));
                    assert_eq!(liquidation.method, HyperliquidLiquidationMethod::Market);
                    assert_eq!(
                        liquidation.liquidated_user.as_deref(),
                        Some("0x360878d351f05975e25f1807a27895e1e5e004fb"),
                    );
                }
                _ => panic!("Expected Fills variant"),
            },
            _ => panic!("Expected User channel message"),
        }
    }

    #[rstest]
    fn test_ws_trade_data_round_trips_decimals_as_strings() {
        // Deserializing into Decimal then serializing must reproduce the
        // string wire form (with scale preserved), not emit a JSON number.
        let json = r#"{"coin":"BTC","side":"B","px":"66653.0","sz":"0.001","hash":"0xabc","time":1,"tid":2,"users":["0x1111111111111111111111111111111111111111","0x2222222222222222222222222222222222222222"]}"#;

        let trade: Trade = serde_json::from_str(json).unwrap();
        assert_eq!(trade.px, dec!(66653.0));
        assert_eq!(trade.sz, dec!(0.001));

        let value = serde_json::to_value(&trade).unwrap();
        assert_eq!(value["px"], serde_json::Value::from("66653.0"));
        assert_eq!(value["sz"], serde_json::Value::from("0.001"));
    }
}

/// Nautilus WebSocket message wrapper for routing to execution engine.
///
/// Wraps parsed messages from the handler.
///
/// All parsing happens in the handler layer, with parsed Nautilus domain objects.
/// passed through to the Python layer.
#[derive(Debug, Clone)]
pub enum NautilusWsMessage {
    /// Execution reports (order status and fills).
    ExecutionReports(Vec<ExecutionReport>),
    /// Parsed trade ticks.
    Trades(Vec<TradeTick>),
    /// Parsed quote tick (from BBO).
    Quote(QuoteTick),
    /// Parsed order book deltas.
    Deltas(OrderBookDeltas),
    /// Parsed order book depth-10 snapshot.
    Depth(Box<OrderBookDepth>),
    /// An order book frame that failed to parse, leaving the instrument's book out of sync.
    BookInvalid(InstrumentId),
    /// Parsed candle/bar.
    Candle(Bar),
    /// Mark price update.
    MarkPrice(MarkPriceUpdate),
    /// Index price update.
    IndexPrice(IndexPriceUpdate),
    /// Funding rate update.
    FundingRate(FundingRateUpdate),
    /// Custom data (e.g. allMids).
    CustomData(Data),
    /// Error occurred.
    Error(String),
    /// WebSocket reconnected.
    Reconnected,
}

/// Execution report wrapper for order status and fill reports.
///
/// This enum allows both order status updates and fill reports.
/// to be sent through the execution engine.
#[derive(Debug, Clone)]
#[allow(
    clippy::large_enum_variant,
    reason = "the variant size gap only crosses the threshold when high-precision widens the raw types"
)]
pub enum ExecutionReport {
    /// Order status report.
    Order(OrderStatusReport),
    /// Fill report.
    Fill(FillReport),
}
