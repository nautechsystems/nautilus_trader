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

use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, SystemTime},
};

use chrono::Utc;
use futures_util::{Stream, StreamExt};
use nautilus_common::runtime::get_runtime;
use nautilus_core::{consts::NAUTILUS_USER_AGENT, time::get_atomic_clock_realtime};
use nautilus_model::{
    data::{BarType, Data, OrderBookDeltas_API},
    identifiers::InstrumentId,
    instruments::{Instrument, InstrumentAny},
};
use nautilus_network::websocket::{MessageReader, WebSocketClient, WebSocketConfig};
use reqwest::header::USER_AGENT;
use tokio::sync::Mutex;
use tokio_tungstenite::tungstenite::{Error, Message};
use ustr::Ustr;

use super::{
    enums::{CoinbaseIntxWsChannel, WsOperation},
    error::CoinbaseIntxWsError,
    messages::{CoinbaseIntxSubscription, CoinbaseIntxWsMessage, NautilusWsMessage},
    parse::{
        parse_candle_msg, parse_index_price_msg, parse_mark_price_msg,
        parse_orderbook_snapshot_msg, parse_orderbook_update_msg, parse_quote_msg,
    },
};
use crate::{
    common::{
        consts::COINBASE_INTX_WS_URL,
        credential::{Credential, get_env_var},
        parse::bar_spec_as_coinbase_channel,
    },
    websocket::parse::{parse_instrument_any, parse_trade_msg},
};

/// Provides a WebSocket client for connecting to [Coinbase International](https://www.coinbase.com/en/international-exchange).
#[derive(Clone)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.adapters")
)]
pub struct CoinbaseIntxWebSocketClient {
    url: String,
    credential: Credential,
    heartbeat: Option<u64>,
    inner: Option<Arc<WebSocketClient>>,
    rx: Option<Arc<tokio::sync::mpsc::UnboundedReceiver<NautilusWsMessage>>>,
    signal: Arc<AtomicBool>,
    task_handle: Option<Arc<tokio::task::JoinHandle<()>>>,
    subscriptions: Arc<Mutex<HashMap<CoinbaseIntxWsChannel, Vec<Ustr>>>>,
}

impl Default for CoinbaseIntxWebSocketClient {
    fn default() -> Self {
        Self::new(None, None, None, None, Some(10)).expect("Failed to create client")
    }
}

impl CoinbaseIntxWebSocketClient {
    /// Creates a new [`CoinbaseIntxWebSocketClient`] instance.
    pub fn new(
        url: Option<String>,
        api_key: Option<String>,
        api_secret: Option<String>,
        api_passphrase: Option<String>,
        heartbeat: Option<u64>,
    ) -> anyhow::Result<Self> {
        let url = url.unwrap_or(COINBASE_INTX_WS_URL.to_string());
        let api_key = api_key.unwrap_or(get_env_var("COINBASE_INTX_API_KEY")?);
        let api_secret = api_secret.unwrap_or(get_env_var("COINBASE_INTX_API_SECRET")?);
        let api_passphrase = api_passphrase.unwrap_or(get_env_var("COINBASE_INTX_API_PASSPHRASE")?);

        let credential = Credential::new(api_key, api_secret, api_passphrase);
        let signal = Arc::new(AtomicBool::new(false));
        let subscriptions = Arc::new(Mutex::new(HashMap::new()));

        Ok(Self {
            url,
            credential,
            heartbeat,
            inner: None,
            rx: None,
            signal,
            task_handle: None,
            subscriptions,
        })
    }

    /// Creates a new authenticated [`CoinbaseIntxWebSocketClient`] using environment variables and
    /// the default Coinbase International production websocket url.
    pub fn from_env() -> anyhow::Result<Self> {
        Self::new(None, None, None, None, None)
    }

    /// Returns the websocket url being used by the client.
    pub fn url(&self) -> &str {
        self.url.as_str()
    }

    /// Returns the public API key being used by the client.
    pub fn api_key(&self) -> &str {
        self.credential.api_key.as_str()
    }

    /// Returns a value indicating whether the client is active.
    pub fn is_active(&self) -> bool {
        match &self.inner {
            Some(inner) => inner.is_active(),
            None => false,
        }
    }

