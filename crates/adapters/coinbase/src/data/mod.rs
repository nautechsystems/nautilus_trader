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

//! Coinbase Advanced Trade data client for NautilusTrader.
//!
//! Implements the [`DataClient`] trait, providing market data subscriptions and
//! historical data requests through the Coinbase Advanced Trade API.

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use anyhow::Context;
use nautilus_common::{
    clients::DataClient,
    live::{runner::get_data_event_sender, runtime::get_runtime},
    messages::{
        DataEvent,
        data::{
            BarsResponse, BookResponse, DataResponse, InstrumentResponse, InstrumentsResponse,
            RequestBars, RequestBookSnapshot, RequestInstrument, RequestInstruments, RequestTrades,
            SubscribeBars, SubscribeBookDeltas, SubscribeFundingRates, SubscribeIndexPrices,
            SubscribeInstrument, SubscribeMarkPrices, SubscribeQuotes, SubscribeTrades,
            TradesResponse, UnsubscribeBars, UnsubscribeBookDeltas, UnsubscribeFundingRates,
            UnsubscribeIndexPrices, UnsubscribeInstrument, UnsubscribeMarkPrices,
            UnsubscribeQuotes, UnsubscribeTrades,
        },
    },
};
use nautilus_core::{
    AtomicMap,
    datetime::datetime_to_unix_nanos,
    time::{AtomicTime, get_atomic_clock_realtime},
};
use nautilus_model::{
    data::{Data, OrderBookDeltas_API},
    enums::{BarAggregation, BookType, OrderSide},
    identifiers::{ClientId, InstrumentId, Venue},
    instruments::{Instrument, InstrumentAny},
    orderbook::OrderBook,
};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use ustr::Ustr;

pub(crate) mod poll;

use crate::{
    common::{
        consts::COINBASE_VENUE, credential::CoinbaseCredential, enums::CoinbaseWsChannel,
        parse::bar_type_to_granularity,
    },
    config::CoinbaseDataClientConfig,
    data::poll::DerivPollManager,
    http::{
        client::{CoinbaseHttpClient, data_client_retry_config},
        models::{CandlesResponse, PriceBook, TickerResponse},
        parse::{parse_bar, parse_product_book_snapshot, parse_trade_tick},
    },
    provider::CoinbaseInstrumentProvider,
    websocket::{client::CoinbaseWebSocketClient, handler::NautilusWsMessage},
};

/// Data client for Coinbase Advanced Trade.
///
/// Owns an HTTP client, WebSocket client, and instrument provider. Bootstraps
/// instruments on connect, subscribes to WS channels for live data, and handles
/// historical data requests through the REST API.
#[derive(Debug)]
pub struct CoinbaseDataClient {
    client_id: ClientId,
    #[allow(dead_code)]
    config: CoinbaseDataClientConfig,
    http_client: CoinbaseHttpClient,
    ws_client: CoinbaseWebSocketClient,
    provider: CoinbaseInstrumentProvider,
    is_connected: AtomicBool,
    cancellation_token: CancellationToken,
    tasks: Vec<JoinHandle<()>>,
    data_sender: tokio::sync::mpsc::UnboundedSender<DataEvent>,
    instruments: Arc<AtomicMap<InstrumentId, InstrumentAny>>,
    deriv_polls: DerivPollManager,
    clock: &'static AtomicTime,
}

