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

//! Query parameter structs for Kraken Futures HTTP API requests.

use derive_builder::Builder;
use serde::{Deserialize, Serialize};
use ustr::Ustr;

use crate::common::enums::{KrakenFuturesOrderType, KrakenOrderSide, KrakenTriggerSignal};

/// Parameters for sending an order via `POST /api/v3/sendorder`.
///
/// # References
/// - <https://docs.kraken.com/api/docs/futures-api/trading/send-order/>
#[derive(Clone, Debug, Serialize, Deserialize, Builder)]
#[serde(rename_all = "camelCase")]
#[builder(setter(into, strip_option), build_fn(validate = "Self::validate"))]
pub struct KrakenFuturesSendOrderParams {
    /// The symbol of the futures contract (e.g., "PI_XBTUSD").
    pub symbol: Ustr,

    /// The order side: "buy" or "sell".
    pub side: KrakenOrderSide,

    /// The order type: lmt, ioc, post, mkt, stp, take_profit, stop_loss.
    pub order_type: KrakenFuturesOrderType,

    /// The order size in contracts.
    pub size: String,

    /// Optional client order ID for tracking.
    #[builder(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cli_ord_id: Option<String>,

    /// Limit price (required for limit orders).
    #[builder(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit_price: Option<String>,

    /// Stop/trigger price (required for stop orders).
    #[builder(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_price: Option<String>,

    /// If true, the order will only reduce an existing position.
    #[builder(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reduce_only: Option<bool>,

    /// Trigger signal for stop orders: last, mark, or spot.
    #[builder(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trigger_signal: Option<KrakenTriggerSignal>,

    /// Trailing stop offset value.
    #[builder(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trailing_stop_deviation_unit: Option<String>,

    /// Trailing stop max deviation.
    #[builder(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trailing_stop_max_deviation: Option<String>,

    /// Partner/broker attribution ID.
    #[builder(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub broker: Option<Ustr>,
}

impl KrakenFuturesSendOrderParamsBuilder {
    fn validate(&self) -> Result<(), String> {
        // Validate limit price is present for limit-type orders
        if let Some(ref order_type) = self.order_type {
            match order_type {
                KrakenFuturesOrderType::Limit
                | KrakenFuturesOrderType::Ioc
                | KrakenFuturesOrderType::Post
                    if (self.limit_price.is_none()
                        || self.limit_price.as_ref().unwrap().is_none()) =>
                {
                    return Err("limit_price is required for limit orders".to_string());
                }
                KrakenFuturesOrderType::Stop | KrakenFuturesOrderType::StopLoss
                    if (self.stop_price.is_none()
                        || self.stop_price.as_ref().unwrap().is_none()) =>
                {
                    return Err("stop_price is required for stop orders".to_string());
                }
                _ => {}
            }
        }
        Ok(())
    }
}

/// Parameters for canceling an order via `POST /api/v3/cancelorder`.
///
/// # References
/// - <https://docs.kraken.com/api/docs/futures-api/trading/cancel-order/>
#[derive(Clone, Debug, Serialize, Deserialize, Builder)]
#[serde(rename_all = "camelCase")]
#[builder(setter(into, strip_option))]
pub struct KrakenFuturesCancelOrderParams {
    /// The venue order ID to cancel.
    #[builder(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub order_id: Option<String>,

    /// The client order ID to cancel.
    #[builder(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cli_ord_id: Option<String>,
}

/// A batch cancel item for `POST /derivatives/api/v3/batchorder`.
///
/// # References
/// - <https://docs.kraken.com/api/docs/futures-api/trading/send-batch-order/>
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct KrakenFuturesBatchCancelItem {
    /// The operation type, always "cancel" for this item.
    pub order: String,

    /// The venue order ID to cancel.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub order_id: Option<String>,

    /// The client order ID to cancel (alternative to order_id).
    #[serde(rename = "cliOrdId", skip_serializing_if = "Option::is_none")]
    pub cli_ord_id: Option<String>,
}

impl KrakenFuturesBatchCancelItem {
    /// Create a batch cancel item from a venue order ID.
    #[must_use]
    pub fn from_order_id(order_id: impl Into<String>) -> Self {
        Self {
            order: "cancel".to_string(),
            order_id: Some(order_id.into()),
            cli_ord_id: None,
        }
    }

    /// Create a batch cancel item from a client order ID.
    #[must_use]
    pub fn from_client_order_id(cli_ord_id: impl Into<String>) -> Self {
        Self {
            order: "cancel".to_string(),
            order_id: None,
            cli_ord_id: Some(cli_ord_id.into()),
        }
    }
}

