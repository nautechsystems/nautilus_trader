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

//! Python bindings for live tester configuration.

use nautilus_common::{actor::DataActorConfig, python::config_error_to_pyvalue_err};
use nautilus_model::{
    data::BarType,
    enums::TimeInForce,
    identifiers::{ActorId, ClientId, InstrumentId, StrategyId},
    types::Quantity,
};
use nautilus_trading::strategy::StrategyConfig;
use pyo3::prelude::*;
use rust_decimal::Decimal;

use crate::{DataTesterConfig, ExecTesterConfig};

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl DataTesterConfig {
    /// Configuration for the data tester actor.
    #[new]
    #[allow(
        clippy::needless_pass_by_value,
        reason = "PyO3 #[new] requires owned params"
    )]
    #[expect(clippy::too_many_arguments)]
    #[pyo3(signature = (
        actor_id = None,
        client_id = None,
        instrument_ids = None,
        bar_types = None,
        subscribe_book_deltas = None,
        subscribe_book_depth = None,
        subscribe_book_at_interval = None,
        subscribe_quotes = None,
        subscribe_trades = None,
        subscribe_mark_prices = None,
        subscribe_index_prices = None,
        subscribe_funding_rates = None,
        subscribe_bars = None,
        subscribe_instrument = None,
        subscribe_instrument_status = None,
        subscribe_instrument_close = None,
        subscribe_option_greeks = None,
        can_unsubscribe = None,
        request_instruments = None,
        request_quotes = None,
        request_trades = None,
        request_bars = None,
        request_book_snapshot = None,
        request_book_deltas = None,
        request_funding_rates = None,
        book_depth = None,
        book_interval_ms = None,
        book_levels_to_print = None,
        manage_book = None,
        log_data = None,
        stats_interval_secs = None,
        log_events = None,
        log_commands = None,
    ))]
    fn py_new(
        actor_id: Option<ActorId>,
        client_id: Option<ClientId>,
        instrument_ids: Option<Vec<InstrumentId>>,
        bar_types: Option<Vec<BarType>>,
        subscribe_book_deltas: Option<bool>,
        subscribe_book_depth: Option<bool>,
        subscribe_book_at_interval: Option<bool>,
        subscribe_quotes: Option<bool>,
        subscribe_trades: Option<bool>,
        subscribe_mark_prices: Option<bool>,
        subscribe_index_prices: Option<bool>,
        subscribe_funding_rates: Option<bool>,
        subscribe_bars: Option<bool>,
        subscribe_instrument: Option<bool>,
        subscribe_instrument_status: Option<bool>,
        subscribe_instrument_close: Option<bool>,
        subscribe_option_greeks: Option<bool>,
        can_unsubscribe: Option<bool>,
        request_instruments: Option<bool>,
        request_quotes: Option<bool>,
        request_trades: Option<bool>,
        request_bars: Option<bool>,
        request_book_snapshot: Option<bool>,
        request_book_deltas: Option<bool>,
        request_funding_rates: Option<bool>,
        book_depth: Option<usize>,
        book_interval_ms: Option<usize>,
        book_levels_to_print: Option<usize>,
        manage_book: Option<bool>,
        log_data: Option<bool>,
        stats_interval_secs: Option<u64>,
        log_events: Option<bool>,
        log_commands: Option<bool>,
    ) -> PyResult<Self> {
        let defaults = Self::default();
        let config = Self {
            base: DataActorConfig {
                actor_id,
                log_events: log_events.unwrap_or(defaults.base.log_events),
                log_commands: log_commands.unwrap_or(defaults.base.log_commands),
            },
            instrument_ids: instrument_ids.unwrap_or(defaults.instrument_ids),
            client_id,
            bar_types,
            subscribe_book_deltas: subscribe_book_deltas.unwrap_or(defaults.subscribe_book_deltas),
            subscribe_book_depth: subscribe_book_depth.unwrap_or(defaults.subscribe_book_depth),
            subscribe_book_at_interval: subscribe_book_at_interval
                .unwrap_or(defaults.subscribe_book_at_interval),
            subscribe_quotes: subscribe_quotes.unwrap_or(defaults.subscribe_quotes),
            subscribe_trades: subscribe_trades.unwrap_or(defaults.subscribe_trades),
            subscribe_mark_prices: subscribe_mark_prices.unwrap_or(defaults.subscribe_mark_prices),
            subscribe_index_prices: subscribe_index_prices
                .unwrap_or(defaults.subscribe_index_prices),
            subscribe_funding_rates: subscribe_funding_rates
                .unwrap_or(defaults.subscribe_funding_rates),
            subscribe_bars: subscribe_bars.unwrap_or(defaults.subscribe_bars),
            subscribe_instrument: subscribe_instrument.unwrap_or(defaults.subscribe_instrument),
            subscribe_instrument_status: subscribe_instrument_status
                .unwrap_or(defaults.subscribe_instrument_status),
            subscribe_instrument_close: subscribe_instrument_close
                .unwrap_or(defaults.subscribe_instrument_close),
            subscribe_option_greeks: subscribe_option_greeks
                .unwrap_or(defaults.subscribe_option_greeks),
            subscribe_params: defaults.subscribe_params,
            request_params: defaults.request_params,
            can_unsubscribe: can_unsubscribe.unwrap_or(defaults.can_unsubscribe),
            request_instruments: request_instruments.unwrap_or(defaults.request_instruments),
            request_quotes: request_quotes.unwrap_or(defaults.request_quotes),
            request_trades: request_trades.unwrap_or(defaults.request_trades),
            request_bars: request_bars.unwrap_or(defaults.request_bars),
            request_book_snapshot: request_book_snapshot.unwrap_or(defaults.request_book_snapshot),
            request_book_deltas: request_book_deltas.unwrap_or(defaults.request_book_deltas),
            request_funding_rates: request_funding_rates.unwrap_or(defaults.request_funding_rates),
            book_type: defaults.book_type,
            book_depth,
            book_interval_ms: book_interval_ms.unwrap_or(defaults.book_interval_ms),
            book_levels_to_print: book_levels_to_print.unwrap_or(defaults.book_levels_to_print),
            manage_book: manage_book.unwrap_or(defaults.manage_book),
            log_data: log_data.unwrap_or(defaults.log_data),
            stats_interval_secs: stats_interval_secs.unwrap_or(defaults.stats_interval_secs),
        };
        config.validate().map_err(config_error_to_pyvalue_err)?;
        Ok(config)
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl ExecTesterConfig {
    /// Configuration for the execution tester strategy.
    #[new]
    #[allow(
        clippy::needless_pass_by_value,
        reason = "PyO3 #[new] requires owned params"
    )]
    #[expect(clippy::too_many_arguments)]
    #[pyo3(signature = (
        strategy_id = None,
        order_id_tag = None,
        external_order_claims = None,
        instrument_id = None,
        client_id = None,
        order_qty = None,
        subscribe_book = None,
        subscribe_quotes = None,
        subscribe_trades = None,
        open_position_on_start_qty = None,
        open_position_on_first_quote = None,
        open_position_time_in_force = None,
        enable_limit_buys = None,
        enable_limit_sells = None,
        enable_stop_buys = None,
        enable_stop_sells = None,
        tob_offset_ticks = None,
        limit_time_in_force = None,
        use_post_only = None,
        limit_aggressive = None,
        cancel_orders_on_stop = None,
        close_positions_on_stop = None,
        close_positions_time_in_force = None,
        reduce_only_on_stop = None,
        dry_run = None,
        log_data = None,
        can_unsubscribe = None,
        clamp_to_instrument_price_range = None,
        log_events = None,
        log_commands = None,
    ))]
    fn py_new(
        strategy_id: Option<StrategyId>,
        order_id_tag: Option<String>,
        external_order_claims: Option<Vec<InstrumentId>>,
        instrument_id: Option<InstrumentId>,
        client_id: Option<ClientId>,
        order_qty: Option<Quantity>,
        subscribe_book: Option<bool>,
        subscribe_quotes: Option<bool>,
        subscribe_trades: Option<bool>,
        open_position_on_start_qty: Option<Decimal>,
        open_position_on_first_quote: Option<bool>,
        open_position_time_in_force: Option<TimeInForce>,
        enable_limit_buys: Option<bool>,
        enable_limit_sells: Option<bool>,
        enable_stop_buys: Option<bool>,
        enable_stop_sells: Option<bool>,
        tob_offset_ticks: Option<u64>,
        limit_time_in_force: Option<TimeInForce>,
        use_post_only: Option<bool>,
        limit_aggressive: Option<bool>,
        cancel_orders_on_stop: Option<bool>,
        close_positions_on_stop: Option<bool>,
        close_positions_time_in_force: Option<TimeInForce>,
        reduce_only_on_stop: Option<bool>,
        dry_run: Option<bool>,
        log_data: Option<bool>,
        can_unsubscribe: Option<bool>,
        clamp_to_instrument_price_range: Option<bool>,
        log_events: Option<bool>,
        log_commands: Option<bool>,
    ) -> Self {
        let defaults = Self::default();
        Self {
            base: StrategyConfig {
                strategy_id,
                order_id_tag,
                external_order_claims,
                log_events: log_events.unwrap_or(defaults.base.log_events),
                log_commands: log_commands.unwrap_or(defaults.base.log_commands),
                ..Default::default()
            },
            instrument_id: instrument_id.unwrap_or(defaults.instrument_id),
            order_qty: order_qty.unwrap_or(defaults.order_qty),
            order_display_qty: defaults.order_display_qty,
            order_expire_time_delta_mins: defaults.order_expire_time_delta_mins,
            order_params: defaults.order_params,
            client_id,
            subscribe_book: subscribe_book.unwrap_or(defaults.subscribe_book),
            subscribe_quotes: subscribe_quotes.unwrap_or(defaults.subscribe_quotes),
            subscribe_trades: subscribe_trades.unwrap_or(defaults.subscribe_trades),
            book_type: defaults.book_type,
            book_depth: defaults.book_depth,
            book_interval_ms: defaults.book_interval_ms,
            book_levels_to_print: defaults.book_levels_to_print,
            open_position_on_start_qty,
            open_position_on_first_quote: open_position_on_first_quote
                .unwrap_or(defaults.open_position_on_first_quote),
            open_position_time_in_force: open_position_time_in_force
                .unwrap_or(defaults.open_position_time_in_force),
            enable_limit_buys: enable_limit_buys.unwrap_or(defaults.enable_limit_buys),
            enable_limit_sells: enable_limit_sells.unwrap_or(defaults.enable_limit_sells),
            enable_stop_buys: enable_stop_buys.unwrap_or(defaults.enable_stop_buys),
            enable_stop_sells: enable_stop_sells.unwrap_or(defaults.enable_stop_sells),
            tob_offset_ticks: tob_offset_ticks.unwrap_or(defaults.tob_offset_ticks),
            limit_time_in_force,
            stop_order_type: defaults.stop_order_type,
            stop_offset_ticks: defaults.stop_offset_ticks,
            stop_limit_offset_ticks: defaults.stop_limit_offset_ticks,
            stop_trigger_type: defaults.stop_trigger_type,
            stop_time_in_force: defaults.stop_time_in_force,
            trailing_offset: defaults.trailing_offset,
            trailing_offset_type: defaults.trailing_offset_type,
            enable_brackets: defaults.enable_brackets,
            batch_submit_limit_pair: defaults.batch_submit_limit_pair,
            bracket_entry_order_type: defaults.bracket_entry_order_type,
            bracket_offset_ticks: defaults.bracket_offset_ticks,
            modify_orders_to_maintain_tob_offset: defaults.modify_orders_to_maintain_tob_offset,
            modify_stop_orders_to_maintain_offset: defaults.modify_stop_orders_to_maintain_offset,
            cancel_replace_orders_to_maintain_tob_offset: defaults
                .cancel_replace_orders_to_maintain_tob_offset,
            cancel_replace_stop_orders_to_maintain_offset: defaults
                .cancel_replace_stop_orders_to_maintain_offset,
            use_post_only: use_post_only.unwrap_or(defaults.use_post_only),
            limit_aggressive: limit_aggressive.unwrap_or(defaults.limit_aggressive),
            use_quote_quantity: defaults.use_quote_quantity,
            emulation_trigger: defaults.emulation_trigger,
            cancel_orders_on_stop: cancel_orders_on_stop.unwrap_or(defaults.cancel_orders_on_stop),
            close_positions_on_stop: close_positions_on_stop
                .unwrap_or(defaults.close_positions_on_stop),
            close_positions_time_in_force,
            reduce_only_on_stop: reduce_only_on_stop.unwrap_or(defaults.reduce_only_on_stop),
            use_individual_cancels_on_stop: defaults.use_individual_cancels_on_stop,
            use_batch_cancel_on_stop: defaults.use_batch_cancel_on_stop,
            dry_run: dry_run.unwrap_or(defaults.dry_run),
            log_data: log_data.unwrap_or(defaults.log_data),
            test_reject_post_only: defaults.test_reject_post_only,
            test_reject_reduce_only: defaults.test_reject_reduce_only,
            test_modify_rejected: defaults.test_modify_rejected,
            can_unsubscribe: can_unsubscribe.unwrap_or(defaults.can_unsubscribe),
            clamp_to_instrument_price_range: clamp_to_instrument_price_range
                .unwrap_or(defaults.clamp_to_instrument_price_range),
        }
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }
}
