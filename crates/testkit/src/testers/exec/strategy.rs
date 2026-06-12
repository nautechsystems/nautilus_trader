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

use ahash::AHashSet;
use nautilus_common::{actor::DataActor, enums::LogColor, log_info, log_warn, timer::TimeEvent};
use nautilus_core::UnixNanos;
use nautilus_model::{
    data::{Bar, IndexPriceUpdate, MarkPriceUpdate, OrderBookDeltas, QuoteTick, TradeTick},
    enums::{ContingencyType, OrderSide, OrderType, TimeInForce},
    identifiers::{ClientId, ClientOrderId, InstrumentId, StrategyId},
    instruments::{Instrument, InstrumentAny},
    orderbook::OrderBook,
    orders::{Order, OrderAny},
    types::{Price, price::PriceRaw},
};
use nautilus_trading::{
    nautilus_strategy,
    strategy::{Strategy, StrategyCore},
};
use rust_decimal::{Decimal, prelude::ToPrimitive};

use super::config::ExecTesterConfig;

/// An execution tester strategy for live testing order execution functionality.
///
/// This strategy is designed for testing execution adapters by submitting
/// limit orders, stop orders, and managing positions. It can maintain orders
/// at a configurable offset from the top of book.
///
/// **WARNING**: This strategy has no alpha advantage whatsoever.
/// It is not intended to be used for live trading with real money.
#[derive(Debug)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "tester state tracks independent execution scenarios"
)]
pub struct ExecTester {
    pub(super) core: StrategyCore,
    pub(super) config: ExecTesterConfig,
    pub(super) instrument: Option<InstrumentAny>,
    pub(super) price_offset: Option<u64>,
    pub(super) preinitialized_market_data: bool,

    // Order tracking
    pub(super) buy_order: Option<OrderAny>,
    pub(super) sell_order: Option<OrderAny>,
    pub(super) buy_stop_order: Option<OrderAny>,
    pub(super) sell_stop_order: Option<OrderAny>,
    pub(super) open_position_submitted: bool,

    // One-shot guard for `test_modify_rejected`: ensures the programmatic
    // modify is attempted at most once across the strategy's lifetime.
    pub(super) modify_rejected_attempted: bool,
    pub(super) pending_open_position_qty: Option<Decimal>,
    pub(super) buy_cancel_replace_attempted: bool,
    pub(super) sell_cancel_replace_attempted: bool,
    pub(super) buy_stop_cancel_replace_attempted: bool,
    pub(super) sell_stop_cancel_replace_attempted: bool,
}

nautilus_strategy!(ExecTester, {
    fn external_order_claims(&self) -> Option<Vec<InstrumentId>> {
        self.config.base.external_order_claims.clone()
    }
});

impl DataActor for ExecTester {
    fn on_start(&mut self) -> anyhow::Result<()> {
        Strategy::on_start(self)?;

        let instrument_id = self.config.instrument_id;
        let client_id = self.config.client_id;

        let instrument = {
            let cache = self.cache();
            cache.instrument(&instrument_id).cloned()
        };

        if let Some(inst) = instrument {
            self.initialize_with_instrument(inst, true)?;
        } else {
            log::info!("Instrument {instrument_id} not in cache, subscribing...");
            self.subscribe_instrument(instrument_id, client_id, None);

            // Also subscribe to market data to trigger instrument definitions from data providers
            // (e.g., Databento sends instrument definitions as part of market data subscriptions)
            if self.config.subscribe_quotes {
                self.subscribe_quotes(instrument_id, client_id, None);
            }

            if self.config.subscribe_trades {
                self.subscribe_trades(instrument_id, client_id, None);
            }
            self.preinitialized_market_data =
                self.config.subscribe_quotes || self.config.subscribe_trades;
        }

        Ok(())
    }

    fn on_instrument(&mut self, instrument: &InstrumentAny) -> anyhow::Result<()> {
        if instrument.id() == self.config.instrument_id && self.instrument.is_none() {
            let id = instrument.id();
            log::info!("Received instrument {id}, initializing...");
            self.initialize_with_instrument(instrument.clone(), !self.preinitialized_market_data)?;
        }
        Ok(())
    }

    fn on_stop(&mut self) -> anyhow::Result<()> {
        if self.config.dry_run {
            log_warn!("Dry run mode, skipping cancel all orders and close all positions");
            return Ok(());
        }

        let instrument_id = self.config.instrument_id;
        let client_id = self.config.client_id;
        let strategy_id = StrategyId::from(self.core.actor_id.inner().as_str());

        if self.config.cancel_orders_on_stop {
            self.cancel_active_orders(instrument_id, strategy_id, client_id);
        }

        if self.config.close_positions_on_stop {
            let time_in_force = self
                .config
                .close_positions_time_in_force
                .or(Some(TimeInForce::Gtc));

            if let Err(e) = self.close_all_positions(
                instrument_id,
                None,
                client_id,
                None,
                time_in_force,
                Some(self.config.reduce_only_on_stop),
                None,
            ) {
                log::error!("Failed to close all positions: {e}");
            }
        }

        if self.config.can_unsubscribe && self.instrument.is_some() {
            if self.config.subscribe_quotes {
                self.unsubscribe_quotes(instrument_id, client_id, None);
            }

            if self.config.subscribe_trades {
                self.unsubscribe_trades(instrument_id, client_id, None);
            }

            if self.config.subscribe_book {
                self.unsubscribe_book_at_interval(
                    instrument_id,
                    self.config.book_interval_ms,
                    client_id,
                    None,
                );
            }
        }

        Ok(())
    }

    fn on_quote(&mut self, quote: &QuoteTick) -> anyhow::Result<()> {
        if self.config.log_data {
            log_info!("{quote:?}", color = LogColor::Cyan);
        }

        if quote.instrument_id == self.config.instrument_id
            && self.config.open_position_on_first_quote
        {
            self.submit_pending_open_position();
        }

        self.maintain_orders(quote.bid_price, quote.ask_price);
        Ok(())
    }

    fn on_trade(&mut self, trade: &TradeTick) -> anyhow::Result<()> {
        if self.config.log_data {
            log_info!("{trade:?}", color = LogColor::Cyan);
        }
        Ok(())
    }

