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

use std::{
    cmp::{Reverse, min},
    ops::{Add, ControlFlow},
};

use indexmap::IndexMap;
use nautilus_core::UUID4;
use nautilus_model::{
    accounts::Account,
    enums::{
        BookType, LiquiditySide, OmsType, OrderSide, OrderStatus, OrderType, PriceType, TimeInForce,
    },
    events::{OrderEventAny, OrderFilled},
    identifiers::{
        AccountId, ClientOrderId, InstrumentId, PositionId, VenueOrderId,
        parse_generic_spread_id_legs,
    },
    instruments::{Instrument, InstrumentAny, TickScheme, get_tick_scheme},
    orders::{Order, OrderAny},
    position::Position,
    types::{Currency, Money, Price, Quantity, fixed::FIXED_PRECISION, quantity::QuantityRaw},
};
use rust_decimal::{Decimal, RoundingStrategy, prelude::ToPrimitive};

use super::OrderMatchingEngine;
use crate::models::{
    fee::FeeModel,
    fill::{FillModel, is_price_tradable},
};

impl OrderMatchingEngine {
    pub(super) fn reject_spread_order(&mut self, order: &OrderAny) -> bool {
        if !self.instrument.is_spread() || !self.config.liquidity_consumption {
            return false;
        }

        let reason = "Simulated spread orders do not support liquidity_consumption=true";

        if order.is_open() {
            log::warn!(
                "Canceling spread order {}: {reason}",
                order.client_order_id()
            );
            self.cancel_order(order, None);
        } else {
            self.generate_order_rejected(order, reason.into());
        }

        true
    }

