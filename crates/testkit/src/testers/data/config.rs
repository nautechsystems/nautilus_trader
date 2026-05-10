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
use serde::{Deserialize, Serialize};

/// Configuration for the data tester actor.
#[derive(Debug, Clone, Deserialize, Serialize, bon::Builder)]
#[serde(default, deny_unknown_fields)]
pub struct DataTesterConfig {
    /// Base data actor configuration.
    #[builder(default)]
    pub base: DataActorConfig,
    /// Instrument IDs to subscribe to.
    #[builder(default)]
    pub instrument_ids: Vec<InstrumentId>,
    /// Client ID to use for subscriptions.
    pub client_id: Option<ClientId>,
    /// Bar types to subscribe to.
    pub bar_types: Option<Vec<BarType>>,
    /// Whether to subscribe to order book deltas.
    #[builder(default = false)]
    pub subscribe_book_deltas: bool,
    /// Whether to subscribe to order book depth snapshots.
    #[builder(default = false)]
    pub subscribe_book_depth: bool,
    /// Whether to subscribe to order book at interval.
    #[builder(default = false)]
    pub subscribe_book_at_interval: bool,
    /// Whether to subscribe to quotes.
    #[builder(default = false)]
    pub subscribe_quotes: bool,
    /// Whether to subscribe to trades.
    #[builder(default = false)]
    pub subscribe_trades: bool,
    /// Whether to subscribe to mark prices.
    #[builder(default = false)]
    pub subscribe_mark_prices: bool,
    /// Whether to subscribe to index prices.
    #[builder(default = false)]
    pub subscribe_index_prices: bool,
    /// Whether to subscribe to funding rates.
    #[builder(default = false)]
    pub subscribe_funding_rates: bool,
    /// Whether to subscribe to bars.
    #[builder(default = false)]
    pub subscribe_bars: bool,
    /// Whether to subscribe to instrument updates.
    #[builder(default = false)]
    pub subscribe_instrument: bool,
    /// Whether to subscribe to instrument status.
    #[builder(default = false)]
    pub subscribe_instrument_status: bool,
    /// Whether to subscribe to instrument close.
    #[builder(default = false)]
    pub subscribe_instrument_close: bool,
    /// Whether to subscribe to option greeks.
    #[builder(default = false)]
    pub subscribe_option_greeks: bool,
    /// Optional parameters passed to all subscribe calls.
    pub subscribe_params: Option<Params>,
    /// Optional parameters passed to all request calls.
    pub request_params: Option<Params>,
    /// Whether unsubscribe is supported on stop.
    #[builder(default = true)]
    pub can_unsubscribe: bool,
    /// Whether to request instruments on start.
    #[builder(default = false)]
    pub request_instruments: bool,
    // TODO: Support request_quotes when historical data requests are available
    /// Whether to request historical quotes (not yet implemented).
    #[builder(default = false)]
    pub request_quotes: bool,
    // TODO: Support request_trades when historical data requests are available
    /// Whether to request historical trades (not yet implemented).
    #[builder(default = false)]
    pub request_trades: bool,
    /// Whether to request historical bars.
    #[builder(default = false)]
    pub request_bars: bool,
    /// Whether to request order book snapshots.
    #[builder(default = false)]
    pub request_book_snapshot: bool,
    // TODO: Support request_book_deltas when Rust data engine has RequestBookDeltas
    /// Whether to request historical order book deltas (not yet implemented).
    #[builder(default = false)]
    pub request_book_deltas: bool,
    /// Whether to request historical funding rates.
    #[builder(default = false)]
    pub request_funding_rates: bool,
    // TODO: Support requests_start_delta when we implement historical data requests
    /// Book type for order book subscriptions.
    #[builder(default = BookType::L2_MBP)]
    pub book_type: BookType,
    /// Order book depth for subscriptions.
    pub book_depth: Option<NonZeroUsize>,
    // TODO: Support book_group_size when order book grouping is implemented
    /// Order book interval in milliseconds for at_interval subscriptions.
    #[builder(default = NonZeroUsize::new(1000).unwrap())]
    pub book_interval_ms: NonZeroUsize,
    /// Number of order book levels to print when logging.
    #[builder(default = 10)]
    pub book_levels_to_print: usize,
    /// Whether to manage local order book from deltas.
    #[builder(default = true)]
    pub manage_book: bool,
    /// Whether to log received data.
    #[builder(default = true)]
    pub log_data: bool,
    /// Stats logging interval in seconds (0 to disable).
    #[builder(default = 5)]
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
}

impl Default for DataTesterConfig {
    fn default() -> Self {
        Self::builder().build()
    }
}
