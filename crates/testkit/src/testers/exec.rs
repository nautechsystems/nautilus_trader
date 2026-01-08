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

//! Execution tester strategy for live testing order execution.

use std::{
    num::NonZeroUsize,
    ops::{Deref, DerefMut},
};

use indexmap::IndexMap;
use nautilus_common::{
    actor::{DataActor, DataActorCore},
    enums::LogColor,
    log_info, log_warn,
    timer::TimeEvent,
};
use nautilus_core::UnixNanos;
use nautilus_model::{
    data::{Bar, IndexPriceUpdate, MarkPriceUpdate, OrderBookDeltas, QuoteTick, TradeTick},
    enums::{BookType, OrderSide, OrderType, TimeInForce, TriggerType},
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
    /// Order quantity.
    pub order_qty: Quantity,
    /// Display quantity for iceberg orders (None for full display, Some(0) for hidden).
    pub order_display_qty: Option<Quantity>,
    /// Minutes until GTD orders expire (None for GTC).
    pub order_expire_time_delta_mins: Option<u64>,
    /// Adapter-specific order parameters.
    pub order_params: Option<IndexMap<String, String>>,
    /// Client ID to use for orders and subscriptions.
    pub client_id: Option<ClientId>,
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
    /// Enable stop buy orders.
    pub enable_stop_buys: bool,
    /// Enable stop sell orders.
    pub enable_stop_sells: bool,
    /// Offset from TOB in price ticks for limit orders.
    pub tob_offset_ticks: u64,
    /// Type of stop order (STOP_MARKET, STOP_LIMIT, MARKET_IF_TOUCHED, LIMIT_IF_TOUCHED).
    pub stop_order_type: OrderType,
    /// Offset from market in price ticks for stop trigger.
    pub stop_offset_ticks: u64,
    /// Offset from trigger price in ticks for stop limit price.
    pub stop_limit_offset_ticks: Option<u64>,
    /// Trigger type for stop orders.
    pub stop_trigger_type: TriggerType,
    /// Enable bracket orders (entry with TP/SL).
    pub enable_brackets: bool,
    /// Entry order type for bracket orders.
    pub bracket_entry_order_type: OrderType,
    /// Offset in ticks for bracket TP/SL from entry price.
    pub bracket_offset_ticks: u64,
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
    /// Use quote quantity for orders.
    pub use_quote_quantity: bool,
    /// Emulation trigger type for orders.
    pub emulation_trigger: Option<TriggerType>,
    /// Cancel all orders on stop.
    pub cancel_orders_on_stop: bool,
    /// Close all positions on stop.
    pub close_positions_on_stop: bool,
    /// Time in force for closing positions (None defaults to GTC).
    pub close_positions_time_in_force: Option<TimeInForce>,
    /// Use reduce_only when closing positions.
    pub reduce_only_on_stop: bool,
    /// Use individual cancel commands instead of cancel_all.
    pub use_individual_cancels_on_stop: bool,
    /// Use batch cancel command when stopping.
    pub use_batch_cancel_on_stop: bool,
    /// Dry run mode (no order submission).
    pub dry_run: bool,
    /// Log received data.
    pub log_data: bool,
    /// Test post-only rejection by placing orders on wrong side of spread.
    pub test_reject_post_only: bool,
    /// Test reduce-only rejection by setting reduce_only on open position order.
    pub test_reject_reduce_only: bool,
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
            order_qty,
            order_display_qty: None,
            order_expire_time_delta_mins: None,
            order_params: None,
            client_id: Some(client_id),
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
            enable_stop_buys: false,
            enable_stop_sells: false,
            tob_offset_ticks: 500,
            stop_order_type: OrderType::StopMarket,
            stop_offset_ticks: 100,
            stop_limit_offset_ticks: None,
            stop_trigger_type: TriggerType::Default,
            enable_brackets: false,
            bracket_entry_order_type: OrderType::Limit,
            bracket_offset_ticks: 500,
            modify_orders_to_maintain_tob_offset: false,
            modify_stop_orders_to_maintain_offset: false,
            cancel_replace_orders_to_maintain_tob_offset: false,
            cancel_replace_stop_orders_to_maintain_offset: false,
            use_post_only: false,
            use_quote_quantity: false,
            emulation_trigger: None,
            cancel_orders_on_stop: true,
            close_positions_on_stop: true,
            close_positions_time_in_force: None,
            reduce_only_on_stop: true,
            use_individual_cancels_on_stop: false,
            use_batch_cancel_on_stop: false,
            dry_run: false,
            log_data: true,
            test_reject_post_only: false,
            test_reject_reduce_only: false,
            can_unsubscribe: true,
        }
    }

    #[must_use]
    pub fn with_log_data(mut self, log_data: bool) -> Self {
        self.log_data = log_data;
        self
    }

    #[must_use]
    pub fn with_dry_run(mut self, dry_run: bool) -> Self {
        self.dry_run = dry_run;
        self
    }

    #[must_use]
    pub fn with_subscribe_quotes(mut self, subscribe: bool) -> Self {
        self.subscribe_quotes = subscribe;
        self
    }

    #[must_use]
    pub fn with_subscribe_trades(mut self, subscribe: bool) -> Self {
        self.subscribe_trades = subscribe;
        self
    }

    #[must_use]
    pub fn with_subscribe_book(mut self, subscribe: bool) -> Self {
        self.subscribe_book = subscribe;
        self
    }

    #[must_use]
    pub fn with_book_type(mut self, book_type: BookType) -> Self {
        self.book_type = book_type;
        self
    }

    #[must_use]
    pub fn with_book_depth(mut self, depth: Option<NonZeroUsize>) -> Self {
        self.book_depth = depth;
        self
    }

    #[must_use]
    pub fn with_enable_limit_buys(mut self, enable: bool) -> Self {
        self.enable_limit_buys = enable;
        self
    }

    #[must_use]
    pub fn with_enable_limit_sells(mut self, enable: bool) -> Self {
        self.enable_limit_sells = enable;
        self
    }

    #[must_use]
    pub fn with_enable_stop_buys(mut self, enable: bool) -> Self {
        self.enable_stop_buys = enable;
        self
    }

    #[must_use]
    pub fn with_enable_stop_sells(mut self, enable: bool) -> Self {
        self.enable_stop_sells = enable;
        self
    }

    #[must_use]
    pub fn with_tob_offset_ticks(mut self, ticks: u64) -> Self {
        self.tob_offset_ticks = ticks;
        self
    }

    #[must_use]
    pub fn with_stop_order_type(mut self, order_type: OrderType) -> Self {
        self.stop_order_type = order_type;
        self
    }

    #[must_use]
    pub fn with_stop_offset_ticks(mut self, ticks: u64) -> Self {
        self.stop_offset_ticks = ticks;
        self
    }

    #[must_use]
    pub fn with_use_post_only(mut self, use_post_only: bool) -> Self {
        self.use_post_only = use_post_only;
        self
    }

    #[must_use]
    pub fn with_open_position_on_start(mut self, qty: Option<Decimal>) -> Self {
        self.open_position_on_start_qty = qty;
        self
    }

    #[must_use]
    pub fn with_cancel_orders_on_stop(mut self, cancel: bool) -> Self {
        self.cancel_orders_on_stop = cancel;
        self
    }

    #[must_use]
    pub fn with_close_positions_on_stop(mut self, close: bool) -> Self {
        self.close_positions_on_stop = close;
        self
    }

    #[must_use]
    pub fn with_close_positions_time_in_force(
        mut self,
        time_in_force: Option<TimeInForce>,
    ) -> Self {
        self.close_positions_time_in_force = time_in_force;
        self
    }

    #[must_use]
    pub fn with_use_batch_cancel_on_stop(mut self, use_batch: bool) -> Self {
        self.use_batch_cancel_on_stop = use_batch;
        self
    }

    #[must_use]
    pub fn with_can_unsubscribe(mut self, can_unsubscribe: bool) -> Self {
        self.can_unsubscribe = can_unsubscribe;
        self
    }

    #[must_use]
    pub fn with_enable_brackets(mut self, enable: bool) -> Self {
        self.enable_brackets = enable;
        self
    }

    #[must_use]
    pub fn with_bracket_entry_order_type(mut self, order_type: OrderType) -> Self {
        self.bracket_entry_order_type = order_type;
        self
    }

    #[must_use]
    pub fn with_bracket_offset_ticks(mut self, ticks: u64) -> Self {
        self.bracket_offset_ticks = ticks;
        self
    }

    #[must_use]
    pub fn with_test_reject_post_only(mut self, test: bool) -> Self {
        self.test_reject_post_only = test;
        self
    }

    #[must_use]
    pub fn with_test_reject_reduce_only(mut self, test: bool) -> Self {
        self.test_reject_reduce_only = test;
        self
    }

    #[must_use]
    pub fn with_emulation_trigger(mut self, trigger: Option<TriggerType>) -> Self {
        self.emulation_trigger = trigger;
        self
    }

    #[must_use]
    pub fn with_use_quote_quantity(mut self, use_quote: bool) -> Self {
        self.use_quote_quantity = use_quote;
        self
    }

    #[must_use]
    pub fn with_order_params(mut self, params: Option<IndexMap<String, String>>) -> Self {
        self.order_params = params;
        self
    }
}

