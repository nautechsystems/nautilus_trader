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

//! Provides a `BacktestDataClient` implementation for backtesting.

use std::{cell::RefCell, rc::Rc};

#[cfg(feature = "defi")]
use nautilus_common::messages::defi::{
    RequestPoolSnapshot, SubscribeBlocks, SubscribePool, SubscribePoolFeeCollects,
    SubscribePoolFlashEvents, SubscribePoolLiquidityUpdates, SubscribePoolSwaps, UnsubscribeBlocks,
    UnsubscribePool, UnsubscribePoolFeeCollects, UnsubscribePoolFlashEvents,
    UnsubscribePoolLiquidityUpdates, UnsubscribePoolSwaps,
};
use nautilus_common::{
    cache::Cache,
    clients::DataClient,
    messages::data::{
        RequestOptionChainReferencePrice, SubscribeBars, SubscribeBookDeltas, SubscribeBookDepth10,
        SubscribeCustomData, SubscribeIndexPrices, SubscribeInstrument, SubscribeInstrumentClose,
        SubscribeInstrumentStatus, SubscribeInstruments, SubscribeMarkPrices, SubscribeQuotes,
        SubscribeTrades, UnsubscribeBars, UnsubscribeBookDeltas, UnsubscribeBookDepth10,
        UnsubscribeCustomData, UnsubscribeIndexPrices, UnsubscribeInstrument,
        UnsubscribeInstrumentClose, UnsubscribeInstrumentStatus, UnsubscribeInstruments,
        UnsubscribeMarkPrices, UnsubscribeQuotes, UnsubscribeTrades,
    },
};
use nautilus_model::identifiers::{ClientId, Venue};

/// Data client implementation for backtesting market data operations.
///
/// The `BacktestDataClient` provides a data client interface specifically designed
/// for backtesting environments. It handles market data subscriptions and requests
/// during backtesting, coordinating with the backtesting engine to provide
/// historical data replay functionality.
#[derive(Debug)]
pub struct BacktestDataClient {
    pub client_id: ClientId,
    pub venue: Venue,
    _cache: Rc<RefCell<Cache>>,
}

impl BacktestDataClient {
    /// Creates a new [`BacktestDataClient`] instance.
    #[must_use]
    pub const fn new(client_id: ClientId, venue: Venue, cache: Rc<RefCell<Cache>>) -> Self {
        Self {
            client_id,
            venue,
            _cache: cache,
        }
    }
}

#[async_trait::async_trait(?Send)]
impl DataClient for BacktestDataClient {
    fn client_id(&self) -> ClientId {
        self.client_id
    }

    fn venue(&self) -> Option<Venue> {
        Some(self.venue)
    }

    fn start(&mut self) -> anyhow::Result<()> {
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        Ok(())
    }

    fn reset(&mut self) -> anyhow::Result<()> {
        Ok(())
    }

    fn dispose(&mut self) -> anyhow::Result<()> {
        Ok(())
    }

    fn is_connected(&self) -> bool {
        true
    }

    fn is_disconnected(&self) -> bool {
        false
    }

    fn subscribe(&mut self, _cmd: SubscribeCustomData) -> anyhow::Result<()> {
        Ok(())
    }

    fn subscribe_instruments(&mut self, _cmd: SubscribeInstruments) -> anyhow::Result<()> {
        Ok(())
    }

    fn subscribe_instrument(&mut self, _cmd: SubscribeInstrument) -> anyhow::Result<()> {
        Ok(())
    }

    fn subscribe_book_deltas(&mut self, _cmd: SubscribeBookDeltas) -> anyhow::Result<()> {
        Ok(())
    }

    fn subscribe_book_depth10(&mut self, _cmd: SubscribeBookDepth10) -> anyhow::Result<()> {
        Ok(())
    }

    fn subscribe_quotes(&mut self, _cmd: SubscribeQuotes) -> anyhow::Result<()> {
        Ok(())
    }

    fn subscribe_trades(&mut self, _cmd: SubscribeTrades) -> anyhow::Result<()> {
        Ok(())
    }

    fn subscribe_bars(&mut self, _cmd: SubscribeBars) -> anyhow::Result<()> {
        Ok(())
    }

    fn subscribe_mark_prices(&mut self, _cmd: SubscribeMarkPrices) -> anyhow::Result<()> {
        Ok(())
    }

    fn subscribe_index_prices(&mut self, _cmd: SubscribeIndexPrices) -> anyhow::Result<()> {
        Ok(())
    }