    fn on_book(&mut self, book: &OrderBook) -> anyhow::Result<()> {
        if self.config.log_data {
            let num_levels = self.config.book_levels_to_print;
            let instrument_id = book.instrument_id;
            let book_str = book.pprint(num_levels, None);
            log_info!("\n{instrument_id}\n{book_str}", color = LogColor::Cyan);

            // Log own order book if available
            if self.is_registered() {
                let cache = self.cache();
                if let Some(own_book) = cache.own_order_book(&instrument_id) {
                    let own_book_str = own_book.pprint(num_levels, None);
                    log_info!(
                        "\n{instrument_id} (own)\n{own_book_str}",
                        color = LogColor::Magenta
                    );
                }
            }
        }

        let Some(best_bid) = book.best_bid_price() else {
            return Ok(()); // Wait for market
        };
        let Some(best_ask) = book.best_ask_price() else {
            return Ok(()); // Wait for market
        };

        self.maintain_orders(best_bid, best_ask);
        Ok(())
    }

    fn on_book_deltas(&mut self, deltas: &OrderBookDeltas) -> anyhow::Result<()> {
        if self.config.log_data {
            log_info!("{deltas:?}", color = LogColor::Cyan);
        }
        Ok(())
    }

    fn on_bar(&mut self, bar: &Bar) -> anyhow::Result<()> {
        if self.config.log_data {
            log_info!("{bar:?}", color = LogColor::Cyan);
        }
        Ok(())
    }

    fn on_mark_price(&mut self, mark_price: &MarkPriceUpdate) -> anyhow::Result<()> {
        if self.config.log_data {
            log_info!("{mark_price:?}", color = LogColor::Cyan);
        }
        Ok(())
    }

    fn on_index_price(&mut self, index_price: &IndexPriceUpdate) -> anyhow::Result<()> {
        if self.config.log_data {
            log_info!("{index_price:?}", color = LogColor::Cyan);
        }
        Ok(())
    }

    fn on_time_event(&mut self, event: &TimeEvent) -> anyhow::Result<()> {
        Strategy::on_time_event(self, event)
    }
}

impl ExecTester {
    /// Creates a new [`ExecTester`] instance.
    #[must_use]
    pub fn new(config: ExecTesterConfig) -> Self {
        let pending_open_position_qty = config.open_position_on_start_qty;

        Self {
            core: StrategyCore::new(config.base.clone()),
            config,
            instrument: None,
            price_offset: None,
            preinitialized_market_data: false,
            buy_order: None,
            sell_order: None,
            buy_stop_order: None,
            sell_stop_order: None,
            open_position_submitted: false,
            modify_rejected_attempted: false,
            pending_open_position_qty,
            buy_cancel_replace_attempted: false,
            sell_cancel_replace_attempted: false,
            buy_stop_cancel_replace_attempted: false,
            sell_stop_cancel_replace_attempted: false,
        }
    }

    fn initialize_with_instrument(
        &mut self,
        instrument: InstrumentAny,
        subscribe_market_data: bool,
    ) -> anyhow::Result<()> {
        let instrument_id = self.config.instrument_id;
        let client_id = self.config.client_id;

        self.price_offset = Some(self.get_price_offset(&instrument));
        self.instrument = Some(instrument);

        if subscribe_market_data && self.config.subscribe_quotes {
            self.subscribe_quotes(instrument_id, client_id, None);
        }

        if subscribe_market_data && self.config.subscribe_trades {
            self.subscribe_trades(instrument_id, client_id, None);
        }

        if self.config.subscribe_book {
            self.subscribe_book_at_interval(
                instrument_id,
                self.config.book_type,
                self.config.book_depth,
                self.config.book_interval_ms,
                client_id,
                None,
            );
        }

        if let Some(qty) = self.pending_open_position_qty {
            let quote_ready = {
                let cache = self.cache();
                cache.quote(&instrument_id).is_some()
            };

            if self.config.open_position_on_first_quote
                && self.config.subscribe_quotes
                && !quote_ready
            {
                log::info!("Waiting for first quote before opening {instrument_id} position");
            } else {
                self.pending_open_position_qty = None;
                self.open_position(qty)?;
                self.open_position_submitted = true;
            }
        }

        Ok(())
    }

    pub(super) fn get_price_offset(&self, _instrument: &InstrumentAny) -> u64 {
        self.config.tob_offset_ticks
    }

    fn expire_time_from_delta(&self, mins: u64) -> UnixNanos {
        let current_ns = self.timestamp_ns();
        let delta_ns = mins.saturating_mul(60).saturating_mul(1_000_000_000);
        UnixNanos::from(current_ns.as_u64() + delta_ns)
    }

    fn resolve_time_in_force(
        &self,
        tif_override: Option<TimeInForce>,
    ) -> (TimeInForce, Option<UnixNanos>) {
        match (tif_override, self.config.order_expire_time_delta_mins) {
            (Some(TimeInForce::Gtd), Some(mins)) => {
                (TimeInForce::Gtd, Some(self.expire_time_from_delta(mins)))
            }
            (Some(TimeInForce::Gtd), None) => {
                log_warn!(
                    "GTD time in force requires order_expire_time_delta_mins, falling back to GTC"
                );
                (TimeInForce::Gtc, None)
            }
            (Some(tif), _) => (tif, None),
            (None, Some(mins)) => (TimeInForce::Gtd, Some(self.expire_time_from_delta(mins))),
            (None, None) => (TimeInForce::Gtc, None),
        }
    }

    fn submit_pending_open_position(&mut self) {
        if self.instrument.is_none() {
            return;
        }

        let Some(qty) = self.pending_open_position_qty.take() else {
            return;
        };

        if let Err(e) = self.open_position(qty) {
            log::error!("Failed to submit pending open position: {e}");
        } else {
            self.open_position_submitted = true;
        }
    }

    pub(super) fn is_order_active(order: &OrderAny) -> bool {
        order.is_active_local() || order.is_inflight() || order.is_open()
    }

    pub(super) fn limit_order_is_one_shot(&self) -> bool {
        self.config.test_reject_post_only
            || self.config.limit_aggressive
            || self.config.order_expire_time_delta_mins.is_some()
            || matches!(
                self.config.limit_time_in_force,
                Some(TimeInForce::Ioc | TimeInForce::Fok)
            )
    }

    pub(super) fn stop_order_is_one_shot(&self) -> bool {
        self.config.order_expire_time_delta_mins.is_some()
            || matches!(
                self.config.stop_time_in_force,
                Some(TimeInForce::Ioc | TimeInForce::Fok)
            )
            || matches!(self.config.stop_order_type, OrderType::TrailingStopMarket)
    }

