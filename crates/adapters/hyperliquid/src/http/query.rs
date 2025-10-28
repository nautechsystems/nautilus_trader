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

use crate::http::models::{
    HyperliquidExecCancelByCloidRequest, HyperliquidExecModifyOrderRequest,
    HyperliquidExecPlaceOrderRequest,
};

/// Exchange action types for Hyperliquid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ExchangeActionType {
    /// Place orders
    Order,
    /// Cancel orders by order ID
    Cancel,
    /// Cancel orders by client order ID
    CancelByCloid,
    /// Modify an existing order
    Modify,
    /// Update leverage for an asset
    UpdateLeverage,
    /// Update isolated margin for an asset
    UpdateIsolatedMargin,
}

impl AsRef<str> for ExchangeActionType {
    fn as_ref(&self) -> &str {
        match self {
            Self::Order => "order",
            Self::Cancel => "cancel",
            Self::CancelByCloid => "cancelByCloid",
            Self::Modify => "modify",
            Self::UpdateLeverage => "updateLeverage",
            Self::UpdateIsolatedMargin => "updateIsolatedMargin",
        }
    }
}

/// Parameters for placing orders.
#[derive(Debug, Clone, Serialize)]
pub struct OrderParams {
    pub orders: Vec<HyperliquidExecPlaceOrderRequest>,
    pub grouping: String,
}

/// Parameters for canceling orders.
#[derive(Debug, Clone, Serialize)]
pub struct CancelParams {
    pub cancels: Vec<HyperliquidExecCancelByCloidRequest>,
}

/// Parameters for modifying an order.
#[derive(Debug, Clone, Serialize)]
pub struct ModifyParams {
    pub oid: u64,
    pub order: HyperliquidExecModifyOrderRequest,
}

/// Parameters for updating leverage.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateLeverageParams {
    pub asset: u32,
    pub is_cross: bool,
    pub leverage: u32,
}

/// Parameters for updating isolated margin.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateIsolatedMarginParams {
    pub asset: u32,
    pub is_buy: bool,
    pub ntli: i64,
}

/// Parameters for L2 book request.
#[derive(Debug, Clone, Serialize)]
pub struct L2BookParams {
    pub coin: String,
}

/// Parameters for user fills request.
#[derive(Debug, Clone, Serialize)]
pub struct UserFillsParams {
    pub user: String,
}

/// Parameters for order status request.
#[derive(Debug, Clone, Serialize)]
pub struct OrderStatusParams {
    pub user: String,
    pub oid: u64,
}

/// Parameters for open orders request.
#[derive(Debug, Clone, Serialize)]
pub struct OpenOrdersParams {
    pub user: String,
}

/// Parameters for clearinghouse state request.
#[derive(Debug, Clone, Serialize)]
pub struct ClearinghouseStateParams {
    pub user: String,
}

/// Parameters for candle snapshot request.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CandleSnapshotReq {
    pub coin: String,
    pub interval: String,
    pub start_time: u64,
    pub end_time: u64,
}

/// Wrapper for candle snapshot parameters.
#[derive(Debug, Clone, Serialize)]
pub struct CandleSnapshotParams {
    pub req: CandleSnapshotReq,
}

/// Info request parameters.
#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum InfoRequestParams {
    L2Book(L2BookParams),
    UserFills(UserFillsParams),
    OrderStatus(OrderStatusParams),
    OpenOrders(OpenOrdersParams),
    ClearinghouseState(ClearinghouseStateParams),
    CandleSnapshot(CandleSnapshotParams),
    None,
}

/// Represents an info request wrapper for `POST /info`.
#[derive(Debug, Clone, Serialize)]
pub struct InfoRequest {
    #[serde(rename = "type")]
    pub request_type: String,
    #[serde(flatten)]
    pub params: InfoRequestParams,
}

impl InfoRequest {
    /// Creates a request to get metadata about available markets.
    pub fn meta() -> Self {
        Self {
            request_type: "meta".to_string(),
            params: InfoRequestParams::None,
        }
    }

    /// Creates a request to get spot metadata (tokens and pairs).
    pub fn spot_meta() -> Self {
        Self {
            request_type: "spotMeta".to_string(),
            params: InfoRequestParams::None,
        }
    }

    /// Creates a request to get metadata with asset contexts (for price precision).
    pub fn meta_and_asset_ctxs() -> Self {
        Self {
            request_type: "metaAndAssetCtxs".to_string(),
            params: InfoRequestParams::None,
        }
    }

    /// Creates a request to get spot metadata with asset contexts.
    pub fn spot_meta_and_asset_ctxs() -> Self {
        Self {
            request_type: "spotMetaAndAssetCtxs".to_string(),
            params: InfoRequestParams::None,
        }
    }

    /// Creates a request to get L2 order book for a coin.
    pub fn l2_book(coin: &str) -> Self {
        Self {
            request_type: "l2Book".to_string(),
            params: InfoRequestParams::L2Book(L2BookParams {
                coin: coin.to_string(),
            }),
        }
    }