/// A batch send item for `POST /derivatives/api/v3/batchorder`.
///
/// # References
/// - <https://docs.kraken.com/api/docs/futures-api/trading/send-batch-order/>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KrakenFuturesBatchSendItem {
    /// The operation type, always "send" for this item.
    pub order: String,

    /// An order tag to correlate batch responses with requests.
    pub order_tag: String,

    /// The symbol of the futures contract.
    pub symbol: Ustr,

    /// The order side.
    pub side: KrakenOrderSide,

    /// The order type.
    pub order_type: KrakenFuturesOrderType,

    /// The order size in contracts.
    pub size: String,

    /// Optional client order ID for tracking.
    #[serde(rename = "cliOrdId", skip_serializing_if = "Option::is_none")]
    pub cli_ord_id: Option<String>,

    /// Limit price (required for limit orders).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit_price: Option<String>,

    /// Stop/trigger price (required for stop orders).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_price: Option<String>,

    /// If true, the order will only reduce an existing position.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reduce_only: Option<bool>,

    /// Trigger signal for stop orders.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trigger_signal: Option<KrakenTriggerSignal>,
}

impl KrakenFuturesBatchSendItem {
    /// Creates a batch send item from send order params.
    #[must_use]
    pub fn from_params(params: KrakenFuturesSendOrderParams, order_tag: impl Into<String>) -> Self {
        Self {
            order: "send".to_string(),
            order_tag: order_tag.into(),
            symbol: params.symbol,
            side: params.side,
            order_type: params.order_type,
            size: params.size,
            cli_ord_id: params.cli_ord_id,
            limit_price: params.limit_price,
            stop_price: params.stop_price,
            reduce_only: params.reduce_only,
            trigger_signal: params.trigger_signal,
        }
    }
}

/// A batch edit item for `POST /derivatives/api/v3/batchorder`.
///
/// # References
/// - <https://docs.kraken.com/api/docs/futures-api/trading/send-batch-order/>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KrakenFuturesBatchEditItem {
    /// The operation type, always "edit" for this item.
    pub order: String,

    /// An order tag to correlate batch responses with requests.
    pub order_tag: String,

    /// The venue order ID to edit.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub order_id: Option<String>,

    /// The client order ID to edit.
    #[serde(rename = "cliOrdId", skip_serializing_if = "Option::is_none")]
    pub cli_ord_id: Option<String>,

    /// New order size.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<String>,

    /// New limit price.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit_price: Option<String>,

    /// New stop price.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_price: Option<String>,
}

impl KrakenFuturesBatchEditItem {
    /// Creates a batch edit item from edit order params.
    #[must_use]
    pub fn from_params(params: KrakenFuturesEditOrderParams, order_tag: impl Into<String>) -> Self {
        Self {
            order: "edit".to_string(),
            order_tag: order_tag.into(),
            order_id: params.order_id,
            cli_ord_id: params.cli_ord_id,
            size: params.size,
            limit_price: params.limit_price,
            stop_price: params.stop_price,
        }
    }
}

/// Parameters for batch order operations via `POST /derivatives/api/v3/batchorder`.
///
/// The batchorder endpoint uses a special body format: `json={"batchOrder": [...]}`
/// where the JSON is NOT URL-encoded.
///
/// # References
/// - <https://docs.kraken.com/api/docs/futures-api/trading/send-batch-order/>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KrakenFuturesBatchOrderParams<T: Serialize> {
    /// List of batch order operations.
    pub batch_order: Vec<T>,
}

impl<T: Serialize> KrakenFuturesBatchOrderParams<T> {
    /// Create new batch order params.
    #[must_use]
    pub fn new(batch_order: Vec<T>) -> Self {
        Self { batch_order }
    }

    /// Serialize to the special `json=...` body format required by this endpoint.
    pub fn to_body(&self) -> Result<String, serde_json::Error> {
        let json_str = serde_json::to_string(self)?;
        Ok(format!("json={json_str}"))
    }
}

/// Parameters for editing an order via `POST /api/v3/editorder`.
///
/// # References
/// - <https://docs.kraken.com/api/docs/futures-api/trading/edit-order/>
#[derive(Clone, Debug, Serialize, Deserialize, Builder)]
#[serde(rename_all = "camelCase")]
#[builder(setter(into, strip_option))]
pub struct KrakenFuturesEditOrderParams {
    /// The venue order ID to edit.
    #[builder(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub order_id: Option<String>,

    /// The client order ID to edit.
    #[builder(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cli_ord_id: Option<String>,

    /// New order size.
    #[builder(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<String>,

    /// New limit price.
    #[builder(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit_price: Option<String>,

    /// New stop price.
    #[builder(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_price: Option<String>,
}