    pub(super) fn get_order_trigger_price(order: &OrderAny) -> Option<Price> {
        order.trigger_price()
    }

    fn modify_stop_order(
        &mut self,
        order: &OrderAny,
        trigger_price: Price,
        limit_price: Option<Price>,
    ) -> anyhow::Result<()> {
        let client_id = self.config.client_id;

        match order {
            OrderAny::StopMarket(_)
            | OrderAny::MarketIfTouched(_)
            | OrderAny::TrailingStopMarket(_) => self.modify_order(
                order.client_order_id(),
                None,
                None,
                Some(trigger_price),
                client_id,
                None,
            ),
            OrderAny::StopLimit(_) | OrderAny::LimitIfTouched(_) => self.modify_order(
                order.client_order_id(),
                None,
                limit_price,
                Some(trigger_price),
                client_id,
                None,
            ),
            _ => {
                log_warn!("Cannot modify order of type {:?}", order.order_type());
                Ok(())
            }
        }
    }

    /// Submit an order, applying `order_params` if configured.
    fn submit_order_apply_params(&mut self, order: OrderAny) -> anyhow::Result<()> {
        let client_id = self.config.client_id;
        if let Some(params) = &self.config.order_params {
            self.submit_order(order, None, client_id, Some(params.clone()))
        } else {
            self.submit_order(order, None, client_id, None)
        }
    }

    /// Maintain orders based on current market prices.
    pub(super) fn maintain_orders(&mut self, best_bid: Price, best_ask: Price) {
        if self.instrument.is_none() || self.config.dry_run {
            return;
        }

        if self.config.batch_submit_limit_pair
            && self.config.enable_limit_buys
            && self.config.enable_limit_sells
        {
            self.maintain_batch_limit_pair(best_bid, best_ask);
            return;
        }

        if self.config.enable_limit_buys {
            self.maintain_buy_orders(best_bid, best_ask);
        }

        if self.config.enable_limit_sells {
            self.maintain_sell_orders(best_bid, best_ask);
        }

        if self.config.enable_stop_buys {
            self.maintain_stop_buy_orders(best_bid, best_ask);
        }

        if self.config.enable_stop_sells {
            self.maintain_stop_sell_orders(best_bid, best_ask);
        }
    }

    /// Refreshes the locally-tracked order for `side` from the cache so that
    /// downstream checks (`venue_order_id()`, `is_pending_*`, status) see the
    /// latest event-driven state instead of the stale clone captured at submit.
    fn refresh_tracked_order(&mut self, side: OrderSide) {
        let cid = match side {
            OrderSide::Buy => self.buy_order.as_ref().map(OrderAny::client_order_id),
            OrderSide::Sell => self.sell_order.as_ref().map(OrderAny::client_order_id),
            OrderSide::NoOrderSide => None,
        };
        let Some(cid) = cid else {
            return;
        };
        let latest = self.cache().order(&cid).map(|o| o.clone());
        if let Some(latest) = latest {
            match side {
                OrderSide::Buy => self.buy_order = Some(latest),
                OrderSide::Sell => self.sell_order = Some(latest),
                OrderSide::NoOrderSide => {}
            }
        }
    }

    fn refresh_tracked_stop_order(&mut self, side: OrderSide) {
        let cid = match side {
            OrderSide::Buy => self.buy_stop_order.as_ref().map(OrderAny::client_order_id),
            OrderSide::Sell => self.sell_stop_order.as_ref().map(OrderAny::client_order_id),
            OrderSide::NoOrderSide => None,
        };
        let Some(cid) = cid else {
            return;
        };
        let latest = self.cache().order(&cid).map(|o| o.clone());
        if let Some(latest) = latest {
            match side {
                OrderSide::Buy => self.buy_stop_order = Some(latest),
                OrderSide::Sell => self.sell_stop_order = Some(latest),
                OrderSide::NoOrderSide => {}
            }
        }
    }

    /// Maintain buy limit orders.
    fn maintain_buy_orders(&mut self, best_bid: Price, best_ask: Price) {
        // Refresh from cache first so post-submit event state (venue_order_id,
        // status) is visible. Done before binding `&self.instrument` to avoid
        // holding an immutable borrow across the mutable refresh call.
        self.refresh_tracked_order(OrderSide::Buy);

        let Some(instrument) = &self.instrument else {
            return;
        };
        let Some(price_offset_ticks) = self.price_offset else {
            return;
        };

        let increment = instrument.price_increment();
        let precision = instrument.price_precision();

        // `test_reject_post_only` and `limit_aggressive` both cross the spread for
        // BUY (place at/above the ask). `test_reject_post_only` additionally sets
        // post_only=true downstream to trigger venue rejection; `limit_aggressive`
        // pairs with IOC/FOK TIF for marketable-fill scenarios.
        let cross_spread = self.config.test_reject_post_only || self.config.limit_aggressive;
        let raw_price = if cross_spread {
            add_price_ticks(best_ask, increment, price_offset_ticks, precision)
        } else {
            sub_price_ticks(best_bid, increment, price_offset_ticks, precision)
        };
        let price = clamp_price_to_range(
            raw_price,
            instrument,
            self.config.clamp_to_instrument_price_range,
        );

        let needs_new_order = match &self.buy_order {
            None => true,
            Some(order) => !Self::is_order_active(order) && !self.limit_order_is_one_shot(),
        };

        if needs_new_order {
            let result = if self.config.enable_brackets {
                self.submit_bracket_order(OrderSide::Buy, price)
            } else {
                self.submit_limit_order(OrderSide::Buy, price)
            };

            if let Err(e) = result {
                log::error!("Failed to submit buy order: {e}");
            }
        } else if let Some(order) = &self.buy_order
            && order.venue_order_id().is_some()
            && !order.is_pending_update()
            && !order.is_pending_cancel()
        {
            let client_id = self.config.client_id;

            // One-shot programmatic modify to exercise the adapter's modify-rejection
            // path (TC-E36). Uses a small price bump rather than waiting for drift.
            if self.config.test_modify_rejected && !self.modify_rejected_attempted {
                self.modify_rejected_attempted = true;
                let order_clone = order.clone();
                let bumped = clamp_price_to_range(
                    add_price_ticks(price, increment, 1, precision),
                    instrument,
                    self.config.clamp_to_instrument_price_range,
                );

                if let Err(e) = self.modify_order(
                    order_clone.client_order_id(),
                    None,
                    Some(bumped),
                    None,
                    client_id,
                    None,
                ) {
                    log::error!("Failed to submit test modify on buy order: {e}");
                }
                return;
            }

            if let Some(order_price) = order.price()
                && order_price < price
            {
                if self.config.modify_orders_to_maintain_tob_offset {
                    let order_clone = order.clone();
                    if let Err(e) = self.modify_order(
                        order_clone.client_order_id(),
                        None,
                        Some(price),
                        None,
                        client_id,
                        None,
                    ) {
                        log::error!("Failed to modify buy order: {e}");
                    }
                } else if self.config.cancel_replace_orders_to_maintain_tob_offset
                    && !self.buy_cancel_replace_attempted
                {
                    self.buy_cancel_replace_attempted = true;
                    let order_clone = order.clone();
                    let _ = self.cancel_order(order_clone.client_order_id(), client_id, None);

                    if let Err(e) = self.submit_limit_order(OrderSide::Buy, price) {
                        log::error!("Failed to submit replacement buy order: {e}");
                    }
                }
            }
        }
    }

