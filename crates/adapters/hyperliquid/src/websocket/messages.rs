// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

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

/// Represents subscription request types for WebSocket feeds.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type")]
#[serde(rename_all = "camelCase")]
pub enum SubscriptionRequest {
    /// All mid prices across markets.
    AllMids {
        #[serde(skip_serializing_if = "Option::is_none")]
        dex: Option<String>,
    },
    /// Notifications for a user
    Notification { user: String },
    /// Web data for frontend
    WebData2 { user: String },
    /// Candlestick data
    Candle { coin: String, interval: String },
    /// Level 2 order book
    L2Book {
        coin: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        #[serde(rename = "nSigFigs")]
        n_sig_figs: Option<u32>,
        #[serde(skip_serializing_if = "Option::is_none")]
        mantissa: Option<u32>,
    },
    /// Trade updates
    Trades { coin: String },
    /// Order updates for a user
    OrderUpdates { user: String },
    /// User events (fills, funding, liquidations)
    UserEvents { user: String },
    /// User fill history
    UserFills {
        user: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        #[serde(rename = "aggregateByTime")]
        aggregate_by_time: Option<bool>,
    },
    /// User funding payments
    UserFundings { user: String },
    /// User ledger updates (non-funding)
    UserNonFundingLedgerUpdates { user: String },
    /// Active asset context
    ActiveAssetCtx { coin: String },
    /// Active asset data for user
    ActiveAssetData { user: String, coin: String },
    /// TWAP slice fills
    UserTwapSliceFills { user: String },
    /// TWAP history
    UserTwapHistory { user: String },
    /// Best bid/offer updates
    Bbo { coin: String },
}

/// Post request wrapper for info and action requests
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
#[serde(rename_all = "lowercase")]
pub enum PostRequest {
    /// Info request (no signature required)
    Info { payload: serde_json::Value },
    /// Action request (requires signature)
    Action { payload: ActionPayload },
}

/// Action payload with signature
#[derive(Debug, Clone, Serialize)]
pub struct ActionPayload {
    pub action: ActionRequest,
    pub nonce: u64,
    pub signature: SignatureData,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "vaultAddress")]
    pub vault_address: Option<String>,
}

/// Signature data
#[derive(Debug, Clone, Serialize)]
pub struct SignatureData {
    pub r: String,
    pub s: String,
    pub v: String,
}

/// Action request types
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
#[serde(rename_all = "lowercase")]
pub enum ActionRequest {
    /// Place orders
    Order {
        orders: Vec<OrderRequest>,
        grouping: String,
    },
    /// Cancel orders
    Cancel { cancels: Vec<CancelRequest> },
    /// Cancel orders by client order ID
    CancelByCloid { cancels: Vec<CancelByCloidRequest> },
    /// Modify orders
    Modify { modifies: Vec<ModifyRequest> },
}

/// Order placement request
#[derive(Debug, Clone, Serialize)]
pub struct OrderRequest {
    /// Asset ID
    pub a: u32,
    /// Buy side (true = buy, false = sell)
    pub b: bool,
    /// Price
    pub p: String,
    /// Size
    pub s: String,
    /// Reduce only
    pub r: bool,
    /// Order type
    pub t: OrderTypeRequest,
    /// Client order ID (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub c: Option<String>,
}

/// Order type in request format
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
#[serde(rename_all = "lowercase")]
pub enum OrderTypeRequest {
    Limit {
        tif: TimeInForceRequest,
    },
    Trigger {
        #[serde(rename = "isMarket")]
        is_market: bool,
        #[serde(rename = "triggerPx")]
        trigger_px: String,
        tpsl: TpSlRequest,
    },
}

/// Time in force in request format
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "PascalCase")]
pub enum TimeInForceRequest {
    Alo,
    Ioc,
    Gtc,
}

/// TP/SL in request format
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum TpSlRequest {
    Tp,
    Sl,
}

/// Cancel order request
#[derive(Debug, Clone, Serialize)]
pub struct CancelRequest {
    /// Asset ID
    pub a: u32,
    /// Order ID
    pub o: u64,
}

/// Cancel by client order ID request
#[derive(Debug, Clone, Serialize)]
pub struct CancelByCloidRequest {
    /// Asset ID
    pub asset: u32,
    /// Client order ID
    pub cloid: String,
}

