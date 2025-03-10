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

use std::fmt::{Debug, Display};

use derive_builder::Builder;
use indexmap::IndexMap;
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
    orders::OrderAny,
    types::{Currency, Money, Price, Quantity},
};

/// Represents an event where an order has been initialized.
///
/// This is a seed event which can instantiate any order through a creation
/// method. This event should contain enough information to be able to send it
/// 'over the wire' and have a valid order created with exactly the same
/// properties as if it had been instantiated locally.
#[repr(C)]
#[derive(Clone, PartialEq, Eq, Builder, Serialize, Deserialize)]
#[builder(default)]
#[serde(tag = "type")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model")
)]
pub struct OrderInitialized {
    /// The trader ID associated with the event.
    pub trader_id: TraderId,
    /// The strategy ID associated with the event.
    pub strategy_id: StrategyId,
    /// The instrument ID associated with the event.
    pub instrument_id: InstrumentId,
    /// The client order ID associated with the event.
    pub client_order_id: ClientOrderId,
    /// The order side.
    pub order_side: OrderSide,
    /// The order type.
    pub order_type: OrderType,
    /// The order quantity.
    pub quantity: Quantity,
    /// The order time in force.
    pub time_in_force: TimeInForce,
    /// If the order will only provide liquidity (make a market).
    pub post_only: bool,
    /// If the order carries the 'reduce-only' execution instruction.
    pub reduce_only: bool,
    /// If the order quantity is denominated in the quote currency.
    pub quote_quantity: bool,
    /// If the event was generated during reconciliation.
    pub reconciliation: bool,
    /// The unique identifier for the event.
    pub event_id: UUID4,
    /// UNIX timestamp (nanoseconds) when the event occurred.
    pub ts_event: UnixNanos,
    /// UNIX timestamp (nanoseconds) when the event was initialized.
    pub ts_init: UnixNanos,
    /// The order price (LIMIT).
    pub price: Option<Price>,
    /// The order trigger price (STOP).
    pub trigger_price: Option<Price>,
    /// The trigger type for the order.
    pub trigger_type: Option<TriggerType>,
    /// The trailing offset for the orders limit price.
    pub limit_offset: Option<Decimal>,
    /// The trailing offset for the orders trigger price (STOP).
    pub trailing_offset: Option<Decimal>,
    /// The trailing offset type.
    pub trailing_offset_type: Option<TrailingOffsetType>,
    /// The order expiration, `None` for no expiration.
    pub expire_time: Option<UnixNanos>,
    /// The quantity of the `LIMIT` order to display on the public book (iceberg).
    pub display_qty: Option<Quantity>,
    /// The emulation trigger type for the order.
    pub emulation_trigger: Option<TriggerType>,
    /// The emulation trigger instrument ID for the order (if `None` then will be the `instrument_id`).
    pub trigger_instrument_id: Option<InstrumentId>,
    /// The order contingency type.
    pub contingency_type: Option<ContingencyType>,
    /// The order list ID associated with the order.
    pub order_list_id: Option<OrderListId>,
    ///  The order linked client order ID(s).
    pub linked_order_ids: Option<Vec<ClientOrderId>>,
    /// The orders parent client order ID.
    pub parent_order_id: Option<ClientOrderId>,
    /// The execution algorithm ID for the order.
    pub exec_algorithm_id: Option<ExecAlgorithmId>,
    /// The execution algorithm parameters for the order.
    pub exec_algorithm_params: Option<IndexMap<Ustr, Ustr>>,
    /// The execution algorithm spawning primary client order ID.
    pub exec_spawn_id: Option<ClientOrderId>,
    /// The custom user tags for the order.
    pub tags: Option<Vec<Ustr>>,
}