impl CoinbaseDataClient {
    /// Creates a new [`CoinbaseDataClient`] instance.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP client fails to initialize.
    pub fn new(client_id: ClientId, config: CoinbaseDataClientConfig) -> anyhow::Result<Self> {
        let clock = get_atomic_clock_realtime();
        let data_sender = get_data_event_sender();

        let retry_config = data_client_retry_config();

        let http_client = match CoinbaseCredential::resolve(
            config.api_key.as_deref(),
            config.api_secret.as_deref(),
        ) {
            Some(credential) => CoinbaseHttpClient::with_credentials(
                credential,
                config.environment,
                config.http_timeout_secs,
                config.proxy_url.clone(),
                Some(retry_config),
            )
            .map_err(|e| anyhow::anyhow!("Failed to create HTTP client: {e}"))?,
            None => CoinbaseHttpClient::new(
                config.environment,
                config.http_timeout_secs,
                config.proxy_url.clone(),
                Some(retry_config),
            )
            .map_err(|e| anyhow::anyhow!("Failed to create HTTP client: {e}"))?,
        };

        if let Some(url) = &config.base_url_rest {
            http_client.set_base_url(url.clone());
        }

        let ws_url = config.ws_url();
        let ws_client = CoinbaseWebSocketClient::new(
            &ws_url,
            config.transport_backend,
            config.proxy_url.clone(),
        );
        let provider = CoinbaseInstrumentProvider::new(http_client.clone());

        let deriv_polls = DerivPollManager::new(
            http_client.clone(),
            data_sender.clone(),
            clock,
            config.derivatives_poll_interval_secs,
        );

        Ok(Self {
            client_id,
            config,
            http_client,
            ws_client,
            provider,
            is_connected: AtomicBool::new(false),
            cancellation_token: CancellationToken::new(),
            tasks: Vec::new(),
            data_sender,
            instruments: Arc::new(AtomicMap::new()),
            deriv_polls,
            clock,
        })
    }

    fn venue(&self) -> Venue {
        *COINBASE_VENUE
    }

    async fn bootstrap_instruments(&self) -> anyhow::Result<Vec<InstrumentAny>> {
        let instruments = self
            .provider
            .load_all()
            .await
            .context("failed to fetch instruments during bootstrap")?;

        self.instruments.rcu(|m| {
            for instrument in &instruments {
                m.insert(instrument.id(), instrument.clone());
            }
        });

        for instrument in &instruments {
            self.ws_client.update_instrument(instrument.clone()).await;
        }

        log::info!("Bootstrapped {} instruments", instruments.len());
        Ok(instruments)
    }

    async fn spawn_ws(&mut self) -> anyhow::Result<()> {
        self.ws_client
            .connect()
            .await
            .context("failed to connect to Coinbase WebSocket")?;

        let mut out_rx = self
            .ws_client
            .take_out_rx()
            .ok_or_else(|| anyhow::anyhow!("WebSocket output receiver not available"))?;

        let data_sender = self.data_sender.clone();
        let cancellation_token = self.cancellation_token.clone();

        let task = get_runtime().spawn(async move {
            log::info!("Coinbase WebSocket consumption loop started");

            loop {
                tokio::select! {
                    () = cancellation_token.cancelled() => {
                        log::info!("WebSocket consumption loop cancelled");
                        break;
                    }
                    msg_opt = out_rx.recv() => {
                        match msg_opt {
                            Some(msg) => dispatch_ws_message(msg, &data_sender),
                            None => {
                                log::debug!("WebSocket output channel closed");
                                break;
                            }
                        }
                    }
                }
            }

            log::info!("Coinbase WebSocket consumption loop finished");
        });

        self.tasks.push(task);
        log::info!("WebSocket consumption task spawned");
        Ok(())
    }

    fn product_id(instrument_id: InstrumentId) -> Ustr {
        instrument_id.symbol.inner()
    }

    // Resolves a caller-supplied product id to Coinbase's canonical alias (if
    // any). Coinbase consolidates aliased pairs into a single book server-side
    // and rewrites WS subscription confirmations and inbound messages to use
    // the canonical id (e.g. BTC-USDC -> BTC-USD), so we must subscribe with
    // the canonical id and remember the mapping so inbound messages can be
    // re-keyed to what the strategy actually subscribed to.
    fn resolve_wire_product_id(&self, subscribed: Ustr) -> Ustr {
        self.http_client
            .product_aliases()
            .get_cloned(&subscribed)
            .filter(|alias| !alias.is_empty())
            .unwrap_or(subscribed)
    }
}