    /// Creates a request to get user fills.
    pub fn user_fills(user: &str) -> Self {
        Self {
            request_type: "userFills".to_string(),
            params: InfoRequestParams::UserFills(UserFillsParams {
                user: user.to_string(),
            }),
        }
    }

    /// Creates a request to get order status for a user.
    pub fn order_status(user: &str, oid: u64) -> Self {
        Self {
            request_type: "orderStatus".to_string(),
            params: InfoRequestParams::OrderStatus(OrderStatusParams {
                user: user.to_string(),
                oid,
            }),
        }
    }

    /// Creates a request to get all open orders for a user.
    pub fn open_orders(user: &str) -> Self {
        Self {
            request_type: "openOrders".to_string(),
            params: InfoRequestParams::OpenOrders(OpenOrdersParams {
                user: user.to_string(),
            }),
        }
    }

    /// Creates a request to get frontend open orders (includes more detail).
    pub fn frontend_open_orders(user: &str) -> Self {
        Self {
            request_type: "frontendOpenOrders".to_string(),
            params: InfoRequestParams::OpenOrders(OpenOrdersParams {
                user: user.to_string(),
            }),
        }
    }

    /// Creates a request to get user state (balances, positions, margin).
    pub fn clearinghouse_state(user: &str) -> Self {
        Self {
            request_type: "clearinghouseState".to_string(),
            params: InfoRequestParams::ClearinghouseState(ClearinghouseStateParams {
                user: user.to_string(),
            }),
        }
    }

    /// Creates a request to get candle/bar data.
    ///
    /// # Arguments
    /// * `coin` - The coin symbol (e.g., "BTC")
    /// * `interval` - The timeframe (e.g., "1m", "5m", "15m", "1h", "4h", "1d")
    /// * `start_time` - Start timestamp in milliseconds
    /// * `end_time` - End timestamp in milliseconds
    pub fn candle_snapshot(coin: &str, interval: &str, start_time: u64, end_time: u64) -> Self {
        Self {
            request_type: "candleSnapshot".to_string(),
            params: InfoRequestParams::CandleSnapshot(CandleSnapshotParams {
                req: CandleSnapshotReq {
                    coin: coin.to_string(),
                    interval: interval.to_string(),
                    start_time,
                    end_time,
                },
            }),
        }
    }
}

/// Exchange action parameters.
#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum ExchangeActionParams {
    Order(OrderParams),
    Cancel(CancelParams),
    Modify(ModifyParams),
    UpdateLeverage(UpdateLeverageParams),
    UpdateIsolatedMargin(UpdateIsolatedMarginParams),
}

/// Represents an exchange action wrapper for `POST /exchange`.
#[derive(Debug, Clone, Serialize)]
pub struct ExchangeAction {
    #[serde(rename = "type", serialize_with = "serialize_action_type")]
    pub action_type: ExchangeActionType,
    #[serde(flatten)]
    pub params: ExchangeActionParams,
}

fn serialize_action_type<S>(
    action_type: &ExchangeActionType,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    serializer.serialize_str(action_type.as_ref())
}

impl ExchangeAction {
    /// Creates an action to place orders.
    pub fn order(orders: Vec<HyperliquidExecPlaceOrderRequest>) -> Self {
        Self {
            action_type: ExchangeActionType::Order,
            params: ExchangeActionParams::Order(OrderParams {
                orders,
                grouping: "na".to_string(),
            }),
        }
    }

    /// Creates an action to cancel orders.
    pub fn cancel(cancels: Vec<HyperliquidExecCancelByCloidRequest>) -> Self {
        Self {
            action_type: ExchangeActionType::Cancel,
            params: ExchangeActionParams::Cancel(CancelParams { cancels }),
        }
    }

    /// Creates an action to cancel orders by client order ID.
    pub fn cancel_by_cloid(cancels: Vec<HyperliquidExecCancelByCloidRequest>) -> Self {
        Self {
            action_type: ExchangeActionType::CancelByCloid,
            params: ExchangeActionParams::Cancel(CancelParams { cancels }),
        }
    }

    /// Creates an action to modify an order.
    pub fn modify(oid: u64, order: HyperliquidExecModifyOrderRequest) -> Self {
        Self {
            action_type: ExchangeActionType::Modify,
            params: ExchangeActionParams::Modify(ModifyParams { oid, order }),
        }
    }

    /// Creates an action to update leverage for an asset.
    pub fn update_leverage(asset: u32, is_cross: bool, leverage: u32) -> Self {
        Self {
            action_type: ExchangeActionType::UpdateLeverage,
            params: ExchangeActionParams::UpdateLeverage(UpdateLeverageParams {
                asset,
                is_cross,
                leverage,
            }),
        }
    }

