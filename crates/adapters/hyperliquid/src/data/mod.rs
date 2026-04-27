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

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use ahash::AHashMap;
use anyhow::Context;
use chrono::{DateTime, Utc};
use nautilus_common::{
    clients::DataClient,
    live::{runner::get_data_event_sender, runtime::get_runtime},
    messages::{
        DataEvent,
        data::{
            BarsResponse, BookResponse, DataResponse, FundingRatesResponse, InstrumentResponse,
            InstrumentsResponse, RequestBars, RequestBookSnapshot, RequestFundingRates,
            RequestInstrument, RequestInstruments, RequestTrades, SubscribeBars,
            SubscribeBookDeltas, SubscribeBookDepth10, SubscribeFundingRates, SubscribeIndexPrices,
            SubscribeInstrument, SubscribeMarkPrices, SubscribeQuotes, SubscribeTrades,
            UnsubscribeBars, UnsubscribeBookDeltas, UnsubscribeBookDepth10,
            UnsubscribeFundingRates, UnsubscribeIndexPrices, UnsubscribeMarkPrices,
            UnsubscribeQuotes, UnsubscribeTrades,
        },
    },
};
use nautilus_core::{
    AtomicMap, Params, UnixNanos,
    datetime::datetime_to_unix_nanos,
    time::{AtomicTime, get_atomic_clock_realtime},
};
use nautilus_model::{
    data::{Bar, BarType, BookOrder, Data, FundingRateUpdate, OrderBookDeltas_API},
    enums::{BarAggregation, BookType, OrderSide},
    identifiers::{ClientId, InstrumentId, Venue},
    instruments::{Instrument, InstrumentAny},
    orderbook::OrderBook,
    types::{Price, Quantity},
};
use rust_decimal::Decimal;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use ustr::Ustr;

use crate::{
    common::{
        consts::HYPERLIQUID_VENUE,
        credential::{Secrets, credential_env_vars},
        parse::bar_type_to_interval,
    },
    config::HyperliquidDataClientConfig,
    http::{
        client::HyperliquidHttpClient,
        models::{HyperliquidCandle, HyperliquidFundingHistoryEntry},
    },
    websocket::{
        client::HyperliquidWebSocketClient,
        messages::{HyperliquidWsMessage, NautilusWsMessage},
        parse::{
            parse_ws_candle, parse_ws_order_book_deltas, parse_ws_quote_tick, parse_ws_trade_tick,
        },
    },
};

#[derive(Debug)]
pub struct HyperliquidDataClient {
    client_id: ClientId,
    #[allow(dead_code)]
    config: HyperliquidDataClientConfig,
    http_client: HyperliquidHttpClient,
    ws_client: HyperliquidWebSocketClient,
    is_connected: AtomicBool,
    cancellation_token: CancellationToken,
    tasks: Vec<JoinHandle<()>>,
    data_sender: tokio::sync::mpsc::UnboundedSender<DataEvent>,
    instruments: Arc<AtomicMap<InstrumentId, InstrumentAny>>,
    // Maps coin symbols (e.g., "BTC") to instrument IDs (e.g., "BTC-PERP")
    coin_to_instrument_id: Arc<AtomicMap<Ustr, InstrumentId>>,
    clock: &'static AtomicTime,
    #[allow(dead_code)]
    instrument_refresh_active: bool,
}

impl HyperliquidDataClient {
    /// Creates a new [`HyperliquidDataClient`] instance.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP client fails to initialize.
    pub fn new(client_id: ClientId, config: HyperliquidDataClientConfig) -> anyhow::Result<Self> {
        let clock = get_atomic_clock_realtime();
        let data_sender = get_data_event_sender();

        // Only fall back to unauthenticated when credentials are absent,
        // not when they're invalid (fail fast on malformed keys)
        let (pk_var, _) = credential_env_vars(config.environment);
        let has_credentials = config.has_credentials() || std::env::var(pk_var).is_ok();

        let mut http_client = if has_credentials {
            let secrets =
                Secrets::resolve(config.private_key.as_deref(), None, config.environment)?;
            HyperliquidHttpClient::with_secrets(
                &secrets,
                config.http_timeout_secs,
                config.proxy_url.clone(),
            )?
        } else {
            HyperliquidHttpClient::new(
                config.environment,
                config.http_timeout_secs,
                config.proxy_url.clone(),
            )?
        };

        // Apply URL overrides from config (used for testing with mock servers)
        if let Some(url) = &config.base_url_http {
            http_client.set_base_info_url(url.clone());
        }

        let ws_url = config.base_url_ws.clone();
        let ws_client = HyperliquidWebSocketClient::new(
            ws_url,
            config.environment,
            None,
            config.transport_backend,
            config.proxy_url.clone(),
        );

        Ok(Self {
            client_id,
            config,
            http_client,
            ws_client,
            is_connected: AtomicBool::new(false),
            cancellation_token: CancellationToken::new(),
            tasks: Vec::new(),
            data_sender,
            instruments: Arc::new(AtomicMap::new()),
            coin_to_instrument_id: Arc::new(AtomicMap::new()),
            clock,
            instrument_refresh_active: false,
        })
    }

    fn venue(&self) -> Venue {
        *HYPERLIQUID_VENUE
    }