/// Modify order request
#[derive(Debug, Clone, Serialize)]
pub struct ModifyRequest {
    /// Order ID
    pub oid: u64,
    /// New order details
    pub order: OrderRequest,
}

/// Inbound WebSocket message from Hyperliquid server
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "channel")]
#[serde(rename_all = "camelCase")]
pub enum HyperliquidWsMessage {
    /// Subscription confirmation
    SubscriptionResponse { data: SubscriptionRequest },
    /// Post request response
    Post { data: PostResponse },
    /// All mid prices
    AllMids { data: AllMidsData },
    /// Notifications
    Notification { data: NotificationData },
    /// Web data
    WebData2 { data: serde_json::Value },
    /// Candlestick data
    Candle { data: Vec<CandleData> },
    /// Level 2 order book
    L2Book { data: WsBookData },
    /// Trade updates
    Trades { data: Vec<WsTradeData> },
    /// Order updates
    OrderUpdates { data: Vec<WsOrderData> },
    /// User events
    UserEvents { data: WsUserEventData },
    /// User fills
    UserFills { data: WsUserFillsData },
    /// User funding payments
    UserFundings { data: WsUserFundingsData },
    /// User ledger updates
    UserNonFundingLedgerUpdates { data: serde_json::Value },
    /// Active asset context
    ActiveAssetCtx { data: WsActiveAssetCtxData },
    /// Active asset data
    ActiveAssetData { data: WsActiveAssetData },
    /// TWAP slice fills
    UserTwapSliceFills { data: WsUserTwapSliceFillsData },
    /// TWAP history
    UserTwapHistory { data: WsUserTwapHistoryData },
    /// Best bid/offer
    Bbo { data: WsBboData },
    /// Pong response
    Pong,
}

/// Post response data
#[derive(Debug, Clone, Deserialize)]
pub struct PostResponse {
    pub id: u64,
    pub response: PostResponsePayload,
}

/// Post response payload
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
#[serde(rename_all = "lowercase")]
pub enum PostResponsePayload {
    Info { payload: serde_json::Value },
    Action { payload: serde_json::Value },
    Error { payload: String },
}

/// All mid prices data
#[derive(Debug, Clone, Deserialize)]
pub struct AllMidsData {
    pub mids: HashMap<String, String>,
}

/// Notification data
#[derive(Debug, Clone, Deserialize)]
pub struct NotificationData {
    pub notification: String,
}

/// Candlestick data
#[derive(Debug, Clone, Deserialize)]
pub struct CandleData {
    /// Open time (millis)
    pub t: u64,
    /// Close time (millis)
    #[serde(rename = "T")]
    pub close_time: u64,
    /// Symbol
    pub s: String,
    /// Interval
    pub i: String,
    /// Open price
    pub o: f64,
    /// Close price
    pub c: f64,
    /// High price
    pub h: f64,
    /// Low price
    pub l: f64,
    /// Volume
    pub v: f64,
    /// Number of trades
    pub n: u32,
}

/// WebSocket book data
#[derive(Debug, Clone, Deserialize)]
pub struct WsBookData {
    pub coin: String,
    pub levels: [Vec<WsLevelData>; 2], // [bids, asks]
    pub time: u64,
}

/// WebSocket level data
#[derive(Debug, Clone, Deserialize)]
pub struct WsLevelData {
    /// Price
    pub px: String,
    /// Size
    pub sz: String,
    /// Number of orders
    pub n: u32,
}

/// WebSocket trade data
#[derive(Debug, Clone, Deserialize)]
pub struct WsTradeData {
    pub coin: String,
    pub side: String,
    pub px: String,
    pub sz: String,
    pub hash: String,
    pub time: u64,
    pub tid: u64,
    pub users: [String; 2], // [buyer, seller]
}

/// WebSocket order data
#[derive(Debug, Clone, Deserialize)]
pub struct WsOrderData {
    pub order: WsBasicOrderData,
    pub status: String,
    #[serde(rename = "statusTimestamp")]
    pub status_timestamp: u64,
}

/// Basic order data
#[derive(Debug, Clone, Deserialize)]
pub struct WsBasicOrderData {
    pub coin: String,
    pub side: String,
    #[serde(rename = "limitPx")]
    pub limit_px: String,
    pub sz: String,
    pub oid: u64,
    pub timestamp: u64,
    #[serde(rename = "origSz")]
    pub orig_sz: String,
    pub cloid: Option<String>,
}