    /// Maintain sell limit orders.
    fn maintain_sell_orders(&mut self, best_bid: Price, best_ask: Price) {
        // Refresh from cache before borrowing `&self.instrument`; see the
        // matching comment in `maintain_buy_orders`.
        self.refresh_tracked_order(OrderSide::Sell);

        let Some(instrument) = &self.instrument else {
            return;
        };
        let Some(price_offset_ticks) = self.price_offset else {
            return;
        };

        let increment = instrument.price_increment();
        let precision = instrument.price_precision();

        // See `maintain_buy_orders` for the cross_spread and refresh rationale.
        let cross_spread = self.config.test_reject_post_only || self.config.limit_aggressive;
        let raw_price = if cross_spread {
            sub_price_ticks(best_bid, increment, price_offset_ticks, precision)
        } else {
            add_price_ticks(best_ask, increment, price_offset_ticks, precision)
        };
        let price = clamp_price_to_range(
            raw_price,
            instrument,
            self.config.clamp_to_instrument_price_range,
        );

        let needs_new_order = match &self.sell_order {
            None => true,
            Some(order) => !Self::is_order_active(order) && !self.limit_order_is_one_shot(),
        };

        if needs_new_order {
            let result = if self.config.enable_brackets {
                self.submit_bracket_order(OrderSide::Sell, price)
            } else {
                self.submit_limit_order(OrderSide::Sell, price)
            };

            if let Err(e) = result {
                log::error!("Failed to submit sell order: {e}");
            }
        } else if let Some(order) = &self.sell_order
            && order.venue_order_id().is_some()
            && !order.is_pending_update()
            && !order.is_pending_cancel()
        {
            let client_id = self.config.client_id;

            // One-shot programmatic modify (TC-E36); see maintain_buy_orders.
            if self.config.test_modify_rejected && !self.modify_rejected_attempted {
                self.modify_rejected_attempted = true;
                let order_clone = order.clone();
                let bumped = clamp_price_to_range(
                    sub_price_ticks(price, increment, 1, precision),
                    instrument,
                    self.config.clamp_to_instrument_price_range,
                );

                if let Err(e) = self.modify_order(
                    order_clone.client_order_id(),
                    None,
                    Some(bumped),
                    None,
                    client_id,
                    None,
                ) {
                    log::error!("Failed to submit test modify on sell order: {e}");
                }
                return;
            }

            if let Some(order_price) = order.price()
                && order_price > price
            {
                if self.config.modify_orders_to_maintain_tob_offset {
                    let order_clone = order.clone();
                    if let Err(e) = self.modify_order(
                        order_clone.client_order_id(),
                        None,
                        Some(price),
                        None,
                        client_id,
                        None,
                    ) {
                        log::error!("Failed to modify sell order: {e}");
                    }
                } else if self.config.cancel_replace_orders_to_maintain_tob_offset
                    && !self.sell_cancel_replace_attempted
                {
                    self.sell_cancel_replace_attempted = true;
                    let order_clone = order.clone();
                    let _ = self.cancel_order(order_clone.client_order_id(), client_id, None);

                    if let Err(e) = self.submit_limit_order(OrderSide::Sell, price) {
                        log::error!("Failed to submit replacement sell order: {e}");
                    }
                }
            }
        }
    }

    /// Submits a buy and sell limit order as an order list (batch).
    fn maintain_batch_limit_pair(&mut self, best_bid: Price, best_ask: Price) {
        // Same rationale as the non-batch path: refresh from cache so the
        // active-order check sees the latest status. Done before binding
        // `&self.instrument` to avoid an immutable-vs-mutable borrow conflict.
        self.refresh_tracked_order(OrderSide::Buy);
        self.refresh_tracked_order(OrderSide::Sell);

        let Some(instrument) = &self.instrument else {
            return;
        };
        let Some(price_offset_ticks) = self.price_offset else {
            return;
        };

        let buy_needs = match &self.buy_order {
            None => true,
            Some(order) => !Self::is_order_active(order) && !self.limit_order_is_one_shot(),
        };
        let sell_needs = match &self.sell_order {
            None => true,
            Some(order) => !Self::is_order_active(order) && !self.limit_order_is_one_shot(),
        };

        if !buy_needs || !sell_needs {
            return;
        }

        let increment = instrument.price_increment();
        let precision = instrument.price_precision();

        // `test_reject_post_only` and `limit_aggressive` flip the BUY/SELL
        // pricing to cross the spread; mirrored from `maintain_buy_orders` /
        // `maintain_sell_orders` so batch mode supports the same scenarios.
        let cross_spread = self.config.test_reject_post_only || self.config.limit_aggressive;
        let (raw_buy_price, raw_sell_price) = if cross_spread {
            (
                add_price_ticks(best_ask, increment, price_offset_ticks, precision),
                sub_price_ticks(best_bid, increment, price_offset_ticks, precision),
            )
        } else {
            (
                sub_price_ticks(best_bid, increment, price_offset_ticks, precision),
                add_price_ticks(best_ask, increment, price_offset_ticks, precision),
            )
        };
        let clamp = self.config.clamp_to_instrument_price_range;
        let buy_price = clamp_price_to_range(raw_buy_price, instrument, clamp);
        let sell_price = clamp_price_to_range(raw_sell_price, instrument, clamp);
        let quantity = instrument.make_qty(self.config.order_qty.as_f64(), None);
        let (time_in_force, expire_time) =
            self.resolve_time_in_force(self.config.limit_time_in_force);

        let buy_order = self.core.order_factory().limit(
            self.config.instrument_id,
            OrderSide::Buy,
            quantity,
            buy_price,
            Some(time_in_force),
            expire_time,
            Some(self.config.use_post_only || self.config.test_reject_post_only),
            None,
            Some(self.config.use_quote_quantity),
            self.config.order_display_qty,
            self.config.emulation_trigger,
            None,
            None,
            None,
            None,
            None,
        );

        let sell_order = self.core.order_factory().limit(
            self.config.instrument_id,
            OrderSide::Sell,
            quantity,
            sell_price,
            Some(time_in_force),
            expire_time,
            Some(self.config.use_post_only || self.config.test_reject_post_only),
            None,
            Some(self.config.use_quote_quantity),
            self.config.order_display_qty,
            self.config.emulation_trigger,
            None,
            None,
            None,
            None,
            None,
        );

        self.buy_order = Some(buy_order.clone());
        self.sell_order = Some(sell_order.clone());

        let client_id = self.config.client_id;
        if let Err(e) = self.submit_order_list(vec![buy_order, sell_order], None, client_id, None) {
            log::error!("Failed to submit batch limit pair: {e}");
        }
    }