    async fn bootstrap_instruments(&self) -> anyhow::Result<Vec<InstrumentAny>> {
        let instruments = self
            .http_client
            .request_instruments()
            .await
            .context("failed to fetch instruments during bootstrap")?;

        self.instruments.rcu(|m| {
            for instrument in &instruments {
                m.insert(instrument.id(), instrument.clone());
            }
        });

        self.coin_to_instrument_id.rcu(|m| {
            for instrument in &instruments {
                m.insert(instrument.raw_symbol().inner(), instrument.id());
            }
        });

        for instrument in &instruments {
            self.ws_client.cache_instrument(instrument.clone());
        }

        log::info!(
            "Bootstrapped {} instruments with {} coin mappings",
            self.instruments.len(),
            self.coin_to_instrument_id.len()
        );
        Ok(instruments)
    }

    async fn spawn_ws(&mut self) -> anyhow::Result<()> {
        // Clone client before connecting so the clone can have out_rx set
        let mut ws_client = self.ws_client.clone();

        ws_client
            .connect()
            .await
            .context("failed to connect to Hyperliquid WebSocket")?;

        // Transfer task handle to original so disconnect() can await it
        if let Some(handle) = ws_client.take_task_handle() {
            self.ws_client.set_task_handle(handle);
        }

        let data_sender = self.data_sender.clone();
        let cancellation_token = self.cancellation_token.clone();

        let task = get_runtime().spawn(async move {
            log::info!("Hyperliquid WebSocket consumption loop started");

            loop {
                tokio::select! {
                    () = cancellation_token.cancelled() => {
                        log::info!("WebSocket consumption loop cancelled");
                        break;
                    }
                    msg_opt = ws_client.next_event() => {
                        if let Some(msg) = msg_opt {
                            match msg {
                                NautilusWsMessage::Trades(trades) => {
                                    for trade in trades {
                                        if let Err(e) = data_sender
                                            .send(DataEvent::Data(Data::Trade(trade)))
                                        {
                                            log::error!("Failed to send trade tick: {e}");
                                        }
                                    }
                                }
                                NautilusWsMessage::Quote(quote) => {
                                    if let Err(e) = data_sender
                                        .send(DataEvent::Data(Data::Quote(quote)))
                                    {
                                        log::error!("Failed to send quote tick: {e}");
                                    }
                                }
                                NautilusWsMessage::Deltas(deltas) => {
                                    if let Err(e) = data_sender
                                        .send(DataEvent::Data(Data::Deltas(
                                            OrderBookDeltas_API::new(deltas),
                                        )))
                                    {
                                        log::error!("Failed to send order book deltas: {e}");
                                    }
                                }
                                NautilusWsMessage::Depth10(depth) => {
                                    if let Err(e) =
                                        data_sender.send(DataEvent::Data(Data::Depth10(depth)))
                                    {
                                        log::error!("Failed to send order book depth10: {e}");
                                    }
                                }
                                NautilusWsMessage::Candle(bar) => {
                                    if let Err(e) = data_sender
                                        .send(DataEvent::Data(Data::Bar(bar)))
                                    {
                                        log::error!("Failed to send bar: {e}");
                                    }
                                }
                                NautilusWsMessage::MarkPrice(update) => {
                                    if let Err(e) = data_sender
                                        .send(DataEvent::Data(Data::MarkPriceUpdate(update)))
                                    {
                                        log::error!("Failed to send mark price update: {e}");
                                    }
                                }
                                NautilusWsMessage::IndexPrice(update) => {
                                    if let Err(e) = data_sender
                                        .send(DataEvent::Data(Data::IndexPriceUpdate(update)))
                                    {
                                        log::error!("Failed to send index price update: {e}");
                                    }
                                }
                                NautilusWsMessage::FundingRate(update) => {
                                    if let Err(e) = data_sender
                                        .send(DataEvent::FundingRate(update))
                                    {
                                        log::error!("Failed to send funding rate update: {e}");
                                    }
                                }
                                NautilusWsMessage::Reconnected => {
                                    log::info!("WebSocket reconnected");
                                }
                                NautilusWsMessage::Error(e) => {
                                    log::error!("WebSocket error: {e}");
                                }
                                NautilusWsMessage::ExecutionReports(_) => {
                                    // Handled by execution client
                                }
                            }
                        } else {
                            // Connection closed or error
                            log::debug!("WebSocket next_event returned None, stream closed");
                            tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
                        }
                    }
                }
            }

            log::info!("Hyperliquid WebSocket consumption loop finished");
        });

        self.tasks.push(task);
        log::info!("WebSocket consumption task spawned");

        Ok(())
    }