/// Parameters for canceling all orders via `POST /api/v3/cancelallorders`.
///
/// # References
/// - <https://docs.kraken.com/api/docs/futures-api/trading/cancel-all-orders/>
#[derive(Clone, Debug, Default, Serialize, Deserialize, Builder)]
#[serde(rename_all = "camelCase")]
#[builder(setter(into, strip_option), default)]
pub struct KrakenFuturesCancelAllOrdersParams {
    /// Optional symbol filter - only cancel orders for this symbol.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbol: Option<Ustr>,
}

/// Parameters for getting open orders via `GET /api/v3/openorders`.
///
/// # References
/// - <https://docs.kraken.com/api/docs/futures-api/trading/get-open-orders/>
#[derive(Clone, Debug, Default, Serialize, Deserialize, Builder)]
#[serde(rename_all = "camelCase")]
#[builder(setter(into, strip_option), default)]
pub struct KrakenFuturesOpenOrdersParams {
    // Currently no parameters, but kept for future extensibility
}

/// Parameters for getting fills via `GET /api/v3/fills`.
///
/// # References
/// - <https://docs.kraken.com/api/docs/futures-api/trading/get-fills/>
#[derive(Clone, Debug, Default, Serialize, Deserialize, Builder)]
#[serde(rename_all = "camelCase")]
#[builder(setter(into, strip_option), default)]
pub struct KrakenFuturesFillsParams {
    /// Filter fills after this timestamp (milliseconds).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_fill_time: Option<String>,
}

/// Parameters for getting open positions via `GET /api/v3/openpositions`.
///
/// # References
/// - <https://docs.kraken.com/api/docs/futures-api/trading/get-open-positions/>
#[derive(Clone, Debug, Default, Serialize, Deserialize, Builder)]
#[serde(rename_all = "camelCase")]
#[builder(setter(into, strip_option), default)]
pub struct KrakenFuturesOpenPositionsParams {
    // Currently no parameters, but kept for future extensibility
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_send_order_params_builder() {
        let params = KrakenFuturesSendOrderParamsBuilder::default()
            .symbol("PI_XBTUSD")
            .side(KrakenOrderSide::Buy)
            .order_type(KrakenFuturesOrderType::Limit)
            .size("1000")
            .limit_price("50000.0")
            .cli_ord_id("test-order-123")
            .reduce_only(false)
            .build()
            .unwrap();

        assert_eq!(params.symbol, Ustr::from("PI_XBTUSD"));
        assert_eq!(params.side, KrakenOrderSide::Buy);
        assert_eq!(params.order_type, KrakenFuturesOrderType::Limit);
        assert_eq!(params.size, "1000");
        assert_eq!(params.limit_price, Some("50000.0".to_string()));
        assert_eq!(params.cli_ord_id, Some("test-order-123".to_string()));
    }

    #[rstest]
    fn test_send_order_params_serialization() {
        let params = KrakenFuturesSendOrderParamsBuilder::default()
            .symbol("PI_XBTUSD")
            .side(KrakenOrderSide::Buy)
            .order_type(KrakenFuturesOrderType::Ioc)
            .size("500")
            .limit_price("48000.0")
            .build()
            .unwrap();

        let json = serde_json::to_string(&params).unwrap();
        assert!(json.contains("\"orderType\":\"ioc\""));
        assert!(json.contains("\"limitPrice\":\"48000.0\""));
    }

    #[rstest]
    fn test_send_order_params_serialization_with_trigger_signal() {
        let params = KrakenFuturesSendOrderParamsBuilder::default()
            .symbol("PI_XBTUSD")
            .side(KrakenOrderSide::Buy)
            .order_type(KrakenFuturesOrderType::Stop)
            .size("500")
            .stop_price("47000.0")
            .trigger_signal(KrakenTriggerSignal::Mark)
            .build()
            .unwrap();

        let json = serde_json::to_string(&params).unwrap();
        assert!(json.contains("\"triggerSignal\":\"mark\""));
        assert!(json.contains("\"stopPrice\":\"47000.0\""));
    }

    #[rstest]
    fn test_send_order_params_serialization_with_index_trigger_signal() {
        let params = KrakenFuturesSendOrderParamsBuilder::default()
            .symbol("PI_XBTUSD")
            .side(KrakenOrderSide::Buy)
            .order_type(KrakenFuturesOrderType::Stop)
            .size("500")
            .stop_price("47000.0")
            .trigger_signal(KrakenTriggerSignal::Index)
            .build()
            .unwrap();

        let json = serde_json::to_string(&params).unwrap();
        assert!(json.contains("\"triggerSignal\":\"spot\""));
        assert!(json.contains("\"stopPrice\":\"47000.0\""));
    }

