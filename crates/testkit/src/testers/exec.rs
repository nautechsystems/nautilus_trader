// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

//! Execution tester strategy for live testing order execution.

use std::{
    num::NonZeroUsize,
    ops::{Deref, DerefMut},
};

use nautilus_common::{
    actor::{DataActor, DataActorCore},
    enums::LogColor,
    log_info, log_warn,
    timer::TimeEvent,
};
use nautilus_model::{
    data::{QuoteTick, TradeTick},
    enums::{BookType, OrderSide, OrderStatus, OrderType, TimeInForce, TriggerType},
    identifiers::{ClientId, InstrumentId, StrategyId},
    instruments::{Instrument, InstrumentAny},
    orderbook::OrderBook,
    orders::{Order, OrderAny},
    types::{Price, Quantity},
};
use nautilus_trading::strategy::{Strategy, StrategyConfig, StrategyCore};
use rust_decimal::{Decimal, prelude::ToPrimitive};

/// Configuration for the execution tester strategy.
#[derive(Debug, Clone)]
pub struct ExecTesterConfig {
    /// Base strategy configuration.
    pub base: StrategyConfig,
    /// Instrument ID to test.
    pub instrument_id: InstrumentId,
    /// Client ID to use for orders and subscriptions.
    pub client_id: Option<ClientId>,
    /// Order quantity.
    pub order_qty: Quantity,
    /// Display quantity for iceberg orders (None for full display, Some(0) for hidden).
    pub order_display_qty: Option<Quantity>,
    /// Minutes until GTD orders expire (None for GTC).
    pub order_expire_time_delta_mins: Option<u64>,
    /// Whether to subscribe to quotes.
    pub subscribe_quotes: bool,
    /// Whether to subscribe to trades.
    pub subscribe_trades: bool,
    /// Whether to subscribe to order book.
    pub subscribe_book: bool,
    /// Book type for order book subscriptions.
    pub book_type: BookType,
    /// Order book depth for subscriptions.
    pub book_depth: Option<NonZeroUsize>,
    /// Order book interval in milliseconds.
    pub book_interval_ms: NonZeroUsize,
    /// Number of order book levels to print when logging.
    pub book_levels_to_print: usize,
    /// Quantity to open position on start (positive for buy, negative for sell).
    pub open_position_on_start_qty: Option<Decimal>,
    /// Time in force for opening position order.
    pub open_position_time_in_force: TimeInForce,
    /// Enable limit buy orders.
    pub enable_limit_buys: bool,
    /// Enable limit sell orders.
    pub enable_limit_sells: bool,
    /// Offset from TOB in price ticks for limit orders.
    pub tob_offset_ticks: u64,
    /// Enable stop buy orders.
    pub enable_stop_buys: bool,
    /// Enable stop sell orders.
    pub enable_stop_sells: bool,
    /// Type of stop order (STOP_MARKET, STOP_LIMIT, MARKET_IF_TOUCHED, LIMIT_IF_TOUCHED).
    pub stop_order_type: OrderType,
    /// Offset from market in price ticks for stop trigger.
    pub stop_offset_ticks: u64,
    /// Offset from trigger price in ticks for stop limit price.
    pub stop_limit_offset_ticks: Option<u64>,
    /// Trigger type for stop orders.
    pub stop_trigger_type: TriggerType,
    /// Modify limit orders to maintain TOB offset.
    pub modify_orders_to_maintain_tob_offset: bool,
    /// Modify stop orders to maintain offset.
    pub modify_stop_orders_to_maintain_offset: bool,
    /// Cancel and replace limit orders to maintain TOB offset.
    pub cancel_replace_orders_to_maintain_tob_offset: bool,
    /// Cancel and replace stop orders to maintain offset.
    pub cancel_replace_stop_orders_to_maintain_offset: bool,
    /// Use post-only for limit orders.
    pub use_post_only: bool,
    /// Cancel all orders on stop.
    pub cancel_orders_on_stop: bool,
    /// Close all positions on stop.
    pub close_positions_on_stop: bool,
    /// Use reduce_only when closing positions.
    pub reduce_only_on_stop: bool,
    /// Use individual cancel commands instead of cancel_all.
    pub use_individual_cancels_on_stop: bool,
    /// Dry run mode (no order submission).
    pub dry_run: bool,
    /// Log received data.
    pub log_data: bool,
    /// Whether unsubscribe is supported on stop.
    pub can_unsubscribe: bool,
}