    #[allow(dead_code)]
    fn handle_ws_message(
        msg: HyperliquidWsMessage,
        ws_client: &HyperliquidWebSocketClient,
        data_sender: &tokio::sync::mpsc::UnboundedSender<DataEvent>,
        instruments: &Arc<AtomicMap<InstrumentId, InstrumentAny>>,
        coin_to_instrument_id: &Arc<AtomicMap<Ustr, InstrumentId>>,
        _venue: Venue,
        clock: &'static AtomicTime,
    ) {
        match msg {
            HyperliquidWsMessage::Bbo { data } => {
                let coin = data.coin;
                log::debug!("Received BBO message for coin: {coin}");

                let coin_map = coin_to_instrument_id.load();
                let instrument_id = coin_map.get(&data.coin);

                if let Some(&instrument_id) = instrument_id {
                    let instruments_map = instruments.load();
                    if let Some(instrument) = instruments_map.get(&instrument_id) {
                        let ts_init = clock.get_time_ns();

                        match parse_ws_quote_tick(&data, instrument, ts_init) {
                            Ok(quote_tick) => {
                                log::debug!(
                                    "Parsed quote tick for {}: bid={}, ask={}",
                                    data.coin,
                                    quote_tick.bid_price,
                                    quote_tick.ask_price
                                );

                                if let Err(e) =
                                    data_sender.send(DataEvent::Data(Data::Quote(quote_tick)))
                                {
                                    log::error!("Failed to send quote tick: {e}");
                                }
                            }
                            Err(e) => {
                                log::error!("Failed to parse quote tick for {}: {e}", data.coin);
                            }
                        }
                    }
                } else {
                    log::warn!(
                        "Received BBO for unknown coin: {} (no matching instrument found)",
                        data.coin
                    );
                }
            }
            HyperliquidWsMessage::Trades { data } => {
                let count = data.len();
                log::debug!("Received {count} trade(s)");

                for trade_data in data {
                    let coin = trade_data.coin;
                    let coin_map = coin_to_instrument_id.load();

                    if let Some(&instrument_id) = coin_map.get(&coin) {
                        let instruments_map = instruments.load();
                        if let Some(instrument) = instruments_map.get(&instrument_id) {
                            let ts_init = clock.get_time_ns();

                            match parse_ws_trade_tick(&trade_data, instrument, ts_init) {
                                Ok(trade_tick) => {
                                    if let Err(e) =
                                        data_sender.send(DataEvent::Data(Data::Trade(trade_tick)))
                                    {
                                        log::error!("Failed to send trade tick: {e}");
                                    }
                                }
                                Err(e) => {
                                    log::error!("Failed to parse trade tick for {coin}: {e}");
                                }
                            }
                        }
                    } else {
                        log::warn!("Received trade for unknown coin: {coin}");
                    }
                }
            }
            HyperliquidWsMessage::L2Book { data } => {
                let coin = data.coin;
                log::debug!("Received L2 book update for coin: {coin}");

                let coin_map = coin_to_instrument_id.load();
                if let Some(&instrument_id) = coin_map.get(&data.coin) {
                    let instruments_map = instruments.load();
                    if let Some(instrument) = instruments_map.get(&instrument_id) {
                        let ts_init = clock.get_time_ns();

                        match parse_ws_order_book_deltas(&data, instrument, ts_init) {
                            Ok(deltas) => {
                                if let Err(e) = data_sender.send(DataEvent::Data(Data::Deltas(
                                    OrderBookDeltas_API::new(deltas),
                                ))) {
                                    log::error!("Failed to send order book deltas: {e}");
                                }
                            }
                            Err(e) => {
                                log::error!(
                                    "Failed to parse order book deltas for {}: {e}",
                                    data.coin
                                );
                            }
                        }
                    }
                } else {
                    log::warn!("Received L2 book for unknown coin: {coin}");
                }
            }
            HyperliquidWsMessage::Candle { data } => {
                let coin = &data.s;
                let interval = &data.i;
                log::debug!("Received candle for {coin}:{interval}");

                if let Some(bar_type) = ws_client.get_bar_type(&data.s, &data.i) {
                    let coin = Ustr::from(&data.s);
                    let coin_map = coin_to_instrument_id.load();

                    if let Some(&instrument_id) = coin_map.get(&coin) {
                        let instruments_map = instruments.load();
                        if let Some(instrument) = instruments_map.get(&instrument_id) {
                            let ts_init = clock.get_time_ns();

                            match parse_ws_candle(&data, instrument, &bar_type, ts_init) {
                                Ok(bar) => {
                                    if let Err(e) =
                                        data_sender.send(DataEvent::Data(Data::Bar(bar)))
                                    {
                                        log::error!("Failed to send bar data: {e}");
                                    }
                                }
                                Err(e) => {
                                    log::error!("Failed to parse candle for {coin}: {e}");
                                }
                            }
                        }
                    } else {
                        log::warn!("Received candle for unknown coin: {coin}");
                    }
                } else {
                    log::debug!("Received candle for {coin}:{interval} but no BarType tracked");
                }
            }
            _ => {
                log::trace!("Received unhandled WebSocket message: {msg:?}");
            }
        }
    }
}

impl HyperliquidDataClient {
    #[allow(dead_code)]
    fn send_data(sender: &tokio::sync::mpsc::UnboundedSender<DataEvent>, data: Data) {
        if let Err(e) = sender.send(DataEvent::Data(data)) {
            log::error!("Failed to emit data event: {e}");
        }
    }
}

#[async_trait::async_trait(?Send)]
impl DataClient for HyperliquidDataClient {
    fn client_id(&self) -> ClientId {
        self.client_id
    }

    fn venue(&self) -> Option<Venue> {
        Some(self.venue())
    }

    fn start(&mut self) -> anyhow::Result<()> {
        log::info!(
            "Starting Hyperliquid data client: client_id={}, environment={:?}, proxy_url={:?}",
            self.client_id,
            self.config.environment,
            self.config.proxy_url,
        );
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        log::info!("Stopping Hyperliquid data client {}", self.client_id);
        self.cancellation_token.cancel();
        self.is_connected.store(false, Ordering::Relaxed);
        Ok(())
    }

