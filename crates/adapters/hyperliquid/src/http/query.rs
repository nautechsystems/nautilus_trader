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

use hypersdk::hypercore::CandleSnapshotRequest;
use serde::{Serialize, Serializer};

use crate::common::enums::{HyperliquidBarInterval, HyperliquidInfoRequestType};

/// Parameters for updating isolated margin.
#[derive(Debug, Clone, Serialize)]
#[serde(
    tag = "type",
    rename = "updateIsolatedMargin",
    rename_all = "camelCase"
)]
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

/// Parameters for recent trades request.
#[derive(Debug, Clone, Serialize)]
pub struct RecentTradesParams {
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dex: Option<String>,
}

/// Parameters for clearinghouse state request.
#[derive(Debug, Clone, Serialize)]
pub struct ClearinghouseStateParams {
    pub user: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dex: Option<String>,
}

/// Parameters for spot clearinghouse state request.
#[derive(Debug, Clone, Serialize)]
pub struct SpotClearinghouseStateParams {
    pub user: String,
}

/// Parameters for candle snapshot request.
#[derive(Debug, Clone)]
pub struct CandleSnapshotReq {
    pub coin: String,
    pub interval: HyperliquidBarInterval,
    pub start_time: u64,
    pub end_time: u64,
}

impl Serialize for CandleSnapshotReq {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        CandleSnapshotRequest {
            coin: self.coin.clone(),
            interval: self
                .interval
                .as_str()
                .parse()
                .map_err(serde::ser::Error::custom)?,
            start_time: self.start_time,
            end_time: self.end_time,
        }
        .serialize(serializer)
    }
}

/// Wrapper for candle snapshot parameters.
#[derive(Debug, Clone, Serialize)]
pub struct CandleSnapshotParams {
    pub req: CandleSnapshotReq,
}

/// Parameters for funding history request.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FundingHistoryParams {
    pub coin: String,
    pub start_time: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_time: Option<u64>,
}

/// Info request parameters.
#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum InfoRequestParams {
    L2Book(L2BookParams),
    RecentTrades(RecentTradesParams),
    UserFills(UserFillsParams),
    OrderStatus(OrderStatusParams),
    OpenOrders(OpenOrdersParams),
    ClearinghouseState(ClearinghouseStateParams),
    SpotClearinghouseState(SpotClearinghouseStateParams),
    CandleSnapshot(CandleSnapshotParams),
    FundingHistory(FundingHistoryParams),
    None,
}

/// Represents an info request wrapper for `POST /info`.
#[derive(Debug, Clone, Serialize)]
pub struct InfoRequest {
    #[serde(rename = "type")]
    pub request_type: HyperliquidInfoRequestType,
    #[serde(flatten)]
    pub params: InfoRequestParams,
}

impl InfoRequest {
    /// Creates a request to get metadata about available markets.
    pub fn meta() -> Self {
        Self {
            request_type: HyperliquidInfoRequestType::Meta,
            params: InfoRequestParams::None,
        }
    }

    /// Creates a request to get metadata for all perp dexes (standard + HIP-3).
    pub fn all_perp_metas() -> Self {
        Self {
            request_type: HyperliquidInfoRequestType::AllPerpMetas,
            params: InfoRequestParams::None,
        }
    }

    /// Creates a request to get the list of perp dexes.
    pub fn perp_dexs() -> Self {
        Self {
            request_type: HyperliquidInfoRequestType::PerpDexs,
            params: InfoRequestParams::None,
        }
    }

    /// Creates a request to get spot metadata (tokens and pairs).
    pub fn spot_meta() -> Self {
        Self {
            request_type: HyperliquidInfoRequestType::SpotMeta,
            params: InfoRequestParams::None,
        }
    }

    /// Creates a request to get metadata with asset contexts (for price precision).
    pub fn meta_and_asset_ctxs() -> Self {
        Self {
            request_type: HyperliquidInfoRequestType::MetaAndAssetCtxs,
            params: InfoRequestParams::None,
        }
    }

    /// Creates a request to get spot metadata with asset contexts.
    pub fn spot_meta_and_asset_ctxs() -> Self {
        Self {
            request_type: HyperliquidInfoRequestType::SpotMetaAndAssetCtxs,
            params: InfoRequestParams::None,
        }
    }

