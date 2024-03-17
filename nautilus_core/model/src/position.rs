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

use std::{
    collections::{HashMap, HashSet},
    fmt::Display,
    hash::{Hash, Hasher},
};

use nautilus_core::time::UnixNanos;
use serde::{Deserialize, Serialize};

use crate::{
    enums::{OrderSide, PositionSide},
    events::order::filled::OrderFilled,
    identifiers::{
        account_id::AccountId, client_order_id::ClientOrderId, instrument_id::InstrumentId,
        position_id::PositionId, strategy_id::StrategyId, symbol::Symbol, trade_id::TradeId,
        trader_id::TraderId, venue::Venue, venue_order_id::VenueOrderId,
    },
    instruments::Instrument,
    types::{currency::Currency, money::Money, price::Price, quantity::Quantity},
};

/// Represents a position in a financial market.
///
/// The position ID may be assigned at the trading venue, or can be system
/// generated depending on a strategies OMS (Order Management System) settings.
#[repr(C)]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model")
)]
pub struct Position {
    pub events: Vec<OrderFilled>,
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
    pub duration_ns: u64,
    pub avg_px_open: f64,
    pub avg_px_close: Option<f64>,
    pub realized_return: f64,
    pub realized_pnl: Option<Money>,
    pub trade_ids: Vec<TradeId>,
    pub buy_qty: Quantity,
    pub sell_qty: Quantity,
    pub commissions: HashMap<Currency, Money>,
}

impl Position {
    pub fn new<T: Instrument>(instrument: T, fill: OrderFilled) -> anyhow::Result<Self> {
        assert_eq!(instrument.id(), fill.instrument_id);
        assert!(fill.position_id.is_some());
        assert_ne!(fill.order_side, OrderSide::NoOrderSide);

        let mut item = Self {
            events: Vec::<OrderFilled>::new(),
            trade_ids: Vec::<TradeId>::new(),
            buy_qty: Quantity::zero(instrument.size_precision()),
            sell_qty: Quantity::zero(instrument.size_precision()),
            commissions: HashMap::<Currency, Money>::new(),
            trader_id: fill.trader_id,
            strategy_id: fill.strategy_id,
            instrument_id: fill.instrument_id,
            id: fill.position_id.unwrap(), // TODO: Improve validation
            account_id: fill.account_id,
            opening_order_id: fill.client_order_id,
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
            base_currency: instrument.base_currency(),
            quote_currency: instrument.quote_currency(),
            settlement_currency: instrument.settlement_currency(),
            ts_init: fill.ts_init,
            ts_opened: fill.ts_event,
            ts_last: fill.ts_event,
            ts_closed: None,
            duration_ns: 0,
            avg_px_open: fill.last_px.as_f64(),
            avg_px_close: None,
            realized_return: 0.0,
            realized_pnl: None,
        };
        item.apply(&fill);
        Ok(item)
    }

    pub fn apply(&mut self, fill: &OrderFilled) {
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
            self.opening_order_id = fill.client_order_id;
            self.closing_order_id = None;
            self.peak_qty = Quantity::zero(self.size_precision);
            self.ts_init = fill.ts_init;
            self.ts_opened = fill.ts_event;
            self.ts_closed = None;
            self.duration_ns = 0;
            self.avg_px_open = fill.last_px.as_f64();
            self.avg_px_close = None;
            self.realized_return = 0.0;
            self.realized_pnl = None;
        }

        self.events.push(*fill);
        self.trade_ids.push(fill.trade_id);

        // Calculate cumulative commissions
        if let Some(commission_value) = fill.commission {
            let commission_currency = commission_value.currency;
            if let Some(existing_commission) = self.commissions.get_mut(&commission_currency) {
                *existing_commission += commission_value;
            } else {
                self.commissions
                    .insert(commission_currency, commission_value);
            }
        }

        // Calculate avg prices, points, return, PnL
        if fill.order_side == OrderSide::Buy {
            self.handle_buy_order_fill(fill);
        } else if fill.order_side == OrderSide::Sell {
            self.handle_sell_order_fill(fill);
        } else {
            panic!("Invalid order side {}", fill.order_side);
        }