    fn reset(&mut self) -> anyhow::Result<()> {
        log::debug!("Resetting Hyperliquid data client {}", self.client_id);
        self.is_connected.store(false, Ordering::Relaxed);
        self.cancellation_token = CancellationToken::new();
        self.tasks.clear();
        Ok(())
    }

    fn dispose(&mut self) -> anyhow::Result<()> {
        log::debug!("Disposing Hyperliquid data client {}", self.client_id);
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

        self.is_connected.store(true, Ordering::Relaxed);
        log::info!("Connected: client_id={}", self.client_id);

        Ok(())
    }

    async fn disconnect(&mut self) -> anyhow::Result<()> {
        if !self.is_connected() {
            return Ok(());
        }

        self.cancellation_token.cancel();

        for task in self.tasks.drain(..) {
            if let Err(e) = task.await {
                log::error!("Error waiting for task to complete: {e}");
            }
        }

        if let Err(e) = self.ws_client.disconnect().await {
            log::error!("Error disconnecting WebSocket client: {e}");
        }

        self.instruments.store(AHashMap::new());

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
            anyhow::bail!("Hyperliquid only supports L2_MBP order book deltas");
        }

        let ws = self.ws_client.clone();
        let instrument_id = subscription.instrument_id;
        let (n_sig_figs, mantissa) = parse_book_precision_params(subscription.params.as_ref())?;

        get_runtime().spawn(async move {
            if let Err(e) = ws
                .subscribe_book_with_options(instrument_id, n_sig_figs, mantissa)
                .await
            {
                log::error!("Failed to subscribe to book deltas: {e:?}");
            }
        });

