// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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

use std::str::FromStr;

use nautilus_core::{nanos::UnixNanos, uuid::UUID4};

use super::{any::OrderAny, limit::LimitOrder, stop_market::StopMarketOrder};
use crate::{
    enums::{ContingencyType, LiquiditySide, OrderSide, TimeInForce, TriggerType},
    events::order::{OrderAccepted, OrderEventAny, OrderFilled, OrderSubmitted},
    identifiers::{
        AccountId, ClientOrderId, InstrumentId, PositionId, StrategyId, TradeId, TraderId,
        VenueOrderId,
    },
    instruments::any::InstrumentAny,
    orders::{market::MarketOrder, market_if_touched::MarketIfTouchedOrder},
    types::{money::Money, price::Price, quantity::Quantity},
};

// Test Event Stubs
pub struct TestOrderEventStubs;

impl TestOrderEventStubs {
    pub fn order_submitted(order: &OrderAny, account_id: AccountId) -> OrderEventAny {
        let event = OrderSubmitted::new(
            order.trader_id(),
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            account_id,
            UUID4::new(),
            UnixNanos::default(),
            UnixNanos::default(),
        );
        OrderEventAny::Submitted(event)
    }

    pub fn order_accepted(
        order: &OrderAny,
        account_id: AccountId,
        venue_order_id: VenueOrderId,
    ) -> OrderEventAny {
        let event = OrderAccepted::new(
            order.trader_id(),
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            venue_order_id,
            account_id,
            UUID4::new(),
            UnixNanos::default(),
            UnixNanos::default(),
            false,
        );
        OrderEventAny::Accepted(event)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn order_filled(
        order: &OrderAny,
        instrument: &InstrumentAny,
        trade_id: Option<TradeId>,
        position_id: Option<PositionId>,
        last_px: Option<Price>,
        last_qty: Option<Quantity>,
        liquidity_side: Option<LiquiditySide>,
        commission: Option<Money>,
        ts_filled_ns: Option<UnixNanos>,
        account_id: Option<AccountId>,
    ) -> OrderEventAny {
        let venue_order_id = order.venue_order_id().unwrap_or_default();
        let account_id = account_id
            .or(order.account_id())
            .unwrap_or(AccountId::from("SIM-001"));
        let trade_id = trade_id.unwrap_or(TradeId::new(
            order.client_order_id().as_str().replace('O', "E").as_str(),
        ));
        let liquidity_side = liquidity_side.unwrap_or(LiquiditySide::Maker);
        let event = UUID4::new();
        let position_id = position_id
            .or_else(|| order.position_id())
            .unwrap_or(PositionId::new("1"));
        let commission = commission.unwrap_or(Money::from("2 USD"));
        let last_px = last_px.unwrap_or(Price::from_str("1.0").unwrap());
        let last_qty = last_qty.unwrap_or(order.quantity());
        let event = OrderFilled::new(
            order.trader_id(),
            order.strategy_id(),
            instrument.id(),
            order.client_order_id(),
            venue_order_id,
            account_id,
            trade_id,
            order.order_side(),
            order.order_type(),
            last_qty,
            last_px,
            instrument.quote_currency(),
            liquidity_side,
            event,
            ts_filled_ns.unwrap_or_default(),
            UnixNanos::default(),
            false,
            Some(position_id),
            Some(commission),
        );
        OrderEventAny::Filled(event)
    }
}

pub struct TestOrderStubs;

impl TestOrderStubs {
    #[must_use]
    pub fn market_order(
        instrument_id: InstrumentId,
        order_side: OrderSide,
        quantity: Quantity,
        client_order_id: Option<ClientOrderId>,
        time_in_force: Option<TimeInForce>,
    ) -> OrderAny {
        let order = MarketOrder::new(
            TraderId::default(),
            StrategyId::default(),
            instrument_id,
            client_order_id.unwrap_or_default(),
            order_side,
            quantity,
            time_in_force.unwrap_or(TimeInForce::Gtc),
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
        );
        OrderAny::Market(order)
    }

    #[must_use]
    pub fn market_order_reduce(
        instrument_id: InstrumentId,
        order_side: OrderSide,
        quantity: Quantity,
        client_order_id: Option<ClientOrderId>,
        time_in_force: Option<TimeInForce>,
    ) -> OrderAny {
        let order = MarketOrder::new(
            TraderId::default(),
            StrategyId::default(),
            instrument_id,
            client_order_id.unwrap_or_default(),
            order_side,
            quantity,
            time_in_force.unwrap_or(TimeInForce::Gtc),
            UUID4::new(),
            UnixNanos::default(),
            true, // reduce only
            false,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        );
        OrderAny::Market(order)
    }

