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

use nautilus_common::actor::DataActorConfig;
use nautilus_core::Params;
use nautilus_model::{
    data::bar::BarType,
    enums::BookType,
    identifiers::{ClientId, InstrumentId},
};

/// Configuration for the data tester actor.
#[derive(Debug, Clone)]
pub struct DataTesterConfig {
    /// Base data actor configuration.
    pub base: DataActorConfig,
    /// Instrument IDs to subscribe to.
    pub instrument_ids: Vec<InstrumentId>,
    /// Client ID to use for subscriptions.
    pub client_id: Option<ClientId>,
    /// Bar types to subscribe to.
    pub bar_types: Option<Vec<BarType>>,
    /// Whether to subscribe to order book deltas.
    pub subscribe_book_deltas: bool,
    /// Whether to subscribe to order book depth snapshots.
    pub subscribe_book_depth: bool,
    /// Whether to subscribe to order book at interval.
    pub subscribe_book_at_interval: bool,
    /// Whether to subscribe to quotes.
    pub subscribe_quotes: bool,
    /// Whether to subscribe to trades.
    pub subscribe_trades: bool,
    /// Whether to subscribe to mark prices.
    pub subscribe_mark_prices: bool,
    /// Whether to subscribe to index prices.
    pub subscribe_index_prices: bool,
    /// Whether to subscribe to funding rates.
    pub subscribe_funding_rates: bool,
    /// Whether to subscribe to bars.
    pub subscribe_bars: bool,
    /// Whether to subscribe to instrument updates.
    pub subscribe_instrument: bool,
    /// Whether to subscribe to instrument status.
    pub subscribe_instrument_status: bool,
    /// Whether to subscribe to instrument close.
    pub subscribe_instrument_close: bool,
    /// Whether to subscribe to option greeks.
    pub subscribe_option_greeks: bool,
    /// Optional parameters passed to all subscribe calls.
    pub subscribe_params: Option<Params>,
    /// Optional parameters passed to all request calls.
    pub request_params: Option<Params>,
    /// Whether unsubscribe is supported on stop.
    pub can_unsubscribe: bool,
    /// Whether to request instruments on start.
    pub request_instruments: bool,
    // TODO: Support request_quotes when historical data requests are available
    /// Whether to request historical quotes (not yet implemented).
    pub request_quotes: bool,
    // TODO: Support request_trades when historical data requests are available
    /// Whether to request historical trades (not yet implemented).
    pub request_trades: bool,
    /// Whether to request historical bars.
    pub request_bars: bool,
    /// Whether to request order book snapshots.
    pub request_book_snapshot: bool,
    // TODO: Support request_book_deltas when Rust data engine has RequestBookDeltas
    /// Whether to request historical order book deltas (not yet implemented).
    pub request_book_deltas: bool,
    /// Whether to request historical funding rates.
    pub request_funding_rates: bool,
    // TODO: Support requests_start_delta when we implement historical data requests
    /// Book type for order book subscriptions.
    pub book_type: BookType,
    /// Order book depth for subscriptions.
    pub book_depth: Option<NonZeroUsize>,
    // TODO: Support book_group_size when order book grouping is implemented
    /// Order book interval in milliseconds for at_interval subscriptions.
    pub book_interval_ms: NonZeroUsize,
    /// Number of order book levels to print when logging.
    pub book_levels_to_print: usize,
    /// Whether to manage local order book from deltas.
    pub manage_book: bool,
    /// Whether to log received data.
    pub log_data: bool,
    /// Stats logging interval in seconds (0 to disable).
    pub stats_interval_secs: u64,
}

impl DataTesterConfig {
    /// Creates a new [`DataTesterConfig`] instance with minimal settings.
    ///
    /// # Panics
    ///
    /// Panics if `NonZeroUsize::new(1000)` fails (which should never happen).
    #[must_use]
    pub fn new(client_id: ClientId, instrument_ids: Vec<InstrumentId>) -> Self {
        Self {
            base: DataActorConfig::default(),
            instrument_ids,
            client_id: Some(client_id),
            bar_types: None,
            subscribe_book_deltas: false,
            subscribe_book_depth: false,
            subscribe_book_at_interval: false,
            subscribe_quotes: false,
            subscribe_trades: false,
            subscribe_mark_prices: false,
            subscribe_index_prices: false,
            subscribe_funding_rates: false,
            subscribe_bars: false,

            subscribe_instrument: false,
            subscribe_instrument_status: false,
            subscribe_instrument_close: false,
            subscribe_option_greeks: false,
            subscribe_params: None,
            request_params: None,
            can_unsubscribe: true,
            request_instruments: false,
            request_quotes: false,
            request_trades: false,
            request_bars: false,
            request_book_snapshot: false,
            request_book_deltas: false,
            request_funding_rates: false,
            book_type: BookType::L2_MBP,
            book_depth: None,
            book_interval_ms: NonZeroUsize::new(1000).unwrap(),
            book_levels_to_print: 10,
            manage_book: true,
            log_data: true,
            stats_interval_secs: 5,
        }
    }

    #[must_use]
    pub fn with_log_data(mut self, log_data: bool) -> Self {
        self.log_data = log_data;
        self
    }

