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

//! Query parameter structs for Kraken Spot HTTP API requests.

use derive_builder::Builder;
use serde::{Deserialize, Serialize};
use ustr::Ustr;

use crate::common::enums::{KrakenOrderSide, KrakenOrderType};

/// Parameters for adding an order via `POST /0/private/AddOrder`.
///
/// # References
/// - <https://docs.kraken.com/api/docs/rest-api/add-order>
#[derive(Clone, Debug, Serialize, Deserialize, Builder)]
#[builder(setter(into, strip_option), build_fn(validate = "Self::validate"))]
pub struct KrakenSpotAddOrderParams {
    /// Asset pair (e.g., "XXBTZUSD").
    pub pair: Ustr,

    /// Order side: "buy" or "sell".
    #[serde(rename = "type")]
    pub side: KrakenOrderSide,

    /// Order type: market, limit, stop-loss, etc.
    #[serde(rename = "ordertype")]
    pub order_type: KrakenOrderType,

    /// Order quantity in base currency.
    pub volume: String,

    /// Limit price (required for limit orders).
    #[builder(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub price: Option<String>,

    /// Secondary price for stop-loss-limit and take-profit-limit.
    #[builder(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub price2: Option<String>,

    /// Client order ID (must be UUID format).
    #[builder(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cl_ord_id: Option<String>,

    /// Order flags (comma-separated: post, fcib, fciq, nompp, viqc).
    #[builder(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oflags: Option<String>,

    /// Time in force: GTC, IOC, GTD.
    #[builder(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeinforce: Option<String>,

    /// Expiration time for GTD orders (Unix timestamp or `+<seconds>`).
    #[builder(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expiretm: Option<String>,

    /// Partner/broker attribution ID.
    #[builder(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub broker: Option<Ustr>,
}

impl KrakenSpotAddOrderParamsBuilder {
    fn validate(&self) -> Result<(), String> {
        // Validate price is present for limit-type orders
        if let Some(
            KrakenOrderType::Limit
            | KrakenOrderType::StopLossLimit
            | KrakenOrderType::TakeProfitLimit,
        ) = self.order_type
            && (self.price.is_none() || self.price.as_ref().unwrap().is_none())
        {
            return Err("price is required for limit orders".to_string());
        }
        Ok(())
    }
}

/// Parameters for cancelling an order via `POST /0/private/CancelOrder`.
///
/// # References
/// - <https://docs.kraken.com/api/docs/rest-api/cancel-order>
#[derive(Clone, Debug, Serialize, Deserialize, Builder)]
#[builder(setter(into, strip_option))]
pub struct KrakenSpotCancelOrderParams {
    /// Transaction ID (venue order ID) to cancel.
    /// Note: The Kraken v0 API uses `txid` as the parameter name.
    #[builder(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub txid: Option<String>,

    /// Client order ID to cancel.
    #[builder(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cl_ord_id: Option<String>,
}

/// Parameters for batch cancelling orders via `POST /0/private/CancelOrderBatch`.
///
/// # References
/// - <https://docs.kraken.com/api/docs/rest-api/cancel-order-batch>
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct KrakenSpotCancelOrderBatchParams {
    /// List of transaction IDs (venue order IDs) or client order IDs to cancel.
    /// Maximum 50 IDs.
    pub orders: Vec<String>,
}

/// Parameters for editing an order via `POST /0/private/EditOrder`.
///
/// Note: Consider using `KrakenSpotAmendOrderParams` with `AmendOrder` instead,
/// which is faster and keeps queue priority.
///
/// # References
/// - <https://docs.kraken.com/api/docs/rest-api/edit-order>
#[derive(Clone, Debug, Serialize, Deserialize, Builder)]
#[builder(setter(into, strip_option))]
pub struct KrakenSpotEditOrderParams {
    /// Asset pair (e.g., "XXBTZUSD"). Required.
    pub pair: Ustr,

    /// Transaction ID (venue order ID) of the order to edit.
    #[builder(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub txid: Option<String>,