    /// Maintain stop buy orders.
    fn maintain_stop_buy_orders(&mut self, best_bid: Price, best_ask: Price) {
        self.refresh_tracked_stop_order(OrderSide::Buy);

        let Some(instrument) = &self.instrument else {
            return;
        };

        let increment = instrument.price_increment();
        let precision = instrument.price_precision();
        let stop_offset_ticks = self.config.stop_offset_ticks;

        // Determine trigger price based on order type
        let raw_trigger_price = if matches!(
            self.config.stop_order_type,
            OrderType::LimitIfTouched | OrderType::MarketIfTouched | OrderType::TrailingStopMarket
        ) {
            // IF_TOUCHED and trailing-stop buy: place BELOW market
            sub_price_ticks(best_bid, increment, stop_offset_ticks, precision)
        } else {
            // STOP buy orders are placed ABOVE the market (stop loss on short)
            add_price_ticks(best_ask, increment, stop_offset_ticks, precision)
        };
        let clamp = self.config.clamp_to_instrument_price_range;
        let trigger_price = clamp_price_to_range(raw_trigger_price, instrument, clamp);

        // Calculate limit price if needed
        let limit_price = if matches!(
            self.config.stop_order_type,
            OrderType::StopLimit | OrderType::LimitIfTouched
        ) {
            let raw_limit = if let Some(limit_offset_ticks) = self.config.stop_limit_offset_ticks {
                // BUY LIT/StopLimit both require trigger_price <= price.
                add_price_ticks(trigger_price, increment, limit_offset_ticks, precision)
            } else {
                trigger_price
            };
            Some(clamp_price_to_range(raw_limit, instrument, clamp))
        } else {
            None
        };

        let needs_new_order = match &self.buy_stop_order {
            None => true,
            Some(order) => !Self::is_order_active(order) && !self.stop_order_is_one_shot(),
        };

        if needs_new_order {
            if let Err(e) = self.submit_stop_order(OrderSide::Buy, trigger_price, limit_price) {
                log::error!("Failed to submit buy stop order: {e}");
            }
        } else if let Some(order) = &self.buy_stop_order
            && order.venue_order_id().is_some()
            && !order.is_pending_update()
            && !order.is_pending_cancel()
        {
            let current_trigger = Self::get_order_trigger_price(order);
            if current_trigger.is_some() && current_trigger != Some(trigger_price) {
                if self.config.modify_stop_orders_to_maintain_offset {
                    let order_clone = order.clone();
                    if let Err(e) = self.modify_stop_order(&order_clone, trigger_price, limit_price)
                    {
                        log::error!("Failed to modify buy stop order: {e}");
                    }
                } else if self.config.cancel_replace_stop_orders_to_maintain_offset
                    && !self.buy_stop_cancel_replace_attempted
                {
                    self.buy_stop_cancel_replace_attempted = true;
                    let order_clone = order.clone();
                    let _ = self.cancel_order(
                        order_clone.client_order_id(),
                        self.config.client_id,
                        None,
                    );

                    if let Err(e) =
                        self.submit_stop_order(OrderSide::Buy, trigger_price, limit_price)
                    {
                        log::error!("Failed to submit replacement buy stop order: {e}");
                    }
                }
            }
        }
    }

    /// Maintain stop sell orders.
    fn maintain_stop_sell_orders(&mut self, best_bid: Price, best_ask: Price) {
        self.refresh_tracked_stop_order(OrderSide::Sell);

        let Some(instrument) = &self.instrument else {
            return;
        };

        let increment = instrument.price_increment();
        let precision = instrument.price_precision();
        let stop_offset_ticks = self.config.stop_offset_ticks;

        // Determine trigger price based on order type
        let raw_trigger_price = if matches!(
            self.config.stop_order_type,
            OrderType::LimitIfTouched | OrderType::MarketIfTouched | OrderType::TrailingStopMarket
        ) {
            // IF_TOUCHED and trailing-stop sell: place ABOVE market
            add_price_ticks(best_ask, increment, stop_offset_ticks, precision)
        } else {
            // STOP sell orders are placed BELOW the market (stop loss on long)
            sub_price_ticks(best_bid, increment, stop_offset_ticks, precision)
        };
        let clamp = self.config.clamp_to_instrument_price_range;
        let trigger_price = clamp_price_to_range(raw_trigger_price, instrument, clamp);

        // Calculate limit price if needed
        let limit_price = if matches!(
            self.config.stop_order_type,
            OrderType::StopLimit | OrderType::LimitIfTouched
        ) {
            let raw_limit = if let Some(limit_offset_ticks) = self.config.stop_limit_offset_ticks {
                // SELL LIT/StopLimit both require trigger_price >= price.
                sub_price_ticks(trigger_price, increment, limit_offset_ticks, precision)
            } else {
                trigger_price
            };
            Some(clamp_price_to_range(raw_limit, instrument, clamp))
        } else {
            None
        };

        let needs_new_order = match &self.sell_stop_order {
            None => true,
            Some(order) => !Self::is_order_active(order) && !self.stop_order_is_one_shot(),
        };

        if needs_new_order {
            if let Err(e) = self.submit_stop_order(OrderSide::Sell, trigger_price, limit_price) {
                log::error!("Failed to submit sell stop order: {e}");
            }
        } else if let Some(order) = &self.sell_stop_order
            && order.venue_order_id().is_some()
            && !order.is_pending_update()
            && !order.is_pending_cancel()
        {
            let current_trigger = Self::get_order_trigger_price(order);
            if current_trigger.is_some() && current_trigger != Some(trigger_price) {
                if self.config.modify_stop_orders_to_maintain_offset {
                    let order_clone = order.clone();
                    if let Err(e) = self.modify_stop_order(&order_clone, trigger_price, limit_price)
                    {
                        log::error!("Failed to modify sell stop order: {e}");
                    }
                } else if self.config.cancel_replace_stop_orders_to_maintain_offset
                    && !self.sell_stop_cancel_replace_attempted
                {
                    self.sell_stop_cancel_replace_attempted = true;
                    let order_clone = order.clone();
                    let _ = self.cancel_order(
                        order_clone.client_order_id(),
                        self.config.client_id,
                        None,
                    );

                    if let Err(e) =
                        self.submit_stop_order(OrderSide::Sell, trigger_price, limit_price)
                    {
                        log::error!("Failed to submit replacement sell stop order: {e}");
                    }
                }
            }
        }
    }