    /// Creates a request to get outcome metadata.
    pub fn outcome_meta() -> Self {
        Self {
            request_type: HyperliquidInfoRequestType::OutcomeMeta,
            params: InfoRequestParams::None,
        }
    }

    /// Creates a request to get L2 order book for a coin.
    pub fn l2_book(coin: &str) -> Self {
        Self {
            request_type: HyperliquidInfoRequestType::L2Book,
            params: InfoRequestParams::L2Book(L2BookParams {
                coin: coin.to_string(),
            }),
        }
    }

    /// Creates a request to get recent public trades for a coin.
    pub fn recent_trades(coin: &str) -> Self {
        Self {
            request_type: HyperliquidInfoRequestType::RecentTrades,
            params: InfoRequestParams::RecentTrades(RecentTradesParams {
                coin: coin.to_string(),
            }),
        }
    }

    /// Creates a request to get user fills.
    pub fn user_fills(user: &str) -> Self {
        Self {
            request_type: HyperliquidInfoRequestType::UserFills,
            params: InfoRequestParams::UserFills(UserFillsParams {
                user: user.to_string(),
            }),
        }
    }

    /// Creates a request to get order status for a user.
    pub fn order_status(user: &str, oid: u64) -> Self {
        Self {
            request_type: HyperliquidInfoRequestType::OrderStatus,
            params: InfoRequestParams::OrderStatus(OrderStatusParams {
                user: user.to_string(),
                oid,
            }),
        }
    }

    /// Creates a request to get all open orders for a user.
    pub fn open_orders(user: &str) -> Self {
        Self::open_orders_for_dex(user, None)
    }

    /// Creates a request to get all open orders for a user on a specific perp dex.
    pub(crate) fn open_orders_for_dex(user: &str, dex: Option<&str>) -> Self {
        Self {
            request_type: HyperliquidInfoRequestType::OpenOrders,
            params: InfoRequestParams::OpenOrders(OpenOrdersParams {
                user: user.to_string(),
                dex: dex.map(str::to_string),
            }),
        }
    }

    /// Creates a request to get frontend open orders (includes more detail).
    pub fn frontend_open_orders(user: &str) -> Self {
        Self::frontend_open_orders_for_dex(user, None)
    }

    /// Creates a frontend open-orders request for a user on a specific perp dex.
    pub(crate) fn frontend_open_orders_for_dex(user: &str, dex: Option<&str>) -> Self {
        Self {
            request_type: HyperliquidInfoRequestType::FrontendOpenOrders,
            params: InfoRequestParams::OpenOrders(OpenOrdersParams {
                user: user.to_string(),
                dex: dex.map(str::to_string),
            }),
        }
    }

    /// Creates a request to get historical orders for a user.
    pub fn historical_orders(user: &str) -> Self {
        Self {
            request_type: HyperliquidInfoRequestType::HistoricalOrders,
            params: InfoRequestParams::OpenOrders(OpenOrdersParams {
                user: user.to_string(),
                dex: None,
            }),
        }
    }

    /// Creates a request to get user state (balances, positions, margin).
    pub fn clearinghouse_state(user: &str) -> Self {
        Self::clearinghouse_state_for_dex(user, None)
    }

    /// Creates a clearinghouse-state request for a user on a specific perp dex.
    pub(crate) fn clearinghouse_state_for_dex(user: &str, dex: Option<&str>) -> Self {
        Self {
            request_type: HyperliquidInfoRequestType::ClearinghouseState,
            params: InfoRequestParams::ClearinghouseState(ClearinghouseStateParams {
                user: user.to_string(),
                dex: dex.map(str::to_string),
            }),
        }
    }

    /// Creates a request to get spot clearinghouse state (per-token spot balances).
    pub fn spot_clearinghouse_state(user: &str) -> Self {
        Self {
            request_type: HyperliquidInfoRequestType::SpotClearinghouseState,
            params: InfoRequestParams::SpotClearinghouseState(SpotClearinghouseStateParams {
                user: user.to_string(),
            }),
        }
    }

    /// Creates a request to get the account abstraction mode for a user.
    pub fn user_abstraction(user: &str) -> Self {
        Self {
            request_type: HyperliquidInfoRequestType::UserAbstraction,
            params: InfoRequestParams::SpotClearinghouseState(SpotClearinghouseStateParams {
                user: user.to_string(),
            }),
        }
    }