    fn check_spread_fills(
        &mut self,
        order: &OrderAny,
        fills: &[(Price, Quantity)],
    ) -> anyhow::Result<()> {
        if order.time_in_force() != TimeInForce::Fok {
            return Ok(());
        }

        if fills.len() > 1 {
            self.cancel_order(order, None);
            anyhow::bail!("Simulated spread FOK orders do not support multiple executions");
        }

        if let Some(&(price, quantity)) = fills.first()
            && (self
                .normalize_fill_price(price, order.client_order_id())
                .is_none()
                || self
                    .normalize_fill_quantity(quantity, order.client_order_id())
                    .is_none())
        {
            self.cancel_order(order, None);
            anyhow::bail!("Simulated spread FOK fill candidate is not executable");
        }

        Ok(())
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "fill context needs order, position, pricing, and liquidity-source context"
    )]
    pub(super) fn apply_spread_fills(
        &mut self,
        order: &OrderAny,
        fills: &[(Price, Quantity)],
        liquidity_side: LiquiditySide,
        venue_position_id: Option<PositionId>,
        position: Option<&Position>,
        protection_price: Option<Price>,
        from_synthetic: bool,
    ) -> anyhow::Result<()> {
        let reduce_only_remaining = self.reduce_only_remaining(order, position)?;
        self.check_spread_fills(order, fills)?;
        let required_qty = reduce_only_remaining.map_or(order.leaves_qty(), |remaining| {
            order.leaves_qty().min(remaining)
        });

        if self.is_fok_unfillable(order, fills, required_qty) {
            self.cancel_order(order, None);
            return Ok(());
        }

        if fills.is_empty() {
            match (order.status(), order.time_in_force()) {
                (OrderStatus::Initialized | OrderStatus::Submitted, time_in_force)
                    if time_in_force != TimeInForce::Ioc =>
                {
                    self.generate_order_rejected(
                        order,
                        format!("No market for {}", order.instrument_id()).into(),
                    );
                }
                _ => self.cancel_order(order, None),
            }

            return Ok(());
        }

        if reduce_only_remaining.is_some_and(|remaining| remaining.is_zero()) {
            self.cancel_order(order, None);
            return Ok(());
        }

        let venue_position_id = if self.oms_type == OmsType::Netting {
            None
        } else {
            venue_position_id
        };

        let total_filled = self.engine_filled_qty(order);

        let mut context = FillContext {
            order,
            liquidity_side,
            venue_position_id,
            position,
            protection_price,
            from_synthetic,
            initial_market_to_limit: order.order_type() == OrderType::MarketToLimit
                && order.filled_qty().is_zero(),
            initial_total_filled: total_filled,
            total_filled,
            reduce_only_remaining,
            first_price: None,
            last_fill_px: None,
        };

        for &(fill_px, fill_qty) in fills {
            match self.apply_spread_order_fill(&mut context, fill_px, fill_qty, fills.len() == 1)? {
                FillAction::Next => {}
                FillAction::Finish => break,
                FillAction::Return => return Ok(()),
            }
        }

        self.complete_order_fills(&context)
    }

    fn apply_spread_order_fill(
        &mut self,
        context: &mut FillContext<'_>,
        fill_px: Price,
        fill_qty: Quantity,
        single_fill: bool,
    ) -> anyhow::Result<FillAction> {
        let Some(fill_px) = self.normalize_fill_price(fill_px, context.order.client_order_id())
        else {
            return Ok(FillAction::Next);
        };

        let Some(fill_qty) =
            self.normalize_fill_quantity(fill_qty, context.order.client_order_id())
        else {
            return Ok(FillAction::Next);
        };

        if context.initial_market_to_limit
            && let Some(action) = self.prepare_initial_fill_price(context, fill_px, fill_qty)
        {
            return Ok(action);
        }

        let fill_px = match self.fill_price_with_slippage(context, fill_px) {
            Ok(price) => price,
            Err(e) => {
                self.cancel_spread_fill_on_error(context);
                return Err(e);
            }
        };

        let (effective_fill_qty, capped_fill_qty) =
            match self.prepare_fill_quantity(context, fill_qty, single_fill) {
                ControlFlow::Continue(quantities) => quantities,
                ControlFlow::Break(action) => return Ok(action),
            };

        let prepared = match self.prepare_spread_leg_fills(
            context.order,
            capped_fill_qty,
            fill_px,
            context.liquidity_side,
            context.total_filled,
        ) {
            Ok(prepared) => prepared,
            Err(e) => {
                self.abort_spread_fill(context, &e);
                return Ok(FillAction::Return);
            }
        };

        if context.total_filled == context.initial_total_filled
            && let Some(price) = context.first_price
        {
            self.generate_order_updated(
                context.order,
                context.order.quantity(),
                Some(price),
                None,
                None,
                None,
            );
        }

        self.update_fill_progress(context, capped_fill_qty);
        self.fill_spread_order(context, fill_px, effective_fill_qty, prepared)?;

        Ok(self.finish_order_fill(context, fill_px))
    }

    fn fill_spread_remainder(
        &mut self,
        context: &FillContext<'_>,
        fill_px: Price,
        fill_qty: Quantity,
    ) -> anyhow::Result<()> {
        let prepared = match self.prepare_spread_leg_fills(
            context.order,
            fill_qty,
            fill_px,
            context.liquidity_side,
            context.total_filled,
        ) {
            Ok(prepared) => prepared,
            Err(e) => {
                self.abort_spread_fill(context, &e);
                return Ok(());
            }
        };

        self.fill_spread_order(context, fill_px, fill_qty, prepared)?;
        self.purge_cached_filled_qty_if_closed(context.order.client_order_id());
        Ok(())
    }

    fn fill_spread_order(
        &mut self,
        context: &FillContext<'_>,
        fill_px: Price,
        fill_qty: Quantity,
        prepared: PreparedSpreadFill,
    ) -> anyhow::Result<()> {
        let Some((fill_qty, new_filled_qty)) =
            self.cap_order_fill_quantity(context.order, fill_qty)?
        else {
            return Ok(());
        };

        let commission = prepared.commission;
        let (venue_order_id, reduce_only_order_ids) =
            self.begin_order_fill(context.order, new_filled_qty, context.position);

        // Book component exposure before callbacks observe the parent fill
        self.dispatch_spread_leg_fills(
            context.order,
            venue_order_id,
            prepared,
            context.liquidity_side,
        );
        self.generate_order_filled(
            context.order,
            venue_order_id,
            context.venue_position_id,
            fill_qty,
            fill_px,
            self.instrument.quote_currency(),
            commission,
            context.liquidity_side,
        );

        self.complete_order_fill(
            context.order,
            context.venue_position_id,
            context.position,
            reduce_only_order_ids,
        )
    }

    fn cancel_spread_fill_on_error(&mut self, context: &FillContext<'_>) {
        if context.order.time_in_force() == TimeInForce::Fok
            || (context.initial_market_to_limit
                && context.total_filled == context.initial_total_filled)
        {
            self.cancel_order(context.order, None);
        }
    }

    fn abort_spread_fill(&mut self, context: &FillContext<'_>, e: &anyhow::Error) {
        let order = context.order;
        log::error!(
            "Cannot fill spread order {} without leg fills: {e}",
            order.client_order_id()
        );

        let can_rest = match order.order_type() {
            OrderType::MarketToLimit => {
                order.price().is_some()
                    || (context.first_price.is_some()
                        && context.total_filled > context.initial_total_filled)
            }
            OrderType::Limit
            | OrderType::StopLimit
            | OrderType::LimitIfTouched
            | OrderType::TrailingStopLimit => true,
            _ => false,
        } && !matches!(order.time_in_force(), TimeInForce::Ioc | TimeInForce::Fok);

        if !can_rest {
            self.cancel_order(order, None);
        }
    }

    fn prepare_initial_fill_price(
        &self,
        context: &mut FillContext<'_>,
        fill_px: Price,
        fill_qty: Quantity,
    ) -> Option<FillAction> {
        if fill_qty.is_zero() {
            return Some(FillAction::Next);
        }

        if context.first_price.is_some_and(|price| price != fill_px) {
            return Some(FillAction::Finish);
        }

        context.first_price = Some(fill_px);

        None
    }

    fn fill_price_with_slippage(
        &mut self,
        context: &FillContext<'_>,
        fill_px: Price,
    ) -> anyhow::Result<Price> {
        if self.book_type != BookType::L1_MBP {
            return Ok(fill_px);
        }

        if self.fill_model.is_slipped()? {
            return Ok(self
                .slipped_price(fill_px, context.order.order_side())
                .unwrap_or(fill_px));
        }

        Ok(fill_px)
    }

    fn prepare_fill_quantity(
        &self,
        context: &FillContext<'_>,
        fill_qty: Quantity,
        single_fill: bool,
    ) -> ControlFlow<FillAction, (Quantity, Quantity)> {
        let mut effective_fill_qty = fill_qty;

        if let Some(remaining) = context.reduce_only_remaining {
            if remaining.is_zero() {
                return ControlFlow::Break(FillAction::Return);
            }

            if effective_fill_qty > remaining {
                let precision = effective_fill_qty.precision;
                effective_fill_qty = remaining;
                effective_fill_qty.precision = precision;
            }
        }

        if fill_qty.is_zero() {
            if single_fill && context.order.status() == OrderStatus::Submitted {
                self.generate_order_rejected(
                    context.order,
                    format!("No market for {}", context.order.instrument_id()).into(),
                );
            }

            return ControlFlow::Break(FillAction::Return);
        }

        let capped_fill_qty = min(
            effective_fill_qty,
            context
                .order
                .quantity()
                .saturating_sub(context.total_filled),
        );

        if capped_fill_qty.is_zero() {
            return ControlFlow::Break(FillAction::Next);
        }

        ControlFlow::Continue((effective_fill_qty, capped_fill_qty))
    }

    fn update_fill_progress(&self, context: &mut FillContext<'_>, capped_fill_qty: Quantity) {
        if context
            .reduce_only_remaining
            .is_some_and(|remaining| capped_fill_qty >= remaining)
        {
            let mut target = context
                .total_filled
                .checked_add(capped_fill_qty)
                .expect("Overflow occurred when adding reduce-only target quantity");
            target.precision = self.instrument.size_precision();
            if context.order.quantity() != target {
                self.generate_order_updated(context.order, target, None, None, None, None);
            }
        }

        context.total_filled = context.total_filled.add(capped_fill_qty);
        if let Some(remaining) = context.reduce_only_remaining.as_mut() {
            *remaining = *remaining - capped_fill_qty.min(*remaining);
        }
    }

    fn finish_order_fill(&self, context: &mut FillContext<'_>, fill_px: Price) -> FillAction {
        context.last_fill_px = Some(fill_px);
        let exhausted = context
            .reduce_only_remaining
            .is_some_and(|remaining| remaining.is_zero());

        if context.initial_market_to_limit
            && (context.total_filled >= context.order.quantity() || exhausted)
        {
            return FillAction::Return;
        }

        if exhausted {
            self.purge_cached_filled_qty_if_closed(context.order.client_order_id());

            return FillAction::Return;
        }

        FillAction::Next
    }

    fn complete_order_fills(&mut self, context: &FillContext<'_>) -> anyhow::Result<()> {
        let filled_in_loop = context.total_filled > context.initial_total_filled;
        if context.initial_market_to_limit && filled_in_loop {
            return Ok(());
        }

        let leaves_remaining = context.total_filled < context.order.quantity();
        if context.order.time_in_force() == TimeInForce::Ioc && leaves_remaining {
            self.cancel_order(context.order, None);
            return Ok(());
        }

        if leaves_remaining
            && !context.from_synthetic
            && (context.order.is_open() || filled_in_loop)
            && self.book_type == BookType::L1_MBP
            && matches!(
                context.order.order_type(),
                OrderType::Market
                    | OrderType::MarketIfTouched
                    | OrderType::StopMarket
                    | OrderType::TrailingStopMarket
            )
        {
            self.fill_l1_remainder(context)?;
        }

        Ok(())
    }

    fn fill_l1_remainder(&mut self, context: &FillContext<'_>) -> anyhow::Result<()> {
        let Some(last_fill_px) = context.last_fill_px else {
            return Ok(());
        };

        let side = context.order.order_side();

        let Some(slip_fill_px) = self.slipped_price(last_fill_px, side) else {
            return Ok(());
        };

        if let Some(protection_price) = context.protection_price {
            let exceeds_boundary = match side {
                OrderSide::Buy => slip_fill_px > protection_price,
                OrderSide::Sell => slip_fill_px < protection_price,
            };

            if exceeds_boundary {
                return Ok(());
            }
        }

        let Some(leaves_qty) = self.prepare_remainder_quantity(context) else {
            return Ok(());
        };

        self.fill_spread_remainder(context, slip_fill_px, leaves_qty)
    }

    fn prepare_remainder_quantity(&self, context: &FillContext<'_>) -> Option<Quantity> {
        let mut leaves_qty = context
            .order
            .quantity()
            .saturating_sub(context.total_filled);
        if let Some(remaining) = context.reduce_only_remaining {
            if remaining.is_zero() {
                return None;
            }

            if leaves_qty > remaining {
                let precision = leaves_qty.precision;
                leaves_qty = remaining;
                leaves_qty.precision = precision;
            }

            if leaves_qty >= remaining {
                let mut target = context
                    .total_filled
                    .checked_add(leaves_qty)
                    .expect("Overflow occurred when adding reduce-only target quantity");
                target.precision = self.instrument.size_precision();
                if context.order.quantity() != target {
                    self.generate_order_updated(context.order, target, None, None, None, None);
                }
            }
        }

        (!leaves_qty.is_zero()).then_some(leaves_qty)
    }

    fn cap_order_fill_quantity(
        &self,
        order: &OrderAny,
        last_qty: Quantity,
    ) -> anyhow::Result<Option<(Quantity, Quantity)>> {
        self.check_size_precision(last_qty.precision, "fill quantity")?;

        let (last_qty, new_filled_qty) = if let Some(filled_qty) = self
            .orders
            .cached_filled_qty
            .borrow()
            .get(&order.client_order_id())
            .copied()
        {
            let leaves_qty = order.quantity().saturating_sub(filled_qty);
            let last_qty = min(last_qty, leaves_qty);
            (last_qty, filled_qty + last_qty)
        } else {
            let last_qty = min(last_qty, order.quantity());
            (last_qty, last_qty)
        };

        if last_qty.is_zero() {
            return Ok(None);
        }

        Ok(Some((last_qty, new_filled_qty)))
    }

    fn begin_order_fill(
        &mut self,
        order: &OrderAny,
        new_filled_qty: Quantity,
        position: Option<&Position>,
    ) -> (VenueOrderId, Vec<ClientOrderId>) {
        // Resolve implicit membership before dispatch can close the cached position
        let reduce_only_order_ids = position
            .map(|position| self.reduce_only_order_ids(position.id))
            .unwrap_or_default();

        self.orders
            .cached_filled_qty
            .borrow_mut()
            .insert(order.client_order_id(), new_filled_qty);

        let venue_order_id = self.ids_generator.get_venue_order_id(order).unwrap();
        (venue_order_id, reduce_only_order_ids)
    }

    pub(super) fn prepare_spread_leg_fills(
        &mut self,
        order: &OrderAny,
        spread_fill_qty: Quantity,
        spread_fill_px: Price,
        liquidity_side: LiquiditySide,
        pre_fill_qty: Quantity,
    ) -> anyhow::Result<PreparedSpreadFill> {
        let mut commission_order = order.clone();
        super::write_filled_qty(&mut commission_order, pre_fill_qty);
        commission_order.set_liquidity_side(liquidity_side);
        let leg_tuples = parse_generic_spread_id_legs(&order.instrument_id())?;
        anyhow::ensure!(
            leg_tuples.len() > 1,
            "Spread must contain at least two legs"
        );

        let leg_prices = self.calculate_spread_leg_execution_prices(
            &leg_tuples,
            spread_fill_px,
            order.order_side(),
        )?;

        let account_id = order
            .account_id()
            .or_else(|| self.account_ids.get(&order.trader_id()).copied())
            .ok_or_else(|| {
                anyhow::anyhow!("Account ID not found for trader {}", order.trader_id())
            })?;

        self.cache.borrow().try_account(&account_id)?;
        let mut fills = Vec::with_capacity(leg_tuples.len());

        for (leg_position, (leg_instrument_id, ratio)) in leg_tuples.iter().enumerate() {
            let leg_price = leg_prices[leg_instrument_id];

            let leg_instrument = self
                .cache
                .borrow()
                .instrument(leg_instrument_id)
                .cloned()
                .ok_or_else(|| {
                    anyhow::anyhow!("Leg instrument not found in cache: {leg_instrument_id}")
                })?;

            let precision = leg_instrument.size_precision();
            let source_precision = spread_fill_qty.precision.max(FIXED_PRECISION);
            let target_precision = precision.max(FIXED_PRECISION);
            let numerator =
                10_u128.pow(u32::from(target_precision.saturating_sub(source_precision)));
            let denominator =
                10_u128.pow(u32::from(source_precision.saturating_sub(target_precision)));

            #[allow(
                clippy::useless_conversion,
                reason = "QuantityRaw widens from u64 only in standard precision"
            )]
            let quantity = u128::from(spread_fill_qty.raw())
                .checked_mul(u128::from(ratio.unsigned_abs()))
                .and_then(|value| value.checked_mul(numerator))
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "Spread leg quantity overflow for {leg_instrument_id} with ratio {ratio}"
                    )
                })?;

            anyhow::ensure!(
                quantity % denominator == 0,
                "Spread leg {leg_instrument_id} quantity requires rounding",
            );

            #[allow(
                clippy::useless_conversion,
                reason = "QuantityRaw narrows to u64 only in standard precision"
            )]
            let raw: QuantityRaw = (quantity / denominator).try_into().map_err(|_| {
                anyhow::anyhow!(
                    "Spread leg quantity overflow for {leg_instrument_id} with ratio {ratio}"
                )
            })?;

            let leg_quantity = Quantity::from_raw_checked(raw, precision)?;
            anyhow::ensure!(
                !leg_quantity.is_zero(),
                "Spread leg {leg_instrument_id} quantity must be positive",
            );
            let leg_quantity = leg_instrument.try_normalize_qty(leg_quantity)?;
            anyhow::ensure!(
                Quantity::from_decimal_dp(leg_quantity.as_decimal(), precision)
                    .is_ok_and(|quantity| quantity == leg_quantity),
                "Spread leg {leg_instrument_id} quantity exceeds exact position accounting representation",
            );
            anyhow::ensure!(
                !leg_instrument.is_spread(),
                "Nested spread leg {leg_instrument_id} is not supported",
            );

            let order_side = match (order.order_side(), *ratio > 0) {
                (OrderSide::Buy, true) | (OrderSide::Sell, false) => OrderSide::Buy,
                (OrderSide::Sell, true) | (OrderSide::Buy, false) => OrderSide::Sell,
            };

            let client_order_id = ClientOrderId::new_checked(format!(
                "{}-LEG-{}",
                order.client_order_id(),
                leg_instrument_id.symbol
            ))?;
            anyhow::ensure!(
                !self.cache.borrow().order_exists(&client_order_id),
                "Spread leg client order ID {client_order_id} already belongs to an order",
            );

            let underlying_px = self.fee_underlying_price(&leg_instrument)?;

            let commission = self
                .fee_model
                .get_commission_with_context(
                    &commission_order,
                    leg_quantity,
                    leg_price,
                    &leg_instrument,
                    underlying_px,
                )
                .map_err(|e| {
                    anyhow::anyhow!(
                        "Failed to compute commission for spread leg {leg_instrument_id}: {e}"
                    )
                })?;

            fills.push(SpreadLegFill {
                leg_position,
                client_order_id,
                instrument_id: *leg_instrument_id,
                quantity: leg_quantity,
                price: leg_price,
                currency: leg_instrument.quote_currency(),
                side: order_side,
                commission,
            });
        }

        let mut commission = fills[0].commission;

        for fill in &fills[1..] {
            anyhow::ensure!(
                fill.commission.currency == commission.currency,
                "Spread leg commissions must use the same currency",
            );
            commission = commission
                .checked_add(fill.commission)
                .ok_or_else(|| anyhow::anyhow!("Spread leg commission sum overflow"))?;
        }

        self.check_spread_account_currencies(account_id, &fills)?;
        self.order_cash_spread_fills(account_id, &mut fills)?;

        let venue_order_id = self.ids_generator.get_venue_order_id(order)?;
        let cache = self.cache.borrow();

        for fill in &fills {
            let leg_venue_order_id = spread_leg_venue_order_id(venue_order_id, fill.leg_position);
            anyhow::ensure!(
                cache
                    .client_order_id(&leg_venue_order_id)
                    .is_none_or(|owner| *owner == fill.client_order_id),
                "Spread leg venue order ID {leg_venue_order_id} already belongs to another order",
            );
        }

        Ok(PreparedSpreadFill {
            fills,
            account_id,
            commission,
        })
    }

    fn check_spread_account_currencies(
        &self,
        account_id: AccountId,
        fills: &[SpreadLegFill],
    ) -> anyhow::Result<()> {
        let cache = self.cache.borrow();
        let account = cache.try_account(&account_id)?;
        if !account.calculated_account_state() {
            return Ok(());
        }

        for fill in fills {
            let instrument = cache.instrument(&fill.instrument_id).ok_or_else(|| {
                anyhow::anyhow!("Spread leg instrument not found: {}", fill.instrument_id)
            })?;

            if let Some(currency) = account.base_currency() {
                // Validate the PnL currency even when this fill has no realized PnL
                self.convert_spread_amount(
                    Money::zero(instrument.cost_currency()),
                    fill,
                    currency,
                )?;
                self.convert_spread_amount(fill.commission, fill, currency)?;
                continue;
            }

            if account.is_margin_account() {
                let currency = instrument.cost_currency();
                anyhow::ensure!(
                    account.balance_total(Some(currency)).is_some(),
                    "Spread leg cost currency {currency} has no account balance",
                );
            }

            if !fill.commission.is_positive() {
                continue;
            }

            let currency = fill.commission.currency;
            anyhow::ensure!(
                currency == instrument.cost_currency()
                    || account.balance_total(Some(currency)).is_some(),
                "Spread leg commission currency {currency} has no account balance",
            );
        }

        Ok(())
    }

    pub(super) fn dispatch_spread_leg_fills(
        &mut self,
        order: &OrderAny,
        venue_order_id: VenueOrderId,
        prepared: PreparedSpreadFill,
        liquidity_side: LiquiditySide,
    ) {
        let PreparedSpreadFill {
            account_id, fills, ..
        } = prepared;

        let ts_now = self.clock.borrow().timestamp_ns();

        for fill in fills {
            let leg_venue_order_id = spread_leg_venue_order_id(venue_order_id, fill.leg_position);
            let event = OrderEventAny::Filled(OrderFilled::new(
                order.trader_id(),
                order.strategy_id(),
                fill.instrument_id,
                fill.client_order_id,
                leg_venue_order_id,
                account_id,
                self.ids_generator.generate_trade_id(ts_now),
                fill.side,
                order.order_type(),
                fill.quantity,
                fill.price,
                fill.currency,
                liquidity_side,
                UUID4::new(),
                ts_now,
                ts_now,
                false,
                None,
                Some(fill.commission),
                None,
            ));

            self.dispatch_order_event(event);
        }
    }

    fn order_cash_spread_fills(
        &self,
        account_id: AccountId,
        fills: &mut [SpreadLegFill],
    ) -> anyhow::Result<()> {
        let cache = self.cache.borrow();
        let account = cache.try_account(&account_id)?;
        if !account.is_cash_account() || !account.calculated_account_state() {
            return Ok(());
        }

        let base_currency = account.base_currency();
        let mut common_currency = None;
        let commissions_nonnegative = fills.iter().all(|fill| !fill.commission.is_negative());
        let mut debits = Vec::with_capacity(fills.len());

        for fill in fills.iter() {
            let instrument = cache.instrument(&fill.instrument_id).ok_or_else(|| {
                anyhow::anyhow!("Spread leg instrument not found: {}", fill.instrument_id)
            })?;

            let currency = instrument.cost_currency();

            if common_currency.is_some_and(|common| common != currency)
                || (base_currency.is_none()
                    && !commissions_nonnegative
                    && fill.commission.currency != currency)
                || (base_currency.is_none()
                    && instrument
                        .base_currency()
                        .is_some_and(|base| base != currency))
            {
                return Ok(());
            }

            common_currency = Some(currency);
            debits.push(
                self.spread_cash_movement(instrument, fill, base_currency)?
                    .is_negative(),
            );
        }

        fills.sort_by_key(|fill| debits[fill.leg_position]);
        Ok(())
    }

    fn spread_cash_movement(
        &self,
        instrument: &InstrumentAny,
        fill: &SpreadLegFill,
        base_currency: Option<Currency>,
    ) -> anyhow::Result<Money> {
        let product = fill
            .quantity
            .as_decimal()
            .checked_mul(instrument.multiplier().as_decimal())
            .ok_or_else(|| anyhow::anyhow!("Spread leg cash notional overflow"))?;
        let price = fill.price.as_decimal();

        let value = if instrument
            .instrument_class()
            .divides_notional_by_price(instrument.is_inverse())
        {
            anyhow::ensure!(
                price > Decimal::ZERO,
                "Inverse spread leg cash price must be positive"
            );
            product.checked_div(price)
        } else {
            product.checked_mul(price)
        }
        .ok_or_else(|| anyhow::anyhow!("Spread leg cash notional overflow"))?;

        // Cash retains native sub-cent amounts; the public notional API rounds to currency precision
        let notional = Money::from_decimal_native(value, instrument.cost_currency())?;
        let movement = match fill.side {
            OrderSide::Buy => Money::zero(notional.currency).checked_sub(notional),
            OrderSide::Sell => Some(notional),
        };

        let movement =
            movement.ok_or_else(|| anyhow::anyhow!("Spread leg cash movement overflow"))?;
        let currency = base_currency.unwrap_or(notional.currency);

        // Account balances round converted notional and commission separately
        let movement = self.convert_spread_amount(movement, fill, currency)?;

        // Foreign fee debits do not change the native currency cash movement
        let commission = if base_currency.is_none() && fill.commission.currency != currency {
            Money::zero(currency)
        } else {
            self.convert_spread_amount(fill.commission, fill, currency)?
        };

        movement
            .checked_sub(commission)
            .ok_or_else(|| anyhow::anyhow!("Spread leg net cash movement overflow"))
    }

    fn convert_spread_amount(
        &self,
        amount: Money,
        fill: &SpreadLegFill,
        currency: Currency,
    ) -> anyhow::Result<Money> {
        if amount.currency == currency {
            return Ok(amount);
        }

        let price_type = match fill.side {
            OrderSide::Buy => PriceType::Ask,
            OrderSide::Sell => PriceType::Bid,
        };

        let rate = self
            .cache
            .borrow()
            .get_xrate(
                fill.instrument_id.venue,
                amount.currency,
                currency,
                price_type,
            )
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Spread leg cash conversion rate unavailable for {}/{currency}",
                    amount.currency
                )
            })?;

        let value = amount
            .as_decimal()
            .checked_mul(rate)
            .ok_or_else(|| anyhow::anyhow!("Spread leg cash conversion overflow"))?;

        Money::from_decimal_native(value, currency).map_err(Into::into)
    }

    fn calculate_spread_leg_execution_prices(
        &self,
        leg_tuples: &[(InstrumentId, i64)],
        spread_execution_price: Price,
        order_side: OrderSide,
    ) -> anyhow::Result<IndexMap<InstrumentId, Price>> {
        let target = spread_execution_price.as_decimal();
        anyhow::ensure!(
            Price::from_decimal_dp(target, spread_execution_price.precision)
                .is_ok_and(|price| price == spread_execution_price),
            "Spread execution price {spread_execution_price} cannot use exact Decimal arithmetic"
        );
        let legs =
            self.spread_leg_reference_prices(leg_tuples, spread_execution_price, order_side)?;
        let mut anchors = legs
            .iter()
            .map(|(id, leg)| (*id, leg.reference))
            .collect::<Vec<_>>();
        anchors.sort_by_key(|(_, reference)| Reverse(*reference));
        for (anchor_id, _) in anchors {
            if let Some(prices) =
                Self::spread_prices_with_anchor(&legs, leg_tuples, anchor_id, target)
            {
                return Ok(prices);
            }
        }

        if let Some(prices) = Self::adjust_spread_prices_on_tick_grid(&legs, leg_tuples, target)? {
            return Ok(prices);
        }

        anyhow::bail!(
            "Spread execution price {spread_execution_price} cannot be allocated exactly on leg price grids"
        )
    }

    fn spread_leg_reference_prices(
        &self,
        leg_tuples: &[(InstrumentId, i64)],
        spread_execution_price: Price,
        order_side: OrderSide,
    ) -> anyhow::Result<IndexMap<InstrumentId, SpreadLegReference>> {
        let mut quoted_legs = Vec::with_capacity(leg_tuples.len());

        for (leg_instrument_id, ratio) in leg_tuples {
            let cache = self.cache.borrow();

            let quote = cache.quote(leg_instrument_id).copied().ok_or_else(|| {
                anyhow::anyhow!("Quote not found for spread leg {leg_instrument_id}")
            })?;

            let instrument = cache
                .instrument(leg_instrument_id)
                .cloned()
                .ok_or_else(|| {
                    anyhow::anyhow!("Instrument not found for spread leg {leg_instrument_id}")
                })?;

            let (executable, opposite) = if (order_side == OrderSide::Buy) == (*ratio > 0) {
                (quote.ask_price.as_decimal(), quote.bid_price.as_decimal())
            } else {
                (quote.bid_price.as_decimal(), quote.ask_price.as_decimal())
            };

            quoted_legs.push((*leg_instrument_id, *ratio, instrument, executable, opposite));
        }

        // Share improvement by interpolating every leg between its executable and opposite quote
        let weighted = |select: fn(Decimal, Decimal) -> Option<Decimal>| {
            quoted_legs
                .iter()
                .try_fold(Decimal::ZERO, |sum, (_, ratio, _, executable, opposite)| {
                    Decimal::from(*ratio)
                        .checked_mul(select(*executable, *opposite)?)
                        .and_then(|value| sum.checked_add(value))
                })
                .ok_or_else(|| anyhow::anyhow!("Spread leg quote sum overflow"))
        };

        let executable_sum = weighted(|executable, _| Some(executable))?;
        let improvement_range = weighted(|executable, opposite| opposite.checked_sub(executable))?;

        let fraction = if improvement_range.is_zero() {
            Decimal::ZERO
        } else {
            spread_execution_price
                .as_decimal()
                .checked_sub(executable_sum)
                .and_then(|improvement| improvement.checked_div(improvement_range))
                .ok_or_else(|| anyhow::anyhow!("Spread leg fraction overflow"))?
                .clamp(Decimal::ZERO, Decimal::ONE)
        };

        let mut legs = IndexMap::new();

        for (leg_instrument_id, ratio, instrument, executable, opposite) in quoted_legs {
            let reference = opposite
                .checked_sub(executable)
                .and_then(|value| value.checked_mul(fraction))
                .and_then(|offset| executable.checked_add(offset))
                .ok_or_else(|| anyhow::anyhow!("Spread leg reference price overflow"))?;
            legs.insert(
                leg_instrument_id,
                SpreadLegReference {
                    ratio,
                    instrument,
                    reference,
                },
            );
        }

        anyhow::ensure!(
            legs.len() == leg_tuples.len(),
            "Spread leg identifiers must be unique"
        );

        Ok(legs)
    }

    fn spread_prices_with_anchor(
        legs: &IndexMap<InstrumentId, SpreadLegReference>,
        leg_tuples: &[(InstrumentId, i64)],
        anchor_id: InstrumentId,
        target: Decimal,
    ) -> Option<IndexMap<InstrumentId, Price>> {
        let mut prices = IndexMap::new();

        for (id, leg) in legs {
            if *id == anchor_id {
                continue;
            }

            let price = Self::make_spread_leg_price(&leg.instrument, leg.reference).ok()?;
            prices.insert(*id, price);
        }

        let weighted_sum = Self::spread_weighted_sum(
            &prices,
            leg_tuples.iter().filter(|(id, _)| *id != anchor_id),
        )?;
        let SpreadLegReference {
            ratio, instrument, ..
        } = &legs[&anchor_id];
        let value = target
            .checked_sub(weighted_sum)?
            .checked_div(Decimal::from(*ratio))?;
        let price = Self::make_spread_leg_price(instrument, value).ok()?;
        prices.insert(anchor_id, price);
        (Self::spread_weighted_sum(&prices, leg_tuples)? == target).then_some(prices)
    }

    fn adjust_spread_prices_on_tick_grid(
        legs: &IndexMap<InstrumentId, SpreadLegReference>,
        leg_tuples: &[(InstrumentId, i64)],
        target: Decimal,
    ) -> anyhow::Result<Option<IndexMap<InstrumentId, Price>>> {
        let mut prices = IndexMap::new();
        for (id, leg) in legs {
            prices.insert(
                *id,
                Self::make_spread_leg_price(&leg.instrument, leg.reference)?,
            );
        }

        let sum = Self::spread_weighted_sum(&prices, leg_tuples)
            .ok_or_else(|| anyhow::anyhow!("Spread leg weighted sum overflow"))?;
        let residual = target
            .checked_sub(sum)
            .ok_or_else(|| anyhow::anyhow!("Spread leg residual overflow"))?;

        let variable = legs.values().try_fold(false, |variable, leg| {
            Self::spread_leg_fixed_increment(&leg.instrument)
                .map(|increment| variable || increment.is_none())
        })?;

        let mut best_fixed = None;
        let mut best_variable = None;

        for left in 0..leg_tuples.len() {
            for right in (left + 1)..leg_tuples.len() {
                let pair = [leg_tuples[left], leg_tuples[right]];
                let fixed = Self::spread_prices_on_fixed_pair(
                    &prices, legs, leg_tuples, pair, residual, target,
                )?;

                if variable {
                    Self::search_spread_pair(
                        &prices,
                        legs,
                        leg_tuples,
                        pair,
                        target,
                        fixed,
                        &mut best_variable,
                    )?;
                } else if let Some((distance, candidate)) = fixed
                    && best_fixed.as_ref().is_none_or(|(best, _)| distance < *best)
                {
                    best_fixed = Some((distance, candidate));
                }
            }
        }

        Ok(if variable {
            best_variable.map(|(_, prices)| prices)
        } else {
            best_fixed.map(|(_, prices)| prices)
        })
    }

    fn spread_prices_on_fixed_pair(
        prices: &IndexMap<InstrumentId, Price>,
        legs: &IndexMap<InstrumentId, SpreadLegReference>,
        leg_tuples: &[(InstrumentId, i64)],
        pair: [(InstrumentId, i64); 2],
        residual: Decimal,
        target: Decimal,
    ) -> anyhow::Result<Option<(i128, IndexMap<InstrumentId, Price>)>> {
        let [left_increment, right_increment] =
            pair.map(|(id, _)| Self::spread_leg_fixed_increment(&legs[&id].instrument));

        let [Some(left_tick), Some(right_tick)] = [left_increment?, right_increment?] else {
            return Ok(None);
        };

        let ticks = [left_tick, right_tick];

        let [Some(left_coefficient), Some(right_coefficient)] = [
            left_tick.checked_mul(Decimal::from(pair[0].1)),
            right_tick.checked_mul(Decimal::from(pair[1].1)),
        ] else {
            return Ok(None);
        };

        let scale = residual
            .scale()
            .max(left_coefficient.scale())
            .max(right_coefficient.scale());
        let units = [left_coefficient, right_coefficient, residual]
            .map(|value| Self::spread_decimal_units(value, scale));

        let [Some(left_units), Some(right_units), Some(residual_units)] = units else {
            return Ok(None);
        };

        let Some((initial, steps)) =
            Self::spread_pair_solution(left_units, right_units, residual_units)
        else {
            return Ok(None);
        };

        let distance_scale = legs
            .values()
            .map(|leg| u32::from(leg.instrument.price_precision()))
            .max()
            .unwrap_or(0)
            .max(target.scale());
        let tick_scale = left_tick
            .normalize()
            .scale()
            .max(right_tick.normalize().scale());
        let tick_units = ticks.map(|tick| Self::spread_decimal_units(tick, tick_scale));

        let [Some(left_tick), Some(right_tick)] = tick_units else {
            return Ok(None);
        };

        let Some(center) =
            Self::spread_pair_minimum_parameter(initial, steps, [left_tick, right_tick])
        else {
            return Ok(None);
        };

        let parameters =
            Self::spread_pair_parameters(prices, legs, pair, ticks, initial, steps, center);
        let mut best = None;

        for parameter in parameters {
            let Some((distance, candidate)) = Self::spread_pair_candidate(
                prices,
                legs,
                pair,
                ticks,
                initial,
                steps,
                parameter,
                distance_scale,
            ) else {
                continue;
            };

            if Self::spread_weighted_sum(&candidate, leg_tuples) == Some(target)
                && best
                    .as_ref()
                    .is_none_or(|(best_distance, _)| distance < *best_distance)
            {
                best = Some((distance, candidate));
            }
        }

        Ok(best)
    }

    fn spread_pair_parameters(
        prices: &IndexMap<InstrumentId, Price>,
        legs: &IndexMap<InstrumentId, SpreadLegReference>,
        pair: [(InstrumentId, i64); 2],
        ticks: [Decimal; 2],
        initial: [i128; 2],
        steps: [i128; 2],
        center: i128,
    ) -> Vec<i128> {
        let mut parameters = vec![center.saturating_sub(1), center, center.saturating_add(1)];

        for index in 0..2 {
            let (id, _) = pair[index];
            let instrument = &legs[&id].instrument;

            let Some(offset) = Decimal::try_from_i128_with_scale(initial[index], 0)
                .ok()
                .and_then(|value| value.checked_mul(ticks[index]))
            else {
                continue;
            };

            let Some(base) = prices[&id].as_decimal().checked_add(offset) else {
                continue;
            };

            let Some(step) = Decimal::try_from_i128_with_scale(steps[index], 0)
                .ok()
                .and_then(|value| value.checked_mul(ticks[index]))
            else {
                continue;
            };

            if step.is_zero() {
                continue;
            }

            let minimum = instrument.min_price().map_or_else(
                || {
                    if instrument.allows_negative_price() {
                        Price::min(instrument.price_precision()).as_decimal()
                    } else {
                        ticks[index]
                    }
                },
                |price| price.as_decimal(),
            );

            let maximum = instrument
                .max_price()
                .unwrap_or_else(|| Price::max(instrument.price_precision()))
                .as_decimal();

            for boundary in [minimum, maximum] {
                if let Some(center) = boundary
                    .checked_sub(base)
                    .and_then(|value| value.checked_div(step))
                    .and_then(|value| value.trunc().to_i128())
                {
                    parameters.extend([center.saturating_sub(1), center, center.saturating_add(1)]);
                }
            }
        }

        parameters
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "A fixed-grid candidate uses both pair solution and objective scale"
    )]
    fn spread_pair_candidate(
        prices: &IndexMap<InstrumentId, Price>,
        legs: &IndexMap<InstrumentId, SpreadLegReference>,
        pair: [(InstrumentId, i64); 2],
        ticks: [Decimal; 2],
        initial: [i128; 2],
        steps: [i128; 2],
        parameter: i128,
        distance_scale: u32,
    ) -> Option<(i128, IndexMap<InstrumentId, Price>)> {
        let mut candidate = prices.clone();
        let mut distance = 0_i128;

        for index in 0..2 {
            let (id, _) = pair[index];
            let instrument = &legs[&id].instrument;
            let adjustment = steps[index]
                .checked_mul(parameter)?
                .checked_add(initial[index])?;
            let adjustment = Decimal::try_from_i128_with_scale(adjustment, 0)
                .ok()?
                .checked_mul(ticks[index])?;
            let value = prices[&id].as_decimal().checked_add(adjustment)?;
            let price = Self::make_spread_leg_price(instrument, value).ok()?;
            if price.as_decimal() != value {
                return None;
            }

            let units = Self::spread_decimal_units(adjustment, distance_scale)?;
            distance = distance.checked_add(units.checked_mul(units)?)?;
            candidate.insert(id, price);
        }

        Some((distance, candidate))
    }

    fn spread_pair_minimum_parameter(
        initial: [i128; 2],
        steps: [i128; 2],
        ticks: [i128; 2],
    ) -> Option<i128> {
        let left_step = steps[0].checked_mul(ticks[0])?;
        let right_step = steps[1].checked_mul(ticks[1])?;
        let left_initial = initial[0].checked_mul(ticks[0])?;
        let right_initial = initial[1].checked_mul(ticks[1])?;
        let numerator = left_step
            .checked_mul(left_initial)?
            .checked_add(right_step.checked_mul(right_initial)?)?
            .checked_neg()?;
        let denominator = left_step
            .checked_mul(left_step)?
            .checked_add(right_step.checked_mul(right_step)?)?;

        numerator.checked_div(denominator)
    }

    fn search_spread_pair(
        prices: &IndexMap<InstrumentId, Price>,
        legs: &IndexMap<InstrumentId, SpreadLegReference>,
        leg_tuples: &[(InstrumentId, i64)],
        pair: [(InstrumentId, i64); 2],
        target: Decimal,
        fixed: Option<(i128, IndexMap<InstrumentId, Price>)>,
        best: &mut Option<(Decimal, IndexMap<InstrumentId, Price>)>,
    ) -> anyhow::Result<()> {
        if let Some((_, candidate)) = fixed {
            Self::retain_spread_price_candidate(candidate, legs, best);
            return Ok(());
        }

        let [left_increment, right_increment] =
            pair.map(|(id, _)| Self::spread_leg_fixed_increment(&legs[&id].instrument));
        let [left_increment, right_increment] = [left_increment?, right_increment?];

        let (search_id, solve_id) = if left_increment.is_none() {
            (pair[0].0, pair[1].0)
        } else if right_increment.is_none() {
            (pair[1].0, pair[0].0)
        } else {
            return Ok(());
        };

        let Some(search) =
            SpreadPriceSearch::new(prices, legs, leg_tuples, search_id, solve_id, target)
        else {
            return Ok(());
        };

        Self::search_spread_price_ladder(&search, best);
        Ok(())
    }

    fn search_spread_price_ladder(
        search: &SpreadPriceSearch<'_>,
        best: &mut Option<(Decimal, IndexMap<InstrumentId, Price>)>,
    ) {
        let SpreadLegReference {
            instrument,
            reference,
            ..
        } = &search.legs[&search.search_id];

        let Some(value) = reference.to_f64() else {
            return;
        };

        let mut offset = 0_i32;
        loop {
            let bid = instrument.next_bid_price(value, offset);
            let ask = instrument.next_ask_price(value, offset);
            if (bid.is_none() && ask.is_none())
                || Self::spread_search_cannot_improve(search, [bid, ask], best.as_ref())
            {
                break;
            }

            Self::search_spread_price_offset(search, [bid, ask], best);

            let Some(next) = offset.checked_add(1) else {
                break;
            };

            offset = next;
        }
    }

    fn spread_search_cannot_improve(
        search: &SpreadPriceSearch<'_>,
        prices: [Option<Price>; 2],
        best: Option<&(Decimal, IndexMap<InstrumentId, Price>)>,
    ) -> bool {
        let (Some((best_distance, _)), Some(other_distance)) = (best, search.other_distance) else {
            return false;
        };

        let reference = search.legs[&search.search_id].reference;
        prices.into_iter().flatten().all(|price| {
            let difference = price.as_decimal() - reference;
            difference
                .checked_mul(difference)
                .and_then(|squared| other_distance.checked_add(squared))
                .is_some_and(|bound| bound >= *best_distance)
        })
    }

    fn search_spread_price_offset(
        search: &SpreadPriceSearch<'_>,
        prices: [Option<Price>; 2],
        best: &mut Option<(Decimal, IndexMap<InstrumentId, Price>)>,
    ) {
        for (index, price) in prices.into_iter().enumerate() {
            let Some(price) = price else {
                continue;
            };

            if index == 1 && prices[0] == Some(price) {
                continue;
            }

            if let Some(candidate) = Self::solve_spread_search_price(search, price) {
                Self::retain_spread_price_candidate(candidate, search.legs, best);
            }
        }
    }

    fn solve_spread_search_price(
        search: &SpreadPriceSearch<'_>,
        price: Price,
    ) -> Option<IndexMap<InstrumentId, Price>> {
        let SpreadLegReference {
            ratio, instrument, ..
        } = &search.legs[&search.search_id];

        if !is_price_tradable(instrument, price) {
            return None;
        }

        let price = instrument.try_normalize_price(price).ok()?;
        let SpreadLegReference {
            ratio: solve_ratio,
            instrument: solve_instrument,
            ..
        } = &search.legs[&search.solve_id];
        let value = price
            .as_decimal()
            .checked_mul(Decimal::from(*ratio))
            .and_then(|weighted| {
                search
                    .target
                    .checked_sub(search.other_sum)?
                    .checked_sub(weighted)
            })?
            .checked_div(Decimal::from(*solve_ratio))?;

        let solved =
            Self::spread_leg_price_on_grid(solve_instrument, value, search.solve_increment).ok()?;
        if solved.as_decimal() != value {
            return None;
        }

        let mut candidate = search.prices.clone();
        candidate.insert(search.search_id, price);
        candidate.insert(search.solve_id, solved);
        (Self::spread_weighted_sum(&candidate, search.leg_tuples) == Some(search.target))
            .then_some(candidate)
    }

    fn retain_spread_price_candidate(
        prices: IndexMap<InstrumentId, Price>,
        legs: &IndexMap<InstrumentId, SpreadLegReference>,
        best: &mut Option<(Decimal, IndexMap<InstrumentId, Price>)>,
    ) {
        if let Some(distance) = Self::spread_price_distance(&prices, legs, &[])
            && best
                .as_ref()
                .is_none_or(|(best_distance, _)| distance < *best_distance)
        {
            *best = Some((distance, prices));
        }
    }

    fn spread_price_distance(
        prices: &IndexMap<InstrumentId, Price>,
        legs: &IndexMap<InstrumentId, SpreadLegReference>,
        excluded: &[InstrumentId],
    ) -> Option<Decimal> {
        legs.iter().try_fold(Decimal::ZERO, |distance, (id, leg)| {
            if excluded.contains(id) {
                return Some(distance);
            }

            let difference = prices.get(id)?.as_decimal().checked_sub(leg.reference)?;
            difference
                .checked_mul(difference)
                .and_then(|squared| distance.checked_add(squared))
        })
    }

    fn spread_decimal_units(mut value: Decimal, scale: u32) -> Option<i128> {
        value.rescale(scale);
        (value.scale() == scale).then(|| value.mantissa())
    }

    fn spread_pair_solution(
        left: i128,
        right: i128,
        target: i128,
    ) -> Option<([i128; 2], [i128; 2])> {
        let (mut gcd, mut remainder) = (left.checked_abs()?, right.checked_abs()?);
        let (mut left_weight, mut next_left) = (1_i128, 0_i128);
        let (mut right_weight, mut next_right) = (0_i128, 1_i128);

        while remainder != 0 {
            let quotient = gcd / remainder;

            (gcd, remainder) = (
                remainder,
                gcd.checked_sub(quotient.checked_mul(remainder)?)?,
            );

            (left_weight, next_left) = (
                next_left,
                left_weight.checked_sub(quotient.checked_mul(next_left)?)?,
            );

            (right_weight, next_right) = (
                next_right,
                right_weight.checked_sub(quotient.checked_mul(next_right)?)?,
            );
        }

        if gcd == 0 || target % gcd != 0 {
            return None;
        }

        let multiplier = target / gcd;

        let initial = [
            left_weight
                .checked_mul(left.signum())?
                .checked_mul(multiplier)?,
            right_weight
                .checked_mul(right.signum())?
                .checked_mul(multiplier)?,
        ];

        Some((initial, [right / gcd, (left / gcd).checked_neg()?]))
    }

    fn spread_leg_fixed_increment(instrument: &InstrumentAny) -> anyhow::Result<Option<Decimal>> {
        let Some(name) = instrument.tick_scheme() else {
            return Ok(Some(instrument.price_increment().as_decimal()));
        };

        let scheme = get_tick_scheme(name.as_str()).ok_or_else(|| {
            anyhow::anyhow!(
                "Spread leg {} has unknown tick scheme {name}",
                instrument.id()
            )
        })?;

        Ok(match scheme {
            TickScheme::Fixed(scheme) => Some(scheme.tick().as_decimal()),
            TickScheme::Crypto => Some(Decimal::new(1, 2)),
            TickScheme::Tiered(_) | TickScheme::Betfair => None,
        })
    }

    fn spread_weighted_sum<'a>(
        prices: &IndexMap<InstrumentId, Price>,
        leg_tuples: impl IntoIterator<Item = &'a (InstrumentId, i64)>,
    ) -> Option<Decimal> {
        let scale = prices
            .values()
            .map(|price| u32::from(price.precision))
            .max()
            .unwrap_or(0);

        let sum = leg_tuples
            .into_iter()
            .try_fold(0_i128, |sum, (id, ratio)| {
                let price = prices.get(id)?;
                let price_scale = u32::from(price.precision.max(FIXED_PRECISION));
                let factor = 10_i128.checked_pow(scale.abs_diff(price_scale))?;
                #[allow(
                    clippy::useless_conversion,
                    reason = "PriceRaw widens from i64 only in standard precision"
                )]
                let raw = i128::from(price.raw());
                let units = if price_scale > scale {
                    (raw % factor == 0).then_some(raw / factor)?
                } else {
                    raw.checked_mul(factor)?
                };

                sum.checked_add(units.checked_mul(i128::from(*ratio))?)
            })?;

        Self::spread_decimal_from_units(sum, scale)
    }

    fn spread_decimal_from_units(mut units: i128, mut scale: u32) -> Option<Decimal> {
        while scale > 0 && units % 10 == 0 {
            units /= 10;
            scale -= 1;
        }

        Decimal::try_from_i128_with_scale(units, scale).ok()
    }

    fn make_spread_leg_price(instrument: &InstrumentAny, value: Decimal) -> anyhow::Result<Price> {
        Self::spread_leg_price_on_grid(
            instrument,
            value,
            Self::spread_leg_fixed_increment(instrument)?,
        )
    }

    fn spread_leg_price_on_grid(
        instrument: &InstrumentAny,
        value: Decimal,
        increment: Option<Decimal>,
    ) -> anyhow::Result<Price> {
        let Some(increment) = increment else {
            let value_f64 = value.to_f64().ok_or_else(|| {
                anyhow::anyhow!("Spread leg price {value} cannot use tick scheme navigation")
            })?;

            let price = [
                instrument.next_bid_price(value_f64, 0),
                instrument.next_ask_price(value_f64, 0),
            ]
            .into_iter()
            .flatten()
            .filter(|price| is_price_tradable(instrument, *price))
            .filter_map(|price| instrument.try_normalize_price(price).ok())
            .min_by_key(|price| (price.as_decimal() - value).abs())
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Spread leg {} price {value} is outside its tick scheme",
                    instrument.id()
                )
            })?;

            return Ok(price);
        };

        anyhow::ensure!(
            increment > Decimal::ZERO,
            "Spread leg {} has a non-positive price increment",
            instrument.id()
        );
        let ticks = value
            .checked_div(increment)
            .ok_or_else(|| anyhow::anyhow!("Spread leg tick conversion overflow"))?;

        let ticks = if instrument.tick_scheme().is_some() {
            let bid = ticks.floor();
            if ticks - bid <= Decimal::new(5, 1) {
                bid
            } else {
                ticks.ceil()
            }
        } else {
            ticks.round_dp_with_strategy(0, RoundingStrategy::MidpointNearestEven)
        };

        let rounded = ticks
            .checked_mul(increment)
            .ok_or_else(|| anyhow::anyhow!("Spread leg price rounding overflow"))?;
        let price = Price::from_decimal_dp(rounded, instrument.price_precision())?;
        anyhow::ensure!(
            price.as_decimal() == rounded && is_price_tradable(instrument, price),
            "Spread leg {} price {rounded} is not tradable",
            instrument.id()
        );
        instrument.try_normalize_price(price).map_err(Into::into)
    }
}