    #[must_use]
    pub fn with_subscribe_book_deltas(mut self, subscribe: bool) -> Self {
        self.subscribe_book_deltas = subscribe;
        self
    }

    #[must_use]
    pub fn with_subscribe_book_depth(mut self, subscribe: bool) -> Self {
        self.subscribe_book_depth = subscribe;
        self
    }

    #[must_use]
    pub fn with_subscribe_book_at_interval(mut self, subscribe: bool) -> Self {
        self.subscribe_book_at_interval = subscribe;
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
    pub fn with_subscribe_mark_prices(mut self, subscribe: bool) -> Self {
        self.subscribe_mark_prices = subscribe;
        self
    }

    #[must_use]
    pub fn with_subscribe_index_prices(mut self, subscribe: bool) -> Self {
        self.subscribe_index_prices = subscribe;
        self
    }

    #[must_use]
    pub fn with_subscribe_funding_rates(mut self, subscribe: bool) -> Self {
        self.subscribe_funding_rates = subscribe;
        self
    }

    #[must_use]
    pub fn with_subscribe_bars(mut self, subscribe: bool) -> Self {
        self.subscribe_bars = subscribe;
        self
    }

    #[must_use]
    pub fn with_bar_types(mut self, bar_types: Vec<BarType>) -> Self {
        self.bar_types = Some(bar_types);
        self
    }

    #[must_use]
    pub fn with_subscribe_instrument(mut self, subscribe: bool) -> Self {
        self.subscribe_instrument = subscribe;
        self
    }

    #[must_use]
    pub fn with_subscribe_instrument_status(mut self, subscribe: bool) -> Self {
        self.subscribe_instrument_status = subscribe;
        self
    }

    #[must_use]
    pub fn with_subscribe_instrument_close(mut self, subscribe: bool) -> Self {
        self.subscribe_instrument_close = subscribe;
        self
    }

    #[must_use]
    pub fn with_subscribe_option_greeks(mut self, subscribe: bool) -> Self {
        self.subscribe_option_greeks = subscribe;
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
    pub fn with_book_interval_ms(mut self, interval_ms: NonZeroUsize) -> Self {
        self.book_interval_ms = interval_ms;
        self
    }

    #[must_use]
    pub fn with_manage_book(mut self, manage: bool) -> Self {
        self.manage_book = manage;
        self
    }

    #[must_use]
    pub fn with_request_instruments(mut self, request: bool) -> Self {
        self.request_instruments = request;
        self
    }

    #[must_use]
    pub fn with_request_book_snapshot(mut self, request: bool) -> Self {
        self.request_book_snapshot = request;
        self
    }

    #[must_use]
    pub fn with_request_book_deltas(mut self, request: bool) -> Self {
        self.request_book_deltas = request;
        self
    }

    #[must_use]
    pub fn with_request_trades(mut self, request: bool) -> Self {
        self.request_trades = request;
        self
    }

    #[must_use]
    pub fn with_request_bars(mut self, request: bool) -> Self {
        self.request_bars = request;
        self
    }

    #[must_use]
    pub fn with_request_quotes(mut self, request: bool) -> Self {
        self.request_quotes = request;
        self
    }

    #[must_use]
    pub fn with_request_funding_rates(mut self, request: bool) -> Self {
        self.request_funding_rates = request;
        self
    }

    #[must_use]
    pub fn with_book_levels_to_print(mut self, levels: usize) -> Self {
        self.book_levels_to_print = levels;
        self
    }

    #[must_use]
    pub fn with_can_unsubscribe(mut self, can_unsubscribe: bool) -> Self {
        self.can_unsubscribe = can_unsubscribe;
        self
    }

    #[must_use]
    pub fn with_subscribe_params(mut self, params: Option<Params>) -> Self {
        self.subscribe_params = params;
        self
    }

    #[must_use]
    pub fn with_request_params(mut self, params: Option<Params>) -> Self {
        self.request_params = params;
        self
    }

    #[must_use]
    pub fn with_stats_interval_secs(mut self, interval_secs: u64) -> Self {
        self.stats_interval_secs = interval_secs;
        self
    }
}

impl Default for DataTesterConfig {
    fn default() -> Self {
        Self {
            base: DataActorConfig::default(),
            instrument_ids: Vec::new(),
            client_id: None,
            bar_types: None,
            subscribe_book_deltas: false,
            subscribe_book_depth: false,
            subscribe_book_at_interval: false,
            subscribe_quotes: false,
            subscribe_trades: false,
            subscribe_mark_prices: false,
            subscribe_index_prices: false,
            subscribe_funding_rates: false,
            subscribe_bars: false,
            subscribe_instrument: false,
            subscribe_instrument_status: false,
            subscribe_instrument_close: false,
            subscribe_option_greeks: false,
            subscribe_params: None,
            request_params: None,
            can_unsubscribe: true,
            request_instruments: false,
            request_quotes: false,
            request_trades: false,
            request_bars: false,
            request_book_snapshot: false,
            request_book_deltas: false,
            request_funding_rates: false,
            book_type: BookType::L2_MBP,
            book_depth: None,
            book_interval_ms: NonZeroUsize::new(1000).unwrap(),
            book_levels_to_print: 10,
            manage_book: false,
            log_data: true,
            stats_interval_secs: 5,
        }
    }
}