    /// Submit a limit order.
    ///
    /// # Errors
    ///
    /// Returns an error if order creation or submission fails.
    pub(super) fn submit_limit_order(
        &mut self,
        order_side: OrderSide,
        price: Price,
    ) -> anyhow::Result<()> {
        let Some(instrument) = &self.instrument else {
            anyhow::bail!("No instrument loaded");
        };

        if self.config.dry_run {
            log_warn!("Dry run, skipping create {order_side:?} order");
            return Ok(());
        }

        if order_side == OrderSide::Buy && !self.config.enable_limit_buys {
            log_warn!("BUY orders not enabled, skipping");
            return Ok(());
        } else if order_side == OrderSide::Sell && !self.config.enable_limit_sells {
            log_warn!("SELL orders not enabled, skipping");
            return Ok(());
        }

        let (time_in_force, expire_time) =
            self.resolve_time_in_force(self.config.limit_time_in_force);

        let quantity = instrument.make_qty(self.config.order_qty.as_f64(), None);

        let order = self.core.order_factory().limit(
            self.config.instrument_id,
            order_side,
            quantity,
            price,
            Some(time_in_force),
            expire_time,
            Some(self.config.use_post_only || self.config.test_reject_post_only),
            None, // reduce_only
            Some(self.config.use_quote_quantity),
            self.config.order_display_qty,
            self.config.emulation_trigger,
            None, // trigger_instrument_id
            None, // exec_algorithm_id
            None, // exec_algorithm_params
            None, // tags
            None, // client_order_id
        );

        if order_side == OrderSide::Buy {
            self.buy_order = Some(order.clone());
        } else {
            self.sell_order = Some(order.clone());
        }

        self.submit_order_apply_params(order)
    }

