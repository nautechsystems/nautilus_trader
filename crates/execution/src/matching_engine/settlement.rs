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

use nautilus_core::{UUID4, UnixNanos};
use nautilus_model::{
    enums::{
        LiquiditySide, OptionKind, OrderSide, OrderType, PositionSide, PriceType, TimeInForce,
    },
    events::{OrderEventAny, OrderFilled},
    identifiers::{ClientOrderId, InstrumentId, PositionId, TradeId, VenueOrderId},
    instruments::{Instrument, InstrumentAny},
    orders::{MarketOrder, Order, OrderAny, OrderCore},
    position::Position,
    types::{Money, Price, Quantity},
};
use rust_decimal::Decimal;
use ustr::Ustr;

use super::OrderMatchingEngine;

struct OptionSettlementLeg {
    order: OrderAny,
    venue_order_id: VenueOrderId,
    position_id: Option<PositionId>,
    fill: OrderFilled,
}

struct OptionSettlementPlan {
    legs: Vec<OptionSettlementLeg>,
}

impl OrderMatchingEngine {
    pub(super) fn process_option_expiry(&mut self, ts_now: UnixNanos) -> anyhow::Result<bool> {
        let instrument_id = self.instrument.id();

        let positions: Vec<Position> = {
            let cache = self.cache.borrow();
            cache
                .positions_open(None, Some(&instrument_id), None, None, None)
                .into_iter()
                .map(|p| p.cloned())
                .collect()
        };

        if positions.is_empty() {
            return Ok(true);
        }

        let underlying = match self.instrument.underlying() {
            Some(u) => u,
            None => {
                return Ok(self.option_settlement_retry(
                    "missing-underlying",
                    &format!("No underlying for option {instrument_id}"),
                ));
            }
        };
        let underlying_id = InstrumentId::from(format!("{underlying}.{}", self.venue).as_str());

        let underlying_instrument = {
            let cache = self.cache.borrow();
            cache.instrument(&underlying_id).cloned()
        };

        let underlying_instrument = match underlying_instrument {
            Some(u) => u,
            None => {
                return Ok(self.option_settlement_retry(
                    "missing-underlying-instrument",
                    &format!("No underlying instrument for option {instrument_id}"),
                ));
            }
        };

        // Resolve the underlying price by the underlying's instrument type. An index
        // is disseminated via `IndexPriceUpdate` (it does not trade), so its level is
        // held in the cache's index-price store rather than the trade/quote `price(...)`
        // store; a tradeable underlying (e.g. an equity) keeps the `Last` trade lookup.
        let underlying_price = {
            let cache = self.cache.borrow();
            if matches!(underlying_instrument, InstrumentAny::IndexInstrument(_)) {
                cache.index_price(&underlying_id).map(|ip| ip.value)
            } else {
                cache.price(&underlying_id, PriceType::Last)
            }
        };

        let underlying_price = match underlying_price {
            Some(p) => p,
            None => {
                return Ok(self.option_settlement_retry(
                    "missing-underlying-price",
                    &format!("No underlying price for option {instrument_id}"),
                ));
            }
        };

        let custom_option_price = self.settlement_price;
        let should_exercise = self.option_should_exercise(underlying_price);

        let plan = self.option_create_settlement_plan(
            &positions,
            &underlying_instrument,
            underlying_price,
            should_exercise,
            ts_now,
            custom_option_price,
        );
        self.option_apply_settlement_plan(plan)?;
        Ok(true)
    }

    fn option_settlement_retry(&mut self, reason: &'static str, message: &str) -> bool {
        if self.option_settlement_warning != Some(reason) {
            log::warn!("{message}; settlement will retry");
            self.option_settlement_warning = Some(reason);
        }
        false
    }

