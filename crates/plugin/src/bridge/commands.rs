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

//! JSON command envelopes posted from plug-ins to the host.
//!
//! Plug-ins hand the host opaque JSON for order commands via
//! [`HostVTable::submit_order`](crate::HostVTable::submit_order) and
//! friends. The host deserializes the payload into one of the structs below
//! and dispatches to the matching [`Strategy`](nautilus_trading::strategy::Strategy)
//! method on the calling adapter. JSON is the boundary format so the command
//! schema can evolve independently of the in-engine `TradingCommand` shape.

use nautilus_core::Params;
use nautilus_model::{
    enums::{OrderSide, PositionSide, TimeInForce},
    identifiers::{AccountId, ClientId, ClientOrderId, InstrumentId, PositionId},
    orders::OrderAny,
    types::{Price, Quantity},
};
use serde::{Deserialize, Serialize};
use ustr::Ustr;

/// Submit-order command. Mirrors the arguments to
/// [`Strategy::submit_order`](nautilus_trading::strategy::Strategy::submit_order).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SubmitOrderCommand {
    /// The order to submit.
    pub order: OrderAny,

    /// Optional position the order is associated with.
    #[serde(default)]
    pub position_id: Option<PositionId>,

    /// Optional client routing identifier.
    #[serde(default)]
    pub client_id: Option<ClientId>,

    /// Optional venue-specific parameters.
    #[serde(default)]
    pub params: Option<Params>,
}

/// Cancel-order command. Mirrors the arguments to
/// [`Strategy::cancel_order`](nautilus_trading::strategy::Strategy::cancel_order).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CancelOrderCommand {
    /// The client order identifier of the order to cancel.
    pub client_order_id: ClientOrderId,

    /// Optional client routing identifier.
    #[serde(default)]
    pub client_id: Option<ClientId>,

    /// Optional venue-specific parameters.
    #[serde(default)]
    pub params: Option<Params>,
}

/// Modify-order command. Mirrors the arguments to
/// [`Strategy::modify_order`](nautilus_trading::strategy::Strategy::modify_order).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModifyOrderCommand {
    /// The client order identifier of the order to modify.
    pub client_order_id: ClientOrderId,

    /// New order quantity.
    #[serde(default)]
    pub quantity: Option<Quantity>,

    /// New limit price.
    #[serde(default)]
    pub price: Option<Price>,

    /// New trigger price.
    #[serde(default)]
    pub trigger_price: Option<Price>,

    /// Optional client routing identifier.
    #[serde(default)]
    pub client_id: Option<ClientId>,

    /// Optional venue-specific parameters.
    #[serde(default)]
    pub params: Option<Params>,
}

/// Submit-order-list command. Mirrors the arguments to
/// [`Strategy::submit_order_list`](nautilus_trading::strategy::Strategy::submit_order_list).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SubmitOrderListCommand {
    /// The orders to submit as a batched list.
    pub orders: Vec<OrderAny>,

    /// Optional position the orders are associated with.
    #[serde(default)]
    pub position_id: Option<PositionId>,

    /// Optional client routing identifier.
    #[serde(default)]
    pub client_id: Option<ClientId>,

    /// Optional venue-specific parameters.
    #[serde(default)]
    pub params: Option<Params>,
}

/// Cancel-orders (batched) command. Mirrors the arguments to
/// [`Strategy::cancel_orders`](nautilus_trading::strategy::Strategy::cancel_orders).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CancelOrdersCommand {
    /// The client order identifiers of the orders to cancel.
    pub client_order_ids: Vec<ClientOrderId>,

    /// Optional client routing identifier.
    #[serde(default)]
    pub client_id: Option<ClientId>,

    /// Optional venue-specific parameters.
    #[serde(default)]
    pub params: Option<Params>,
}

/// Cancel-all-orders command. Mirrors the arguments to
/// [`Strategy::cancel_all_orders`](nautilus_trading::strategy::Strategy::cancel_all_orders).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CancelAllOrdersCommand {
    /// The instrument identifier filtering which orders to cancel.
    pub instrument_id: InstrumentId,

    /// Optional order side filter.
    #[serde(default)]
    pub order_side: Option<OrderSide>,

    /// Optional client routing identifier.
    #[serde(default)]
    pub client_id: Option<ClientId>,

    /// Optional venue-specific parameters.
    #[serde(default)]
    pub params: Option<Params>,
}