    /// Returns a value indicating whether the client is closed.
    pub fn is_closed(&self) -> bool {
        match &self.inner {
            Some(inner) => inner.is_closed(),
            None => true,
        }
    }

    /// Connects the client to the server and caches the given instruments.
    pub async fn connect(&mut self, instruments: Vec<InstrumentAny>) -> anyhow::Result<()> {
        let client = self.clone();
        let post_reconnect = Arc::new(move || {
            let client = client.clone();
            tokio::spawn(async move { client.resubscribe_all().await });
        });

        let config = WebSocketConfig {
            url: self.url.clone(),
            headers: vec![(USER_AGENT.to_string(), NAUTILUS_USER_AGENT.to_string())],
            heartbeat: self.heartbeat,
            heartbeat_msg: None,
            handler: None,
            ping_handler: None,
            reconnect_timeout_ms: Some(5_000),
            reconnect_delay_initial_ms: None, // Use default
            reconnect_delay_max_ms: None,     // Use default
            reconnect_backoff_factor: None,   // Use default
            reconnect_jitter_ms: None,        // Use default
        };
        let (reader, client) =
            WebSocketClient::connect_stream(config, vec![], None, Some(post_reconnect)).await?;

        self.inner = Some(Arc::new(client));

        let mut instruments_map: HashMap<Ustr, InstrumentAny> = HashMap::new();
        for inst in instruments {
            instruments_map.insert(inst.raw_symbol().inner(), inst);
        }

        let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<NautilusWsMessage>();
        self.rx = Some(Arc::new(rx));
        let signal = self.signal.clone();

        let stream_handle = get_runtime().spawn(async move {
            CoinbaseIntxWsMessageHandler::new(instruments_map, reader, signal, tx)
                .run()
                .await;
        });

        self.task_handle = Some(Arc::new(stream_handle));

        Ok(())
    }