impl Default for ExecTesterConfig {
    fn default() -> Self {
        Self {
            base: StrategyConfig::default(),
            instrument_id: InstrumentId::from("BTCUSDT-PERP.BINANCE"),
            order_qty: Quantity::from("0.001"),
            order_display_qty: None,
            order_expire_time_delta_mins: None,
            order_params: None,
            client_id: None,
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
            enable_stop_buys: false,
            enable_stop_sells: false,
            tob_offset_ticks: 500,
            stop_order_type: OrderType::StopMarket,
            stop_offset_ticks: 100,
            stop_limit_offset_ticks: None,
            stop_trigger_type: TriggerType::Default,
            enable_brackets: false,
            bracket_entry_order_type: OrderType::Limit,
            bracket_offset_ticks: 500,
            modify_orders_to_maintain_tob_offset: false,
            modify_stop_orders_to_maintain_offset: false,
            cancel_replace_orders_to_maintain_tob_offset: false,
            cancel_replace_stop_orders_to_maintain_offset: false,
            use_post_only: false,
            use_quote_quantity: false,
            emulation_trigger: None,
            cancel_orders_on_stop: true,
            close_positions_on_stop: true,
            close_positions_time_in_force: None,
            reduce_only_on_stop: true,
            use_individual_cancels_on_stop: false,
            use_batch_cancel_on_stop: false,
            dry_run: false,
            log_data: true,
            test_reject_post_only: false,
            test_reject_reduce_only: false,
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
            let strategy_id = StrategyId::from(self.core.actor.actor_id.inner().as_str());
            if self.config.use_individual_cancels_on_stop {
                let cache = self.cache();
                let open_orders: Vec<OrderAny> = cache
                    .orders_open(None, Some(&instrument_id), Some(&strategy_id), None)
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
                    .orders_open(None, Some(&instrument_id), Some(&strategy_id), None)
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
            log_info!("Received {deltas:?}", color = LogColor::Cyan);
        }
        Ok(())
    }

    fn on_bar(&mut self, bar: &Bar) -> anyhow::Result<()> {
        if self.config.log_data {
            log_info!("Received {bar:?}", color = LogColor::Cyan);
        }
        Ok(())
    }

    fn on_mark_price(&mut self, mark_price: &MarkPriceUpdate) -> anyhow::Result<()> {
        if self.config.log_data {
            log_info!("Received {mark_price:?}", color = LogColor::Cyan);
        }
        Ok(())
    }

    fn on_index_price(&mut self, index_price: &IndexPriceUpdate) -> anyhow::Result<()> {
        if self.config.log_data {
            log_info!("Received {index_price:?}", color = LogColor::Cyan);
        }
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

    fn external_order_claims(&self) -> Option<Vec<InstrumentId>> {
        self.config.base.external_order_claims.clone()
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
        order.is_active_local() || order.is_inflight() || order.is_open()
    }

    /// Get the trigger price from a stop/conditional order.
    fn get_order_trigger_price(&self, order: &OrderAny) -> Option<Price> {
        order.trigger_price()
    }

    /// Modify a stop order's trigger price and optionally limit price.
    fn modify_stop_order(
        &mut self,
        order: OrderAny,
        trigger_price: Price,
        limit_price: Option<Price>,
    ) -> anyhow::Result<()> {
        let client_id = self.config.client_id;

        match &order {
            OrderAny::StopMarket(_) | OrderAny::MarketIfTouched(_) => {
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
    fn maintain_buy_orders(&mut self, best_bid: Price, best_ask: Price) {
        let Some(instrument) = &self.instrument else {
            return;
        };
        let Some(price_offset) = self.price_offset else {
            return;
        };

        // test_reject_post_only places order on wrong side of spread to trigger rejection
        let price = if self.config.use_post_only && self.config.test_reject_post_only {
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
        let price = if self.config.use_post_only && self.config.test_reject_post_only {
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

        let (time_in_force, expire_time) =
            if let Some(mins) = self.config.order_expire_time_delta_mins {
                let current_ns = self.timestamp_ns();
                let delta_ns = mins * 60 * 1_000_000_000;
                let expire_ns = UnixNanos::from(current_ns.as_u64() + delta_ns);
                (TimeInForce::Gtd, Some(expire_ns))
            } else {
                (TimeInForce::Gtc, None)
            };

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
            expire_time,
            Some(self.config.use_post_only),
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

        let (time_in_force, expire_time) =
            if let Some(mins) = self.config.order_expire_time_delta_mins {
                let current_ns = self.timestamp_ns();
                let delta_ns = mins * 60 * 1_000_000_000;
                let expire_ns = UnixNanos::from(current_ns.as_u64() + delta_ns);
                (TimeInForce::Gtd, Some(expire_ns))
            } else {
                (TimeInForce::Gtc, None)
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
            _ => {
                anyhow::bail!("Unknown stop order type: {:?}", self.config.stop_order_type);
            }
        };

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
    fn submit_bracket_order(
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
                "Only Limit entry orders are supported for brackets, got {:?}",
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
            if let Some(mins) = self.config.order_expire_time_delta_mins {
                let current_ns = self.timestamp_ns();
                let delta_ns = mins * 60 * 1_000_000_000;
                let expire_ns = UnixNanos::from(current_ns.as_u64() + delta_ns);
                (TimeInForce::Gtd, Some(expire_ns))
            } else {
                (TimeInForce::Gtc, None)
            };

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

        let Some(factory) = &mut self.core.order_factory else {
            anyhow::bail!("Strategy not registered: OrderFactory missing");
        };

        let order_list = factory.bracket(
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
            Some(self.config.use_post_only),
            None, // reduce_only
            Some(self.config.use_quote_quantity),
            self.config.emulation_trigger,
            None, // trigger_instrument_id
            None, // exec_algorithm_id
            None, // exec_algorithm_params
            None, // tags
        );

        if let Some(entry_order) = order_list.orders.first() {
            if order_side == OrderSide::Buy {
                self.buy_order = Some(entry_order.clone());
            } else {
                self.sell_order = Some(entry_order.clone());
            }
        }

        let client_id = self.config.client_id;
        if let Some(params) = &self.config.order_params {
            self.submit_order_list_with_params(order_list, None, client_id, params.clone())
        } else {
            self.submit_order_list(order_list, None, client_id)
        }
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

        // Test reduce_only rejection by setting reduce_only on open position order
        let reduce_only = if self.config.test_reject_reduce_only {
            Some(true)
        } else {
            None
        };

        let order = factory.market(
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

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use nautilus_common::{
        cache::Cache,
        clock::{Clock, TestClock},
    };
    use nautilus_core::UnixNanos;
    use nautilus_model::{
        data::stubs::{OrderBookDeltaTestBuilder, stub_bar},
        enums::{AggressorSide, ContingencyType, OrderStatus},
        identifiers::{StrategyId, TradeId, TraderId},
        instruments::stubs::crypto_perpetual_ethusdt,
        orders::LimitOrder,
        stubs::TestDefault,
    };
    use nautilus_portfolio::portfolio::Portfolio;
    use rstest::*;

    use super::*;

    /// Register an ExecTester with all required components.
    /// This gives the tester access to OrderFactory for actual order creation.
    fn register_exec_tester(tester: &mut ExecTester, cache: Rc<RefCell<Cache>>) {
        let trader_id = TraderId::from("TRADER-001");
        let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(TestClock::new()));
        let portfolio = Rc::new(RefCell::new(Portfolio::new(
            cache.clone(),
            clock.clone(),
            None,
        )));

        tester
            .core
            .register(trader_id, clock, cache, portfolio)
            .unwrap();
    }

    /// Create a cache with the test instrument pre-loaded.
    fn create_cache_with_instrument(instrument: &InstrumentAny) -> Rc<RefCell<Cache>> {
        let cache = Rc::new(RefCell::new(Cache::default()));
        let _ = cache.borrow_mut().add_instrument(instrument.clone());
        cache
    }

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
        OrderAny::Limit(LimitOrder::test_default())
    }

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
        assert!(config.enable_limit_buys);
        assert!(config.enable_limit_sells);
        assert!(config.cancel_orders_on_stop);
        assert!(config.close_positions_on_stop);
        assert!(config.close_positions_time_in_force.is_none());
        assert!(!config.use_batch_cancel_on_stop);
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
    fn test_config_with_batch_cancel() {
        let config = ExecTesterConfig::default().with_use_batch_cancel_on_stop(true);
        assert!(config.use_batch_cancel_on_stop);
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
    fn test_config_with_close_positions_time_in_force_builder() {
        let config =
            ExecTesterConfig::default().with_close_positions_time_in_force(Some(TimeInForce::Ioc));

        assert_eq!(config.close_positions_time_in_force, Some(TimeInForce::Ioc));
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

    #[rstest]
    fn test_is_order_active_initialized(config: ExecTesterConfig) {
        let tester = ExecTester::new(config);
        let order = create_initialized_limit_order();

        assert!(tester.is_order_active(&order));
        assert_eq!(order.status(), OrderStatus::Initialized);
    }

    #[rstest]
    fn test_get_order_trigger_price_limit_order_returns_none(config: ExecTesterConfig) {
        let tester = ExecTester::new(config);
        let order = create_initialized_limit_order();

        assert!(tester.get_order_trigger_price(&order).is_none());
    }

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
            AggressorSide::Buyer,
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
            AggressorSide::Buyer,
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

    #[rstest]
    fn test_on_book_deltas_with_logging(config: ExecTesterConfig) {
        let mut tester = ExecTester::new(config);
        let instrument_id = InstrumentId::from("BTCUSDT-PERP.BINANCE");
        let delta = OrderBookDeltaTestBuilder::new(instrument_id).build();
        let deltas = OrderBookDeltas::new(instrument_id, vec![delta]);

        let result = tester.on_book_deltas(&deltas);

        assert!(result.is_ok());
    }

    #[rstest]
    fn test_on_book_deltas_without_logging(mut config: ExecTesterConfig) {
        config.log_data = false;
        let mut tester = ExecTester::new(config);
        let instrument_id = InstrumentId::from("BTCUSDT-PERP.BINANCE");
        let delta = OrderBookDeltaTestBuilder::new(instrument_id).build();
        let deltas = OrderBookDeltas::new(instrument_id, vec![delta]);

        let result = tester.on_book_deltas(&deltas);

        assert!(result.is_ok());
    }

    #[rstest]
    fn test_on_bar_with_logging(config: ExecTesterConfig) {
        let mut tester = ExecTester::new(config);
        let bar = stub_bar();

        let result = tester.on_bar(&bar);

        assert!(result.is_ok());
    }

    #[rstest]
    fn test_on_bar_without_logging(mut config: ExecTesterConfig) {
        config.log_data = false;
        let mut tester = ExecTester::new(config);
        let bar = stub_bar();

        let result = tester.on_bar(&bar);

        assert!(result.is_ok());
    }

    #[rstest]
    fn test_on_mark_price_with_logging(config: ExecTesterConfig) {
        let mut tester = ExecTester::new(config);
        let instrument_id = InstrumentId::from("BTCUSDT-PERP.BINANCE");
        let mark_price = MarkPriceUpdate::new(
            instrument_id,
            Price::from("50000.0"),
            UnixNanos::default(),
            UnixNanos::default(),
        );

        let result = tester.on_mark_price(&mark_price);

        assert!(result.is_ok());
    }

    #[rstest]
    fn test_on_mark_price_without_logging(mut config: ExecTesterConfig) {
        config.log_data = false;
        let mut tester = ExecTester::new(config);
        let instrument_id = InstrumentId::from("BTCUSDT-PERP.BINANCE");
        let mark_price = MarkPriceUpdate::new(
            instrument_id,
            Price::from("50000.0"),
            UnixNanos::default(),
            UnixNanos::default(),
        );

        let result = tester.on_mark_price(&mark_price);

        assert!(result.is_ok());
    }

    #[rstest]
    fn test_on_index_price_with_logging(config: ExecTesterConfig) {
        let mut tester = ExecTester::new(config);
        let instrument_id = InstrumentId::from("BTCUSDT-PERP.BINANCE");
        let index_price = IndexPriceUpdate::new(
            instrument_id,
            Price::from("49999.0"),
            UnixNanos::default(),
            UnixNanos::default(),
        );

        let result = tester.on_index_price(&index_price);

        assert!(result.is_ok());
    }

    #[rstest]
    fn test_on_index_price_without_logging(mut config: ExecTesterConfig) {
        config.log_data = false;
        let mut tester = ExecTester::new(config);
        let instrument_id = InstrumentId::from("BTCUSDT-PERP.BINANCE");
        let index_price = IndexPriceUpdate::new(
            instrument_id,
            Price::from("49999.0"),
            UnixNanos::default(),
            UnixNanos::default(),
        );

        let result = tester.on_index_price(&index_price);

        assert!(result.is_ok());
    }

    #[rstest]
    fn test_on_stop_dry_run(mut config: ExecTesterConfig) {
        config.dry_run = true;
        let mut tester = ExecTester::new(config);

        let result = tester.on_stop();

        assert!(result.is_ok());
    }

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

    #[rstest]
    fn test_config_with_enable_brackets() {
        let config = ExecTesterConfig::default().with_enable_brackets(true);
        assert!(config.enable_brackets);
    }

    #[rstest]
    fn test_config_with_bracket_offset_ticks() {
        let config = ExecTesterConfig::default().with_bracket_offset_ticks(1000);
        assert_eq!(config.bracket_offset_ticks, 1000);
    }

    #[rstest]
    fn test_config_with_test_reject_post_only() {
        let config = ExecTesterConfig::default().with_test_reject_post_only(true);
        assert!(config.test_reject_post_only);
    }

    #[rstest]
    fn test_config_with_test_reject_reduce_only() {
        let config = ExecTesterConfig::default().with_test_reject_reduce_only(true);
        assert!(config.test_reject_reduce_only);
    }

    #[rstest]
    fn test_config_with_emulation_trigger() {
        let config =
            ExecTesterConfig::default().with_emulation_trigger(Some(TriggerType::LastPrice));
        assert_eq!(config.emulation_trigger, Some(TriggerType::LastPrice));
    }

    #[rstest]
    fn test_config_with_use_quote_quantity() {
        let config = ExecTesterConfig::default().with_use_quote_quantity(true);
        assert!(config.use_quote_quantity);
    }

    #[rstest]
    fn test_config_with_order_params() {
        let mut params = IndexMap::new();
        params.insert("key".to_string(), "value".to_string());
        let config = ExecTesterConfig::default().with_order_params(Some(params.clone()));
        assert_eq!(config.order_params, Some(params));
    }

    #[rstest]
    fn test_submit_bracket_order_no_instrument_returns_error(config: ExecTesterConfig) {
        let mut tester = ExecTester::new(config);

        let result = tester.submit_bracket_order(OrderSide::Buy, Price::from("50000.0"));

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("No instrument"));
    }

    #[rstest]
    fn test_submit_bracket_order_dry_run_returns_ok(
        mut config: ExecTesterConfig,
        instrument: InstrumentAny,
    ) {
        config.dry_run = true;
        config.enable_brackets = true;
        let mut tester = ExecTester::new(config);
        tester.instrument = Some(instrument);

        let result = tester.submit_bracket_order(OrderSide::Buy, Price::from("50000.0"));

        assert!(result.is_ok());
        assert!(tester.buy_order.is_none());
    }

    #[rstest]
    fn test_submit_bracket_order_unsupported_entry_type_returns_error(
        mut config: ExecTesterConfig,
        instrument: InstrumentAny,
    ) {
        config.enable_brackets = true;
        config.bracket_entry_order_type = OrderType::Market;
        let mut tester = ExecTester::new(config);
        tester.instrument = Some(instrument);

        let result = tester.submit_bracket_order(OrderSide::Buy, Price::from("50000.0"));

        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Only Limit entry orders are supported")
        );
    }

    #[rstest]
    fn test_submit_bracket_order_buys_disabled_returns_ok(
        mut config: ExecTesterConfig,
        instrument: InstrumentAny,
    ) {
        config.enable_brackets = true;
        config.enable_limit_buys = false;
        let mut tester = ExecTester::new(config);
        tester.instrument = Some(instrument);

        let result = tester.submit_bracket_order(OrderSide::Buy, Price::from("50000.0"));

        assert!(result.is_ok());
        assert!(tester.buy_order.is_none());
    }

    #[rstest]
    fn test_submit_bracket_order_sells_disabled_returns_ok(
        mut config: ExecTesterConfig,
        instrument: InstrumentAny,
    ) {
        config.enable_brackets = true;
        config.enable_limit_sells = false;
        let mut tester = ExecTester::new(config);
        tester.instrument = Some(instrument);

        let result = tester.submit_bracket_order(OrderSide::Sell, Price::from("50000.0"));

        assert!(result.is_ok());
        assert!(tester.sell_order.is_none());
    }

    #[rstest]
    fn test_submit_limit_order_creates_buy_order(
        config: ExecTesterConfig,
        instrument: InstrumentAny,
    ) {
        let cache = create_cache_with_instrument(&instrument);
        let mut tester = ExecTester::new(config);
        register_exec_tester(&mut tester, cache);
        tester.instrument = Some(instrument);

        let result = tester.submit_limit_order(OrderSide::Buy, Price::from("3000.0"));

        assert!(result.is_ok());
        assert!(tester.buy_order.is_some());
        let order = tester.buy_order.unwrap();
        assert_eq!(order.order_side(), OrderSide::Buy);
        assert_eq!(order.order_type(), OrderType::Limit);
    }

    #[rstest]
    fn test_submit_limit_order_creates_sell_order(
        config: ExecTesterConfig,
        instrument: InstrumentAny,
    ) {
        let cache = create_cache_with_instrument(&instrument);
        let mut tester = ExecTester::new(config);
        register_exec_tester(&mut tester, cache);
        tester.instrument = Some(instrument);

        let result = tester.submit_limit_order(OrderSide::Sell, Price::from("3000.0"));

        assert!(result.is_ok());
        assert!(tester.sell_order.is_some());
        let order = tester.sell_order.unwrap();
        assert_eq!(order.order_side(), OrderSide::Sell);
        assert_eq!(order.order_type(), OrderType::Limit);
    }

    #[rstest]
    fn test_submit_limit_order_with_post_only(
        mut config: ExecTesterConfig,
        instrument: InstrumentAny,
    ) {
        config.use_post_only = true;
        let cache = create_cache_with_instrument(&instrument);
        let mut tester = ExecTester::new(config);
        register_exec_tester(&mut tester, cache);
        tester.instrument = Some(instrument);

        let result = tester.submit_limit_order(OrderSide::Buy, Price::from("3000.0"));

        assert!(result.is_ok());
        let order = tester.buy_order.unwrap();
        assert!(order.is_post_only());
    }

    #[rstest]
    fn test_submit_limit_order_with_expire_time(
        mut config: ExecTesterConfig,
        instrument: InstrumentAny,
    ) {
        config.order_expire_time_delta_mins = Some(30);
        let cache = create_cache_with_instrument(&instrument);
        let mut tester = ExecTester::new(config);
        register_exec_tester(&mut tester, cache);
        tester.instrument = Some(instrument);

        let result = tester.submit_limit_order(OrderSide::Buy, Price::from("3000.0"));

        assert!(result.is_ok());
        let order = tester.buy_order.unwrap();
        assert_eq!(order.time_in_force(), TimeInForce::Gtd);
        assert!(order.expire_time().is_some());
    }

    #[rstest]
    fn test_submit_limit_order_with_order_params(
        mut config: ExecTesterConfig,
        instrument: InstrumentAny,
    ) {
        let mut params = IndexMap::new();
        params.insert("tdMode".to_string(), "cross".to_string());
        config.order_params = Some(params);
        let cache = create_cache_with_instrument(&instrument);
        let mut tester = ExecTester::new(config);
        register_exec_tester(&mut tester, cache);
        tester.instrument = Some(instrument);

        let result = tester.submit_limit_order(OrderSide::Buy, Price::from("3000.0"));

        assert!(result.is_ok());
        assert!(tester.buy_order.is_some());
    }

    #[rstest]
    fn test_submit_stop_market_order_creates_order(
        mut config: ExecTesterConfig,
        instrument: InstrumentAny,
    ) {
        config.enable_stop_buys = true;
        config.stop_order_type = OrderType::StopMarket;
        let cache = create_cache_with_instrument(&instrument);
        let mut tester = ExecTester::new(config);
        register_exec_tester(&mut tester, cache);
        tester.instrument = Some(instrument);

        let result = tester.submit_stop_order(OrderSide::Buy, Price::from("3500.0"), None);

        assert!(result.is_ok());
        assert!(tester.buy_stop_order.is_some());
        let order = tester.buy_stop_order.unwrap();
        assert_eq!(order.order_type(), OrderType::StopMarket);
        assert_eq!(order.trigger_price(), Some(Price::from("3500.0")));
    }

    #[rstest]
    fn test_submit_stop_limit_order_creates_order(
        mut config: ExecTesterConfig,
        instrument: InstrumentAny,
    ) {
        config.enable_stop_sells = true;
        config.stop_order_type = OrderType::StopLimit;
        let cache = create_cache_with_instrument(&instrument);
        let mut tester = ExecTester::new(config);
        register_exec_tester(&mut tester, cache);
        tester.instrument = Some(instrument);

        let result = tester.submit_stop_order(
            OrderSide::Sell,
            Price::from("2500.0"),
            Some(Price::from("2490.0")),
        );

        assert!(result.is_ok());
        assert!(tester.sell_stop_order.is_some());
        let order = tester.sell_stop_order.unwrap();
        assert_eq!(order.order_type(), OrderType::StopLimit);
        assert_eq!(order.trigger_price(), Some(Price::from("2500.0")));
        assert_eq!(order.price(), Some(Price::from("2490.0")));
    }

    #[rstest]
    fn test_submit_market_if_touched_order_creates_order(
        mut config: ExecTesterConfig,
        instrument: InstrumentAny,
    ) {
        config.enable_stop_buys = true;
        config.stop_order_type = OrderType::MarketIfTouched;
        let cache = create_cache_with_instrument(&instrument);
        let mut tester = ExecTester::new(config);
        register_exec_tester(&mut tester, cache);
        tester.instrument = Some(instrument);

        let result = tester.submit_stop_order(OrderSide::Buy, Price::from("2800.0"), None);

        assert!(result.is_ok());
        assert!(tester.buy_stop_order.is_some());
        let order = tester.buy_stop_order.unwrap();
        assert_eq!(order.order_type(), OrderType::MarketIfTouched);
    }

    #[rstest]
    fn test_submit_limit_if_touched_order_creates_order(
        mut config: ExecTesterConfig,
        instrument: InstrumentAny,
    ) {
        config.enable_stop_sells = true;
        config.stop_order_type = OrderType::LimitIfTouched;
        let cache = create_cache_with_instrument(&instrument);
        let mut tester = ExecTester::new(config);
        register_exec_tester(&mut tester, cache);
        tester.instrument = Some(instrument);

        let result = tester.submit_stop_order(
            OrderSide::Sell,
            Price::from("3200.0"),
            Some(Price::from("3190.0")),
        );

        assert!(result.is_ok());
        assert!(tester.sell_stop_order.is_some());
        let order = tester.sell_stop_order.unwrap();
        assert_eq!(order.order_type(), OrderType::LimitIfTouched);
    }

    #[rstest]
    fn test_submit_stop_order_with_emulation_trigger(
        mut config: ExecTesterConfig,
        instrument: InstrumentAny,
    ) {
        config.enable_stop_buys = true;
        config.stop_order_type = OrderType::StopMarket;
        config.emulation_trigger = Some(TriggerType::LastPrice);
        let cache = create_cache_with_instrument(&instrument);
        let mut tester = ExecTester::new(config);
        register_exec_tester(&mut tester, cache);
        tester.instrument = Some(instrument);

        let result = tester.submit_stop_order(OrderSide::Buy, Price::from("3500.0"), None);

        assert!(result.is_ok());
        let order = tester.buy_stop_order.unwrap();
        assert_eq!(order.emulation_trigger(), Some(TriggerType::LastPrice));
    }

    #[rstest]
    fn test_submit_bracket_order_creates_order_list(
        mut config: ExecTesterConfig,
        instrument: InstrumentAny,
    ) {
        config.enable_brackets = true;
        config.bracket_offset_ticks = 100;
        let cache = create_cache_with_instrument(&instrument);
        let mut tester = ExecTester::new(config);
        register_exec_tester(&mut tester, cache);
        tester.instrument = Some(instrument);

        let result = tester.submit_bracket_order(OrderSide::Buy, Price::from("3000.0"));

        assert!(result.is_ok());
        assert!(tester.buy_order.is_some());
        let order = tester.buy_order.unwrap();
        assert_eq!(order.order_side(), OrderSide::Buy);
        assert_eq!(order.order_type(), OrderType::Limit);
        assert_eq!(order.contingency_type(), Some(ContingencyType::Oto));
    }

    #[rstest]
    fn test_submit_bracket_order_sell_creates_order_list(
        mut config: ExecTesterConfig,
        instrument: InstrumentAny,
    ) {
        config.enable_brackets = true;
        config.bracket_offset_ticks = 100;
        let cache = create_cache_with_instrument(&instrument);
        let mut tester = ExecTester::new(config);
        register_exec_tester(&mut tester, cache);
        tester.instrument = Some(instrument);

        let result = tester.submit_bracket_order(OrderSide::Sell, Price::from("3000.0"));

        assert!(result.is_ok());
        assert!(tester.sell_order.is_some());
        let order = tester.sell_order.unwrap();
        assert_eq!(order.order_side(), OrderSide::Sell);
        assert_eq!(order.contingency_type(), Some(ContingencyType::Oto));
    }

    #[rstest]
    fn test_open_position_creates_market_order(
        config: ExecTesterConfig,
        instrument: InstrumentAny,
    ) {
        let cache = create_cache_with_instrument(&instrument);
        let mut tester = ExecTester::new(config);
        register_exec_tester(&mut tester, cache);
        tester.instrument = Some(instrument);

        let result = tester.open_position(Decimal::from(1));

        assert!(result.is_ok());
    }

    #[rstest]
    fn test_open_position_with_reduce_only_rejection(
        mut config: ExecTesterConfig,
        instrument: InstrumentAny,
    ) {
        config.test_reject_reduce_only = true;
        let cache = create_cache_with_instrument(&instrument);
        let mut tester = ExecTester::new(config);
        register_exec_tester(&mut tester, cache);
        tester.instrument = Some(instrument);

        // Should succeed in creating order (rejection happens at exchange)
        let result = tester.open_position(Decimal::from(1));

        assert!(result.is_ok());
    }

    #[rstest]
    fn test_submit_stop_limit_without_limit_price_fails(
        mut config: ExecTesterConfig,
        instrument: InstrumentAny,
    ) {
        config.enable_stop_buys = true;
        config.stop_order_type = OrderType::StopLimit;
        let cache = create_cache_with_instrument(&instrument);
        let mut tester = ExecTester::new(config);
        register_exec_tester(&mut tester, cache);
        tester.instrument = Some(instrument);

        let result = tester.submit_stop_order(OrderSide::Buy, Price::from("3500.0"), None);

        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("requires limit_price")
        );
    }

    #[rstest]
    fn test_submit_limit_if_touched_without_limit_price_fails(
        mut config: ExecTesterConfig,
        instrument: InstrumentAny,
    ) {
        config.enable_stop_sells = true;
        config.stop_order_type = OrderType::LimitIfTouched;
        let cache = create_cache_with_instrument(&instrument);
        let mut tester = ExecTester::new(config);
        register_exec_tester(&mut tester, cache);
        tester.instrument = Some(instrument);

        let result = tester.submit_stop_order(OrderSide::Sell, Price::from("3200.0"), None);

        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("requires limit_price")
        );
    }
}
