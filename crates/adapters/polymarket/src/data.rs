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

//! Live market data client implementation for the Polymarket adapter.

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use ahash::AHashMap;
use dashmap::{DashMap, DashSet};
use nautilus_common::{
    clients::DataClient,
    live::{get_runtime, runner::get_data_event_sender},
    messages::{
        DataEvent,
        data::{
            SubscribeBookDeltas, SubscribeQuotes, SubscribeTrades, UnsubscribeBookDeltas,
            UnsubscribeQuotes, UnsubscribeTrades,
        },
    },
    providers::InstrumentProvider,
};
use nautilus_core::time::{AtomicTime, get_atomic_clock_realtime};
use nautilus_model::{
    data::{Data as NautilusData, OrderBookDeltas_API, QuoteTick},
    enums::BookType,
    identifiers::{ClientId, InstrumentId, Venue},
    instruments::{Instrument, InstrumentAny},
    orderbook::OrderBook,
};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use ustr::Ustr;

use crate::{
    common::consts::POLYMARKET_VENUE,
    config::PolymarketDataClientConfig,
    http::client::{PolymarketHttpClient, PolymarketRawHttpClient},
    providers::PolymarketInstrumentProvider,
    websocket::{
        client::PolymarketWebSocketClient,
        messages::{MarketWsMessage, PolymarketQuotes, PolymarketWsMessage},
        parse::{
            parse_book_deltas, parse_book_snapshot, parse_quote_from_price_change,
            parse_quote_from_snapshot, parse_timestamp_ms, parse_trade_tick,
        },
    },
};

struct WsMessageContext {
    clock: &'static AtomicTime,
    data_sender: tokio::sync::mpsc::UnboundedSender<DataEvent>,
    token_instruments: Arc<AHashMap<Ustr, InstrumentAny>>,
    order_books: Arc<DashMap<InstrumentId, OrderBook>>,
    last_quotes: Arc<DashMap<InstrumentId, QuoteTick>>,
    active_quote_subs: Arc<DashSet<InstrumentId>>,
    active_delta_subs: Arc<DashSet<InstrumentId>>,
    active_trade_subs: Arc<DashSet<InstrumentId>>,
}

/// Polymarket data client for live market data streaming.
///
/// Integrates with the Nautilus DataEngine to provide:
/// - Real-time order book snapshots and deltas via WebSocket
/// - Quote ticks synthesized from book data
/// - Trade ticks from last trade price messages
/// - Automatic instrument discovery from the Gamma API
#[derive(Debug)]
pub struct PolymarketDataClient {
    clock: &'static AtomicTime,
    client_id: ClientId,
    config: PolymarketDataClientConfig,
    provider: PolymarketInstrumentProvider,
    ws_client: PolymarketWebSocketClient,
    is_connected: AtomicBool,
    cancellation_token: CancellationToken,
    tasks: Vec<JoinHandle<()>>,
    data_sender: tokio::sync::mpsc::UnboundedSender<DataEvent>,
    order_books: Arc<DashMap<InstrumentId, OrderBook>>,
    last_quotes: Arc<DashMap<InstrumentId, QuoteTick>>,
    active_quote_subs: Arc<DashSet<InstrumentId>>,
    active_delta_subs: Arc<DashSet<InstrumentId>>,
    active_trade_subs: Arc<DashSet<InstrumentId>>,
}

impl PolymarketDataClient {
    /// Creates a new [`PolymarketDataClient`].
    pub fn new(
        client_id: ClientId,
        config: PolymarketDataClientConfig,
        http_client: PolymarketRawHttpClient,
        ws_client: PolymarketWebSocketClient,
    ) -> Self {
        let clock = get_atomic_clock_realtime();
        let data_sender = get_data_event_sender();
        let domain_client = PolymarketHttpClient::from_raw(Arc::new(http_client));
        let provider = PolymarketInstrumentProvider::new(domain_client);

        Self {
            clock,
            client_id,
            config,
            provider,
            ws_client,
            is_connected: AtomicBool::new(false),
            cancellation_token: CancellationToken::new(),
            tasks: Vec::new(),
            data_sender,
            order_books: Arc::new(DashMap::new()),
            last_quotes: Arc::new(DashMap::new()),
            active_quote_subs: Arc::new(DashSet::new()),
            active_delta_subs: Arc::new(DashSet::new()),
            active_trade_subs: Arc::new(DashSet::new()),
        }
    }