    /// Provides the internal data stream as a channel-based stream.
    ///
    /// # Panics
    ///
    /// This function panics:
    /// - If the websocket is not connected.
    /// - If `stream_data` has already been called somewhere else (stream receiver is then taken).
    pub fn stream(&mut self) -> impl Stream<Item = NautilusWsMessage> + 'static {
        let rx = self
            .rx
            .take()
            .expect("Data stream receiver already taken or not connected"); // Design-time error
        let mut rx = Arc::try_unwrap(rx).expect("Cannot take ownership - other references exist");
        async_stream::stream! {
            while let Some(data) = rx.recv().await {
                yield data;
            }
        }
    }

    /// Closes the client.
    pub async fn close(&mut self) -> Result<(), Error> {
        tracing::debug!("Closing");
        self.signal.store(true, Ordering::Relaxed);

        match tokio::time::timeout(Duration::from_secs(5), async {
            if let Some(inner) = &self.inner {
                inner.disconnect().await;
            } else {
                log::error!("Error on close: not connected");
            }
        })
        .await
        {
            Ok(()) => {
                tracing::debug!("Inner disconnected");
            }
            Err(_) => {
                tracing::error!("Timeout waiting for inner client to disconnect");
            }
        }

        log::debug!("Closed");

        Ok(())
    }

    /// Subscribes to the given channels and product IDs.
    async fn subscribe(
        &self,
        channels: Vec<CoinbaseIntxWsChannel>,
        product_ids: Vec<Ustr>,
    ) -> Result<(), CoinbaseIntxWsError> {
        // Update active subscriptions
        let mut active_subs = self.subscriptions.lock().await;
        for channel in &channels {
            active_subs
                .entry(*channel)
                .or_insert_with(Vec::new)
                .extend(product_ids.clone());
        }
        tracing::debug!(
            "Added active subscription(s): channels={channels:?}, product_ids={product_ids:?}"
        );

        let time = chrono::DateTime::<Utc>::from(SystemTime::now())
            .timestamp()
            .to_string();
        let signature = self.credential.sign_ws(&time);
        let message = CoinbaseIntxSubscription {
            op: WsOperation::Subscribe,
            product_ids: Some(product_ids),
            channels,
            time,
            key: self.credential.api_key,
            passphrase: self.credential.api_passphrase,
            signature,
        };

        let json_txt = serde_json::to_string(&message)
            .map_err(|e| CoinbaseIntxWsError::JsonError(e.to_string()))?;

        if let Some(inner) = &self.inner {
            inner.send_text(json_txt, None).await;
        } else {
            return Err(CoinbaseIntxWsError::ClientError(
                "Cannot send message: not connected".to_string(),
            ));
        }

        Ok(())
    }

    /// Unsubscribes from the given channels and product IDs.
    async fn unsubscribe(
        &self,
        channels: Vec<CoinbaseIntxWsChannel>,
        product_ids: Vec<Ustr>,
    ) -> Result<(), CoinbaseIntxWsError> {
        // Update active subscriptions
        let mut active_subs = self.subscriptions.lock().await;
        for channel in &channels {
            if let Some(subs) = active_subs.get_mut(channel) {
                for product_id in &product_ids {
                    subs.retain(|pid| pid != product_id);
                }
                if subs.is_empty() {
                    active_subs.remove(channel);
                }
            }
        }
        tracing::debug!(
            "Removed active subscription(s): channels={channels:?}, product_ids={product_ids:?}"
        );

        let time = chrono::DateTime::<Utc>::from(SystemTime::now())
            .timestamp()
            .to_string();
        let signature = self.credential.sign_ws(&time);
        let message = CoinbaseIntxSubscription {
            op: WsOperation::Unsubscribe,
            product_ids: Some(product_ids),
            channels,
            time,
            key: self.credential.api_key,
            passphrase: self.credential.api_passphrase,
            signature,
        };

        let json_txt = serde_json::to_string(&message)
            .map_err(|e| CoinbaseIntxWsError::JsonError(e.to_string()))?;

        if let Some(inner) = &self.inner {
            inner.send_text(json_txt, None).await;
        } else {
            return Err(CoinbaseIntxWsError::ClientError(
                "Cannot send message: not connected".to_string(),
            ));
        }

        Ok(())
    }

    /// Resubscribes for all active subscriptions.
    async fn resubscribe_all(&self) {
        let subs = self.subscriptions.lock().await.clone();

        for (channel, product_ids) in subs {
            if product_ids.is_empty() {
                continue;
            }

            tracing::debug!("Resubscribing: channel={channel}, product_ids={product_ids:?}");

            if let Err(e) = self.subscribe(vec![channel], product_ids).await {
                tracing::error!("Failed to resubscribe to channel {channel}: {e}");
            }
        }
    }

    /// Subscribes to instrument definition updates for the given instrument IDs.
    pub async fn subscribe_instruments(
        &self,
        instrument_ids: Vec<InstrumentId>,
    ) -> Result<(), CoinbaseIntxWsError> {
        let product_ids = instrument_ids_to_product_ids(&instrument_ids);
        self.subscribe(vec![CoinbaseIntxWsChannel::Instruments], product_ids)
            .await
    }

    /// Subscribes to funding message streams for the given instrument IDs.
    pub async fn subscribe_funding(
        &self,
        instrument_ids: Vec<InstrumentId>,
    ) -> Result<(), CoinbaseIntxWsError> {
        let product_ids = instrument_ids_to_product_ids(&instrument_ids);
        self.subscribe(vec![CoinbaseIntxWsChannel::Funding], product_ids)
            .await
    }

    /// Subscribes to risk message streams for the given instrument IDs.
    pub async fn subscribe_risk(
        &self,
        instrument_ids: Vec<InstrumentId>,
    ) -> Result<(), CoinbaseIntxWsError> {
        let product_ids = instrument_ids_to_product_ids(&instrument_ids);
        self.subscribe(vec![CoinbaseIntxWsChannel::Risk], product_ids)
            .await
    }

    /// Subscribes to order book (level 2) streams for the given instrument IDs.
    pub async fn subscribe_order_book(
        &self,
        instrument_ids: Vec<InstrumentId>,
    ) -> Result<(), CoinbaseIntxWsError> {
        let product_ids = instrument_ids_to_product_ids(&instrument_ids);
        self.subscribe(vec![CoinbaseIntxWsChannel::Level2], product_ids)
            .await
    }

    /// Subscribes to quote (level 1) streams for the given instrument IDs.
    pub async fn subscribe_quotes(
        &self,
        instrument_ids: Vec<InstrumentId>,
    ) -> Result<(), CoinbaseIntxWsError> {
        let product_ids = instrument_ids_to_product_ids(&instrument_ids);
        self.subscribe(vec![CoinbaseIntxWsChannel::Level1], product_ids)
            .await
    }

    /// Subscribes to trade (match) streams for the given instrument IDs.
    pub async fn subscribe_trades(
        &self,
        instrument_ids: Vec<InstrumentId>,
    ) -> Result<(), CoinbaseIntxWsError> {
        let product_ids = instrument_ids_to_product_ids(&instrument_ids);
        self.subscribe(vec![CoinbaseIntxWsChannel::Match], product_ids)
            .await
    }

    /// Subscribes to risk streams (for mark prices) for the given instrument IDs.
    pub async fn subscribe_mark_prices(
        &self,
        instrument_ids: Vec<InstrumentId>,
    ) -> Result<(), CoinbaseIntxWsError> {
        let product_ids = instrument_ids_to_product_ids(&instrument_ids);
        self.subscribe(vec![CoinbaseIntxWsChannel::Risk], product_ids)
            .await
    }

    /// Subscribes to risk streams (for index prices) for the given instrument IDs.
    pub async fn subscribe_index_prices(
        &self,
        instrument_ids: Vec<InstrumentId>,
    ) -> Result<(), CoinbaseIntxWsError> {
        let product_ids = instrument_ids_to_product_ids(&instrument_ids);
        self.subscribe(vec![CoinbaseIntxWsChannel::Risk], product_ids)
            .await
    }

    /// Subscribes to bar (candle) streams for the given instrument IDs.
    pub async fn subscribe_bars(&self, bar_type: BarType) -> Result<(), CoinbaseIntxWsError> {
        let channel = bar_spec_as_coinbase_channel(bar_type.spec())
            .map_err(|e| CoinbaseIntxWsError::ClientError(e.to_string()))?;
        let product_ids = vec![bar_type.standard().instrument_id().symbol.inner()];
        self.subscribe(vec![channel], product_ids).await
    }

    /// Unsubscribes from instrument definition streams for the given instrument IDs.
    pub async fn unsubscribe_instruments(
        &self,
        instrument_ids: Vec<InstrumentId>,
    ) -> Result<(), CoinbaseIntxWsError> {
        let product_ids = instrument_ids_to_product_ids(&instrument_ids);
        self.unsubscribe(vec![CoinbaseIntxWsChannel::Instruments], product_ids)
            .await
    }

    /// Unsubscribes from risk message streams for the given instrument IDs.
    pub async fn unsubscribe_risk(
        &self,
        instrument_ids: Vec<InstrumentId>,
    ) -> Result<(), CoinbaseIntxWsError> {
        let product_ids = instrument_ids_to_product_ids(&instrument_ids);
        self.unsubscribe(vec![CoinbaseIntxWsChannel::Risk], product_ids)
            .await
    }

    /// Unsubscribes from funding message streams for the given instrument IDs.
    pub async fn unsubscribe_funding(
        &self,
        instrument_ids: Vec<InstrumentId>,
    ) -> Result<(), CoinbaseIntxWsError> {
        let product_ids = instrument_ids_to_product_ids(&instrument_ids);
        self.unsubscribe(vec![CoinbaseIntxWsChannel::Funding], product_ids)
            .await
    }

    /// Unsubscribes from order book (level 2) streams for the given instrument IDs.
    pub async fn unsubscribe_order_book(
        &self,
        instrument_ids: Vec<InstrumentId>,
    ) -> Result<(), CoinbaseIntxWsError> {
        let product_ids = instrument_ids_to_product_ids(&instrument_ids);
        self.unsubscribe(vec![CoinbaseIntxWsChannel::Level2], product_ids)
            .await
    }

    /// Unsubscribes from quote (level 1) streams for the given instrument IDs.
    pub async fn unsubscribe_quotes(
        &self,
        instrument_ids: Vec<InstrumentId>,
    ) -> Result<(), CoinbaseIntxWsError> {
        let product_ids = instrument_ids_to_product_ids(&instrument_ids);
        self.unsubscribe(vec![CoinbaseIntxWsChannel::Level1], product_ids)
            .await
    }

    /// Unsubscribes from trade (match) streams for the given instrument IDs.
    pub async fn unsubscribe_trades(
        &self,
        instrument_ids: Vec<InstrumentId>,
    ) -> Result<(), CoinbaseIntxWsError> {
        let product_ids = instrument_ids_to_product_ids(&instrument_ids);
        self.unsubscribe(vec![CoinbaseIntxWsChannel::Match], product_ids)
            .await
    }

    /// Unsubscribes from risk streams (for mark prices) for the given instrument IDs.
    pub async fn unsubscribe_mark_prices(
        &self,
        instrument_ids: Vec<InstrumentId>,
    ) -> Result<(), CoinbaseIntxWsError> {
        let product_ids = instrument_ids_to_product_ids(&instrument_ids);
        self.unsubscribe(vec![CoinbaseIntxWsChannel::Risk], product_ids)
            .await
    }

    /// Unsubscribes from risk streams (for index prices) for the given instrument IDs.
    pub async fn unsubscribe_index_prices(
        &self,
        instrument_ids: Vec<InstrumentId>,
    ) -> Result<(), CoinbaseIntxWsError> {
        let product_ids = instrument_ids_to_product_ids(&instrument_ids);
        self.unsubscribe(vec![CoinbaseIntxWsChannel::Risk], product_ids)
            .await
    }

    /// Unsubscribes from bar (candle) streams for the given instrument IDs.
    pub async fn unsubscribe_bars(&self, bar_type: BarType) -> Result<(), CoinbaseIntxWsError> {
        let channel = bar_spec_as_coinbase_channel(bar_type.spec())
            .map_err(|e| CoinbaseIntxWsError::ClientError(e.to_string()))?;
        let product_id = bar_type.standard().instrument_id().symbol.inner();
        self.unsubscribe(vec![channel], vec![product_id]).await
    }
}

