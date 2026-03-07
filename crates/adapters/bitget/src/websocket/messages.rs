// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
// -------------------------------------------------------------------------------------------------

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BitgetWsArg {
    #[serde(rename = "instType")]
    pub inst_type: String,
    pub channel: String,
    #[serde(rename = "instId")]
    pub inst_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BitgetWsSubscriptionMessage {
    pub op: String,
    pub args: Vec<BitgetWsArg>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BitgetWsAccountArg {
    #[serde(rename = "instType")]
    pub inst_type: String,
    pub channel: String,
    pub coin: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BitgetWsAccountSubscriptionMessage {
    pub op: String,
    pub args: Vec<BitgetWsAccountArg>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BitgetWsLoginArg {
    #[serde(rename = "apiKey")]
    pub api_key: String,
    pub passphrase: String,
    pub timestamp: String,
    pub sign: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BitgetWsLoginMessage {
    pub op: String,
    pub args: Vec<BitgetWsLoginArg>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BitgetWsEvent {
    pub action: Option<String>,
    pub arg: Option<BitgetWsArg>,
    pub event: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BitgetWsBookData {
    pub asks: Vec<[String; 2]>,
    pub bids: Vec<[String; 2]>,
    pub checksum: i32,
    pub ts: String,
    pub seq: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BitgetWsBookMessage {
    pub action: String,
    pub arg: BitgetWsArg,
    pub data: Vec<BitgetWsBookData>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BitgetWsTrade {
    #[serde(rename = "tradeId", alias = "trade_id")]
    pub trade_id: String,
    #[serde(alias = "px")]
    pub price: String,
    #[serde(alias = "sz")]
    pub size: String,
    pub side: String,
    pub ts: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BitgetWsTradeMessage {
    pub arg: BitgetWsArg,
    pub data: Vec<BitgetWsTrade>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BitgetWsTickerData {
    #[serde(rename = "instId")]
    pub inst_id: String,
    #[serde(rename = "last")]
    #[serde(alias = "lastPr")]
    #[serde(default)]
    pub last_price: Option<String>,
    #[serde(rename = "bidPr")]
    #[serde(alias = "bidPx")]
    #[serde(alias = "bidPrice")]
    #[serde(default)]
    pub bid_price: Option<String>,
    #[serde(rename = "askPr")]
    #[serde(alias = "askPx")]
    #[serde(alias = "askPrice")]
    #[serde(default)]
    pub ask_price: Option<String>,
    #[serde(rename = "bidSz")]
    #[serde(alias = "bidSize")]
    #[serde(default)]
    pub bid_size: Option<String>,
    #[serde(rename = "askSz")]
    #[serde(alias = "askSize")]
    #[serde(default)]
    pub ask_size: Option<String>,
    #[serde(rename = "markPr")]
    #[serde(alias = "markPrice")]
    #[serde(default)]
    pub mark_price: Option<String>,
    #[serde(rename = "indexPx")]
    #[serde(alias = "indexPrice")]
    #[serde(default)]
    pub index_price: Option<String>,
    #[serde(rename = "fundingRate")]
    #[serde(alias = "funding_rate")]
    #[serde(default)]
    pub funding_rate: Option<String>,
    #[serde(rename = "nextFundingTime")]
    #[serde(alias = "next_funding_time")]
    #[serde(default)]
    pub next_funding_time: Option<String>,
    #[serde(rename = "ts")]
    #[serde(default)]
    pub ts: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BitgetWsTickerMessage {
    pub action: String,
    pub arg: BitgetWsArg,
    pub data: Vec<BitgetWsTickerData>,
}

pub type BitgetWsCandle = Vec<String>;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BitgetWsCandleMessage {
    pub action: String,
    pub arg: BitgetWsArg,
    pub data: Vec<BitgetWsCandle>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BitgetWsPrivateAccount {
    pub coin: String,
    pub available: String,
    pub frozen: String,
    pub locked: String,
    #[serde(rename = "limitAvailable")]
    pub limit_available: String,
    #[serde(rename = "uTime")]
    pub u_time: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BitgetWsPrivateOrderFeeDetail {
    #[serde(rename = "feeCoin")]
    pub fee_coin: String,
    pub fee: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BitgetWsPrivateOrder {
    #[serde(rename = "instId")]
    pub inst_id: String,
    #[serde(rename = "orderId")]
    pub order_id: String,
    #[serde(rename = "clientOid")]
    pub client_oid: String,
    pub price: String,
    pub size: String,
    #[serde(rename = "fillPrice")]
    pub fill_price: Option<String>,
    #[serde(rename = "fillQuantity")]
    pub fill_quantity: Option<String>,
    #[serde(rename = "fillFee")]
    pub fill_fee: Option<String>,
    #[serde(rename = "fillFeeCoin")]
    pub fill_fee_coin: Option<String>,
    #[serde(rename = "tradeId")]
    pub trade_id: Option<String>,
    pub side: String,
    #[serde(rename = "orderType")]
    pub order_type: String,
    #[serde(rename = "force")]
    pub time_in_force: Option<String>,
    #[serde(rename = "accBaseVolume")]
    pub acc_base_volume: String,
    #[serde(rename = "priceAvg")]
    pub price_avg: String,
    pub status: String,
    #[serde(rename = "cTime")]
    pub c_time: String,
    #[serde(rename = "uTime")]
    pub u_time: String,
    #[serde(rename = "feeDetail", default)]
    pub fee_detail: Vec<BitgetWsPrivateOrderFeeDetail>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BitgetWsPrivateFillFeeDetail {
    #[serde(rename = "feeCoin")]
    pub fee_coin: String,
    pub deduction: String,
    #[serde(rename = "totalDeductionFee")]
    pub total_deduction_fee: String,
    #[serde(rename = "totalFee")]
    pub total_fee: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BitgetWsPrivateFill {
    #[serde(rename = "orderId")]
    pub order_id: String,
    #[serde(rename = "tradeId")]
    pub trade_id: String,
    pub symbol: String,
    #[serde(rename = "orderType")]
    pub order_type: String,
    pub side: String,
    #[serde(rename = "priceAvg")]
    pub price_avg: String,
    pub size: String,
    pub amount: String,
    #[serde(rename = "tradeScope")]
    pub trade_scope: String,
    #[serde(rename = "feeDetail", default)]
    pub fee_detail: Vec<BitgetWsPrivateFillFeeDetail>,
    #[serde(rename = "cTime")]
    pub c_time: String,
    #[serde(rename = "uTime")]
    pub u_time: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BitgetWsPrivatePosition {
    #[serde(rename = "posId")]
    pub pos_id: String,
    #[serde(rename = "instId")]
    pub inst_id: String,
    #[serde(rename = "marginCoin")]
    pub margin_coin: String,
    #[serde(rename = "marginSize")]
    pub margin_size: String,
    #[serde(rename = "marginMode")]
    pub margin_mode: String,
    #[serde(rename = "holdSide")]
    pub hold_side: String,
    #[serde(rename = "posMode")]
    pub pos_mode: Option<String>,
    pub total: String,
    pub available: String,
    pub frozen: String,
    #[serde(rename = "openPriceAvg")]
    pub open_price_avg: String,
    pub leverage: String,
    #[serde(rename = "unrealizedPL")]
    pub unrealized_pl: String,
    #[serde(rename = "liquidationPrice")]
    pub liquidation_price: String,
    #[serde(rename = "markPrice")]
    pub mark_price: String,
    #[serde(rename = "uTime")]
    pub u_time: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BitgetWsPrivateChannelMessage<T> {
    pub action: String,
    pub arg: serde_json::Value,
    pub data: Vec<T>,
}
