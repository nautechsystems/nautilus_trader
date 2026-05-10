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

//! Live market data client implementation for the Bybit adapter.

use std::{
    future::Future,
    sync::{
        Arc, RwLock,
        atomic::{AtomicBool, Ordering},
    },
};

use ahash::{AHashMap, AHashSet};
use anyhow::Context;
use futures_util::{StreamExt, pin_mut};
use nautilus_common::{
    clients::DataClient,
    live::{runner::get_data_event_sender, runtime::get_runtime},
    messages::{
        DataEvent,
        data::{
            BarsResponse, BookResponse, DataResponse, FundingRatesResponse, InstrumentResponse,
            InstrumentsResponse, RequestBars, RequestBookSnapshot, RequestFundingRates,
            RequestInstrument, RequestInstruments, RequestTrades, SubscribeBars,
            SubscribeBookDeltas, SubscribeFundingRates, SubscribeIndexPrices, SubscribeMarkPrices,
            SubscribeQuotes, SubscribeTrades, TradesResponse, UnsubscribeBars,
            UnsubscribeBookDeltas, UnsubscribeFundingRates, UnsubscribeIndexPrices,
            UnsubscribeMarkPrices, UnsubscribeQuotes, UnsubscribeTrades,
        },
    },
};
use nautilus_core::{
    MUTEX_POISONED,
    datetime::datetime_to_unix_nanos,
    time::{AtomicTime, get_atomic_clock_realtime},
};
use nautilus_model::{
    data::{Data, OrderBookDeltas_API},
    enums::BookType,
    identifiers::{ClientId, InstrumentId, Venue},
    instruments::{Instrument, InstrumentAny},
    orderbook::book::OrderBook,
};
use tokio::{task::JoinHandle, time::Duration};
use tokio_util::sync::CancellationToken;

use crate::{
    common::{
        consts::{BYBIT_DEFAULT_ORDERBOOK_DEPTH, BYBIT_VENUE},
        enums::BybitProductType,
        parse::extract_raw_symbol,
    },
    config::BybitDataClientConfig,
    http::client::BybitHttpClient,
    websocket::{client::BybitWebSocketClient, messages::NautilusWsMessage},
};

/// Live market data client for Bybit.
#[derive(Debug)]
pub struct BybitDataClient {
    client_id: ClientId,
    config: BybitDataClientConfig,
    http_client: BybitHttpClient,
    ws_clients: Vec<BybitWebSocketClient>,
    is_connected: AtomicBool,
    cancellation_token: CancellationToken,
    tasks: Vec<JoinHandle<()>>,
    data_sender: tokio::sync::mpsc::UnboundedSender<DataEvent>,
    instruments: Arc<RwLock<AHashMap<InstrumentId, InstrumentAny>>>,
    book_depths: Arc<RwLock<AHashMap<InstrumentId, u32>>>,
    quote_depths: Arc<RwLock<AHashMap<InstrumentId, u32>>>,
    ticker_subs: Arc<RwLock<AHashMap<InstrumentId, AHashSet<&'static str>>>>,
    clock: &'static AtomicTime,
}

impl BybitDataClient {
    /// Creates a new [`BybitDataClient`] instance.
    ///
    /// # Errors
    ///
    /// Returns an error if the client fails to initialize.
    pub fn new(client_id: ClientId, config: BybitDataClientConfig) -> anyhow::Result<Self> {
        let clock = get_atomic_clock_realtime();
        let data_sender = get_data_event_sender();

        let http_client = if let (Some(api_key), Some(api_secret)) =
            (config.api_key.clone(), config.api_secret.clone())
        {
            BybitHttpClient::with_credentials(
                api_key,
                api_secret,
                Some(config.http_base_url()),
                config.http_timeout_secs,
                config.max_retries,
                config.retry_delay_initial_ms,
                config.retry_delay_max_ms,
                config.recv_window_ms,
                config.http_proxy_url.clone(),
            )?
        } else {
            BybitHttpClient::new(
                Some(config.http_base_url()),
                config.http_timeout_secs,
                config.max_retries,
                config.retry_delay_initial_ms,
                config.retry_delay_max_ms,
                config.recv_window_ms,
                config.http_proxy_url.clone(),
            )?
        };

        // Create a WebSocket client for each product type (default to Linear if empty)
        let product_types = if config.product_types.is_empty() {
            vec![BybitProductType::Linear]
        } else {
            config.product_types.clone()
        };

        let ws_clients: Vec<BybitWebSocketClient> = product_types
            .iter()
            .map(|product_type| {
                BybitWebSocketClient::new_public_with(
                    *product_type,
                    config.environment,
                    Some(config.ws_public_url_for(*product_type)),
                    config.heartbeat_interval_secs,
                )
            })
            .collect();

        Ok(Self {
            client_id,
            config,
            http_client,
            ws_clients,
            is_connected: AtomicBool::new(false),
            cancellation_token: CancellationToken::new(),
            tasks: Vec::new(),
            data_sender,
            instruments: Arc::new(RwLock::new(AHashMap::new())),
            book_depths: Arc::new(RwLock::new(AHashMap::new())),
            quote_depths: Arc::new(RwLock::new(AHashMap::new())),
            ticker_subs: Arc::new(RwLock::new(AHashMap::new())),
            clock,
        })
    }