fn instrument_ids_to_product_ids(instrument_ids: &[InstrumentId]) -> Vec<Ustr> {
    instrument_ids.iter().map(|x| x.symbol.inner()).collect()
}

/// Provides a raw message handler for Coinbase International WebSocket feed.
struct CoinbaseIntxFeedHandler {
    reader: MessageReader,
    signal: Arc<AtomicBool>,
}

impl CoinbaseIntxFeedHandler {
    /// Creates a new [`CoinbaseIntxFeedHandler`] instance.
    pub const fn new(reader: MessageReader, signal: Arc<AtomicBool>) -> Self {
        Self { reader, signal }
    }

    /// Gets the next message from the WebSocket message stream.
    async fn next(&mut self) -> Option<CoinbaseIntxWsMessage> {
        // Timeout awaiting the next message before checking signal
        let timeout = Duration::from_millis(10);

        loop {
            if self.signal.load(Ordering::Relaxed) {
                tracing::debug!("Stop signal received");
                break;
            }

            match tokio::time::timeout(timeout, self.reader.next()).await {
                Ok(Some(msg)) => match msg {
                    Ok(Message::Pong(_)) => {
                        tracing::trace!("Received pong");
                    }
                    Ok(Message::Ping(_)) => {
                        tracing::trace!("Received pong"); // Coinbase send ping frames as pongs
                    }
                    Ok(Message::Text(text)) => {
                        match serde_json::from_str(&text) {
                            Ok(event) => match &event {
                                CoinbaseIntxWsMessage::Reject(msg) => {
                                    tracing::error!("{msg:?}");
                                }
                                CoinbaseIntxWsMessage::Confirmation(msg) => {
                                    tracing::debug!("{msg:?}");
                                    continue;
                                }
                                CoinbaseIntxWsMessage::Instrument(_) => return Some(event),
                                CoinbaseIntxWsMessage::Funding(_) => return Some(event),
                                CoinbaseIntxWsMessage::Risk(_) => return Some(event),
                                CoinbaseIntxWsMessage::BookSnapshot(_) => return Some(event),
                                CoinbaseIntxWsMessage::BookUpdate(_) => return Some(event),
                                CoinbaseIntxWsMessage::Quote(_) => return Some(event),
                                CoinbaseIntxWsMessage::Trade(_) => return Some(event),
                                CoinbaseIntxWsMessage::CandleSnapshot(_) => return Some(event),
                                CoinbaseIntxWsMessage::CandleUpdate(_) => continue, // Ignore
                            },
                            Err(e) => {
                                tracing::error!("Failed to parse message: {e}: {text}");
                                break;
                            }
                        }
                    }
                    Ok(Message::Binary(msg)) => {
                        tracing::debug!("Raw binary: {msg:?}");
                    }
                    Ok(Message::Close(_)) => {
                        tracing::debug!("Received close message");
                        return None;
                    }
                    Ok(msg) => {
                        tracing::warn!("Unexpected message: {msg:?}");
                    }
                    Err(e) => {
                        tracing::error!("{e}, stopping client");
                        break; // Break as indicates a bug in the code
                    }
                },
                Ok(None) => {
                    tracing::info!("WebSocket stream closed");
                    break;
                }
                Err(_) => {} // Timeout occurred awaiting a message, continue loop to check signal
            }
        }

        tracing::debug!("Stopped message streaming");
        None
    }
}

