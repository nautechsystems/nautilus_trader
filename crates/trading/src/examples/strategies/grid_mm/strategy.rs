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

//! Grid market making strategy implementation.

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

use super::config::GridMarketMakerConfig;
use crate::{
    nautilus_strategy,
    strategy::{Strategy, StrategyCore},
};

/// Grid market making strategy with inventory-based skewing.
///
/// Places a symmetric grid of limit buy and sell orders around the mid-price.
/// Orders persist across ticks and are only replaced when the mid-price moves
/// by at least `requote_threshold_bps`. The grid is shifted by a skew proportional
/// to the current net position to discourage inventory buildup.
pub struct GridMarketMaker {
    pub(super) core: StrategyCore,
    pub(super) config: GridMarketMakerConfig,
    pub(super) instrument: Option<InstrumentAny>,
    pub(super) trade_size: Option<Quantity>,
    pub(super) price_precision: Option<u8>,
    pub(super) last_quoted_mid: Option<Price>,
    pub(super) pending_self_cancels: AHashSet<ClientOrderId>,
}

impl GridMarketMaker {
    /// Creates a new [`GridMarketMaker`] instance from config.
    #[must_use]
    pub fn new(config: GridMarketMakerConfig) -> Self {
        Self {
            core: StrategyCore::new(config.base.clone()),
            instrument: None,
            trade_size: config.trade_size,
            config,
            price_precision: None,
            last_quoted_mid: None,
            pending_self_cancels: AHashSet::new(),
        }
    }

    pub(super) fn should_requote(&self, mid: Price) -> bool {
        match self.last_quoted_mid {
            Some(last_mid) => {
                let last_f64 = last_mid.as_f64();
                if last_f64 == 0.0 {
                    return true;
                }
                let threshold = self.config.requote_threshold_bps as f64 / 10_000.0;
                (mid.as_f64() - last_f64).abs() / last_f64 >= threshold
            }
            None => true,
        }
    }

    pub(super) fn grid_orders(
        &self,
        mid: Price,
        net_position: f64,
        worst_long: Decimal,
        worst_short: Decimal,
    ) -> Vec<(OrderSide, Price)> {
        let instrument = self
            .instrument
            .as_ref()
            .expect("instrument should be resolved in on_start");
        let mid_f64 = mid.as_f64();
        let skew_f64 = self.config.skew_factor * net_position;
        let pct = self.config.grid_step_bps as f64 / 10_000.0;
        let trade_size = self
            .trade_size
            .expect("trade_size should be resolved in on_start")
            .as_decimal();
        let max_pos = self.config.max_position.as_decimal();
        let mut projected_long = worst_long;
        let mut projected_short = worst_short;
        let mut orders = Vec::new();

        for level in 1..=self.config.num_levels {
            let buy_f64 = mid_f64 * (1.0 - pct).powi(level as i32) - skew_f64;
            let sell_f64 = mid_f64 * (1.0 + pct).powi(level as i32) - skew_f64;
            // next_bid_price floors to the nearest valid bid tick (<=buy_f64),
            // next_ask_price ceils to the nearest valid ask tick (>=sell_f64),
            // preventing self-cross on coarse-tick instruments.
            let buy_price = instrument.next_bid_price(buy_f64, 0);
            let sell_price = instrument.next_ask_price(sell_f64, 0);

            if let Some(buy_price) = buy_price
                && projected_long + trade_size <= max_pos
            {
                orders.push((OrderSide::Buy, buy_price));
                projected_long += trade_size;
            }

            if let Some(sell_price) = sell_price
                && projected_short - trade_size >= -max_pos
            {
                orders.push((OrderSide::Sell, sell_price));
                projected_short -= trade_size;
            }
        }

        orders
    }
}

nautilus_strategy!(GridMarketMaker, {
    fn on_order_rejected(&mut self, event: OrderRejected) {
        self.pending_self_cancels.remove(&event.client_order_id);
        // Reset so the next quote tick can retry placing the full grid
        self.last_quoted_mid = None;
    }

    fn on_order_expired(&mut self, event: OrderExpired) {
        self.pending_self_cancels.remove(&event.client_order_id);
        // GTD expiry means the grid is gone; reset so re-quoting is not suppressed
        self.last_quoted_mid = None;
    }
});

impl Debug for GridMarketMaker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(GridMarketMaker))
            .field("config", &self.config)
            .field("trade_size", &self.trade_size)
            .finish()
    }
}

impl DataActor for GridMarketMaker {
    fn on_start(&mut self) -> anyhow::Result<()> {
        let instrument_id = self.config.instrument_id;
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

        // Resolve trade_size from instrument when not explicitly provided
        if self.trade_size.is_none() {
            self.trade_size =
                Some(min_quantity.unwrap_or_else(|| Quantity::new(1.0, size_precision)));
        }

        self.subscribe_quotes(instrument_id, None, None);
        Ok(())
    }

    fn on_stop(&mut self) -> anyhow::Result<()> {
        let instrument_id = self.config.instrument_id;
        self.cancel_all_orders(instrument_id, None, None)?;
        self.close_all_positions(instrument_id, None, None, None, None, None, None)?;
        self.unsubscribe_quotes(instrument_id, None, None);
        Ok(())
    }

    fn on_quote(&mut self, quote: &QuoteTick) -> anyhow::Result<()> {
        // f64 division by 2 is exact in IEEE 754
        let mid_f64 = (quote.bid_price.as_f64() + quote.ask_price.as_f64()) / 2.0;
        let mid = Price::new(
            mid_f64,
            self.price_precision
                .expect("price_precision should be resolved in on_start"),
        );

        let instrument_id = self.config.instrument_id;
        let strategy_id = StrategyId::from(self.actor_id.inner().as_str());

        // Always requote when the grid is empty, even if mid is within threshold
        let has_resting = {
            let cache = self.cache();
            let inst = Some(&instrument_id);
            let sid = Some(&strategy_id);
            !cache.orders_open(None, inst, sid, None, None).is_empty()
                || !cache
                    .orders_inflight(None, inst, sid, None, None)
                    .is_empty()
        };

        if !self.should_requote(mid) && has_resting {
            return Ok(());
        }

        log::info!(
            "Requoting grid: mid={mid}, last_mid={:?}, instrument={instrument_id}",
            self.last_quoted_mid,
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

        self.cancel_all_orders(instrument_id, None, None)?;

        // Compute worst-case per-side exposure for max_position checks,
        // since cancels are async and pending orders may still fill
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

            // Deduplicate open/inflight (can overlap during state transitions)
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

        let grid = self.grid_orders(mid, net_position, worst_long, worst_short);

        // Don't advance the requote anchor when no orders are placed,
        // otherwise the strategy can stall with zero resting orders
        if grid.is_empty() {
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

        for (side, price) in grid {
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
            self.submit_order(order, None, None)?;
        }

        self.last_quoted_mid = Some(mid);
        Ok(())
    }

    fn on_order_filled(&mut self, event: &OrderFilled) -> anyhow::Result<()> {
        // Only discard once fully filled; partial fills must keep the ID so a
        // subsequent self-cancel is not misclassified as external.
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
            // Reset so the next incoming quote triggers a full grid resubmission
            self.last_quoted_mid = None;
        }
        Ok(())
    }

    fn on_reset(&mut self) -> anyhow::Result<()> {
        self.instrument = None;
        self.trade_size = self.config.trade_size;
        self.price_precision = None;
        self.last_quoted_mid = None;
        self.pending_self_cancels.clear();
        Ok(())
    }
}
