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

use nautilus_common::config::{ConfigError, ConfigErrorCollector, ConfigResult};
use nautilus_core::Params;
use nautilus_model::{
    enums::{BookType, OrderType, TimeInForce, TrailingOffsetType, TriggerType},
    identifiers::{ClientId, InstrumentId, StrategyId},
    types::Quantity,
};
use nautilus_trading::strategy::StrategyConfig;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

/// Configuration for the execution tester strategy.
#[derive(Debug, Clone, Deserialize, Serialize, bon::Builder)]
#[builder(finish_fn(name = build_inner, vis = ""))]
#[serde(default, deny_unknown_fields)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.testkit", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.testkit")
)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "tester configuration exposes independent execution scenario toggles"
)]
#[allow(
    clippy::unsafe_derive_deserialize,
    reason = "config type deserializes plain field values; unsafe PyO3 methods are unrelated"
)]
pub struct ExecTesterConfig {
    /// Base strategy configuration.
    #[builder(default)]
    pub base: StrategyConfig,
    /// Instrument ID to test.
    #[builder(default = InstrumentId::from("BTCUSDT-PERP.BINANCE"))]
    pub instrument_id: InstrumentId,
    /// Order quantity.
    #[builder(default = Quantity::from("0.001"))]
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
    #[builder(default = false)]
    pub subscribe_book: bool,
    /// Whether to subscribe to quotes.
    #[builder(default = true)]
    pub subscribe_quotes: bool,
    /// Whether to subscribe to trades.
    #[builder(default = true)]
    pub subscribe_trades: bool,
    /// Book type for order book subscriptions.
    #[builder(default = BookType::L2_MBP)]
    pub book_type: BookType,
    /// Order book depth for subscriptions.
    pub book_depth: Option<usize>,
    /// Order book interval in milliseconds.
    #[builder(default = 1000)]
    pub book_interval_ms: usize,
    /// Number of order book levels to print when logging.
    #[builder(default = 10)]
    pub book_levels_to_print: usize,
    /// Quantity to open position on start (positive for buy, negative for sell).
    pub open_position_on_start_qty: Option<Decimal>,
    /// Delay opening the start position until the first quote arrives.
    #[builder(default = false)]
    pub open_position_on_first_quote: bool,
    /// Time in force for opening position order.
    #[builder(default = TimeInForce::Gtc)]
    pub open_position_time_in_force: TimeInForce,
    /// Enable limit buy orders.
    #[builder(default = true)]
    pub enable_limit_buys: bool,
    /// Enable limit sell orders.
    #[builder(default = true)]
    pub enable_limit_sells: bool,
    /// Enable stop buy orders.
    #[builder(default = false)]
    pub enable_stop_buys: bool,
    /// Enable stop sell orders.
    #[builder(default = false)]
    pub enable_stop_sells: bool,
    /// Offset from TOB in price ticks for limit orders.
    #[builder(default = 500)]
    pub tob_offset_ticks: u64,
    /// Override time in force for limit orders (None uses GTC/GTD logic).
    pub limit_time_in_force: Option<TimeInForce>,
    /// Type of stop order (`STOP_MARKET`, `STOP_LIMIT`, `MARKET_IF_TOUCHED`, `LIMIT_IF_TOUCHED`).
    #[builder(default = OrderType::StopMarket)]
    pub stop_order_type: OrderType,
    /// Offset from market in price ticks for stop trigger.
    #[builder(default = 100)]
    pub stop_offset_ticks: u64,
    /// Offset from trigger price in ticks for stop limit price.
    pub stop_limit_offset_ticks: Option<u64>,
    /// Trigger type for stop orders.
    #[builder(default = TriggerType::Default)]
    pub stop_trigger_type: TriggerType,
    /// Override time in force for stop orders (None uses GTC/GTD logic).
    pub stop_time_in_force: Option<TimeInForce>,
    /// Trailing offset for `TRAILING_STOP_MARKET` orders.
    pub trailing_offset: Option<Decimal>,
    /// Trailing offset type (`BasisPoints` or `Price`).
    #[builder(default = TrailingOffsetType::BasisPoints)]
    pub trailing_offset_type: TrailingOffsetType,
    /// Enable bracket orders (entry with TP/SL).
    #[builder(default = false)]
    pub enable_brackets: bool,
    /// Submit limit buy and sell as an order list instead of individual orders.
    #[builder(default = false)]
    pub batch_submit_limit_pair: bool,
    /// Entry order type for bracket orders.
    #[builder(default = OrderType::Limit)]
    pub bracket_entry_order_type: OrderType,
    /// Offset in ticks for bracket TP/SL from entry price.
    #[builder(default = 500)]
    pub bracket_offset_ticks: u64,
    /// Modify limit orders to maintain TOB offset.
    #[builder(default = false)]
    pub modify_orders_to_maintain_tob_offset: bool,
    /// Modify stop orders to maintain offset.
    #[builder(default = false)]
    pub modify_stop_orders_to_maintain_offset: bool,
    /// Cancel and replace limit orders to maintain TOB offset.
    #[builder(default = false)]
    pub cancel_replace_orders_to_maintain_tob_offset: bool,
    /// Cancel and replace stop orders to maintain offset.
    #[builder(default = false)]
    pub cancel_replace_stop_orders_to_maintain_offset: bool,
    /// Use post-only for limit orders.
    #[builder(default = false)]
    pub use_post_only: bool,
    /// Place limit orders at marketable prices (cross the spread). Combined
    /// with `limit_time_in_force = Ioc`/`Fok`, exercises aggressive-fill
    /// (TC-E13, TC-E15) and passive-no-fill (TC-E14, TC-E16) scenarios when
    /// inverted with the standard passive offset.
    #[builder(default = false)]
    pub limit_aggressive: bool,
    /// Use quote quantity for orders.
    #[builder(default = false)]
    pub use_quote_quantity: bool,
    /// Emulation trigger type for orders.
    pub emulation_trigger: Option<TriggerType>,
    /// Cancel all orders on stop.
    #[builder(default = true)]
    pub cancel_orders_on_stop: bool,
    /// Close all positions on stop.
    #[builder(default = true)]
    pub close_positions_on_stop: bool,
    /// Time in force for closing positions (None defaults to GTC).
    pub close_positions_time_in_force: Option<TimeInForce>,
    /// Use `reduce_only` when closing positions.
    #[builder(default = true)]
    pub reduce_only_on_stop: bool,
    /// Use individual cancel commands instead of `cancel_all`.
    #[builder(default = false)]
    pub use_individual_cancels_on_stop: bool,
    /// Use batch cancel command when stopping.
    #[builder(default = false)]
    pub use_batch_cancel_on_stop: bool,
    /// Dry run mode (no order submission).
    #[builder(default = false)]
    pub dry_run: bool,
    /// Log received data.
    #[builder(default = true)]
    pub log_data: bool,
    /// Test post-only rejection by placing orders on wrong side of spread.
    #[builder(default = false)]
    pub test_reject_post_only: bool,
    /// Test reduce-only rejection by setting `reduce_only` on open position order.
    #[builder(default = false)]
    pub test_reject_reduce_only: bool,
    /// Programmatically attempt one strategy-wide modify against the next
    /// accepted limit order (whichever side acks first) to exercise the
    /// adapter's modify-rejection path (TC-E36). Independent of
    /// `modify_orders_to_maintain_tob_offset`, which only fires on price drift.
    /// Not honored when `batch_submit_limit_pair` is true; combine with
    /// individual buy/sell maintenance instead.
    #[builder(default = false)]
    pub test_modify_rejected: bool,
    /// Whether unsubscribe is supported on stop.
    #[builder(default = true)]
    pub can_unsubscribe: bool,
    /// Clamp computed prices to the instrument's `[min_price, max_price]` before submit.
    #[builder(default = false)]
    pub clamp_to_instrument_price_range: bool,
}