    fn option_create_settlement_plan(
        &self,
        positions: &[Position],
        underlying_instrument: &InstrumentAny,
        underlying_price: Price,
        should_exercise: bool,
        ts_now: UnixNanos,
        custom_option_price: Option<Price>,
    ) -> OptionSettlementPlan {
        let mut legs = Vec::new();

        for position in positions {
            if should_exercise {
                self.option_plan_exercise_position(
                    &mut legs,
                    position,
                    underlying_instrument,
                    underlying_price,
                    ts_now,
                    custom_option_price,
                );
            } else {
                legs.push(self.option_plan_otm_expiry(position, ts_now, custom_option_price));
            }
        }
        OptionSettlementPlan { legs }
    }

    fn option_register_settlement_plan(&self, plan: &OptionSettlementPlan) -> anyhow::Result<()> {
        for leg in &plan.legs {
            let client_order_id = leg.order.client_order_id();
            let mut cache = self.cache.borrow_mut();
            cache
                .add_order(leg.order.clone(), leg.position_id, None, false)
                .map_err(|e| {
                    anyhow::anyhow!("cannot add settlement order {client_order_id}: {e}")
                })?;
            cache
                .add_venue_order_id(&client_order_id, &leg.venue_order_id, false)
                .map_err(|e| {
                    anyhow::anyhow!(
                        "cannot claim venue order ID {} for settlement order {client_order_id}: {e}",
                        leg.venue_order_id
                    )
                })?;
        }
        Ok(())
    }

    fn option_apply_settlement_plan(&mut self, plan: OptionSettlementPlan) -> anyhow::Result<()> {
        self.option_register_settlement_plan(&plan)?;

        for leg in &plan.legs {
            self.account_ids
                .insert(leg.fill.trader_id, leg.fill.account_id);
        }

        for leg in &plan.legs {
            self.publish_order_initialized(&leg.order);
        }

        for leg in &plan.legs {
            self.generate_order_accepted(&leg.order, leg.venue_order_id);
        }

        for leg in plan.legs {
            self.dispatch_order_event(OrderEventAny::Filled(leg.fill));
        }

        Ok(())
    }

    fn option_should_exercise(&self, underlying_price: Price) -> bool {
        let strike = match self.instrument.strike_price() {
            Some(p) => p.as_decimal(),
            None => return false,
        };
        let spot = underlying_price.as_decimal();
        match self.instrument.option_kind() {
            Some(OptionKind::Call) => spot > strike,
            Some(OptionKind::Put) => strike > spot,
            None => false,
        }
    }

    fn option_settlement_price(&self, underlying_price: Price, cash_settled: bool) -> Price {
        let strike = self
            .instrument
            .strike_price()
            .expect("option must have strike");
        if !cash_settled {
            return strike;
        }

        let spot = underlying_price.as_decimal();
        let strike_value = strike.as_decimal();
        let value = match self.instrument.option_kind() {
            Some(OptionKind::Call) => (spot - strike_value).max(Decimal::ZERO),
            _ => (strike_value - spot).max(Decimal::ZERO),
        };
        Price::from_decimal_dp(value, strike.precision).expect("Invalid option settlement price")
    }

    fn option_plan_exercise_position(
        &self,
        legs: &mut Vec<OptionSettlementLeg>,
        position: &Position,
        underlying_instrument: &InstrumentAny,
        underlying_price: Price,
        ts_now: UnixNanos,
        custom_option_price: Option<Price>,
    ) {
        if matches!(underlying_instrument, InstrumentAny::IndexInstrument(_)) {
            legs.push(self.option_plan_cash_settlement(
                position,
                underlying_price,
                ts_now,
                custom_option_price,
            ));
        } else {
            legs.extend(self.option_plan_physical_settlement(
                position,
                underlying_instrument,
                underlying_price,
                ts_now,
                custom_option_price,
            ));
        }
    }

