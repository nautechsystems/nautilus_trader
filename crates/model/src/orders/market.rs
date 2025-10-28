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

use std::{
    fmt::Display,
    ops::{Deref, DerefMut},
};

use indexmap::IndexMap;
use nautilus_core::{
    UUID4, UnixNanos,
    correctness::{FAILED, check_predicate_false},
};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use ustr::Ustr;

use super::{Order, OrderAny, OrderCore};
use crate::{
    enums::{
        ContingencyType, LiquiditySide, OrderSide, OrderStatus, OrderType, PositionSide,
        TimeInForce, TrailingOffsetType, TriggerType,
    },
    events::{OrderEventAny, OrderInitialized, OrderUpdated},
    identifiers::{
        AccountId, ClientOrderId, ExecAlgorithmId, InstrumentId, OrderListId, PositionId,
        StrategyId, Symbol, TradeId, TraderId, Venue, VenueOrderId,
    },
    orders::OrderError,
    types::{Currency, Money, Price, Quantity, quantity::check_positive_quantity},
};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model")
)]
pub struct MarketOrder {
    core: OrderCore,
}

impl MarketOrder {
    /// Creates a new [`MarketOrder`] instance.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The `quantity` is not positive.
    /// - The `time_in_force` is GTD (invalid for market orders).
    #[allow(clippy::too_many_arguments)]
    pub fn new_checked(
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        order_side: OrderSide,
        quantity: Quantity,
        time_in_force: TimeInForce,
        init_id: UUID4,
        ts_init: UnixNanos,
        reduce_only: bool,
        quote_quantity: bool,
        contingency_type: Option<ContingencyType>,
        order_list_id: Option<OrderListId>,
        linked_order_ids: Option<Vec<ClientOrderId>>,
        parent_order_id: Option<ClientOrderId>,
        exec_algorithm_id: Option<ExecAlgorithmId>,
        exec_algorithm_params: Option<IndexMap<Ustr, Ustr>>,
        exec_spawn_id: Option<ClientOrderId>,
        tags: Option<Vec<Ustr>>,
    ) -> anyhow::Result<Self> {
        check_positive_quantity(quantity, stringify!(quantity))?;
        check_predicate_false(
            time_in_force == TimeInForce::Gtd,
            "GTD not supported for Market orders",
        )?;

        let init_order = OrderInitialized::new(
            trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            order_side,
            OrderType::Market,
            quantity,
            time_in_force,
            false,
            reduce_only,
            quote_quantity,
            false,
            init_id,
            ts_init,
            ts_init,
            None,
            None,
            Some(TriggerType::NoTrigger),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            contingency_type,
            order_list_id,
            linked_order_ids,
            parent_order_id,
            exec_algorithm_id,
            exec_algorithm_params,
            exec_spawn_id,
            tags,
        );

        Ok(Self {
            core: OrderCore::new(init_order),
        })
    }

    /// Creates a new [`MarketOrder`] instance.
    ///
    /// # Panics
    ///
    /// Panics if any order validation fails (see [`MarketOrder::new_checked`]).
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        order_side: OrderSide,
        quantity: Quantity,
        time_in_force: TimeInForce,
        init_id: UUID4,
        ts_init: UnixNanos,
        reduce_only: bool,
        quote_quantity: bool,
        contingency_type: Option<ContingencyType>,
        order_list_id: Option<OrderListId>,
        linked_order_ids: Option<Vec<ClientOrderId>>,
        parent_order_id: Option<ClientOrderId>,
        exec_algorithm_id: Option<ExecAlgorithmId>,
        exec_algorithm_params: Option<IndexMap<Ustr, Ustr>>,
        exec_spawn_id: Option<ClientOrderId>,
        tags: Option<Vec<Ustr>>,
    ) -> Self {
        Self::new_checked(
            trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            order_side,
            quantity,
            time_in_force,
            init_id,
            ts_init,
            reduce_only,
            quote_quantity,
            contingency_type,
            order_list_id,
            linked_order_ids,
            parent_order_id,
            exec_algorithm_id,
            exec_algorithm_params,
            exec_spawn_id,
            tags,
        )
        .expect(FAILED)
    }
}

