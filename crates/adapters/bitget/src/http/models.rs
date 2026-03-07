// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
// -------------------------------------------------------------------------------------------------

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BitgetApiResponse<T> {
    pub code: String,
    pub msg: String,
    #[serde(default)]
    pub request_time: i64,
    pub data: T,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.bitget")
)]
pub struct BitgetSpotSymbol {
    pub symbol: String,
    pub base_coin: String,
    pub quote_coin: String,
    #[serde(default)]
    pub price_precision: String,
    #[serde(default)]
    pub quantity_precision: String,
    #[serde(default)]
    pub min_trade_amount: String,
    #[serde(default, rename = "minTradeUSDT")]
    pub min_trade_usdt: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.bitget")
)]
pub struct BitgetContractSymbol {
    pub symbol: String,
    #[serde(default)]
    pub product_type: Option<String>,
    #[serde(default)]
    pub symbol_type: Option<String>,
    #[serde(default)]
    pub base_coin: Option<String>,
    #[serde(default)]
    pub quote_coin: Option<String>,
    #[serde(default)]
    pub delivery_time: Option<String>,
    #[serde(default)]
    pub price_place: Option<String>,
    #[serde(default)]
    pub price_end_step: Option<String>,
    #[serde(default)]
    pub volume_place: Option<String>,
    #[serde(default)]
    pub size_multiplier: Option<String>,
    #[serde(default)]
    pub min_trade_num: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.bitget")
)]
pub struct BitgetOrderBookSnapshot {
    pub asks: Vec<[String; 2]>,
    pub bids: Vec<[String; 2]>,
    pub ts: String,
    #[serde(default)]
    pub scale: Option<String>,
    #[serde(default)]
    pub precision: Option<String>,
    #[serde(default)]
    pub is_max_precision: Option<String>,
}

/// Response alias for spot symbols endpoint.
pub type BitgetSpotSymbolsResponse = BitgetApiResponse<Vec<BitgetSpotSymbol>>;

/// Response alias for contract configuration endpoint.
pub type BitgetContractConfigResponse = BitgetApiResponse<Vec<BitgetContractSymbol>>;

/// Response alias for candlestick endpoint (spot or futures).
pub type BitgetCandlesResponse = BitgetApiResponse<Vec<Vec<String>>>;

/// Response alias for funding rate endpoint.
pub type BitgetFundingResponse = BitgetApiResponse<Vec<BitgetCurrentFundingRate>>;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct BitgetFundingHistoryPage {
    #[serde(default)]
    pub result_list: Vec<BitgetHistoricalFundingRate>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct BitgetCurrentFundingRate {
    #[serde(default)]
    pub symbol: String,
    #[serde(default)]
    pub funding_rate: String,
    #[serde(default)]
    pub next_funding_rate: String,
    #[serde(default)]
    pub next_funding_time: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct BitgetHistoricalFundingRate {
    #[serde(default)]
    pub symbol: String,
    #[serde(default)]
    pub funding_rate: String,
    #[serde(default)]
    pub funding_time: String,
}


#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct BitgetOrderAck {
    #[serde(default)]
    pub order_id: String,
    #[serde(default)]
    pub client_oid: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct BitgetBatchCancelFailure {
    #[serde(default)]
    pub order_id: String,
    #[serde(default)]
    pub client_oid: String,
    #[serde(default)]
    pub error_code: String,
    #[serde(default)]
    pub error_msg: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct BitgetBatchCancelResponse {
    #[serde(default)]
    pub success_list: Vec<BitgetOrderAck>,
    #[serde(default)]
    pub failure_list: Vec<BitgetBatchCancelFailure>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct BitgetOrderInfo {
    #[serde(default)]
    pub symbol: String,
    #[serde(default)]
    pub order_id: String,
    #[serde(default)]
    pub client_oid: String,
    #[serde(default)]
    pub side: String,
    #[serde(default)]
    pub order_type: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub force: String,
    #[serde(default)]
    pub price: String,
    #[serde(default)]
    pub price_avg: String,
    #[serde(default)]
    pub size: String,
    #[serde(default)]
    pub base_volume: String,
    #[serde(default)]
    pub quote_volume: String,
    #[serde(default)]
    pub reduce_only: String,
    #[serde(default)]
    pub pos_side: String,
    #[serde(default)]
    pub c_time: String,
    #[serde(default)]
    pub u_time: String,
    #[serde(default)]
    pub fee_detail: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct BitgetMixOrdersPage {
    #[serde(default)]
    pub entrusted_list: Vec<BitgetOrderInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct BitgetFillInfo {
    #[serde(default)]
    pub symbol: String,
    #[serde(default)]
    pub order_id: String,
    #[serde(default)]
    pub trade_id: String,
    #[serde(default)]
    pub side: String,
    #[serde(default)]
    pub price: String,
    #[serde(default)]
    pub price_avg: String,
    #[serde(default)]
    pub size: String,
    #[serde(default)]
    pub trade_scope: String,
    #[serde(default)]
    pub fee_coin: String,
    #[serde(default)]
    pub fill_fee: String,
    #[serde(default)]
    pub c_time: String,
    #[serde(default)]
    pub u_time: String,
    #[serde(default)]
    pub fee_detail: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct BitgetMixFillsPage {
    #[serde(default)]
    pub fill_list: Vec<BitgetFillInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct BitgetPositionInfo {
    #[serde(default)]
    pub symbol: String,
    #[serde(default)]
    pub total: String,
    #[serde(default)]
    pub hold_side: String,
    #[serde(default)]
    pub open_price_avg: String,
    #[serde(default)]
    pub pos_id: String,
    #[serde(default)]
    pub u_time: String,
}