    fn option_plan_cash_settlement(
        &self,
        position: &Position,
        underlying_price: Price,
        ts_now: UnixNanos,
        custom_option_price: Option<Price>,
    ) -> OptionSettlementLeg {
        let venue = self.venue;
        let client_order_id = ClientOrderId::from(format!("EXPIRATION-{venue}-{}", UUID4::new()));
        let venue_order_id = VenueOrderId::from(format!("EXPIRATION-{venue}-{}", UUID4::new()));
        let trade_id = TradeId::from(UUID4::new().to_string());
        let close_px = custom_option_price
            .unwrap_or_else(|| self.option_settlement_price(underlying_price, true));
        let close_side = OrderCore::closing_side(position.side);
        let order = self.option_create_settlement_order(
            position,
            self.instrument.id(),
            close_side,
            position.quantity,
            client_order_id,
            true,
            &format!("EXPIRATION_{venue}_CASH"),
        );
        let fill = self.option_create_close_fill(
            position,
            close_px,
            client_order_id,
            venue_order_id,
            trade_id,
            ts_now,
        );
        OptionSettlementLeg {
            order,
            venue_order_id,
            position_id: Some(position.id),
            fill,
        }
    }

    fn option_plan_physical_settlement(
        &self,
        position: &Position,
        underlying_instrument: &InstrumentAny,
        underlying_price: Price,
        ts_now: UnixNanos,
        custom_option_price: Option<Price>,
    ) -> [OptionSettlementLeg; 2] {
        let multiplier = self.instrument.multiplier();
        let underlying_qty = Quantity::from_decimal_dp(
            position.quantity.as_decimal() * multiplier.as_decimal(),
            underlying_instrument.size_precision(),
        )
        .expect("Invalid underlying settlement quantity");

        let underlying_side = if self.instrument.option_kind() == Some(OptionKind::Call) {
            position.side
        } else {
            match position.side {
                PositionSide::Long => PositionSide::Short,
                PositionSide::Short => PositionSide::Long,
                other => other,
            }
        };

        let venue = self.venue;
        let close_client_order_id =
            ClientOrderId::from(format!("EXPIRATION-{venue}-{}", UUID4::new()));
        let close_venue_order_id =
            VenueOrderId::from(format!("EXPIRATION-{venue}-{}", UUID4::new()));
        let close_trade_id = TradeId::from(UUID4::new().to_string());
        let open_client_order_id =
            ClientOrderId::from(format!("EXPIRATION-{venue}-{}", UUID4::new()));
        let open_venue_order_id =
            VenueOrderId::from(format!("EXPIRATION-{venue}-{}", UUID4::new()));
        let open_trade_id = TradeId::from(UUID4::new().to_string());
        let settlement_px = self.option_settlement_price(underlying_price, false);
        let option_close_px =
            custom_option_price.unwrap_or_else(|| Price::zero(self.instrument.price_precision()));
        let close_side = OrderCore::closing_side(position.side);
        let underlying_order_side = match underlying_side {
            PositionSide::Long => OrderSide::Buy,
            _ => OrderSide::Sell,
        };

        let close_order = self.option_create_settlement_order(
            position,
            self.instrument.id(),
            close_side,
            position.quantity,
            close_client_order_id,
            true,
            &format!("EXPIRATION_{venue}_PHYSICAL_CLOSE"),
        );
        let open_order = self.option_create_settlement_order(
            position,
            underlying_instrument.id(),
            underlying_order_side,
            underlying_qty,
            open_client_order_id,
            false,
            &format!("EXPIRATION_{venue}_PHYSICAL_OPEN"),
        );

        let option_fill = self.option_create_close_fill(
            position,
            option_close_px,
            close_client_order_id,
            close_venue_order_id,
            close_trade_id,
            ts_now,
        );
        let underlying_fill = self.option_create_underlying_fill(
            position,
            underlying_instrument,
            underlying_qty,
            underlying_side,
            settlement_px,
            open_client_order_id,
            open_venue_order_id,
            open_trade_id,
            ts_now,
        );
        [
            OptionSettlementLeg {
                order: close_order,
                venue_order_id: close_venue_order_id,
                position_id: Some(position.id),
                fill: option_fill,
            },
            OptionSettlementLeg {
                order: open_order,
                venue_order_id: open_venue_order_id,
                position_id: None,
                fill: underlying_fill,
            },
        ]
    }