    /// Returns a reference to the client configuration.
    #[must_use]
    pub fn config(&self) -> &PolymarketDataClientConfig {
        &self.config
    }

    /// Returns the venue for this data client.
    #[must_use]
    pub fn venue(&self) -> Venue {
        *POLYMARKET_VENUE
    }

    /// Returns a reference to the instrument provider.
    #[must_use]
    pub fn provider(&self) -> &PolymarketInstrumentProvider {
        &self.provider
    }

    /// Returns `true` when the client is connected.
    #[must_use]
    pub fn is_connected(&self) -> bool {
        self.is_connected.load(Ordering::Relaxed)
    }

    fn resolve_token_id(&self, instrument_id: InstrumentId) -> anyhow::Result<String> {
        let instrument = self
            .provider
            .store()
            .find(&instrument_id)
            .ok_or_else(|| anyhow::anyhow!("Instrument {instrument_id} not found"))?;
        Ok(instrument.raw_symbol().as_str().to_string())
    }

    fn subscribe_ws_market(&self, token_id: String) {
        let ws = self.ws_client.clone_subscription_handle();
        get_runtime().spawn(async move {
            if let Err(e) = ws.subscribe_market(vec![token_id]).await {
                log::error!("Failed to subscribe to market data: {e:?}");
            }
        });
    }

    fn unsubscribe_ws_market(&self, token_id: String) {
        let ws = self.ws_client.clone_subscription_handle();
        get_runtime().spawn(async move {
            if let Err(e) = ws.unsubscribe_market(vec![token_id]).await {
                log::error!("Failed to unsubscribe from market data: {e:?}");
            }
        });
    }

    fn has_any_market_sub(&self, instrument_id: &InstrumentId) -> bool {
        self.active_quote_subs.contains(instrument_id)
            || self.active_delta_subs.contains(instrument_id)
            || self.active_trade_subs.contains(instrument_id)
    }

    async fn bootstrap_instruments(&mut self) -> anyhow::Result<()> {
        self.provider.load_all(None).await?;

        for instrument in self.provider.store().list_all() {
            if let Err(e) = self
                .data_sender
                .send(DataEvent::Instrument(instrument.clone()))
            {
                log::warn!("Failed to publish instrument {}: {e}", instrument.id());
            }
        }

        Ok(())
    }

    fn spawn_message_handler(
        &mut self,
        mut rx: tokio::sync::mpsc::UnboundedReceiver<PolymarketWsMessage>,
    ) {
        let cancellation = self.cancellation_token.clone();
        let token_instruments = Arc::new(self.provider.build_token_map());
        let ctx = WsMessageContext {
            clock: self.clock,
            data_sender: self.data_sender.clone(),
            token_instruments,
            order_books: self.order_books.clone(),
            last_quotes: self.last_quotes.clone(),
            active_quote_subs: self.active_quote_subs.clone(),
            active_delta_subs: self.active_delta_subs.clone(),
            active_trade_subs: self.active_trade_subs.clone(),
        };

        let handle = get_runtime().spawn(async move {
            log::debug!("Polymarket message handler started");

            loop {
                tokio::select! {
                    maybe_msg = rx.recv() => {
                        match maybe_msg {
                            Some(msg) => Self::handle_ws_message(msg, &ctx),
                            None => {
                                log::debug!("WebSocket message channel closed");
                                break;
                            }
                        }
                    }
                    () = cancellation.cancelled() => {
                        log::debug!("Polymarket message handler cancelled");
                        break;
                    }
                }
            }

            log::debug!("Polymarket message handler ended");
        });

        self.tasks.push(handle);
    }

