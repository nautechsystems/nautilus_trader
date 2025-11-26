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

//! Test actors for live testing and development.

use std::{
    num::NonZeroUsize,
    ops::{Deref, DerefMut},
    time::Duration,
};

use ahash::AHashMap;
use nautilus_common::{
    actor::{DataActor, DataActorConfig, DataActorCore},
    enums::LogColor,
    log_info,
    timer::TimeEvent,
};
use nautilus_model::{
    data::{
        Bar, FundingRateUpdate, IndexPriceUpdate, InstrumentClose, InstrumentStatus,
        MarkPriceUpdate, OrderBookDeltas, QuoteTick, TradeTick, bar::BarType,
    },
    enums::BookType,
    identifiers::{ClientId, InstrumentId},
    instruments::InstrumentAny,
    orderbook::OrderBook,
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
    // TODO: Support subscribe_params when we have a type-safe way to pass arbitrary params
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
    // TODO: Support request_bars when historical data requests are available
    /// Whether to request historical bars (not yet implemented).
    pub request_bars: bool,
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
    /// For subscribing to quotes and trades on specified instruments.
    ///
    /// # Panics
    ///
    /// Panics if `NonZeroUsize::new(1000)` fails (which should never happen).
    #[must_use]
    pub fn new(
        client_id: ClientId,
        instrument_ids: Vec<InstrumentId>,
        subscribe_quotes: bool,
        subscribe_trades: bool,
    ) -> Self {
        Self {
            base: DataActorConfig::default(),
            instrument_ids,
            client_id: Some(client_id),
            bar_types: None,
            subscribe_book_deltas: false,
            subscribe_book_depth: false,
            subscribe_book_at_interval: false,
            subscribe_quotes,
            subscribe_trades,
            subscribe_mark_prices: false,
            subscribe_index_prices: false,
            subscribe_funding_rates: false,
            subscribe_bars: false,
            subscribe_instrument: false,
            subscribe_instrument_status: false,
            subscribe_instrument_close: false,
            can_unsubscribe: true,
            request_instruments: false,
            request_quotes: false,
            request_trades: false,
            request_bars: false,
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
            can_unsubscribe: true,
            request_instruments: false,
            request_quotes: false,
            request_trades: false,
            request_bars: false,
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

/// A data tester actor for live testing market data subscriptions.
///
/// Subscribes to configured data types for specified instruments and logs
/// received data to demonstrate the data flow. Useful for testing adapters
/// and validating data connectivity.
///
/// This actor provides equivalent functionality to the Python `DataTester`
/// in the test kit.
#[derive(Debug)]
pub struct DataTester {
    core: DataActorCore,
    config: DataTesterConfig,
    books: AHashMap<InstrumentId, OrderBook>,
}

impl Deref for DataTester {
    type Target = DataActorCore;

    fn deref(&self) -> &Self::Target {
        &self.core
    }
}

impl DerefMut for DataTester {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.core
    }
}

impl DataActor for DataTester {
    fn on_start(&mut self) -> anyhow::Result<()> {
        let instrument_ids = self.config.instrument_ids.clone();
        let client_id = self.config.client_id;
        let stats_interval_secs = self.config.stats_interval_secs;

        // Request instruments if configured
        if self.config.request_instruments {
            let mut venues = std::collections::HashSet::new();
            for instrument_id in &instrument_ids {
                venues.insert(instrument_id.venue);
            }

            for venue in venues {
                let _ = self.request_instruments(Some(venue), None, None, client_id, None);
            }
        }

        // Subscribe to data for each instrument
        for instrument_id in instrument_ids {
            if self.config.subscribe_instrument {
                self.subscribe_instrument(instrument_id, client_id, None);
            }

            if self.config.subscribe_book_deltas {
                self.subscribe_book_deltas(
                    instrument_id,
                    self.config.book_type,
                    None,
                    client_id,
                    self.config.manage_book,
                    None,
                );

                if self.config.manage_book {
                    let book = OrderBook::new(instrument_id, self.config.book_type);
                    self.books.insert(instrument_id, book);
                }
            }

            if self.config.subscribe_book_at_interval {
                self.subscribe_book_at_interval(
                    instrument_id,
                    self.config.book_type,
                    self.config.book_depth,
                    self.config.book_interval_ms,
                    client_id,
                    None,
                );
            }

            // TODO: Support subscribe_book_depth when the method is available
            // if self.config.subscribe_book_depth {
            //     self.subscribe_book_depth(
            //         instrument_id,
            //         self.config.book_type,
            //         self.config.book_depth,
            //         client_id,
            //         None,
            //     );
            // }

            if self.config.subscribe_quotes {
                self.subscribe_quotes(instrument_id, client_id, None);
            }

            if self.config.subscribe_trades {
                self.subscribe_trades(instrument_id, client_id, None);
            }

            if self.config.subscribe_mark_prices {
                self.subscribe_mark_prices(instrument_id, client_id, None);
            }

            if self.config.subscribe_index_prices {
                self.subscribe_index_prices(instrument_id, client_id, None);
            }

            if self.config.subscribe_funding_rates {
                self.subscribe_funding_rates(instrument_id, client_id, None);
            }

            if self.config.subscribe_instrument_status {
                self.subscribe_instrument_status(instrument_id, client_id, None);
            }

            if self.config.subscribe_instrument_close {
                self.subscribe_instrument_close(instrument_id, client_id, None);
            }

            // TODO: Implement historical data requests
            // if self.config.request_quotes {
            //     self.request_quote_ticks(...);
            // }

            // TODO: Implement historical data requests
            // if self.config.request_trades {
            //     self.request_trade_ticks(...);
            // }
        }

        // Subscribe to bars
        if let Some(bar_types) = self.config.bar_types.clone() {
            for bar_type in bar_types {
                if self.config.subscribe_bars {
                    self.subscribe_bars(bar_type, client_id, None);
                }

                // TODO: Implement historical data requests
                // if self.config.request_bars {
                //     self.request_bars(...);
                // }
            }
        }

        // Set up stats timer
        if stats_interval_secs > 0 {
            self.clock().set_timer(
                "STATS-TIMER",
                Duration::from_secs(stats_interval_secs),
                None,
                None,
                None,
                Some(true),
                Some(false),
            )?;
        }

        Ok(())
    }

    fn on_stop(&mut self) -> anyhow::Result<()> {
        if !self.config.can_unsubscribe {
            return Ok(());
        }

        let instrument_ids = self.config.instrument_ids.clone();
        let client_id = self.config.client_id;

        for instrument_id in instrument_ids {
            if self.config.subscribe_instrument {
                self.unsubscribe_instrument(instrument_id, client_id, None);
            }

            if self.config.subscribe_book_deltas {
                self.unsubscribe_book_deltas(instrument_id, client_id, None);
            }

            if self.config.subscribe_book_at_interval {
                self.unsubscribe_book_at_interval(
                    instrument_id,
                    self.config.book_interval_ms,
                    client_id,
                    None,
                );
            }

            // TODO: Support unsubscribe_book_depth when the method is available
            // if self.config.subscribe_book_depth {
            //     self.unsubscribe_book_depth(instrument_id, client_id, None);
            // }

            if self.config.subscribe_quotes {
                self.unsubscribe_quotes(instrument_id, client_id, None);
            }

            if self.config.subscribe_trades {
                self.unsubscribe_trades(instrument_id, client_id, None);
            }

            if self.config.subscribe_mark_prices {
                self.unsubscribe_mark_prices(instrument_id, client_id, None);
            }

            if self.config.subscribe_index_prices {
                self.unsubscribe_index_prices(instrument_id, client_id, None);
            }

            if self.config.subscribe_funding_rates {
                self.unsubscribe_funding_rates(instrument_id, client_id, None);
            }

            if self.config.subscribe_instrument_status {
                self.unsubscribe_instrument_status(instrument_id, client_id, None);
            }

            if self.config.subscribe_instrument_close {
                self.unsubscribe_instrument_close(instrument_id, client_id, None);
            }
        }

        if let Some(bar_types) = self.config.bar_types.clone() {
            for bar_type in bar_types {
                if self.config.subscribe_bars {
                    self.unsubscribe_bars(bar_type, client_id, None);
                }
            }
        }

        Ok(())
    }

    fn on_time_event(&mut self, _event: &TimeEvent) -> anyhow::Result<()> {
        // Timer events are used by the actor but don't require specific handling
        Ok(())
    }

    fn on_instrument(&mut self, instrument: &InstrumentAny) -> anyhow::Result<()> {
        if self.config.log_data {
            log_info!("Received {instrument:?}", color = LogColor::Cyan);
        }
        Ok(())
    }

    fn on_book_deltas(&mut self, deltas: &OrderBookDeltas) -> anyhow::Result<()> {
        if self.config.manage_book {
            if let Some(book) = self.books.get_mut(&deltas.instrument_id) {
                book.apply_deltas(deltas)?;

                if self.config.log_data {
                    let levels = self.config.book_levels_to_print;
                    let instrument_id = deltas.instrument_id;
                    let book_str = book.pprint(levels, None);
                    log_info!("\n{instrument_id}\n{book_str}", color = LogColor::Cyan);
                }
            }
        } else if self.config.log_data {
            log_info!("Received {deltas:?}", color = LogColor::Cyan);
        }
        Ok(())
    }

    fn on_quote(&mut self, quote: &QuoteTick) -> anyhow::Result<()> {
        if self.config.log_data {
            log_info!("Received {quote:?}", color = LogColor::Cyan);
        }
        Ok(())
    }

    fn on_trade(&mut self, trade: &TradeTick) -> anyhow::Result<()> {
        if self.config.log_data {
            log_info!("Received {trade:?}", color = LogColor::Cyan);
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

    fn on_funding_rate(&mut self, funding_rate: &FundingRateUpdate) -> anyhow::Result<()> {
        if self.config.log_data {
            log_info!("Received {funding_rate:?}", color = LogColor::Cyan);
        }
        Ok(())
    }

    fn on_instrument_status(&mut self, data: &InstrumentStatus) -> anyhow::Result<()> {
        if self.config.log_data {
            log_info!("Received {data:?}", color = LogColor::Cyan);
        }
        Ok(())
    }

    fn on_instrument_close(&mut self, update: &InstrumentClose) -> anyhow::Result<()> {
        if self.config.log_data {
            log_info!("Received {update:?}", color = LogColor::Cyan);
        }
        Ok(())
    }
}

impl DataTester {
    /// Creates a new [`DataTester`] instance.
    #[must_use]
    pub fn new(config: DataTesterConfig) -> Self {
        Self {
            core: DataActorCore::new(config.base.clone()),
            config,
            books: AHashMap::new(),
        }
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use nautilus_core::UnixNanos;
    use nautilus_model::{
        data::OrderBookDelta,
        enums::{InstrumentCloseType, MarketStatusAction},
        identifiers::Symbol,
        instruments::CurrencyPair,
        types::{Currency, Price, Quantity},
    };
    use rstest::*;
    use rust_decimal::Decimal;

    use super::*;

    #[fixture]
    fn config() -> DataTesterConfig {
        let client_id = ClientId::new("TEST");
        let instrument_ids = vec![
            InstrumentId::from("BTC-USDT.TEST"),
            InstrumentId::from("ETH-USDT.TEST"),
        ];
        DataTesterConfig::new(client_id, instrument_ids, true, true)
    }

    #[rstest]
    fn test_config_creation() {
        let client_id = ClientId::new("TEST");
        let instrument_ids = vec![InstrumentId::from("BTC-USDT.TEST")];
        let config = DataTesterConfig::new(client_id, instrument_ids.clone(), true, false);

        assert_eq!(config.client_id, Some(client_id));
        assert_eq!(config.instrument_ids, instrument_ids);
        assert!(config.subscribe_quotes);
        assert!(!config.subscribe_trades);
        assert!(config.log_data);
        assert_eq!(config.stats_interval_secs, 5);
    }

    #[rstest]
    fn test_config_default() {
        let config = DataTesterConfig::default();

        assert_eq!(config.client_id, None);
        assert!(config.instrument_ids.is_empty());
        assert!(!config.subscribe_quotes);
        assert!(!config.subscribe_trades);
        assert!(!config.subscribe_bars);
        assert!(config.can_unsubscribe);
        assert!(config.log_data);
    }

    #[rstest]
    fn test_actor_creation(config: DataTesterConfig) {
        let actor = DataTester::new(config);

        assert_eq!(actor.config.client_id, Some(ClientId::new("TEST")));
        assert_eq!(actor.config.instrument_ids.len(), 2);
    }

    #[rstest]
    fn test_on_quote_with_logging_enabled(config: DataTesterConfig) {
        let mut actor = DataTester::new(config);

        let quote = QuoteTick::default();
        let result = actor.on_quote(&quote);

        assert!(result.is_ok());
    }

    #[rstest]
    fn test_on_quote_with_logging_disabled(mut config: DataTesterConfig) {
        config.log_data = false;
        let mut actor = DataTester::new(config);

        let quote = QuoteTick::default();
        let result = actor.on_quote(&quote);

        assert!(result.is_ok());
    }

    #[rstest]
    fn test_on_trade(config: DataTesterConfig) {
        let mut actor = DataTester::new(config);

        let trade = TradeTick::default();
        let result = actor.on_trade(&trade);

        assert!(result.is_ok());
    }

    #[rstest]
    fn test_on_bar(config: DataTesterConfig) {
        let mut actor = DataTester::new(config);

        let bar = Bar::default();
        let result = actor.on_bar(&bar);

        assert!(result.is_ok());
    }

    #[rstest]
    fn test_on_instrument(config: DataTesterConfig) {
        let mut actor = DataTester::new(config);

        let instrument_id = InstrumentId::from("BTC-USDT.TEST");
        let instrument = CurrencyPair::new(
            instrument_id,
            Symbol::from("BTC/USDT"),
            Currency::USD(),
            Currency::USD(),
            4,
            3,
            Price::from("0.0001"),
            Quantity::from("0.001"),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            UnixNanos::default(),
            UnixNanos::default(),
        );
        let result = actor.on_instrument(&InstrumentAny::CurrencyPair(instrument));

        assert!(result.is_ok());
    }

    #[rstest]
    fn test_on_book_deltas_without_managed_book(config: DataTesterConfig) {
        let mut actor = DataTester::new(config);

        let instrument_id = InstrumentId::from("BTC-USDT.TEST");
        let delta =
            OrderBookDelta::clear(instrument_id, 0, UnixNanos::default(), UnixNanos::default());
        let deltas = OrderBookDeltas::new(instrument_id, vec![delta]);
        let result = actor.on_book_deltas(&deltas);

        assert!(result.is_ok());
    }

    #[rstest]
    fn test_on_mark_price(config: DataTesterConfig) {
        let mut actor = DataTester::new(config);

        let instrument_id = InstrumentId::from("BTC-USDT.TEST");
        let price = Price::from("50000.0");
        let mark_price = MarkPriceUpdate::new(
            instrument_id,
            price,
            UnixNanos::default(),
            UnixNanos::default(),
        );
        let result = actor.on_mark_price(&mark_price);

        assert!(result.is_ok());
    }

    #[rstest]
    fn test_on_index_price(config: DataTesterConfig) {
        let mut actor = DataTester::new(config);

        let instrument_id = InstrumentId::from("BTC-USDT.TEST");
        let price = Price::from("50000.0");
        let index_price = IndexPriceUpdate::new(
            instrument_id,
            price,
            UnixNanos::default(),
            UnixNanos::default(),
        );
        let result = actor.on_index_price(&index_price);

        assert!(result.is_ok());
    }

    #[rstest]
    fn test_on_funding_rate(config: DataTesterConfig) {
        let mut actor = DataTester::new(config);

        let instrument_id = InstrumentId::from("BTC-USDT.TEST");
        let funding_rate = FundingRateUpdate::new(
            instrument_id,
            Decimal::new(1, 4),
            None,
            UnixNanos::default(),
            UnixNanos::default(),
        );
        let result = actor.on_funding_rate(&funding_rate);

        assert!(result.is_ok());
    }

    #[rstest]
    fn test_on_instrument_status(config: DataTesterConfig) {
        let mut actor = DataTester::new(config);

        let instrument_id = InstrumentId::from("BTC-USDT.TEST");
        let status = InstrumentStatus::new(
            instrument_id,
            MarketStatusAction::Trading,
            UnixNanos::default(),
            UnixNanos::default(),
            None,
            None,
            None,
            None,
            None,
        );
        let result = actor.on_instrument_status(&status);

        assert!(result.is_ok());
    }

    #[rstest]
    fn test_on_instrument_close(config: DataTesterConfig) {
        let mut actor = DataTester::new(config);

        let instrument_id = InstrumentId::from("BTC-USDT.TEST");
        let price = Price::from("50000.0");
        let close = InstrumentClose::new(
            instrument_id,
            price,
            InstrumentCloseType::EndOfSession,
            UnixNanos::default(),
            UnixNanos::default(),
        );
        let result = actor.on_instrument_close(&close);

        assert!(result.is_ok());
    }

    #[rstest]
    fn test_on_time_event(config: DataTesterConfig) {
        let mut actor = DataTester::new(config);

        let event = TimeEvent::new(
            "TEST".into(),
            Default::default(),
            UnixNanos::default(),
            UnixNanos::default(),
        );
        let result = actor.on_time_event(&event);

        assert!(result.is_ok());
    }

    #[rstest]
    fn test_config_with_all_subscriptions_enabled(mut config: DataTesterConfig) {
        config.subscribe_book_deltas = true;
        config.subscribe_book_at_interval = true;
        config.subscribe_bars = true;
        config.subscribe_mark_prices = true;
        config.subscribe_index_prices = true;
        config.subscribe_funding_rates = true;
        config.subscribe_instrument = true;
        config.subscribe_instrument_status = true;
        config.subscribe_instrument_close = true;

        let actor = DataTester::new(config);

        assert!(actor.config.subscribe_book_deltas);
        assert!(actor.config.subscribe_book_at_interval);
        assert!(actor.config.subscribe_bars);
        assert!(actor.config.subscribe_mark_prices);
        assert!(actor.config.subscribe_index_prices);
        assert!(actor.config.subscribe_funding_rates);
        assert!(actor.config.subscribe_instrument);
        assert!(actor.config.subscribe_instrument_status);
        assert!(actor.config.subscribe_instrument_close);
    }

    #[rstest]
    fn test_config_with_book_management(mut config: DataTesterConfig) {
        config.manage_book = true;
        config.book_levels_to_print = 5;

        let actor = DataTester::new(config);

        assert!(actor.config.manage_book);
        assert_eq!(actor.config.book_levels_to_print, 5);
        assert!(actor.books.is_empty());
    }

    #[rstest]
    fn test_config_with_custom_stats_interval(mut config: DataTesterConfig) {
        config.stats_interval_secs = 10;

        let actor = DataTester::new(config);

        assert_eq!(actor.config.stats_interval_secs, 10);
    }

    #[rstest]
    fn test_config_with_unsubscribe_disabled(mut config: DataTesterConfig) {
        config.can_unsubscribe = false;

        let actor = DataTester::new(config);

        assert!(!actor.config.can_unsubscribe);
    }
}