impl ExecTesterConfig {
    /// Creates a new [`ExecTesterConfig`] with minimal settings.
    ///
    /// # Panics
    ///
    /// Panics if `NonZeroUsize::new(1000)` fails (which should never happen).
    #[must_use]
    pub fn new(
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_id: ClientId,
        order_qty: Quantity,
    ) -> Self {
        Self {
            base: StrategyConfig {
                strategy_id: Some(strategy_id),
                order_id_tag: None,
                ..Default::default()
            },
            instrument_id,
            client_id: Some(client_id),
            order_qty,
            order_display_qty: None,
            order_expire_time_delta_mins: None,
            subscribe_quotes: true,
            subscribe_trades: true,
            subscribe_book: false,
            book_type: BookType::L2_MBP,
            book_depth: None,
            book_interval_ms: NonZeroUsize::new(1000).unwrap(),
            book_levels_to_print: 10,
            open_position_on_start_qty: None,
            open_position_time_in_force: TimeInForce::Gtc,
            enable_limit_buys: true,
            enable_limit_sells: true,
            tob_offset_ticks: 500,
            enable_stop_buys: false,
            enable_stop_sells: false,
            stop_order_type: OrderType::StopMarket,
            stop_offset_ticks: 100,
            stop_limit_offset_ticks: None,
            stop_trigger_type: TriggerType::Default,
            modify_orders_to_maintain_tob_offset: false,
            modify_stop_orders_to_maintain_offset: false,
            cancel_replace_orders_to_maintain_tob_offset: false,
            cancel_replace_stop_orders_to_maintain_offset: false,
            use_post_only: false,
            cancel_orders_on_stop: true,
            close_positions_on_stop: true,
            reduce_only_on_stop: true,
            use_individual_cancels_on_stop: false,
            dry_run: false,
            log_data: true,
            can_unsubscribe: true,
        }
    }
}

impl Default for ExecTesterConfig {
    fn default() -> Self {
        Self {
            base: StrategyConfig::default(),
            instrument_id: InstrumentId::from("BTCUSDT-PERP.BINANCE"),
            client_id: None,
            order_qty: Quantity::from("0.001"),
            order_display_qty: None,
            order_expire_time_delta_mins: None,
            subscribe_quotes: true,
            subscribe_trades: true,
            subscribe_book: false,
            book_type: BookType::L2_MBP,
            book_depth: None,
            book_interval_ms: NonZeroUsize::new(1000).unwrap(),
            book_levels_to_print: 10,
            open_position_on_start_qty: None,
            open_position_time_in_force: TimeInForce::Gtc,
            enable_limit_buys: false,
            enable_limit_sells: false,
            tob_offset_ticks: 500,
            enable_stop_buys: false,
            enable_stop_sells: false,
            stop_order_type: OrderType::StopMarket,
            stop_offset_ticks: 100,
            stop_limit_offset_ticks: None,
            stop_trigger_type: TriggerType::Default,
            modify_orders_to_maintain_tob_offset: false,
            modify_stop_orders_to_maintain_offset: false,
            cancel_replace_orders_to_maintain_tob_offset: false,
            cancel_replace_stop_orders_to_maintain_offset: false,
            use_post_only: false,
            cancel_orders_on_stop: true,
            close_positions_on_stop: true,
            reduce_only_on_stop: true,
            use_individual_cancels_on_stop: false,
            dry_run: false,
            log_data: true,
            can_unsubscribe: true,
        }
    }
}

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
    core: StrategyCore,
    config: ExecTesterConfig,
    instrument: Option<InstrumentAny>,
    price_offset: Option<f64>,

    // Order tracking
    buy_order: Option<OrderAny>,
    sell_order: Option<OrderAny>,
    buy_stop_order: Option<OrderAny>,
    sell_stop_order: Option<OrderAny>,
}

impl Deref for ExecTester {
    type Target = DataActorCore;

    fn deref(&self) -> &Self::Target {
        &self.core.actor
    }
}

