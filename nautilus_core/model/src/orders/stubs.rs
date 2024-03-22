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

use nautilus_core::uuid::UUID4;

use super::{limit::LimitOrder, stop_market::StopMarketOrder};
use crate::{
    enums::{LiquiditySide, OrderSide, TimeInForce, TriggerType},
    events::order::filled::OrderFilled,
    identifiers::{
        account_id::AccountId,
        client_order_id::ClientOrderId,
        instrument_id::InstrumentId,
        position_id::PositionId,
        strategy_id::StrategyId,
        stubs::{strategy_id_ema_cross, trader_id},
        trade_id::TradeId,
        venue_order_id::VenueOrderId,
    },
    instruments::Instrument,
    orders::{base::Order, market::MarketOrder},
    types::{money::Money, price::Price, quantity::Quantity},
};

// Test Event Stubs
pub struct TestOrderEventStubs;

impl TestOrderEventStubs {
    #[allow(clippy::too_many_arguments)]
    pub fn order_filled<T: Order, I: Instrument>(
        order: &T,
        instrument: &I,
        strategy_id: Option<StrategyId>,
        trade_id: Option<TradeId>,
        position_id: Option<PositionId>,
        last_px: Option<Price>,
        last_qty: Option<Quantity>,
        commission: Option<Money>,
        ts_filled_ns: Option<u64>,
    ) -> OrderFilled {
        let trader_id = trader_id();
        let strategy_id = strategy_id.unwrap_or(order.strategy_id());
        let instrument_id = order.instrument_id();
        let venue_order_id = order
            .venue_order_id()
            .unwrap_or(VenueOrderId::new("1").unwrap());
        let account_id = order
            .account_id()
            .unwrap_or(AccountId::new("SIM-001").unwrap());
        let trade_id = trade_id.unwrap_or(
            TradeId::new(order.client_order_id().value.replace('O', "E").as_str()).unwrap(),
        );
        let liquidity_side = order.liquidity_side().unwrap_or(LiquiditySide::Maker);
        let event = UUID4::new();
        let position_id = position_id
            .or_else(|| order.position_id())
            .unwrap_or(PositionId::new("1").unwrap());
        let commission = commission.unwrap_or(Money::from_str("2 USD").unwrap());
        let last_px = last_px.unwrap_or(Price::from_str("1.0").unwrap());
        let last_qty = last_qty.unwrap_or(order.quantity());
        OrderFilled::new(
            trader_id,
            strategy_id,
            instrument_id,
            order.client_order_id(),
            venue_order_id,
            account_id,
            trade_id,
            order.side(),
            order.order_type(),
            last_qty,
            last_px,
            instrument.quote_currency(),
            liquidity_side,
            event,
            ts_filled_ns.unwrap_or(0),
            0,
            false,
            Some(position_id),
            Some(commission),
        )
        .unwrap()
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
    ) -> MarketOrder {
        let trader = trader_id();
        let strategy = strategy_id_ema_cross();
        let client_order_id =
            client_order_id.unwrap_or(ClientOrderId::from("O-20200814-102234-001-001-1"));
        let time_in_force = time_in_force.unwrap_or(TimeInForce::Gtc);
        MarketOrder::new(
            trader,
            strategy,
            instrument_id,
            client_order_id,
            order_side,
            quantity,
            time_in_force,
            UUID4::new(),
            12_321_312_321_312,
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
        )
        .unwrap()
    }

    #[must_use]
    pub fn limit_order(
        instrument_id: InstrumentId,
        order_side: OrderSide,
        price: Price,
        quantity: Quantity,
        client_order_id: Option<ClientOrderId>,
        time_in_force: Option<TimeInForce>,
    ) -> LimitOrder {
        let trader = trader_id();
        let strategy = strategy_id_ema_cross();
        let client_order_id =
            client_order_id.unwrap_or(ClientOrderId::from("O-19700101-0000-000-001-1"));
        let time_in_force = time_in_force.unwrap_or(TimeInForce::Gtc);
        LimitOrder::new(
            trader,
            strategy,
            instrument_id,
            client_order_id,
            order_side,
            quantity,
            price,
            time_in_force,
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
            12_321_312_321_312,
        )
        .unwrap()
    }

    #[must_use]
    pub fn stop_market_order(
        instrument_id: InstrumentId,
        order_side: OrderSide,
        trigger_price: Price,
        quantity: Quantity,
        trigger_type: Option<TriggerType>,
        client_order_id: Option<ClientOrderId>,
        time_in_force: Option<TimeInForce>,
    ) -> StopMarketOrder {
        let trader = trader_id();
        let strategy = strategy_id_ema_cross();
        let client_order_id =
            client_order_id.unwrap_or(ClientOrderId::from("O-19700101-010000-001-001-1"));
        let time_in_force = time_in_force.unwrap_or(TimeInForce::Gtc);
        StopMarketOrder::new(
            trader,
            strategy,
            instrument_id,
            client_order_id,
            order_side,
            quantity,
            trigger_price,
            trigger_type.unwrap_or(TriggerType::BidAsk),
            time_in_force,
            None,
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
            None,
            None,
            UUID4::new(),
            12_321_312_321_312,
        )
        .unwrap()
    }
}