    #[rstest]
    fn test_send_order_params_missing_limit_price() {
        let result = KrakenFuturesSendOrderParamsBuilder::default()
            .symbol("PI_XBTUSD")
            .side(KrakenOrderSide::Buy)
            .order_type(KrakenFuturesOrderType::Limit)
            .size("1000")
            .build();

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("limit_price"));
    }

    #[rstest]
    fn test_cancel_order_params_builder() {
        let params = KrakenFuturesCancelOrderParamsBuilder::default()
            .order_id("abc-123")
            .build()
            .unwrap();

        assert_eq!(params.order_id, Some("abc-123".to_string()));
    }

    #[rstest]
    fn test_edit_order_params_builder() {
        let params = KrakenFuturesEditOrderParamsBuilder::default()
            .order_id("abc-123")
            .size("2000")
            .limit_price("51000.0")
            .build()
            .unwrap();

        assert_eq!(params.order_id, Some("abc-123".to_string()));
        assert_eq!(params.size, Some("2000".to_string()));
        assert_eq!(params.limit_price, Some("51000.0".to_string()));
    }

    #[rstest]
    fn test_batch_send_item_from_params() {
        let params = KrakenFuturesSendOrderParamsBuilder::default()
            .symbol("PI_XBTUSD")
            .side(KrakenOrderSide::Buy)
            .order_type(KrakenFuturesOrderType::Limit)
            .size("1000")
            .limit_price("50000.0")
            .cli_ord_id("test-batch-1")
            .build()
            .unwrap();

        let item = KrakenFuturesBatchSendItem::from_params(params, "0");

        assert_eq!(item.order, "send");
        assert_eq!(item.order_tag, "0");
        assert_eq!(item.symbol, Ustr::from("PI_XBTUSD"));
        assert_eq!(item.side, KrakenOrderSide::Buy);
        assert_eq!(item.order_type, KrakenFuturesOrderType::Limit);
        assert_eq!(item.size, "1000");
        assert_eq!(item.limit_price, Some("50000.0".to_string()));
        assert_eq!(item.cli_ord_id, Some("test-batch-1".to_string()));
    }

    #[rstest]
    fn test_batch_send_item_serialization() {
        let params = KrakenFuturesSendOrderParamsBuilder::default()
            .symbol("PI_XBTUSD")
            .side(KrakenOrderSide::Sell)
            .order_type(KrakenFuturesOrderType::Market)
            .size("500")
            .reduce_only(true)
            .build()
            .unwrap();

        let item = KrakenFuturesBatchSendItem::from_params(params, "1");
        let json = serde_json::to_string(&item).unwrap();

        assert!(json.contains("\"order\":\"send\""));
        assert!(json.contains("\"orderTag\":\"1\""));
        assert!(json.contains("\"orderType\":\"mkt\""));
        assert!(json.contains("\"reduceOnly\":true"));
    }

    #[rstest]
    fn test_batch_edit_item_from_params() {
        let params = KrakenFuturesEditOrderParamsBuilder::default()
            .order_id("order-123")
            .size("2000")
            .limit_price("51000.0")
            .build()
            .unwrap();

        let item = KrakenFuturesBatchEditItem::from_params(params, "0");

        assert_eq!(item.order, "edit");
        assert_eq!(item.order_tag, "0");
        assert_eq!(item.order_id, Some("order-123".to_string()));
        assert_eq!(item.size, Some("2000".to_string()));
        assert_eq!(item.limit_price, Some("51000.0".to_string()));
    }

    #[rstest]
    fn test_batch_edit_item_serialization() {
        let params = KrakenFuturesEditOrderParamsBuilder::default()
            .cli_ord_id("my-order")
            .limit_price("55000.0")
            .build()
            .unwrap();

        let item = KrakenFuturesBatchEditItem::from_params(params, "2");
        let json = serde_json::to_string(&item).unwrap();

        assert!(json.contains("\"order\":\"edit\""));
        assert!(json.contains("\"orderTag\":\"2\""));
        assert!(json.contains("\"cliOrdId\":\"my-order\""));
        assert!(json.contains("\"limitPrice\":\"55000.0\""));
    }

    #[rstest]
    fn test_batch_order_params_to_body() {
        let params = KrakenFuturesSendOrderParamsBuilder::default()
            .symbol("PI_XBTUSD")
            .side(KrakenOrderSide::Buy)
            .order_type(KrakenFuturesOrderType::Limit)
            .size("100")
            .limit_price("50000.0")
            .build()
            .unwrap();

        let item = KrakenFuturesBatchSendItem::from_params(params, "0");
        let batch = KrakenFuturesBatchOrderParams::new(vec![item]);
        let body = batch.to_body().unwrap();

        assert!(body.starts_with("json="));
        assert!(body.contains("\"batchOrder\""));
        assert!(body.contains("\"order\":\"send\""));
    }
}