    fn subscribe_instrument_status(
        &mut self,
        _cmd: SubscribeInstrumentStatus,
    ) -> anyhow::Result<()> {
        Ok(())
    }

    fn subscribe_instrument_close(&mut self, _cmd: SubscribeInstrumentClose) -> anyhow::Result<()> {
        Ok(())
    }

    // DeFi subscriptions/requests are served by replayed data; these silent overrides of the
    // `DataClient` default stay here because a trait impl cannot be split across modules.
    #[cfg(feature = "defi")]
    fn subscribe_blocks(&mut self, _cmd: SubscribeBlocks) -> anyhow::Result<()> {
        Ok(())
    }

    #[cfg(feature = "defi")]
    fn subscribe_pool(&mut self, _cmd: SubscribePool) -> anyhow::Result<()> {
        Ok(())
    }

    #[cfg(feature = "defi")]
    fn subscribe_pool_swaps(&mut self, _cmd: SubscribePoolSwaps) -> anyhow::Result<()> {
        Ok(())
    }

    #[cfg(feature = "defi")]
    fn subscribe_pool_liquidity_updates(
        &mut self,
        _cmd: SubscribePoolLiquidityUpdates,
    ) -> anyhow::Result<()> {
        Ok(())
    }

    #[cfg(feature = "defi")]
    fn subscribe_pool_fee_collects(
        &mut self,
        _cmd: SubscribePoolFeeCollects,
    ) -> anyhow::Result<()> {
        Ok(())
    }

    #[cfg(feature = "defi")]
    fn subscribe_pool_flash_events(
        &mut self,
        _cmd: SubscribePoolFlashEvents,
    ) -> anyhow::Result<()> {
        Ok(())
    }

    fn unsubscribe(&mut self, _cmd: &UnsubscribeCustomData) -> anyhow::Result<()> {
        Ok(())
    }

    fn unsubscribe_instruments(&mut self, _cmd: &UnsubscribeInstruments) -> anyhow::Result<()> {
        Ok(())
    }

    fn unsubscribe_instrument(&mut self, _cmd: &UnsubscribeInstrument) -> anyhow::Result<()> {
        Ok(())
    }

    fn unsubscribe_book_deltas(&mut self, _cmd: &UnsubscribeBookDeltas) -> anyhow::Result<()> {
        Ok(())
    }

    fn unsubscribe_book_depth10(&mut self, _cmd: &UnsubscribeBookDepth10) -> anyhow::Result<()> {
        Ok(())
    }

    fn unsubscribe_quotes(&mut self, _cmd: &UnsubscribeQuotes) -> anyhow::Result<()> {
        Ok(())
    }

    fn unsubscribe_trades(&mut self, _cmd: &UnsubscribeTrades) -> anyhow::Result<()> {
        Ok(())
    }

    fn unsubscribe_bars(&mut self, _cmd: &UnsubscribeBars) -> anyhow::Result<()> {
        Ok(())
    }

    fn unsubscribe_mark_prices(&mut self, _cmd: &UnsubscribeMarkPrices) -> anyhow::Result<()> {
        Ok(())
    }

    fn unsubscribe_index_prices(&mut self, _cmd: &UnsubscribeIndexPrices) -> anyhow::Result<()> {
        Ok(())
    }

    fn unsubscribe_instrument_status(
        &mut self,
        _cmd: &UnsubscribeInstrumentStatus,
    ) -> anyhow::Result<()> {
        Ok(())
    }

    fn unsubscribe_instrument_close(
        &mut self,
        _cmd: &UnsubscribeInstrumentClose,
    ) -> anyhow::Result<()> {
        Ok(())
    }

    #[cfg(feature = "defi")]
    fn unsubscribe_blocks(&mut self, _cmd: &UnsubscribeBlocks) -> anyhow::Result<()> {
        Ok(())
    }

    #[cfg(feature = "defi")]
    fn unsubscribe_pool(&mut self, _cmd: &UnsubscribePool) -> anyhow::Result<()> {
        Ok(())
    }

    #[cfg(feature = "defi")]
    fn unsubscribe_pool_swaps(&mut self, _cmd: &UnsubscribePoolSwaps) -> anyhow::Result<()> {
        Ok(())
    }

    #[cfg(feature = "defi")]
    fn unsubscribe_pool_liquidity_updates(
        &mut self,
        _cmd: &UnsubscribePoolLiquidityUpdates,
    ) -> anyhow::Result<()> {
        Ok(())
    }