impl Default for OrderInitialized {
    /// Creates a new default [`OrderInitialized`] instance for testing.
    fn default() -> Self {
        Self {
            trader_id: TraderId::default(),
            strategy_id: StrategyId::default(),
            instrument_id: InstrumentId::default(),
            client_order_id: ClientOrderId::default(),
            order_side: OrderSide::Buy,
            order_type: OrderType::Market,
            quantity: Quantity::new(100_000.0, 0),
            price: Default::default(),
            trigger_price: Default::default(),
            trigger_type: Default::default(),
            time_in_force: TimeInForce::Day,
            expire_time: Default::default(),
            post_only: Default::default(),
            reduce_only: Default::default(),
            display_qty: Default::default(),
            quote_quantity: Default::default(),
            limit_offset: Default::default(),
            trailing_offset: Default::default(),
            trailing_offset_type: Default::default(),
            emulation_trigger: Default::default(),
            trigger_instrument_id: Default::default(),
            contingency_type: Default::default(),
            order_list_id: Default::default(),
            linked_order_ids: Default::default(),
            parent_order_id: Default::default(),
            exec_algorithm_id: Default::default(),
            exec_algorithm_params: Default::default(),
            exec_spawn_id: Default::default(),
            tags: Default::default(),
            event_id: Default::default(),
            ts_event: Default::default(),
            ts_init: Default::default(),
            reconciliation: Default::default(),
        }
    }
}

impl OrderInitialized {
    /// Creates a new [`OrderInitialized`] instance.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        order_side: OrderSide,
        order_type: OrderType,
        quantity: Quantity,
        time_in_force: TimeInForce,
        post_only: bool,
        reduce_only: bool,
        quote_quantity: bool,
        reconciliation: bool,
        event_id: UUID4,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
        price: Option<Price>,
        trigger_price: Option<Price>,
        trigger_type: Option<TriggerType>,
        limit_offset: Option<Decimal>,
        trailing_offset: Option<Decimal>,
        trailing_offset_type: Option<TrailingOffsetType>,
        expire_time: Option<UnixNanos>,
        display_qty: Option<Quantity>,
        emulation_trigger: Option<TriggerType>,
        trigger_instrument_id: Option<InstrumentId>,
        contingency_type: Option<ContingencyType>,
        order_list_id: Option<OrderListId>,
        linked_order_ids: Option<Vec<ClientOrderId>>,
        parent_order_id: Option<ClientOrderId>,
        exec_algorithm_id: Option<ExecAlgorithmId>,
        exec_algorithm_params: Option<IndexMap<Ustr, Ustr>>,
        exec_spawn_id: Option<ClientOrderId>,
        tags: Option<Vec<Ustr>>,
    ) -> Self {
        Self {
            trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            order_side,
            order_type,
            quantity,
            time_in_force,
            post_only,
            reduce_only,
            quote_quantity,
            reconciliation,
            event_id,
            ts_event,
            ts_init,
            price,
            trigger_price,
            trigger_type,
            limit_offset,
            trailing_offset,
            trailing_offset_type,
            expire_time,
            display_qty,
            emulation_trigger,
            trigger_instrument_id,
            contingency_type,
            order_list_id,
            linked_order_ids,
            parent_order_id,
            exec_algorithm_id,
            exec_algorithm_params,
            exec_spawn_id,
            tags,
        }
    }
}