    /// Client order ID of the order to edit. Note: Not supported by Kraken EditOrder.
    #[builder(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cl_ord_id: Option<String>,

    /// New order quantity in base currency.
    #[builder(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub volume: Option<String>,

    /// New limit price.
    #[builder(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub price: Option<String>,

    /// New secondary price for stop-loss-limit and take-profit-limit.
    #[builder(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub price2: Option<String>,
}

/// Parameters for amending an order via `POST /0/private/AmendOrder`.
///
/// This is Kraken's atomic amend endpoint which modifies order parameters
/// in-place without cancelling the original order. Faster and keeps queue priority.
///
/// # References
/// - <https://docs.kraken.com/api/docs/rest-api/amend-order>
#[derive(Clone, Debug, Serialize, Deserialize, Builder)]
#[builder(setter(into, strip_option))]
pub struct KrakenSpotAmendOrderParams {
    /// Transaction ID (venue order ID) of the order to amend.
    #[builder(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub txid: Option<String>,

    /// Client order ID of the order to amend.
    #[builder(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cl_ord_id: Option<String>,

    /// New order quantity in base currency.
    #[builder(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub order_qty: Option<String>,

    /// New limit price.
    #[builder(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit_price: Option<String>,

    /// New trigger price for stop/conditional orders.
    #[builder(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trigger_price: Option<String>,
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_add_order_params_builder() {
        let params = KrakenSpotAddOrderParamsBuilder::default()
            .pair("XXBTZUSD")
            .side(KrakenOrderSide::Buy)
            .order_type(KrakenOrderType::Limit)
            .volume("0.01")
            .price("50000.0")
            .cl_ord_id("my-order-123")
            .broker("test-broker")
            .build()
            .unwrap();

        assert_eq!(params.pair, Ustr::from("XXBTZUSD"));
        assert_eq!(params.side, KrakenOrderSide::Buy);
        assert_eq!(params.order_type, KrakenOrderType::Limit);
        assert_eq!(params.volume, "0.01");
        assert_eq!(params.price, Some("50000.0".to_string()));
        assert_eq!(params.cl_ord_id, Some("my-order-123".to_string()));
        assert_eq!(params.broker, Some(Ustr::from("test-broker")));
    }

    #[rstest]
    fn test_add_order_params_serialization() {
        let params = KrakenSpotAddOrderParamsBuilder::default()
            .pair("XXBTZUSD")
            .side(KrakenOrderSide::Buy)
            .order_type(KrakenOrderType::Market)
            .volume("0.01")
            .broker("broker-id")
            .build()
            .unwrap();

        let encoded = serde_urlencoded::to_string(&params).unwrap();

        assert!(encoded.contains("pair=XXBTZUSD"));
        assert!(encoded.contains("type=buy"));
        assert!(encoded.contains("ordertype=market"));
        assert!(encoded.contains("volume=0.01"));
        assert!(encoded.contains("broker=broker-id"));
        assert!(!encoded.contains("price="));
    }

    #[rstest]
    fn test_add_order_params_limit_requires_price() {
        let result = KrakenSpotAddOrderParamsBuilder::default()
            .pair("XXBTZUSD")
            .side(KrakenOrderSide::Buy)
            .order_type(KrakenOrderType::Limit)
            .volume("0.01")
            .build();

        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("price is required")
        );
    }

    #[rstest]
    fn test_cancel_order_params_builder() {
        let params = KrakenSpotCancelOrderParamsBuilder::default()
            .txid("TXID123")
            .build()
            .unwrap();

        assert_eq!(params.txid, Some("TXID123".to_string()));
        assert_eq!(params.cl_ord_id, None);
    }

    #[rstest]
    fn test_cancel_order_params_serialization() {
        let params = KrakenSpotCancelOrderParamsBuilder::default()
            .cl_ord_id("my-order")
            .build()
            .unwrap();

        let encoded = serde_urlencoded::to_string(&params).unwrap();

        assert!(encoded.contains("cl_ord_id=my-order"));
        assert!(!encoded.contains("txid="));
    }
}