    fn venue(&self) -> Venue {
        *BYBIT_VENUE
    }

    fn get_ws_client_for_product(
        &self,
        product_type: BybitProductType,
    ) -> Option<&BybitWebSocketClient> {
        self.ws_clients
            .iter()
            .find(|ws| ws.product_type() == Some(product_type))
    }

    fn get_product_type_for_instrument(
        &self,
        instrument_id: InstrumentId,
    ) -> Option<BybitProductType> {
        let guard = self.instruments.read().expect(MUTEX_POISONED);
        guard
            .get(&instrument_id)
            .and_then(|_| BybitProductType::from_suffix(instrument_id.symbol.as_str()))
    }

    fn send_data(sender: &tokio::sync::mpsc::UnboundedSender<DataEvent>, data: Data) {
        if let Err(e) = sender.send(DataEvent::Data(data)) {
            log::error!("Failed to emit data event: {e}");
        }
    }

    fn spawn_ws<F>(&self, fut: F, context: &'static str)
    where
        F: Future<Output = anyhow::Result<()>> + Send + 'static,
    {
        get_runtime().spawn(async move {
            if let Err(e) = fut.await {
                log::error!("{context}: {e:?}");
            }
        });
    }

    fn handle_ws_message(
        message: NautilusWsMessage,
        data_sender: &tokio::sync::mpsc::UnboundedSender<DataEvent>,
        _instruments: &Arc<RwLock<AHashMap<InstrumentId, InstrumentAny>>>,
        ticker_subs: &Arc<RwLock<AHashMap<InstrumentId, AHashSet<&'static str>>>>,
        quote_depths: &Arc<RwLock<AHashMap<InstrumentId, u32>>>,
        book_depths: &Arc<RwLock<AHashMap<InstrumentId, u32>>>,
    ) {
        match message {
            NautilusWsMessage::Data(payloads) => {
                let ticker = ticker_subs.read().expect(MUTEX_POISONED);
                let depths = quote_depths.read().expect(MUTEX_POISONED);
                for data in payloads {
                    // Filter quotes - only emit if subscribed via ticker (LINEAR) or depth (SPOT)
                    if let Data::Quote(ref quote) = data {
                        let has_ticker_sub = ticker
                            .get(&quote.instrument_id)
                            .is_some_and(|s| s.contains("quotes"));
                        let has_depth_sub = depths.contains_key(&quote.instrument_id);
                        if !has_ticker_sub && !has_depth_sub {
                            continue;
                        }
                    }
                    Self::send_data(data_sender, data);
                }
            }
            NautilusWsMessage::Deltas(deltas) => {
                let books = book_depths.read().expect(MUTEX_POISONED);
                if books.contains_key(&deltas.instrument_id) {
                    Self::send_data(data_sender, Data::Deltas(OrderBookDeltas_API::new(deltas)));
                }
            }
            NautilusWsMessage::FundingRates(updates) => {
                let subs = ticker_subs.read().expect(MUTEX_POISONED);
                for update in updates {
                    if !subs
                        .get(&update.instrument_id)
                        .is_some_and(|s| s.contains("funding"))
                    {
                        continue;
                    }

                    if let Err(e) = data_sender.send(DataEvent::FundingRate(update)) {
                        log::error!("Failed to emit funding rate event: {e}");
                    }
                }
            }
            NautilusWsMessage::MarkPrices(updates) => {
                let subs = ticker_subs.read().expect(MUTEX_POISONED);
                for update in updates {
                    if subs
                        .get(&update.instrument_id)
                        .is_some_and(|s| s.contains("mark_prices"))
                    {
                        Self::send_data(data_sender, Data::MarkPriceUpdate(update));
                    }
                }
            }
            NautilusWsMessage::IndexPrices(updates) => {
                let subs = ticker_subs.read().expect(MUTEX_POISONED);
                for update in updates {
                    if subs
                        .get(&update.instrument_id)
                        .is_some_and(|s| s.contains("index_prices"))
                    {
                        Self::send_data(data_sender, Data::IndexPriceUpdate(update));
                    }
                }
            }
            NautilusWsMessage::OrderStatusReports(_)
            | NautilusWsMessage::FillReports(_)
            | NautilusWsMessage::PositionStatusReport(_)
            | NautilusWsMessage::AccountState(_)
            | NautilusWsMessage::OrderRejected(_)
            | NautilusWsMessage::OrderCancelRejected(_)
            | NautilusWsMessage::OrderModifyRejected(_) => {
                log::debug!("Ignoring trading message on data client");
            }
            NautilusWsMessage::Error(e) => {
                log::error!(
                    "Bybit websocket error: code={} message={}",
                    e.code,
                    e.message
                );
            }
            NautilusWsMessage::Reconnected => {
                log::info!("Websocket reconnected");
            }
            NautilusWsMessage::Authenticated => {
                log::debug!("Websocket authenticated");
            }
        }
    }
}

fn upsert_instrument(
    cache: &Arc<RwLock<AHashMap<InstrumentId, InstrumentAny>>>,
    instrument: InstrumentAny,
) {
    let mut guard = cache.write().expect(MUTEX_POISONED);
    guard.insert(instrument.id(), instrument);
}

#[async_trait::async_trait(?Send)]
impl DataClient for BybitDataClient {
    fn client_id(&self) -> ClientId {
        self.client_id
    }

