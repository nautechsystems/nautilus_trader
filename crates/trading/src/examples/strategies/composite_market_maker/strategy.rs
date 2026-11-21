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

//! Composite market making strategy implementation.

use std::fmt::Debug;

use ahash::AHashSet;
use anyhow::Context;
use nautilus_common::actor::DataActor;
use nautilus_core::DurationNanos;
use nautilus_model::{
    data::QuoteTick,
    enums::{OrderSide, OrderStatus, TimeInForce},
    events::{OrderCanceled, OrderExpired, OrderFilled, OrderRejected},
    identifiers::{ClientOrderId, StrategyId},
    instruments::{Instrument, InstrumentAny},
    orders::Order,
    types::{Price, Quantity},
};
use rust_decimal::Decimal;

use super::{
    super::{price_change_reaches_threshold, quote_midpoint},
    config::CompositeMarketMakerConfig,
};
use crate::{
    nautilus_strategy,
    strategy::{Strategy, StrategyCore},
};

/// Composite market making strategy with book-mid quoting and signal-driven skew.
///
/// Quotes a single bid and a single ask around the target instrument's book mid.
/// A second instrument (typically a `SyntheticInstrument`) supplies a signal
/// whose residual against a baseline shifts both sides up or down. Inventory
/// skew shifts both sides in the opposite direction of the current position.
/// With nonzero signal skew, quoting waits for a valid signal quote and baseline.
/// Orders persist across ticks and are only replaced when either the anchor
/// or the signal residual's price impact (`signal_skew_factor * residual`)
/// moves by at least `requote_threshold_bps` of the anchor.
pub struct CompositeMarketMaker {
    pub(super) core: StrategyCore,
    pub(super) config: CompositeMarketMakerConfig,
    pub(super) instrument: Option<InstrumentAny>,
    pub(super) trade_size: Option<Quantity>,
    pub(super) last_quoted_anchor: Option<Decimal>,
    pub(super) last_quoted_residual: Option<Decimal>,
    pub(super) signal_baseline: Option<Decimal>,
    pub(super) last_signal: Option<Decimal>,
    pub(super) pending_self_cancels: AHashSet<ClientOrderId>,
}

impl CompositeMarketMaker {
    /// Creates a new [`CompositeMarketMaker`] instance from config.
    #[must_use]
    pub fn new(config: CompositeMarketMakerConfig) -> Self {
        let signal_baseline = config
            .signal_baseline
            .and_then(|value| Decimal::try_from(value).ok());

        Self {
            core: StrategyCore::new(config.base.clone()),
            instrument: None,
            trade_size: config.trade_size,
            config,
            last_quoted_anchor: None,
            last_quoted_residual: None,
            signal_baseline,
            last_signal: None,
            pending_self_cancels: AHashSet::new(),
        }
    }

    pub(super) fn should_requote_on_anchor(&self, anchor: Decimal) -> bool {
        match self.last_quoted_anchor {
            Some(last) => {
                if last.is_zero() {
                    return true;
                }

                price_change_reaches_threshold(
                    (anchor - last).abs(),
                    last,
                    self.config.requote_threshold_bps,
                )
            }
            None => true,
        }
    }

    pub(super) fn should_requote_on_residual(&self, residual: Decimal, anchor: Decimal) -> bool {
        let Ok(skew) = Decimal::try_from(self.config.signal_skew_factor) else {
            return false;
        };

        if skew.is_zero() {
            return false;
        }

        if anchor.is_zero() {
            return false;
        }

        match self.last_quoted_residual {
            Some(last) => {
                // The signal's contribution to bid/ask price is signal_skew_factor * residual,
                // so a residual delta translates to that price units. Compare against the same
                // bps threshold expressed in price units of the anchor.
                residual
                    .checked_sub(last)
                    .and_then(|delta| delta.abs().checked_mul(skew.abs()))
                    .is_some_and(|delta| {
                        price_change_reaches_threshold(
                            delta,
                            anchor,
                            self.config.requote_threshold_bps,
                        )
                    })
            }
            None => true,
        }
    }

    pub(super) fn should_requote(&self, anchor: Decimal, residual: Decimal) -> bool {
        self.should_requote_on_anchor(anchor) || self.should_requote_on_residual(residual, anchor)
    }

    pub(super) fn signal_residual(&self) -> Option<Decimal> {
        if self.config.signal_skew_factor == 0.0 {
            return Some(Decimal::ZERO);
        }

        let signal = self.last_signal?;
        let baseline = self.signal_baseline?;
        if signal <= Decimal::ZERO || baseline <= Decimal::ZERO {
            return None;
        }

        signal.checked_sub(baseline)?.checked_div(baseline)
    }