    /// Submit a stop order.
    ///
    /// # Errors
    ///
    /// Returns an error if order creation or submission fails.
    #[expect(
        clippy::too_many_lines,
        reason = "stop order submission covers all supported stop order scenarios"
    )]
    pub(super) fn submit_stop_order(
        &mut self,
        order_side: OrderSide,
        trigger_price: Price,
        limit_price: Option<Price>,
    ) -> anyhow::Result<()> {
        let Some(instrument) = &self.instrument else {
            anyhow::bail!("No instrument loaded");
        };

        if self.config.dry_run {
            log_warn!("Dry run, skipping create {order_side:?} stop order");
            return Ok(());
        }

        if order_side == OrderSide::Buy && !self.config.enable_stop_buys {
            log_warn!("BUY stop orders not enabled, skipping");
            return Ok(());
        } else if order_side == OrderSide::Sell && !self.config.enable_stop_sells {
            log_warn!("SELL stop orders not enabled, skipping");
            return Ok(());
        }

        let (time_in_force, expire_time) =
            self.resolve_time_in_force(self.config.stop_time_in_force);

        // Use instrument's make_qty to ensure correct precision
        let quantity = instrument.make_qty(self.config.order_qty.as_f64(), None);

        let factory = self.core.order_factory();

        let mut order: OrderAny = match self.config.stop_order_type {
            OrderType::StopMarket => factory.stop_market(
                self.config.instrument_id,
                order_side,
                quantity,
                trigger_price,
                Some(self.config.stop_trigger_type),
                Some(time_in_force),
                expire_time,
                None, // reduce_only
                Some(self.config.use_quote_quantity),
                None, // display_qty
                self.config.emulation_trigger,
                None, // trigger_instrument_id
                None, // exec_algorithm_id
                None, // exec_algorithm_params
                None, // tags
                None, // client_order_id
            ),
            OrderType::StopLimit => {
                let Some(limit_price) = limit_price else {
                    anyhow::bail!("STOP_LIMIT order requires limit_price");
                };
                factory.stop_limit(
                    self.config.instrument_id,
                    order_side,
                    quantity,
                    limit_price,
                    trigger_price,
                    Some(self.config.stop_trigger_type),
                    Some(time_in_force),
                    expire_time,
                    None, // post_only
                    None, // reduce_only
                    Some(self.config.use_quote_quantity),
                    self.config.order_display_qty,
                    self.config.emulation_trigger,
                    None, // trigger_instrument_id
                    None, // exec_algorithm_id
                    None, // exec_algorithm_params
                    None, // tags
                    None, // client_order_id
                )
            }
            OrderType::MarketIfTouched => factory.market_if_touched(
                self.config.instrument_id,
                order_side,
                quantity,
                trigger_price,
                Some(self.config.stop_trigger_type),
                Some(time_in_force),
                expire_time,
                None, // reduce_only
                Some(self.config.use_quote_quantity),
                self.config.emulation_trigger,
                None, // trigger_instrument_id
                None, // exec_algorithm_id
                None, // exec_algorithm_params
                None, // tags
                None, // client_order_id
            ),
            OrderType::LimitIfTouched => {
                let Some(limit_price) = limit_price else {
                    anyhow::bail!("LIMIT_IF_TOUCHED order requires limit_price");
                };
                factory.limit_if_touched(
                    self.config.instrument_id,
                    order_side,
                    quantity,
                    limit_price,
                    trigger_price,
                    Some(self.config.stop_trigger_type),
                    Some(time_in_force),
                    expire_time,
                    None, // post_only
                    None, // reduce_only
                    Some(self.config.use_quote_quantity),
                    self.config.order_display_qty,
                    self.config.emulation_trigger,
                    None, // trigger_instrument_id
                    None, // exec_algorithm_id
                    None, // exec_algorithm_params
                    None, // tags
                    None, // client_order_id
                )
            }
            OrderType::TrailingStopMarket => {
                let Some(trailing_offset) = self.config.trailing_offset else {
                    anyhow::bail!("TRAILING_STOP_MARKET order requires trailing_offset config");
                };
                factory.trailing_stop_market(
                    self.config.instrument_id,
                    order_side,
                    quantity,
                    trailing_offset,
                    Some(self.config.trailing_offset_type),
                    None,
                    Some(trigger_price),
                    Some(self.config.stop_trigger_type),
                    Some(time_in_force),
                    expire_time,
                    None, // reduce_only
                    Some(self.config.use_quote_quantity),
                    None, // display_qty
                    self.config.emulation_trigger,
                    None, // trigger_instrument_id
                    None, // exec_algorithm_id
                    None, // exec_algorithm_params
                    None, // tags
                    None, // client_order_id
                )
            }
            _ => {
                anyhow::bail!("Unknown stop order type: {:?}", self.config.stop_order_type);
            }
        };

        if let OrderAny::TrailingStopMarket(order) = &mut order {
            order.activation_price = Some(trigger_price);
        }

        if order_side == OrderSide::Buy {
            self.buy_stop_order = Some(order.clone());
        } else {
            self.sell_stop_order = Some(order.clone());
        }

        self.submit_order_apply_params(order)
    }

    /// Submit a bracket order (entry with stop-loss and take-profit).
    ///
    /// # Errors
    ///
    /// Returns an error if order creation or submission fails.
    pub(super) fn submit_bracket_order(
        &mut self,
        order_side: OrderSide,
        entry_price: Price,
    ) -> anyhow::Result<()> {
        let Some(instrument) = &self.instrument else {
            anyhow::bail!("No instrument loaded");
        };

        if self.config.dry_run {
            log_warn!("Dry run, skipping create {order_side:?} bracket order");
            return Ok(());
        }

        if self.config.bracket_entry_order_type != OrderType::Limit {
            anyhow::bail!(
                "Only Limit entry orders are supported for brackets, was {:?}",
                self.config.bracket_entry_order_type
            );
        }

        if order_side == OrderSide::Buy && !self.config.enable_limit_buys {
            log_warn!("BUY orders not enabled, skipping bracket");
            return Ok(());
        } else if order_side == OrderSide::Sell && !self.config.enable_limit_sells {
            log_warn!("SELL orders not enabled, skipping bracket");
            return Ok(());
        }

        let (time_in_force, expire_time) =
            self.resolve_time_in_force(self.config.limit_time_in_force);
        let sl_time_in_force = self.config.stop_time_in_force.unwrap_or(TimeInForce::Gtc);
        if sl_time_in_force == TimeInForce::Gtd {
            anyhow::bail!("GTD time in force not supported for bracket stop-loss legs");
        }

        let quantity = instrument.make_qty(self.config.order_qty.as_f64(), None);
        let increment = instrument.price_increment();
        let precision = instrument.price_precision();
        let bracket_offset_ticks = self.config.bracket_offset_ticks;

        let (raw_tp_price, raw_sl_trigger_price) = match order_side {
            OrderSide::Buy => {
                let tp = add_price_ticks(entry_price, increment, bracket_offset_ticks, precision);
                let sl = sub_price_ticks(entry_price, increment, bracket_offset_ticks, precision);
                (tp, sl)
            }
            OrderSide::Sell => {
                let tp = sub_price_ticks(entry_price, increment, bracket_offset_ticks, precision);
                let sl = add_price_ticks(entry_price, increment, bracket_offset_ticks, precision);
                (tp, sl)
            }
            OrderSide::NoOrderSide => {
                anyhow::bail!("Invalid order side for bracket: {order_side:?}")
            }
        };
        let clamp = self.config.clamp_to_instrument_price_range;
        let tp_price = clamp_price_to_range(raw_tp_price, instrument, clamp);
        let sl_trigger_price = clamp_price_to_range(raw_sl_trigger_price, instrument, clamp);

        let entry_post_only = self.config.use_post_only || self.config.test_reject_post_only;
        let orders = self
            .core
            .order_factory()
            .bracket()
            .instrument_id(self.config.instrument_id)
            .order_side(order_side)
            .quantity(quantity)
            .quote_quantity(self.config.use_quote_quantity)
            .entry_order_type(OrderType::Limit)
            .entry_price(entry_price)
            .time_in_force(time_in_force)
            .entry_post_only(entry_post_only)
            .maybe_emulation_trigger(self.config.emulation_trigger)
            .maybe_expire_time(expire_time)
            .tp_price(tp_price)
            .tp_post_only(entry_post_only)
            .tp_time_in_force(time_in_force)
            .sl_trigger_price(sl_trigger_price)
            .sl_trigger_type(self.config.stop_trigger_type)
            .sl_time_in_force(sl_time_in_force)
            .call();

        if let Some(entry_order) = orders.first() {
            if order_side == OrderSide::Buy {
                self.buy_order = Some(entry_order.clone());
            } else {
                self.sell_order = Some(entry_order.clone());
            }
        }

        let client_id = self.config.client_id;
        if let Some(params) = &self.config.order_params {
            self.submit_order_list(orders, None, client_id, Some(params.clone()))
        } else {
            self.submit_order_list(orders, None, client_id, None)
        }
    }

    /// Open a position with a market order.
    ///
    /// # Errors
    ///
    /// Returns an error if order creation or submission fails.
    pub(super) fn open_position(&mut self, net_qty: Decimal) -> anyhow::Result<()> {
        let Some(instrument) = &self.instrument else {
            anyhow::bail!("No instrument loaded");
        };

        if net_qty == Decimal::ZERO {
            log_warn!("Open position with zero quantity, skipping");
            return Ok(());
        }

        let order_side = if net_qty > Decimal::ZERO {
            OrderSide::Buy
        } else {
            OrderSide::Sell
        };

        let quantity = instrument.make_qty(net_qty.abs().to_f64().unwrap_or(0.0), None);

        // Test reduce_only rejection by setting reduce_only on open position order
        let reduce_only = if self.config.test_reject_reduce_only {
            Some(true)
        } else {
            None
        };

        let order = self.core.order_factory().market(
            self.config.instrument_id,
            order_side,
            quantity,
            Some(self.config.open_position_time_in_force),
            reduce_only,
            Some(self.config.use_quote_quantity),
            None, // exec_algorithm_id
            None, // exec_algorithm_params
            None, // tags
            None, // client_order_id
        );

        self.submit_order_apply_params(order)
    }

    pub(super) fn cancel_active_orders(
        &mut self,
        instrument_id: InstrumentId,
        strategy_id: StrategyId,
        client_id: Option<ClientId>,
    ) {
        // Reach INITIALIZED contingent legs that the open/emulated/inflight indexes
        // miss. Skip non-bracket lists so the configured cancel mode owns them.
        let bracket_targets: Vec<ClientOrderId> = {
            let cache = self.cache();
            let mut targets = Vec::new();

            for order_list in
                cache.order_lists(None, Some(&instrument_id), Some(&strategy_id), None)
            {
                let is_bracket = order_list.client_order_ids.iter().any(|cid| {
                    cache
                        .order(cid)
                        .is_some_and(|o| is_in_contingency_group(&o))
                });

                if !is_bracket {
                    continue;
                }

                for cid in &order_list.client_order_ids {
                    if let Some(order) = cache.order(cid)
                        && !order.is_closed()
                        && !order.is_pending_cancel()
                    {
                        targets.push(*cid);
                    }
                }
            }
            targets
        };

        for cid in bracket_targets {
            if let Err(e) = self.cancel_order(cid, client_id, None) {
                log::error!("Failed to cancel bracket leg {cid}: {e}");
            }
        }

        if self.config.use_individual_cancels_on_stop {
            for cid in self.collect_cancellable_order_ids(instrument_id, strategy_id) {
                if let Err(e) = self.cancel_order(cid, client_id, None) {
                    log::error!("Failed to cancel order {cid}: {e}");
                }
            }
        } else if self.config.use_batch_cancel_on_stop {
            let candidates = self.collect_cancellable_orders(instrument_id, strategy_id);
            let mut batchable: Vec<ClientOrderId> = Vec::new();

            for order in candidates {
                let cid = order.client_order_id();
                if order.is_emulated() || order.is_active_local() {
                    if let Err(e) = self.cancel_order(cid, client_id, None) {
                        log::error!("Failed to cancel local order {cid}: {e}");
                    }
                } else {
                    batchable.push(cid);
                }
            }

            if !batchable.is_empty()
                && let Err(e) = self.cancel_orders(batchable, client_id, None)
            {
                log::error!("Failed to batch cancel orders: {e}");
            }
        } else {
            // `cancel_all_orders` does not reach active-local orders; cancel those
            // individually first. Brackets are handled by the sweep above.
            let local_ids: Vec<ClientOrderId> = {
                let cache = self.cache();
                cache
                    .orders_active_local(None, Some(&instrument_id), Some(&strategy_id), None, None)
                    .into_iter()
                    .filter(|o| {
                        !o.is_closed() && !o.is_pending_cancel() && !is_in_contingency_group(o)
                    })
                    .map(|o| o.client_order_id())
                    .collect()
            };

            for cid in local_ids {
                if let Err(e) = self.cancel_order(cid, client_id, None) {
                    log::error!("Failed to cancel active-local order {cid}: {e}");
                }
            }

            if let Err(e) = self.cancel_all_orders(instrument_id, None, client_id, None) {
                log::error!("Failed to cancel all orders: {e}");
            }
        }
    }

    pub(super) fn collect_cancellable_orders(
        &self,
        instrument_id: InstrumentId,
        strategy_id: StrategyId,
    ) -> Vec<OrderAny> {
        let cache = self.cache();
        let mut seen: AHashSet<ClientOrderId> = AHashSet::new();
        let mut candidates: Vec<OrderAny> = Vec::new();
        // `orders_active_local` catches just-submitted orders not yet in the other
        // indexes. Bracket legs are excluded; the sweep in `cancel_active_orders` owns them.
        let sources = [
            cache.orders_active_local(None, Some(&instrument_id), Some(&strategy_id), None, None),
            cache.orders_emulated(None, Some(&instrument_id), Some(&strategy_id), None, None),
            cache.orders_inflight(None, Some(&instrument_id), Some(&strategy_id), None, None),
            cache.orders_open(None, Some(&instrument_id), Some(&strategy_id), None, None),
        ];

        for orders in sources {
            for order in orders {
                if order.is_closed() || order.is_pending_cancel() || is_in_contingency_group(&order)
                {
                    continue;
                }
                let cid = order.client_order_id();
                if seen.insert(cid) {
                    candidates.push(order.cloned());
                }
            }
        }
        candidates
    }

    pub(super) fn collect_cancellable_order_ids(
        &self,
        instrument_id: InstrumentId,
        strategy_id: StrategyId,
    ) -> Vec<ClientOrderId> {
        self.collect_cancellable_orders(instrument_id, strategy_id)
            .into_iter()
            .map(|o| o.client_order_id())
            .collect()
    }
}