struct SpreadLegReference {
    ratio: i64,
    instrument: InstrumentAny,
    reference: Decimal,
}

struct SpreadPriceSearch<'a> {
    prices: &'a IndexMap<InstrumentId, Price>,
    legs: &'a IndexMap<InstrumentId, SpreadLegReference>,
    leg_tuples: &'a [(InstrumentId, i64)],
    search_id: InstrumentId,
    solve_id: InstrumentId,
    solve_increment: Option<Decimal>,
    target: Decimal,
    other_sum: Decimal,
    other_distance: Option<Decimal>,
}

impl<'a> SpreadPriceSearch<'a> {
    fn new(
        prices: &'a IndexMap<InstrumentId, Price>,
        legs: &'a IndexMap<InstrumentId, SpreadLegReference>,
        leg_tuples: &'a [(InstrumentId, i64)],
        search_id: InstrumentId,
        solve_id: InstrumentId,
        target: Decimal,
    ) -> Option<Self> {
        let excluded = [search_id, solve_id];
        let other_sum = OrderMatchingEngine::spread_weighted_sum(
            prices,
            leg_tuples.iter().filter(|(id, _)| !excluded.contains(id)),
        )?;
        let other_distance = OrderMatchingEngine::spread_price_distance(prices, legs, &excluded);

        // Copying a tiered scheme per candidate copies its full ladder
        let solve_increment =
            OrderMatchingEngine::spread_leg_fixed_increment(&legs[&solve_id].instrument).ok()?;

        Some(Self {
            prices,
            legs,
            leg_tuples,
            search_id,
            solve_id,
            solve_increment,
            target,
            other_sum,
            other_distance,
        })
    }
}

