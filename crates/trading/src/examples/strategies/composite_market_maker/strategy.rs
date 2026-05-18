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
use nautilus_common::actor::DataActor;
use nautilus_model::{
    data::QuoteTick,
    enums::{OrderSide, TimeInForce},
    events::{OrderCanceled, OrderExpired, OrderFilled, OrderRejected},
    identifiers::{ClientOrderId, StrategyId},
    instruments::{Instrument, InstrumentAny},
    orders::Order,
    types::{Price, Quantity},
};
use rust_decimal::Decimal;

use super::config::CompositeMarketMakerConfig;
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
/// Orders persist across ticks and are only replaced when either the anchor
/// or the signal residual's price impact (`signal_skew_factor * residual`)
/// moves by at least `requote_threshold_bps` of the anchor.
pub struct CompositeMarketMaker {
    pub(super) core: StrategyCore,
    pub(super) config: CompositeMarketMakerConfig,
    pub(super) instrument: Option<InstrumentAny>,
    pub(super) trade_size: Option<Quantity>,
    pub(super) price_precision: Option<u8>,
    pub(super) last_quoted_anchor: Option<Price>,
    pub(super) last_quoted_residual: Option<f64>,
    pub(super) signal_baseline: Option<f64>,
    pub(super) last_signal: Option<f64>,
    pub(super) pending_self_cancels: AHashSet<ClientOrderId>,
}

impl CompositeMarketMaker {
    /// Creates a new [`CompositeMarketMaker`] instance from config.
    #[must_use]
    pub fn new(config: CompositeMarketMakerConfig) -> Self {
        let signal_baseline = config.signal_baseline;
        Self {
            core: StrategyCore::new(config.base.clone()),
            instrument: None,
            trade_size: config.trade_size,
            config,
            price_precision: None,
            last_quoted_anchor: None,
            last_quoted_residual: None,
            signal_baseline,
            last_signal: None,
            pending_self_cancels: AHashSet::new(),
        }
    }

    pub(super) fn should_requote_on_anchor(&self, anchor: Price) -> bool {
        match self.last_quoted_anchor {
            Some(last_anchor) => {
                let last_f64 = last_anchor.as_f64();
                if last_f64 == 0.0 {
                    return true;
                }
                let threshold = self.config.requote_threshold_bps as f64 / 10_000.0;
                (anchor.as_f64() - last_f64).abs() / last_f64 >= threshold
            }
            None => true,
        }
    }

    pub(super) fn should_requote_on_residual(&self, residual: f64, anchor: Price) -> bool {
        if self.config.signal_skew_factor == 0.0 {
            return false;
        }
        let anchor_f64 = anchor.as_f64();
        if anchor_f64 == 0.0 {
            return false;
        }

        match self.last_quoted_residual {
            Some(last) => {
                // The signal's contribution to bid/ask price is signal_skew_factor * residual,
                // so a residual delta translates to that price units. Compare against the same
                // bps threshold expressed in price units of the anchor.
                let price_delta = (residual - last).abs() * self.config.signal_skew_factor.abs();
                let threshold = self.config.requote_threshold_bps as f64 / 10_000.0;
                price_delta / anchor_f64 >= threshold
            }
            None => true,
        }
    }

    pub(super) fn should_requote(&self, anchor: Price, residual: f64) -> bool {
        self.should_requote_on_anchor(anchor) || self.should_requote_on_residual(residual, anchor)
    }

    pub(super) fn signal_residual(&self) -> f64 {
        match (self.last_signal, self.signal_baseline) {
            (Some(signal), Some(baseline)) if baseline != 0.0 => signal / baseline - 1.0,
            _ => 0.0,
        }
    }

    pub(super) fn compute_quotes(
        &self,
        anchor: Price,
        signal_residual: f64,
        net_position: f64,
        worst_long: Decimal,
        worst_short: Decimal,
    ) -> Vec<(OrderSide, Price)> {
        let instrument = self
            .instrument
            .as_ref()
            .expect("instrument should be resolved in on_start");
        let trade_size = self
            .trade_size
            .expect("trade_size should be resolved in on_start")
            .as_decimal();
        let max_pos = self.config.max_position.as_decimal();

        let anchor_f64 = anchor.as_f64();
        let half_spread = anchor_f64 * (self.config.half_spread_bps as f64 / 10_000.0);
        let inventory_shift = self.config.inventory_skew_factor * net_position;
        let signal_shift = self.config.signal_skew_factor * signal_residual;
        // Positive signal residual lifts both sides; positive inventory lowers both sides.
        let total_shift = signal_shift - inventory_shift;

        let bid_f64 = anchor_f64 - half_spread + total_shift;
        let ask_f64 = anchor_f64 + half_spread + total_shift;
        // next_bid_price floors to the nearest valid bid tick (<=bid_f64),
        // next_ask_price ceils to the nearest valid ask tick (>=ask_f64),
        // so a non-crossing pair stays non-crossing after rounding.
        let bid_price = instrument.next_bid_price(bid_f64, 0);
        let ask_price = instrument.next_ask_price(ask_f64, 0);

        // Drop both sides if rounding has collapsed or crossed the spread,
        // which can happen when skew exceeds the half-spread on coarse-tick instruments.
        let crossed = match (bid_price, ask_price) {
            (Some(bp), Some(ap)) => bp >= ap,
            _ => false,
        };

        if crossed {
            return Vec::new();
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

        orders
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
            let instrument = cache
                .instrument(&instrument_id)
                .ok_or_else(|| anyhow::anyhow!("Instrument {instrument_id} not found in cache"))?;
            (
                instrument.clone(),
                instrument.size_precision(),
                instrument.min_quantity(),
            )
        };
        self.price_precision = Some(instrument.price_precision());
        self.instrument = Some(instrument);

        if self.trade_size.is_none() {
            self.trade_size =
                Some(min_quantity.unwrap_or_else(|| Quantity::new(1.0, size_precision)));
        }

        self.subscribe_quotes(instrument_id, None, None);
        self.subscribe_quotes(signal_instrument_id, None, None);
        Ok(())
    }