    fn handle_ws_message(message: PolymarketWsMessage, ctx: &WsMessageContext) {
        match message {
            PolymarketWsMessage::Market(market_msg) => {
                Self::handle_market_message(market_msg, ctx);
            }
            PolymarketWsMessage::User(_) => {
                log::debug!("Ignoring user message on data client");
            }
            PolymarketWsMessage::Reconnected => {
                log::info!("Polymarket WS reconnected");
            }
        }
    }

    fn handle_market_message(message: MarketWsMessage, ctx: &WsMessageContext) {
        match message {
            MarketWsMessage::Book(snap) => {
                let token_id = Ustr::from(snap.asset_id.as_str());
                let instrument = match ctx.token_instruments.get(&token_id) {
                    Some(inst) => inst,
                    None => {
                        log::debug!("No instrument for token_id {token_id}");
                        return;
                    }
                };
                let instrument_id = instrument.id();
                let ts_init = ctx.clock.get_time_ns();

                if ctx.active_delta_subs.contains(&instrument_id) {
                    match parse_book_snapshot(&snap, instrument, ts_init) {
                        Ok(deltas) => {
                            let mut book = ctx
                                .order_books
                                .entry(instrument_id)
                                .or_insert_with(|| OrderBook::new(instrument_id, BookType::L2_MBP));

                            if let Err(e) = book.apply_deltas(&deltas) {
                                log::error!(
                                    "Failed to apply book snapshot for {instrument_id}: {e}"
                                );
                            }

                            let data: NautilusData = OrderBookDeltas_API::new(deltas).into();
                            if let Err(e) = ctx.data_sender.send(DataEvent::Data(data)) {
                                log::error!("Failed to emit book deltas: {e}");
                            }
                        }
                        Err(e) => log::error!("Failed to parse book snapshot: {e}"),
                    }
                }

                if ctx.active_quote_subs.contains(&instrument_id) {
                    match parse_quote_from_snapshot(&snap, instrument, ts_init) {
                        Ok(Some(quote)) => {
                            Self::emit_quote_if_changed(ctx, instrument_id, quote);
                        }
                        Ok(None) => {}
                        Err(e) => log::error!("Failed to parse quote from snapshot: {e}"),
                    }
                }
            }

            MarketWsMessage::PriceChange(quotes) => {
                let ts_init = ctx.clock.get_time_ns();
                let ts_event = match parse_timestamp_ms(&quotes.timestamp) {
                    Ok(ts) => ts,
                    Err(e) => {
                        log::error!("Failed to parse price change timestamp: {e}");
                        return;
                    }
                };

                // Each change may belong to a different asset, so resolve per-change
                for change in &quotes.price_changes {
                    let token_id = Ustr::from(change.asset_id.as_str());
                    let instrument = match ctx.token_instruments.get(&token_id) {
                        Some(inst) => inst,
                        None => {
                            log::debug!("No instrument for token_id {token_id}");
                            continue;
                        }
                    };
                    let instrument_id = instrument.id();

                    if ctx.active_delta_subs.contains(&instrument_id) {
                        let per_asset = PolymarketQuotes {
                            market: quotes.market,
                            price_changes: vec![change.clone()],
                            timestamp: quotes.timestamp.clone(),
                        };

                        match parse_book_deltas(&per_asset, instrument, ts_init) {
                            Ok(deltas) => {
                                if let Some(mut book) = ctx.order_books.get_mut(&instrument_id)
                                    && let Err(e) = book.apply_deltas(&deltas)
                                {
                                    log::error!(
                                        "Failed to apply book deltas for {instrument_id}: {e}"
                                    );
                                }

                                let data: NautilusData = OrderBookDeltas_API::new(deltas).into();

                                if let Err(e) = ctx.data_sender.send(DataEvent::Data(data)) {
                                    log::error!("Failed to emit book deltas: {e}");
                                }
                            }
                            Err(e) => log::error!("Failed to parse book deltas: {e}"),
                        }
                    }

                    if ctx.active_quote_subs.contains(&instrument_id) {
                        // Clone and drop guard before emit to avoid DashMap deadlock
                        let last_quote = ctx.last_quotes.get(&instrument_id).map(|r| *r);
                        match parse_quote_from_price_change(
                            change,
                            instrument,
                            last_quote.as_ref(),
                            ts_event,
                            ts_init,
                        ) {
                            Ok(quote) => {
                                Self::emit_quote_if_changed(ctx, instrument_id, quote);
                            }
                            Err(e) => {
                                log::error!("Failed to parse quote from price change: {e}");
                            }
                        }
                    }
                }
            }

            MarketWsMessage::LastTradePrice(trade) => {
                let token_id = Ustr::from(trade.asset_id.as_str());
                let instrument = match ctx.token_instruments.get(&token_id) {
                    Some(inst) => inst,
                    None => {
                        log::debug!("No instrument for token_id {token_id}");
                        return;
                    }
                };
                let instrument_id = instrument.id();

                if ctx.active_trade_subs.contains(&instrument_id) {
                    let ts_init = ctx.clock.get_time_ns();
                    match parse_trade_tick(&trade, instrument, ts_init) {
                        Ok(tick) => {
                            if let Err(e) = ctx
                                .data_sender
                                .send(DataEvent::Data(NautilusData::Trade(tick)))
                            {
                                log::error!("Failed to emit trade tick: {e}");
                            }
                        }
                        Err(e) => log::error!("Failed to parse trade tick: {e}"),
                    }
                }
            }

            MarketWsMessage::TickSizeChange(change) => {
                log::info!(
                    "Tick size changed for {}: {} -> {}",
                    change.asset_id,
                    change.old_tick_size,
                    change.new_tick_size
                );
            }
        }
    }