fn spread_leg_venue_order_id(parent: VenueOrderId, leg_position: usize) -> VenueOrderId {
    VenueOrderId::from(format!("{parent}-LEG-{leg_position}"))
}

#[derive(Debug)]
struct FillContext<'a> {
    order: &'a OrderAny,
    liquidity_side: LiquiditySide,
    venue_position_id: Option<PositionId>,
    position: Option<&'a Position>,
    protection_price: Option<Price>,
    from_synthetic: bool,
    initial_market_to_limit: bool,
    initial_total_filled: Quantity,
    total_filled: Quantity,
    reduce_only_remaining: Option<Quantity>,
    first_price: Option<Price>,
    last_fill_px: Option<Price>,
}

enum FillAction {
    Next,
    Finish,
    Return,
}

#[derive(Debug)]
pub(super) struct PreparedSpreadFill {
    fills: Vec<SpreadLegFill>,
    account_id: AccountId,
    commission: Money,
}

#[derive(Debug)]
struct SpreadLegFill {
    leg_position: usize,
    client_order_id: ClientOrderId,
    instrument_id: InstrumentId,
    quantity: Quantity,
    price: Price,
    currency: Currency,
    side: OrderSide,
    commission: Money,
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[cfg(feature = "high-precision")]
    #[rstest]
    fn test_spread_price_allocator_rejects_weighted_sum_overflow() {
        let leg_tuples = [
            (InstrumentId::from("L0.SIM"), i64::MAX),
            (InstrumentId::from("L1.SIM"), -i64::MAX),
        ];

        let price = Price::from("1000000000000.0000000000000001");
        let prices = leg_tuples.iter().map(|(id, _)| (*id, price)).collect();

        assert_eq!(
            OrderMatchingEngine::spread_weighted_sum(&prices, &leg_tuples),
            None
        );
    }

    #[rstest]
    #[case([9, 8], [2, -3], [5, 7], Some(1))]
    #[case([-9, -8], [2, -3], [5, 7], Some(-1))]
    #[case([i128::MAX, 1], [2, 1], [1, 1], None)]
    #[case([0, 0], [i128::MAX, 1], [1, 1], None)]
    #[case([i128::MIN, 0], [1, 0], [1, 1], None)]
    #[case([1, 2], [0, 0], [3, 4], None)]
    fn test_spread_pair_minimum_parameter(
        #[case] initial: [i128; 2],
        #[case] steps: [i128; 2],
        #[case] ticks: [i128; 2],
        #[case] expected: Option<i128>,
    ) {
        assert_eq!(
            OrderMatchingEngine::spread_pair_minimum_parameter(initial, steps, ticks),
            expected
        );
    }
}