    pub(super) fn compute_quotes(
        &self,
        anchor: Decimal,
        signal_residual: Decimal,
        net_position: Decimal,
        worst_long: Decimal,
        worst_short: Decimal,
    ) -> anyhow::Result<Vec<(OrderSide, Price)>> {
        let Some(instrument) = self.instrument.as_ref() else {
            anyhow::bail!("Cannot compute quotes: instrument is not resolved");
        };
        let Some(trade_size) = self.trade_size else {
            anyhow::bail!("Cannot compute quotes: trade_size is not resolved");
        };
        let trade_size = trade_size.as_decimal();
        let max_pos = self.config.max_position.as_decimal();

        let half_spread = anchor * Decimal::new(i64::from(self.config.half_spread_bps), 4);
        let inventory_factor = Decimal::try_from(self.config.inventory_skew_factor)?;
        let signal_factor = Decimal::try_from(self.config.signal_skew_factor)?;
        let signal_shift = signal_factor
            .checked_mul(signal_residual)
            .ok_or_else(|| anyhow::anyhow!("Signal skew exceeds the decimal range"))?;
        let shift = inventory_factor
            .checked_mul(net_position)
            .and_then(|inventory_shift| signal_shift.checked_sub(inventory_shift))
            .ok_or_else(|| anyhow::anyhow!("Quote skew exceeds the decimal range"))?;

        let bid_target = (anchor - half_spread)
            .checked_add(shift)
            .context("Skewed bid price exceeds the decimal range")?;
        let ask_target = (anchor + half_spread)
            .checked_add(shift)
            .context("Skewed ask price exceeds the decimal range")?;
        let bid_price = instrument.next_price_from_decimal(OrderSide::Buy, bid_target, 0);
        let ask_price = instrument.next_price_from_decimal(OrderSide::Sell, ask_target, 0);

        // Drop both sides if rounding has collapsed or crossed the spread,
        // which can happen when skew exceeds the half-spread on coarse-tick instruments.
        let crossed = match (bid_price, ask_price) {
            (Some(bp), Some(ap)) => bp >= ap,
            _ => false,
        };

        if crossed {
            return Ok(Vec::new());
        }

        let mut orders = Vec::new();

        if let Some(price) = bid_price
            && worst_long + trade_size <= max_pos
        {
            orders.push((OrderSide::Buy, price));
        }

        if let Some(price) = ask_price
            && worst_short - trade_size >= -max_pos
        {
            orders.push((OrderSide::Sell, price));
        }

        Ok(orders)
    }
}

nautilus_strategy!(CompositeMarketMaker, {
    fn on_order_rejected(&mut self, event: OrderRejected) {
        self.pending_self_cancels.remove(&event.client_order_id);
        self.last_quoted_anchor = None;
        self.last_quoted_residual = None;
    }

    fn on_order_expired(&mut self, event: OrderExpired) {
        self.pending_self_cancels.remove(&event.client_order_id);
        self.last_quoted_anchor = None;
        self.last_quoted_residual = None;
    }

    fn on_order_filled(&mut self, event: &OrderFilled) {
        let filled = {
            let cache = self.cache();
            cache
                .order(&event.client_order_id)
                .is_some_and(|o| matches!(o.status(), OrderStatus::Filled | OrderStatus::Voided))
        };

        if filled {
            self.pending_self_cancels.remove(&event.client_order_id);
        }
    }

    fn on_order_canceled(&mut self, event: &OrderCanceled) {
        if self.pending_self_cancels.remove(&event.client_order_id) {
            return;
        }

        if self.config.on_cancel_resubmit {
            self.last_quoted_anchor = None;
            self.last_quoted_residual = None;
        }
    }
});

impl Debug for CompositeMarketMaker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(CompositeMarketMaker))
            .field("config", &self.config)
            .field("trade_size", &self.trade_size)
            .field("signal_baseline", &self.signal_baseline)
            .field("last_signal", &self.last_signal)
            .finish()
    }
}

impl DataActor for CompositeMarketMaker {
    fn on_start(&mut self) -> anyhow::Result<()> {
        let instrument_id = self.config.instrument_id;
        let signal_instrument_id = self.config.signal_instrument_id;

        let (instrument, size_precision, min_quantity) = {
            let cache = self.cache();
            let instrument = cache.try_instrument(&instrument_id)?;
            let size_precision = instrument.size_precision();
            let min_quantity = instrument.min_quantity();
            (instrument, size_precision, min_quantity)
        };

        self.instrument = Some(instrument);

        if self.trade_size.is_none() {
            self.trade_size = Some(min_quantity.map_or_else(
                || Quantity::from_decimal_dp(Decimal::ONE, size_precision),
                Ok,
            )?);
        }

        self.subscribe_quotes(instrument_id, None, None);
        self.subscribe_quotes(signal_instrument_id, None, None);
        Ok(())
    }

    fn on_stop(&mut self) -> anyhow::Result<()> {
        let instrument_id = self.config.instrument_id;
        let signal_instrument_id = self.config.signal_instrument_id;
        self.cancel_all_orders(instrument_id, None, None, true, None)?;
        self.close_all_positions(instrument_id, None, None, None, None, None, None, None)?;
        self.unsubscribe_quotes(instrument_id, None, None);
        self.unsubscribe_quotes(signal_instrument_id, None, None);
        Ok(())
    }