impl Deref for MarketOrder {
    type Target = OrderCore;

    fn deref(&self) -> &Self::Target {
        &self.core
    }
}

impl DerefMut for MarketOrder {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.core
    }
}

impl PartialEq for MarketOrder {
    fn eq(&self, other: &Self) -> bool {
        self.client_order_id == other.client_order_id
    }
}

impl Order for MarketOrder {
    fn into_any(self) -> OrderAny {
        OrderAny::Market(self)
    }

    fn status(&self) -> OrderStatus {
        self.status
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

    fn symbol(&self) -> Symbol {
        self.instrument_id.symbol
    }

    fn venue(&self) -> Venue {
        self.instrument_id.venue
    }

    fn client_order_id(&self) -> ClientOrderId {
        self.client_order_id
    }

    fn venue_order_id(&self) -> Option<VenueOrderId> {
        self.venue_order_id
    }

    fn position_id(&self) -> Option<PositionId> {
        self.position_id
    }

    fn account_id(&self) -> Option<AccountId> {
        self.account_id
    }

    fn last_trade_id(&self) -> Option<TradeId> {
        self.last_trade_id
    }

    fn order_side(&self) -> OrderSide {
        self.side
    }

    fn order_type(&self) -> OrderType {
        self.order_type
    }

    fn quantity(&self) -> Quantity {
        self.quantity
    }

    fn time_in_force(&self) -> TimeInForce {
        self.time_in_force
    }

    fn expire_time(&self) -> Option<UnixNanos> {
        None
    }

    fn price(&self) -> Option<Price> {
        None
    }

    fn trigger_price(&self) -> Option<Price> {
        None
    }

    fn trigger_type(&self) -> Option<TriggerType> {
        None
    }

    fn liquidity_side(&self) -> Option<LiquiditySide> {
        self.liquidity_side
    }

    fn is_post_only(&self) -> bool {
        false
    }

    fn is_reduce_only(&self) -> bool {
        self.is_reduce_only
    }

    fn is_quote_quantity(&self) -> bool {
        self.is_quote_quantity
    }

    fn has_price(&self) -> bool {
        false
    }

    fn display_qty(&self) -> Option<Quantity> {
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

    fn emulation_trigger(&self) -> Option<TriggerType> {
        None
    }

    fn trigger_instrument_id(&self) -> Option<InstrumentId> {
        None
    }

    fn contingency_type(&self) -> Option<ContingencyType> {
        self.contingency_type
    }

    fn order_list_id(&self) -> Option<OrderListId> {
        self.order_list_id
    }

    fn linked_order_ids(&self) -> Option<&[ClientOrderId]> {
        self.linked_order_ids.as_deref()
    }

    fn parent_order_id(&self) -> Option<ClientOrderId> {
        self.parent_order_id
    }

    fn exec_algorithm_id(&self) -> Option<ExecAlgorithmId> {
        self.exec_algorithm_id
    }

    fn exec_algorithm_params(&self) -> Option<&IndexMap<Ustr, Ustr>> {
        self.exec_algorithm_params.as_ref()
    }

    fn exec_spawn_id(&self) -> Option<ClientOrderId> {
        self.exec_spawn_id
    }

    fn tags(&self) -> Option<&[Ustr]> {
        self.tags.as_deref()
    }

    fn filled_qty(&self) -> Quantity {
        self.filled_qty
    }

    fn leaves_qty(&self) -> Quantity {
        self.leaves_qty
    }

    fn avg_px(&self) -> Option<f64> {
        self.avg_px
    }

    fn slippage(&self) -> Option<f64> {
        self.slippage
    }

    fn init_id(&self) -> UUID4 {
        self.init_id
    }

    fn ts_init(&self) -> UnixNanos {
        self.ts_init
    }

    fn ts_submitted(&self) -> Option<UnixNanos> {
        self.ts_submitted
    }

    fn ts_accepted(&self) -> Option<UnixNanos> {
        self.ts_accepted
    }

    fn ts_closed(&self) -> Option<UnixNanos> {
        self.ts_closed
    }

    fn ts_last(&self) -> UnixNanos {
        self.ts_last
    }

    fn apply(&mut self, event: OrderEventAny) -> Result<(), OrderError> {
        if let OrderEventAny::Updated(ref event) = event {
            self.update(event);
        };

        self.core.apply(event)?;

        Ok(())
    }

    fn update(&mut self, event: &OrderUpdated) {
        assert!(event.price.is_none(), "{}", OrderError::InvalidOrderEvent);
        assert!(
            event.trigger_price.is_none(),
            "{}",
            OrderError::InvalidOrderEvent
        );

        self.quantity = event.quantity;
        self.leaves_qty = self.quantity.saturating_sub(self.filled_qty);
    }

    fn events(&self) -> Vec<&OrderEventAny> {
        self.events.iter().collect()
    }

    fn venue_order_ids(&self) -> Vec<&VenueOrderId> {
        self.venue_order_ids.iter().collect()
    }

    fn trade_ids(&self) -> Vec<&TradeId> {
        self.trade_ids.iter().collect()
    }

    fn commissions(&self) -> &IndexMap<Currency, Money> {
        &self.commissions
    }

    fn is_triggered(&self) -> Option<bool> {
        None
    }

    fn set_position_id(&mut self, position_id: Option<PositionId>) {
        self.position_id = position_id;
    }

    fn set_quantity(&mut self, quantity: Quantity) {
        self.quantity = quantity;
    }

    fn set_leaves_qty(&mut self, leaves_qty: Quantity) {
        self.leaves_qty = leaves_qty;
    }

    fn set_emulation_trigger(&mut self, emulation_trigger: Option<TriggerType>) {
        self.emulation_trigger = emulation_trigger;
    }

    fn set_is_quote_quantity(&mut self, is_quote_quantity: bool) {
        self.is_quote_quantity = is_quote_quantity;
    }

    fn set_liquidity_side(&mut self, liquidity_side: LiquiditySide) {
        self.liquidity_side = Some(liquidity_side);
    }

    fn would_reduce_only(&self, side: PositionSide, position_qty: Quantity) -> bool {
        self.core.would_reduce_only(side, position_qty)
    }

    fn previous_status(&self) -> Option<OrderStatus> {
        self.core.previous_status
    }
}

impl Display for MarketOrder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "MarketOrder(\
            {} {} {} @ {} {}, \
            status={}, \
            client_order_id={}, \
            venue_order_id={}, \
            position_id={}, \
            exec_algorithm_id={}, \
            exec_spawn_id={}, \
            tags={:?}\
            )",
            self.side,
            self.quantity.to_formatted_string(),
            self.instrument_id,
            self.order_type,
            self.time_in_force,
            self.status,
            self.client_order_id,
            self.venue_order_id.map_or_else(
                || "None".to_string(),
                |venue_order_id| format!("{venue_order_id}")
            ),
            self.position_id.map_or_else(
                || "None".to_string(),
                |position_id| format!("{position_id}")
            ),
            self.exec_algorithm_id
                .map_or_else(|| "None".to_string(), |id| format!("{id}")),
            self.exec_spawn_id
                .map_or_else(|| "None".to_string(), |id| format!("{id}")),
            self.tags
        )
    }
}

