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

use serde::Serialize;
use serde_json::Value;

/// Represents an info request wrapper for `POST /info`.
#[derive(Debug, Clone, Serialize)]
pub struct InfoRequest {
    #[serde(rename = "type")]
    pub request_type: String,
    #[serde(flatten)]
    pub params: Value,
}

impl InfoRequest {
    /// Creates a request to get metadata about available markets.
    pub fn meta() -> Self {
        Self {
            request_type: "meta".to_string(),
            params: Value::Null,
        }
    }

    /// Creates a request to get L2 order book for a coin.
    pub fn l2_book(coin: &str) -> Self {
        Self {
            request_type: "l2Book".to_string(),
            params: serde_json::json!({ "coin": coin }),
        }
    }

    /// Creates a request to get user fills.
    pub fn user_fills(user: &str) -> Self {
        Self {
            request_type: "userFills".to_string(),
            params: serde_json::json!({ "user": user }),
        }
    }

    /// Creates a request to get order status for a user.
    pub fn order_status(user: &str, oid: u64) -> Self {
        Self {
            request_type: "orderStatus".to_string(),
            params: serde_json::json!({ "user": user, "oid": oid }),
        }
    }
}

/// Represents an exchange action wrapper for `POST /exchange`.
#[derive(Debug, Clone, Serialize)]
pub struct ExchangeAction {
    #[serde(rename = "type")]
    pub action_type: String,
    #[serde(flatten)]
    pub params: Value,
}

impl ExchangeAction {
    /// Creates an action to place orders.
    pub fn order(orders: Value) -> Self {
        Self {
            action_type: "order".to_string(),
            params: serde_json::json!({ "orders": orders }),
        }
    }

    /// Creates an action to cancel orders.
    pub fn cancel(cancels: Value) -> Self {
        Self {
            action_type: "cancel".to_string(),
            params: serde_json::json!({ "cancels": cancels }),
        }
    }

    /// Creates an action to cancel orders by client order ID.
    pub fn cancel_by_cloid(cancels: Value) -> Self {
        Self {
            action_type: "cancelByCloid".to_string(),
            params: serde_json::json!({ "cancels": cancels }),
        }
    }

    /// Creates an action to modify an order.
    pub fn modify(oid: u64, order: Value) -> Self {
        Self {
            action_type: "modify".to_string(),
            params: serde_json::json!({ "oid": oid, "order": order }),
        }
    }

    /// Creates an action to update leverage for an asset.
    pub fn update_leverage(asset: u32, is_cross: bool, leverage: u32) -> Self {
        Self {
            action_type: "updateLeverage".to_string(),
            params: serde_json::json!({
                "asset": asset,
                "isCross": is_cross,
                "leverage": leverage
            }),
        }
    }

    /// Creates an action to update isolated margin for an asset.
    pub fn update_isolated_margin(asset: u32, is_buy: bool, ntli: i64) -> Self {
        Self {
            action_type: "updateIsolatedMargin".to_string(),
            params: serde_json::json!({
                "asset": asset,
                "isBuy": is_buy,
                "ntli": ntli
            }),
        }
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_info_request_meta() {
        let req = InfoRequest::meta();

        assert_eq!(req.request_type, "meta");
        assert_eq!(req.params, Value::Null);
    }

    #[rstest]
    fn test_info_request_l2_book() {
        let req = InfoRequest::l2_book("BTC");

        assert_eq!(req.request_type, "l2Book");
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("\"coin\":\"BTC\""));
    }

    #[rstest]
    fn test_exchange_action_order() {
        let orders =
            serde_json::json!([{"asset": 0, "isBuy": true, "sz": "1.0", "limitPx": "50000"}]);

        let action = ExchangeAction::order(orders);

        assert_eq!(action.action_type, "order");
        let json = serde_json::to_string(&action).unwrap();
        assert!(json.contains("\"orders\""));
    }

    #[rstest]
    fn test_exchange_action_cancel() {
        let cancels = serde_json::json!([{"asset": 0, "oid": 123}]);

        let action = ExchangeAction::cancel(cancels);

        assert_eq!(action.action_type, "cancel");
    }
}