    #[must_use]
    pub fn limit_order(
        instrument_id: InstrumentId,
        order_side: OrderSide,
        price: Price,
        quantity: Quantity,
        client_order_id: Option<ClientOrderId>,
        time_in_force: Option<TimeInForce>,
    ) -> OrderAny {
        let client_order_id = client_order_id.unwrap_or_default();
        let order = LimitOrder::new(
            TraderId::default(),
            StrategyId::default(),
            instrument_id,
            client_order_id,
            order_side,
            quantity,
            price,
            time_in_force.unwrap_or(TimeInForce::Gtc),
            None,
            false,
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
            None,
            Some(client_order_id),
            None,
            UUID4::new(),
            UnixNanos::default(),
        )
        .unwrap();
        OrderAny::Limit(order)
    }

    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub fn stop_market_order(
        instrument_id: InstrumentId,
        order_side: OrderSide,
        trigger_price: Price,
        quantity: Quantity,
        trigger_type: Option<TriggerType>,
        contingency_type: Option<ContingencyType>,
        client_order_id: Option<ClientOrderId>,
        time_in_force: Option<TimeInForce>,
        parent_order_id: Option<ClientOrderId>,
        linked_order_ids: Option<Vec<ClientOrderId>>,
    ) -> OrderAny {
        let order = StopMarketOrder::new(
            TraderId::default(),
            StrategyId::default(),
            instrument_id,
            client_order_id.unwrap_or_default(),
            order_side,
            quantity,
            trigger_price,
            trigger_type.unwrap_or(TriggerType::BidAsk),
            time_in_force.unwrap_or(TimeInForce::Gtc),
            None,
            false,
            false,
            None,
            None,
            None,
            contingency_type,
            None,
            linked_order_ids,
            parent_order_id,
            None,
            None,
            None,
            None,
            UUID4::new(),
            UnixNanos::default(),
        );
        OrderAny::StopMarket(order)
    }

    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub fn market_if_touched_order(
        instrument_id: InstrumentId,
        order_side: OrderSide,
        trigger_price: Price,
        quantity: Quantity,
        trigger_type: Option<TriggerType>,
        contingency_type: Option<ContingencyType>,
        client_order_id: Option<ClientOrderId>,
        time_in_force: Option<TimeInForce>,
        linked_order_ids: Option<Vec<ClientOrderId>>,
    ) -> OrderAny {
        OrderAny::MarketIfTouched(MarketIfTouchedOrder::new(
            TraderId::default(),
            StrategyId::default(),
            instrument_id,
            client_order_id.unwrap_or_default(),
            order_side,
            quantity,
            trigger_price,
            trigger_type.unwrap_or(TriggerType::BidAsk),
            time_in_force.unwrap_or(TimeInForce::Gtc),
            None,
            false,
            false,
            None,
            None,
            None,
            contingency_type,
            None,
            linked_order_ids,
            None,
            None,
            None,
            None,
            None,
            UUID4::new(),
            UnixNanos::default(),
        ))
    }

    pub fn make_accepted_order(order: &OrderAny) -> OrderAny {
        let mut new_order = order.clone();
        let submitted_event =
            TestOrderEventStubs::order_submitted(&new_order, AccountId::from("SIM-001"));
        let accepted_event = TestOrderEventStubs::order_accepted(
            &new_order,
            AccountId::from("SIM-001"),
            VenueOrderId::from("V-001"),
        );
        new_order.apply(submitted_event).unwrap();
        new_order.apply(accepted_event).unwrap();
        new_order
    }

    pub fn make_filled_order(
        order: &OrderAny,
        instrument: &InstrumentAny,
        liquidity_side: LiquiditySide,
    ) -> OrderAny {
        let mut accepted_order = TestOrderStubs::make_accepted_order(order);
        let fill = TestOrderEventStubs::order_filled(
            &accepted_order,
            instrument,
            None,
            None,
            None,
            None,
            Some(liquidity_side),
            None,
            None,
            None,
        );
        accepted_order.apply(fill).unwrap();
        accepted_order
    }
}