    fn emit_quote_if_changed(
        ctx: &WsMessageContext,
        instrument_id: InstrumentId,
        quote: QuoteTick,
    ) {
        // Compare prices and sizes only; timestamps always differ between messages
        let emit = !matches!(
            ctx.last_quotes.get(&instrument_id),
            Some(existing) if existing.bid_price == quote.bid_price
                && existing.ask_price == quote.ask_price
                && existing.bid_size == quote.bid_size
                && existing.ask_size == quote.ask_size
        );

        if emit {
            ctx.last_quotes.insert(instrument_id, quote);
            if let Err(e) = ctx
                .data_sender
                .send(DataEvent::Data(NautilusData::Quote(quote)))
            {
                log::error!("Failed to emit quote tick: {e}");
            }
        }
    }

    async fn await_tasks_with_timeout(&mut self, timeout: tokio::time::Duration) {
        for handle in self.tasks.drain(..) {
            let _ = tokio::time::timeout(timeout, handle).await;
        }
    }
}

#[async_trait::async_trait(?Send)]
impl DataClient for PolymarketDataClient {
    fn client_id(&self) -> ClientId {
        self.client_id
    }

    fn venue(&self) -> Option<Venue> {
        Some(*POLYMARKET_VENUE)
    }

    fn start(&mut self) -> anyhow::Result<()> {
        log::info!("Starting Polymarket data client: {}", self.client_id);
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        log::info!("Stopping Polymarket data client: {}", self.client_id);
        self.cancellation_token.cancel();
        self.is_connected.store(false, Ordering::Relaxed);
        Ok(())
    }

    fn reset(&mut self) -> anyhow::Result<()> {
        log::debug!("Resetting Polymarket data client: {}", self.client_id);
        self.is_connected.store(false, Ordering::Relaxed);
        self.cancellation_token = CancellationToken::new();
        for handle in self.tasks.drain(..) {
            handle.abort();
        }
        Ok(())
    }

    fn dispose(&mut self) -> anyhow::Result<()> {
        self.stop()
    }

