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

use std::fmt::{Debug, Display};

use nautilus_core::{UUID4, UnixNanos};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use ustr::Ustr;

use crate::{
    enums::{
        ContingencyType, LiquiditySide, OrderSide, OrderType, TimeInForce, TrailingOffsetType,
        TriggerType,
    },
    events::OrderEvent,
    identifiers::{
        AccountId, ClientOrderId, ExecAlgorithmId, InstrumentId, OrderListId, PositionId,
        StrategyId, TradeId, TraderId, VenueOrderId,
    },
    types::{Currency, Money, Price, Quantity},
};

/// Represents an event where an order was released from the `OrderEmulated` by the Nautilus system.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.model", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.model")
)]
pub struct OrderReleased {
    /// The trader ID associated with the event.
    pub trader_id: TraderId,
    /// The strategy ID associated with the event.
    pub strategy_id: StrategyId,
    /// The instrument ID associated with the event.
    pub instrument_id: InstrumentId,
    /// The client order ID associated with the event.
    pub client_order_id: ClientOrderId,
    /// The price the order was released at.
    pub released_price: Price,
    /// The unique identifier for the event.
    pub event_id: UUID4,
    /// UNIX timestamp (nanoseconds) when the event occurred.
    pub ts_event: UnixNanos,
    /// UNIX timestamp (nanoseconds) when the event was initialized.
    pub ts_init: UnixNanos,
    /// The causation ID associated with the event.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub causation_id: Option<UUID4>,
}

impl OrderReleased {
    /// Creates a new [`OrderReleased`] instance.
    #[expect(clippy::too_many_arguments)]
    #[must_use]
    pub fn new(
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        released_price: Price,
        event_id: UUID4,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
    ) -> Self {
        Self {
            trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            released_price,
            event_id,
            ts_event,
            ts_init,
            causation_id: None,
        }
    }
}

impl Debug for OrderReleased {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}(trader_id={}, strategy_id={}, instrument_id={}, client_order_id={}, released_price={}, event_id={}, ts_init={})",
            stringify!(OrderReleased),
            self.trader_id,
            self.strategy_id,
            self.instrument_id,
            self.client_order_id,
            self.released_price.to_formatted_string(),
            self.event_id,
            self.ts_init
        )
    }
}

impl Display for OrderReleased {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}(instrument_id={}, client_order_id={}, released_price={})",
            stringify!(OrderReleased),
            self.instrument_id,
            self.client_order_id,
            self.released_price.to_formatted_string(),
        )
    }
}

impl OrderEvent for OrderReleased {
    fn id(&self) -> UUID4 {
        self.event_id
    }

