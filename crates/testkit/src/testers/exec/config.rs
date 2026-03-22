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

use std::num::NonZeroUsize;

use nautilus_core::Params;
use nautilus_model::{
    enums::{BookType, OrderType, TimeInForce, TrailingOffsetType, TriggerType},
    identifiers::{ClientId, InstrumentId, StrategyId},
    types::Quantity,
};
use nautilus_trading::strategy::StrategyConfig;
use rust_decimal::Decimal;

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
    pub order_params: Option<Params>,
    /// Client ID to use for orders and subscriptions.
    pub client_id: Option<ClientId>,
    /// Whether to subscribe to order book.
    pub subscribe_book: bool,
    /// Whether to subscribe to quotes.
    pub subscribe_quotes: bool,
    /// Whether to subscribe to trades.
    pub subscribe_trades: bool,
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
    /// Override time in force for limit orders (None uses GTC/GTD logic).
    pub limit_time_in_force: Option<TimeInForce>,
    /// Type of stop order (STOP_MARKET, STOP_LIMIT, MARKET_IF_TOUCHED, LIMIT_IF_TOUCHED).
    pub stop_order_type: OrderType,
    /// Offset from market in price ticks for stop trigger.
    pub stop_offset_ticks: u64,
    /// Offset from trigger price in ticks for stop limit price.
    pub stop_limit_offset_ticks: Option<u64>,
    /// Trigger type for stop orders.
    pub stop_trigger_type: TriggerType,
    /// Override time in force for stop orders (None uses GTC/GTD logic).
    pub stop_time_in_force: Option<TimeInForce>,
    /// Trailing offset for TRAILING_STOP_MARKET orders.
    pub trailing_offset: Option<Decimal>,
    /// Trailing offset type (BasisPoints or Price).
    pub trailing_offset_type: TrailingOffsetType,
    /// Enable bracket orders (entry with TP/SL).
    pub enable_brackets: bool,
    /// Submit limit buy and sell as an order list instead of individual orders.
    pub batch_submit_limit_pair: bool,
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
            limit_time_in_force: None,
            stop_order_type: OrderType::StopMarket,
            stop_offset_ticks: 100,
            stop_limit_offset_ticks: None,
            stop_trigger_type: TriggerType::Default,
            stop_time_in_force: None,
            trailing_offset: None,
            trailing_offset_type: TrailingOffsetType::BasisPoints,
            enable_brackets: false,
            batch_submit_limit_pair: false,
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
    pub fn with_open_position_on_start(mut self, qty: Decimal) -> Self {
        self.open_position_on_start_qty = Some(qty);
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
    pub fn with_batch_submit_limit_pair(mut self, batch: bool) -> Self {
        self.batch_submit_limit_pair = batch;
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
    pub fn with_order_params(mut self, params: Option<Params>) -> Self {
        self.order_params = params;
        self
    }

    #[must_use]
    pub fn with_limit_time_in_force(mut self, tif: Option<TimeInForce>) -> Self {
        self.limit_time_in_force = tif;
        self
    }

    #[must_use]
    pub fn with_stop_time_in_force(mut self, tif: Option<TimeInForce>) -> Self {
        self.stop_time_in_force = tif;
        self
    }

    #[must_use]
    pub fn with_trailing_offset(mut self, offset: Decimal) -> Self {
        self.trailing_offset = Some(offset);
        self
    }

    #[must_use]
    pub fn with_trailing_offset_type(mut self, offset_type: TrailingOffsetType) -> Self {
        self.trailing_offset_type = offset_type;
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
            limit_time_in_force: None,
            stop_order_type: OrderType::StopMarket,
            stop_offset_ticks: 100,
            stop_limit_offset_ticks: None,
            stop_trigger_type: TriggerType::Default,
            stop_time_in_force: None,
            trailing_offset: None,
            trailing_offset_type: TrailingOffsetType::BasisPoints,
            enable_brackets: false,
            batch_submit_limit_pair: false,
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