impl<S: exec_tester_config_builder::IsComplete> ExecTesterConfigBuilder<S> {
    /// Validates and builds the [`ExecTesterConfig`].
    ///
    /// # Errors
    ///
    /// Returns a [`ConfigError`] if any field fails validation
    /// (see [`ExecTesterConfig::validate`]).
    pub fn build(self) -> ConfigResult<ExecTesterConfig> {
        let config = self.build_inner();
        config.validate()?;
        Ok(config)
    }
}

impl ExecTesterConfig {
    /// Validates the execution tester configuration, collecting every field violation.
    ///
    /// # Errors
    ///
    /// Returns a [`ConfigError`] (a [`ConfigError::Multiple`] when more than one field is
    /// invalid) if any field fails validation.
    pub fn validate(&self) -> ConfigResult<()> {
        let mut errors = ConfigErrorCollector::new();

        errors.check(
            self.book_interval_ms > 0,
            ConfigError::range("book_interval_ms", "must be positive, was 0"),
        );

        if let Some(book_depth) = self.book_depth {
            errors.check(
                book_depth > 0,
                ConfigError::range("book_depth", "must be positive, was 0"),
            );
        }

        errors.into_result()
    }

    /// Creates a new [`ExecTesterConfig`] with minimal settings.
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
            book_interval_ms: 1000,
            book_levels_to_print: 10,
            open_position_on_start_qty: None,
            open_position_on_first_quote: false,
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
            limit_aggressive: false,
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
            test_modify_rejected: false,
            can_unsubscribe: true,
            clamp_to_instrument_price_range: false,
        }
    }
}

impl Default for ExecTesterConfig {
    fn default() -> Self {
        Self::builder()
            .build()
            .expect("default ExecTesterConfig should be valid")
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_default_config_is_valid() {
        assert!(ExecTesterConfig::builder().build().is_ok());
    }

    #[rstest]
    fn test_zero_book_interval_ms_rejected() {
        let result = ExecTesterConfig::builder().book_interval_ms(0).build();
        assert!(
            matches!(result, Err(ConfigError::Range { field, .. }) if field == "book_interval_ms")
        );
    }

    #[rstest]
    fn test_zero_book_depth_rejected() {
        let result = ExecTesterConfig::builder().book_depth(0).build();
        assert!(matches!(result, Err(ConfigError::Range { field, .. }) if field == "book_depth"));
    }
}
