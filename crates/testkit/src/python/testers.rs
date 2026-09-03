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
use nautilus_core::from_pydict;
use nautilus_model::{
    data::BarType,
    enums::{BookType, OrderType, TimeInForce, TrailingOffsetType, TriggerType},
    identifiers::{ActorId, ClientId, InstrumentId, StrategyId},
    types::Quantity,
};
use nautilus_trading::strategy::StrategyConfig;
use pyo3::{prelude::*, types::PyDict};
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

    #[getter]
    #[pyo3(name = "actor_id")]
    const fn py_actor_id(&self) -> Option<ActorId> {
        self.base.actor_id
    }

    #[getter]
    #[pyo3(name = "client_id")]
    const fn py_client_id(&self) -> Option<ClientId> {
        self.client_id
    }

    #[getter]
    #[pyo3(name = "instrument_ids")]
    fn py_instrument_ids(&self) -> Vec<InstrumentId> {
        self.instrument_ids.clone()
    }

    #[getter]
    #[pyo3(name = "bar_types")]
    fn py_bar_types(&self) -> Option<Vec<BarType>> {
        self.bar_types.clone()
    }

    #[getter]
    #[pyo3(name = "subscribe_book_deltas")]
    const fn py_subscribe_book_deltas(&self) -> bool {
        self.subscribe_book_deltas
    }

    #[getter]
    #[pyo3(name = "subscribe_book_depth")]
    const fn py_subscribe_book_depth(&self) -> bool {
        self.subscribe_book_depth
    }

    #[getter]
    #[pyo3(name = "subscribe_book_at_interval")]
    const fn py_subscribe_book_at_interval(&self) -> bool {
        self.subscribe_book_at_interval
    }

    #[getter]
    #[pyo3(name = "subscribe_quotes")]
    const fn py_subscribe_quotes(&self) -> bool {
        self.subscribe_quotes
    }

    #[getter]
    #[pyo3(name = "subscribe_trades")]
    const fn py_subscribe_trades(&self) -> bool {
        self.subscribe_trades
    }

    #[getter]
    #[pyo3(name = "subscribe_mark_prices")]
    const fn py_subscribe_mark_prices(&self) -> bool {
        self.subscribe_mark_prices
    }

    #[getter]
    #[pyo3(name = "subscribe_index_prices")]
    const fn py_subscribe_index_prices(&self) -> bool {
        self.subscribe_index_prices
    }

    #[getter]
    #[pyo3(name = "subscribe_funding_rates")]
    const fn py_subscribe_funding_rates(&self) -> bool {
        self.subscribe_funding_rates
    }

    #[getter]
    #[pyo3(name = "subscribe_bars")]
    const fn py_subscribe_bars(&self) -> bool {
        self.subscribe_bars
    }

    #[getter]
    #[pyo3(name = "subscribe_instrument")]
    const fn py_subscribe_instrument(&self) -> bool {
        self.subscribe_instrument
    }

    #[getter]
    #[pyo3(name = "subscribe_instrument_status")]
    const fn py_subscribe_instrument_status(&self) -> bool {
        self.subscribe_instrument_status
    }

    #[getter]
    #[pyo3(name = "subscribe_instrument_close")]
    const fn py_subscribe_instrument_close(&self) -> bool {
        self.subscribe_instrument_close
    }

    #[getter]
    #[pyo3(name = "subscribe_option_greeks")]
    const fn py_subscribe_option_greeks(&self) -> bool {
        self.subscribe_option_greeks
    }

    #[getter]
    #[pyo3(name = "can_unsubscribe")]
    const fn py_can_unsubscribe(&self) -> bool {
        self.can_unsubscribe
    }

    #[getter]
    #[pyo3(name = "request_instruments")]
    const fn py_request_instruments(&self) -> bool {
        self.request_instruments
    }

    #[getter]
    #[pyo3(name = "request_quotes")]
    const fn py_request_quotes(&self) -> bool {
        self.request_quotes
    }

    #[getter]
    #[pyo3(name = "request_trades")]
    const fn py_request_trades(&self) -> bool {
        self.request_trades
    }

    #[getter]
    #[pyo3(name = "request_bars")]
    const fn py_request_bars(&self) -> bool {
        self.request_bars
    }

    #[getter]
    #[pyo3(name = "request_book_snapshot")]
    const fn py_request_book_snapshot(&self) -> bool {
        self.request_book_snapshot
    }

    #[getter]
    #[pyo3(name = "request_book_deltas")]
    const fn py_request_book_deltas(&self) -> bool {
        self.request_book_deltas
    }

    #[getter]
    #[pyo3(name = "request_funding_rates")]
    const fn py_request_funding_rates(&self) -> bool {
        self.request_funding_rates
    }

    #[getter]
    #[pyo3(name = "book_depth")]
    const fn py_book_depth(&self) -> Option<usize> {
        self.book_depth
    }

    #[getter]
    #[pyo3(name = "book_interval_ms")]
    const fn py_book_interval_ms(&self) -> usize {
        self.book_interval_ms
    }

    #[getter]
    #[pyo3(name = "book_levels_to_print")]
    const fn py_book_levels_to_print(&self) -> usize {
        self.book_levels_to_print
    }

    #[getter]
    #[pyo3(name = "manage_book")]
    const fn py_manage_book(&self) -> bool {
        self.manage_book
    }

    #[getter]
    #[pyo3(name = "log_data")]
    const fn py_log_data(&self) -> bool {
        self.log_data
    }

    #[getter]
    #[pyo3(name = "stats_interval_secs")]
    const fn py_stats_interval_secs(&self) -> u64 {
        self.stats_interval_secs
    }

    #[getter]
    #[pyo3(name = "log_events")]
    const fn py_log_events(&self) -> bool {
        self.base.log_events
    }

    #[getter]
    #[pyo3(name = "log_commands")]
    const fn py_log_commands(&self) -> bool {
        self.base.log_commands
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
        use_hyphens_in_client_order_ids = None,
        use_uuid_client_order_ids = None,
        external_order_instrument_ids = None,
        instrument_id = None,
        client_id = None,
        order_qty = None,
        order_display_qty = None,
        order_expire_time_delta_mins = None,
        order_params = None,
        subscribe_book = None,
        subscribe_quotes = None,
        subscribe_trades = None,
        book_type = None,
        book_depth = None,
        book_interval_ms = None,
        book_levels_to_print = None,
        open_position_on_start_qty = None,
        open_position_on_first_quote = None,
        open_position_time_in_force = None,
        enable_limit_buys = None,
        enable_limit_sells = None,
        enable_stop_buys = None,
        enable_stop_sells = None,
        tob_offset_ticks = None,
        limit_time_in_force = None,
        stop_order_type = None,
        stop_offset_ticks = None,
        stop_limit_offset_ticks = None,
        stop_trigger_type = None,
        stop_time_in_force = None,
        trailing_offset = None,
        trailing_offset_type = None,
        enable_brackets = None,
        batch_submit_limit_pair = None,
        bracket_entry_order_type = None,
        bracket_offset_ticks = None,
        modify_orders_to_maintain_tob_offset = None,
        modify_stop_orders_to_maintain_offset = None,
        cancel_replace_orders_to_maintain_tob_offset = None,
        cancel_replace_stop_orders_to_maintain_offset = None,
        trigger_limit_order_maintenance_once = None,
        use_post_only = None,
        limit_aggressive = None,
        use_quote_quantity = None,
        emulation_trigger = None,
        use_individual_cancels_on_stop = None,
        cancel_orders_on_stop = None,
        close_positions_on_stop = None,
        close_positions_qty_precision = None,
        close_positions_time_in_force = None,
        reduce_only_on_stop = None,
        use_batch_cancel_on_stop = None,
        dry_run = None,
        log_data = None,
        test_reject_post_only = None,
        test_reject_reduce_only = None,
        test_modify_rejected = None,
        can_unsubscribe = None,
        clamp_to_instrument_price_range = None,
        log_events = None,
        log_commands = None,
    ))]
    fn py_new(
        py: Python<'_>,
        strategy_id: Option<StrategyId>,
        order_id_tag: Option<String>,
        use_hyphens_in_client_order_ids: Option<bool>,
        use_uuid_client_order_ids: Option<bool>,
        external_order_instrument_ids: Option<Vec<InstrumentId>>,
        instrument_id: Option<InstrumentId>,
        client_id: Option<ClientId>,
        order_qty: Option<Quantity>,
        order_display_qty: Option<Quantity>,
        order_expire_time_delta_mins: Option<u64>,
        order_params: Option<Py<PyDict>>,
        subscribe_book: Option<bool>,
        subscribe_quotes: Option<bool>,
        subscribe_trades: Option<bool>,
        book_type: Option<BookType>,
        book_depth: Option<usize>,
        book_interval_ms: Option<usize>,
        book_levels_to_print: Option<usize>,
        open_position_on_start_qty: Option<Decimal>,
        open_position_on_first_quote: Option<bool>,
        open_position_time_in_force: Option<TimeInForce>,
        enable_limit_buys: Option<bool>,
        enable_limit_sells: Option<bool>,
        enable_stop_buys: Option<bool>,
        enable_stop_sells: Option<bool>,
        tob_offset_ticks: Option<u64>,
        limit_time_in_force: Option<TimeInForce>,
        stop_order_type: Option<OrderType>,
        stop_offset_ticks: Option<u64>,
        stop_limit_offset_ticks: Option<u64>,
        stop_trigger_type: Option<TriggerType>,
        stop_time_in_force: Option<TimeInForce>,
        trailing_offset: Option<Decimal>,
        trailing_offset_type: Option<TrailingOffsetType>,
        enable_brackets: Option<bool>,
        batch_submit_limit_pair: Option<bool>,
        bracket_entry_order_type: Option<OrderType>,
        bracket_offset_ticks: Option<u64>,
        modify_orders_to_maintain_tob_offset: Option<bool>,
        modify_stop_orders_to_maintain_offset: Option<bool>,
        cancel_replace_orders_to_maintain_tob_offset: Option<bool>,
        cancel_replace_stop_orders_to_maintain_offset: Option<bool>,
        trigger_limit_order_maintenance_once: Option<bool>,
        use_post_only: Option<bool>,
        limit_aggressive: Option<bool>,
        use_quote_quantity: Option<bool>,
        emulation_trigger: Option<TriggerType>,
        use_individual_cancels_on_stop: Option<bool>,
        cancel_orders_on_stop: Option<bool>,
        close_positions_on_stop: Option<bool>,
        close_positions_qty_precision: Option<u8>,
        close_positions_time_in_force: Option<TimeInForce>,
        reduce_only_on_stop: Option<bool>,
        use_batch_cancel_on_stop: Option<bool>,
        dry_run: Option<bool>,
        log_data: Option<bool>,
        test_reject_post_only: Option<bool>,
        test_reject_reduce_only: Option<bool>,
        test_modify_rejected: Option<bool>,
        can_unsubscribe: Option<bool>,
        clamp_to_instrument_price_range: Option<bool>,
        log_events: Option<bool>,
        log_commands: Option<bool>,
    ) -> PyResult<Self> {
        let defaults = Self::default();
        let order_params = match order_params {
            Some(dict) => from_pydict(py, &dict)?,
            None => None,
        };
        let config = Self {
            base: StrategyConfig {
                strategy_id,
                order_id_tag,
                use_hyphens_in_client_order_ids: use_hyphens_in_client_order_ids
                    .unwrap_or(defaults.base.use_hyphens_in_client_order_ids),
                use_uuid_client_order_ids: use_uuid_client_order_ids
                    .unwrap_or(defaults.base.use_uuid_client_order_ids),
                external_order_instrument_ids,
                log_events: log_events.unwrap_or(defaults.base.log_events),
                log_commands: log_commands.unwrap_or(defaults.base.log_commands),
                ..Default::default()
            },
            instrument_id: instrument_id.unwrap_or(defaults.instrument_id),
            order_qty: order_qty.unwrap_or(defaults.order_qty),
            order_display_qty,
            order_expire_time_delta_mins,
            order_params,
            client_id,
            subscribe_book: subscribe_book.unwrap_or(defaults.subscribe_book),
            subscribe_quotes: subscribe_quotes.unwrap_or(defaults.subscribe_quotes),
            subscribe_trades: subscribe_trades.unwrap_or(defaults.subscribe_trades),
            book_type: book_type.unwrap_or(defaults.book_type),
            book_depth,
            book_interval_ms: book_interval_ms.unwrap_or(defaults.book_interval_ms),
            book_levels_to_print: book_levels_to_print.unwrap_or(defaults.book_levels_to_print),
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
            stop_order_type: stop_order_type.unwrap_or(defaults.stop_order_type),
            stop_offset_ticks: stop_offset_ticks.unwrap_or(defaults.stop_offset_ticks),
            stop_limit_offset_ticks,
            stop_trigger_type: stop_trigger_type.unwrap_or(defaults.stop_trigger_type),
            stop_time_in_force,
            trailing_offset,
            trailing_offset_type: trailing_offset_type.unwrap_or(defaults.trailing_offset_type),
            enable_brackets: enable_brackets.unwrap_or(defaults.enable_brackets),
            batch_submit_limit_pair: batch_submit_limit_pair
                .unwrap_or(defaults.batch_submit_limit_pair),
            bracket_entry_order_type: bracket_entry_order_type
                .unwrap_or(defaults.bracket_entry_order_type),
            bracket_offset_ticks: bracket_offset_ticks.unwrap_or(defaults.bracket_offset_ticks),
            modify_orders_to_maintain_tob_offset: modify_orders_to_maintain_tob_offset
                .unwrap_or(defaults.modify_orders_to_maintain_tob_offset),
            modify_stop_orders_to_maintain_offset: modify_stop_orders_to_maintain_offset
                .unwrap_or(defaults.modify_stop_orders_to_maintain_offset),
            cancel_replace_orders_to_maintain_tob_offset:
                cancel_replace_orders_to_maintain_tob_offset
                    .unwrap_or(defaults.cancel_replace_orders_to_maintain_tob_offset),
            cancel_replace_stop_orders_to_maintain_offset:
                cancel_replace_stop_orders_to_maintain_offset
                    .unwrap_or(defaults.cancel_replace_stop_orders_to_maintain_offset),
            trigger_limit_order_maintenance_once: trigger_limit_order_maintenance_once
                .unwrap_or(defaults.trigger_limit_order_maintenance_once),
            use_post_only: use_post_only.unwrap_or(defaults.use_post_only),
            limit_aggressive: limit_aggressive.unwrap_or(defaults.limit_aggressive),
            use_quote_quantity: use_quote_quantity.unwrap_or(defaults.use_quote_quantity),
            emulation_trigger,
            use_individual_cancels_on_stop: use_individual_cancels_on_stop
                .unwrap_or(defaults.use_individual_cancels_on_stop),
            cancel_orders_on_stop: cancel_orders_on_stop.unwrap_or(defaults.cancel_orders_on_stop),
            close_positions_on_stop: close_positions_on_stop
                .unwrap_or(defaults.close_positions_on_stop),
            close_positions_qty_precision,
            close_positions_time_in_force,
            reduce_only_on_stop: reduce_only_on_stop.unwrap_or(defaults.reduce_only_on_stop),
            use_batch_cancel_on_stop: use_batch_cancel_on_stop
                .unwrap_or(defaults.use_batch_cancel_on_stop),
            dry_run: dry_run.unwrap_or(defaults.dry_run),
            log_data: log_data.unwrap_or(defaults.log_data),
            test_reject_post_only: test_reject_post_only.unwrap_or(defaults.test_reject_post_only),
            test_reject_reduce_only: test_reject_reduce_only
                .unwrap_or(defaults.test_reject_reduce_only),
            test_modify_rejected: test_modify_rejected.unwrap_or(defaults.test_modify_rejected),
            can_unsubscribe: can_unsubscribe.unwrap_or(defaults.can_unsubscribe),
            clamp_to_instrument_price_range: clamp_to_instrument_price_range
                .unwrap_or(defaults.clamp_to_instrument_price_range),
        };
        config.validate().map_err(config_error_to_pyvalue_err)?;
        Ok(config)
    }

    #[getter]
    #[pyo3(name = "strategy_id")]
    const fn py_strategy_id(&self) -> Option<StrategyId> {
        self.base.strategy_id
    }

    #[getter]
    #[pyo3(name = "order_id_tag")]
    fn py_order_id_tag(&self) -> Option<&str> {
        self.base.order_id_tag.as_deref()
    }

    #[getter]
    #[pyo3(name = "use_hyphens_in_client_order_ids")]
    const fn py_use_hyphens_in_client_order_ids(&self) -> bool {
        self.base.use_hyphens_in_client_order_ids
    }

    #[getter]
    #[pyo3(name = "use_uuid_client_order_ids")]
    const fn py_use_uuid_client_order_ids(&self) -> bool {
        self.base.use_uuid_client_order_ids
    }

    #[getter]
    #[pyo3(name = "external_order_instrument_ids")]
    fn py_external_order_instrument_ids(&self) -> Option<Vec<InstrumentId>> {
        self.base.external_order_instrument_ids.clone()
    }

    #[getter]
    #[pyo3(name = "instrument_id")]
    const fn py_instrument_id(&self) -> InstrumentId {
        self.instrument_id
    }

    #[getter]
    #[pyo3(name = "client_id")]
    const fn py_client_id(&self) -> Option<ClientId> {
        self.client_id
    }

    #[getter]
    #[pyo3(name = "order_qty")]
    const fn py_order_qty(&self) -> Quantity {
        self.order_qty
    }

    #[getter]
    #[pyo3(name = "order_display_qty")]
    const fn py_order_display_qty(&self) -> Option<Quantity> {
        self.order_display_qty
    }

    #[getter]
    #[pyo3(name = "order_expire_time_delta_mins")]
    const fn py_order_expire_time_delta_mins(&self) -> Option<u64> {
        self.order_expire_time_delta_mins
    }

    #[getter]
    #[pyo3(name = "order_params")]
    fn py_order_params(&self, py: Python<'_>) -> PyResult<Option<Py<PyDict>>> {
        self.order_params
            .as_ref()
            .map(|params| params.to_pydict(py))
            .transpose()
    }

    #[getter]
    #[pyo3(name = "subscribe_book")]
    const fn py_subscribe_book(&self) -> bool {
        self.subscribe_book
    }

    #[getter]
    #[pyo3(name = "subscribe_quotes")]
    const fn py_subscribe_quotes(&self) -> bool {
        self.subscribe_quotes
    }

    #[getter]
    #[pyo3(name = "subscribe_trades")]
    const fn py_subscribe_trades(&self) -> bool {
        self.subscribe_trades
    }

    #[getter]
    #[pyo3(name = "book_type")]
    const fn py_book_type(&self) -> BookType {
        self.book_type
    }

    #[getter]
    #[pyo3(name = "book_depth")]
    const fn py_book_depth(&self) -> Option<usize> {
        self.book_depth
    }

    #[getter]
    #[pyo3(name = "book_interval_ms")]
    const fn py_book_interval_ms(&self) -> usize {
        self.book_interval_ms
    }

    #[getter]
    #[pyo3(name = "book_levels_to_print")]
    const fn py_book_levels_to_print(&self) -> usize {
        self.book_levels_to_print
    }

    #[getter]
    #[pyo3(name = "open_position_on_start_qty")]
    const fn py_open_position_on_start_qty(&self) -> Option<Decimal> {
        self.open_position_on_start_qty
    }

    #[getter]
    #[pyo3(name = "open_position_on_first_quote")]
    const fn py_open_position_on_first_quote(&self) -> bool {
        self.open_position_on_first_quote
    }

    #[getter]
    #[pyo3(name = "open_position_time_in_force")]
    const fn py_open_position_time_in_force(&self) -> TimeInForce {
        self.open_position_time_in_force
    }

    #[getter]
    #[pyo3(name = "enable_limit_buys")]
    const fn py_enable_limit_buys(&self) -> bool {
        self.enable_limit_buys
    }

    #[getter]
    #[pyo3(name = "enable_limit_sells")]
    const fn py_enable_limit_sells(&self) -> bool {
        self.enable_limit_sells
    }

    #[getter]
    #[pyo3(name = "enable_stop_buys")]
    const fn py_enable_stop_buys(&self) -> bool {
        self.enable_stop_buys
    }

    #[getter]
    #[pyo3(name = "enable_stop_sells")]
    const fn py_enable_stop_sells(&self) -> bool {
        self.enable_stop_sells
    }

    #[getter]
    #[pyo3(name = "tob_offset_ticks")]
    const fn py_tob_offset_ticks(&self) -> u64 {
        self.tob_offset_ticks
    }

    #[getter]
    #[pyo3(name = "limit_time_in_force")]
    const fn py_limit_time_in_force(&self) -> Option<TimeInForce> {
        self.limit_time_in_force
    }

    #[getter]
    #[pyo3(name = "stop_order_type")]
    const fn py_stop_order_type(&self) -> OrderType {
        self.stop_order_type
    }

    #[getter]
    #[pyo3(name = "stop_offset_ticks")]
    const fn py_stop_offset_ticks(&self) -> u64 {
        self.stop_offset_ticks
    }

    #[getter]
    #[pyo3(name = "stop_limit_offset_ticks")]
    const fn py_stop_limit_offset_ticks(&self) -> Option<u64> {
        self.stop_limit_offset_ticks
    }

    #[getter]
    #[pyo3(name = "stop_trigger_type")]
    const fn py_stop_trigger_type(&self) -> TriggerType {
        self.stop_trigger_type
    }

    #[getter]
    #[pyo3(name = "stop_time_in_force")]
    const fn py_stop_time_in_force(&self) -> Option<TimeInForce> {
        self.stop_time_in_force
    }

    #[getter]
    #[pyo3(name = "trailing_offset")]
    const fn py_trailing_offset(&self) -> Option<Decimal> {
        self.trailing_offset
    }

    #[getter]
    #[pyo3(name = "trailing_offset_type")]
    const fn py_trailing_offset_type(&self) -> TrailingOffsetType {
        self.trailing_offset_type
    }

    #[getter]
    #[pyo3(name = "enable_brackets")]
    const fn py_enable_brackets(&self) -> bool {
        self.enable_brackets
    }

    #[getter]
    #[pyo3(name = "batch_submit_limit_pair")]
    const fn py_batch_submit_limit_pair(&self) -> bool {
        self.batch_submit_limit_pair
    }

    #[getter]
    #[pyo3(name = "bracket_entry_order_type")]
    const fn py_bracket_entry_order_type(&self) -> OrderType {
        self.bracket_entry_order_type
    }

    #[getter]
    #[pyo3(name = "bracket_offset_ticks")]
    const fn py_bracket_offset_ticks(&self) -> u64 {
        self.bracket_offset_ticks
    }

    #[getter]
    #[pyo3(name = "modify_orders_to_maintain_tob_offset")]
    const fn py_modify_orders_to_maintain_tob_offset(&self) -> bool {
        self.modify_orders_to_maintain_tob_offset
    }

    #[getter]
    #[pyo3(name = "modify_stop_orders_to_maintain_offset")]
    const fn py_modify_stop_orders_to_maintain_offset(&self) -> bool {
        self.modify_stop_orders_to_maintain_offset
    }

    #[getter]
    #[pyo3(name = "cancel_replace_orders_to_maintain_tob_offset")]
    const fn py_cancel_replace_orders_to_maintain_tob_offset(&self) -> bool {
        self.cancel_replace_orders_to_maintain_tob_offset
    }

    #[getter]
    #[pyo3(name = "cancel_replace_stop_orders_to_maintain_offset")]
    const fn py_cancel_replace_stop_orders_to_maintain_offset(&self) -> bool {
        self.cancel_replace_stop_orders_to_maintain_offset
    }

    #[getter]
    #[pyo3(name = "trigger_limit_order_maintenance_once")]
    const fn py_trigger_limit_order_maintenance_once(&self) -> bool {
        self.trigger_limit_order_maintenance_once
    }

    #[getter]
    #[pyo3(name = "use_post_only")]
    const fn py_use_post_only(&self) -> bool {
        self.use_post_only
    }

    #[getter]
    #[pyo3(name = "limit_aggressive")]
    const fn py_limit_aggressive(&self) -> bool {
        self.limit_aggressive
    }

    #[getter]
    #[pyo3(name = "use_quote_quantity")]
    const fn py_use_quote_quantity(&self) -> bool {
        self.use_quote_quantity
    }

    #[getter]
    #[pyo3(name = "emulation_trigger")]
    const fn py_emulation_trigger(&self) -> Option<TriggerType> {
        self.emulation_trigger
    }

    #[getter]
    #[pyo3(name = "use_individual_cancels_on_stop")]
    const fn py_use_individual_cancels_on_stop(&self) -> bool {
        self.use_individual_cancels_on_stop
    }

    #[getter]
    #[pyo3(name = "cancel_orders_on_stop")]
    const fn py_cancel_orders_on_stop(&self) -> bool {
        self.cancel_orders_on_stop
    }

    #[getter]
    #[pyo3(name = "close_positions_on_stop")]
    const fn py_close_positions_on_stop(&self) -> bool {
        self.close_positions_on_stop
    }

    #[getter]
    #[pyo3(name = "close_positions_qty_precision")]
    const fn py_close_positions_qty_precision(&self) -> Option<u8> {
        self.close_positions_qty_precision
    }

    #[getter]
    #[pyo3(name = "close_positions_time_in_force")]
    const fn py_close_positions_time_in_force(&self) -> Option<TimeInForce> {
        self.close_positions_time_in_force
    }

    #[getter]
    #[pyo3(name = "reduce_only_on_stop")]
    const fn py_reduce_only_on_stop(&self) -> bool {
        self.reduce_only_on_stop
    }

    #[getter]
    #[pyo3(name = "use_batch_cancel_on_stop")]
    const fn py_use_batch_cancel_on_stop(&self) -> bool {
        self.use_batch_cancel_on_stop
    }

    #[getter]
    #[pyo3(name = "dry_run")]
    const fn py_dry_run(&self) -> bool {
        self.dry_run
    }

    #[getter]
    #[pyo3(name = "log_data")]
    const fn py_log_data(&self) -> bool {
        self.log_data
    }

    #[getter]
    #[pyo3(name = "test_reject_post_only")]
    const fn py_test_reject_post_only(&self) -> bool {
        self.test_reject_post_only
    }

    #[getter]
    #[pyo3(name = "test_reject_reduce_only")]
    const fn py_test_reject_reduce_only(&self) -> bool {
        self.test_reject_reduce_only
    }

    #[getter]
    #[pyo3(name = "test_modify_rejected")]
    const fn py_test_modify_rejected(&self) -> bool {
        self.test_modify_rejected
    }

    #[getter]
    #[pyo3(name = "can_unsubscribe")]
    const fn py_can_unsubscribe(&self) -> bool {
        self.can_unsubscribe
    }

    #[getter]
    #[pyo3(name = "clamp_to_instrument_price_range")]
    const fn py_clamp_to_instrument_price_range(&self) -> bool {
        self.clamp_to_instrument_price_range
    }

    #[getter]
    #[pyo3(name = "log_events")]
    const fn py_log_events(&self) -> bool {
        self.base.log_events
    }

    #[getter]
    #[pyo3(name = "log_commands")]
    const fn py_log_commands(&self) -> bool {
        self.base.log_commands
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }
}