    fn on_stop(&mut self) -> anyhow::Result<()> {
        let instrument_id = self.config.instrument_id;
        let signal_instrument_id = self.config.signal_instrument_id;
        self.cancel_all_orders(instrument_id, None, None, None)?;
        self.close_all_positions(instrument_id, None, None, None, None, None, None)?;
        self.unsubscribe_quotes(instrument_id, None, None);
        self.unsubscribe_quotes(signal_instrument_id, None, None);
        Ok(())
    }

    fn on_quote(&mut self, quote: &QuoteTick) -> anyhow::Result<()> {
        if quote.instrument_id == self.config.signal_instrument_id {
            let signal_mid = (quote.bid_price.as_f64() + quote.ask_price.as_f64()) / 2.0;
            self.last_signal = Some(signal_mid);
            if self.signal_baseline.is_none() {
                self.signal_baseline = Some(signal_mid);
            }
            return Ok(());
        }

        if quote.instrument_id != self.config.instrument_id {
            return Ok(());
        }

        let anchor_f64 = (quote.bid_price.as_f64() + quote.ask_price.as_f64()) / 2.0;
        let anchor = Price::new(
            anchor_f64,
            self.price_precision
                .expect("price_precision should be resolved in on_start"),
        );

        let signal_residual = self.signal_residual();
        let instrument_id = self.config.instrument_id;
        let strategy_id = StrategyId::from(self.actor_id.inner().as_str());

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

        if self.config.on_cancel_resubmit {
            let inst = Some(&instrument_id);
            let strategy = Some(&strategy_id);
            let ids: Vec<ClientOrderId> = {
                let cache = self.cache();
                cache
                    .orders_open(None, inst, strategy, None, None)
                    .iter()
                    .chain(
                        cache
                            .orders_inflight(None, inst, strategy, None, None)
                            .iter(),
                    )
                    .map(|o| o.client_order_id())
                    .collect()
            };
            self.pending_self_cancels.extend(ids);
        }

        self.cancel_all_orders(instrument_id, None, None, None)?;

        let (net_position, worst_long, worst_short) = {
            let instrument_id = Some(&instrument_id);
            let strategy = Some(&strategy_id);
            let cache = self.cache();

            let mut position_qty = 0.0_f64;
            let mut position_dec = Decimal::ZERO;

            for p in cache.positions_open(None, instrument_id, strategy, None, None) {
                position_qty += p.signed_qty;
                position_dec += p.quantity.as_decimal()
                    * if p.signed_qty < 0.0 {
                        Decimal::NEGATIVE_ONE
                    } else {
                        Decimal::ONE
                    };
            }

            let mut pending_buy_dec = Decimal::ZERO;
            let mut pending_sell_dec = Decimal::ZERO;
            let mut seen = AHashSet::new();

            for order in cache
                .orders_open(None, instrument_id, strategy, None, None)
                .iter()
                .chain(
                    cache
                        .orders_inflight(None, instrument_id, strategy, None, None)
                        .iter(),
                )
            {
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
                position_qty,
                position_dec + pending_buy_dec,
                position_dec - pending_sell_dec,
            )
        };

        let quotes = self.compute_quotes(
            anchor,
            signal_residual,
            net_position,
            worst_long,
            worst_short,
        );

        if quotes.is_empty() {
            return Ok(());
        }

        let trade_size = self
            .trade_size
            .expect("trade_size should be resolved in on_start");

        let (tif, expire_time) = match self.config.expire_time_secs {
            Some(secs) => {
                let now_ns = self.core.clock().timestamp_ns();
                let expire_ns = now_ns + secs * 1_000_000_000;
                (Some(TimeInForce::Gtd), Some(expire_ns))
            }
            None => (None, None),
        };

        for (side, price) in quotes {
            let order = self.core.order_factory().limit(
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

    fn on_order_filled(&mut self, event: &OrderFilled) -> anyhow::Result<()> {
        let closed = {
            let cache = self.cache();
            cache
                .order(&event.client_order_id)
                .is_some_and(|o| o.is_closed())
        };

        if closed {
            self.pending_self_cancels.remove(&event.client_order_id);
        }
        Ok(())
    }

    fn on_order_canceled(&mut self, event: &OrderCanceled) -> anyhow::Result<()> {
        if self.pending_self_cancels.remove(&event.client_order_id) {
            return Ok(());
        }

        if self.config.on_cancel_resubmit {
            self.last_quoted_anchor = None;
            self.last_quoted_residual = None;
        }
        Ok(())
    }

    fn on_reset(&mut self) -> anyhow::Result<()> {
        self.instrument = None;
        self.trade_size = self.config.trade_size;
        self.price_precision = None;
        self.last_quoted_anchor = None;
        self.last_quoted_residual = None;
        self.signal_baseline = self.config.signal_baseline;
        self.last_signal = None;
        self.pending_self_cancels.clear();
        Ok(())
    }
}