    fn venue(&self) -> Option<Venue> {
        Some(self.venue())
    }

    fn start(&mut self) -> anyhow::Result<()> {
        log::info!(
            "Started: client_id={}, product_types={:?}, environment={:?}, http_proxy_url={:?}, ws_proxy_url={:?}",
            self.client_id,
            self.config.product_types,
            self.config.environment,
            self.config.http_proxy_url,
            self.config.ws_proxy_url,
        );
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        log::info!("Stopping {id}", id = self.client_id);
        self.cancellation_token.cancel();
        self.is_connected.store(false, Ordering::Relaxed);
        Ok(())
    }

    fn reset(&mut self) -> anyhow::Result<()> {
        log::debug!("Resetting {id}", id = self.client_id);
        self.is_connected.store(false, Ordering::Relaxed);
        self.cancellation_token = CancellationToken::new();
        self.tasks.clear();
        self.book_depths.write().expect(MUTEX_POISONED).clear();
        self.quote_depths.write().expect(MUTEX_POISONED).clear();
        self.ticker_subs.write().expect(MUTEX_POISONED).clear();
        Ok(())
    }

    fn dispose(&mut self) -> anyhow::Result<()> {
        log::debug!("Disposing {id}", id = self.client_id);
        self.stop()
    }

    async fn connect(&mut self) -> anyhow::Result<()> {
        if self.is_connected() {
            return Ok(());
        }

        let product_types = if self.config.product_types.is_empty() {
            vec![BybitProductType::Linear]
        } else {
            self.config.product_types.clone()
        };

        let mut all_instruments = Vec::new();
        for product_type in &product_types {
            let fetched = self
                .http_client
                .request_instruments(*product_type, None)
                .await
                .with_context(|| {
                    format!("failed to request Bybit instruments for {product_type:?}")
                })?;

            self.http_client.cache_instruments(fetched.clone());

            let mut guard = self.instruments.write().expect(MUTEX_POISONED);
            for instrument in &fetched {
                guard.insert(instrument.id(), instrument.clone());
            }
            drop(guard);

            all_instruments.extend(fetched);
        }

        for instrument in all_instruments {
            if let Err(e) = self.data_sender.send(DataEvent::Instrument(instrument)) {
                log::warn!("Failed to send instrument: {e}");
            }
        }

        for ws_client in &mut self.ws_clients {
            // Cache instruments before connecting so parser has price/size precision
            let instruments: Vec<_> = self
                .instruments
                .read()
                .expect(MUTEX_POISONED)
                .values()
                .cloned()
                .collect();
            ws_client.cache_instruments(instruments);

            ws_client
                .connect()
                .await
                .context("failed to connect Bybit WebSocket")?;
            ws_client
                .wait_until_active(10.0)
                .await
                .context("WebSocket did not become active")?;

            let stream = ws_client.stream();
            let sender = self.data_sender.clone();
            let insts = self.instruments.clone();
            let ticker_subs = self.ticker_subs.clone();
            let quote_depths = self.quote_depths.clone();
            let book_depths = self.book_depths.clone();
            let cancel = self.cancellation_token.clone();
            let handle = get_runtime().spawn(async move {
                pin_mut!(stream);
                loop {
                    tokio::select! {
                        Some(message) = stream.next() => {
                            Self::handle_ws_message(message, &sender, &insts, &ticker_subs, &quote_depths, &book_depths);
                        }
                        () = cancel.cancelled() => {
                            log::debug!("WebSocket stream task cancelled");
                            break;
                        }
                    }
                }
            });
            self.tasks.push(handle);
        }

        self.is_connected.store(true, Ordering::Release);
        log::info!("Connected: client_id={}", self.client_id);
        Ok(())
    }