    /// Creates a request to get user fee schedule and effective rates.
    pub fn user_fees(user: &str) -> Self {
        Self {
            request_type: HyperliquidInfoRequestType::UserFees,
            params: InfoRequestParams::OpenOrders(OpenOrdersParams {
                user: user.to_string(),
                dex: None,
            }),
        }
    }

    /// Creates a request to get candle/bar data.
    pub fn candle_snapshot(
        coin: &str,
        interval: HyperliquidBarInterval,
        start_time: u64,
        end_time: u64,
    ) -> Self {
        Self {
            request_type: HyperliquidInfoRequestType::CandleSnapshot,
            params: InfoRequestParams::CandleSnapshot(CandleSnapshotParams {
                req: CandleSnapshotReq {
                    coin: coin.to_string(),
                    interval,
                    start_time,
                    end_time,
                },
            }),
        }
    }

    /// Creates a request to get funding rate history for a coin.
    pub fn funding_history(coin: &str, start_time: u64, end_time: Option<u64>) -> Self {
        Self {
            request_type: HyperliquidInfoRequestType::FundingHistory,
            params: InfoRequestParams::FundingHistory(FundingHistoryParams {
                coin: coin.to_string(),
                start_time,
                end_time,
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use hypersdk::hypercore::{
        BatchCancel, BatchCancelCloid, BatchOrder, Cancel, CancelByCloid, Modify, OidOrCloid,
        OrderGrouping, OrderRequest, OrderTypePlacement, TimeInForce as SdkTimeInForce,
        api::{Action, ModifyAction, UpdateLeverage},
    };
    use rstest::rstest;
    use rust_decimal::Decimal;

    use super::*;
    use crate::http::models::Cloid;

    #[rstest]
    fn test_info_request_meta() {
        let req = InfoRequest::meta();

        assert_eq!(req.request_type, HyperliquidInfoRequestType::Meta);
        assert!(matches!(req.params, InfoRequestParams::None));
    }

    #[rstest]
    fn test_sdk_candle_request_preserves_interval_and_time_bounds(
        #[values(
            "1m", "3m", "5m", "15m", "30m", "1h", "2h", "4h", "8h", "12h", "1d", "3d", "1w", "1M"
        )]
        interval: &str,
    ) {
        let request = InfoRequest::candle_snapshot(
            "testdex:BTC",
            interval.parse().unwrap(),
            1_700_000_000_000,
            1_700_000_059_999,
        );
        assert_eq!(
            serde_json::to_value(request).unwrap(),
            serde_json::json!({
                "type": "candleSnapshot",
                "req": {
                    "coin": "testdex:BTC", "interval": interval,
                    "startTime": 1_700_000_000_000_u64, "endTime": 1_700_000_059_999_u64,
                },
            })
        );
    }

    #[rstest]
    fn test_info_request_all_perp_metas() {
        let req = InfoRequest::all_perp_metas();

        assert_eq!(req.request_type, HyperliquidInfoRequestType::AllPerpMetas);
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains(r#""type":"allPerpMetas""#));
    }

    #[rstest]
    fn test_info_request_outcome_meta() {
        let req = InfoRequest::outcome_meta();

        assert_eq!(req.request_type, HyperliquidInfoRequestType::OutcomeMeta);
        assert!(matches!(req.params, InfoRequestParams::None));
        let json = serde_json::to_string(&req).unwrap();
        assert_eq!(json, r#"{"type":"outcomeMeta"}"#);
    }

    #[rstest]
    fn test_info_request_l2_book() {
        let req = InfoRequest::l2_book("BTC");

        assert_eq!(req.request_type, HyperliquidInfoRequestType::L2Book);
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("\"coin\":\"BTC\""));
    }

    #[rstest]
    fn test_info_request_recent_trades() {
        let req = InfoRequest::recent_trades("BTC");

        assert_eq!(req.request_type, HyperliquidInfoRequestType::RecentTrades);
        let json = serde_json::to_string(&req).unwrap();
        assert_eq!(json, r#"{"type":"recentTrades","coin":"BTC"}"#);
    }

    #[rstest]
    fn test_info_request_open_orders_dex_serialization() {
        let default = serde_json::to_value(InfoRequest::open_orders("0xabc")).unwrap();
        let xyz =
            serde_json::to_value(InfoRequest::open_orders_for_dex("0xabc", Some("xyz"))).unwrap();

        assert_eq!(
            default,
            serde_json::json!({"type": "openOrders", "user": "0xabc"})
        );
        assert_eq!(
            xyz,
            serde_json::json!({"type": "openOrders", "user": "0xabc", "dex": "xyz"})
        );
    }

    #[rstest]
    fn test_info_request_frontend_open_orders_dex_serialization() {
        let default = serde_json::to_value(InfoRequest::frontend_open_orders("0xabc")).unwrap();
        let xyz = serde_json::to_value(InfoRequest::frontend_open_orders_for_dex(
            "0xabc",
            Some("xyz"),
        ))
        .unwrap();

        assert_eq!(
            default,
            serde_json::json!({"type": "frontendOpenOrders", "user": "0xabc"})
        );
        assert_eq!(
            xyz,
            serde_json::json!({"type": "frontendOpenOrders", "user": "0xabc", "dex": "xyz"})
        );
    }

    #[rstest]
    fn test_info_request_clearinghouse_state_dex_serialization() {
        let default = serde_json::to_value(InfoRequest::clearinghouse_state("0xabc")).unwrap();
        let xyz = serde_json::to_value(InfoRequest::clearinghouse_state_for_dex(
            "0xabc",
            Some("xyz"),
        ))
        .unwrap();

        assert_eq!(
            default,
            serde_json::json!({"type": "clearinghouseState", "user": "0xabc"})
        );
        assert_eq!(
            xyz,
            serde_json::json!({"type": "clearinghouseState", "user": "0xabc", "dex": "xyz"})
        );
    }

    #[rstest]
    fn test_info_request_spot_clearinghouse_state() {
        let req = InfoRequest::spot_clearinghouse_state("0xabc");

        assert_eq!(
            req.request_type,
            HyperliquidInfoRequestType::SpotClearinghouseState
        );
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains(r#""type":"spotClearinghouseState""#));
        assert!(json.contains(r#""user":"0xabc""#));
    }

    #[rstest]
    fn test_info_request_user_abstraction() {
        let req = InfoRequest::user_abstraction("0xabc");

        assert_eq!(
            serde_json::to_value(&req).unwrap(),
            serde_json::json!({"type": "userAbstraction", "user": "0xabc"})
        );
    }

    #[rstest]
    fn test_info_request_funding_history_with_end_time() {
        let req = InfoRequest::funding_history("BTC", 1_700_000_000_000, Some(1_700_003_600_000));

        assert_eq!(req.request_type, HyperliquidInfoRequestType::FundingHistory);
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains(r#""type":"fundingHistory""#));
        assert!(json.contains(r#""coin":"BTC""#));
        assert!(json.contains(r#""startTime":1700000000000"#));
        assert!(json.contains(r#""endTime":1700003600000"#));
    }

    #[rstest]
    fn test_info_request_funding_history_omits_end_time_when_none() {
        // Hyperliquid defaults `endTime` to current time when absent; the
        // serializer must omit the field rather than emit `null`.
        let req = InfoRequest::funding_history("BTC", 1_700_000_000_000, None);
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains(r#""startTime":1700000000000"#));
        assert!(
            !json.contains("endTime"),
            "endTime must be omitted when None; json={json}",
        );
    }

    #[rstest]
    fn test_exchange_action_order() {
        let order = OrderRequest {
            asset: 0,
            is_buy: true,
            limit_px: Decimal::new(50000, 0),
            sz: Decimal::new(1, 0),
            reduce_only: false,
            order_type: OrderTypePlacement::Limit {
                tif: SdkTimeInForce::Gtc,
            },
            cloid: Default::default(),
        };

        let action = Action::Order(BatchOrder {
            orders: vec![order],
            grouping: OrderGrouping::Na,
            builder: None,
        });

        assert!(matches!(action, Action::Order(_)));
        let json = serde_json::to_string(&action).unwrap();
        assert!(json.contains("\"orders\""));
    }

    #[rstest]
    fn test_exchange_action_cancel() {
        let action = Action::Cancel(BatchCancel {
            cancels: vec![Cancel { asset: 0, oid: 0 }],
            fast: false,
        });
        assert_eq!(serde_json::to_value(&action).unwrap()["type"], "cancel");
    }

    #[rstest]
    fn test_exchange_action_serialization() {
        let order = OrderRequest {
            asset: 0,
            is_buy: true,
            limit_px: Decimal::new(50000, 0),
            sz: Decimal::new(1, 0),
            reduce_only: false,
            order_type: OrderTypePlacement::Limit {
                tif: SdkTimeInForce::Gtc,
            },
            cloid: Default::default(),
        };

        let action = Action::Order(BatchOrder {
            orders: vec![order],
            grouping: OrderGrouping::Na,
            builder: None,
        });

        let json = serde_json::to_string(&action).unwrap();
        // Verify the serialized action type and order fields
        assert!(json.contains(r#""type":"order""#));
        assert!(json.contains(r#""orders""#));
        assert!(json.contains(r#""grouping":"na""#));
    }

    #[rstest]
    fn test_exchange_action_type_serialization() {
        let actions = [
            (
                Action::Order(BatchOrder {
                    orders: vec![],
                    grouping: OrderGrouping::Na,
                    builder: None,
                }),
                "order",
            ),
            (
                Action::Cancel(BatchCancel {
                    cancels: vec![],
                    fast: false,
                }),
                "cancel",
            ),
            (
                Action::CancelByCloid(BatchCancelCloid {
                    cancels: vec![],
                    fast: false,
                }),
                "cancelByCloid",
            ),
            (
                Action::UpdateLeverage(UpdateLeverage {
                    asset: 1,
                    is_cross: true,
                    leverage: 10,
                }),
                "updateLeverage",
            ),
        ];

        for (action, expected) in actions {
            assert_eq!(serde_json::to_value(action).unwrap()["type"], expected);
        }
    }

    #[rstest]
    fn test_update_leverage_serialization() {
        let action = Action::UpdateLeverage(UpdateLeverage {
            asset: 1,
            is_cross: true,
            leverage: 10,
        });
        let json = serde_json::to_string(&action).unwrap();

        assert!(json.contains(r#""type":"updateLeverage""#));
        assert!(json.contains(r#""asset":1"#));
        assert!(json.contains(r#""isCross":true"#));
        assert!(json.contains(r#""leverage":10"#));
    }

    #[rstest]
    fn test_update_isolated_margin_serialization() {
        let action = UpdateIsolatedMarginParams {
            asset: 2,
            is_buy: false,
            ntli: 1000,
        };
        let json = serde_json::to_string(&action).unwrap();

        assert!(json.contains(r#""type":"updateIsolatedMargin""#));
        assert!(json.contains(r#""asset":2"#));
        assert!(json.contains(r#""isBuy":false"#));
        assert!(json.contains(r#""ntli":1000"#));
    }

    #[rstest]
    fn test_cancel_by_cloid_serialization() {
        let cancel_request = CancelByCloid {
            asset: 0,
            cloid: Cloid::from_hex("0x00000000000000000000000000000000")
                .unwrap()
                .0
                .into(),
        };
        let action = Action::CancelByCloid(BatchCancelCloid {
            cancels: vec![cancel_request],
            fast: false,
        });
        let json = serde_json::to_string(&action).unwrap();

        assert!(json.contains(r#""type":"cancelByCloid""#));
        assert!(json.contains(r#""cancels""#));
    }

    #[rstest]
    fn test_modify_serialization() {
        let modify_request = Modify {
            oid: OidOrCloid::Left(12345),
            order: OrderRequest {
                asset: 0,
                is_buy: true,
                limit_px: Decimal::new(51000, 0),
                sz: Decimal::new(2, 0),
                reduce_only: false,
                order_type: OrderTypePlacement::Limit {
                    tif: SdkTimeInForce::Gtc,
                },
                cloid: Default::default(),
            },
        };
        let action = Action::Modify(ModifyAction {
            oid: modify_request.oid,
            order: modify_request.order,
            always_place: false,
        });
        let json = serde_json::to_string(&action).unwrap();

        assert!(json.contains(r#""type":"modify""#));
        assert!(json.contains(r#""oid":12345"#));
        assert!(json.contains(r#""order""#));
    }
}
