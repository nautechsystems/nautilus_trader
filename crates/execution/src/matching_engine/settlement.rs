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

use ahash::AHashMap;
use nautilus_core::{UUID4, UnixNanos};
use nautilus_model::{
    accounts::Account,
    enums::{
        LiquiditySide, OmsType, OptionKind, OrderSide, OrderType, PriceType, SettlementType,
        TimeInForce,
    },
    events::{OrderEventAny, OrderFilled},
    identifiers::{
        AccountId, ClientId, ClientOrderId, InstrumentId, PositionId, TradeId, VenueOrderId,
    },
    instruments::{Instrument, InstrumentAny},
    orders::{MarketOrder, Order, OrderAny, OrderCore},
    position::Position,
    types::{Money, Price, Quantity, fixed::FIXED_PRECISION, quantity::QuantityRaw},
};
use rust_decimal::Decimal;
use ustr::Ustr;

use super::OrderMatchingEngine;
use crate::engine::netting_position_id;

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

        let (underlying_id, settlement_type) = match &self.instrument {
            InstrumentAny::OptionContract(option) => {
                (option.underlying_instrument_id, option.settlement_type)
            }
            InstrumentAny::CryptoOption(option) => {
                (option.underlying_instrument_id, option.settlement_type)
            }
            _ => unreachable!("Option expiry requires a vanilla option"),
        };

        let Some(settlement_type) = settlement_type else {
            return Ok(self.option_settlement_retry(
                "missing-settlement-type",
                &format!("No settlement type for option {instrument_id}"),
            ));
        };

        let Some(underlying_id) = underlying_id else {
            return Ok(self.option_settlement_retry(
                "missing-underlying-id",
                &format!("No underlying instrument ID for option {instrument_id}"),
            ));
        };

        anyhow::ensure!(
            underlying_id != instrument_id,
            "Option {instrument_id} cannot be its own underlying"
        );

        let underlying_instrument = self.cache.borrow().instrument(&underlying_id).cloned();

        let Some(underlying_instrument) = underlying_instrument else {
            return Ok(self.option_settlement_retry(
                "missing-underlying-instrument",
                &format!(
                    "Underlying instrument {underlying_id} for option {instrument_id} is not cached"
                ),
            ));
        };

        if settlement_type == SettlementType::Physical {
            anyhow::ensure!(
                !matches!(underlying_instrument, InstrumentAny::IndexInstrument(_)),
                "Cannot physically deliver reference index {underlying_id}"
            );
            anyhow::ensure!(
                !underlying_instrument.is_spread(),
                "Cannot physically deliver spread underlying {underlying_id}"
            );
        }

        let underlying_currency = underlying_instrument.quote_currency();
        let strike_currency = self
            .instrument
            .strike_currency()
            .expect("Option must have strike currency");
        anyhow::ensure!(
            underlying_currency == strike_currency,
            "Underlying {underlying_id} is priced in {underlying_currency}, option {instrument_id} has a {strike_currency} strike"
        );

        // Index levels arrive as IndexPriceUpdate events, separately from trade prices
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
                    &format!("No price for underlying {underlying_id} of option {instrument_id}"),
                ));
            }
        };

        // Inverse settlement divides by the spot, so an invalid level defers like a missing one
        if self.instrument.is_inverse() && !underlying_price.is_positive() {
            return Ok(self.option_settlement_retry(
                "non-positive-underlying-price",
                &format!(
                    "Non-positive underlying price {underlying_price} for inverse option {instrument_id}"
                ),
            ));
        }

        let option_close_price = self
            .instrument_close
            .as_ref()
            .map(|close| close.close_price);

        let plan = self.option_create_settlement_plan(
            &positions,
            &underlying_instrument,
            underlying_price,
            settlement_type,
            ts_now,
            option_close_price,
        )?;
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
        settlement_type: SettlementType,
        ts_now: UnixNanos,
        option_close_price: Option<Price>,
    ) -> anyhow::Result<OptionSettlementPlan> {
        let should_exercise = self.option_should_exercise(underlying_price);
        if settlement_type == SettlementType::Physical && should_exercise {
            let mut legs = Vec::with_capacity(positions.len() * 2);
            let mut netting_owners = AHashMap::new();

            for position in positions {
                let client_id =
                    self.option_position_client_id(position, underlying_instrument.id());
                self.validate_option_delivery_netting(
                    position,
                    underlying_instrument.id(),
                    client_id,
                    &mut netting_owners,
                )?;
                legs.extend(self.option_plan_physical_settlement(
                    position,
                    underlying_instrument,
                    client_id,
                    ts_now,
                )?);
            }

            return Ok(OptionSettlementPlan { legs });
        }

        if settlement_type == SettlementType::Cash {
            let settlement_currency = self.instrument.settlement_currency();
            anyhow::ensure!(
                self.instrument.quote_currency() == settlement_currency
                    && self.instrument.cost_currency() == settlement_currency,
                "Currency conversion is required for cash settlement of {} in {settlement_currency}",
                self.instrument.id()
            );
        }

        let price = match (option_close_price, should_exercise) {
            (Some(price), _) => price,
            (None, true) => {
                self.option_cash_settlement_price(underlying_price, underlying_instrument)?
            }
            (None, false) => Price::zero(self.instrument.price_precision()),
        };

        let tag = if should_exercise { "CASH" } else { "OTM" };
        let legs = positions
            .iter()
            .map(|position| self.option_plan_close(position, price, ts_now, tag))
            .collect();

        Ok(OptionSettlementPlan { legs })
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
            self.generate_order_accepted(&leg.order, leg.fill.venue_order_id);
        }

        for leg in plan.legs {
            self.dispatch_order_event(OrderEventAny::Filled(leg.fill));
        }

        Ok(())
    }

    fn option_register_settlement_plan(&self, plan: &OptionSettlementPlan) -> anyhow::Result<()> {
        for leg in &plan.legs {
            let client_order_id = leg.order.client_order_id();
            let mut cache = self.cache.borrow_mut();
            cache
                .add_order(
                    leg.order.clone(),
                    leg.fill.position_id,
                    leg.client_id,
                    false,
                )
                .map_err(|e| {
                    anyhow::anyhow!("cannot add settlement order {client_order_id}: {e}")
                })?;

            cache
                .add_venue_order_id(&client_order_id, &leg.fill.venue_order_id, false)
                .map_err(|e| {
                    anyhow::anyhow!(
                        "cannot claim venue order ID {} for settlement order {client_order_id}: {e}",
                        leg.fill.venue_order_id
                    )
                })?;
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

    fn option_cash_settlement_price(
        &self,
        underlying_price: Price,
        underlying_instrument: &InstrumentAny,
    ) -> anyhow::Result<Price> {
        let strike = self
            .instrument
            .strike_price()
            .expect("Option must have strike");
        let spot = underlying_price.as_decimal();
        let strike_value = strike.as_decimal();
        let intrinsic = match self.instrument.option_kind() {
            Some(OptionKind::Call) => (spot - strike_value).max(Decimal::ZERO),
            _ => (strike_value - spot).max(Decimal::ZERO),
        };

        let settlement_currency = self.instrument.settlement_currency();

        if self.instrument.is_inverse() {
            if !matches!(underlying_instrument, InstrumentAny::IndexInstrument(_)) {
                let base_currency = underlying_instrument.base_currency().ok_or_else(|| {
                    anyhow::anyhow!(
                        "Underlying {} has no base currency for inverse settlement",
                        underlying_instrument.id()
                    )
                })?;

                anyhow::ensure!(
                    base_currency == settlement_currency,
                    "Cannot settle {} in {settlement_currency} using underlying base currency {base_currency}",
                    self.instrument.id()
                );
            }

            // Convert the strike-currency payout to the premium currency at settlement spot
            let value = intrinsic.checked_div(spot).ok_or_else(|| {
                anyhow::anyhow!(
                    "Cannot convert intrinsic value {intrinsic} at settlement spot {spot}"
                )
            })?;

            return Ok(Price::from_decimal_dp(
                value,
                self.instrument.price_precision(),
            )?);
        }

        anyhow::ensure!(
            self.instrument.strike_currency() == Some(settlement_currency),
            "Cannot settle intrinsic value of {} in {settlement_currency} without converting its strike currency",
            self.instrument.id()
        );
        Ok(Price::from_decimal_dp(
            intrinsic,
            strike.precision.max(underlying_price.precision),
        )?)
    }

    fn option_plan_physical_settlement(
        &self,
        position: &Position,
        underlying_instrument: &InstrumentAny,
        client_id: Option<ClientId>,
        ts_now: UnixNanos,
    ) -> anyhow::Result<[OptionSettlementLeg; 2]> {
        let underlying_id = underlying_instrument.id();
        let account_type = self
            .cache
            .borrow()
            .account(&position.account_id)
            .map_or(self.account_type, |account| account.account_type());
        Self::validate_account_instrument(account_type, underlying_instrument)?;
        if let Some(expiration) = underlying_instrument.expiration_ns()
            && expiration != UnixNanos::default()
            && expiration < ts_now
        {
            anyhow::bail!("Cannot deliver {underlying_id}: underlying expired at {expiration}");
        }

        let multiplier = self.instrument.multiplier();
        let position_qty = position.quantity.as_decimal();
        let underlying_multiplier = underlying_instrument.multiplier();
        let underlying_precision = underlying_instrument.size_precision();
        let exercise_qty = position_qty
            .checked_mul(multiplier.as_decimal())
            .ok_or_else(|| anyhow::anyhow!("Option exercise quantity exceeds decimal range"))?;

        let delivery = exercise_qty.checked_div(underlying_multiplier.as_decimal()).ok_or_else(|| {
            anyhow::anyhow!("Cannot divide exercise quantity {exercise_qty} by underlying multiplier {underlying_multiplier}")
        })?;

        let underlying_qty = Quantity::from_decimal_dp(delivery, underlying_precision)?;
        let precision = FIXED_PRECISION
            .max(position.quantity.precision)
            .max(multiplier.precision)
            .max(underlying_qty.precision)
            .max(underlying_multiplier.precision);

        let scaled = |qty: Quantity| {
            let extra = u32::from(precision - qty.precision.max(FIXED_PRECISION));
            qty.raw() * QuantityRaw::pow(10, extra)
        };

        // Check the full fixed-point products because Decimal multiplication can round
        if !underlying_qty.is_positive()
            || scaled(underlying_qty).carrying_mul(scaled(underlying_multiplier), 0)
                != scaled(position.quantity).carrying_mul(scaled(multiplier), 0)
        {
            anyhow::bail!(
                "cannot deliver {delivery} {underlying_id} at size precision {underlying_precision}"
            );
        }

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
        let settlement_px = self
            .instrument
            .strike_price()
            .expect("Option must have strike");

        // Delivery at strike carries the intrinsic value, so the option leg closes at zero
        // whatever the `InstrumentClose` price.
        let option_close_px = Price::zero(self.instrument.price_precision());
        let close_side = OrderCore::closing_side(position.side)
            .expect("Settlement position must be Long or Short");

        let underlying_order_side = if self.instrument.option_kind() == Some(OptionKind::Call) {
            OrderCore::opposite_side(close_side)
        } else {
            close_side
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
            underlying_order_side,
            settlement_px,
            open_client_order_id,
            open_venue_order_id,
            open_trade_id,
            ts_now,
        );
        Ok([
            OptionSettlementLeg {
                order: close_order,
                fill: option_fill,
                client_id,
            },
            OptionSettlementLeg {
                order: open_order,
                fill: underlying_fill,
                client_id,
            },
        ])
    }

    fn validate_option_delivery_netting(
        &self,
        position: &Position,
        underlying_id: InstrumentId,
        client_id: Option<ClientId>,
        netting_owners: &mut AHashMap<PositionId, AccountId>,
    ) -> anyhow::Result<()> {
        let cache = self.cache.borrow();
        if cache.resolve_oms_type(position.strategy_id, client_id) == OmsType::Hedging {
            return Ok(());
        }

        let delivery_id = netting_position_id(underlying_id, position.strategy_id);
        let planned_owner = netting_owners
            .entry(delivery_id)
            .or_insert(position.account_id);
        anyhow::ensure!(
            *planned_owner == position.account_id,
            "Cannot deliver {underlying_id} to {}: settlement already plans netting position {delivery_id} for account {planned_owner}",
            position.account_id,
        );

        if let Some(existing) = cache.position_ref(&delivery_id)
            && existing.is_open()
            && existing.account_id != position.account_id
        {
            anyhow::bail!(
                "Cannot deliver {underlying_id} to {}: netting position {} belongs to account {}",
                position.account_id,
                existing.id,
                existing.account_id,
            );
        }

        self.validate_pending_option_deliveries(position, underlying_id)
    }

    fn validate_pending_option_deliveries(
        &self,
        position: &Position,
        underlying_id: InstrumentId,
    ) -> anyhow::Result<()> {
        let cache = self.cache.borrow();
        for order in cache.orders_refs(
            None,
            Some(&underlying_id),
            Some(&position.strategy_id),
            None,
            None,
        ) {
            let client_order_id = order.client_order_id();
            if order.is_closed()
                || !client_order_id.as_str().starts_with("EXPIRATION-")
                || !order
                    .tags()
                    .is_some_and(|tags| tags.iter().any(|tag| tag.ends_with("_PHYSICAL_OPEN")))
            {
                continue;
            }

            let client_id = cache.client_id(&client_order_id);
            if cache.resolve_oms_type(order.strategy_id(), client_id.copied()) == OmsType::Hedging {
                continue;
            }

            let pending_account = order.account_id().or_else(|| {
                client_id
                    .and_then(|id| cache.account_id_for_client(id))
                    .copied()
            });

            anyhow::ensure!(
                pending_account == Some(position.account_id),
                "Cannot deliver {underlying_id} to {}: pending netting delivery {client_order_id} has a different or unknown account",
                position.account_id,
            );
        }

        Ok(())
    }

    fn option_plan_close(
        &self,
        position: &Position,
        close_px: Price,
        ts_now: UnixNanos,
        tag: &str,
    ) -> OptionSettlementLeg {
        let venue = self.venue;
        let client_order_id = ClientOrderId::from(format!("EXPIRATION-{venue}-{}", UUID4::new()));
        let venue_order_id = VenueOrderId::from(format!("EXPIRATION-{venue}-{}", UUID4::new()));
        let trade_id = TradeId::from(UUID4::new().to_string());
        let close_side = OrderCore::closing_side(position.side)
            .expect("Settlement position must be Long or Short");
        let order = self.option_create_settlement_order(
            position,
            self.instrument.id(),
            close_side,
            position.quantity,
            client_order_id,
            true,
            &format!("EXPIRATION_{venue}_{tag}"),
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
            fill,
            client_id: self.option_position_client_id(position, self.instrument.id()),
        }
    }

    fn option_position_client_id(
        &self,
        position: &Position,
        instrument_id: InstrumentId,
    ) -> Option<ClientId> {
        let cache = self.cache.borrow();
        cache
            .client_id(&position.opening_order_id)
            .filter(|id| cache.account_id_for_client(id) == Some(&position.account_id))
            .copied()
            .or_else(|| {
                cache
                    .client_id_for_venue(&self.venue)
                    .filter(|id| cache.account_id_for_client(id) == Some(&position.account_id))
                    .copied()
            })
            .or_else(|| {
                cache.unique_client_id_for_account(position.account_id, Some(instrument_id.venue))
            })
            .or_else(|| cache.unique_client_id_for_account(position.account_id, None))
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
        let close_side = OrderCore::closing_side(position.side)
            .expect("Settlement position must be Long or Short");

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
        order_side: OrderSide,
        price: Price,
        client_order_id: ClientOrderId,
        venue_order_id: VenueOrderId,
        trade_id: TradeId,
        ts_now: UnixNanos,
    ) -> OrderFilled {
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

struct OptionSettlementLeg {
    order: OrderAny,
    fill: OrderFilled,
    client_id: Option<ClientId>,
}

struct OptionSettlementPlan {
    legs: Vec<OptionSettlementLeg>,
}