    fn type_name(&self) -> &'static str {
        stringify!(OrderReleased)
    }

    fn order_type(&self) -> Option<OrderType> {
        None
    }

    fn order_side(&self) -> Option<OrderSide> {
        None
    }

    fn trader_id(&self) -> TraderId {
        self.trader_id
    }

    fn strategy_id(&self) -> StrategyId {
        self.strategy_id
    }

    fn instrument_id(&self) -> InstrumentId {
        self.instrument_id
    }

    fn trade_id(&self) -> Option<TradeId> {
        None
    }

    fn currency(&self) -> Option<Currency> {
        None
    }

    fn client_order_id(&self) -> ClientOrderId {
        self.client_order_id
    }

    fn reason(&self) -> Option<Ustr> {
        None
    }

    fn quantity(&self) -> Option<Quantity> {
        None
    }

    fn time_in_force(&self) -> Option<TimeInForce> {
        None
    }

    fn liquidity_side(&self) -> Option<LiquiditySide> {
        None
    }

    fn post_only(&self) -> Option<bool> {
        None
    }

    fn reduce_only(&self) -> Option<bool> {
        None
    }

    fn quote_quantity(&self) -> Option<bool> {
        None
    }

    fn reconciliation(&self) -> bool {
        false
    }

    fn price(&self) -> Option<Price> {
        None
    }

    fn last_px(&self) -> Option<Price> {
        None
    }

    fn last_qty(&self) -> Option<Quantity> {
        None
    }

    fn activation_price(&self) -> Option<Price> {
        None
    }

    fn trigger_price(&self) -> Option<Price> {
        None
    }

    fn trigger_type(&self) -> Option<TriggerType> {
        None
    }

    fn limit_offset(&self) -> Option<Decimal> {
        None
    }

    fn trailing_offset(&self) -> Option<Decimal> {
        None
    }

    fn trailing_offset_type(&self) -> Option<TrailingOffsetType> {
        None
    }

    fn expire_time(&self) -> Option<UnixNanos> {
        None
    }

    fn display_qty(&self) -> Option<Quantity> {
        None
    }

    fn emulation_trigger(&self) -> Option<TriggerType> {
        None
    }

    fn trigger_instrument_id(&self) -> Option<InstrumentId> {
        None
    }

    fn contingency_type(&self) -> Option<ContingencyType> {
        None
    }

    fn order_list_id(&self) -> Option<OrderListId> {
        None
    }

    fn linked_order_ids(&self) -> Option<Vec<ClientOrderId>> {
        None
    }

    fn parent_order_id(&self) -> Option<ClientOrderId> {
        None
    }

    fn exec_algorithm_id(&self) -> Option<ExecAlgorithmId> {
        None
    }

    fn exec_spawn_id(&self) -> Option<ClientOrderId> {
        None
    }

    fn venue_order_id(&self) -> Option<VenueOrderId> {
        None
    }

    fn account_id(&self) -> Option<AccountId> {
        None
    }

    fn position_id(&self) -> Option<PositionId> {
        None
    }

    fn commission(&self) -> Option<Money> {
        None
    }

    fn ts_event(&self) -> UnixNanos {
        self.ts_event
    }

    fn ts_init(&self) -> UnixNanos {
        self.ts_init
    }
    fn causation_id(&self) -> Option<UUID4> {
        self.causation_id
    }

    fn released_price(&self) -> Option<Price> {
        Some(self.released_price)
    }
}

#[cfg(test)]
mod tests {
    use nautilus_core::{UUID4, UnixNanos};
    use rstest::rstest;

    use crate::{
        events::order::{released::OrderReleased, stubs::*},
        identifiers::{ClientOrderId, InstrumentId, StrategyId, TraderId},
        types::Price,
    };

    fn distinct_order_released() -> OrderReleased {
        let mut event = OrderReleased::new(
            TraderId::from("TRADER-001"),
            StrategyId::from("S-002"),
            InstrumentId::from("ETHUSDT-PERP.BINANCE"),
            ClientOrderId::from("O-19700101-000000-001-001-3"),
            Price::from("1234.56"),
            UUID4::new(),
            UnixNanos::from(111_222_333_444_555_666_u64),
            UnixNanos::from(777_888_999_111_222_333_u64),
        );
        event.causation_id = Some(UUID4::new());
        event
    }

    #[rstest]
    fn test_order_released_display(order_released: OrderReleased) {
        let display = format!("{order_released}");
        assert_eq!(
            display,
            "OrderReleased(instrument_id=BTCUSDT.COINBASE, client_order_id=O-19700101-000000-001-001-1, released_price=22_000)"
        );
    }

    #[rstest]
    fn test_order_released_serialization() {
        let original = distinct_order_released();

        let json = serde_json::to_string(&original).unwrap();
        let deserialized: OrderReleased = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.trader_id, original.trader_id);
        assert_eq!(deserialized.strategy_id, original.strategy_id);
        assert_eq!(deserialized.instrument_id, original.instrument_id);
        assert_eq!(deserialized.client_order_id, original.client_order_id);
        assert_eq!(deserialized.released_price, original.released_price);
        assert_eq!(deserialized.event_id, original.event_id);
        assert_eq!(deserialized.ts_event, original.ts_event);
        assert_eq!(deserialized.ts_init, original.ts_init);
        assert_eq!(deserialized.causation_id, original.causation_id);
        assert_eq!(deserialized, original);
    }
}