fn add_price_ticks(base: Price, increment: Price, ticks: u64, precision: u8) -> Price {
    let offset_raw = tick_offset_raw(increment, ticks);
    Price::from_raw(base.raw + offset_raw, precision)
}

fn sub_price_ticks(base: Price, increment: Price, ticks: u64, precision: u8) -> Price {
    let offset_raw = tick_offset_raw(increment, ticks);
    Price::from_raw(base.raw - offset_raw, precision)
}

fn tick_offset_raw(increment: Price, ticks: u64) -> PriceRaw {
    #[cfg(feature = "high-precision")]
    let ticks_raw = PriceRaw::from(ticks);
    #[cfg(not(feature = "high-precision"))]
    let ticks_raw = PriceRaw::try_from(ticks).expect("tick offset must fit PriceRaw");

    increment.raw * ticks_raw
}

// `OrderAny::is_contingency` returns true for `Some(NoContingency)` (the factory
// default on every order), so match the variant directly to distinguish bracket legs.
fn is_in_contingency_group(order: &OrderAny) -> bool {
    matches!(
        order.contingency_type(),
        Some(ContingencyType::Oto | ContingencyType::Oco | ContingencyType::Ouo)
    )
}

fn clamp_price_to_range(price: Price, instrument: &InstrumentAny, enabled: bool) -> Price {
    if !enabled {
        return price;
    }
    let mut clamped = price;
    if let Some(min) = instrument.min_price()
        && clamped < min
    {
        clamped = min;
    }

    if let Some(max) = instrument.max_price()
        && clamped > max
    {
        clamped = max;
    }

    clamped
}