        // Set quantities
        self.quantity = Quantity::new(self.signed_qty.abs(), self.size_precision).unwrap();
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
            self.closing_order_id = Some(fill.client_order_id);
            self.ts_closed = Some(fill.ts_event);
            self.duration_ns = if self.ts_closed.is_some() {
                self.ts_closed.unwrap() - self.ts_opened
            } else {
                0
            };
        }

        self.ts_last = fill.ts_event;
    }

    pub fn handle_buy_order_fill(&mut self, fill: &OrderFilled) {
        let mut realized_pnl = if fill.commission.unwrap().currency == self.settlement_currency {
            -fill.commission.unwrap().as_f64()
        } else {
            0.0
        };
        let last_px = fill.last_px.as_f64();
        let last_qty = fill.last_qty.as_f64();
        let last_qty_object = fill.last_qty;

        if self.signed_qty > 0.0 {
            self.avg_px_open = self.calculate_avg_px_open_px(last_px, last_qty);
        } else if self.signed_qty < 0.0 {
            // SHORT POSITION
            self.avg_px_close = Some(self.calculate_avg_px_close_px(last_px, last_qty));
            self.realized_return =
                self.calculate_return(self.avg_px_open, self.avg_px_close.unwrap());
            realized_pnl += self.calculate_pnl_raw(self.avg_px_open, last_px, last_qty);
        }
        if self.realized_pnl.is_none() {
            self.realized_pnl = Some(Money::new(realized_pnl, self.settlement_currency).unwrap());
        } else {
            self.realized_pnl = Some(
                Money::new(
                    self.realized_pnl.unwrap().as_f64() + realized_pnl,
                    self.settlement_currency,
                )
                .unwrap(),
            );
        }

        self.signed_qty += last_qty;
        self.buy_qty += last_qty_object;
    }

    pub fn handle_sell_order_fill(&mut self, fill: &OrderFilled) {
        let mut realized_pnl = if fill.commission.unwrap().currency == self.settlement_currency {
            -fill.commission.unwrap().as_f64()
        } else {
            0.0
        };
        let last_px = fill.last_px.as_f64();
        let last_qty = fill.last_qty.as_f64();
        let last_qty_object = fill.last_qty;

        if self.signed_qty < 0.0 {
            self.avg_px_open = self.calculate_avg_px_open_px(last_px, last_qty);
        } else if self.signed_qty > 0.0 {
            self.avg_px_close = Some(self.calculate_avg_px_close_px(last_px, last_qty));
            self.realized_return =
                self.calculate_return(self.avg_px_open, self.avg_px_close.unwrap());
            realized_pnl += self.calculate_pnl_raw(self.avg_px_open, last_px, last_qty);
        }

        if self.realized_pnl.is_none() {
            self.realized_pnl = Some(Money::new(realized_pnl, self.settlement_currency).unwrap());
        } else {
            self.realized_pnl = Some(
                Money::new(
                    self.realized_pnl.unwrap().as_f64() + realized_pnl,
                    self.settlement_currency,
                )
                .unwrap(),
            );
        }

        self.signed_qty -= last_qty;
        self.sell_qty += last_qty_object;
    }

    #[must_use]
    pub fn calculate_avg_px(&self, qty: f64, avg_pg: f64, last_px: f64, last_qty: f64) -> f64 {
        let start_cost = avg_pg * qty;
        let event_cost = last_px * last_qty;
        (start_cost + event_cost) / (qty + last_qty)
    }

    #[must_use]
    pub fn calculate_avg_px_open_px(&self, last_px: f64, last_qty: f64) -> f64 {
        self.calculate_avg_px(self.quantity.as_f64(), self.avg_px_open, last_px, last_qty)
    }

    #[must_use]
    pub fn calculate_avg_px_close_px(&self, last_px: f64, last_qty: f64) -> f64 {
        if self.avg_px_close.is_none() {
            return last_px;
        }
        let closing_qty = if self.side == PositionSide::Long {
            self.sell_qty
        } else {
            self.buy_qty
        };
        self.calculate_avg_px(
            closing_qty.as_f64(),
            self.avg_px_close.unwrap(),
            last_px,
            last_qty,
        )
    }

    #[must_use]
    pub fn total_pnl(&self, last: Price) -> Money {
        let realized_pnl = self.realized_pnl.map_or(0.0, |pnl| pnl.as_f64());
        Money::new(
            realized_pnl + self.unrealized_pnl(last).as_f64(),
            self.settlement_currency,
        )
        .unwrap()
    }

    fn calculate_points(&self, avg_px_open: f64, avg_px_close: f64) -> f64 {
        match self.side {
            PositionSide::Long => avg_px_close - avg_px_open,
            PositionSide::Short => avg_px_open - avg_px_close,
            _ => 0.0, // FLAT
        }
    }

    fn calculate_points_inverse(&self, avg_px_open: f64, avg_px_close: f64) -> f64 {
        let inverse_open = 1.0 / avg_px_open;
        let inverse_close = 1.0 / avg_px_close;
        match self.side {
            PositionSide::Long => inverse_open - inverse_close,
            PositionSide::Short => inverse_close - inverse_open,
            _ => 0.0, // FLAT
        }
    }

    #[must_use]
    pub fn calculate_pnl(&self, avg_px_open: f64, avg_px_close: f64, quantity: Quantity) -> Money {
        let pnl_raw = self.calculate_pnl_raw(avg_px_open, avg_px_close, quantity.as_f64());
        Money::new(pnl_raw, self.settlement_currency).unwrap()
    }

    #[must_use]
    pub fn unrealized_pnl(&self, last: Price) -> Money {
        if self.side == PositionSide::Flat {
            Money::new(0.0, self.settlement_currency).unwrap()
        } else {
            let avg_px_open = self.avg_px_open;
            let avg_px_close = last.as_f64();
            let quantity = self.quantity.as_f64();
            let pnl = self.calculate_pnl_raw(avg_px_open, avg_px_close, quantity);
            Money::new(pnl, self.settlement_currency).unwrap()
        }
    }

    #[must_use]
    pub fn calculate_return(&self, avg_px_open: f64, avg_px_close: f64) -> f64 {
        self.calculate_points(avg_px_open, avg_px_close) / avg_px_open
    }

    fn calculate_pnl_raw(&self, avg_px_open: f64, avg_px_close: f64, quantity: f64) -> f64 {
        let quantity = quantity.min(self.signed_qty.abs());
        if self.is_inverse {
            quantity
                * self.multiplier.as_f64()
                * self.calculate_points_inverse(avg_px_open, avg_px_close)
        } else {
            quantity * self.multiplier.as_f64() * self.calculate_points(avg_px_open, avg_px_close)
        }
    }

    #[must_use]
    pub fn is_opposite_side(&self, side: OrderSide) -> bool {
        self.entry != side
    }

    #[must_use]
    pub fn symbol(&self) -> Symbol {
        self.instrument_id.symbol
    }

    #[must_use]
    pub fn venue(&self) -> Venue {
        self.instrument_id.venue
    }

    #[must_use]
    pub fn event_count(&self) -> usize {
        self.events.len()
    }

    #[must_use]
    pub fn client_order_ids(&self) -> Vec<ClientOrderId> {
        // first to hash set to remove duplicate,
        // then again iter to vector
        let mut result = self
            .events
            .iter()
            .map(|event| event.client_order_id)
            .collect::<HashSet<ClientOrderId>>()
            .into_iter()
            .collect::<Vec<ClientOrderId>>();
        result.sort_unstable();
        result
    }

    #[must_use]
    pub fn venue_order_ids(&self) -> Vec<VenueOrderId> {
        // first to hash set to remove duplicate,
        // then again iter to vector
        let mut result = self
            .events
            .iter()
            .map(|event| event.venue_order_id)
            .collect::<HashSet<VenueOrderId>>()
            .into_iter()
            .collect::<Vec<VenueOrderId>>();
        result.sort_unstable();
        result
    }

    #[must_use]
    pub fn trade_ids(&self) -> Vec<TradeId> {
        let mut result = self
            .events
            .iter()
            .map(|event| event.trade_id)
            .collect::<HashSet<TradeId>>()
            .into_iter()
            .collect::<Vec<TradeId>>();
        result.sort_unstable();
        result
    }

    #[must_use]
    pub fn notional_value(&self, last: Price) -> Money {
        if self.is_inverse {
            Money::new(
                self.quantity.as_f64() * self.multiplier.as_f64() * (1.0 / last.as_f64()),
                self.base_currency.unwrap(),
            )
            .unwrap()
        } else {
            Money::new(
                self.quantity.as_f64() * last.as_f64() * self.multiplier.as_f64(),
                self.quote_currency,
            )
            .unwrap()
        }
    }

    #[must_use]
    pub fn last_event(&self) -> OrderFilled {
        // SAFETY: Position invariant guarantees at least one event
        *self.events.last().unwrap()
    }

    #[must_use]
    pub fn last_trade_id(&self) -> Option<TradeId> {
        self.trade_ids.last().copied()
    }

    #[must_use]
    pub fn is_long(&self) -> bool {
        self.side == PositionSide::Long
    }

    #[must_use]
    pub fn is_short(&self) -> bool {
        self.side == PositionSide::Short
    }

    #[must_use]
    pub fn is_open(&self) -> bool {
        self.side != PositionSide::Flat && self.ts_closed.is_none()
    }

    #[must_use]
    pub fn is_closed(&self) -> bool {
        self.side == PositionSide::Flat && self.ts_closed.is_some()
    }

    #[must_use]
    pub fn commissions(&self) -> Vec<Money> {
        self.commissions.values().copied().collect()
    }
}

impl PartialEq<Self> for Position {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl Eq for Position {}

impl Hash for Position {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}

impl Display for Position {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let quantity_str = if self.quantity != Quantity::zero(self.price_precision) {
            self.quantity.to_formatted_string() + " "
        } else {
            String::new()
        };
        write!(
            f,
            "Position({} {}{}, id={})",
            self.side, quantity_str, self.instrument_id, self.id
        )
    }
}