fn dispatch_ws_message(
    msg: NautilusWsMessage,
    data_sender: &tokio::sync::mpsc::UnboundedSender<DataEvent>,
) {
    match msg {
        NautilusWsMessage::Trade(trade) => {
            if let Err(e) = data_sender.send(DataEvent::Data(Data::Trade(trade))) {
                log::error!("Failed to send trade tick: {e}");
            }
        }
        NautilusWsMessage::Quote(quote) => {
            if let Err(e) = data_sender.send(DataEvent::Data(Data::Quote(quote))) {
                log::error!("Failed to send quote tick: {e}");
            }
        }
        NautilusWsMessage::Deltas(deltas) => {
            if let Err(e) = data_sender.send(DataEvent::Data(Data::Deltas(
                OrderBookDeltas_API::new(deltas),
            ))) {
                log::error!("Failed to send order book deltas: {e}");
            }
        }
        NautilusWsMessage::Bar(bar) => {
            if let Err(e) = data_sender.send(DataEvent::Data(Data::Bar(bar))) {
                log::error!("Failed to send bar: {e}");
            }
        }
        NautilusWsMessage::Reconnected => {
            log::info!("WebSocket reconnected");
        }
        NautilusWsMessage::Error(e) => {
            log::error!("WebSocket error: {e}");
        }
        NautilusWsMessage::UserOrder(_) => {
            // User-channel execution reports are consumed by the execution client
            log::debug!("Dropping user-channel update received on the data client");
        }
        NautilusWsMessage::FuturesBalanceSummary(_) => {
            // Futures balance summary events are consumed by the execution client
            log::debug!("Dropping futures_balance_summary event received on the data client");
        }
    }
}

#[async_trait::async_trait(?Send)]
impl DataClient for CoinbaseDataClient {
    fn client_id(&self) -> ClientId {
        self.client_id
    }

    fn venue(&self) -> Option<Venue> {
        Some(Self::venue(self))
    }

    fn start(&mut self) -> anyhow::Result<()> {
        log::info!(
            "Starting Coinbase data client: client_id={}, environment={:?}",
            self.client_id,
            self.config.environment,
        );
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        log::info!("Stopping Coinbase data client {}", self.client_id);
        self.cancellation_token.cancel();
        self.deriv_polls.shutdown();
        self.is_connected.store(false, Ordering::Relaxed);
        Ok(())
    }

    fn reset(&mut self) -> anyhow::Result<()> {
        log::debug!("Resetting Coinbase data client {}", self.client_id);
        self.cancellation_token.cancel();
        self.deriv_polls.shutdown();
        self.is_connected.store(false, Ordering::Relaxed);
        self.cancellation_token = CancellationToken::new();
        self.tasks.clear();
        Ok(())
    }

    fn dispose(&mut self) -> anyhow::Result<()> {
        log::debug!("Disposing Coinbase data client {}", self.client_id);
        self.stop()
    }

    fn is_connected(&self) -> bool {
        self.is_connected.load(Ordering::Acquire)
    }

    fn is_disconnected(&self) -> bool {
        !self.is_connected()
    }

    async fn connect(&mut self) -> anyhow::Result<()> {
        if self.is_connected() {
            return Ok(());
        }

        self.cancellation_token = CancellationToken::new();

        let instruments = self
            .bootstrap_instruments()
            .await
            .context("failed to bootstrap instruments")?;

        for instrument in instruments {
            if let Err(e) = self.data_sender.send(DataEvent::Instrument(instrument)) {
                log::warn!("Failed to send instrument: {e}");
            }
        }

        self.spawn_ws()
            .await
            .context("failed to spawn WebSocket client")?;

        // Re-spawn polling tasks for any derivatives subscriptions that
        // survived a previous disconnect. The data engine's client adapter
        // remembers the subscription set and suppresses duplicate subscribe
        // commands, so without this resume step index-price and
        // funding-rate streams would stay dark after a reconnect.
        self.deriv_polls.resume();

        self.is_connected.store(true, Ordering::Relaxed);
        log::info!("Connected: client_id={}", self.client_id);

        Ok(())
    }

    async fn disconnect(&mut self) -> anyhow::Result<()> {
        if !self.is_connected() {
            return Ok(());
        }

        self.cancellation_token.cancel();
        self.deriv_polls.shutdown();

        for task in self.tasks.drain(..) {
            if let Err(e) = task.await {
                log::error!("Error waiting for task to complete: {e}");
            }
        }

        self.ws_client.disconnect().await;
        self.instruments.store(ahash::AHashMap::new());
        self.is_connected.store(false, Ordering::Relaxed);
        log::info!("Disconnected: client_id={}", self.client_id);

        Ok(())
    }

