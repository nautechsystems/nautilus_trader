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

use std::{collections::HashMap, str::FromStr};

use nautilus_core::{UUID4, UnixNanos};

use super::any::OrderAny;
use crate::{
    enums::{LiquiditySide, OrderType},
    events::{OrderAccepted, OrderEventAny, OrderFilled, OrderSubmitted},
    identifiers::{
        AccountId, ClientOrderId, InstrumentId, PositionId, TradeId, Venue, VenueOrderId,
    },
    instruments::{Instrument, InstrumentAny},
    orders::{Order, OrderTestBuilder},
    types::{Money, Price, Quantity},
};

// Test Event Stubs
pub struct TestOrderEventStubs;

impl TestOrderEventStubs {
    pub fn submitted(order: &OrderAny, account_id: AccountId) -> OrderEventAny {
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

    pub fn accepted(
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
    pub fn filled(
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
    pub fn make_accepted_order(order: &OrderAny) -> OrderAny {
        let mut new_order = order.clone();
        let accepted_event = TestOrderEventStubs::accepted(
            &new_order,
            AccountId::from("SIM-001"),
            VenueOrderId::from("V-001"),
        );
        new_order.apply(accepted_event).unwrap();
        new_order
    }

    pub fn make_filled_order(
        order: &OrderAny,
        instrument: &InstrumentAny,
        liquidity_side: LiquiditySide,
    ) -> OrderAny {
        let mut accepted_order = TestOrderStubs::make_accepted_order(order);
        let fill = TestOrderEventStubs::filled(
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

pub struct TestOrdersGenerator {
    order_type: OrderType,
    venue_instruments: HashMap<Venue, u32>,
    orders_per_instrument: u32,
}

impl TestOrdersGenerator {
    pub fn new(order_type: OrderType) -> Self {
        Self {
            order_type,
            venue_instruments: HashMap::new(),
            orders_per_instrument: 5,
        }
    }

    pub fn set_orders_per_instrument(&mut self, total_orders: u32) {
        self.orders_per_instrument = total_orders;
    }

    pub fn add_venue_and_total_instruments(&mut self, venue: Venue, total_instruments: u32) {
        self.venue_instruments.insert(venue, total_instruments);
    }

    fn generate_order(&self, instrument_id: InstrumentId, client_order_id_index: u32) -> OrderAny {
        let client_order_id =
            ClientOrderId::from(format!("O-{}-{}", instrument_id, client_order_id_index));
        OrderTestBuilder::new(self.order_type)
            .quantity(Quantity::from("1"))
            .price(Price::from("1"))
            .instrument_id(instrument_id)
            .client_order_id(client_order_id)
            .build()
    }

    pub fn build(&self) -> Vec<OrderAny> {
        let mut orders = Vec::new();
        for (venue, total_instruments) in self.venue_instruments.iter() {
            for i in 0..*total_instruments {
                let instrument_id = InstrumentId::from(format!("SYMBOL-{}.{}", i, venue));
                for order_index in 0..self.orders_per_instrument {
                    let order = self.generate_order(instrument_id, order_index);
                    orders.push(order);
                }
            }
        }
        orders
    }
}

pub fn create_order_list_sample(
    total_venues: u8,
    total_instruments: u32,
    orders_per_instrument: u32,
) -> Vec<OrderAny> {
    // Create Limit orders list from order generator with spec:
    // x venues * x instruments * x orders per instrument
    let mut order_generator = TestOrdersGenerator::new(OrderType::Limit);
    for i in 0..total_venues {
        let venue = Venue::from(format!("VENUE-{}", i));
        order_generator.add_venue_and_total_instruments(venue, total_instruments);
    }
    order_generator.set_orders_per_instrument(orders_per_instrument);

    order_generator.build()
}