        Ok(())
    }

    fn subscribe_book_depth10(&mut self, subscription: SubscribeBookDepth10) -> anyhow::Result<()> {
        log::debug!(
            "Subscribing to book depth10: {}",
            subscription.instrument_id
        );

        if subscription.book_type != BookType::L2_MBP {
            anyhow::bail!("Hyperliquid only supports L2_MBP order book depth10");
        }

        let ws = self.ws_client.clone();
        let instrument_id = subscription.instrument_id;
        let (n_sig_figs, mantissa) = parse_book_precision_params(subscription.params.as_ref())?;

        get_runtime().spawn(async move {
            if let Err(e) = ws
                .subscribe_book_depth10_with_options(instrument_id, n_sig_figs, mantissa)
                .await
            {
                log::error!("Failed to subscribe to book depth10: {e:?}");
            }
        });

        Ok(())
    }

    fn subscribe_quotes(&mut self, subscription: SubscribeQuotes) -> anyhow::Result<()> {
        log::debug!("Subscribing to quotes: {}", subscription.instrument_id);

        let ws = self.ws_client.clone();
        let instrument_id = subscription.instrument_id;

        get_runtime().spawn(async move {
            if let Err(e) = ws.subscribe_quotes(instrument_id).await {
                log::error!("Failed to subscribe to quotes: {e:?}");
            }
        });

        Ok(())
    }

    fn subscribe_trades(&mut self, subscription: SubscribeTrades) -> anyhow::Result<()> {
        log::debug!("Subscribing to trades: {}", subscription.instrument_id);

        let ws = self.ws_client.clone();
        let instrument_id = subscription.instrument_id;

        get_runtime().spawn(async move {
            if let Err(e) = ws.subscribe_trades(instrument_id).await {
                log::error!("Failed to subscribe to trades: {e:?}");
            }
        });

        Ok(())
    }

    fn subscribe_mark_prices(&mut self, cmd: SubscribeMarkPrices) -> anyhow::Result<()> {
        let ws = self.ws_client.clone();
        let instrument_id = cmd.instrument_id;

        get_runtime().spawn(async move {
            if let Err(e) = ws.subscribe_mark_prices(instrument_id).await {
                log::error!("Failed to subscribe to mark prices: {e:?}");
            }
        });

        Ok(())
    }

    fn subscribe_index_prices(&mut self, cmd: SubscribeIndexPrices) -> anyhow::Result<()> {
        let ws = self.ws_client.clone();
        let instrument_id = cmd.instrument_id;

        get_runtime().spawn(async move {
            if let Err(e) = ws.subscribe_index_prices(instrument_id).await {
                log::error!("Failed to subscribe to index prices: {e:?}");
            }
        });

        Ok(())
    }

    fn subscribe_funding_rates(&mut self, cmd: SubscribeFundingRates) -> anyhow::Result<()> {
        let ws = self.ws_client.clone();
        let instrument_id = cmd.instrument_id;

        get_runtime().spawn(async move {
            if let Err(e) = ws.subscribe_funding_rates(instrument_id).await {
                log::error!("Failed to subscribe to funding rates: {e:?}");
            }
        });

        Ok(())
    }

    fn subscribe_bars(&mut self, subscription: SubscribeBars) -> anyhow::Result<()> {
        log::debug!("Subscribing to bars: {}", subscription.bar_type);

        let instrument_id = subscription.bar_type.instrument_id();
        if !self.instruments.contains_key(&instrument_id) {
            anyhow::bail!("Instrument {instrument_id} not found");
        }

        let bar_type = subscription.bar_type;
        let ws = self.ws_client.clone();

        get_runtime().spawn(async move {
            if let Err(e) = ws.subscribe_bars(bar_type).await {
                log::error!("Failed to subscribe to bars: {e:?}");
            }
        });

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
        let instrument_id = unsubscription.instrument_id;

        get_runtime().spawn(async move {
            if let Err(e) = ws.unsubscribe_book(instrument_id).await {
                log::error!("Failed to unsubscribe from book deltas: {e:?}");
            }
        });

        Ok(())
    }

    fn unsubscribe_book_depth10(
        &mut self,
        unsubscription: &UnsubscribeBookDepth10,
    ) -> anyhow::Result<()> {
        log::debug!(
            "Unsubscribing from book depth10: {}",
            unsubscription.instrument_id
        );

        let ws = self.ws_client.clone();
        let instrument_id = unsubscription.instrument_id;

        get_runtime().spawn(async move {
            if let Err(e) = ws.unsubscribe_book_depth10(instrument_id).await {
                log::error!("Failed to unsubscribe from book depth10: {e:?}");
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
        let instrument_id = unsubscription.instrument_id;

        get_runtime().spawn(async move {
            if let Err(e) = ws.unsubscribe_quotes(instrument_id).await {
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
        let instrument_id = unsubscription.instrument_id;

        get_runtime().spawn(async move {
            if let Err(e) = ws.unsubscribe_trades(instrument_id).await {
                log::error!("Failed to unsubscribe from trades: {e:?}");
            }
        });

        Ok(())
    }

    fn unsubscribe_mark_prices(&mut self, cmd: &UnsubscribeMarkPrices) -> anyhow::Result<()> {
        let ws = self.ws_client.clone();
        let instrument_id = cmd.instrument_id;

        get_runtime().spawn(async move {
            if let Err(e) = ws.unsubscribe_mark_prices(instrument_id).await {
                log::error!("Failed to unsubscribe from mark prices: {e:?}");
            }
        });

        Ok(())
    }

    fn unsubscribe_index_prices(&mut self, cmd: &UnsubscribeIndexPrices) -> anyhow::Result<()> {
        let ws = self.ws_client.clone();
        let instrument_id = cmd.instrument_id;

        get_runtime().spawn(async move {
            if let Err(e) = ws.unsubscribe_index_prices(instrument_id).await {
                log::error!("Failed to unsubscribe from index prices: {e:?}");
            }
        });

        Ok(())
    }

    fn unsubscribe_funding_rates(&mut self, cmd: &UnsubscribeFundingRates) -> anyhow::Result<()> {
        let ws = self.ws_client.clone();
        let instrument_id = cmd.instrument_id;

        get_runtime().spawn(async move {
            if let Err(e) = ws.unsubscribe_funding_rates(instrument_id).await {
                log::error!("Failed to unsubscribe from funding rates: {e:?}");
            }
        });

        Ok(())
    }

    fn unsubscribe_bars(&mut self, unsubscription: &UnsubscribeBars) -> anyhow::Result<()> {
        log::debug!("Unsubscribing from bars: {}", unsubscription.bar_type);

        let bar_type = unsubscription.bar_type;
        let ws = self.ws_client.clone();

        get_runtime().spawn(async move {
            if let Err(e) = ws.unsubscribe_bars(bar_type).await {
                log::error!("Failed to unsubscribe from bars: {e:?}");
            }
        });

        Ok(())
    }

    fn request_instruments(&self, request: RequestInstruments) -> anyhow::Result<()> {
        log::debug!("Requesting all instruments");

        let http = self.http_client.clone();
        let sender = self.data_sender.clone();
        let instruments_cache = self.instruments.clone();
        let coin_map = self.coin_to_instrument_id.clone();
        let ws_instruments = self.ws_client.instruments_cache();
        let request_id = request.request_id;
        let client_id = request.client_id.unwrap_or(self.client_id);
        let venue = self.venue();
        let start_nanos = datetime_to_unix_nanos(request.start);
        let end_nanos = datetime_to_unix_nanos(request.end);
        let params = request.params;
        let clock = self.clock;

        get_runtime().spawn(async move {
            match http.request_instruments().await {
                Ok(instruments) => {
                    instruments_cache.rcu(|instruments_map| {
                        coin_map.rcu(|coin_to_id| {
                            for instrument in &instruments {
                                let instrument_id = instrument.id();
                                instruments_map.insert(instrument_id, instrument.clone());
                                let coin = instrument.raw_symbol().inner();
                                coin_to_id.insert(coin, instrument_id);
                                ws_instruments.insert(coin, instrument.clone());
                            }
                        });
                    });

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
                    log::error!("Failed to fetch instruments from Hyperliquid: {e:?}");
                }
            }
        });

        Ok(())
    }

    fn request_instrument(&self, request: RequestInstrument) -> anyhow::Result<()> {
        log::debug!("Requesting instrument: {}", request.instrument_id);

        let http = self.http_client.clone();
        let sender = self.data_sender.clone();
        let instruments_cache = self.instruments.clone();
        let coin_map = self.coin_to_instrument_id.clone();
        let ws_instruments = self.ws_client.instruments_cache();
        let instrument_id = request.instrument_id;
        let request_id = request.request_id;
        let client_id = request.client_id.unwrap_or(self.client_id);
        let start_nanos = datetime_to_unix_nanos(request.start);
        let end_nanos = datetime_to_unix_nanos(request.end);
        let params = request.params;
        let clock = self.clock;

        get_runtime().spawn(async move {
            match http.request_instruments().await {
                Ok(all_instruments) => {
                    instruments_cache.rcu(|instruments_map| {
                        coin_map.rcu(|coin_to_id| {
                            for instrument in &all_instruments {
                                let id = instrument.id();
                                instruments_map.insert(id, instrument.clone());
                                let coin = instrument.raw_symbol().inner();
                                coin_to_id.insert(coin, id);
                                ws_instruments.insert(coin, instrument.clone());
                            }
                        });
                    });

                    if let Some(instrument) = all_instruments
                        .into_iter()
                        .find(|i| i.id() == instrument_id)
                    {
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
                Err(e) => {
                    log::error!("Failed to fetch instruments from Hyperliquid: {e:?}");
                }
            }
        });

        Ok(())
    }

    fn request_bars(&self, request: RequestBars) -> anyhow::Result<()> {
        log::debug!("Requesting bars for {}", request.bar_type);

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
        let instruments = Arc::clone(&self.instruments);

        get_runtime().spawn(async move {
            match request_bars_from_http(http, bar_type, start, end, limit, instruments).await {
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

    fn request_trades(&self, request: RequestTrades) -> anyhow::Result<()> {
        // Hyperliquid has no public trade-tape REST endpoint; real-time
        // trades are available via the `trades` WebSocket channel and
        // account-scoped fills via `userFills`/`userFillsByTime`, but
        // market-wide trade history cannot be served.
        anyhow::bail!(
            "Historical trade requests are not supported by Hyperliquid for {}; \
             subscribe to trades via WebSocket for live trade ticks",
            request.instrument_id,
        )
    }

    fn request_funding_rates(&self, request: RequestFundingRates) -> anyhow::Result<()> {
        let instrument_id = request.instrument_id;
        log::debug!("Requesting funding rates for {instrument_id}");

        let instruments = self.instruments.load();
        let instrument = instruments
            .get(&instrument_id)
            .ok_or_else(|| anyhow::anyhow!("Instrument {instrument_id} not found"))?;

        if !matches!(instrument, InstrumentAny::CryptoPerpetual(_)) {
            anyhow::bail!("Funding rates are only available for perpetual instruments");
        }

        let coin = instrument.raw_symbol().to_string();
        let http = self.http_client.clone();
        let sender = self.data_sender.clone();
        let client_id = request.client_id.unwrap_or(self.client_id);
        let request_id = request.request_id;
        let params = request.params;
        let clock = self.clock;
        let limit = request.limit.map(|n| n.get());
        let start_dt = request.start;
        let end_dt = request.end;
        let start_nanos = datetime_to_unix_nanos(start_dt);
        let end_nanos = datetime_to_unix_nanos(end_dt);

        let now_ms = Utc::now().timestamp_millis() as u64;

        // Hyperliquid requires a startTime; default to a 7-day lookback when none given
        let default_lookback_ms: u64 = 7 * 86_400_000;
        let start_ms = match start_dt {
            Some(dt) => dt.timestamp_millis().max(0) as u64,
            None => now_ms.saturating_sub(default_lookback_ms),
        };
        let end_ms = end_dt.map(|dt| dt.timestamp_millis().max(0) as u64);

        get_runtime().spawn(async move {
            match http.info_funding_history(&coin, start_ms, end_ms).await {
                Ok(entries) => {
                    let mut funding_rates: Vec<FundingRateUpdate> = entries
                        .iter()
                        .filter_map(
                            |entry| match funding_entry_to_update(entry, instrument_id) {
                                Ok(update) => Some(update),
                                Err(e) => {
                                    log::warn!(
                                        "Skipping funding history entry for {instrument_id}: {e}",
                                    );
                                    None
                                }
                            },
                        )
                        .collect();

                    if let Some(limit) = limit
                        && funding_rates.len() > limit
                    {
                        funding_rates.truncate(limit);
                    }

                    log::debug!(
                        "Fetched {} funding rates for {instrument_id}",
                        funding_rates.len(),
                    );

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

    fn request_book_snapshot(&self, request: RequestBookSnapshot) -> anyhow::Result<()> {
        let instrument_id = request.instrument_id;
        let instruments = self.instruments.load();
        let instrument = instruments
            .get(&instrument_id)
            .ok_or_else(|| anyhow::anyhow!("Instrument {instrument_id} not found"))?;

        let raw_symbol = instrument.raw_symbol().to_string();
        let price_precision = instrument.price_precision();
        let size_precision = instrument.size_precision();
        let depth = request.depth.map(|d| d.get());

        let http = self.http_client.clone();
        let sender = self.data_sender.clone();
        let client_id = request.client_id.unwrap_or(self.client_id);
        let request_id = request.request_id;
        let params = request.params;
        let clock = self.clock;

        get_runtime().spawn(async move {
            match http.info_l2_book(&raw_symbol).await {
                Ok(l2_book) => {
                    let mut book = OrderBook::new(instrument_id, BookType::L2_MBP);
                    let ts_event = UnixNanos::from(l2_book.time * 1_000_000);

                    let all_bids = l2_book
                        .levels
                        .first()
                        .map_or([].as_slice(), |v| v.as_slice());
                    let all_asks = l2_book
                        .levels
                        .get(1)
                        .map_or([].as_slice(), |v| v.as_slice());

                    let bids = match depth {
                        Some(d) if d < all_bids.len() => &all_bids[..d],
                        _ => all_bids,
                    };
                    let asks = match depth {
                        Some(d) if d < all_asks.len() => &all_asks[..d],
                        _ => all_asks,
                    };

                    for (i, level) in bids.iter().enumerate() {
                        let px: f64 = match level.px.parse() {
                            Ok(v) => v,
                            Err(_) => continue,
                        };
                        let sz: f64 = match level.sz.parse() {
                            Ok(v) => v,
                            Err(_) => continue,
                        };

                        if sz > 0.0 {
                            let price = Price::new(px, price_precision);
                            let size = Quantity::new(sz, size_precision);
                            let order = BookOrder::new(OrderSide::Buy, price, size, i as u64);
                            book.add(order, 0, i as u64, ts_event);
                        }
                    }

                    let bids_len = bids.len();

                    for (i, level) in asks.iter().enumerate() {
                        let px: f64 = match level.px.parse() {
                            Ok(v) => v,
                            Err(_) => continue,
                        };
                        let sz: f64 = match level.sz.parse() {
                            Ok(v) => v,
                            Err(_) => continue,
                        };

                        if sz > 0.0 {
                            let price = Price::new(px, price_precision);
                            let size = Quantity::new(sz, size_precision);
                            let order =
                                BookOrder::new(OrderSide::Sell, price, size, (bids_len + i) as u64);
                            book.add(order, 0, (bids_len + i) as u64, ts_event);
                        }
                    }

                    log::info!(
                        "Fetched order book for {instrument_id} with {} bids and {} asks",
                        bids.len(),
                        asks.len(),
                    );

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
}

// Reads optional `nSigFigs` / `mantissa` L2 precision controls from
// `subscribe_params`; bails on non-positive integer values.
pub(crate) fn parse_book_precision_params(
    params: Option<&Params>,
) -> anyhow::Result<(Option<u32>, Option<u32>)> {
    let Some(params) = params else {
        return Ok((None, None));
    };

    let read_u32 = |key: &str| -> anyhow::Result<Option<u32>> {
        match params.get(key) {
            None => Ok(None),
            Some(v) => v
                .as_u64()
                .and_then(|n| u32::try_from(n).ok())
                .ok_or_else(|| anyhow::anyhow!("`{key}` must be a positive u32"))
                .map(Some),
        }
    };

    Ok((read_u32("n_sig_figs")?, read_u32("mantissa")?))
}

// Hyperliquid funds perpetuals hourly, so `interval` is fixed at 60 mins;
// `time` from the venue marks the end of the funding interval in ms.
pub(crate) fn funding_entry_to_update(
    entry: &HyperliquidFundingHistoryEntry,
    instrument_id: InstrumentId,
) -> anyhow::Result<FundingRateUpdate> {
    let rate: Decimal = entry
        .funding_rate
        .parse()
        .with_context(|| format!("invalid fundingRate '{}'", entry.funding_rate))?;
    let ts = UnixNanos::from(entry.time * 1_000_000);
    Ok(FundingRateUpdate::new(
        instrument_id,
        rate,
        Some(60),
        None,
        ts,
        ts,
    ))
}

pub(crate) fn candle_to_bar(
    candle: &HyperliquidCandle,
    bar_type: BarType,
    price_precision: u8,
    size_precision: u8,
) -> anyhow::Result<Bar> {
    let ts_init = UnixNanos::from(candle.timestamp * 1_000_000);
    let ts_event = ts_init;

    let open = candle.open.parse::<f64>().context("parse open price")?;
    let high = candle.high.parse::<f64>().context("parse high price")?;
    let low = candle.low.parse::<f64>().context("parse low price")?;
    let close = candle.close.parse::<f64>().context("parse close price")?;
    let volume = candle.volume.parse::<f64>().context("parse volume")?;

    Ok(Bar::new(
        bar_type,
        Price::new(open, price_precision),
        Price::new(high, price_precision),
        Price::new(low, price_precision),
        Price::new(close, price_precision),
        Quantity::new(volume, size_precision),
        ts_event,
        ts_init,
    ))
}

/// Request bars from HTTP API.
async fn request_bars_from_http(
    http_client: HyperliquidHttpClient,
    bar_type: BarType,
    start: Option<DateTime<Utc>>,
    end: Option<DateTime<Utc>>,
    limit: Option<u32>,
    instruments: Arc<AtomicMap<InstrumentId, InstrumentAny>>,
) -> anyhow::Result<Vec<Bar>> {
    // Get instrument details for precision
    let instrument_id = bar_type.instrument_id();
    let instrument = instruments
        .load()
        .get(&instrument_id)
        .cloned()
        .context("instrument not found in cache")?;

    let price_precision = instrument.price_precision();
    let size_precision = instrument.size_precision();
    let raw_symbol = instrument.raw_symbol();
    let coin = raw_symbol.as_str();

    let interval = bar_type_to_interval(&bar_type)?;

    // Hyperliquid uses millisecond timestamps
    let now = Utc::now();
    let end_time = end.unwrap_or(now).timestamp_millis() as u64;
    let start_time = if let Some(start) = start {
        start.timestamp_millis() as u64
    } else {
        // Default to 1000 bars before end_time
        let spec = bar_type.spec();
        let step_ms = match spec.aggregation {
            BarAggregation::Minute => spec.step.get() as u64 * 60_000,
            BarAggregation::Hour => spec.step.get() as u64 * 3_600_000,
            BarAggregation::Day => spec.step.get() as u64 * 86_400_000,
            _ => 60_000,
        };
        end_time.saturating_sub(1000 * step_ms)
    };

    let candles = http_client
        .info_candle_snapshot(coin, interval, start_time, end_time)
        .await
        .context("failed to fetch candle snapshot from Hyperliquid")?;

    let mut bars: Vec<Bar> = candles
        .iter()
        .filter_map(|candle| {
            candle_to_bar(candle, bar_type, price_precision, size_precision)
                .map_err(|e| {
                    log::warn!("Failed to convert candle to bar: {e}");
                    e
                })
                .ok()
        })
        .collect();

    if let Some(limit) = limit
        && bars.len() > limit as usize
    {
        bars = bars.into_iter().take(limit as usize).collect();
    }

    log::debug!("Fetched {} bars for {}", bars.len(), bar_type);
    Ok(bars)
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use rust_decimal_macros::dec;
    use ustr::Ustr;

    use super::*;
    use crate::common::testing::load_test_data;

    fn btc_perp_id() -> InstrumentId {
        InstrumentId::from("BTC-PERP.HYPERLIQUID")
    }

    #[rstest]
    fn test_funding_entry_to_update_parses_positive_rate() {
        let entry = HyperliquidFundingHistoryEntry {
            coin: Ustr::from("BTC"),
            funding_rate: "0.0000125".to_string(),
            premium: Some("0.00029005".to_string()),
            time: 1769908800000,
        };
        let instrument_id = btc_perp_id();

        let update = funding_entry_to_update(&entry, instrument_id).unwrap();

        assert_eq!(update.instrument_id, instrument_id);
        assert_eq!(update.rate, dec!(0.0000125));
        assert_eq!(update.interval, Some(60));
        assert!(update.next_funding_ns.is_none());
        assert_eq!(update.ts_event, UnixNanos::from(1769908800000 * 1_000_000));
        assert_eq!(update.ts_init, update.ts_event);
    }

    #[rstest]
    fn test_funding_entry_to_update_handles_negative_rate() {
        let entry = HyperliquidFundingHistoryEntry {
            coin: Ustr::from("BTC"),
            funding_rate: "-0.0000081".to_string(),
            premium: None,
            time: 1769912400000,
        };
        let update = funding_entry_to_update(&entry, btc_perp_id()).unwrap();
        assert_eq!(update.rate, dec!(-0.0000081));
    }

    #[rstest]
    fn test_funding_entry_to_update_rejects_invalid_rate() {
        let entry = HyperliquidFundingHistoryEntry {
            coin: Ustr::from("BTC"),
            funding_rate: "not-a-number".to_string(),
            premium: None,
            time: 1769912400000,
        };
        let result = funding_entry_to_update(&entry, btc_perp_id());
        assert!(result.is_err());
    }

    #[rstest]
    fn test_parse_book_precision_params_none() {
        let (n, m) = parse_book_precision_params(None).unwrap();
        assert_eq!(n, None);
        assert_eq!(m, None);
    }

    fn make_params(json: serde_json::Value) -> Params {
        serde_json::from_value(json).expect("valid params payload")
    }

    #[rstest]
    fn test_parse_book_precision_params_only_n_sig_figs() {
        let params = make_params(serde_json::json!({"n_sig_figs": 4}));
        let (n, m) = parse_book_precision_params(Some(&params)).unwrap();
        assert_eq!(n, Some(4));
        assert_eq!(m, None);
    }

    #[rstest]
    fn test_parse_book_precision_params_both() {
        let params = make_params(serde_json::json!({"n_sig_figs": 5, "mantissa": 2}));
        let (n, m) = parse_book_precision_params(Some(&params)).unwrap();
        assert_eq!(n, Some(5));
        assert_eq!(m, Some(2));
    }

    #[rstest]
    fn test_parse_book_precision_params_rejects_negative() {
        let params = make_params(serde_json::json!({"n_sig_figs": -1}));
        let err = parse_book_precision_params(Some(&params)).unwrap_err();
        assert!(err.to_string().contains("n_sig_figs"));
    }

    #[rstest]
    fn test_funding_history_fixture_parses() {
        let entries: Vec<HyperliquidFundingHistoryEntry> =
            load_test_data("http_funding_history.json");
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].coin.as_str(), "BTC");
        assert_eq!(entries[0].funding_rate, "0.0000125");
        assert_eq!(entries[0].premium.as_deref(), Some("0.00029005"));
        assert!(entries[2].premium.is_none());

        let updates: Vec<FundingRateUpdate> = entries
            .iter()
            .map(|e| funding_entry_to_update(e, btc_perp_id()).unwrap())
            .collect();
        assert_eq!(updates.len(), 3);
        assert_eq!(updates[0].rate, dec!(0.0000125));
        assert_eq!(updates[1].rate, dec!(-0.0000081));
        assert_eq!(updates[2].rate, dec!(0.0000033));
    }
}