// Tests either need to:
// - Use more primitive objects so that `model` doesn't depend on `common`
// - Transfer these sorts of tests to a dedicated testing crate (less desirable)

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use rstest::rstest;

    use crate::{
        enums::{LiquiditySide, OrderSide, OrderType, PositionSide},
        events::order::filled::OrderFilled,
        identifiers::{
            account_id::AccountId, position_id::PositionId, strategy_id::StrategyId, stubs::uuid4,
            trade_id::TradeId, venue_order_id::VenueOrderId,
        },
        instruments::{crypto_perpetual::CryptoPerpetual, currency_pair::CurrencyPair, stubs::*},
        orders::{
            market::MarketOrder,
            stubs::{TestOrderEventStubs, TestOrderStubs},
        },
        position::Position,
        stubs::*,
        types::{money::Money, price::Price, quantity::Quantity},
    };

    #[rstest]
    fn test_position_long_display(test_position_long: Position) {
        let display = format!("{test_position_long}");
        assert_eq!(display, "Position(LONG 1 AUD/USD.SIM, id=1)");
    }

    #[rstest]
    fn test_position_short_display(test_position_short: Position) {
        let display = format!("{test_position_short}");
        assert_eq!(display, "Position(SHORT 1 AUD/USD.SIM, id=1)");
    }

    #[rstest]
    #[should_panic(expected = "`fill.trade_id` already contained in `trade_ids")]
    fn test_two_trades_with_same_trade_id_throws(audusd_sim: CurrencyPair) {
        let order1 = TestOrderStubs::market_order(
            audusd_sim.id,
            OrderSide::Buy,
            Quantity::from(100_000),
            None,
            None,
        );
        let order2 = TestOrderStubs::market_order(
            audusd_sim.id,
            OrderSide::Buy,
            Quantity::from(100_000),
            None,
            None,
        );
        let fill1 = TestOrderEventStubs::order_filled::<MarketOrder, CurrencyPair>(
            &order1,
            &audusd_sim,
            None,
            Some(TradeId::new("1").unwrap()),
            None,
            Some(Price::from("1.00001")),
            None,
            None,
            None,
        );
        let fill2 = TestOrderEventStubs::order_filled::<MarketOrder, CurrencyPair>(
            &order2,
            &audusd_sim,
            None,
            Some(TradeId::new("1").unwrap()),
            None,
            Some(Price::from("1.00002")),
            None,
            None,
            None,
        );
        let mut position = Position::new(audusd_sim, fill1).unwrap();
        position.apply(&fill2);
    }

    #[rstest]
    fn test_position_filled_with_buy_order(audusd_sim: CurrencyPair) {
        let order = TestOrderStubs::market_order(
            audusd_sim.id,
            OrderSide::Buy,
            Quantity::from(100_000),
            None,
            None,
        );
        let fill = TestOrderEventStubs::order_filled(
            &order,
            &audusd_sim,
            None,
            None,
            None,
            Some(Price::from("1.00001")),
            None,
            None,
            None,
        );
        let last_price = Price::from_str("1.0005").unwrap();
        let position = Position::new(audusd_sim, fill).unwrap();
        assert_eq!(position.symbol(), audusd_sim.id.symbol);
        assert_eq!(position.venue(), audusd_sim.id.venue);
        assert!(!position.is_opposite_side(fill.order_side));
        assert_eq!(position, position); // equality operator test
        assert!(position.closing_order_id.is_none());
        assert_eq!(position.quantity, Quantity::from(100_000));
        assert_eq!(position.peak_qty, Quantity::from(100_000));
        assert_eq!(position.size_precision, 0);
        assert_eq!(position.signed_qty, 100_000.0);
        assert_eq!(position.entry, OrderSide::Buy);
        assert_eq!(position.side, PositionSide::Long);
        assert_eq!(position.ts_opened, 0);
        assert_eq!(position.duration_ns, 0);
        assert_eq!(position.avg_px_open, 1.00001);
        assert_eq!(position.event_count(), 1);
        assert_eq!(position.id, PositionId::new("1").unwrap());
        assert_eq!(position.events.len(), 1);
        assert!(position.is_long());
        assert!(!position.is_short());
        assert!(position.is_open());
        assert!(!position.is_closed());
        assert_eq!(position.realized_return, 0.0);
        assert_eq!(
            position.realized_pnl,
            Some(Money::from_str("-2.0 USD").unwrap())
        );
        assert_eq!(
            position.unrealized_pnl(last_price),
            Money::from_str("49.0 USD").unwrap()
        );
        assert_eq!(
            position.total_pnl(last_price),
            Money::from_str("47.0 USD").unwrap()
        );
        assert_eq!(
            position.commissions(),
            vec![Money::from_str("2.0 USD").unwrap()]
        );
        assert_eq!(
            format!("{position}"),
            "Position(LONG 100_000 AUD/USD.SIM, id=1)"
        );
    }

    #[rstest]
    fn test_position_filled_with_sell_order(audusd_sim: CurrencyPair) {
        let order = TestOrderStubs::market_order(
            audusd_sim.id,
            OrderSide::Sell,
            Quantity::from(100_000),
            None,
            None,
        );
        let fill = TestOrderEventStubs::order_filled(
            &order,
            &audusd_sim,
            None,
            None,
            None,
            Some(Price::from("1.00001")),
            None,
            None,
            None,
        );
        let last_price = Price::from_str("1.00050").unwrap();
        let position = Position::new(audusd_sim, fill).unwrap();
        assert_eq!(position.symbol(), audusd_sim.id.symbol);
        assert_eq!(position.venue(), audusd_sim.id.venue);
        assert!(!position.is_opposite_side(fill.order_side));
        assert_eq!(position, position); // equality operator test
        assert!(position.closing_order_id.is_none());
        assert_eq!(position.quantity, Quantity::from(100_000));
        assert_eq!(position.peak_qty, Quantity::from(100_000));
        assert_eq!(position.signed_qty, -100_000.0);
        assert_eq!(position.entry, OrderSide::Sell);
        assert_eq!(position.side, PositionSide::Short);
        assert_eq!(position.ts_opened, 0);
        assert_eq!(position.avg_px_open, 1.00001);
        assert_eq!(position.event_count(), 1);
        assert_eq!(position.id, PositionId::new("1").unwrap());
        assert_eq!(position.events.len(), 1);
        assert!(!position.is_long());
        assert!(position.is_short());
        assert!(position.is_open());
        assert!(!position.is_closed());
        assert_eq!(position.realized_return, 0.0);
        assert_eq!(
            position.realized_pnl,
            Some(Money::from_str("-2.0 USD").unwrap())
        );
        assert_eq!(
            position.unrealized_pnl(last_price),
            Money::from_str("-49.0 USD").unwrap()
        );
        assert_eq!(
            position.total_pnl(last_price),
            Money::from_str("-51.0 USD").unwrap()
        );
        assert_eq!(
            position.commissions(),
            vec![Money::from_str("2.0 USD").unwrap()]
        );
        assert_eq!(
            format!("{position}"),
            "Position(SHORT 100_000 AUD/USD.SIM, id=1)"
        );
    }

    #[rstest]
    fn test_position_partial_fills_with_buy_order(audusd_sim: CurrencyPair) {
        let order = TestOrderStubs::market_order(
            audusd_sim.id,
            OrderSide::Buy,
            Quantity::from(100_000),
            None,
            None,
        );
        let fill = TestOrderEventStubs::order_filled(
            &order,
            &audusd_sim,
            None,
            None,
            None,
            Some(Price::from("1.00001")),
            Some(Quantity::from(50_000)),
            None,
            None,
        );
        let last_price = Price::from_str("1.00048").unwrap();
        let position = Position::new(audusd_sim, fill).unwrap();
        assert_eq!(position.quantity, Quantity::from(50_000));
        assert_eq!(position.peak_qty, Quantity::from(50_000));
        assert_eq!(position.side, PositionSide::Long);
        assert_eq!(position.signed_qty, 50000.0);
        assert_eq!(position.avg_px_open, 1.00001);
        assert_eq!(position.event_count(), 1);
        assert_eq!(position.ts_opened, 0);
        assert!(position.is_long());
        assert!(!position.is_short());
        assert!(position.is_open());
        assert!(!position.is_closed());
        assert_eq!(position.realized_return, 0.0);
        assert_eq!(
            position.realized_pnl,
            Some(Money::from_str("-2.0 USD").unwrap())
        );
        assert_eq!(
            position.unrealized_pnl(last_price),
            Money::from_str("23.5 USD").unwrap()
        );
        assert_eq!(
            position.total_pnl(last_price),
            Money::from_str("21.5 USD").unwrap()
        );
        assert_eq!(
            position.commissions(),
            vec![Money::from_str("2.0 USD").unwrap()]
        );
        assert_eq!(
            format!("{position}"),
            "Position(LONG 50_000 AUD/USD.SIM, id=1)"
        );
    }

    #[rstest]
    fn test_position_partial_fills_with_two_sell_orders(audusd_sim: CurrencyPair) {
        let order = TestOrderStubs::market_order(
            audusd_sim.id,
            OrderSide::Sell,
            Quantity::from(100_000),
            None,
            None,
        );
        let fill1 = TestOrderEventStubs::order_filled::<MarketOrder, CurrencyPair>(
            &order,
            &audusd_sim,
            None,
            Some(TradeId::new("1").unwrap()),
            None,
            Some(Price::from("1.00001")),
            Some(Quantity::from(50_000)),
            None,
            None,
        );
        let fill2 = TestOrderEventStubs::order_filled::<MarketOrder, CurrencyPair>(
            &order,
            &audusd_sim,
            None,
            Some(TradeId::new("2").unwrap()),
            None,
            Some(Price::from("1.00002")),
            Some(Quantity::from(50_000)),
            None,
            None,
        );
        let last_price = Price::from_str("1.0005").unwrap();
        let mut position = Position::new(audusd_sim, fill1).unwrap();
        position.apply(&fill2);

        assert_eq!(position.quantity, Quantity::from(100_000));
        assert_eq!(position.peak_qty, Quantity::from(100_000));
        assert_eq!(position.side, PositionSide::Short);
        assert_eq!(position.signed_qty, -100_000.0);
        assert_eq!(position.avg_px_open, 1.000_015);
        assert_eq!(position.event_count(), 2);
        assert_eq!(position.ts_opened, 0);
        assert!(position.is_short());
        assert!(!position.is_long());
        assert!(position.is_open());
        assert!(!position.is_closed());
        assert_eq!(position.realized_return, 0.0);
        assert_eq!(
            position.realized_pnl,
            Some(Money::from_str("-4.0 USD").unwrap())
        );
        assert_eq!(
            position.unrealized_pnl(last_price),
            Money::from_str("-48.5 USD").unwrap()
        );
        assert_eq!(
            position.total_pnl(last_price),
            Money::from_str("-52.5 USD").unwrap()
        );
        assert_eq!(
            position.commissions(),
            vec![Money::from_str("4.0 USD").unwrap()]
        );
    }

    #[rstest]
    pub fn test_position_filled_with_buy_order_then_sell_order(audusd_sim: CurrencyPair) {
        let order = TestOrderStubs::market_order(
            audusd_sim.id,
            OrderSide::Buy,
            Quantity::from(150_000),
            None,
            None,
        );
        let fill = TestOrderEventStubs::order_filled(
            &order,
            &audusd_sim,
            Some(StrategyId::new("S-001").unwrap()),
            Some(TradeId::new("1").unwrap()),
            Some(PositionId::new("P-1").unwrap()),
            Some(Price::from("1.00001")),
            None,
            None,
            Some(1_000_000_000),
        );
        let mut position = Position::new(audusd_sim, fill).unwrap();

        let fill2 = OrderFilled::new(
            order.trader_id,
            StrategyId::new("S-001").unwrap(),
            order.instrument_id,
            order.client_order_id,
            VenueOrderId::from("2"),
            order
                .account_id
                .unwrap_or(AccountId::new("SIM-001").unwrap()),
            TradeId::new("2").unwrap(),
            OrderSide::Sell,
            OrderType::Market,
            order.quantity,
            Price::from("1.00011"),
            audusd_sim.quote_currency,
            LiquiditySide::Taker,
            uuid4(),
            2_000_000_000,
            0,
            false,
            Some(PositionId::new("T1").unwrap()),
            Some(Money::from_str("0.0 USD").unwrap()),
        )
        .unwrap();
        position.apply(&fill2);
        let last = Price::from_str("1.0005").unwrap();

        assert!(position.is_opposite_side(fill2.order_side));
        assert_eq!(
            position.quantity,
            Quantity::zero(audusd_sim.price_precision)
        );
        assert_eq!(position.size_precision, 0);
        assert_eq!(position.signed_qty, 0.0);
        assert_eq!(position.side, PositionSide::Flat);
        assert_eq!(position.ts_opened, 1_000_000_000);
        assert_eq!(position.ts_closed, Some(2_000_000_000));
        assert_eq!(position.duration_ns, 1_000_000_000);
        assert_eq!(position.avg_px_open, 1.00001);
        assert_eq!(position.avg_px_close, Some(1.00011));
        assert!(!position.is_long());
        assert!(!position.is_short());
        assert!(!position.is_open());
        assert!(position.is_closed());
        assert_eq!(position.realized_return, 9.999_900_000_998_888e-5);
        assert_eq!(
            position.realized_pnl,
            Some(Money::from_str("13.0 USD").unwrap())
        );
        assert_eq!(
            position.unrealized_pnl(last),
            Money::from_str("0 USD").unwrap()
        );
        assert_eq!(
            position.commissions(),
            vec![Money::from_str("2 USD").unwrap()]
        );
        assert_eq!(position.total_pnl(last), Money::from_str("13 USD").unwrap());
        assert_eq!(format!("{position}"), "Position(FLAT AUD/USD.SIM, id=P-1)");
    }

    #[rstest]
    pub fn test_position_filled_with_sell_order_then_buy_order(audusd_sim: CurrencyPair) {
        let order1 = TestOrderStubs::market_order(
            audusd_sim.id,
            OrderSide::Sell,
            Quantity::from(100_000),
            None,
            None,
        );
        let order2 = TestOrderStubs::market_order(
            audusd_sim.id,
            OrderSide::Buy,
            Quantity::from(100_000),
            None,
            None,
        );
        let fill1 = TestOrderEventStubs::order_filled::<MarketOrder, CurrencyPair>(
            &order1,
            &audusd_sim,
            None,
            None,
            Some(PositionId::new("P-19700101-0000-000-001-1").unwrap()),
            Some(Price::from("1.0")),
            None,
            None,
            None,
        );
        let mut position = Position::new(audusd_sim, fill1).unwrap();
        // create closing from order from different venue but same strategy
        let fill2 = TestOrderEventStubs::order_filled(
            &order2,
            &audusd_sim,
            Some(StrategyId::new("S-001").unwrap()),
            Some(TradeId::new("1").unwrap()),
            Some(PositionId::new("P-19700101-0000-000-001-1").unwrap()),
            Some(Price::from("1.00001")),
            Some(Quantity::from(50_000)),
            None,
            None,
        );
        let fill3 = TestOrderEventStubs::order_filled(
            &order2,
            &audusd_sim,
            Some(StrategyId::new("S-001").unwrap()),
            Some(TradeId::new("2").unwrap()),
            Some(PositionId::new("P-19700101-0000-000-001-1").unwrap()),
            Some(Price::from("1.00003")),
            Some(Quantity::from(50_000)),
            None,
            None,
        );
        let last = Price::from("1.0005");
        position.apply(&fill2);
        position.apply(&fill3);

        assert_eq!(
            position.quantity,
            Quantity::zero(audusd_sim.price_precision)
        );
        assert_eq!(position.side, PositionSide::Flat);
        assert_eq!(position.ts_opened, 0);
        assert_eq!(position.avg_px_open, 1.0);
        assert_eq!(position.events.len(), 3);
        assert_eq!(position.ts_closed, Some(0));
        assert_eq!(position.avg_px_close, Some(1.00002));
        assert!(!position.is_long());
        assert!(!position.is_short());
        assert!(!position.is_open());
        assert!(position.is_closed());
        assert_eq!(
            position.commissions(),
            vec![Money::from_str("6.0 USD").unwrap()]
        );
        assert_eq!(
            position.unrealized_pnl(last),
            Money::from_str("0 USD").unwrap()
        );
        assert_eq!(
            position.realized_pnl,
            Some(Money::from_str("-8.0 USD").unwrap())
        );
        assert_eq!(
            position.total_pnl(last),
            Money::from_str("-8.0 USD").unwrap()
        );
        assert_eq!(
            format!("{position}"),
            "Position(FLAT AUD/USD.SIM, id=P-19700101-0000-000-001-1)"
        );
    }

    #[rstest]
    fn test_position_filled_with_no_change(audusd_sim: CurrencyPair) {
        let order1 = TestOrderStubs::market_order(
            audusd_sim.id,
            OrderSide::Buy,
            Quantity::from(100_000),
            None,
            None,
        );
        let order2 = TestOrderStubs::market_order(
            audusd_sim.id,
            OrderSide::Sell,
            Quantity::from(100_000),
            None,
            None,
        );
        let fill1 = TestOrderEventStubs::order_filled(
            &order1,
            &audusd_sim,
            None,
            Some(TradeId::new("1").unwrap()),
            Some(PositionId::new("P-19700101-0000-000-001-1").unwrap()),
            Some(Price::from("1.0")),
            None,
            None,
            None,
        );
        let mut position = Position::new(audusd_sim, fill1).unwrap();
        let fill2 = TestOrderEventStubs::order_filled(
            &order2,
            &audusd_sim,
            None,
            Some(TradeId::new("2").unwrap()),
            Some(PositionId::new("P-19700101-0000-000-001-1").unwrap()),
            Some(Price::from("1.0")),
            None,
            None,
            None,
        );
        let last = Price::from("1.0005");
        position.apply(&fill2);

        assert_eq!(
            position.quantity,
            Quantity::zero(audusd_sim.price_precision)
        );
        assert_eq!(position.side, PositionSide::Flat);
        assert_eq!(position.ts_opened, 0);
        assert_eq!(position.avg_px_open, 1.0);
        assert_eq!(position.events.len(), 2);
        assert_eq!(position.trade_ids, vec![fill1.trade_id, fill2.trade_id]);
        assert_eq!(position.ts_closed, Some(0));
        assert_eq!(position.avg_px_close, Some(1.0));
        assert!(!position.is_long());
        assert!(!position.is_short());
        assert!(!position.is_open());
        assert!(position.is_closed());
        assert_eq!(
            position.commissions(),
            vec![Money::from_str("4.0 USD").unwrap()]
        );
        assert_eq!(
            position.unrealized_pnl(last),
            Money::from_str("0 USD").unwrap()
        );
        assert_eq!(
            position.realized_pnl,
            Some(Money::from_str("-4.0 USD").unwrap())
        );
        assert_eq!(
            position.total_pnl(last),
            Money::from_str("-4.0 USD").unwrap()
        );
        assert_eq!(
            format!("{position}"),
            "Position(FLAT AUD/USD.SIM, id=P-19700101-0000-000-001-1)"
        );
    }

    #[rstest]
    fn test_position_long_with_multiple_filled_orders(audusd_sim: CurrencyPair) {
        let order1 = TestOrderStubs::market_order(
            audusd_sim.id,
            OrderSide::Buy,
            Quantity::from(100_000),
            None,
            None,
        );
        let order2 = TestOrderStubs::market_order(
            audusd_sim.id,
            OrderSide::Buy,
            Quantity::from(100_000),
            None,
            None,
        );
        let order3 = TestOrderStubs::market_order(
            audusd_sim.id,
            OrderSide::Sell,
            Quantity::from(200_000),
            None,
            None,
        );
        let fill1 = TestOrderEventStubs::order_filled(
            &order1,
            &audusd_sim,
            Some(StrategyId::from("S-001")),
            Some(TradeId::new("1").unwrap()),
            Some(PositionId::new("P-123456").unwrap()),
            Some(Price::from("1.0")),
            None,
            None,
            None,
        );
        let fill2 = TestOrderEventStubs::order_filled(
            &order2,
            &audusd_sim,
            Some(StrategyId::from("S-001")),
            Some(TradeId::new("2").unwrap()),
            Some(PositionId::new("P-123456").unwrap()),
            Some(Price::from("1.00001")),
            None,
            None,
            None,
        );
        let fill3 = TestOrderEventStubs::order_filled(
            &order3,
            &audusd_sim,
            Some(StrategyId::from("S-001")),
            Some(TradeId::new("3").unwrap()),
            Some(PositionId::new("P-123456").unwrap()),
            Some(Price::from("1.0001")),
            None,
            None,
            None,
        );
        let mut position = Position::new(audusd_sim, fill1).unwrap();
        let last = Price::from("1.0005");
        position.apply(&fill2);
        position.apply(&fill3);

        assert_eq!(
            position.quantity,
            Quantity::zero(audusd_sim.price_precision)
        );
        assert_eq!(position.side, PositionSide::Flat);
        assert_eq!(position.ts_opened, 0);
        assert_eq!(position.avg_px_open, 1.000_005);
        assert_eq!(position.events.len(), 3);
        assert_eq!(
            position.trade_ids,
            vec![fill1.trade_id, fill2.trade_id, fill3.trade_id]
        );
        assert_eq!(position.ts_closed, Some(0));
        assert_eq!(position.avg_px_close, Some(1.0001));
        assert!(position.is_closed());
        assert!(!position.is_open());
        assert!(!position.is_long());
        assert!(!position.is_short());
        assert_eq!(
            position.commissions(),
            vec![Money::from_str("6.0 USD").unwrap()]
        );
        assert_eq!(
            position.realized_pnl,
            Some(Money::from_str("13.0 USD").unwrap())
        );
        assert_eq!(
            position.unrealized_pnl(last),
            Money::from_str("0 USD").unwrap()
        );
        assert_eq!(position.total_pnl(last), Money::from_str("13 USD").unwrap());
        assert_eq!(
            format!("{position}"),
            "Position(FLAT AUD/USD.SIM, id=P-123456)"
        );
    }

    #[rstest]
    fn test_pnl_calculation_from_trading_technologies_example(currency_pair_ethusdt: CurrencyPair) {
        let quantity1 = Quantity::from(12);
        let price1 = Price::from("100.0");
        let order1 = TestOrderStubs::market_order(
            currency_pair_ethusdt.id,
            OrderSide::Buy,
            quantity1,
            None,
            None,
        );
        let commission1 =
            calculate_commission(currency_pair_ethusdt, order1.quantity, price1, None).unwrap();
        let fill1 = TestOrderEventStubs::order_filled::<MarketOrder, CurrencyPair>(
            &order1,
            &currency_pair_ethusdt,
            Some(StrategyId::from("S-001")),
            Some(TradeId::new("1").unwrap()),
            Some(PositionId::new("P-123456").unwrap()),
            Some(price1),
            None,
            Some(commission1),
            None,
        );
        let mut position = Position::new(currency_pair_ethusdt, fill1).unwrap();
        let quantity2 = Quantity::from(17);
        let order2 = TestOrderStubs::market_order(
            currency_pair_ethusdt.id,
            OrderSide::Buy,
            quantity2,
            None,
            None,
        );
        let price2 = Price::from("99.0");
        let commission2 =
            calculate_commission(currency_pair_ethusdt, order2.quantity, price2, None).unwrap();
        let fill2 = TestOrderEventStubs::order_filled::<MarketOrder, CurrencyPair>(
            &order2,
            &currency_pair_ethusdt,
            Some(StrategyId::from("S-001")),
            Some(TradeId::new("2").unwrap()),
            Some(PositionId::new("P-123456").unwrap()),
            Some(price2),
            None,
            Some(commission2),
            None,
        );
        position.apply(&fill2);
        assert_eq!(position.quantity, Quantity::from(29));
        assert_eq!(
            position.realized_pnl,
            Some(Money::from_str("-0.28830000 USDT").unwrap())
        );
        assert_eq!(position.avg_px_open, 99.413_793_103_448_27);
        let quantity3 = Quantity::from(9);
        let order3 = TestOrderStubs::market_order(
            currency_pair_ethusdt.id,
            OrderSide::Sell,
            quantity3,
            None,
            None,
        );
        let price3 = Price::from("101.0");
        let commission3 =
            calculate_commission(currency_pair_ethusdt, order3.quantity, price3, None).unwrap();
        let fill3 = TestOrderEventStubs::order_filled::<MarketOrder, CurrencyPair>(
            &order3,
            &currency_pair_ethusdt,
            Some(StrategyId::from("S-001")),
            Some(TradeId::new("3").unwrap()),
            Some(PositionId::new("P-123456").unwrap()),
            Some(price3),
            None,
            Some(commission3),
            None,
        );
        position.apply(&fill3);
        assert_eq!(position.quantity, Quantity::from(20));
        assert_eq!(position.realized_pnl, Some(Money::from("13.89666207 USDT")));
        assert_eq!(position.avg_px_open, 99.413_793_103_448_27);
        let quantity4 = Quantity::from("4");
        let price4 = Price::from("105.0");
        let order4 = TestOrderStubs::market_order(
            currency_pair_ethusdt.id,
            OrderSide::Sell,
            quantity4,
            None,
            None,
        );
        let commission4 =
            calculate_commission(currency_pair_ethusdt, order4.quantity, price4, None).unwrap();
        let fill4 = TestOrderEventStubs::order_filled::<MarketOrder, CurrencyPair>(
            &order4,
            &currency_pair_ethusdt,
            Some(StrategyId::from("S-001")),
            Some(TradeId::new("4").unwrap()),
            Some(PositionId::new("P-123456").unwrap()),
            Some(price4),
            None,
            Some(commission4),
            None,
        );
        position.apply(&fill4);
        assert_eq!(position.quantity, Quantity::from("16"));
        assert_eq!(position.realized_pnl, Some(Money::from("36.19948966 USDT")));
        assert_eq!(position.avg_px_open, 99.413_793_103_448_27);
        let quantity5 = Quantity::from("3");
        let price5 = Price::from("103.0");
        let order5 = TestOrderStubs::market_order(
            currency_pair_ethusdt.id,
            OrderSide::Buy,
            quantity5,
            None,
            None,
        );
        let commission5 =
            calculate_commission(currency_pair_ethusdt, order5.quantity, price5, None).unwrap();
        let fill5 = TestOrderEventStubs::order_filled::<MarketOrder, CurrencyPair>(
            &order5,
            &currency_pair_ethusdt,
            Some(StrategyId::from("S-001")),
            Some(TradeId::new("5").unwrap()),
            Some(PositionId::new("P-123456").unwrap()),
            Some(price5),
            None,
            Some(commission5),
            None,
        );
        position.apply(&fill5);
        assert_eq!(position.quantity, Quantity::from("19"));
        assert_eq!(position.realized_pnl, Some(Money::from("36.16858966 USDT")));
        assert_eq!(position.avg_px_open, 99.980_036_297_640_65);
        assert_eq!(
            format!("{position}"),
            "Position(LONG 19.00000 ETHUSDT.BINANCE, id=P-123456)"
        );
    }

    #[rstest]
    fn test_position_closed_and_reopened(audusd_sim: CurrencyPair) {
        let quantity1 = Quantity::from(150_000);
        let price1 = Price::from("1.00001");
        let order =
            TestOrderStubs::market_order(audusd_sim.id, OrderSide::Buy, quantity1, None, None);
        let commission1 = calculate_commission(audusd_sim, quantity1, price1, None).unwrap();
        let fill1 = TestOrderEventStubs::order_filled::<MarketOrder, CurrencyPair>(
            &order,
            &audusd_sim,
            Some(StrategyId::from("S-001")),
            Some(TradeId::new("5").unwrap()),
            Some(PositionId::new("P-123456").unwrap()),
            Some(Price::from("1.00001")),
            None,
            Some(commission1),
            Some(1_000_000_000),
        );
        let mut position = Position::new(audusd_sim, fill1).unwrap();

        let fill2 = OrderFilled::new(
            order.trader_id,
            order.strategy_id,
            order.instrument_id,
            order.client_order_id,
            VenueOrderId::from("2"),
            order
                .account_id
                .unwrap_or(AccountId::new("SIM-001").unwrap()),
            TradeId::from("2"),
            OrderSide::Sell,
            OrderType::Market,
            order.quantity,
            Price::from("1.00011"),
            audusd_sim.quote_currency,
            LiquiditySide::Taker,
            uuid4(),
            2_000_000_000,
            0,
            false,
            Some(PositionId::from("P-123456")),
            Some(Money::from("0 USD")),
        )
        .unwrap();
        position.apply(&fill2);
        let fill3 = OrderFilled::new(
            order.trader_id,
            order.strategy_id,
            order.instrument_id,
            order.client_order_id,
            VenueOrderId::from("2"),
            order
                .account_id
                .unwrap_or(AccountId::new("SIM-001").unwrap()),
            TradeId::from("3"),
            OrderSide::Buy,
            OrderType::Market,
            order.quantity,
            Price::from("1.00012"),
            audusd_sim.quote_currency,
            LiquiditySide::Taker,
            uuid4(),
            3_000_000_000,
            0,
            false,
            Some(PositionId::from("P-123456")),
            Some(Money::from("0 USD")),
        )
        .unwrap();
        position.apply(&fill3);
        let last = Price::from("1.0003");
        assert!(position.is_opposite_side(fill2.order_side));
        assert_eq!(position.quantity, Quantity::from(150_000));
        assert_eq!(position.peak_qty, Quantity::from(150_000));
        assert_eq!(position.side, PositionSide::Long);
        assert_eq!(position.opening_order_id, fill3.client_order_id);
        assert_eq!(position.closing_order_id, None);
        assert_eq!(position.closing_order_id, None);
        assert_eq!(position.ts_opened, 3_000_000_000);
        assert_eq!(position.duration_ns, 0);
        assert_eq!(position.avg_px_open, 1.00012);
        assert_eq!(position.event_count(), 1);
        assert_eq!(position.ts_closed, None);
        assert_eq!(position.avg_px_close, None);
        assert!(position.is_long());
        assert!(!position.is_short());
        assert!(position.is_open());
        assert!(!position.is_closed());
        assert_eq!(position.realized_return, 0.0);
        assert_eq!(
            position.realized_pnl,
            Some(Money::from_str("0 USD").unwrap())
        );
        assert_eq!(
            position.unrealized_pnl(last),
            Money::from_str("27 USD").unwrap()
        );
        assert_eq!(position.total_pnl(last), Money::from_str("27 USD").unwrap());
        assert_eq!(
            position.commissions(),
            vec![Money::from_str("0 USD").unwrap()]
        );
        assert_eq!(
            format!("{position}"),
            "Position(LONG 150_000 AUD/USD.SIM, id=P-123456)"
        );
    }

    #[rstest]
    fn test_position_realised_pnl_with_interleaved_order_sides(
        currency_pair_btcusdt: CurrencyPair,
    ) {
        let order1 = TestOrderStubs::market_order(
            currency_pair_btcusdt.id,
            OrderSide::Buy,
            Quantity::from(12),
            None,
            None,
        );
        let commission1 = calculate_commission(
            currency_pair_btcusdt,
            order1.quantity,
            Price::from("10000.0"),
            None,
        )
        .unwrap();
        let fill1 = TestOrderEventStubs::order_filled(
            &order1,
            &currency_pair_btcusdt,
            None,
            Some(TradeId::from("1")),
            Some(PositionId::from("P-19700101-0000-000-001-1")),
            Some(Price::from("10000.0")),
            None,
            Some(commission1),
            None,
        );
        let mut position = Position::new(currency_pair_btcusdt, fill1).unwrap();
        let order2 = TestOrderStubs::market_order(
            currency_pair_btcusdt.id,
            OrderSide::Buy,
            Quantity::from(17),
            None,
            None,
        );
        let commission2 = calculate_commission(
            currency_pair_btcusdt,
            order2.quantity,
            Price::from("9999.0"),
            None,
        )
        .unwrap();
        let fill2 = TestOrderEventStubs::order_filled(
            &order2,
            &currency_pair_btcusdt,
            None,
            Some(TradeId::from("2")),
            Some(PositionId::from("P-19700101-0000-000-001-1")),
            Some(Price::from("9999.0")),
            None,
            Some(commission2),
            None,
        );
        position.apply(&fill2);
        assert_eq!(position.quantity, Quantity::from(29));
        assert_eq!(
            position.realized_pnl,
            Some(Money::from_str("-289.98300000 USDT").unwrap())
        );
        assert_eq!(position.avg_px_open, 9_999.413_793_103_447);
        let order3 = TestOrderStubs::market_order(
            currency_pair_btcusdt.id,
            OrderSide::Sell,
            Quantity::from(9),
            None,
            None,
        );
        let commission3 = calculate_commission(
            currency_pair_btcusdt,
            order3.quantity,
            Price::from("10001.0"),
            None,
        )
        .unwrap();
        let fill3 = TestOrderEventStubs::order_filled(
            &order3,
            &currency_pair_btcusdt,
            None,
            Some(TradeId::from("3")),
            Some(PositionId::from("P-19700101-0000-000-001-1")),
            Some(Price::from("10001.0")),
            None,
            Some(commission3),
            None,
        );
        position.apply(&fill3);
        assert_eq!(position.quantity, Quantity::from(20));
        assert_eq!(
            position.realized_pnl,
            Some(Money::from_str("-365.71613793 USDT").unwrap())
        );
        assert_eq!(position.avg_px_open, 9_999.413_793_103_447);
        let order4 = TestOrderStubs::market_order(
            currency_pair_btcusdt.id,
            OrderSide::Buy,
            Quantity::from(3),
            None,
            None,
        );
        let commission4 = calculate_commission(
            currency_pair_btcusdt,
            order4.quantity,
            Price::from("10003.0"),
            None,
        )
        .unwrap();
        let fill4 = TestOrderEventStubs::order_filled(
            &order4,
            &currency_pair_btcusdt,
            None,
            Some(TradeId::from("4")),
            Some(PositionId::from("P-19700101-0000-000-001-1")),
            Some(Price::from("10003.0")),
            None,
            Some(commission4),
            None,
        );
        position.apply(&fill4);
        assert_eq!(position.quantity, Quantity::from(23));
        assert_eq!(
            position.realized_pnl,
            Some(Money::from_str("-395.72513793 USDT").unwrap())
        );
        assert_eq!(position.avg_px_open, 9_999.881_559_220_39);
        let order5 = TestOrderStubs::market_order(
            currency_pair_btcusdt.id,
            OrderSide::Sell,
            Quantity::from(4),
            None,
            None,
        );
        let commission5 = calculate_commission(
            currency_pair_btcusdt,
            order5.quantity,
            Price::from("10005.0"),
            None,
        )
        .unwrap();
        let fill5 = TestOrderEventStubs::order_filled(
            &order5,
            &currency_pair_btcusdt,
            None,
            Some(TradeId::from("5")),
            Some(PositionId::from("P-19700101-0000-000-001-1")),
            Some(Price::from("10005.0")),
            None,
            Some(commission5),
            None,
        );
        position.apply(&fill5);
        assert_eq!(position.quantity, Quantity::from(19));
        assert_eq!(
            position.realized_pnl,
            Some(Money::from_str("-415.27137481 USDT").unwrap())
        );
        assert_eq!(position.avg_px_open, 9_999.881_559_220_39);
        assert_eq!(
            format!("{position}"),
            "Position(LONG 19.000000 BTCUSDT.BINANCE, id=P-19700101-0000-000-001-1)"
        );
    }

    #[rstest]
    fn test_calculate_pnl_when_given_position_side_flat_returns_zero(
        currency_pair_btcusdt: CurrencyPair,
    ) {
        let order = TestOrderStubs::market_order(
            currency_pair_btcusdt.id,
            OrderSide::Buy,
            Quantity::from(12),
            None,
            None,
        );
        let fill = TestOrderEventStubs::order_filled(
            &order,
            &currency_pair_btcusdt,
            None,
            None,
            Some(PositionId::from("P-123456")),
            Some(Price::from("10500.0")),
            None,
            None,
            None,
        );
        let position = Position::new(currency_pair_btcusdt, fill).unwrap();
        let result = position.calculate_pnl(10500.0, 10500.0, Quantity::from("100000.0"));
        assert_eq!(result, Money::from("0 USDT"));
    }

    #[rstest]
    fn test_calculate_pnl_for_long_position_win(currency_pair_btcusdt: CurrencyPair) {
        let order = TestOrderStubs::market_order(
            currency_pair_btcusdt.id,
            OrderSide::Buy,
            Quantity::from(12),
            None,
            None,
        );
        let commission = calculate_commission(
            currency_pair_btcusdt,
            order.quantity,
            Price::from("10500.0"),
            None,
        )
        .unwrap();
        let fill = TestOrderEventStubs::order_filled(
            &order,
            &currency_pair_btcusdt,
            None,
            None,
            Some(PositionId::from("P-123456")),
            Some(Price::from("10500.0")),
            None,
            Some(commission),
            None,
        );
        let position = Position::new(currency_pair_btcusdt, fill).unwrap();
        let pnl = position.calculate_pnl(10500.0, 10510.0, Quantity::from("12.0"));
        assert_eq!(pnl, Money::from("120 USDT"));
        assert_eq!(position.realized_pnl, Some(Money::from("-126 USDT")));
        assert_eq!(
            position.unrealized_pnl(Price::from("10510.0")),
            Money::from("120.0 USDT")
        );
        assert_eq!(
            position.total_pnl(Price::from("10510.0")),
            Money::from("-6 USDT")
        );
        assert_eq!(position.commissions(), vec![Money::from("126.0 USDT")]);
    }

    #[rstest]
    fn test_calculate_pnl_for_long_position_loss(currency_pair_btcusdt: CurrencyPair) {
        let order = TestOrderStubs::market_order(
            currency_pair_btcusdt.id,
            OrderSide::Buy,
            Quantity::from(12),
            None,
            None,
        );
        let commission = calculate_commission(
            currency_pair_btcusdt,
            order.quantity,
            Price::from("10500.0"),
            None,
        )
        .unwrap();
        let fill = TestOrderEventStubs::order_filled(
            &order,
            &currency_pair_btcusdt,
            None,
            None,
            Some(PositionId::from("P-123456")),
            Some(Price::from("10500.0")),
            None,
            Some(commission),
            None,
        );
        let position = Position::new(currency_pair_btcusdt, fill).unwrap();
        let pnl = position.calculate_pnl(10500.0, 10480.5, Quantity::from("10.0"));
        assert_eq!(pnl, Money::from("-195 USDT"));
        assert_eq!(position.realized_pnl, Some(Money::from("-126 USDT")));
        assert_eq!(
            position.unrealized_pnl(Price::from("10480.50")),
            Money::from("-234.0 USDT")
        );
        assert_eq!(
            position.total_pnl(Price::from("10480.50")),
            Money::from("-360 USDT")
        );
        assert_eq!(position.commissions(), vec![Money::from("126.0 USDT")]);
    }

    #[rstest]
    fn test_calculate_pnl_for_short_position_winning(currency_pair_btcusdt: CurrencyPair) {
        let order = TestOrderStubs::market_order(
            currency_pair_btcusdt.id,
            OrderSide::Sell,
            Quantity::from("10.15"),
            None,
            None,
        );
        let commission = calculate_commission(
            currency_pair_btcusdt,
            order.quantity,
            Price::from("10500.0"),
            None,
        )
        .unwrap();
        let fill = TestOrderEventStubs::order_filled(
            &order,
            &currency_pair_btcusdt,
            None,
            None,
            Some(PositionId::from("P-123456")),
            Some(Price::from("10500.0")),
            None,
            Some(commission),
            None,
        );
        let position = Position::new(currency_pair_btcusdt, fill).unwrap();
        let pnl = position.calculate_pnl(10500.0, 10390.0, Quantity::from("10.15"));
        assert_eq!(pnl, Money::from("1116.5 USDT"));
        assert_eq!(
            position.unrealized_pnl(Price::from("10390.0")),
            Money::from("1116.5 USDT")
        );
        assert_eq!(position.realized_pnl, Some(Money::from("-106.575 USDT")));
        assert_eq!(position.commissions(), vec![Money::from("106.575 USDT")]);
        assert_eq!(
            position.notional_value(Price::from("10390.0")),
            Money::from("105458.5 USDT")
        );
    }

    #[rstest]
    fn test_calculate_pnl_for_short_position_loss(currency_pair_btcusdt: CurrencyPair) {
        let order = TestOrderStubs::market_order(
            currency_pair_btcusdt.id,
            OrderSide::Sell,
            Quantity::from("10.0"),
            None,
            None,
        );
        let commission = calculate_commission(
            currency_pair_btcusdt,
            order.quantity,
            Price::from("10500.0"),
            None,
        )
        .unwrap();
        let fill = TestOrderEventStubs::order_filled(
            &order,
            &currency_pair_btcusdt,
            None,
            None,
            Some(PositionId::from("P-123456")),
            Some(Price::from("10500.0")),
            None,
            Some(commission),
            None,
        );
        let position = Position::new(currency_pair_btcusdt, fill).unwrap();
        let pnl = position.calculate_pnl(10500.0, 10670.5, Quantity::from("10.0"));
        assert_eq!(pnl, Money::from("-1705 USDT"));
        assert_eq!(
            position.unrealized_pnl(Price::from("10670.5")),
            Money::from("-1705 USDT")
        );
        assert_eq!(position.realized_pnl, Some(Money::from("-105 USDT")));
        assert_eq!(position.commissions(), vec![Money::from("105 USDT")]);
        assert_eq!(
            position.notional_value(Price::from("10670.5")),
            Money::from("106705 USDT")
        );
    }

    #[rstest]
    fn test_calculate_pnl_for_inverse1(xbtusd_bitmex: CryptoPerpetual) {
        let order = TestOrderStubs::market_order(
            xbtusd_bitmex.id,
            OrderSide::Sell,
            Quantity::from("100000"),
            None,
            None,
        );
        let commission =
            calculate_commission(xbtusd_bitmex, order.quantity, Price::from("10000.0"), None)
                .unwrap();
        let fill = TestOrderEventStubs::order_filled(
            &order,
            &xbtusd_bitmex,
            None,
            None,
            Some(PositionId::from("P-123456")),
            Some(Price::from("10000.0")),
            None,
            Some(commission),
            None,
        );
        let position = Position::new(xbtusd_bitmex, fill).unwrap();
        let pnl = position.calculate_pnl(10000.0, 11000.0, Quantity::from("100000.0"));
        assert_eq!(pnl, Money::from("-0.90909091 BTC"));
        assert_eq!(
            position.unrealized_pnl(Price::from("11000.0")),
            Money::from("-0.90909091 BTC")
        );
        assert_eq!(position.realized_pnl, Some(Money::from("-0.00750000 BTC")));
        assert_eq!(
            position.notional_value(Price::from("11000.0")),
            Money::from("9.09090909 BTC")
        );
    }

    #[rstest]
    fn test_calculate_pnl_for_inverse2(ethusdt_bitmex: CryptoPerpetual) {
        let order = TestOrderStubs::market_order(
            ethusdt_bitmex.id,
            OrderSide::Sell,
            Quantity::from("100000"),
            None,
            None,
        );
        let commission =
            calculate_commission(ethusdt_bitmex, order.quantity, Price::from("375.95"), None)
                .unwrap();
        let fill = TestOrderEventStubs::order_filled(
            &order,
            &ethusdt_bitmex,
            None,
            None,
            Some(PositionId::from("P-123456")),
            Some(Price::from("375.95")),
            None,
            Some(commission),
            None,
        );
        let position = Position::new(ethusdt_bitmex, fill).unwrap();

        assert_eq!(
            position.unrealized_pnl(Price::from("370.00")),
            Money::from("4.27745208 ETH")
        );
        assert_eq!(
            position.notional_value(Price::from("370.00")),
            Money::from("270.27027027 ETH")
        );
    }

    #[rstest]
    fn test_calculate_unrealized_pnl_for_long(currency_pair_btcusdt: CurrencyPair) {
        let order1 = TestOrderStubs::market_order(
            currency_pair_btcusdt.id,
            OrderSide::Buy,
            Quantity::from("2.000000"),
            None,
            None,
        );
        let order2 = TestOrderStubs::market_order(
            currency_pair_btcusdt.id,
            OrderSide::Buy,
            Quantity::from("2.000000"),
            None,
            None,
        );
        let commission1 = calculate_commission(
            currency_pair_btcusdt,
            order1.quantity,
            Price::from("10500.0"),
            None,
        )
        .unwrap();
        let fill1 = TestOrderEventStubs::order_filled(
            &order1,
            &currency_pair_btcusdt,
            Some(StrategyId::from("S-001")),
            Some(TradeId::new("1").unwrap()),
            Some(PositionId::new("P-123456").unwrap()),
            Some(Price::from("10500.00")),
            None,
            Some(commission1),
            None,
        );
        let commission2 = calculate_commission(
            currency_pair_btcusdt,
            order2.quantity,
            Price::from("10500.0"),
            None,
        )
        .unwrap();
        let fill2 = TestOrderEventStubs::order_filled(
            &order2,
            &currency_pair_btcusdt,
            Some(StrategyId::from("S-001")),
            Some(TradeId::new("2").unwrap()),
            Some(PositionId::new("P-123456").unwrap()),
            Some(Price::from("10500.00")),
            None,
            Some(commission2),
            None,
        );
        let mut position = Position::new(currency_pair_btcusdt, fill1).unwrap();
        position.apply(&fill2);
        let pnl = position.unrealized_pnl(Price::from("11505.60"));
        assert_eq!(pnl, Money::from("4022.40000000 USDT"));
        assert_eq!(
            position.realized_pnl,
            Some(Money::from("-42.00000000 USDT"))
        );
        assert_eq!(
            position.commissions(),
            vec![Money::from("42.00000000 USDT")]
        );
    }

    #[rstest]
    fn test_calculate_unrealized_pnl_for_short(currency_pair_btcusdt: CurrencyPair) {
        let order = TestOrderStubs::market_order(
            currency_pair_btcusdt.id,
            OrderSide::Sell,
            Quantity::from("5.912000"),
            None,
            None,
        );
        let commission = calculate_commission(
            currency_pair_btcusdt,
            order.quantity,
            Price::from("10505.60"),
            None,
        )
        .unwrap();
        let fill = TestOrderEventStubs::order_filled(
            &order,
            &currency_pair_btcusdt,
            Some(StrategyId::from("S-001")),
            Some(TradeId::new("1").unwrap()),
            Some(PositionId::new("P-123456").unwrap()),
            Some(Price::from("10505.60")),
            None,
            Some(commission),
            None,
        );
        let position = Position::new(currency_pair_btcusdt, fill).unwrap();
        let pnl = position.unrealized_pnl(Price::from("10407.15"));
        assert_eq!(pnl, Money::from("582.03640000 USDT"));
        assert_eq!(
            position.realized_pnl,
            Some(Money::from("-62.10910720 USDT"))
        );
        assert_eq!(
            position.commissions(),
            vec![Money::from("62.10910720 USDT")]
        );
    }

    #[rstest]
    fn test_calculate_unrealized_pnl_for_long_inverse(xbtusd_bitmex: CryptoPerpetual) {
        let order = TestOrderStubs::market_order(
            xbtusd_bitmex.id,
            OrderSide::Buy,
            Quantity::from("100000"),
            None,
            None,
        );
        let commission =
            calculate_commission(xbtusd_bitmex, order.quantity, Price::from("10500.0"), None)
                .unwrap();
        let fill = TestOrderEventStubs::order_filled(
            &order,
            &xbtusd_bitmex,
            Some(StrategyId::from("S-001")),
            Some(TradeId::new("1").unwrap()),
            Some(PositionId::new("P-123456").unwrap()),
            Some(Price::from("10500.00")),
            None,
            Some(commission),
            None,
        );

        let position = Position::new(xbtusd_bitmex, fill).unwrap();
        let pnl = position.unrealized_pnl(Price::from("11505.60"));
        assert_eq!(pnl, Money::from("0.83238969 BTC"));
        assert_eq!(position.realized_pnl, Some(Money::from("-0.00714286 BTC")));
        assert_eq!(position.commissions(), vec![Money::from("0.00714286 BTC")]);
    }

    #[rstest]
    fn test_calculate_unrealized_pnl_for_short_inverse(xbtusd_bitmex: CryptoPerpetual) {
        let order = TestOrderStubs::market_order(
            xbtusd_bitmex.id,
            OrderSide::Sell,
            Quantity::from("1250000"),
            None,
            None,
        );
        let commission =
            calculate_commission(xbtusd_bitmex, order.quantity, Price::from("15500.00"), None)
                .unwrap();
        let fill = TestOrderEventStubs::order_filled(
            &order,
            &xbtusd_bitmex,
            Some(StrategyId::from("S-001")),
            Some(TradeId::new("1").unwrap()),
            Some(PositionId::new("P-123456").unwrap()),
            Some(Price::from("15500.00")),
            None,
            Some(commission),
            None,
        );
        let position = Position::new(xbtusd_bitmex, fill).unwrap();
        let pnl = position.unrealized_pnl(Price::from("12506.65"));

        assert_eq!(pnl, Money::from("19.30166700 BTC"));
        assert_eq!(position.realized_pnl, Some(Money::from("-0.06048387 BTC")));
        assert_eq!(position.commissions(), vec![Money::from("0.06048387 BTC")]);
    }

    #[rstest]
    #[case(OrderSide::Buy, 25, 25.0)]
    #[case(OrderSide::Sell,25,-25.0)]
    fn test_signed_qty_decimal_qty_for_equity(
        #[case] order_side: OrderSide,
        #[case] quantity: i64,
        #[case] expected: f64,
        audusd_sim: CurrencyPair,
    ) {
        let order = TestOrderStubs::market_order(
            audusd_sim.id,
            order_side,
            Quantity::from(quantity),
            None,
            None,
        );

        let commission =
            calculate_commission(audusd_sim, order.quantity, Price::from("1.0"), None).unwrap();
        let fill = TestOrderEventStubs::order_filled(
            &order,
            &audusd_sim,
            None,
            None,
            Some(PositionId::from("P-123456")),
            None,
            None,
            Some(commission),
            None,
        );
        let position = Position::new(audusd_sim, fill).unwrap();
        assert_eq!(position.signed_qty, expected);
    }
}