    fn subscribe_instrument(&mut self, cmd: SubscribeInstrument) -> anyhow::Result<()> {
        let instruments = self.instruments.load();

        if let Some(instrument) = instruments.get(&cmd.instrument_id) {
            if let Err(e) = self
                .data_sender
                .send(DataEvent::Instrument(instrument.clone()))
            {
                log::error!("Failed to send instrument {}: {e}", cmd.instrument_id);
            }
        } else {
            log::warn!("Instrument {} not found in cache", cmd.instrument_id);
        }

        Ok(())
    }

    fn subscribe_book_deltas(&mut self, subscription: SubscribeBookDeltas) -> anyhow::Result<()> {
        log::debug!("Subscribing to book deltas: {}", subscription.instrument_id);

        if subscription.book_type != BookType::L2_MBP {
            anyhow::bail!("Coinbase only supports L2_MBP order book deltas");
        }

        let ws = self.ws_client.clone();
        let subscribed_id = Self::product_id(subscription.instrument_id);
        let wire_id = self.resolve_wire_product_id(subscribed_id);
        if wire_id != subscribed_id {
            ws.register_subscription_alias(wire_id, subscribed_id);
        }

        get_runtime().spawn(async move {
            if let Err(e) = ws.subscribe(CoinbaseWsChannel::Level2, &[wire_id]).await {
                log::error!("Failed to subscribe to book deltas: {e:?}");
            }
        });

        Ok(())
    }

    fn subscribe_quotes(&mut self, subscription: SubscribeQuotes) -> anyhow::Result<()> {
        log::debug!("Subscribing to quotes: {}", subscription.instrument_id);

        let ws = self.ws_client.clone();
        let subscribed_id = Self::product_id(subscription.instrument_id);
        let wire_id = self.resolve_wire_product_id(subscribed_id);
        if wire_id != subscribed_id {
            ws.register_subscription_alias(wire_id, subscribed_id);
        }

        get_runtime().spawn(async move {
            if let Err(e) = ws.subscribe(CoinbaseWsChannel::Ticker, &[wire_id]).await {
                log::error!("Failed to subscribe to quotes: {e:?}");
            }
        });

        Ok(())
    }

    fn subscribe_trades(&mut self, subscription: SubscribeTrades) -> anyhow::Result<()> {
        log::debug!("Subscribing to trades: {}", subscription.instrument_id);

        let ws = self.ws_client.clone();
        let subscribed_id = Self::product_id(subscription.instrument_id);
        let wire_id = self.resolve_wire_product_id(subscribed_id);
        if wire_id != subscribed_id {
            ws.register_subscription_alias(wire_id, subscribed_id);
        }

        get_runtime().spawn(async move {
            if let Err(e) = ws
                .subscribe(CoinbaseWsChannel::MarketTrades, &[wire_id])
                .await
            {
                log::error!("Failed to subscribe to trades: {e:?}");
            }
        });

        Ok(())
    }

    fn subscribe_mark_prices(&mut self, cmd: SubscribeMarkPrices) -> anyhow::Result<()> {
        // Coinbase Advanced Trade does not publish a live mark price for its
        // perpetuals on either WS or REST. `settlement_price` is the prior
        // daily settlement and drifts from the live index, so synthesizing a
        // mark from it would be misleading. Reject explicitly so callers
        // failing this subscription know why.
        anyhow::bail!(
            "Coinbase Advanced Trade does not publish mark prices; \
             cannot subscribe for {}",
            cmd.instrument_id
        )
    }

    fn subscribe_index_prices(&mut self, cmd: SubscribeIndexPrices) -> anyhow::Result<()> {
        log::debug!("Subscribing to index prices: {}", cmd.instrument_id);
        self.deriv_polls.subscribe_index(cmd.instrument_id);
        Ok(())
    }

    fn subscribe_funding_rates(&mut self, cmd: SubscribeFundingRates) -> anyhow::Result<()> {
        log::debug!("Subscribing to funding rates: {}", cmd.instrument_id);
        self.deriv_polls.subscribe_funding(cmd.instrument_id);
        Ok(())
    }