/// WebSocket user event data
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
}

/// WebSocket fill data
#[derive(Debug, Clone, Deserialize)]
pub struct WsFillData {
    pub coin: String,
    pub px: String,
    pub sz: String,
    pub side: String,
    pub time: u64,
    #[serde(rename = "startPosition")]
    pub start_position: String,
    pub dir: String,
    #[serde(rename = "closedPnl")]
    pub closed_pnl: String,
    pub hash: String,
    pub oid: u64,
    pub crossed: bool,
    pub fee: String,
    pub tid: u64,
    pub liquidation: Option<FillLiquidationData>,
    #[serde(rename = "feeToken")]
    pub fee_token: String,
    #[serde(rename = "builderFee")]
    pub builder_fee: Option<String>,
}

/// Fill liquidation data
#[derive(Debug, Clone, Deserialize)]
pub struct FillLiquidationData {
    #[serde(rename = "liquidatedUser")]
    pub liquidated_user: Option<String>,
    #[serde(rename = "markPx")]
    pub mark_px: f64,
    pub method: String, // "market" | "backstop"
}

/// WebSocket user funding data
#[derive(Debug, Clone, Deserialize)]
pub struct WsUserFundingData {
    pub time: u64,
    pub coin: String,
    pub usdc: String,
    pub szi: String,
    #[serde(rename = "fundingRate")]
    pub funding_rate: String,
}

/// WebSocket liquidation data
#[derive(Debug, Clone, Deserialize)]
pub struct WsLiquidationData {
    pub lid: u64,
    pub liquidator: String,
    pub liquidated_user: String,
    pub liquidated_ntl_pos: String,
    pub liquidated_account_value: String,
}

/// WebSocket non-user cancel data
#[derive(Debug, Clone, Deserialize)]
pub struct WsNonUserCancelData {
    pub coin: String,
    pub oid: u64,
}

/// WebSocket user fills data
#[derive(Debug, Clone, Deserialize)]
pub struct WsUserFillsData {
    #[serde(rename = "isSnapshot")]
    pub is_snapshot: Option<bool>,
    pub user: String,
    pub fills: Vec<WsFillData>,
}

/// WebSocket user fundings data
#[derive(Debug, Clone, Deserialize)]
pub struct WsUserFundingsData {
    #[serde(rename = "isSnapshot")]
    pub is_snapshot: Option<bool>,
    pub user: String,
    pub fundings: Vec<WsUserFundingData>,
}

/// WebSocket active asset context data
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum WsActiveAssetCtxData {
    Perp { coin: String, ctx: PerpsAssetCtx },
    Spot { coin: String, ctx: SpotAssetCtx },
}

/// Shared asset context fields
#[derive(Debug, Clone, Deserialize)]
pub struct SharedAssetCtx {
    #[serde(rename = "dayNtlVlm")]
    pub day_ntl_vlm: f64,
    #[serde(rename = "prevDayPx")]
    pub prev_day_px: f64,
    #[serde(rename = "markPx")]
    pub mark_px: f64,
    #[serde(rename = "midPx")]
    pub mid_px: Option<f64>,
}

/// Perps asset context
#[derive(Debug, Clone, Deserialize)]
pub struct PerpsAssetCtx {
    #[serde(flatten)]
    pub shared: SharedAssetCtx,
    pub funding: f64,
    #[serde(rename = "openInterest")]
    pub open_interest: f64,
    #[serde(rename = "oraclePx")]
    pub oracle_px: f64,
}

/// Spot asset context
#[derive(Debug, Clone, Deserialize)]
pub struct SpotAssetCtx {
    #[serde(flatten)]
    pub shared: SharedAssetCtx,
    #[serde(rename = "circulatingSupply")]
    pub circulating_supply: f64,
}

/// WebSocket active asset data
#[derive(Debug, Clone, Deserialize)]
pub struct WsActiveAssetData {
    pub user: String,
    pub coin: String,
    pub leverage: LeverageData,
    #[serde(rename = "maxTradeSzs")]
    pub max_trade_szs: [f64; 2],
    #[serde(rename = "availableToTrade")]
    pub available_to_trade: [f64; 2],
}

