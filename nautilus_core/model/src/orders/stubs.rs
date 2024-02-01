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

use crate::{
    enums::{LiquiditySide, OrderSide, TimeInForce},
    events::order::filled::OrderFilled,
    identifiers::{
        account_id::AccountId, position_id::PositionId, strategy_id::StrategyId, stubs::*,
        trade_id::TradeId, venue_order_id::VenueOrderId,
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
        order: T,
        instrument: I,
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
            *instrument.quote_currency(),
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

// ---- MarketOrder ----
pub fn market_order(quantity: Quantity, time_in_force: Option<TimeInForce>) -> MarketOrder {
    let trader = trader_id();
    let strategy = strategy_id_ema_cross();
    let instrument = instrument_id_eth_usdt_binance();
    let client_order_id = client_order_id();
    MarketOrder::new(
        trader,
        strategy,
        instrument,
        client_order_id,
        OrderSide::Buy,
        quantity,
        time_in_force.unwrap_or(TimeInForce::Gtc),
        UUID4::new(),
        12321312321312,
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