    fn subscribe_bars(&mut self, subscription: SubscribeBars) -> anyhow::Result<()> {
        log::debug!("Subscribing to bars: {}", subscription.bar_type);

        let instrument_id = subscription.bar_type.instrument_id();

        if !self.instruments.contains_key(&instrument_id) {
            anyhow::bail!("Instrument {instrument_id} not found");
        }

        let bar_type = subscription.bar_type;
        let subscribed_id = Self::product_id(instrument_id);
        let wire_id = self.resolve_wire_product_id(subscribed_id);
        if wire_id != subscribed_id {
            self.ws_client
                .register_subscription_alias(wire_id, subscribed_id);
        }
        let key = wire_id.to_string();

        // Register on the original client so the bar type persists across clones
        self.ws_client.register_bar_type(key.clone(), bar_type);

        let mut ws = self.ws_client.clone();

        get_runtime().spawn(async move {
            ws.add_bar_type(key, bar_type).await;

            if let Err(e) = ws.subscribe(CoinbaseWsChannel::Candles, &[wire_id]).await {
                log::error!("Failed to subscribe to bars: {e:?}");
            }
        });

        Ok(())
    }

    // Unsubscribe paths intentionally do NOT call
    // `unregister_subscription_alias`. The same canonical wire id is shared
    // across multiple data channels (ticker, market_trades, level2,
    // candles), so dropping the entry on the first unsubscribe would cause
    // every still-active channel for the same alias to mistag inbound
    // messages. The mapping is stable per product for the process lifetime
    // and the venue does not deliver messages for products that aren't
    // subscribed to, so leaving it in place is safe.

    fn unsubscribe_instrument(
        &mut self,
        _unsubscription: &UnsubscribeInstrument,
    ) -> anyhow::Result<()> {
        // `subscribe_instrument` only replays cached state; no venue subscription to tear down.
        Ok(())
    }

    fn unsubscribe_book_deltas(
        &mut self,
        unsubscription: &UnsubscribeBookDeltas,
    ) -> anyhow::Result<()> {
        log::debug!(
            "Unsubscribing from book deltas: {}",
            unsubscription.instrument_id
        );

        let ws = self.ws_client.clone();
        let subscribed_id = Self::product_id(unsubscription.instrument_id);
        let wire_id = self.resolve_wire_product_id(subscribed_id);

        get_runtime().spawn(async move {
            if let Err(e) = ws.unsubscribe(CoinbaseWsChannel::Level2, &[wire_id]).await {
                log::error!("Failed to unsubscribe from book deltas: {e:?}");
            }
        });

        Ok(())
    }

    fn unsubscribe_quotes(&mut self, unsubscription: &UnsubscribeQuotes) -> anyhow::Result<()> {
        log::debug!(
            "Unsubscribing from quotes: {}",
            unsubscription.instrument_id
        );

        let ws = self.ws_client.clone();
        let subscribed_id = Self::product_id(unsubscription.instrument_id);
        let wire_id = self.resolve_wire_product_id(subscribed_id);

        get_runtime().spawn(async move {
            if let Err(e) = ws.unsubscribe(CoinbaseWsChannel::Ticker, &[wire_id]).await {
                log::error!("Failed to unsubscribe from quotes: {e:?}");
            }
        });

        Ok(())
    }

    fn unsubscribe_trades(&mut self, unsubscription: &UnsubscribeTrades) -> anyhow::Result<()> {
        log::debug!(
            "Unsubscribing from trades: {}",
            unsubscription.instrument_id
        );

        let ws = self.ws_client.clone();
        let subscribed_id = Self::product_id(unsubscription.instrument_id);
        let wire_id = self.resolve_wire_product_id(subscribed_id);

        get_runtime().spawn(async move {
            if let Err(e) = ws
                .unsubscribe(CoinbaseWsChannel::MarketTrades, &[wire_id])
                .await
            {
                log::error!("Failed to unsubscribe from trades: {e:?}");
            }
        });

        Ok(())
    }