    fn option_plan_otm_expiry(
        &self,
        position: &Position,
        ts_now: UnixNanos,
        custom_option_price: Option<Price>,
    ) -> OptionSettlementLeg {
        let venue = self.venue;
        let client_order_id = ClientOrderId::from(format!("EXPIRATION-{venue}-{}", UUID4::new()));
        let venue_order_id = VenueOrderId::from(format!("EXPIRATION-{venue}-{}", UUID4::new()));
        let trade_id = TradeId::from(UUID4::new().to_string());
        let close_px =
            custom_option_price.unwrap_or_else(|| Price::zero(self.instrument.price_precision()));
        let close_side = OrderCore::closing_side(position.side);
        let order = self.option_create_settlement_order(
            position,
            self.instrument.id(),
            close_side,
            position.quantity,
            client_order_id,
            true,
            &format!("EXPIRATION_{venue}_OTM"),
        );
        let fill = self.option_create_close_fill(
            position,
            close_px,
            client_order_id,
            venue_order_id,
            trade_id,
            ts_now,
        );
        OptionSettlementLeg {
            order,
            venue_order_id,
            position_id: Some(position.id),
            fill,
        }
    }

    #[expect(clippy::too_many_arguments)]
    fn option_create_settlement_order(
        &self,
        position: &Position,
        instrument_id: InstrumentId,
        order_side: OrderSide,
        quantity: Quantity,
        client_order_id: ClientOrderId,
        reduce_only: bool,
        tag: &str,
    ) -> OrderAny {
        let ts_now = self.clock.borrow().timestamp_ns();
        OrderAny::Market(MarketOrder::new(
            position.trader_id,
            position.strategy_id,
            instrument_id,
            client_order_id,
            order_side,
            quantity,
            TimeInForce::Gtc,
            UUID4::new(),
            ts_now,
            reduce_only,
            false,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            Some(vec![Ustr::from(tag)]),
        ))
    }

    fn option_create_close_fill(
        &self,
        position: &Position,
        price: Price,
        client_order_id: ClientOrderId,
        venue_order_id: VenueOrderId,
        trade_id: TradeId,
        ts_now: UnixNanos,
    ) -> OrderFilled {
        let close_side = OrderCore::closing_side(position.side);
        OrderFilled::new(
            position.trader_id,
            position.strategy_id,
            self.instrument.id(),
            client_order_id,
            venue_order_id,
            position.account_id,
            trade_id,
            close_side,
            OrderType::Market,
            position.quantity,
            price,
            self.instrument.quote_currency(),
            LiquiditySide::Taker,
            UUID4::new(),
            ts_now,
            ts_now,
            false,
            Some(position.id),
            Some(Money::zero(self.instrument.quote_currency())),
            None,
        )
    }

    #[expect(clippy::too_many_arguments)]
    fn option_create_underlying_fill(
        &self,
        position: &Position,
        underlying_instrument: &InstrumentAny,
        quantity: Quantity,
        side: PositionSide,
        price: Price,
        client_order_id: ClientOrderId,
        venue_order_id: VenueOrderId,
        trade_id: TradeId,
        ts_now: UnixNanos,
    ) -> OrderFilled {
        let order_side = match side {
            PositionSide::Long => OrderSide::Buy,
            _ => OrderSide::Sell,
        };
        OrderFilled::new(
            position.trader_id,
            position.strategy_id,
            underlying_instrument.id(),
            client_order_id,
            venue_order_id,
            position.account_id,
            trade_id,
            order_side,
            OrderType::Market,
            quantity,
            price,
            underlying_instrument.quote_currency(),
            LiquiditySide::Taker,
            UUID4::new(),
            ts_now,
            ts_now,
            false,
            None,
            Some(Money::zero(underlying_instrument.quote_currency())),
            None,
        )
    }
}