    async fn disconnect(&mut self) -> anyhow::Result<()> {
        if self.is_disconnected() {
            return Ok(());
        }

        self.cancellation_token.cancel();

        // Reinitialize token so reconnect can spawn new stream tasks
        self.cancellation_token = CancellationToken::new();

        for ws_client in &mut self.ws_clients {
            if let Err(e) = ws_client.close().await {
                log::warn!("Error closing WebSocket: {e:?}");
            }
        }

        // Allow time for unsubscribe confirmations
        tokio::time::sleep(Duration::from_millis(500)).await;

        let handles: Vec<_> = self.tasks.drain(..).collect();
        for handle in handles {
            if let Err(e) = handle.await {
                log::error!("Error joining WebSocket task: {e}");
            }
        }

        self.book_depths.write().expect(MUTEX_POISONED).clear();
        self.quote_depths.write().expect(MUTEX_POISONED).clear();
        self.ticker_subs.write().expect(MUTEX_POISONED).clear();
        self.is_connected.store(false, Ordering::Release);
        log::info!("Disconnected: client_id={}", self.client_id);
        Ok(())
    }

    fn is_connected(&self) -> bool {
        self.is_connected.load(Ordering::Relaxed)
    }

    fn is_disconnected(&self) -> bool {
        !self.is_connected()
    }

    fn subscribe_book_deltas(&mut self, cmd: &SubscribeBookDeltas) -> anyhow::Result<()> {
        if cmd.book_type != BookType::L2_MBP {
            anyhow::bail!("Bybit only supports L2_MBP order book deltas");
        }

        let depth = cmd
            .depth
            .map_or(BYBIT_DEFAULT_ORDERBOOK_DEPTH, |d| d.get() as u32);

        if !matches!(depth, 1 | 50 | 200 | 500) {
            anyhow::bail!("invalid depth {depth}; valid values are 1, 50, 200, or 500");
        }

        let instrument_id = cmd.instrument_id;
        let product_type = self
            .get_product_type_for_instrument(instrument_id)
            .unwrap_or(BybitProductType::Linear);

        let ws = self
            .get_ws_client_for_product(product_type)
            .context("no WebSocket client for product type")?
            .clone();

        let book_depths = Arc::clone(&self.book_depths);

        self.spawn_ws(
            async move {
                ws.subscribe_orderbook(instrument_id, depth)
                    .await
                    .context("orderbook subscription")?;
                book_depths
                    .write()
                    .expect("book depths cache lock poisoned")
                    .insert(instrument_id, depth);
                Ok(())
            },
            "order book delta subscription",
        );

        Ok(())
    }

    fn subscribe_quotes(&mut self, cmd: &SubscribeQuotes) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;
        let product_type = self
            .get_product_type_for_instrument(instrument_id)
            .unwrap_or(BybitProductType::Linear);

        let ws = self
            .get_ws_client_for_product(product_type)
            .context("no WebSocket client for product type")?
            .clone();

        // SPOT ticker channel doesn't include bid/ask, use orderbook depth=1
        if product_type == BybitProductType::Spot {
            let depth = 1;
            self.quote_depths
                .write()
                .expect(MUTEX_POISONED)
                .insert(instrument_id, depth);

            self.spawn_ws(
                async move {
                    ws.subscribe_orderbook(instrument_id, depth)
                        .await
                        .context("orderbook subscription for quotes")
                },
                "quote subscription (spot orderbook)",
            );
        } else {
            let should_subscribe = {
                let mut subs = self.ticker_subs.write().expect(MUTEX_POISONED);
                let entry = subs.entry(instrument_id).or_default();
                let is_first = entry.is_empty();
                entry.insert("quotes");
                is_first
            };

            if should_subscribe {
                self.spawn_ws(
                    async move {
                        ws.subscribe_ticker(instrument_id)
                            .await
                            .context("ticker subscription")
                    },
                    "quote subscription",
                );
            }
        }
        Ok(())
    }

    fn subscribe_trades(&mut self, cmd: &SubscribeTrades) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;
        let product_type = self
            .get_product_type_for_instrument(instrument_id)
            .unwrap_or(BybitProductType::Linear);

        let ws = self
            .get_ws_client_for_product(product_type)
            .context("no WebSocket client for product type")?
            .clone();