/// Close-position command. Mirrors the arguments to
/// [`Strategy::close_position`](nautilus_trading::strategy::Strategy::close_position).
///
/// The host resolves `position_id` against the live cache to materialise the
/// `&Position` reference the trait method requires.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClosePositionCommand {
    /// The identifier of the position to close.
    pub position_id: PositionId,

    /// Optional client routing identifier.
    #[serde(default)]
    pub client_id: Option<ClientId>,

    /// Optional tags to attach to the closing order.
    #[serde(default)]
    pub tags: Option<Vec<Ustr>>,

    /// Optional time-in-force override.
    #[serde(default)]
    pub time_in_force: Option<TimeInForce>,

    /// Optional reduce-only flag override.
    #[serde(default)]
    pub reduce_only: Option<bool>,

    /// Optional quote-quantity flag override.
    #[serde(default)]
    pub quote_quantity: Option<bool>,
}

/// Close-all-positions command. Mirrors the arguments to
/// [`Strategy::close_all_positions`](nautilus_trading::strategy::Strategy::close_all_positions).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CloseAllPositionsCommand {
    /// The instrument identifier filtering which positions to close.
    pub instrument_id: InstrumentId,

    /// Optional position side filter.
    #[serde(default)]
    pub position_side: Option<PositionSide>,

    /// Optional client routing identifier.
    #[serde(default)]
    pub client_id: Option<ClientId>,

    /// Optional tags to attach to the closing orders.
    #[serde(default)]
    pub tags: Option<Vec<Ustr>>,

    /// Optional time-in-force override.
    #[serde(default)]
    pub time_in_force: Option<TimeInForce>,

    /// Optional reduce-only flag override.
    #[serde(default)]
    pub reduce_only: Option<bool>,

    /// Optional quote-quantity flag override.
    #[serde(default)]
    pub quote_quantity: Option<bool>,
}

/// Query-account command. Mirrors the arguments to
/// [`Strategy::query_account`](nautilus_trading::strategy::Strategy::query_account).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QueryAccountCommand {
    /// The account identifier to query.
    pub account_id: AccountId,

    /// Optional client routing identifier.
    #[serde(default)]
    pub client_id: Option<ClientId>,

    /// Optional venue-specific parameters.
    #[serde(default)]
    pub params: Option<Params>,
}

/// Query-order command. Mirrors the arguments to
/// [`Strategy::query_order`](nautilus_trading::strategy::Strategy::query_order).
///
/// The host resolves `client_order_id` against the live cache to materialise
/// the `&OrderAny` reference the trait method requires.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QueryOrderCommand {
    /// The client order identifier of the order to query.
    pub client_order_id: ClientOrderId,

    /// Optional client routing identifier.
    #[serde(default)]
    pub client_id: Option<ClientId>,

    /// Optional venue-specific parameters.
    #[serde(default)]
    pub params: Option<Params>,
}

#[cfg(test)]
mod tests {
    use nautilus_core::{UUID4, UnixNanos};
    use nautilus_model::{
        enums::{OrderSide, TimeInForce},
        identifiers::{InstrumentId, StrategyId, TraderId},
        orders::{MarketOrder, Order, OrderAny},
    };
    use rstest::rstest;

    use super::*;

    fn make_market_order(client_order_id: &str) -> OrderAny {
        OrderAny::Market(MarketOrder::new(
            TraderId::from("TRADER-001"),
            StrategyId::from("S-001"),
            InstrumentId::from("ETH-USDT.BINANCE"),
            ClientOrderId::from(client_order_id),
            OrderSide::Buy,
            Quantity::from("1.0"),
            TimeInForce::Gtc,
            UUID4::new(),
            UnixNanos::default(),
            false,
            false,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        ))
    }