    #[cfg(feature = "defi")]
    fn unsubscribe_pool_fee_collects(
        &mut self,
        _cmd: &UnsubscribePoolFeeCollects,
    ) -> anyhow::Result<()> {
        Ok(())
    }

    #[cfg(feature = "defi")]
    fn unsubscribe_pool_flash_events(
        &mut self,
        _cmd: &UnsubscribePoolFlashEvents,
    ) -> anyhow::Result<()> {
        Ok(())
    }

    fn request_option_chain_reference_price(
        &self,
        _request: RequestOptionChainReferencePrice,
    ) -> anyhow::Result<()> {
        anyhow::bail!("backtest data client cannot fetch option-chain reference prices")
    }

    // Unlike the other request handlers, this stays silent: the engine itself issues this
    // request when a DeFi subscription arrives before the pool is cached, and the replayed
    // snapshot completes that flow. The default handler would warn during a successful backtest.
    #[cfg(feature = "defi")]
    fn request_pool_snapshot(&self, _request: RequestPoolSnapshot) -> anyhow::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Mutex, MutexGuard};

    use log::{Level, LevelFilter, Log, Metadata, Record};
    use nautilus_common::messages::data::RequestInstruments;
    use nautilus_core::{UUID4, UnixNanos};
    use nautilus_model::identifiers::{InstrumentId, OptionSeriesId};
    use rstest::rstest;
    use ustr::Ustr;

    use super::*;

    struct RequestWarnCapture {
        messages: Mutex<Vec<String>>,
    }

    impl RequestWarnCapture {
        fn clear(&self) {
            self.messages.lock().unwrap().clear();
        }

        fn messages(&self) -> Vec<String> {
            self.messages.lock().unwrap().clone()
        }
    }

    impl Log for RequestWarnCapture {
        fn enabled(&self, metadata: &Metadata<'_>) -> bool {
            metadata.level() == Level::Warn
        }

        fn log(&self, record: &Record<'_>) {
            if self.enabled(record.metadata()) {
                self.messages
                    .lock()
                    .unwrap()
                    .push(record.args().to_string());
            }
        }

        fn flush(&self) {}
    }

    static REQUEST_WARN_CAPTURE: RequestWarnCapture = RequestWarnCapture {
        messages: Mutex::new(Vec::new()),
    };
    static REQUEST_WARN_TEST_LOCK: Mutex<()> = Mutex::new(());

    fn start_request_warn_capture() -> MutexGuard<'static, ()> {
        let guard = REQUEST_WARN_TEST_LOCK.lock().unwrap();
        let _ = log::set_logger(&REQUEST_WARN_CAPTURE);
        log::set_max_level(LevelFilter::Warn);
        REQUEST_WARN_CAPTURE.clear();
        guard
    }

    #[rstest]
    fn test_request_instruments_logs_not_implemented_warning() {
        let _guard = start_request_warn_capture();

        let client_id = ClientId::new("BACKTEST");
        let venue = Venue::new("BACKTEST");
        let cache = Rc::new(RefCell::new(Cache::default()));
        let client = BacktestDataClient::new(client_id, venue, cache);

        let request = RequestInstruments::new(
            None,
            None,
            Some(client_id),
            Some(venue),
            UUID4::new(),
            UnixNanos::default(),
            None,
        );

        let result = client.request_instruments(request);

        assert!(result.is_ok());
        assert!(
            REQUEST_WARN_CAPTURE
                .messages()
                .iter()
                .any(|message| message.contains("RequestInstruments")
                    && message.contains("handler not implemented")),
        );
    }
    #[rstest]
    fn test_option_chain_reference_price_is_unsupported() {
        let client_id = ClientId::new("BACKTEST");
        let venue = Venue::new("BACKTEST");
        let cache = Rc::new(RefCell::new(Cache::default()));
        let client = BacktestDataClient::new(client_id, venue, cache);
        let series_id = OptionSeriesId::new(
            venue,
            Ustr::from("BTC"),
            Ustr::from("BTC"),
            UnixNanos::default(),
        );

        let request = RequestOptionChainReferencePrice::new(
            series_id,
            InstrumentId::from("BTC-TEST-50000-C.BACKTEST"),
            Some(client_id),
            UUID4::new(),
            UnixNanos::default(),
            None,
        );

        let result = client.request_option_chain_reference_price(request);
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("backtest data client"));
    }
}