impl Debug for OrderInitialized {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}(\
            trader_id={}, \
            strategy_id={}, \
            instrument_id={}, \
            client_order_id={}, \
            side={}, \
            type={}, \
            quantity={}, \
            time_in_force={}, \
            post_only={}, \
            reduce_only={}, \
            quote_quantity={}, \
            price={}, \
            emulation_trigger={}, \
            trigger_instrument_id={}, \
            contingency_type={}, \
            order_list_id={}, \
            linked_order_ids=[{}], \
            parent_order_id={}, \
            exec_algorithm_id={}, \
            exec_algorithm_params={}, \
            exec_spawn_id={}, \
            tags={}, \
            event_id={}, \
            ts_init={})",
            stringify!(OrderInitialized),
            self.trader_id,
            self.strategy_id,
            self.instrument_id,
            self.client_order_id,
            self.order_side,
            self.order_type,
            self.quantity,
            self.time_in_force,
            self.post_only,
            self.reduce_only,
            self.quote_quantity,
            self.price
                .map_or("None".to_string(), |price| format!("{price}")),
            self.emulation_trigger
                .map_or("None".to_string(), |trigger| format!("{trigger}")),
            self.trigger_instrument_id
                .map_or("None".to_string(), |instrument_id| format!(
                    "{instrument_id}"
                )),
            self.contingency_type
                .map_or("None".to_string(), |contingency_type| format!(
                    "{contingency_type}"
                )),
            self.order_list_id
                .map_or("None".to_string(), |order_list_id| format!(
                    "{order_list_id}"
                )),
            self.linked_order_ids
                .as_ref()
                .map_or("None".to_string(), |linked_order_ids| linked_order_ids
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")),
            self.parent_order_id
                .map_or("None".to_string(), |parent_order_id| format!(
                    "{parent_order_id}"
                )),
            self.exec_algorithm_id
                .map_or("None".to_string(), |exec_algorithm_id| format!(
                    "{exec_algorithm_id}"
                )),
            self.exec_algorithm_params
                .as_ref()
                .map_or("None".to_string(), |exec_algorithm_params| format!(
                    "{exec_algorithm_params:?}"
                )),
            self.exec_spawn_id
                .map_or("None".to_string(), |exec_spawn_id| format!(
                    "{exec_spawn_id}"
                )),
            self.tags.as_ref().map_or("None".to_string(), |tags| tags
                .iter()
                .map(|x| x.to_string())
                .collect::<Vec<String>>()
                .join(", ")),
            self.event_id,
            self.ts_init
        )
    }
}

impl Display for OrderInitialized {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}(\
            instrument_id={}, \
            client_order_id={}, \
            side={}, \
            type={}, \
            quantity={}, \
            time_in_force={}, \
            post_only={}, \
            reduce_only={}, \
            quote_quantity={}, \
            price={}, \
            emulation_trigger={}, \
            trigger_instrument_id={}, \
            contingency_type={}, \
            order_list_id={}, \
            linked_order_ids=[{}], \
            parent_order_id={}, \
            exec_algorithm_id={}, \
            exec_algorithm_params={}, \
            exec_spawn_id={}, \
            tags={})",
            stringify!(OrderInitialized),
            self.instrument_id,
            self.client_order_id,
            self.order_side,
            self.order_type,
            self.quantity,
            self.time_in_force,
            self.post_only,
            self.reduce_only,
            self.quote_quantity,
            self.price
                .map_or("None".to_string(), |price| format!("{price}")),
            self.emulation_trigger
                .map_or("None".to_string(), |trigger| format!("{trigger}")),
            self.trigger_instrument_id
                .map_or("None".to_string(), |instrument_id| format!(
                    "{instrument_id}"
                )),
            self.contingency_type
                .map_or("None".to_string(), |contingency_type| format!(
                    "{contingency_type}"
                )),
            self.order_list_id
                .map_or("None".to_string(), |order_list_id| format!(
                    "{order_list_id}"
                )),
            self.linked_order_ids
                .as_ref()
                .map_or("None".to_string(), |linked_order_ids| linked_order_ids
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")),
            self.parent_order_id
                .map_or("None".to_string(), |parent_order_id| format!(
                    "{parent_order_id}"
                )),
            self.exec_algorithm_id
                .map_or("None".to_string(), |exec_algorithm_id| format!(
                    "{exec_algorithm_id}"
                )),
            self.exec_algorithm_params
                .as_ref()
                .map_or("None".to_string(), |exec_algorithm_params| format!(
                    "{exec_algorithm_params:?}"
                )),
            self.exec_spawn_id
                .map_or("None".to_string(), |exec_spawn_id| format!(
                    "{exec_spawn_id}"
                )),
            self.tags.as_ref().map_or("None".to_string(), |tags| tags
                .iter()
                .map(|s| s.to_string())
                .collect::<Vec<String>>()
                .join(", ")),
        )
    }
}

impl OrderEvent for OrderInitialized {
    fn id(&self) -> UUID4 {
        self.event_id
    }

    fn kind(&self) -> &str {
        stringify!(OrderInitialized)
    }

    fn order_type(&self) -> Option<OrderType> {
        Some(self.order_type)
    }

    fn order_side(&self) -> Option<OrderSide> {
        Some(self.order_side)
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
        Some(self.quantity)
    }