impl DerefMut for ExecTester {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.core.actor
    }
}

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
            self.initialize_with_instrument(inst)?;
        } else {
            log::info!("Instrument {instrument_id} not in cache, subscribing...");
            self.subscribe_instrument(instrument_id, client_id, None);
        }

        Ok(())
    }

    fn on_instrument(&mut self, instrument: &InstrumentAny) -> anyhow::Result<()> {
        if instrument.id() == self.config.instrument_id && self.instrument.is_none() {
            let id = instrument.id();
            log::info!("Received instrument {id}, initializing...");
            self.initialize_with_instrument(instrument.clone())?;
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
            if self.config.use_individual_cancels_on_stop {
                let strategy_id = StrategyId::from(self.core.actor.actor_id.inner().as_str());
                let cache = self.cache();
                let open_orders: Vec<OrderAny> = cache
                    .orders_open(None, Some(&instrument_id), Some(&strategy_id), None)
                    .iter()
                    .map(|o| (*o).clone())
                    .collect();
                drop(cache);

                for order in open_orders {
                    let _ = self.cancel_order(order, client_id);
                }
            } else {
                let _ = self.cancel_all_orders(instrument_id, None, client_id);
            }
        }

        if self.config.close_positions_on_stop {
            let _ = self.close_all_positions(
                instrument_id,
                None,
                client_id,
                None,
                Some(TimeInForce::Gtc),
                Some(self.config.reduce_only_on_stop),
                None,
            );
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
            log_info!("Received {quote:?}", color = LogColor::Cyan);
        }

        self.maintain_orders(quote.bid_price, quote.ask_price);
        Ok(())
    }

    fn on_trade(&mut self, trade: &TradeTick) -> anyhow::Result<()> {
        if self.config.log_data {
            log_info!("Received {trade:?}", color = LogColor::Cyan);
        }
        Ok(())
    }

    fn on_book(&mut self, book: &OrderBook) -> anyhow::Result<()> {
        if self.config.log_data {
            let num_levels = self.config.book_levels_to_print;
            let instrument_id = book.instrument_id;
            let book_str = book.pprint(num_levels, None);
            log_info!("\n{instrument_id}\n{book_str}", color = LogColor::Cyan);
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

    fn on_time_event(&mut self, event: &TimeEvent) -> anyhow::Result<()> {
        Strategy::on_time_event(self, event)
    }
}

impl Strategy for ExecTester {
    fn core_mut(&mut self) -> &mut StrategyCore {
        &mut self.core
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
            buy_order: None,
            sell_order: None,
            buy_stop_order: None,
            sell_stop_order: None,
        }
    }

    fn initialize_with_instrument(&mut self, instrument: InstrumentAny) -> anyhow::Result<()> {
        let instrument_id = self.config.instrument_id;
        let client_id = self.config.client_id;

        self.price_offset = Some(self.get_price_offset(&instrument));
        self.instrument = Some(instrument);

        if self.config.subscribe_quotes {
            self.subscribe_quotes(instrument_id, client_id, None);
        }

        if self.config.subscribe_trades {
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

    /// Calculate the price offset from TOB based on configuration.
    fn get_price_offset(&self, instrument: &InstrumentAny) -> f64 {
        instrument.price_increment().as_f64() * self.config.tob_offset_ticks as f64
    }

    /// Check if an order is still active.
    fn is_order_active(&self, order: &OrderAny) -> bool {
        matches!(
            order.status(),
            OrderStatus::Initialized
                | OrderStatus::Submitted
                | OrderStatus::Accepted
                | OrderStatus::PartiallyFilled
                | OrderStatus::PendingUpdate
                | OrderStatus::PendingCancel
        )
    }

    /// Get the trigger price from a stop/conditional order.
    fn get_order_trigger_price(&self, order: &OrderAny) -> Option<Price> {
        order.trigger_price()
    }

    /// Maintain orders based on current market prices.
    fn maintain_orders(&mut self, best_bid: Price, best_ask: Price) {
        if self.instrument.is_none() || self.config.dry_run {
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
    fn maintain_buy_orders(&mut self, best_bid: Price, _best_ask: Price) {
        let Some(instrument) = &self.instrument else {
            return;
        };
        let Some(price_offset) = self.price_offset else {
            return;
        };

        let price_value = best_bid.as_f64() - price_offset;
        let price = instrument.make_price(price_value);

        let needs_new_order = match &self.buy_order {
            None => true,
            Some(order) => !self.is_order_active(order),
        };

        if needs_new_order {
            if let Err(e) = self.submit_limit_order(OrderSide::Buy, price) {
                log::error!("Failed to submit buy limit order: {e}");
            }
        } else if let Some(order) = &self.buy_order
            && order.venue_order_id().is_some()
            && order.status() != OrderStatus::PendingUpdate
            && order.status() != OrderStatus::PendingCancel
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
    fn maintain_sell_orders(&mut self, _best_bid: Price, best_ask: Price) {
        let Some(instrument) = &self.instrument else {
            return;
        };
        let Some(price_offset) = self.price_offset else {
            return;
        };

        let price_value = best_ask.as_f64() + price_offset;
        let price = instrument.make_price(price_value);

        let needs_new_order = match &self.sell_order {
            None => true,
            Some(order) => !self.is_order_active(order),
        };

        if needs_new_order {
            if let Err(e) = self.submit_limit_order(OrderSide::Sell, price) {
                log::error!("Failed to submit sell limit order: {e}");
            }
        } else if let Some(order) = &self.sell_order
            && order.venue_order_id().is_some()
            && order.status() != OrderStatus::PendingUpdate
            && order.status() != OrderStatus::PendingCancel
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
            OrderType::LimitIfTouched | OrderType::MarketIfTouched
        ) {
            // IF_TOUCHED buy: place BELOW market (buy on dip)
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
            && order.status() != OrderStatus::PendingUpdate
            && order.status() != OrderStatus::PendingCancel
        {
            let current_trigger = self.get_order_trigger_price(order);
            if current_trigger.is_some() && current_trigger != Some(trigger_price) {
                if self.config.modify_stop_orders_to_maintain_offset {
                    log_warn!("Stop order modification not yet implemented");
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
            OrderType::LimitIfTouched | OrderType::MarketIfTouched
        ) {
            // IF_TOUCHED sell: place ABOVE market (sell on rally)
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
            && order.status() != OrderStatus::PendingUpdate
            && order.status() != OrderStatus::PendingCancel
        {
            let current_trigger = self.get_order_trigger_price(order);
            if current_trigger.is_some() && current_trigger != Some(trigger_price) {
                if self.config.modify_stop_orders_to_maintain_offset {
                    log_warn!("Stop order modification not yet implemented");
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
    fn submit_limit_order(&mut self, order_side: OrderSide, price: Price) -> anyhow::Result<()> {
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

        let time_in_force = if self.config.order_expire_time_delta_mins.is_some() {
            TimeInForce::Gtd
        } else {
            TimeInForce::Gtc
        };

        // TODO: Calculate expire_time from order_expire_time_delta_mins
        let quantity = instrument.make_qty(self.config.order_qty.as_f64(), None);

        let Some(factory) = &mut self.core.order_factory else {
            anyhow::bail!("Strategy not registered: OrderFactory missing");
        };

        let order = factory.limit(
            self.config.instrument_id,
            order_side,
            quantity,
            price,
            Some(time_in_force),
            None, // expire_time
            Some(self.config.use_post_only),
            None, // reduce_only
            None, // quote_quantity
            self.config.order_display_qty,
            None, // emulation_trigger
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

        self.submit_order(order, None, self.config.client_id)
    }

    /// Submit a stop order.
    ///
    /// # Errors
    ///
    /// Returns an error if order creation or submission fails.
    fn submit_stop_order(
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

        let time_in_force = if self.config.order_expire_time_delta_mins.is_some() {
            TimeInForce::Gtd
        } else {
            TimeInForce::Gtc
        };

        // Use instrument's make_qty to ensure correct precision
        let quantity = instrument.make_qty(self.config.order_qty.as_f64(), None);

        let Some(factory) = &mut self.core.order_factory else {
            anyhow::bail!("Strategy not registered: OrderFactory missing");
        };

        let order: OrderAny = match self.config.stop_order_type {
            OrderType::StopMarket => factory.stop_market(
                self.config.instrument_id,
                order_side,
                quantity,
                trigger_price,
                Some(self.config.stop_trigger_type),
                Some(time_in_force),
                None, // expire_time
                None, // reduce_only
                None, // quote_quantity
                None, // display_qty
                None, // emulation_trigger
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
                    None, // expire_time
                    None, // post_only
                    None, // reduce_only
                    None, // quote_quantity
                    self.config.order_display_qty,
                    None, // emulation_trigger
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
                None, // expire_time
                None, // reduce_only
                None, // quote_quantity
                None, // emulation_trigger
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
                    None, // expire_time
                    None, // post_only
                    None, // reduce_only
                    None, // quote_quantity
                    self.config.order_display_qty,
                    None, // emulation_trigger
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

        if order_side == OrderSide::Buy {
            self.buy_stop_order = Some(order.clone());
        } else {
            self.sell_stop_order = Some(order.clone());
        }

        self.submit_order(order, None, self.config.client_id)
    }

    /// Open a position with a market order.
    ///
    /// # Errors
    ///
    /// Returns an error if order creation or submission fails.
    fn open_position(&mut self, net_qty: Decimal) -> anyhow::Result<()> {
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

        let Some(factory) = &mut self.core.order_factory else {
            anyhow::bail!("Strategy not registered: OrderFactory missing");
        };

        let order = factory.market(
            self.config.instrument_id,
            order_side,
            quantity,
            Some(self.config.open_position_time_in_force),
            None, // reduce_only
            None, // quote_quantity
            None, // exec_algorithm_id
            None, // exec_algorithm_params
            None, // tags
            None, // client_order_id
        );

        self.submit_order(order, None, self.config.client_id)
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use nautilus_core::UnixNanos;
    use nautilus_model::{
        identifiers::{StrategyId, TradeId},
        instruments::stubs::crypto_perpetual_ethusdt,
        orders::LimitOrder,
    };
    use rstest::*;

    use super::*;

    // =========================================================================
    // Fixtures
    // =========================================================================

    #[fixture]
    fn config() -> ExecTesterConfig {
        ExecTesterConfig::new(
            StrategyId::from("EXEC_TESTER-001"),
            InstrumentId::from("ETHUSDT-PERP.BINANCE"),
            ClientId::new("BINANCE"),
            Quantity::from("0.001"),
        )
    }

    #[fixture]
    fn instrument() -> InstrumentAny {
        InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt())
    }

    fn create_initialized_limit_order() -> OrderAny {
        OrderAny::Limit(LimitOrder::default())
    }

    // =========================================================================
    // Config Tests
    // =========================================================================

    #[rstest]
    fn test_config_creation(config: ExecTesterConfig) {
        assert_eq!(
            config.base.strategy_id,
            Some(StrategyId::from("EXEC_TESTER-001"))
        );
        assert_eq!(
            config.instrument_id,
            InstrumentId::from("ETHUSDT-PERP.BINANCE")
        );
        assert_eq!(config.client_id, Some(ClientId::new("BINANCE")));
        assert_eq!(config.order_qty, Quantity::from("0.001"));
        assert!(config.subscribe_quotes);
        assert!(config.subscribe_trades);
        assert!(!config.subscribe_book);
        assert!(config.enable_limit_buys);
        assert!(config.enable_limit_sells);
        assert!(!config.enable_stop_buys);
        assert!(!config.enable_stop_sells);
        assert_eq!(config.tob_offset_ticks, 500);
    }

    #[rstest]
    fn test_config_default() {
        let config = ExecTesterConfig::default();

        assert!(config.base.strategy_id.is_none());
        assert!(config.subscribe_quotes);
        assert!(config.subscribe_trades);
        assert!(!config.enable_limit_buys);
        assert!(!config.enable_limit_sells);
        assert!(config.cancel_orders_on_stop);
        assert!(config.close_positions_on_stop);
    }

    #[rstest]
    fn test_config_with_stop_orders(mut config: ExecTesterConfig) {
        config.enable_stop_buys = true;
        config.enable_stop_sells = true;
        config.stop_order_type = OrderType::StopLimit;
        config.stop_offset_ticks = 200;
        config.stop_limit_offset_ticks = Some(50);

        let tester = ExecTester::new(config);

        assert!(tester.config.enable_stop_buys);
        assert!(tester.config.enable_stop_sells);
        assert_eq!(tester.config.stop_order_type, OrderType::StopLimit);
        assert_eq!(tester.config.stop_offset_ticks, 200);
        assert_eq!(tester.config.stop_limit_offset_ticks, Some(50));
    }

    #[rstest]
    fn test_config_with_order_maintenance(mut config: ExecTesterConfig) {
        config.modify_orders_to_maintain_tob_offset = true;
        config.cancel_replace_orders_to_maintain_tob_offset = false;

        let tester = ExecTester::new(config);

        assert!(tester.config.modify_orders_to_maintain_tob_offset);
        assert!(!tester.config.cancel_replace_orders_to_maintain_tob_offset);
    }

    #[rstest]
    fn test_config_with_dry_run(mut config: ExecTesterConfig) {
        config.dry_run = true;

        let tester = ExecTester::new(config);

        assert!(tester.config.dry_run);
    }

    #[rstest]
    fn test_config_with_position_opening(mut config: ExecTesterConfig) {
        config.open_position_on_start_qty = Some(Decimal::from(1));
        config.open_position_time_in_force = TimeInForce::Ioc;

        let tester = ExecTester::new(config);

        assert_eq!(
            tester.config.open_position_on_start_qty,
            Some(Decimal::from(1))
        );
        assert_eq!(tester.config.open_position_time_in_force, TimeInForce::Ioc);
    }

    #[rstest]
    fn test_config_with_all_stop_order_types(mut config: ExecTesterConfig) {
        // Test STOP_MARKET
        config.stop_order_type = OrderType::StopMarket;
        assert_eq!(config.stop_order_type, OrderType::StopMarket);

        // Test STOP_LIMIT
        config.stop_order_type = OrderType::StopLimit;
        assert_eq!(config.stop_order_type, OrderType::StopLimit);

        // Test MARKET_IF_TOUCHED
        config.stop_order_type = OrderType::MarketIfTouched;
        assert_eq!(config.stop_order_type, OrderType::MarketIfTouched);

        // Test LIMIT_IF_TOUCHED
        config.stop_order_type = OrderType::LimitIfTouched;
        assert_eq!(config.stop_order_type, OrderType::LimitIfTouched);
    }

    // =========================================================================
    // ExecTester Creation Tests
    // =========================================================================

    #[rstest]
    fn test_exec_tester_creation(config: ExecTesterConfig) {
        let tester = ExecTester::new(config);

        assert!(tester.instrument.is_none());
        assert!(tester.price_offset.is_none());
        assert!(tester.buy_order.is_none());
        assert!(tester.sell_order.is_none());
        assert!(tester.buy_stop_order.is_none());
        assert!(tester.sell_stop_order.is_none());
    }

    // =========================================================================
    // Price Offset Calculation Tests
    // =========================================================================

    #[rstest]
    fn test_get_price_offset(config: ExecTesterConfig, instrument: InstrumentAny) {
        let tester = ExecTester::new(config);

        // price_increment = 0.01, tob_offset_ticks = 500
        // Expected: 0.01 * 500 = 5.0
        let offset = tester.get_price_offset(&instrument);

        assert!((offset - 5.0).abs() < 1e-10);
    }

    #[rstest]
    fn test_get_price_offset_different_ticks(instrument: InstrumentAny) {
        let config = ExecTesterConfig {
            tob_offset_ticks: 100,
            ..Default::default()
        };

        let tester = ExecTester::new(config);

        // price_increment = 0.01, tob_offset_ticks = 100
        let offset = tester.get_price_offset(&instrument);

        assert!((offset - 1.0).abs() < 1e-10);
    }

    #[rstest]
    fn test_get_price_offset_single_tick(instrument: InstrumentAny) {
        let config = ExecTesterConfig {
            tob_offset_ticks: 1,
            ..Default::default()
        };

        let tester = ExecTester::new(config);

        // price_increment = 0.01, tob_offset_ticks = 1
        let offset = tester.get_price_offset(&instrument);

        assert!((offset - 0.01).abs() < 1e-10);
    }

    // =========================================================================
    // Order Activity Status Tests
    // =========================================================================

    #[rstest]
    fn test_is_order_active_initialized(config: ExecTesterConfig) {
        let tester = ExecTester::new(config);
        let order = create_initialized_limit_order();

        assert!(tester.is_order_active(&order));
        assert_eq!(order.status(), OrderStatus::Initialized);
    }

    // =========================================================================
    // Trigger Price Extraction Tests
    // =========================================================================

    #[rstest]
    fn test_get_order_trigger_price_limit_order_returns_none(config: ExecTesterConfig) {
        let tester = ExecTester::new(config);
        let order = create_initialized_limit_order();

        assert!(tester.get_order_trigger_price(&order).is_none());
    }

    // =========================================================================
    // Data Handler Tests
    // =========================================================================

    #[rstest]
    fn test_on_quote_with_logging(config: ExecTesterConfig) {
        let mut tester = ExecTester::new(config);

        let quote = QuoteTick::new(
            InstrumentId::from("BTCUSDT-PERP.BINANCE"),
            Price::from("50000.0"),
            Price::from("50001.0"),
            Quantity::from("1.0"),
            Quantity::from("1.0"),
            UnixNanos::default(),
            UnixNanos::default(),
        );

        let result = tester.on_quote(&quote);
        assert!(result.is_ok());
    }

    #[rstest]
    fn test_on_quote_without_logging(mut config: ExecTesterConfig) {
        config.log_data = false;
        let mut tester = ExecTester::new(config);

        let quote = QuoteTick::new(
            InstrumentId::from("BTCUSDT-PERP.BINANCE"),
            Price::from("50000.0"),
            Price::from("50001.0"),
            Quantity::from("1.0"),
            Quantity::from("1.0"),
            UnixNanos::default(),
            UnixNanos::default(),
        );

        let result = tester.on_quote(&quote);
        assert!(result.is_ok());
    }

    #[rstest]
    fn test_on_trade_with_logging(config: ExecTesterConfig) {
        let mut tester = ExecTester::new(config);

        let trade = TradeTick::new(
            InstrumentId::from("BTCUSDT-PERP.BINANCE"),
            Price::from("50000.0"),
            Quantity::from("0.1"),
            nautilus_model::enums::AggressorSide::Buyer,
            TradeId::new("12345"),
            UnixNanos::default(),
            UnixNanos::default(),
        );

        let result = tester.on_trade(&trade);
        assert!(result.is_ok());
    }

    #[rstest]
    fn test_on_trade_without_logging(mut config: ExecTesterConfig) {
        config.log_data = false;
        let mut tester = ExecTester::new(config);

        let trade = TradeTick::new(
            InstrumentId::from("BTCUSDT-PERP.BINANCE"),
            Price::from("50000.0"),
            Quantity::from("0.1"),
            nautilus_model::enums::AggressorSide::Buyer,
            TradeId::new("12345"),
            UnixNanos::default(),
            UnixNanos::default(),
        );

        let result = tester.on_trade(&trade);
        assert!(result.is_ok());
    }

    #[rstest]
    fn test_on_book_without_bids_or_asks(config: ExecTesterConfig) {
        let mut tester = ExecTester::new(config);

        let book = OrderBook::new(InstrumentId::from("BTCUSDT-PERP.BINANCE"), BookType::L2_MBP);

        let result = tester.on_book(&book);
        assert!(result.is_ok());
    }

    // =========================================================================
    // Maintain Orders - Dry Run Tests
    // =========================================================================

    #[rstest]
    fn test_maintain_orders_dry_run_does_nothing(mut config: ExecTesterConfig) {
        config.dry_run = true;
        config.enable_limit_buys = true;
        config.enable_limit_sells = true;
        let mut tester = ExecTester::new(config);

        let best_bid = Price::from("50000.0");
        let best_ask = Price::from("50001.0");

        tester.maintain_orders(best_bid, best_ask);

        assert!(tester.buy_order.is_none());
        assert!(tester.sell_order.is_none());
    }

    #[rstest]
    fn test_maintain_orders_no_instrument_does_nothing(config: ExecTesterConfig) {
        let mut tester = ExecTester::new(config);

        let best_bid = Price::from("50000.0");
        let best_ask = Price::from("50001.0");

        tester.maintain_orders(best_bid, best_ask);

        assert!(tester.buy_order.is_none());
        assert!(tester.sell_order.is_none());
    }

    // =========================================================================
    // Submit Order Error Handling Tests
    // =========================================================================

    #[rstest]
    fn test_submit_limit_order_no_instrument_returns_error(config: ExecTesterConfig) {
        let mut tester = ExecTester::new(config);

        let result = tester.submit_limit_order(OrderSide::Buy, Price::from("50000.0"));

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("No instrument"));
    }

    #[rstest]
    fn test_submit_limit_order_dry_run_returns_ok(
        mut config: ExecTesterConfig,
        instrument: InstrumentAny,
    ) {
        config.dry_run = true;
        let mut tester = ExecTester::new(config);
        tester.instrument = Some(instrument);

        let result = tester.submit_limit_order(OrderSide::Buy, Price::from("50000.0"));

        assert!(result.is_ok());
        assert!(tester.buy_order.is_none());
    }

    #[rstest]
    fn test_submit_limit_order_buys_disabled_returns_ok(
        mut config: ExecTesterConfig,
        instrument: InstrumentAny,
    ) {
        config.enable_limit_buys = false;
        let mut tester = ExecTester::new(config);
        tester.instrument = Some(instrument);

        let result = tester.submit_limit_order(OrderSide::Buy, Price::from("50000.0"));

        assert!(result.is_ok());
        assert!(tester.buy_order.is_none());
    }

    #[rstest]
    fn test_submit_limit_order_sells_disabled_returns_ok(
        mut config: ExecTesterConfig,
        instrument: InstrumentAny,
    ) {
        config.enable_limit_sells = false;
        let mut tester = ExecTester::new(config);
        tester.instrument = Some(instrument);

        let result = tester.submit_limit_order(OrderSide::Sell, Price::from("50000.0"));

        assert!(result.is_ok());
        assert!(tester.sell_order.is_none());
    }

    #[rstest]
    fn test_submit_stop_order_no_instrument_returns_error(config: ExecTesterConfig) {
        let mut tester = ExecTester::new(config);

        let result = tester.submit_stop_order(OrderSide::Buy, Price::from("51000.0"), None);

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("No instrument"));
    }

    #[rstest]
    fn test_submit_stop_order_dry_run_returns_ok(
        mut config: ExecTesterConfig,
        instrument: InstrumentAny,
    ) {
        config.dry_run = true;
        config.enable_stop_buys = true;
        let mut tester = ExecTester::new(config);
        tester.instrument = Some(instrument);

        let result = tester.submit_stop_order(OrderSide::Buy, Price::from("51000.0"), None);

        assert!(result.is_ok());
        assert!(tester.buy_stop_order.is_none());
    }

    #[rstest]
    fn test_submit_stop_order_buys_disabled_returns_ok(
        mut config: ExecTesterConfig,
        instrument: InstrumentAny,
    ) {
        config.enable_stop_buys = false;
        let mut tester = ExecTester::new(config);
        tester.instrument = Some(instrument);

        let result = tester.submit_stop_order(OrderSide::Buy, Price::from("51000.0"), None);

        assert!(result.is_ok());
        assert!(tester.buy_stop_order.is_none());
    }

    #[rstest]
    fn test_submit_stop_limit_without_limit_price_returns_error(
        mut config: ExecTesterConfig,
        instrument: InstrumentAny,
    ) {
        config.enable_stop_buys = true;
        config.stop_order_type = OrderType::StopLimit;
        let mut tester = ExecTester::new(config);
        tester.instrument = Some(instrument);

        // Cannot actually submit without a registered OrderFactory
    }

    // =========================================================================
    // Open Position Tests
    // =========================================================================

    #[rstest]
    fn test_open_position_no_instrument_returns_error(config: ExecTesterConfig) {
        let mut tester = ExecTester::new(config);

        let result = tester.open_position(Decimal::from(1));

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("No instrument"));
    }

    #[rstest]
    fn test_open_position_zero_quantity_returns_ok(
        config: ExecTesterConfig,
        instrument: InstrumentAny,
    ) {
        let mut tester = ExecTester::new(config);
        tester.instrument = Some(instrument);

        let result = tester.open_position(Decimal::ZERO);

        assert!(result.is_ok());
    }
}