    /// Creates an action to update isolated margin for an asset.
    pub fn update_isolated_margin(asset: u32, is_buy: bool, ntli: i64) -> Self {
        Self {
            action_type: ExchangeActionType::UpdateIsolatedMargin,
            params: ExchangeActionParams::UpdateIsolatedMargin(UpdateIsolatedMarginParams {
                asset,
                is_buy,
                ntli,
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
        assert!(matches!(req.params, InfoRequestParams::None));
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
        use rust_decimal::Decimal;

        use crate::http::models::{
            HyperliquidExecLimitParams, HyperliquidExecOrderKind, HyperliquidExecPlaceOrderRequest,
            HyperliquidExecTif,
        };

        let order = HyperliquidExecPlaceOrderRequest {
            asset: 0,
            is_buy: true,
            price: Decimal::new(50000, 0),
            size: Decimal::new(1, 0),
            reduce_only: false,
            kind: HyperliquidExecOrderKind::Limit {
                limit: HyperliquidExecLimitParams {
                    tif: HyperliquidExecTif::Gtc,
                },
            },
            cloid: None,
        };

        let action = ExchangeAction::order(vec![order]);

        assert_eq!(action.action_type, ExchangeActionType::Order);
        let json = serde_json::to_string(&action).unwrap();
        assert!(json.contains("\"orders\""));
    }

    #[rstest]
    fn test_exchange_action_cancel() {
        use crate::http::models::HyperliquidExecCancelByCloidRequest;

        let cancel = HyperliquidExecCancelByCloidRequest {
            asset: 0,
            cloid: crate::http::models::Cloid::from_hex("0x00000000000000000000000000000000")
                .unwrap(),
        };

        let action = ExchangeAction::cancel(vec![cancel]);

        assert_eq!(action.action_type, ExchangeActionType::Cancel);
    }

    #[rstest]
    fn test_exchange_action_serialization() {
        use rust_decimal::Decimal;

        use crate::http::models::{
            HyperliquidExecLimitParams, HyperliquidExecOrderKind, HyperliquidExecPlaceOrderRequest,
            HyperliquidExecTif,
        };

        let order = HyperliquidExecPlaceOrderRequest {
            asset: 0,
            is_buy: true,
            price: Decimal::new(50000, 0),
            size: Decimal::new(1, 0),
            reduce_only: false,
            kind: HyperliquidExecOrderKind::Limit {
                limit: HyperliquidExecLimitParams {
                    tif: HyperliquidExecTif::Gtc,
                },
            },
            cloid: None,
        };

        let action = ExchangeAction::order(vec![order]);

        let json = serde_json::to_string(&action).unwrap();
        // Verify that action_type is serialized as "type" with the correct string value
        assert!(json.contains(r#""type":"order""#));
        assert!(json.contains(r#""orders""#));
        assert!(json.contains(r#""grouping":"na""#));
    }

    #[rstest]
    fn test_exchange_action_type_as_ref() {
        assert_eq!(ExchangeActionType::Order.as_ref(), "order");
        assert_eq!(ExchangeActionType::Cancel.as_ref(), "cancel");
        assert_eq!(ExchangeActionType::CancelByCloid.as_ref(), "cancelByCloid");
        assert_eq!(ExchangeActionType::Modify.as_ref(), "modify");
        assert_eq!(
            ExchangeActionType::UpdateLeverage.as_ref(),
            "updateLeverage"
        );
        assert_eq!(
            ExchangeActionType::UpdateIsolatedMargin.as_ref(),
            "updateIsolatedMargin"
        );
    }

    #[rstest]
    fn test_update_leverage_serialization() {
        let action = ExchangeAction::update_leverage(1, true, 10);
        let json = serde_json::to_string(&action).unwrap();

        assert!(json.contains(r#""type":"updateLeverage""#));
        assert!(json.contains(r#""asset":1"#));
        assert!(json.contains(r#""isCross":true"#));
        assert!(json.contains(r#""leverage":10"#));
    }

    #[rstest]
    fn test_update_isolated_margin_serialization() {
        let action = ExchangeAction::update_isolated_margin(2, false, 1000);
        let json = serde_json::to_string(&action).unwrap();

        assert!(json.contains(r#""type":"updateIsolatedMargin""#));
        assert!(json.contains(r#""asset":2"#));
        assert!(json.contains(r#""isBuy":false"#));
        assert!(json.contains(r#""ntli":1000"#));
    }

    #[rstest]
    fn test_cancel_by_cloid_serialization() {
        use crate::http::models::{Cloid, HyperliquidExecCancelByCloidRequest};

        let cancel_request = HyperliquidExecCancelByCloidRequest {
            asset: 0,
            cloid: Cloid::from_hex("0x00000000000000000000000000000000").unwrap(),
        };
        let action = ExchangeAction::cancel_by_cloid(vec![cancel_request]);
        let json = serde_json::to_string(&action).unwrap();

        assert!(json.contains(r#""type":"cancelByCloid""#));
        assert!(json.contains(r#""cancels""#));
    }

    #[rstest]
    fn test_modify_serialization() {
        use rust_decimal::Decimal;

        use crate::http::models::HyperliquidExecModifyOrderRequest;

        let modify_request = HyperliquidExecModifyOrderRequest {
            asset: 0,
            oid: 12345,
            price: Some(Decimal::new(51000, 0)),
            size: Some(Decimal::new(2, 0)),
            reduce_only: None,
            kind: None,
        };
        let action = ExchangeAction::modify(12345, modify_request);
        let json = serde_json::to_string(&action).unwrap();

        assert!(json.contains(r#""type":"modify""#));
        assert!(json.contains(r#""oid":12345"#));
    }
}