    async fn connect(&mut self) -> anyhow::Result<()> {
        if self.is_connected() {
            return Ok(());
        }

        self.cancellation_token = CancellationToken::new();

        log::info!("Connecting Polymarket data client");

        self.bootstrap_instruments().await?;

        self.ws_client.connect().await?;

        let rx = self
            .ws_client
            .take_message_receiver()
            .ok_or_else(|| anyhow::anyhow!("WS message receiver not available after connect"))?;

        self.spawn_message_handler(rx);

        self.is_connected.store(true, Ordering::Relaxed);
        log::info!("Connected Polymarket data client");

        Ok(())
    }

    async fn disconnect(&mut self) -> anyhow::Result<()> {
        if !self.is_connected() {
            return Ok(());
        }

        log::info!("Disconnecting Polymarket data client");

        self.cancellation_token.cancel();
        self.await_tasks_with_timeout(tokio::time::Duration::from_secs(5))
            .await;

        self.ws_client.disconnect().await?;

        self.is_connected.store(false, Ordering::Relaxed);
        log::info!("Disconnected Polymarket data client");

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
            anyhow::bail!(
                "Polymarket only supports L2_MBP order book deltas, received {:?}",
                cmd.book_type
            );
        }

        let instrument_id = cmd.instrument_id;
        let token_id = self.resolve_token_id(instrument_id)?;

        self.order_books
            .entry(instrument_id)
            .or_insert_with(|| OrderBook::new(instrument_id, BookType::L2_MBP));

        let needs_ws_sub = !self.has_any_market_sub(&instrument_id);
        self.active_delta_subs.insert(instrument_id);

        if needs_ws_sub {
            self.subscribe_ws_market(token_id);
        }

        log::info!("Subscribed to book deltas for {instrument_id}");
        Ok(())
    }

    fn subscribe_quotes(&mut self, cmd: &SubscribeQuotes) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;
        let token_id = self.resolve_token_id(instrument_id)?;

        let needs_ws_sub = !self.has_any_market_sub(&instrument_id);
        self.active_quote_subs.insert(instrument_id);

        if needs_ws_sub {
            self.subscribe_ws_market(token_id);
        }

        log::info!("Subscribed to quotes for {instrument_id}");
        Ok(())
    }

    fn subscribe_trades(&mut self, cmd: &SubscribeTrades) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;
        let token_id = self.resolve_token_id(instrument_id)?;

        let needs_ws_sub = !self.has_any_market_sub(&instrument_id);
        self.active_trade_subs.insert(instrument_id);

        if needs_ws_sub {
            self.subscribe_ws_market(token_id);
        }

        log::info!("Subscribed to trades for {instrument_id}");
        Ok(())
    }

    fn unsubscribe_book_deltas(&mut self, cmd: &UnsubscribeBookDeltas) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;
        self.active_delta_subs.remove(&instrument_id);

        if !self.has_any_market_sub(&instrument_id)
            && let Ok(token_id) = self.resolve_token_id(instrument_id)
        {
            self.unsubscribe_ws_market(token_id);
        }

        log::info!("Unsubscribed from book deltas for {instrument_id}");
        Ok(())
    }

    fn unsubscribe_quotes(&mut self, cmd: &UnsubscribeQuotes) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;
        self.active_quote_subs.remove(&instrument_id);

        if !self.has_any_market_sub(&instrument_id)
            && let Ok(token_id) = self.resolve_token_id(instrument_id)
        {
            self.unsubscribe_ws_market(token_id);
        }

        log::info!("Unsubscribed from quotes for {instrument_id}");
        Ok(())
    }

    fn unsubscribe_trades(&mut self, cmd: &UnsubscribeTrades) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;
        self.active_trade_subs.remove(&instrument_id);

        if !self.has_any_market_sub(&instrument_id)
            && let Ok(token_id) = self.resolve_token_id(instrument_id)
        {
            self.unsubscribe_ws_market(token_id);
        }

        log::info!("Unsubscribed from trades for {instrument_id}");
        Ok(())
    }
}