        self.spawn_ws(
            async move {
                ws.subscribe_trades(instrument_id)
                    .await
                    .context("trades subscription")
            },
            "trade subscription",
        );
        Ok(())
    }

    fn subscribe_funding_rates(&mut self, cmd: &SubscribeFundingRates) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;
        let product_type = self
            .get_product_type_for_instrument(instrument_id)
            .unwrap_or(BybitProductType::Linear);

        if product_type == BybitProductType::Spot {
            anyhow::bail!("Funding rates not available for Spot instruments");
        }

        let should_subscribe = {
            let mut subs = self.ticker_subs.write().expect(MUTEX_POISONED);
            let entry = subs.entry(instrument_id).or_default();
            let first = entry.is_empty();
            entry.insert("funding");
            first
        };

        if should_subscribe {
            let ws = self
                .get_ws_client_for_product(product_type)
                .context("no WebSocket client for product type")?
                .clone();

            self.spawn_ws(
                async move {
                    ws.subscribe_ticker(instrument_id)
                        .await
                        .context("ticker subscription for funding rates")
                },
                "funding rate subscription",
            );
        }
        Ok(())
    }

    fn subscribe_mark_prices(&mut self, cmd: &SubscribeMarkPrices) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;
        let product_type = self
            .get_product_type_for_instrument(instrument_id)
            .unwrap_or(BybitProductType::Linear);

        if product_type == BybitProductType::Spot {
            anyhow::bail!("Mark prices not available for Spot instruments");
        }

        let should_subscribe = {
            let mut subs = self.ticker_subs.write().expect(MUTEX_POISONED);
            let entry = subs.entry(instrument_id).or_default();
            let first = entry.is_empty();
            entry.insert("mark_prices");
            first
        };

        if should_subscribe {
            let ws = self
                .get_ws_client_for_product(product_type)
                .context("no WebSocket client for product type")?
                .clone();

            self.spawn_ws(
                async move {
                    ws.subscribe_ticker(instrument_id)
                        .await
                        .context("ticker subscription for mark prices")
                },
                "mark price subscription",
            );
        }
        Ok(())
    }

    fn subscribe_index_prices(&mut self, cmd: &SubscribeIndexPrices) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;
        let product_type = self
            .get_product_type_for_instrument(instrument_id)
            .unwrap_or(BybitProductType::Linear);

        if product_type == BybitProductType::Spot {
            anyhow::bail!("Index prices not available for Spot instruments");
        }

        let should_subscribe = {
            let mut subs = self.ticker_subs.write().expect(MUTEX_POISONED);
            let entry = subs.entry(instrument_id).or_default();
            let first = entry.is_empty();
            entry.insert("index_prices");
            first
        };

        if should_subscribe {
            let ws = self
                .get_ws_client_for_product(product_type)
                .context("no WebSocket client for product type")?
                .clone();

            self.spawn_ws(
                async move {
                    ws.subscribe_ticker(instrument_id)
                        .await
                        .context("ticker subscription for index prices")
                },
                "index price subscription",
            );
        }
        Ok(())
    }

    fn subscribe_bars(&mut self, cmd: &SubscribeBars) -> anyhow::Result<()> {
        let bar_type = cmd.bar_type;
        let instrument_id = bar_type.instrument_id();
        let product_type = self
            .get_product_type_for_instrument(instrument_id)
            .unwrap_or(BybitProductType::Linear);

        let ws = self
            .get_ws_client_for_product(product_type)
            .context("no WebSocket client for product type")?
            .clone();

        self.spawn_ws(
            async move {
                ws.subscribe_bars(bar_type)
                    .await
                    .context("bars subscription")
            },
            "bar subscription",
        );
        Ok(())
    }

    fn unsubscribe_book_deltas(&mut self, cmd: &UnsubscribeBookDeltas) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;
        let depth = self
            .book_depths
            .write()
            .expect(MUTEX_POISONED)
            .remove(&instrument_id)
            .unwrap_or(BYBIT_DEFAULT_ORDERBOOK_DEPTH);

        let product_type = self
            .get_product_type_for_instrument(instrument_id)
            .unwrap_or(BybitProductType::Linear);

        // Check if spot quote subscription is using the same depth
        let quote_using_same_depth = self
            .quote_depths
            .read()
            .expect(MUTEX_POISONED)
            .get(&instrument_id)
            .is_some_and(|&d| d == depth);

        if quote_using_same_depth {
            return Ok(());
        }

        let ws = self
            .get_ws_client_for_product(product_type)
            .context("no WebSocket client for product type")?
            .clone();

        self.spawn_ws(
            async move {
                ws.unsubscribe_orderbook(instrument_id, depth)
                    .await
                    .context("orderbook unsubscribe")
            },
            "order book unsubscribe",
        );
        Ok(())
    }

    fn unsubscribe_quotes(&mut self, cmd: &UnsubscribeQuotes) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;
        let product_type = self
            .get_product_type_for_instrument(instrument_id)
            .unwrap_or(BybitProductType::Linear);

        let ws = self
            .get_ws_client_for_product(product_type)
            .context("no WebSocket client for product type")?
            .clone();

        if product_type == BybitProductType::Spot {
            let depth = self
                .quote_depths
                .write()
                .expect(MUTEX_POISONED)
                .remove(&instrument_id)
                .unwrap_or(1);

            // Check if book deltas subscription is using the same depth
            let book_using_same_depth = self
                .book_depths
                .read()
                .expect(MUTEX_POISONED)
                .get(&instrument_id)
                .is_some_and(|&d| d == depth);

            if !book_using_same_depth {
                self.spawn_ws(
                    async move {
                        ws.unsubscribe_orderbook(instrument_id, depth)
                            .await
                            .context("orderbook unsubscribe for quotes")
                    },
                    "quote unsubscribe (spot orderbook)",
                );
            }
        } else {
            let should_unsubscribe = {
                let mut subs = self.ticker_subs.write().expect(MUTEX_POISONED);
                if let Some(entry) = subs.get_mut(&instrument_id) {
                    entry.remove("quotes");
                    if entry.is_empty() {
                        subs.remove(&instrument_id);
                        true
                    } else {
                        false
                    }
                } else {
                    false
                }
            };

            if should_unsubscribe {
                self.spawn_ws(
                    async move {
                        ws.unsubscribe_ticker(instrument_id)
                            .await
                            .context("ticker unsubscribe")
                    },
                    "quote unsubscribe",
                );
            }
        }
        Ok(())
    }

    fn unsubscribe_trades(&mut self, cmd: &UnsubscribeTrades) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;
        let product_type = self
            .get_product_type_for_instrument(instrument_id)
            .unwrap_or(BybitProductType::Linear);

        let ws = self
            .get_ws_client_for_product(product_type)
            .context("no WebSocket client for product type")?
            .clone();

        self.spawn_ws(
            async move {
                ws.unsubscribe_trades(instrument_id)
                    .await
                    .context("trades unsubscribe")
            },
            "trade unsubscribe",
        );
        Ok(())
    }

    fn unsubscribe_funding_rates(&mut self, cmd: &UnsubscribeFundingRates) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;
        let product_type = self
            .get_product_type_for_instrument(instrument_id)
            .unwrap_or(BybitProductType::Linear);

        let should_unsubscribe = {
            let mut subs = self.ticker_subs.write().expect(MUTEX_POISONED);
            if let Some(entry) = subs.get_mut(&instrument_id) {
                entry.remove("funding");
                if entry.is_empty() {
                    subs.remove(&instrument_id);
                    true
                } else {
                    false
                }
            } else {
                false
            }
        };

        if should_unsubscribe {
            let ws = self
                .get_ws_client_for_product(product_type)
                .context("no WebSocket client for product type")?
                .clone();

            self.spawn_ws(
                async move {
                    ws.unsubscribe_ticker(instrument_id)
                        .await
                        .context("ticker unsubscribe for funding rates")
                },
                "funding rate unsubscribe",
            );
        }
        Ok(())
    }

    fn unsubscribe_mark_prices(&mut self, cmd: &UnsubscribeMarkPrices) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;
        let product_type = self
            .get_product_type_for_instrument(instrument_id)
            .unwrap_or(BybitProductType::Linear);

        let should_unsubscribe = {
            let mut subs = self.ticker_subs.write().expect(MUTEX_POISONED);
            if let Some(entry) = subs.get_mut(&instrument_id) {
                entry.remove("mark_prices");
                if entry.is_empty() {
                    subs.remove(&instrument_id);
                    true
                } else {
                    false
                }
            } else {
                false
            }
        };

        if should_unsubscribe {
            let ws = self
                .get_ws_client_for_product(product_type)
                .context("no WebSocket client for product type")?
                .clone();

            self.spawn_ws(
                async move {
                    ws.unsubscribe_ticker(instrument_id)
                        .await
                        .context("ticker unsubscribe for mark prices")
                },
                "mark price unsubscribe",
            );
        }
        Ok(())
    }

    fn unsubscribe_index_prices(&mut self, cmd: &UnsubscribeIndexPrices) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;
        let product_type = self
            .get_product_type_for_instrument(instrument_id)
            .unwrap_or(BybitProductType::Linear);

        let should_unsubscribe = {
            let mut subs = self.ticker_subs.write().expect(MUTEX_POISONED);
            if let Some(entry) = subs.get_mut(&instrument_id) {
                entry.remove("index_prices");
                if entry.is_empty() {
                    subs.remove(&instrument_id);
                    true
                } else {
                    false
                }
            } else {
                false
            }
        };

        if should_unsubscribe {
            let ws = self
                .get_ws_client_for_product(product_type)
                .context("no WebSocket client for product type")?
                .clone();

            self.spawn_ws(
                async move {
                    ws.unsubscribe_ticker(instrument_id)
                        .await
                        .context("ticker unsubscribe for index prices")
                },
                "index price unsubscribe",
            );
        }
        Ok(())
    }

    fn unsubscribe_bars(&mut self, cmd: &UnsubscribeBars) -> anyhow::Result<()> {
        let bar_type = cmd.bar_type;
        let instrument_id = bar_type.instrument_id();
        let product_type = self
            .get_product_type_for_instrument(instrument_id)
            .unwrap_or(BybitProductType::Linear);

        let ws = self
            .get_ws_client_for_product(product_type)
            .context("no WebSocket client for product type")?
            .clone();

        self.spawn_ws(
            async move {
                ws.unsubscribe_bars(bar_type)
                    .await
                    .context("bars unsubscribe")
            },
            "bar unsubscribe",
        );
        Ok(())
    }

    fn request_instruments(&self, request: RequestInstruments) -> anyhow::Result<()> {
        let http = self.http_client.clone();
        let sender = self.data_sender.clone();
        let instruments_cache = self.instruments.clone();
        let request_id = request.request_id;
        let client_id = request.client_id.unwrap_or(self.client_id);
        let venue = self.venue();
        let start = request.start;
        let end = request.end;
        let params = request.params;
        let clock = self.clock;
        let start_nanos = datetime_to_unix_nanos(start);
        let end_nanos = datetime_to_unix_nanos(end);
        let product_types = if self.config.product_types.is_empty() {
            vec![BybitProductType::Linear]
        } else {
            self.config.product_types.clone()
        };

        get_runtime().spawn(async move {
            let mut all_instruments = Vec::new();

            for product_type in product_types {
                match http.request_instruments(product_type, None).await {
                    Ok(instruments) => {
                        for instrument in instruments {
                            upsert_instrument(&instruments_cache, instrument.clone());
                            all_instruments.push(instrument);
                        }
                    }
                    Err(e) => {
                        log::error!("Failed to fetch instruments for {product_type:?}: {e:?}");
                    }
                }
            }

            let response = DataResponse::Instruments(InstrumentsResponse::new(
                request_id,
                client_id,
                venue,
                all_instruments,
                start_nanos,
                end_nanos,
                clock.get_time_ns(),
                params,
            ));

            if let Err(e) = sender.send(DataEvent::Response(response)) {
                log::error!("Failed to send instruments response: {e}");
            }
        });

        Ok(())
    }

    fn request_instrument(&self, request: RequestInstrument) -> anyhow::Result<()> {
        let http = self.http_client.clone();
        let sender = self.data_sender.clone();
        let instruments = self.instruments.clone();
        let instrument_id = request.instrument_id;
        let request_id = request.request_id;
        let client_id = request.client_id.unwrap_or(self.client_id);
        let start = request.start;
        let end = request.end;
        let params = request.params;
        let clock = self.clock;
        let start_nanos = datetime_to_unix_nanos(start);
        let end_nanos = datetime_to_unix_nanos(end);

        let product_type = BybitProductType::from_suffix(instrument_id.symbol.as_str())
            .unwrap_or(BybitProductType::Linear);
        let raw_symbol = extract_raw_symbol(instrument_id.symbol.as_str()).to_string();

        get_runtime().spawn(async move {
            match http
                .request_instruments(product_type, Some(raw_symbol))
                .await
                .context("fetch instrument from API")
            {
                Ok(fetched) => {
                    if let Some(instrument) = fetched.into_iter().find(|i| i.id() == instrument_id)
                    {
                        upsert_instrument(&instruments, instrument.clone());

                        let response = DataResponse::Instrument(Box::new(InstrumentResponse::new(
                            request_id,
                            client_id,
                            instrument.id(),
                            instrument,
                            start_nanos,
                            end_nanos,
                            clock.get_time_ns(),
                            params,
                        )));

                        if let Err(e) = sender.send(DataEvent::Response(response)) {
                            log::error!("Failed to send instrument response: {e}");
                        }
                    } else {
                        log::error!("Instrument not found: {instrument_id}");
                    }
                }
                Err(e) => log::error!("Instrument request failed: {e:?}"),
            }
        });

        Ok(())
    }

    fn request_book_snapshot(&self, request: RequestBookSnapshot) -> anyhow::Result<()> {
        let http = self.http_client.clone();
        let sender = self.data_sender.clone();
        let instrument_id = request.instrument_id;
        let depth = request.depth.map(|n| n.get() as u32);
        let request_id = request.request_id;
        let client_id = request.client_id.unwrap_or(self.client_id);
        let params = request.params;
        let clock = self.clock;

        let product_type = BybitProductType::from_suffix(instrument_id.symbol.as_str())
            .unwrap_or(BybitProductType::Linear);

        get_runtime().spawn(async move {
            match http
                .request_orderbook_snapshot(product_type, instrument_id, depth)
                .await
                .context("failed to request book snapshot from Bybit")
            {
                Ok(deltas) => {
                    let mut book = OrderBook::new(instrument_id, BookType::L2_MBP);
                    if let Err(e) = book.apply_deltas(&deltas) {
                        log::error!("Failed to apply book deltas for {instrument_id}: {e}");
                        return;
                    }

                    let response = DataResponse::Book(BookResponse::new(
                        request_id,
                        client_id,
                        instrument_id,
                        book,
                        None,
                        None,
                        clock.get_time_ns(),
                        params,
                    ));

                    if let Err(e) = sender.send(DataEvent::Response(response)) {
                        log::error!("Failed to send book snapshot response: {e}");
                    }
                }
                Err(e) => log::error!("Book snapshot request failed for {instrument_id}: {e:?}"),
            }
        });

        Ok(())
    }

    fn request_trades(&self, request: RequestTrades) -> anyhow::Result<()> {
        let http = self.http_client.clone();
        let sender = self.data_sender.clone();
        let instrument_id = request.instrument_id;
        let start = request.start;
        let end = request.end;
        let limit = request.limit.map(|n| n.get() as u32);
        let request_id = request.request_id;
        let client_id = request.client_id.unwrap_or(self.client_id);
        let params = request.params;
        let clock = self.clock;
        let start_nanos = datetime_to_unix_nanos(start);
        let end_nanos = datetime_to_unix_nanos(end);

        let product_type = BybitProductType::from_suffix(instrument_id.symbol.as_str())
            .unwrap_or(BybitProductType::Linear);

        get_runtime().spawn(async move {
            match http
                .request_trades(product_type, instrument_id, limit)
                .await
                .context("failed to request trades from Bybit")
            {
                Ok(trades) => {
                    let response = DataResponse::Trades(TradesResponse::new(
                        request_id,
                        client_id,
                        instrument_id,
                        trades,
                        start_nanos,
                        end_nanos,
                        clock.get_time_ns(),
                        params,
                    ));

                    if let Err(e) = sender.send(DataEvent::Response(response)) {
                        log::error!("Failed to send trades response: {e}");
                    }
                }
                Err(e) => log::error!("Trade request failed: {e:?}"),
            }
        });

        Ok(())
    }

    fn request_bars(&self, request: RequestBars) -> anyhow::Result<()> {
        let http = self.http_client.clone();
        let sender = self.data_sender.clone();
        let bar_type = request.bar_type;
        let start = request.start;
        let end = request.end;
        let limit = request.limit.map(|n| n.get() as u32);
        let request_id = request.request_id;
        let client_id = request.client_id.unwrap_or(self.client_id);
        let params = request.params;
        let clock = self.clock;
        let start_nanos = datetime_to_unix_nanos(start);
        let end_nanos = datetime_to_unix_nanos(end);

        let instrument_id = bar_type.instrument_id();
        let product_type = BybitProductType::from_suffix(instrument_id.symbol.as_str())
            .unwrap_or(BybitProductType::Linear);

        get_runtime().spawn(async move {
            match http
                .request_bars(product_type, bar_type, start, end, limit, true)
                .await
                .context("failed to request bars from Bybit")
            {
                Ok(bars) => {
                    let response = DataResponse::Bars(BarsResponse::new(
                        request_id,
                        client_id,
                        bar_type,
                        bars,
                        start_nanos,
                        end_nanos,
                        clock.get_time_ns(),
                        params,
                    ));

                    if let Err(e) = sender.send(DataEvent::Response(response)) {
                        log::error!("Failed to send bars response: {e}");
                    }
                }
                Err(e) => log::error!("Bar request failed: {e:?}"),
            }
        });

        Ok(())
    }

    fn request_funding_rates(&self, request: RequestFundingRates) -> anyhow::Result<()> {
        let http = self.http_client.clone();
        let sender = self.data_sender.clone();
        let instrument_id = request.instrument_id;
        let start = request.start;
        let end = request.end;
        let limit = request.limit.map(|n| n.get() as u32);
        let request_id = request.request_id;
        let client_id = request.client_id.unwrap_or(self.client_id);
        let params = request.params;
        let clock = self.clock;
        let start_nanos = datetime_to_unix_nanos(start);
        let end_nanos = datetime_to_unix_nanos(end);

        let product_type = BybitProductType::from_suffix(instrument_id.symbol.as_str())
            .unwrap_or(BybitProductType::Linear);

        if product_type == BybitProductType::Spot || product_type == BybitProductType::Option {
            anyhow::bail!("Funding rates not available for {product_type} instruments");
        }

        get_runtime().spawn(async move {
            match http
                .request_funding_rates(product_type, instrument_id, start, end, limit)
                .await
                .context("failed to request funding rates from Bybit")
            {
                Ok(funding_rates) => {
                    let response = DataResponse::FundingRates(FundingRatesResponse::new(
                        request_id,
                        client_id,
                        instrument_id,
                        funding_rates,
                        start_nanos,
                        end_nanos,
                        clock.get_time_ns(),
                        params,
                    ));

                    if let Err(e) = sender.send(DataEvent::Response(response)) {
                        log::error!("Failed to send funding rates response: {e}");
                    }
                }
                Err(e) => log::error!("Funding rates request failed for {instrument_id}: {e:?}"),
            }
        });

        Ok(())
    }
}