impl From<OrderInitialized> for MarketOrder {
    fn from(event: OrderInitialized) -> Self {
        Self::new(
            event.trader_id,
            event.strategy_id,
            event.instrument_id,
            event.client_order_id,
            event.order_side,
            event.quantity,
            event.time_in_force,
            event.event_id,
            event.ts_event,
            event.reduce_only,
            event.quote_quantity,
            event.contingency_type,
            event.order_list_id,
            event.linked_order_ids,
            event.parent_order_id,
            event.exec_algorithm_id,
            event.exec_algorithm_params,
            event.exec_spawn_id,
            event.tags,
        )
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use rstest::rstest;

    use crate::{
        enums::{OrderSide, OrderType, TimeInForce},
        events::{OrderEventAny, OrderUpdated, order::initialized::OrderInitializedBuilder},
        instruments::{CurrencyPair, stubs::*},
        orders::{MarketOrder, Order, builder::OrderTestBuilder, stubs::TestOrderStubs},
        types::Quantity,
    };

    #[rstest]
    #[should_panic(
        expected = "Condition failed: invalid `Quantity` for 'quantity' not positive, was 0"
    )]
    fn test_positive_quantity_condition(audusd_sim: CurrencyPair) {
        let _ = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(audusd_sim.id)
            .side(OrderSide::Buy)
            .quantity(Quantity::from(0))
            .build();
    }

    #[rstest]
    #[should_panic(expected = "GTD not supported for Market orders")]
    fn test_gtd_condition(audusd_sim: CurrencyPair) {
        let _ = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(audusd_sim.id)
            .side(OrderSide::Buy)
            .quantity(Quantity::from(100))
            .time_in_force(TimeInForce::Gtd)
            .build();
    }
    #[rstest]
    fn test_market_order_creation(audusd_sim: CurrencyPair) {
        // Create a MarketOrder with specific parameters
        let order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(audusd_sim.id)
            .quantity(Quantity::from(10))
            .side(OrderSide::Buy)
            .time_in_force(TimeInForce::Ioc)
            .build();

        // Assert that the MarketOrder-specific fields are correctly set
        assert_eq!(order.time_in_force(), TimeInForce::Ioc);
        assert_eq!(order.order_type(), OrderType::Market);
        assert!(order.price().is_none());
    }

    #[rstest]
    fn test_market_order_update(audusd_sim: CurrencyPair) {
        // Create and accept a basic MarketOrder
        let order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(audusd_sim.id)
            .quantity(Quantity::from(10))
            .side(OrderSide::Buy)
            .build();

        let mut accepted_order = TestOrderStubs::make_accepted_order(&order);

        // Update with new values
        let updated_quantity = Quantity::from(5);

        let event = OrderUpdated {
            client_order_id: accepted_order.client_order_id(),
            strategy_id: accepted_order.strategy_id(),
            quantity: updated_quantity,
            ..Default::default()
        };

        accepted_order.apply(OrderEventAny::Updated(event)).unwrap();

        // Verify updates were applied correctly
        assert_eq!(accepted_order.quantity(), updated_quantity);
    }

    #[rstest]
    fn test_market_order_from_order_initialized(audusd_sim: CurrencyPair) {
        // Create an OrderInitialized event with all required fields for a MarketOrder
        let order_initialized = OrderInitializedBuilder::default()
            .order_type(OrderType::Market)
            .instrument_id(audusd_sim.id)
            .quantity(Quantity::from(10))
            .order_side(OrderSide::Buy)
            .build()
            .unwrap();

        // Convert the OrderInitialized event into a MarketOrder
        let order: MarketOrder = order_initialized.clone().into();

        // Assert essential fields match the OrderInitialized fields
        assert_eq!(order.trader_id(), order_initialized.trader_id);
        assert_eq!(order.strategy_id(), order_initialized.strategy_id);
        assert_eq!(order.instrument_id(), order_initialized.instrument_id);
        assert_eq!(order.client_order_id(), order_initialized.client_order_id);
        assert_eq!(order.order_side(), order_initialized.order_side);
        assert_eq!(order.quantity(), order_initialized.quantity);
    }

    #[rstest]
    #[should_panic(
        expected = "Condition failed: invalid `Quantity` for 'quantity' not positive, was 0"
    )]
    fn test_market_order_invalid_quantity(audusd_sim: CurrencyPair) {
        let _ = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(audusd_sim.id)
            .quantity(Quantity::from(0))
            .side(OrderSide::Buy)
            .build();
    }

    #[rstest]
    fn test_display(audusd_sim: CurrencyPair) {
        let order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(audusd_sim.id)
            .quantity(Quantity::from(10))
            .side(OrderSide::Buy)
            .build();

        // Assert that the display method returns a string representation of the order
        assert_eq!(
            order.to_string(),
            format!(
                "MarketOrder({} {} {} @ {} {}, status=INITIALIZED, client_order_id={}, venue_order_id=None, position_id=None, exec_algorithm_id=None, exec_spawn_id=None, tags=None)",
                order.order_side(),
                order.quantity().to_formatted_string(),
                order.instrument_id(),
                order.order_type(),
                order.time_in_force(),
                order.client_order_id()
            )
        );
    }
}
