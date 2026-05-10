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

use std::time::Duration;

use ahash::{AHashMap, AHashSet};
use chrono::Duration as ChronoDuration;
use nautilus_common::{
    actor::{DataActor, DataActorCore},
    enums::LogColor,
    log_info, nautilus_actor,
    timer::TimeEvent,
};
use nautilus_model::{
    data::{
        Bar, FundingRateUpdate, IndexPriceUpdate, InstrumentClose, InstrumentStatus,
        MarkPriceUpdate, OrderBookDeltas, QuoteTick, TradeTick, option_chain::OptionGreeks,
    },
    identifiers::InstrumentId,
    instruments::InstrumentAny,
    orderbook::OrderBook,
};

use super::config::DataTesterConfig;

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
    pub(super) core: DataActorCore,
    pub(super) config: DataTesterConfig,
    pub(super) books: AHashMap<InstrumentId, OrderBook>,
}

nautilus_actor!(DataTester);

impl DataActor for DataTester {
    fn on_start(&mut self) -> anyhow::Result<()> {
        let instrument_ids = self.config.instrument_ids.clone();
        let client_id = self.config.client_id;
        let subscribe_params = self.config.subscribe_params.clone();
        let request_params = self.config.request_params.clone();
        let stats_interval_secs = self.config.stats_interval_secs;

        // Request instruments if configured
        if self.config.request_instruments {
            let mut venues = AHashSet::new();
            for instrument_id in &instrument_ids {
                venues.insert(instrument_id.venue);
            }

            for venue in venues {
                let _ = self.request_instruments(
                    Some(venue),
                    None,
                    None,
                    client_id,
                    request_params.clone(),
                );
            }
        }

        // Subscribe to data for each instrument
        for instrument_id in instrument_ids {
            if self.config.subscribe_instrument {
                self.subscribe_instrument(instrument_id, client_id, subscribe_params.clone());
            }

            if self.config.subscribe_book_deltas {
                self.subscribe_book_deltas(
                    instrument_id,
                    self.config.book_type,
                    None,
                    client_id,
                    self.config.manage_book,
                    subscribe_params.clone(),
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
                    subscribe_params.clone(),
                );
            }

            // TODO: Support subscribe_book_depth when the method is available
            // if self.config.subscribe_book_depth {
            //     self.subscribe_book_depth(
            //         instrument_id,
            //         self.config.book_type,
            //         self.config.book_depth,
            //         client_id,
            //         subscribe_params.clone(),
            //     );
            // }

            if self.config.subscribe_quotes {
                self.subscribe_quotes(instrument_id, client_id, subscribe_params.clone());
            }

            if self.config.subscribe_trades {
                self.subscribe_trades(instrument_id, client_id, subscribe_params.clone());
            }

            if self.config.subscribe_mark_prices {
                self.subscribe_mark_prices(instrument_id, client_id, subscribe_params.clone());
            }

            if self.config.subscribe_index_prices {
                self.subscribe_index_prices(instrument_id, client_id, subscribe_params.clone());
            }

            if self.config.subscribe_funding_rates {
                self.subscribe_funding_rates(instrument_id, client_id, subscribe_params.clone());
            }

            if self.config.subscribe_instrument_status {
                self.subscribe_instrument_status(
                    instrument_id,
                    client_id,
                    subscribe_params.clone(),
                );
            }

            if self.config.subscribe_instrument_close {
                self.subscribe_instrument_close(instrument_id, client_id, subscribe_params.clone());
            }

            if self.config.subscribe_option_greeks {
                self.subscribe_option_greeks(instrument_id, client_id, subscribe_params.clone());
            }

            // TODO: Implement historical data requests
            // if self.config.request_quotes {
            //     self.request_quote_ticks(...);
            // }

            // Request order book snapshot if configured
            if self.config.request_book_snapshot {
                let _ = self.request_book_snapshot(
                    instrument_id,
                    self.config.book_depth,
                    client_id,
                    request_params.clone(),
                );
            }

            // TODO: Request book deltas when Rust data engine has RequestBookDeltas

            // Request historical trades (default to last 1 hour)
            if self.config.request_trades {
                let start = self.clock().utc_now() - ChronoDuration::hours(1);

                if let Err(e) = self.request_trades(
                    instrument_id,
                    Some(start),
                    None,
                    None,
                    client_id,
                    request_params.clone(),
                ) {
                    log::error!("Failed to request trades for {instrument_id}: {e}");
                }
            }

            // Request historical funding rates (default to last 7 days)
            if self.config.request_funding_rates {
                let start = self.clock().utc_now() - ChronoDuration::days(7);

                if let Err(e) = self.request_funding_rates(
                    instrument_id,
                    Some(start),
                    None,
                    None,
                    client_id,
                    request_params.clone(),
                ) {
                    log::error!("Failed to request funding rates for {instrument_id}: {e}");
                }
            }
        }

        // Subscribe to bars
        if let Some(bar_types) = self.config.bar_types.clone() {
            for bar_type in bar_types {
                if self.config.subscribe_bars {
                    self.subscribe_bars(bar_type, client_id, subscribe_params.clone());
                }

                // Request historical bars (default to last 1 hour)
                if self.config.request_bars {
                    let start = self.clock().utc_now() - ChronoDuration::hours(1);

                    if let Err(e) = self.request_bars(
                        bar_type,
                        Some(start),
                        None,
                        None,
                        client_id,
                        request_params.clone(),
                    ) {
                        log::error!("Failed to request bars for {bar_type}: {e}");
                    }
                }
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
        let subscribe_params = self.config.subscribe_params.clone();

        for instrument_id in instrument_ids {
            if self.config.subscribe_instrument {
                self.unsubscribe_instrument(instrument_id, client_id, subscribe_params.clone());
            }

            if self.config.subscribe_book_deltas {
                self.unsubscribe_book_deltas(instrument_id, client_id, subscribe_params.clone());
            }

            if self.config.subscribe_book_at_interval {
                self.unsubscribe_book_at_interval(
                    instrument_id,
                    self.config.book_interval_ms,
                    client_id,
                    subscribe_params.clone(),
                );
            }

            // TODO: Support unsubscribe_book_depth when the method is available
            // if self.config.subscribe_book_depth {
            //     self.unsubscribe_book_depth(instrument_id, client_id, subscribe_params.clone());
            // }

            if self.config.subscribe_quotes {
                self.unsubscribe_quotes(instrument_id, client_id, subscribe_params.clone());
            }

            if self.config.subscribe_trades {
                self.unsubscribe_trades(instrument_id, client_id, subscribe_params.clone());
            }

            if self.config.subscribe_mark_prices {
                self.unsubscribe_mark_prices(instrument_id, client_id, subscribe_params.clone());
            }

            if self.config.subscribe_index_prices {
                self.unsubscribe_index_prices(instrument_id, client_id, subscribe_params.clone());
            }

            if self.config.subscribe_funding_rates {
                self.unsubscribe_funding_rates(instrument_id, client_id, subscribe_params.clone());
            }

            if self.config.subscribe_instrument_status {
                self.unsubscribe_instrument_status(
                    instrument_id,
                    client_id,
                    subscribe_params.clone(),
                );
            }

            if self.config.subscribe_instrument_close {
                self.unsubscribe_instrument_close(
                    instrument_id,
                    client_id,
                    subscribe_params.clone(),
                );
            }

            if self.config.subscribe_option_greeks {
                self.unsubscribe_option_greeks(instrument_id, client_id, subscribe_params.clone());
            }
        }

        if let Some(bar_types) = self.config.bar_types.clone() {
            for bar_type in bar_types {
                if self.config.subscribe_bars {
                    self.unsubscribe_bars(bar_type, client_id, subscribe_params.clone());
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
            log_info!("{instrument:?}", color = LogColor::Cyan);
        }
        Ok(())
    }

    fn on_book(&mut self, book: &OrderBook) -> anyhow::Result<()> {
        if self.config.log_data {
            let levels = self.config.book_levels_to_print;
            let instrument_id = book.instrument_id;
            let book_str = book.pprint(levels, None);
            log_info!("\n{instrument_id}\n{book_str}", color = LogColor::Cyan);
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
            log_info!("{deltas:?}", color = LogColor::Cyan);
        }
        Ok(())
    }

    fn on_quote(&mut self, quote: &QuoteTick) -> anyhow::Result<()> {
        if self.config.log_data {
            log_info!("{quote:?}", color = LogColor::Cyan);
        }
        Ok(())
    }

    fn on_trade(&mut self, trade: &TradeTick) -> anyhow::Result<()> {
        if self.config.log_data {
            log_info!("{trade:?}", color = LogColor::Cyan);
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

    fn on_funding_rate(&mut self, funding_rate: &FundingRateUpdate) -> anyhow::Result<()> {
        if self.config.log_data {
            log_info!("{funding_rate:?}", color = LogColor::Cyan);
        }
        Ok(())
    }

    fn on_instrument_status(&mut self, data: &InstrumentStatus) -> anyhow::Result<()> {
        if self.config.log_data {
            log_info!("{data:?}", color = LogColor::Cyan);
        }
        Ok(())
    }

    fn on_instrument_close(&mut self, update: &InstrumentClose) -> anyhow::Result<()> {
        if self.config.log_data {
            log_info!("{update:?}", color = LogColor::Cyan);
        }
        Ok(())
    }

    fn on_option_greeks(&mut self, greeks: &OptionGreeks) -> anyhow::Result<()> {
        if self.config.log_data {
            log_info!("{greeks:?}", color = LogColor::Cyan);
        }
        Ok(())
    }

    fn on_historical_trades(&mut self, trades: &[TradeTick]) -> anyhow::Result<()> {
        if self.config.log_data {
            log_info!(
                "Received {} historical trades",
                trades.len(),
                color = LogColor::Cyan
            );

            for trade in trades.iter().take(5) {
                log_info!("  {trade:?}", color = LogColor::Cyan);
            }

            if trades.len() > 5 {
                log_info!(
                    "  ... and {} more trades",
                    trades.len() - 5,
                    color = LogColor::Cyan
                );
            }
        }
        Ok(())
    }

    fn on_historical_funding_rates(
        &mut self,
        funding_rates: &[FundingRateUpdate],
    ) -> anyhow::Result<()> {
        if self.config.log_data {
            log_info!(
                "Received {} historical funding rates",
                funding_rates.len(),
                color = LogColor::Cyan
            );

            for rate in funding_rates.iter().take(5) {
                log_info!("  {rate:?}", color = LogColor::Cyan);
            }

            if funding_rates.len() > 5 {
                log_info!(
                    "  ... and {} more funding rates",
                    funding_rates.len() - 5,
                    color = LogColor::Cyan
                );
            }
        }
        Ok(())
    }

    fn on_historical_bars(&mut self, bars: &[Bar]) -> anyhow::Result<()> {
        if self.config.log_data {
            log_info!(
                "Received {} historical bars",
                bars.len(),
                color = LogColor::Cyan
            );

            for bar in bars.iter().take(5) {
                log_info!("  {bar:?}", color = LogColor::Cyan);
            }

            if bars.len() > 5 {
                log_info!(
                    "  ... and {} more bars",
                    bars.len() - 5,
                    color = LogColor::Cyan
                );
            }
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
