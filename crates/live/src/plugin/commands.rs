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
//! [`HostVTable::submit_order`](nautilus_plugin::HostVTable::submit_order) and
//! friends. The host deserializes the payload into one of the structs below
//! and dispatches to the matching [`Strategy`](nautilus_trading::strategy::Strategy)
//! method on the calling adapter. JSON is the boundary format so the command
//! schema can evolve independently of the in-engine `TradingCommand` shape.

use nautilus_core::Params;
use nautilus_model::{
    identifiers::{ClientId, ClientOrderId, PositionId},
    orders::OrderAny,
    types::{Price, Quantity},
};
use serde::{Deserialize, Serialize};

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
}
