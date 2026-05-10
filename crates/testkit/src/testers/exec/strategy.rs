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

use nautilus_common::{actor::DataActor, enums::LogColor, log_info, log_warn, timer::TimeEvent};
use nautilus_core::{UnixNanos, datetime::secs_to_nanos_unchecked};
use nautilus_model::{
    data::{Bar, IndexPriceUpdate, MarkPriceUpdate, OrderBookDeltas, QuoteTick, TradeTick},
    enums::{OrderSide, OrderType, TimeInForce},
    identifiers::{InstrumentId, StrategyId},
    instruments::{Instrument, InstrumentAny},
    orderbook::OrderBook,
    orders::{Order, OrderAny},
    types::Price,
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
pub struct ExecTester {
    pub(super) core: StrategyCore,
    pub(super) config: ExecTesterConfig,
    pub(super) instrument: Option<InstrumentAny>,
    pub(super) price_offset: Option<f64>,
    pub(super) preinitialized_market_data: bool,

    // Order tracking
    pub(super) buy_order: Option<OrderAny>,
    pub(super) sell_order: Option<OrderAny>,
    pub(super) buy_stop_order: Option<OrderAny>,
    pub(super) sell_stop_order: Option<OrderAny>,
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

        if self.config.cancel_orders_on_stop {
            let strategy_id = StrategyId::from(self.core.actor_id.inner().as_str());

            if self.config.use_individual_cancels_on_stop {
                let cache = self.cache();
                let open_orders: Vec<OrderAny> = cache
                    .orders_open(None, Some(&instrument_id), Some(&strategy_id), None, None)
                    .iter()
                    .map(|o| (*o).clone())
                    .collect();
                drop(cache);

                for order in open_orders {
                    if let Err(e) = self.cancel_order(order, client_id) {
                        log::error!("Failed to cancel order: {e}");
                    }
                }
            } else if self.config.use_batch_cancel_on_stop {
                let cache = self.cache();
                let open_orders: Vec<OrderAny> = cache
                    .orders_open(None, Some(&instrument_id), Some(&strategy_id), None, None)
                    .iter()
                    .map(|o| (*o).clone())
                    .collect();
                drop(cache);

                if let Err(e) = self.cancel_orders(open_orders, client_id, None) {
                    log::error!("Failed to batch cancel orders: {e}");
                }
            } else if let Err(e) = self.cancel_all_orders(instrument_id, None, client_id) {
                log::error!("Failed to cancel all orders: {e}");
            }
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

        if let Some(qty) = self.config.open_position_on_start_qty {
            self.open_position(qty)?;
        }

        Ok(())
    }

    pub(super) fn get_price_offset(&self, instrument: &InstrumentAny) -> f64 {
        instrument.price_increment().as_f64() * self.config.tob_offset_ticks as f64
    }

    fn expire_time_from_delta(&self, mins: u64) -> UnixNanos {
        let current_ns = self.timestamp_ns();
        let delta_ns = secs_to_nanos_unchecked((mins * 60) as f64);
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

    pub(super) fn is_order_active(&self, order: &OrderAny) -> bool {
        order.is_active_local() || order.is_inflight() || order.is_open()
    }

    pub(super) fn get_order_trigger_price(&self, order: &OrderAny) -> Option<Price> {
        order.trigger_price()
    }

    fn modify_stop_order(
        &mut self,
        order: OrderAny,
        trigger_price: Price,
        limit_price: Option<Price>,
    ) -> anyhow::Result<()> {
        let client_id = self.config.client_id;

        match &order {
            OrderAny::StopMarket(_)
            | OrderAny::MarketIfTouched(_)
            | OrderAny::TrailingStopMarket(_) => {
                self.modify_order(order, None, None, Some(trigger_price), client_id)
            }
            OrderAny::StopLimit(_) | OrderAny::LimitIfTouched(_) => {
                self.modify_order(order, None, limit_price, Some(trigger_price), client_id)
            }
            _ => {
                log_warn!("Cannot modify order of type {:?}", order.order_type());
                Ok(())
            }
        }
    }

    /// Submit an order, applying order_params if configured.
    fn submit_order_apply_params(&mut self, order: OrderAny) -> anyhow::Result<()> {
        let client_id = self.config.client_id;
        if let Some(params) = &self.config.order_params {
            self.submit_order_with_params(order, None, client_id, params.clone())
        } else {
            self.submit_order(order, None, client_id)
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

    /// Maintain buy limit orders.
    fn maintain_buy_orders(&mut self, best_bid: Price, best_ask: Price) {
        let Some(instrument) = &self.instrument else {
            return;
        };
        let Some(price_offset) = self.price_offset else {
            return;
        };

        // test_reject_post_only places order on wrong side of spread to trigger rejection
        let price = if self.config.test_reject_post_only {
            instrument.make_price(best_ask.as_f64() + price_offset)
        } else {
            instrument.make_price(best_bid.as_f64() - price_offset)
        };

        let needs_new_order = match &self.buy_order {
            None => true,
            Some(order) => !self.is_order_active(order),
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
            && let Some(order_price) = order.price()
            && order_price < price
        {
            let client_id = self.config.client_id;
            if self.config.modify_orders_to_maintain_tob_offset {
                let order_clone = order.clone();
                if let Err(e) = self.modify_order(order_clone, None, Some(price), None, client_id) {
                    log::error!("Failed to modify buy order: {e}");
                }
            } else if self.config.cancel_replace_orders_to_maintain_tob_offset {
                let order_clone = order.clone();
                let _ = self.cancel_order(order_clone, client_id);

                if let Err(e) = self.submit_limit_order(OrderSide::Buy, price) {
                    log::error!("Failed to submit replacement buy order: {e}");
                }
            }
        }
    }

    /// Maintain sell limit orders.
    fn maintain_sell_orders(&mut self, best_bid: Price, best_ask: Price) {
        let Some(instrument) = &self.instrument else {
            return;
        };
        let Some(price_offset) = self.price_offset else {
            return;
        };

        // test_reject_post_only places order on wrong side of spread to trigger rejection
        let price = if self.config.test_reject_post_only {
            instrument.make_price(best_bid.as_f64() - price_offset)
        } else {
            instrument.make_price(best_ask.as_f64() + price_offset)
        };

        let needs_new_order = match &self.sell_order {
            None => true,
            Some(order) => !self.is_order_active(order),
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
            && let Some(order_price) = order.price()
            && order_price > price
        {
            let client_id = self.config.client_id;
            if self.config.modify_orders_to_maintain_tob_offset {
                let order_clone = order.clone();
                if let Err(e) = self.modify_order(order_clone, None, Some(price), None, client_id) {
                    log::error!("Failed to modify sell order: {e}");
                }
            } else if self.config.cancel_replace_orders_to_maintain_tob_offset {
                let order_clone = order.clone();
                let _ = self.cancel_order(order_clone, client_id);

                if let Err(e) = self.submit_limit_order(OrderSide::Sell, price) {
                    log::error!("Failed to submit replacement sell order: {e}");
                }
            }
        }
    }

    /// Submits a buy and sell limit order as an order list (batch).
    fn maintain_batch_limit_pair(&mut self, best_bid: Price, best_ask: Price) {
        let Some(instrument) = &self.instrument else {
            return;
        };
        let Some(price_offset) = self.price_offset else {
            return;
        };

        let buy_needs = match &self.buy_order {
            None => true,
            Some(order) => !self.is_order_active(order),
        };
        let sell_needs = match &self.sell_order {
            None => true,
            Some(order) => !self.is_order_active(order),
        };

        if !buy_needs || !sell_needs {
            return;
        }

        let buy_price = instrument.make_price(best_bid.as_f64() - price_offset);
        let sell_price = instrument.make_price(best_ask.as_f64() + price_offset);
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
        if let Err(e) = self.submit_order_list(vec![buy_order, sell_order], None, client_id) {
            log::error!("Failed to submit batch limit pair: {e}");
        }
    }

    /// Maintain stop buy orders.
    fn maintain_stop_buy_orders(&mut self, best_bid: Price, best_ask: Price) {
        let Some(instrument) = &self.instrument else {
            return;
        };

        let price_increment = instrument.price_increment().as_f64();
        let stop_offset = price_increment * self.config.stop_offset_ticks as f64;

        // Determine trigger price based on order type
        let trigger_price = if matches!(
            self.config.stop_order_type,
            OrderType::LimitIfTouched | OrderType::MarketIfTouched | OrderType::TrailingStopMarket
        ) {
            // IF_TOUCHED and trailing-stop buy: place BELOW market
            instrument.make_price(best_bid.as_f64() - stop_offset)
        } else {
            // STOP buy orders are placed ABOVE the market (stop loss on short)
            instrument.make_price(best_ask.as_f64() + stop_offset)
        };

        // Calculate limit price if needed
        let limit_price = if matches!(
            self.config.stop_order_type,
            OrderType::StopLimit | OrderType::LimitIfTouched
        ) {
            if let Some(limit_offset_ticks) = self.config.stop_limit_offset_ticks {
                let limit_offset = price_increment * limit_offset_ticks as f64;

                if self.config.stop_order_type == OrderType::LimitIfTouched {
                    Some(instrument.make_price(trigger_price.as_f64() - limit_offset))
                } else {
                    Some(instrument.make_price(trigger_price.as_f64() + limit_offset))
                }
            } else {
                Some(trigger_price)
            }
        } else {
            None
        };

        let needs_new_order = match &self.buy_stop_order {
            None => true,
            Some(order) => !self.is_order_active(order),
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
            let current_trigger = self.get_order_trigger_price(order);
            if current_trigger.is_some() && current_trigger != Some(trigger_price) {
                if self.config.modify_stop_orders_to_maintain_offset {
                    let order_clone = order.clone();
                    if let Err(e) = self.modify_stop_order(order_clone, trigger_price, limit_price)
                    {
                        log::error!("Failed to modify buy stop order: {e}");
                    }
                } else if self.config.cancel_replace_stop_orders_to_maintain_offset {
                    let order_clone = order.clone();
                    let _ = self.cancel_order(order_clone, self.config.client_id);

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
        let Some(instrument) = &self.instrument else {
            return;
        };

        let price_increment = instrument.price_increment().as_f64();
        let stop_offset = price_increment * self.config.stop_offset_ticks as f64;

        // Determine trigger price based on order type
        let trigger_price = if matches!(
            self.config.stop_order_type,
            OrderType::LimitIfTouched | OrderType::MarketIfTouched | OrderType::TrailingStopMarket
        ) {
            // IF_TOUCHED and trailing-stop sell: place ABOVE market
            instrument.make_price(best_ask.as_f64() + stop_offset)
        } else {
            // STOP sell orders are placed BELOW the market (stop loss on long)
            instrument.make_price(best_bid.as_f64() - stop_offset)
        };

        // Calculate limit price if needed
        let limit_price = if matches!(
            self.config.stop_order_type,
            OrderType::StopLimit | OrderType::LimitIfTouched
        ) {
            if let Some(limit_offset_ticks) = self.config.stop_limit_offset_ticks {
                let limit_offset = price_increment * limit_offset_ticks as f64;

                if self.config.stop_order_type == OrderType::LimitIfTouched {
                    Some(instrument.make_price(trigger_price.as_f64() + limit_offset))
                } else {
                    Some(instrument.make_price(trigger_price.as_f64() - limit_offset))
                }
            } else {
                Some(trigger_price)
            }
        } else {
            None
        };

        let needs_new_order = match &self.sell_stop_order {
            None => true,
            Some(order) => !self.is_order_active(order),
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
            let current_trigger = self.get_order_trigger_price(order);
            if current_trigger.is_some() && current_trigger != Some(trigger_price) {
                if self.config.modify_stop_orders_to_maintain_offset {
                    let order_clone = order.clone();
                    if let Err(e) = self.modify_stop_order(order_clone, trigger_price, limit_price)
                    {
                        log::error!("Failed to modify sell stop order: {e}");
                    }
                } else if self.config.cancel_replace_stop_orders_to_maintain_offset {
                    let order_clone = order.clone();
                    let _ = self.cancel_order(order_clone, self.config.client_id);

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
        let price_increment = instrument.price_increment().as_f64();
        let bracket_offset = price_increment * self.config.bracket_offset_ticks as f64;

        let (tp_price, sl_trigger_price) = match order_side {
            OrderSide::Buy => {
                let tp = instrument.make_price(entry_price.as_f64() + bracket_offset);
                let sl = instrument.make_price(entry_price.as_f64() - bracket_offset);
                (tp, sl)
            }
            OrderSide::Sell => {
                let tp = instrument.make_price(entry_price.as_f64() - bracket_offset);
                let sl = instrument.make_price(entry_price.as_f64() + bracket_offset);
                (tp, sl)
            }
            _ => anyhow::bail!("Invalid order side for bracket: {order_side:?}"),
        };

        let orders = self.core.order_factory().bracket(
            self.config.instrument_id,
            order_side,
            quantity,
            Some(entry_price),                   // entry_price
            sl_trigger_price,                    // sl_trigger_price
            Some(self.config.stop_trigger_type), // sl_trigger_type
            tp_price,                            // tp_price
            None,                                // entry_trigger_price (limit entry, no trigger)
            Some(time_in_force),
            expire_time,
            Some(sl_time_in_force),
            Some(self.config.use_post_only || self.config.test_reject_post_only),
            None, // reduce_only
            Some(self.config.use_quote_quantity),
            self.config.emulation_trigger,
            None, // trigger_instrument_id
            None, // exec_algorithm_id
            None, // exec_algorithm_params
            None, // tags
        );

        if let Some(entry_order) = orders.first() {
            if order_side == OrderSide::Buy {
                self.buy_order = Some(entry_order.clone());
            } else {
                self.sell_order = Some(entry_order.clone());
            }
        }

        let client_id = self.config.client_id;
        if let Some(params) = &self.config.order_params {
            self.submit_order_list_with_params(orders, None, client_id, params.clone())
        } else {
            self.submit_order_list(orders, None, client_id)
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
}
