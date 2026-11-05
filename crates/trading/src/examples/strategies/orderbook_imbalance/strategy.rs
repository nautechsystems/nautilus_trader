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

//! Current-book imbalance strategy implementation.

use std::fmt::Debug;

use nautilus_common::actor::{DataActor, DataActorNative};
use nautilus_core::{UnixNanos, datetime::secs_to_nanos};
use nautilus_model::{
    data::OrderBookDeltas,
    enums::{OrderSide, TimeInForce},
    instruments::Instrument,
    types::{Price, Quantity},
};
use rust_decimal::Decimal;

use super::{config::OrderBookImbalanceConfig, state::ImbalanceState};
use crate::{
    nautilus_strategy,
    strategy::{Strategy, StrategyCore},
};

/// Takes the thinner best quote when the current book exceeds the configured imbalance.
///
/// Each instance trades one instrument. A managed subscription maintains the cached book.
/// Best sizes follow the book API: L2 uses a price level; L3 uses the first order at that price.
/// Stop releases the subscription and closes this strategy's positions. Dry-run mode reports
/// eligible signals and sends no order commands, including during shutdown.
pub struct OrderBookImbalance {
    pub(super) core: StrategyCore,
    pub(super) config: OrderBookImbalanceConfig,
    pub(super) state: ImbalanceState,
    trigger_interval_ns: u64,
}

impl OrderBookImbalance {
    /// Creates a strategy from its configuration.
    ///
    /// # Errors
    ///
    /// Returns an error if base configuration, thresholds, spacing, or size are invalid, or if
    /// managed order features are enabled in dry-run mode.
    pub fn from_config(config: OrderBookImbalanceConfig) -> anyhow::Result<Self> {
        config.validate()?;
        let trigger_interval_ns = secs_to_nanos(config.min_seconds_between_triggers)?;
        let core = StrategyCore::new_checked(config.base.clone())?;
        Ok(Self {
            core,
            config,
            state: ImbalanceState::default(),
            trigger_interval_ns,
        })
    }
}

nautilus_strategy!(OrderBookImbalance);

impl Debug for OrderBookImbalance {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(OrderBookImbalance))
            .field("config", &self.config)
            .field("state", &self.state)
            .finish()
    }
}

impl DataActor for OrderBookImbalance {
    fn on_start(&mut self) -> anyhow::Result<()> {
        let instrument_id = self.config.instrument_id;
        let instrument = self
            .cache()
            .instrument(&instrument_id)
            .ok_or_else(|| anyhow::anyhow!("Could not find instrument for {instrument_id}"))?;
        self.state.size_precision = Some(instrument.size_precision());
        self.subscribe_book_deltas(instrument_id, self.config.book_type, None, None, true, None);
        Ok(())
    }

    fn on_stop(&mut self) -> anyhow::Result<()> {
        let instrument_id = self.config.instrument_id;
        self.unsubscribe_book_deltas(instrument_id, None, None);
        if !self.config.dry_run {
            self.cancel_all_orders(instrument_id, None, None, true, None)?;
            self.close_all_positions(instrument_id, None, None, None, None, None, None, None)?;
        }

        println!(
            "[{instrument_id}] summary: updates={} signals={} dry_run={}",
            self.state.update_count, self.state.signal_count, self.config.dry_run
        );
        Ok(())
    }

    fn on_resume(&mut self) -> anyhow::Result<()> {
        DataActor::on_start(self)
    }

    fn on_book_deltas(&mut self, deltas: &OrderBookDeltas) -> anyhow::Result<()> {
        let instrument_id = self.config.instrument_id;
        if deltas.instrument_id != instrument_id {
            return Ok(());
        }

        self.state.update_count += 1;

        let Some(size_precision) = self.state.size_precision else {
            return Ok(());
        };

        let (bid_price, ask_price, bid, ask) = {
            // Borrow the native cache to avoid copying the book on every update
            let cache = DataActorNative::cache_ref(self);

            let Some(book) = cache.order_book(&instrument_id) else {
                return Ok(());
            };

            let (Some(bid_price), Some(ask_price), Some(bid), Some(ask)) = (
                book.best_bid_price(),
                book.best_ask_price(),
                book.best_bid_size(),
                book.best_ask_size(),
            ) else {
                return Ok(());
            };

            (bid_price, ask_price, bid.as_decimal(), ask.as_decimal())
        };

        if bid_price >= ask_price || bid <= Decimal::ZERO || ask <= Decimal::ZERO {
            return Ok(());
        }

        let smaller = bid.min(ask);
        let larger = bid.max(ask);
        let ratio = smaller / larger;

        if self.config.log_interval > 0
            && self
                .state
                .update_count
                .is_multiple_of(self.config.log_interval)
        {
            println!(
                "[{instrument_id}] update #{}: bid={bid} ask={ask} ratio={ratio}",
                self.state.update_count
            );
        }

        if larger <= self.config.trigger_min_size || ratio >= self.config.trigger_imbalance_ratio {
            return Ok(());
        }

        let now = self.clock().timestamp_ns();

        if self.state.last_trigger_ns.is_some_and(|last| {
            now.as_u64().saturating_sub(last.as_u64()) < self.trigger_interval_ns
        }) {
            return Ok(());
        }

        if self
            .cache()
            .orders_inflight_count(None, None, self.strategy_id().as_ref(), None, None)
            > 0
        {
            return Ok(());
        }

        let (side, price) = if bid > ask {
            (OrderSide::Buy, ask_price)
        } else {
            (OrderSide::Sell, bid_price)
        };

        let size = smaller
            .min(self.config.max_trade_size.as_decimal())
            .trunc_with_scale(u32::from(size_precision));

        if size <= Decimal::ZERO {
            return Ok(());
        }

        let quantity = Quantity::from_decimal_dp(size, size_precision)?;
        self.trigger(side, quantity, price, now)
    }

    fn on_reset(&mut self) -> anyhow::Result<()> {
        self.state = ImbalanceState::default();
        Ok(())
    }
}

impl OrderBookImbalance {
    fn trigger(
        &mut self,
        side: OrderSide,
        quantity: Quantity,
        price: Price,
        now: UnixNanos,
    ) -> anyhow::Result<()> {
        let instrument_id = self.config.instrument_id;
        self.state.last_trigger_ns = Some(now);
        self.state.signal_count += 1;

        if self.config.dry_run {
            println!(
                "[{instrument_id}] signal #{}: side={side} quantity={quantity} price={price} dry_run=true",
                self.state.signal_count
            );
            return Ok(());
        }

        let order = self.order().limit(
            instrument_id,
            side,
            quantity,
            price,
            Some(TimeInForce::Fok),
            None,
            Some(false),
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
        self.submit_order(order, None, None, None)
    }
}