    fn on_quote(&mut self, quote: &QuoteTick) -> anyhow::Result<()> {
        if quote.instrument_id != self.config.signal_instrument_id
            && quote.instrument_id != self.config.instrument_id
        {
            return Ok(());
        }

        if quote.instrument_id == self.config.signal_instrument_id {
            self.update_signal(quote)?;
            return Ok(());
        }

        let Some(signal_residual) = self.signal_residual() else {
            return Ok(());
        };

        let anchor = quote_midpoint(quote)?;
        anyhow::ensure!(
            self.instrument.is_some(),
            "Cannot handle quote: instrument is not resolved"
        );

        let instrument_id = self.config.instrument_id;
        let strategy_id = self.strategy_id().expect("Strategy must be registered");

        let has_resting = {
            let cache = self.cache();
            let inst = Some(&instrument_id);
            let sid = Some(&strategy_id);
            cache.orders_open_count(None, inst, sid, None, None) > 0
                || cache.orders_inflight_count(None, inst, sid, None, None) > 0
        };

        if !self.should_requote(anchor, signal_residual) && has_resting {
            return Ok(());
        }

        log::info!(
            "Requoting: anchor={anchor}, last_anchor={:?}, residual={signal_residual:.6}, last_residual={:?}, instrument={instrument_id}",
            self.last_quoted_anchor,
            self.last_quoted_residual,
        );

        let (net_position, worst_long, worst_short) = self.position_exposure(strategy_id);

        let quotes = self.compute_quotes(
            anchor,
            signal_residual,
            net_position,
            worst_long,
            worst_short,
        )?;

        if self.config.on_cancel_resubmit {
            let inst = Some(&instrument_id);
            let strategy = Some(&strategy_id);
            let ids: Vec<ClientOrderId> = {
                let cache = self.cache();
                let open = cache.orders_open(None, inst, strategy, None, None);
                let inflight = cache.orders_inflight(None, inst, strategy, None, None);
                open.iter()
                    .chain(inflight.iter())
                    .map(Order::client_order_id)
                    .collect()
            };
            self.pending_self_cancels.extend(ids);
        }

        self.cancel_all_orders(instrument_id, None, None, true, None)?;

        if quotes.is_empty() {
            return Ok(());
        }

        let trade_size = self
            .trade_size
            .ok_or_else(|| anyhow::anyhow!("Cannot handle quote: trade_size is not resolved"))?;

        let (tif, expire_time) = match self.config.expire_time_secs {
            Some(secs) => {
                let expire_ns = self.clock().timestamp_ns() + DurationNanos::try_from_secs(secs)?;
                (Some(TimeInForce::Gtd), Some(expire_ns))
            }
            None => (None, None),
        };

        for (side, price) in quotes {
            let order = self.order().limit(
                instrument_id,
                side,
                trade_size,
                price,
                tif,
                expire_time,
                Some(true), // post_only
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
            );
            self.submit_order(order, None, None, None)?;
        }

        self.last_quoted_anchor = Some(anchor);
        self.last_quoted_residual = Some(signal_residual);
        Ok(())
    }

    fn on_reset(&mut self) -> anyhow::Result<()> {
        self.instrument = None;
        self.trade_size = self.config.trade_size;
        self.last_quoted_anchor = None;
        self.last_quoted_residual = None;
        self.signal_baseline = self
            .config
            .signal_baseline
            .and_then(|value| Decimal::try_from(value).ok());
        self.last_signal = None;
        self.pending_self_cancels.clear();
        Ok(())
    }
}

impl CompositeMarketMaker {
    fn position_exposure(&self, strategy_id: StrategyId) -> (Decimal, Decimal, Decimal) {
        let instrument_id = Some(&self.config.instrument_id);
        let strategy = Some(&strategy_id);
        let cache = self.cache();

        let mut position_dec = Decimal::ZERO;
        for p in cache.positions_open(None, instrument_id, strategy, None, None) {
            position_dec += p.signed_qty;
        }

        let mut pending_buy_dec = Decimal::ZERO;
        let mut pending_sell_dec = Decimal::ZERO;
        let mut seen = AHashSet::new();

        let open = cache.orders_open(None, instrument_id, strategy, None, None);
        let inflight = cache.orders_inflight(None, instrument_id, strategy, None, None);
        for order in open.iter().chain(inflight.iter()) {
            if !seen.insert(order.client_order_id()) {
                continue;
            }

            let qty = order.leaves_qty().as_decimal();
            match order.order_side() {
                OrderSide::Buy => pending_buy_dec += qty,
                _ => pending_sell_dec += qty,
            }
        }

        (
            position_dec,
            position_dec + pending_buy_dec,
            position_dec - pending_sell_dec,
        )
    }
}

impl CompositeMarketMaker {
    fn update_signal(&mut self, quote: &QuoteTick) -> anyhow::Result<()> {
        if !quote.bid_price.is_positive() || quote.bid_price > quote.ask_price {
            return Ok(());
        }

        let signal_mid = quote_midpoint(quote)?;
        self.last_signal = Some(signal_mid);
        if self.signal_baseline.is_none() && self.config.signal_baseline.is_none() {
            self.signal_baseline = Some(signal_mid);
        }

        Ok(())
    }
}