    fn time_in_force(&self) -> Option<TimeInForce> {
        Some(self.time_in_force)
    }

    fn liquidity_side(&self) -> Option<LiquiditySide> {
        None
    }

    fn post_only(&self) -> Option<bool> {
        Some(self.post_only)
    }

    fn reduce_only(&self) -> Option<bool> {
        Some(self.reduce_only)
    }

    fn quote_quantity(&self) -> Option<bool> {
        Some(self.quote_quantity)
    }

    fn reconciliation(&self) -> bool {
        false
    }

    fn price(&self) -> Option<Price> {
        self.price
    }

    fn last_px(&self) -> Option<Price> {
        None
    }

    fn last_qty(&self) -> Option<Quantity> {
        None
    }

    fn trigger_price(&self) -> Option<Price> {
        self.trigger_price
    }

    fn trigger_type(&self) -> Option<TriggerType> {
        self.trigger_type
    }

    fn limit_offset(&self) -> Option<Decimal> {
        self.limit_offset
    }

    fn trailing_offset(&self) -> Option<Decimal> {
        self.trailing_offset
    }

    fn trailing_offset_type(&self) -> Option<TrailingOffsetType> {
        self.trailing_offset_type
    }

    fn expire_time(&self) -> Option<UnixNanos> {
        self.expire_time
    }

    fn display_qty(&self) -> Option<Quantity> {
        self.display_qty
    }

    fn emulation_trigger(&self) -> Option<TriggerType> {
        self.emulation_trigger
    }

    fn trigger_instrument_id(&self) -> Option<InstrumentId> {
        self.trigger_instrument_id
    }

    fn contingency_type(&self) -> Option<ContingencyType> {
        self.contingency_type
    }

    fn order_list_id(&self) -> Option<OrderListId> {
        self.order_list_id
    }

    fn linked_order_ids(&self) -> Option<Vec<ClientOrderId>> {
        self.linked_order_ids.clone()
    }

    fn parent_order_id(&self) -> Option<ClientOrderId> {
        self.parent_order_id
    }

    fn exec_algorithm_id(&self) -> Option<ExecAlgorithmId> {
        self.exec_algorithm_id
    }

    fn exec_spawn_id(&self) -> Option<ClientOrderId> {
        self.exec_spawn_id
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
}

impl From<OrderInitialized> for OrderAny {
    fn from(order: OrderInitialized) -> Self {
        match order.order_type {
            OrderType::Limit => OrderAny::Limit(order.into()),
            OrderType::Market => OrderAny::Market(order.into()),
            OrderType::StopMarket => OrderAny::StopMarket(order.into()),
            OrderType::StopLimit => OrderAny::StopLimit(order.into()),
            OrderType::LimitIfTouched => OrderAny::LimitIfTouched(order.into()),
            OrderType::TrailingStopLimit => OrderAny::TrailingStopLimit(order.into()),
            OrderType::TrailingStopMarket => OrderAny::TrailingStopMarket(order.into()),
            OrderType::MarketToLimit => OrderAny::MarketToLimit(order.into()),
            OrderType::MarketIfTouched => OrderAny::MarketIfTouched(order.into()),
        }
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod test {
    use rstest::rstest;

    use crate::events::order::{initialized::OrderInitialized, stubs::*};
    #[rstest]
    fn test_order_initialized(order_initialized_buy_limit: OrderInitialized) {
        let display = format!("{order_initialized_buy_limit}");
        assert_eq!(
            display,
            "OrderInitialized(instrument_id=BTCUSDT.COINBASE, client_order_id=O-19700101-000000-001-001-1, \
            side=BUY, type=LIMIT, quantity=0.561, time_in_force=DAY, post_only=true, reduce_only=true, \
            quote_quantity=false, price=22000, emulation_trigger=BID_ASK, trigger_instrument_id=BTCUSDT.COINBASE, \
            contingency_type=OTO, order_list_id=1, linked_order_ids=[O-2020872378424], parent_order_id=None, \
            exec_algorithm_id=None, exec_algorithm_params=None, exec_spawn_id=None, tags=None)"
        );
    }
}