    fn unsubscribe_mark_prices(&mut self, _cmd: &UnsubscribeMarkPrices) -> anyhow::Result<()> {
        Ok(())
    }

    fn unsubscribe_index_prices(&mut self, cmd: &UnsubscribeIndexPrices) -> anyhow::Result<()> {
        log::debug!("Unsubscribing from index prices: {}", cmd.instrument_id);
        self.deriv_polls.unsubscribe_index(cmd.instrument_id);
        Ok(())
    }

    fn unsubscribe_funding_rates(&mut self, cmd: &UnsubscribeFundingRates) -> anyhow::Result<()> {
        log::debug!("Unsubscribing from funding rates: {}", cmd.instrument_id);
        self.deriv_polls.unsubscribe_funding(cmd.instrument_id);
        Ok(())
    }

    fn unsubscribe_bars(&mut self, unsubscription: &UnsubscribeBars) -> anyhow::Result<()> {
        log::debug!("Unsubscribing from bars: {}", unsubscription.bar_type);

        let instrument_id = unsubscription.bar_type.instrument_id();
        let subscribed_id = Self::product_id(instrument_id);
        let wire_id = self.resolve_wire_product_id(subscribed_id);
        let ws = self.ws_client.clone();

        get_runtime().spawn(async move {
            if let Err(e) = ws.unsubscribe(CoinbaseWsChannel::Candles, &[wire_id]).await {
                log::error!("Failed to unsubscribe from bars: {e:?}");
            }
        });

        Ok(())
    }

    fn request_instruments(&self, request: RequestInstruments) -> anyhow::Result<()> {
        log::debug!("Requesting all instruments");

        let provider = self.provider.clone();
        let sender = self.data_sender.clone();
        let instruments_cache = self.instruments.clone();
        let ws = self.ws_client.clone();
        let request_id = request.request_id;
        let client_id = request.client_id.unwrap_or(self.client_id);
        let venue = Self::venue(self);
        let start_nanos = datetime_to_unix_nanos(request.start);
        let end_nanos = datetime_to_unix_nanos(request.end);
        let params = request.params;
        let clock = self.clock;

        get_runtime().spawn(async move {
            match provider.load_all().await {
                Ok(instruments) => {
                    instruments_cache.rcu(|m| {
                        for instrument in &instruments {
                            m.insert(instrument.id(), instrument.clone());
                        }
                    });

                    for instrument in &instruments {
                        ws.update_instrument(instrument.clone()).await;
                    }

                    let response = DataResponse::Instruments(InstrumentsResponse::new(
                        request_id,
                        client_id,
                        venue,
                        instruments,
                        start_nanos,
                        end_nanos,
                        clock.get_time_ns(),
                        params,
                    ));

                    if let Err(e) = sender.send(DataEvent::Response(response)) {
                        log::error!("Failed to send instruments response: {e}");
                    }
                }
                Err(e) => {
                    log::error!("Failed to fetch instruments: {e:?}");
                }
            }
        });

        Ok(())
    }

    fn request_instrument(&self, request: RequestInstrument) -> anyhow::Result<()> {
        log::debug!("Requesting instrument: {}", request.instrument_id);

        let provider = self.provider.clone();
        let sender = self.data_sender.clone();
        let instruments_cache = self.instruments.clone();
        let ws = self.ws_client.clone();
        let instrument_id = request.instrument_id;
        let product_id = instrument_id.symbol.to_string();
        let request_id = request.request_id;
        let client_id = request.client_id.unwrap_or(self.client_id);
        let start_nanos = datetime_to_unix_nanos(request.start);
        let end_nanos = datetime_to_unix_nanos(request.end);
        let params = request.params;
        let clock = self.clock;

        get_runtime().spawn(async move {
            match provider.load(&product_id).await {
                Ok(instrument) => {
                    instruments_cache.rcu(|m| {
                        m.insert(instrument.id(), instrument.clone());
                    });
                    ws.update_instrument(instrument.clone()).await;

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
                }
                Err(e) => {
                    log::error!("Failed to fetch instrument {instrument_id}: {e:?}");
                }
            }
        });

        Ok(())
    }

