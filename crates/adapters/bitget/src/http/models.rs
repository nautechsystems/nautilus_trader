// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
// -------------------------------------------------------------------------------------------------

use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
enum BitgetStringOrNumber {
    String(String),
    Number(f64),
}

impl BitgetStringOrNumber {
    fn into_string(self) -> String {
        match self {
            Self::String(value) => value,
            Self::Number(value) => {
                let mut rendered = value.to_string();
                if rendered.contains('e') || rendered.contains('E') {
                    rendered = format!("{value:.16}");
                    while rendered.contains('.') && rendered.ends_with('0') {
                        rendered.pop();
                    }
                    if rendered.ends_with('.') {
                        rendered.pop();
                    }
                }
                rendered
            }
        }
    }
}

fn deserialize_level_pairs<'de, D>(deserializer: D) -> Result<Vec<[String; 2]>, D::Error>
where
    D: Deserializer<'de>,
{
    let raw_levels = Vec::<[BitgetStringOrNumber; 2]>::deserialize(deserializer)?;
    Ok(raw_levels
        .into_iter()
        .map(|[price, size]| [price.into_string(), size.into_string()])
        .collect())
}

fn deserialize_default_on_null<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de> + Default,
{
    Option::<T>::deserialize(deserializer).map(|value| value.unwrap_or_default())
}

fn normalize_fee_detail_value(value: &mut Value) {
    match value {
        Value::Array(items) => {
            for item in items {
                normalize_fee_detail_value(item);
            }
        }
        Value::Object(map) => {
            if !map.contains_key("totalFee") {
                if let Some(fee) = map.get("fee").cloned() {
                    map.insert("totalFee".to_string(), fee);
                }
            }
        }
        _ => {}
    }
}

fn deserialize_fee_detail<'de, D>(deserializer: D) -> Result<Value, D::Error>
where
    D: Deserializer<'de>,
{
    let mut value = Option::<Value>::deserialize(deserializer)?.unwrap_or(Value::Null);
    normalize_fee_detail_value(&mut value);
    Ok(value)
}

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
    #[serde(deserialize_with = "deserialize_level_pairs")]
    pub asks: Vec<[String; 2]>,
    #[serde(deserialize_with = "deserialize_level_pairs")]
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
    #[serde(default, alias = "orderStatus")]
    pub status: String,
    #[serde(default, alias = "timeInForce")]
    pub force: String,
    #[serde(default)]
    pub price: String,
    #[serde(default, alias = "avgPrice")]
    pub price_avg: String,
    #[serde(default, alias = "qty")]
    pub size: String,
    #[serde(default, alias = "cumExecQty", alias = "filledQty")]
    pub base_volume: String,
    #[serde(default, alias = "cumExecValue")]
    pub quote_volume: String,
    #[serde(default)]
    pub reduce_only: String,
    #[serde(default, alias = "posSide")]
    pub pos_side: String,
    #[serde(default, alias = "createdTime")]
    pub c_time: String,
    #[serde(default, alias = "updatedTime")]
    pub u_time: String,
    #[serde(default, deserialize_with = "deserialize_fee_detail")]
    pub fee_detail: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct BitgetMixOrdersPage {
    #[serde(default, deserialize_with = "deserialize_default_on_null")]
    pub entrusted_list: Vec<BitgetOrderInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct BitgetUtaOrdersPage {
    #[serde(default, deserialize_with = "deserialize_default_on_null")]
    pub list: Vec<BitgetOrderInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct BitgetFillInfo {
    #[serde(default)]
    pub symbol: String,
    #[serde(default)]
    pub order_id: String,
    #[serde(default, alias = "execId")]
    pub trade_id: String,
    #[serde(default)]
    pub side: String,
    #[serde(default)]
    pub price: String,
    #[serde(default, alias = "avgPrice", alias = "execPrice")]
    pub price_avg: String,
    #[serde(default, alias = "qty", alias = "execQty")]
    pub size: String,
    #[serde(default)]
    pub trade_scope: String,
    #[serde(default)]
    pub fee_coin: String,
    #[serde(default)]
    pub fill_fee: String,
    #[serde(default, alias = "createdTime")]
    pub c_time: String,
    #[serde(default, alias = "updatedTime")]
    pub u_time: String,
    #[serde(default, deserialize_with = "deserialize_fee_detail")]
    pub fee_detail: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct BitgetMixFillsPage {
    #[serde(default, deserialize_with = "deserialize_default_on_null")]
    pub fill_list: Vec<BitgetFillInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct BitgetUtaFillsPage {
    #[serde(default, deserialize_with = "deserialize_default_on_null")]
    pub list: Vec<BitgetFillInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct BitgetPositionInfo {
    #[serde(default)]
    pub symbol: String,
    #[serde(default)]
    pub total: String,
    #[serde(default, alias = "posSide")]
    pub hold_side: String,
    #[serde(default, alias = "avgPrice")]
    pub open_price_avg: String,
    #[serde(default, alias = "positionId")]
    pub pos_id: String,
    #[serde(default, alias = "updatedTime")]
    pub u_time: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct BitgetUtaPositionsPage {
    #[serde(default, deserialize_with = "deserialize_default_on_null")]
    pub list: Vec<BitgetPositionInfo>,
}
