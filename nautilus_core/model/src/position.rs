// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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

// TODO: Liberal use of cloning to get things compiling initially
#![allow(dead_code)]

use std::collections::HashMap;

use nautilus_core::time::UnixNanos;

use crate::{
    enums::{OrderSide, PositionSide},
    events::order::OrderFilled,
    identifiers::{
        account_id::AccountId, client_order_id::ClientOrderId, instrument_id::InstrumentId,
        position_id::PositionId, strategy_id::StrategyId, trade_id::TradeId, trader_id::TraderId,
        venue_order_id::VenueOrderId,
    },
    instruments::Instrument,
    types::{currency::Currency, money::Money, quantity::Quantity},
};

/// Represents a position in a financial market.
///
/// The position ID may be assigned at the trading venue, or can be system
/// generated depending on a strategies OMS (Order Management System) settings.
struct Position {
    events: Vec<OrderFilled>,
    client_order_ids: Vec<ClientOrderId>,
    venue_order_ids: Vec<VenueOrderId>,
    trade_ids: Vec<TradeId>,
    buy_qty: Quantity,
    sell_qty: Quantity,
    commissions: HashMap<Currency, Money>,
    pub trader_id: TraderId,
    pub strategy_id: StrategyId,
    pub instrument_id: InstrumentId,
    pub id: PositionId,
    pub account_id: AccountId,
    pub opening_order_id: ClientOrderId,
    pub closing_order_id: Option<ClientOrderId>,
    pub entry: OrderSide,
    pub side: PositionSide,
    pub signed_qty: f64,
    pub quantity: Quantity,
    pub peak_qty: Quantity,
    pub price_precision: u8,
    pub size_precision: u8,
    pub multiplier: Quantity,
    pub is_inverse: bool,
    pub base_currency: Option<Currency>,
    pub quote_currency: Currency,
    pub settlement_currency: Currency,
    pub ts_init: UnixNanos,
    pub ts_opened: UnixNanos,
    pub ts_last: UnixNanos,
    pub ts_closed: Option<UnixNanos>,
    pub duration_ns: Option<u64>,
    pub avg_px_open: f64,
    pub avg_px_close: Option<f64>,
    pub realized_return: Option<f64>,
    pub realized_pnl: Option<Money>,
}

impl Position {
    pub fn new<T: Instrument>(instrument: &T, fill: &OrderFilled) -> Self {
        assert_eq!(instrument.id(), &fill.instrument_id);
        assert!(fill.position_id.is_some());
        assert!(fill.order_side != OrderSide::NoOrderSide);

        Self {
            events: Vec::<OrderFilled>::new(),
            client_order_ids: Vec::<ClientOrderId>::new(),
            venue_order_ids: Vec::<VenueOrderId>::new(),
            trade_ids: Vec::<TradeId>::new(),
            buy_qty: Quantity::zero(instrument.size_precision()),
            sell_qty: Quantity::zero(instrument.size_precision()),
            commissions: HashMap::<Currency, Money>::new(),
            trader_id: fill.trader_id.clone(),
            strategy_id: fill.strategy_id.clone(),
            instrument_id: fill.instrument_id.clone(),
            id: fill.position_id.clone().unwrap(), // TODO: Improve validation
            account_id: fill.account_id.clone(),
            opening_order_id: fill.client_order_id.clone(),
            closing_order_id: None,
            entry: fill.order_side,
            side: PositionSide::Flat,
            signed_qty: 0.0,
            quantity: fill.last_qty,
            peak_qty: fill.last_qty,
            price_precision: instrument.price_precision(),
            size_precision: instrument.size_precision(),
            multiplier: instrument.multiplier(),
            is_inverse: instrument.is_inverse(),
            base_currency: instrument.base_currency().clone().to_owned().cloned(),
            quote_currency: instrument.quote_currency().clone(),
            settlement_currency: instrument.settlement_currency().clone(),
            ts_init: fill.ts_init,
            ts_opened: fill.ts_event,
            ts_last: fill.ts_event,
            ts_closed: None,
            duration_ns: None,
            avg_px_open: fill.last_px.as_f64(),
            avg_px_close: None,
            realized_return: None,
            realized_pnl: None,
        }
    }

    pub fn apply(&mut self, fill: OrderFilled) {
        assert!(
            !self.trade_ids.contains(&fill.trade_id),
            "`fill.trade_id` already contained in `trade_ids",
        );

        if self.side == PositionSide::Flat {
            // Reset position
            self.events.clear();
            self.trade_ids.clear();
            self.buy_qty = Quantity::zero(self.size_precision);
            self.sell_qty = Quantity::zero(self.size_precision);
            self.commissions.clear();
            self.opening_order_id = fill.client_order_id.clone();
            self.closing_order_id = None;
            self.peak_qty = Quantity::zero(self.size_precision);
            self.ts_init = fill.ts_init;
            self.ts_opened = fill.ts_event;
            self.duration_ns = None;
            self.avg_px_open = fill.last_px.as_f64();
            self.avg_px_close = None;
            self.realized_return = None;
            self.realized_pnl = None;
        }

        self.events.push(fill.clone()); // Potentially do this last
        self.trade_ids.push(fill.trade_id.clone());

        // Calculate cumulative commissions
        let commission_currency = fill.commission.currency.clone();
        let commission_clone = fill.commission.clone();

        if let Some(existing_commission) = self.commissions.get_mut(&commission_currency) {
            *existing_commission += commission_clone;
        } else {
            self.commissions
                .insert(commission_currency, fill.commission.clone());
        }

        // Calculate avg prices, points, return, PnL
        match fill.order_side {
            OrderSide::Buy => {}
            OrderSide::Sell => {}
            _ => panic!("invalid order side"),
        }

        // Set quantities
        self.quantity = Quantity::new(self.signed_qty.abs(), self.size_precision);
        if self.quantity > self.peak_qty {
            self.peak_qty.raw = self.quantity.raw;
        }

        // Set state
        if self.signed_qty > 0.0 {
            self.entry = OrderSide::Buy;
            self.side = PositionSide::Long;
        } else if self.signed_qty < 0.0 {
            self.entry = OrderSide::Sell;
            self.side = PositionSide::Short;
        } else {
            self.side = PositionSide::Flat;
            self.closing_order_id = Some(fill.client_order_id.clone());
            self.ts_closed = Some(fill.ts_event);
            self.duration_ns = Some(self.ts_closed.unwrap() - self.ts_opened);
        }

        self.ts_last = fill.ts_event;
    }
}

impl PartialEq<Self> for Position {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl Eq for Position {}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