    fn request_book_snapshot(&self, request: RequestBookSnapshot) -> anyhow::Result<()> {
        let instrument_id = request.instrument_id;
        let product_id = instrument_id.symbol.to_string();

        let instruments = self.instruments.load();
        let instrument = instruments
            .get(&instrument_id)
            .ok_or_else(|| anyhow::anyhow!("Instrument {instrument_id} not found"))?;
        let price_precision = instrument.price_precision();
        let size_precision = instrument.size_precision();
        let depth = request.depth.map(|d| d.get() as u32);

        let http = self.http_client.clone();
        let sender = self.data_sender.clone();
        let client_id = request.client_id.unwrap_or(self.client_id);
        let request_id = request.request_id;
        let params = request.params;
        let clock = self.clock;

        get_runtime().spawn(async move {
            match http.get_product_book(&product_id, depth).await {
                Ok(json) => {
                    let pricebook_value = json.get("pricebook").cloned().unwrap_or(json);

                    let pricebook: PriceBook = match serde_json::from_value(pricebook_value) {
                        Ok(b) => b,
                        Err(e) => {
                            log::error!("Failed to parse product book: {e}");
                            return;
                        }
                    };

                    let ts_init = clock.get_time_ns();

                    match parse_product_book_snapshot(
                        &pricebook,
                        instrument_id,
                        price_precision,
                        size_precision,
                        ts_init,
                    ) {
                        Ok(deltas) => {
                            let mut book = OrderBook::new(instrument_id, BookType::L2_MBP);

                            for delta in &deltas.deltas {
                                if delta.order.side != OrderSide::NoOrderSide {
                                    book.add(
                                        delta.order,
                                        delta.flags,
                                        delta.sequence,
                                        delta.ts_event,
                                    );
                                }
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
                        Err(e) => {
                            log::error!("Failed to parse book snapshot for {instrument_id}: {e}");
                        }
                    }
                }
                Err(e) => {
                    log::error!("Book snapshot request failed for {instrument_id}: {e:?}");
                }
            }
        });

        Ok(())
    }

    fn request_trades(&self, request: RequestTrades) -> anyhow::Result<()> {
        log::debug!("Requesting trades for {}", request.instrument_id);

        let instrument_id = request.instrument_id;
        let product_id = instrument_id.symbol.to_string();

        let instruments = self.instruments.load();
        let instrument = instruments
            .get(&instrument_id)
            .ok_or_else(|| anyhow::anyhow!("Instrument {instrument_id} not found"))?;
        let price_precision = instrument.price_precision();
        let size_precision = instrument.size_precision();

        let http = self.http_client.clone();
        let sender = self.data_sender.clone();
        let request_id = request.request_id;
        let client_id = request.client_id.unwrap_or(self.client_id);
        let limit = request.limit.map_or(100, |n| n.get() as u32);
        let start_nanos = datetime_to_unix_nanos(request.start);
        let end_nanos = datetime_to_unix_nanos(request.end);
        let params = request.params;
        let clock = self.clock;

        get_runtime().spawn(async move {
            match http.get_market_trades(&product_id, limit).await {
                Ok(json) => {
                    let ticker: TickerResponse = match serde_json::from_value(json) {
                        Ok(r) => r,
                        Err(e) => {
                            log::error!("Failed to parse trades response: {e}");
                            return;
                        }
                    };

                    let ts_init = clock.get_time_ns();
                    let mut trades: Vec<_> = ticker
                        .trades
                        .iter()
                        .filter_map(|trade| {
                            parse_trade_tick(
                                trade,
                                instrument_id,
                                price_precision,
                                size_precision,
                                ts_init,
                            )
                            .map_err(|e| log::warn!("Failed to parse trade: {e}"))
                            .ok()
                        })
                        .collect();

                    // Coinbase returns newest-first; sort ascending
                    trades.sort_by_key(|t| t.ts_event);

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
                Err(e) => log::error!("Trades request failed for {instrument_id}: {e:?}"),
            }
        });

        Ok(())
    }

    fn request_bars(&self, request: RequestBars) -> anyhow::Result<()> {
        log::debug!("Requesting bars for {}", request.bar_type);

        let bar_type = request.bar_type;
        let granularity = bar_type_to_granularity(&bar_type)?;
        let instrument_id = bar_type.instrument_id();
        let product_id = instrument_id.symbol.to_string();

        let instruments = self.instruments.load();
        let instrument = instruments
            .get(&instrument_id)
            .ok_or_else(|| anyhow::anyhow!("Instrument {instrument_id} not found"))?;
        let price_precision = instrument.price_precision();
        let size_precision = instrument.size_precision();

        let http = self.http_client.clone();
        let sender = self.data_sender.clone();
        let request_id = request.request_id;
        let client_id = request.client_id.unwrap_or(self.client_id);
        let start = request.start;
        let end = request.end;
        let limit = request.limit.map(|n| n.get());
        let start_nanos = datetime_to_unix_nanos(start);
        let end_nanos = datetime_to_unix_nanos(end);
        let params = request.params;
        let clock = self.clock;

        get_runtime().spawn(async move {
            let now = chrono::Utc::now();
            let end_secs = end.unwrap_or(now).timestamp().to_string();
            let start_secs = if let Some(s) = start {
                s.timestamp().to_string()
            } else {
                let spec = bar_type.spec();
                let step_secs = match spec.aggregation {
                    BarAggregation::Minute => spec.step.get() as i64 * 60,
                    BarAggregation::Hour => spec.step.get() as i64 * 3600,
                    BarAggregation::Day => spec.step.get() as i64 * 86400,
                    _ => 60,
                };
                let count = limit.unwrap_or(300) as i64;
                let end_ts = end.unwrap_or(now).timestamp();
                (end_ts - count * step_secs).to_string()
            };

            let granularity_str = granularity.to_string();

            match http
                .get_candles(&product_id, &start_secs, &end_secs, &granularity_str)
                .await
            {
                Ok(json) => {
                    let candles_response: CandlesResponse = match serde_json::from_value(json) {
                        Ok(r) => r,
                        Err(e) => {
                            log::error!("Failed to parse candles response: {e}");
                            return;
                        }
                    };

                    let ts_init = clock.get_time_ns();
                    let mut bars: Vec<_> = candles_response
                        .candles
                        .iter()
                        .filter_map(|candle| {
                            parse_bar(candle, bar_type, price_precision, size_precision, ts_init)
                                .map_err(|e| log::warn!("Failed to parse bar: {e}"))
                                .ok()
                        })
                        .collect();

                    bars.sort_by_key(|b| b.ts_event);

                    if let Some(limit) = limit
                        && bars.len() > limit
                    {
                        bars.drain(..bars.len() - limit);
                    }

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
}

#[cfg(test)]
mod tests {
    use nautilus_common::{
        live::runner::set_data_event_sender, messages::data::SubscribeMarkPrices,
    };
    use nautilus_core::{UUID4, UnixNanos};
    use nautilus_model::identifiers::InstrumentId;
    use rstest::rstest;

    use super::*;

    // Coinbase Advanced Trade does not publish live mark prices for its
    // perpetuals, so `subscribe_mark_prices` must return an explicit error
    // naming the instrument and mentioning mark prices. A regression that
    // silently `Ok(())`s the call would mask the unsupported feature.
    #[rstest]
    #[tokio::test]
    async fn test_subscribe_mark_prices_rejects_with_explicit_error() {
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        set_data_event_sender(tx);

        let config = CoinbaseDataClientConfig::default();
        let mut client = CoinbaseDataClient::new(ClientId::new("COINBASE"), config)
            .expect("client construction");

        let instrument_id = InstrumentId::from("BIP-20DEC30-CDE.COINBASE");
        let cmd = SubscribeMarkPrices::new(
            instrument_id,
            Some(ClientId::new("COINBASE")),
            None,
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        );

        let err = client
            .subscribe_mark_prices(cmd)
            .expect_err("must reject mark-price subscriptions");
        let msg = err.to_string();
        assert!(
            msg.contains("mark prices"),
            "error must mention mark prices, was: {msg}"
        );
        assert!(
            msg.contains("BIP-20DEC30-CDE.COINBASE"),
            "error must name the instrument, was: {msg}"
        );
    }
}