/// Leverage data
#[derive(Debug, Clone, Deserialize)]
pub struct LeverageData {
    pub value: f64,
    pub type_: String,
}

/// WebSocket TWAP slice fills data
#[derive(Debug, Clone, Deserialize)]
pub struct WsUserTwapSliceFillsData {
    #[serde(rename = "isSnapshot")]
    pub is_snapshot: Option<bool>,
    pub user: String,
    #[serde(rename = "twapSliceFills")]
    pub twap_slice_fills: Vec<WsTwapSliceFillData>,
}

/// TWAP slice fill data
#[derive(Debug, Clone, Deserialize)]
pub struct WsTwapSliceFillData {
    pub fill: WsFillData,
    #[serde(rename = "twapId")]
    pub twap_id: u64,
}

/// WebSocket TWAP history data
#[derive(Debug, Clone, Deserialize)]
pub struct WsUserTwapHistoryData {
    #[serde(rename = "isSnapshot")]
    pub is_snapshot: Option<bool>,
    pub user: String,
    pub history: Vec<WsTwapHistoryData>,
}

/// TWAP history data
#[derive(Debug, Clone, Deserialize)]
pub struct WsTwapHistoryData {
    pub state: TwapStateData,
    pub status: TwapStatusData,
    pub time: u64,
}

/// TWAP state data
#[derive(Debug, Clone, Deserialize)]
pub struct TwapStateData {
    pub coin: String,
    pub user: String,
    pub side: String,
    pub sz: f64,
    #[serde(rename = "executedSz")]
    pub executed_sz: f64,
    #[serde(rename = "executedNtl")]
    pub executed_ntl: f64,
    pub minutes: u32,
    #[serde(rename = "reduceOnly")]
    pub reduce_only: bool,
    pub randomize: bool,
    pub timestamp: u64,
}

/// TWAP status data
#[derive(Debug, Clone, Deserialize)]
pub struct TwapStatusData {
    pub status: String, // "activated" | "terminated" | "finished" | "error"
    pub description: String,
}

/// WebSocket BBO data
#[derive(Debug, Clone, Deserialize)]
pub struct WsBboData {
    pub coin: String,
    pub time: u64,
    pub bbo: [Option<WsLevelData>; 2], // [bid, ask]
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use serde_json;

    use super::*;

    #[rstest]
    fn test_subscription_request_serialization() {
        let sub = SubscriptionRequest::L2Book {
            coin: "BTC".to_string(),
            n_sig_figs: Some(5),
            mantissa: None,
        };

        let json = serde_json::to_string(&sub).unwrap();
        assert!(json.contains(r#""type":"l2Book""#));
        assert!(json.contains(r#""coin":"BTC""#));
    }

    #[rstest]
    fn test_hyperliquid_ws_request_serialization() {
        let req = HyperliquidWsRequest::Subscribe {
            subscription: SubscriptionRequest::Trades {
                coin: "ETH".to_string(),
            },
        };

        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains(r#""method":"subscribe""#));
        assert!(json.contains(r#""type":"trades""#));
    }

    #[rstest]
    fn test_order_request_serialization() {
        let order = OrderRequest {
            a: 0,    // BTC asset ID
            b: true, // buy
            p: "50000.0".to_string(),
            s: "0.1".to_string(),
            r: false,
            t: OrderTypeRequest::Limit {
                tif: TimeInForceRequest::Gtc,
            },
            c: Some("client-123".to_string()),
        };

        let json = serde_json::to_string(&order).unwrap();
        assert!(json.contains(r#""a":0"#));
        assert!(json.contains(r#""b":true"#));
        assert!(json.contains(r#""p":"50000.0""#));
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
            "users": ["0xabc", "0xdef"]
        }"#;

        let trade: WsTradeData = serde_json::from_str(json).unwrap();
        assert_eq!(trade.coin, "BTC");
        assert_eq!(trade.side, "B");
        assert_eq!(trade.px, "50000.0");
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

        let book: WsBookData = serde_json::from_str(json).unwrap();
        assert_eq!(book.coin, "ETH");
        assert_eq!(book.levels[0].len(), 1);
        assert_eq!(book.levels[1].len(), 1);
    }
}