    #[rstest]
    fn submit_order_command_round_trips_via_json() {
        let order = make_market_order("O-20240101-000000-001-001-1");
        let cmd = SubmitOrderCommand {
            order: order.clone(),
            position_id: Some(PositionId::from("P-001")),
            client_id: None,
            params: None,
        };
        let json = serde_json::to_string(&cmd).unwrap();
        let back: SubmitOrderCommand = serde_json::from_str(&json).unwrap();
        assert_eq!(back.order.client_order_id(), order.client_order_id());
        assert_eq!(back.order.instrument_id(), order.instrument_id());
        assert_eq!(back.order.quantity(), order.quantity());
        assert_eq!(back.position_id, cmd.position_id);
    }

    #[rstest]
    fn submit_order_command_rejects_unknown_fields() {
        let json = r#"{"order":{},"position_id":null,"extra":1}"#;
        assert!(serde_json::from_str::<SubmitOrderCommand>(json).is_err());
    }

    #[rstest]
    fn cancel_order_command_round_trips_via_json() {
        let cmd = CancelOrderCommand {
            client_order_id: ClientOrderId::from("O-20240101-000000-001-001-1"),
            client_id: None,
            params: None,
        };
        let json = serde_json::to_string(&cmd).unwrap();
        let back: CancelOrderCommand = serde_json::from_str(&json).unwrap();
        assert_eq!(back.client_order_id, cmd.client_order_id);
    }

    #[rstest]
    fn cancel_order_command_rejects_unknown_fields() {
        let json = r#"{"client_order_id":"O-1","extra":1}"#;
        assert!(serde_json::from_str::<CancelOrderCommand>(json).is_err());
    }

    #[rstest]
    fn modify_order_command_round_trips_via_json() {
        let cmd = ModifyOrderCommand {
            client_order_id: ClientOrderId::from("O-20240101-000000-001-001-1"),
            quantity: Some(Quantity::from("1.0")),
            price: Some(Price::from("100.0")),
            trigger_price: None,
            client_id: None,
            params: None,
        };
        let json = serde_json::to_string(&cmd).unwrap();
        let back: ModifyOrderCommand = serde_json::from_str(&json).unwrap();
        assert_eq!(back.client_order_id, cmd.client_order_id);
        assert_eq!(back.quantity, cmd.quantity);
        assert_eq!(back.price, cmd.price);
    }

    #[rstest]
    fn modify_order_command_rejects_unknown_fields() {
        let json = r#"{"client_order_id":"O-1","extra":1}"#;
        assert!(serde_json::from_str::<ModifyOrderCommand>(json).is_err());
    }

    #[rstest]
    fn submit_order_list_command_round_trips_via_json() {
        let order = make_market_order("O-20240101-000000-001-001-1");
        let cmd = SubmitOrderListCommand {
            orders: vec![order.clone()],
            position_id: Some(PositionId::from("P-001")),
            client_id: None,
            params: None,
        };
        let json = serde_json::to_string(&cmd).unwrap();
        let back: SubmitOrderListCommand = serde_json::from_str(&json).unwrap();
        assert_eq!(back.orders.len(), 1);
        assert_eq!(back.orders[0].client_order_id(), order.client_order_id());
        assert_eq!(back.position_id, cmd.position_id);
    }

    #[rstest]
    fn submit_order_list_command_rejects_unknown_fields() {
        let json = r#"{"orders":[],"extra":1}"#;
        assert!(serde_json::from_str::<SubmitOrderListCommand>(json).is_err());
    }

    #[rstest]
    fn cancel_orders_command_round_trips_via_json() {
        let cmd = CancelOrdersCommand {
            client_order_ids: vec![
                ClientOrderId::from("O-20240101-000000-001-001-1"),
                ClientOrderId::from("O-20240101-000000-001-001-2"),
            ],
            client_id: None,
            params: None,
        };
        let json = serde_json::to_string(&cmd).unwrap();
        let back: CancelOrdersCommand = serde_json::from_str(&json).unwrap();
        assert_eq!(back.client_order_ids, cmd.client_order_ids);
    }

    #[rstest]
    fn cancel_orders_command_rejects_unknown_fields() {
        let json = r#"{"client_order_ids":[],"extra":1}"#;
        assert!(serde_json::from_str::<CancelOrdersCommand>(json).is_err());
    }