/// Provides a Nautilus parser for the Coinbase International WebSocket feed.
struct CoinbaseIntxWsMessageHandler {
    instruments: HashMap<Ustr, InstrumentAny>,
    handler: CoinbaseIntxFeedHandler,
    tx: tokio::sync::mpsc::UnboundedSender<NautilusWsMessage>,
}

impl CoinbaseIntxWsMessageHandler {
    /// Creates a new [`CoinbaseIntxWsMessageHandler`] instance.
    pub const fn new(
        instruments: HashMap<Ustr, InstrumentAny>,
        reader: MessageReader,
        signal: Arc<AtomicBool>,
        tx: tokio::sync::mpsc::UnboundedSender<NautilusWsMessage>,
    ) -> Self {
        let handler = CoinbaseIntxFeedHandler::new(reader, signal);
        Self {
            instruments,
            handler,
            tx,
        }
    }

    /// Runs the WebSocket message feed.
    async fn run(&mut self) {
        while let Some(data) = self.next().await {
            if let Err(e) = self.tx.send(data) {
                tracing::error!("Error sending data: {e}");
                break; // Stop processing on channel error
            }
        }
    }

    /// Gets the next message from the WebSocket message handler.
    async fn next(&mut self) -> Option<NautilusWsMessage> {
        let clock = get_atomic_clock_realtime();

        while let Some(event) = self.handler.next().await {
            match event {
                CoinbaseIntxWsMessage::Instrument(msg) => {
                    if let Some(inst) = parse_instrument_any(&msg, clock.get_time_ns()) {
                        // Update instruments map
                        self.instruments
                            .insert(inst.raw_symbol().inner(), inst.clone());
                        return Some(NautilusWsMessage::Instrument(inst));
                    }
                }
                CoinbaseIntxWsMessage::Funding(msg) => {
                    tracing::warn!("Received {msg:?}"); // TODO: Implement
                }
                CoinbaseIntxWsMessage::BookSnapshot(msg) => {
                    if let Some(inst) = self.instruments.get(&msg.product_id) {
                        match parse_orderbook_snapshot_msg(
                            &msg,
                            inst.id(),
                            inst.price_precision(),
                            inst.size_precision(),
                            clock.get_time_ns(),
                        ) {
                            Ok(deltas) => {
                                let deltas = OrderBookDeltas_API::new(deltas);
                                let data = Data::Deltas(deltas);
                                return Some(NautilusWsMessage::Data(data));
                            }
                            Err(e) => {
                                tracing::error!("Failed to parse orderbook snapshot: {e}");
                                return None;
                            }
                        }
                    } else {
                        tracing::error!("No instrument found for {}", msg.product_id);
                        return None;
                    }
                }
                CoinbaseIntxWsMessage::BookUpdate(msg) => {
                    if let Some(inst) = self.instruments.get(&msg.product_id) {
                        match parse_orderbook_update_msg(
                            &msg,
                            inst.id(),
                            inst.price_precision(),
                            inst.size_precision(),
                            clock.get_time_ns(),
                        ) {
                            Ok(deltas) => {
                                let deltas = OrderBookDeltas_API::new(deltas);
                                let data = Data::Deltas(deltas);
                                return Some(NautilusWsMessage::Data(data));
                            }
                            Err(e) => {
                                tracing::error!("Failed to parse orderbook update: {e}");
                            }
                        }
                    } else {
                        tracing::error!("No instrument found for {}", msg.product_id);
                    }
                }
                CoinbaseIntxWsMessage::Quote(msg) => {
                    if let Some(inst) = self.instruments.get(&msg.product_id) {
                        match parse_quote_msg(
                            &msg,
                            inst.id(),
                            inst.price_precision(),
                            inst.size_precision(),
                            clock.get_time_ns(),
                        ) {
                            Ok(quote) => return Some(NautilusWsMessage::Data(Data::Quote(quote))),
                            Err(e) => {
                                tracing::error!("Failed to parse quote: {e}");
                            }
                        }
                    } else {
                        tracing::error!("No instrument found for {}", msg.product_id);
                    }
                }
                CoinbaseIntxWsMessage::Trade(msg) => {
                    if let Some(inst) = self.instruments.get(&msg.product_id) {
                        match parse_trade_msg(
                            &msg,
                            inst.id(),
                            inst.price_precision(),
                            inst.size_precision(),
                            clock.get_time_ns(),
                        ) {
                            Ok(trade) => return Some(NautilusWsMessage::Data(Data::Trade(trade))),
                            Err(e) => {
                                tracing::error!("Failed to parse trade: {e}");
                            }
                        }
                    } else {
                        tracing::error!("No instrument found for {}", msg.product_id);
                    }
                }
                CoinbaseIntxWsMessage::Risk(msg) => {
                    if let Some(inst) = self.instruments.get(&msg.product_id) {
                        let mark_price = match parse_mark_price_msg(
                            &msg,
                            inst.id(),
                            inst.price_precision(),
                            clock.get_time_ns(),
                        ) {
                            Ok(mark_price) => Some(mark_price),
                            Err(e) => {
                                tracing::error!("Failed to parse mark price: {e}");
                                None
                            }
                        };

                        let index_price = match parse_index_price_msg(
                            &msg,
                            inst.id(),
                            inst.price_precision(),
                            clock.get_time_ns(),
                        ) {
                            Ok(index_price) => Some(index_price),
                            Err(e) => {
                                tracing::error!("Failed to parse index price: {e}");
                                None
                            }
                        };

                        match (mark_price, index_price) {
                            (Some(mark), Some(index)) => {
                                return Some(NautilusWsMessage::MarkAndIndex((mark, index)));
                            }
                            (Some(mark), None) => return Some(NautilusWsMessage::MarkPrice(mark)),
                            (None, Some(index)) => {
                                return Some(NautilusWsMessage::IndexPrice(index));
                            }
                            (None, None) => continue,
                        };
                    } else {
                        tracing::error!("No instrument found for {}", msg.product_id);
                    }
                }
                CoinbaseIntxWsMessage::CandleSnapshot(msg) => {
                    if let Some(inst) = self.instruments.get(&msg.product_id) {
                        match parse_candle_msg(
                            &msg,
                            inst.id(),
                            inst.price_precision(),
                            inst.size_precision(),
                            clock.get_time_ns(),
                        ) {
                            Ok(bar) => return Some(NautilusWsMessage::Data(Data::Bar(bar))),
                            Err(e) => {
                                tracing::error!("Failed to parse candle: {e}");
                            }
                        }
                    } else {
                        tracing::error!("No instrument found for {}", msg.product_id);
                    }
                }
                _ => {
                    tracing::warn!("Not implemented: {event:?}");
                }
            }
        }
        None // Connection closed
    }
}