    #[rstest]
    fn cancel_all_orders_command_round_trips_via_json() {
        let cmd = CancelAllOrdersCommand {
            instrument_id: InstrumentId::from("ETH-USDT.BINANCE"),
            order_side: Some(OrderSide::Buy),
            client_id: None,
            params: None,
        };
        let json = serde_json::to_string(&cmd).unwrap();
        let back: CancelAllOrdersCommand = serde_json::from_str(&json).unwrap();
        assert_eq!(back.instrument_id, cmd.instrument_id);
        assert_eq!(back.order_side, cmd.order_side);
    }

    #[rstest]
    fn cancel_all_orders_command_rejects_unknown_fields() {
        let json = r#"{"instrument_id":"ETH-USDT.BINANCE","extra":1}"#;
        assert!(serde_json::from_str::<CancelAllOrdersCommand>(json).is_err());
    }

    #[rstest]
    fn close_position_command_round_trips_via_json() {
        let cmd = ClosePositionCommand {
            position_id: PositionId::from("P-001"),
            client_id: None,
            tags: Some(vec![Ustr::from("exit"), Ustr::from("manual")]),
            time_in_force: Some(TimeInForce::Ioc),
            reduce_only: Some(true),
            quote_quantity: Some(false),
        };
        let json = serde_json::to_string(&cmd).unwrap();
        let back: ClosePositionCommand = serde_json::from_str(&json).unwrap();
        assert_eq!(back.position_id, cmd.position_id);
        assert_eq!(back.tags, cmd.tags);
        assert_eq!(back.time_in_force, cmd.time_in_force);
        assert_eq!(back.reduce_only, cmd.reduce_only);
    }

    #[rstest]
    fn close_position_command_rejects_unknown_fields() {
        let json = r#"{"position_id":"P-001","extra":1}"#;
        assert!(serde_json::from_str::<ClosePositionCommand>(json).is_err());
    }

    #[rstest]
    fn close_all_positions_command_round_trips_via_json() {
        let cmd = CloseAllPositionsCommand {
            instrument_id: InstrumentId::from("ETH-USDT.BINANCE"),
            position_side: Some(PositionSide::Long),
            client_id: None,
            tags: None,
            time_in_force: None,
            reduce_only: None,
            quote_quantity: None,
        };
        let json = serde_json::to_string(&cmd).unwrap();
        let back: CloseAllPositionsCommand = serde_json::from_str(&json).unwrap();
        assert_eq!(back.instrument_id, cmd.instrument_id);
        assert_eq!(back.position_side, cmd.position_side);
    }

    #[rstest]
    fn close_all_positions_command_rejects_unknown_fields() {
        let json = r#"{"instrument_id":"ETH-USDT.BINANCE","extra":1}"#;
        assert!(serde_json::from_str::<CloseAllPositionsCommand>(json).is_err());
    }

    #[rstest]
    fn query_account_command_round_trips_via_json() {
        let cmd = QueryAccountCommand {
            account_id: AccountId::from("BINANCE-001"),
            client_id: None,
            params: None,
        };
        let json = serde_json::to_string(&cmd).unwrap();
        let back: QueryAccountCommand = serde_json::from_str(&json).unwrap();
        assert_eq!(back.account_id, cmd.account_id);
    }

    #[rstest]
    fn query_account_command_rejects_unknown_fields() {
        let json = r#"{"account_id":"BINANCE-001","extra":1}"#;
        assert!(serde_json::from_str::<QueryAccountCommand>(json).is_err());
    }

    #[rstest]
    fn query_order_command_round_trips_via_json() {
        let cmd = QueryOrderCommand {
            client_order_id: ClientOrderId::from("O-20240101-000000-001-001-1"),
            client_id: None,
            params: None,
        };
        let json = serde_json::to_string(&cmd).unwrap();
        let back: QueryOrderCommand = serde_json::from_str(&json).unwrap();
        assert_eq!(back.client_order_id, cmd.client_order_id);
    }

    #[rstest]
    fn query_order_command_rejects_unknown_fields() {
        let json = r#"{"client_order_id":"O-1","extra":1}"#;
        assert!(serde_json::from_str::<QueryOrderCommand>(json).is_err());
    }
}
