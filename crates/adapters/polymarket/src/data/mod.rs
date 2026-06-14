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

mod auto_load;
mod dispatch;
mod instruments;
mod lifecycle;
mod requests;
mod subscriptions;

use std::{
    sync::{
        Arc, Mutex as StdMutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use ahash::AHashSet;
use dashmap::DashMap;
use nautilus_common::{
    clients::DataClient,
    live::{get_runtime, runner::get_data_event_sender},
    messages::{
        DataEvent,
        data::{
            RequestBookSnapshot, RequestCustomData, RequestInstrument, RequestInstruments,
            RequestTrades, SubscribeBookDeltas, SubscribeCustomData, SubscribeInstruments,
            SubscribeQuotes, SubscribeTrades, UnsubscribeBookDeltas, UnsubscribeCustomData,
            UnsubscribeQuotes, UnsubscribeTrades,
        },
    },
    msgbus::TypedHandler,
};
use nautilus_core::{
    AtomicMap, AtomicSet,
    time::{AtomicTime, get_atomic_clock_realtime},
};
use nautilus_model::{
    data::QuoteTick,
    enums::BookType,
    events::PositionEvent,
    identifiers::{ClientId, InstrumentId, Venue},
    instruments::InstrumentAny,
    orderbook::OrderBook,
};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use ustr::Ustr;

use self::{
    instruments::TokenMeta,
    requests::{
        request_book_snapshot, request_data, request_instrument, request_instruments,
        request_trades,
    },
    subscriptions::{resolve_token_id_from, sync_ws_subscription_async},
};
use crate::{
    common::consts::POLYMARKET_VENUE,
    config::PolymarketDataClientConfig,
    filters::InstrumentFilter,
    http::{
        clob::PolymarketClobPublicClient, data_api::PolymarketDataApiHttpClient,
        gamma::PolymarketGammaHttpClient,
    },
    providers::PolymarketInstrumentProvider,
    resolve::ResolveWatchEntry,
    rtds::{PolymarketRtdsFeed, is_supported_rtds_data_type},
    websocket::client::PolymarketWebSocketClient,
};

const NEW_MARKET_FETCH_MAX_CONCURRENCY_CAP: usize = 64;
pub(super) const NEW_MARKET_EMPTY_RECHECK_MAX_ATTEMPTS: usize = 1;
pub(super) const NEW_MARKET_EMPTY_RECHECK_DELAY: Duration = Duration::from_millis(500);

fn clamp_new_market_fetch_max_concurrency(value: usize) -> usize {
    value.clamp(1, NEW_MARKET_FETCH_MAX_CONCURRENCY_CAP)
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
    clob_public_client: PolymarketClobPublicClient,
    data_api_client: PolymarketDataApiHttpClient,
    ws_client: PolymarketWebSocketClient,
    is_connected: AtomicBool,
    cancellation_token: CancellationToken,
    tasks: Vec<JoinHandle<()>>,
    data_sender: tokio::sync::mpsc::UnboundedSender<DataEvent>,
    instruments: Arc<AtomicMap<InstrumentId, InstrumentAny>>,
    token_meta: Arc<DashMap<Ustr, TokenMeta>>,
    order_books: Arc<DashMap<InstrumentId, OrderBook>>,
    last_quotes: Arc<DashMap<InstrumentId, QuoteTick>>,
    active_quote_subs: Arc<AtomicSet<InstrumentId>>,
    active_delta_subs: Arc<AtomicSet<InstrumentId>>,
    active_trade_subs: Arc<AtomicSet<InstrumentId>>,
    resolve_poll_watchlist: Arc<AtomicMap<String, ResolveWatchEntry>>,
    resolve_watch_apply_mutex: Arc<StdMutex<()>>,
    pending_snapshot_after_tick_change: Arc<AtomicSet<InstrumentId>>,
    new_market_inflight_keys: Arc<DashMap<String, ()>>,
    new_market_fetch_semaphore: Arc<tokio::sync::Semaphore>,
    ws_open_tokens: Arc<AtomicSet<Ustr>>,
    ws_sub_mutex: Arc<tokio::sync::Mutex<()>>,
    pending_auto_loads: Arc<StdMutex<AHashSet<InstrumentId>>>,
    auto_load_scheduled: Arc<AtomicBool>,
    position_event_handler: Option<TypedHandler<PositionEvent>>,
    rtds_feed: PolymarketRtdsFeed,
}

impl PolymarketDataClient {
    /// Creates a new [`PolymarketDataClient`].
    pub fn new(
        client_id: ClientId,
        mut config: PolymarketDataClientConfig,
        gamma_client: PolymarketGammaHttpClient,
        clob_public_client: PolymarketClobPublicClient,
        data_api_client: PolymarketDataApiHttpClient,
        ws_client: PolymarketWebSocketClient,
    ) -> Self {
        let clock = get_atomic_clock_realtime();
        let data_sender = get_data_event_sender();
        let provider =
            PolymarketInstrumentProvider::new(gamma_client, config.instrument_config.clone());
        let configured_fetch_max_concurrency = config.new_market_fetch_max_concurrency;
        let fetch_max_concurrency =
            clamp_new_market_fetch_max_concurrency(configured_fetch_max_concurrency);

        if configured_fetch_max_concurrency == 0 {
            log::warn!(
                "PolymarketDataClientConfig.new_market_fetch_max_concurrency=0 is invalid, clamping to 1"
            );
        } else if configured_fetch_max_concurrency > NEW_MARKET_FETCH_MAX_CONCURRENCY_CAP {
            log::warn!(
                "PolymarketDataClientConfig.new_market_fetch_max_concurrency={configured_fetch_max_concurrency} exceeds cap {NEW_MARKET_FETCH_MAX_CONCURRENCY_CAP}, clamping",
            );
        }
        config.new_market_fetch_max_concurrency = fetch_max_concurrency;

        let rtds_url = config.rtds_url();
        let rtds_transport_backend = config.transport_backend;
        let rtds_data_sender = data_sender.clone();

        Self {
            clock,
            client_id,
            config,
            provider,
            clob_public_client,
            data_api_client,
            ws_client,
            is_connected: AtomicBool::new(false),
            cancellation_token: CancellationToken::new(),
            tasks: Vec::new(),
            data_sender,
            instruments: Arc::new(AtomicMap::new()),
            token_meta: Arc::new(DashMap::new()),
            order_books: Arc::new(DashMap::new()),
            last_quotes: Arc::new(DashMap::new()),
            active_quote_subs: Arc::new(AtomicSet::new()),
            active_delta_subs: Arc::new(AtomicSet::new()),
            active_trade_subs: Arc::new(AtomicSet::new()),
            resolve_poll_watchlist: Arc::new(AtomicMap::new()),
            resolve_watch_apply_mutex: Arc::new(StdMutex::new(())),
            pending_snapshot_after_tick_change: Arc::new(AtomicSet::new()),
            new_market_inflight_keys: Arc::new(DashMap::new()),
            new_market_fetch_semaphore: Arc::new(tokio::sync::Semaphore::new(
                fetch_max_concurrency,
            )),
            ws_open_tokens: Arc::new(AtomicSet::new()),
            ws_sub_mutex: Arc::new(tokio::sync::Mutex::new(())),
            pending_auto_loads: Arc::new(StdMutex::new(AHashSet::new())),
            auto_load_scheduled: Arc::new(AtomicBool::new(false)),
            position_event_handler: None,
            rtds_feed: PolymarketRtdsFeed::new(
                rtds_url,
                rtds_transport_backend,
                clock,
                rtds_data_sender,
            ),
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

    /// Adds an instrument filter on the underlying provider.
    pub fn add_instrument_filter(&mut self, filter: Arc<dyn InstrumentFilter>) {
        self.provider.add_filter(filter);
    }

    /// Returns `true` when the client is connected.
    #[must_use]
    pub fn is_connected(&self) -> bool {
        self.is_connected.load(Ordering::Relaxed)
    }

    fn resolve_token_id(&self, instrument_id: InstrumentId) -> anyhow::Result<String> {
        resolve_token_id_from(&self.instruments, instrument_id)
    }

    // Spawns an async task that reconciles the WS subscription for
    // `instrument_id`. The task holds `ws_sub_mutex` across the wire send so
    // concurrent subscribe/unsubscribe calls deliver commands to the WS handler
    // in a consistent order with the final `active_*_subs` state.
    fn sync_ws_subscription(&self, instrument_id: InstrumentId) {
        let token_id_str = match self.resolve_token_id(instrument_id) {
            Ok(s) => s,
            Err(_) => return,
        };
        let active_quote_subs = self.active_quote_subs.clone();
        let active_delta_subs = self.active_delta_subs.clone();
        let active_trade_subs = self.active_trade_subs.clone();
        let ws_open_tokens = self.ws_open_tokens.clone();
        let ws_sub_mutex = self.ws_sub_mutex.clone();
        let ws = self.ws_client.clone_subscription_handle();

        get_runtime().spawn(sync_ws_subscription_async(
            instrument_id,
            token_id_str,
            active_quote_subs,
            active_delta_subs,
            active_trade_subs,
            ws_open_tokens,
            ws_sub_mutex,
            ws,
        ));
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
        self.start_client();
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        self.stop_client();
        Ok(())
    }

    fn reset(&mut self) -> anyhow::Result<()> {
        self.reset_client();
        Ok(())
    }

    fn dispose(&mut self) -> anyhow::Result<()> {
        self.stop()
    }

    async fn connect(&mut self) -> anyhow::Result<()> {
        self.connect_client().await
    }

    async fn disconnect(&mut self) -> anyhow::Result<()> {
        self.disconnect_client().await
    }

    fn is_connected(&self) -> bool {
        self.is_connected.load(Ordering::Relaxed)
    }

    fn is_disconnected(&self) -> bool {
        !self.is_connected()
    }

    fn request_data(&self, request: RequestCustomData) -> anyhow::Result<()> {
        request_data(self, request);
        Ok(())
    }

    fn request_instruments(&self, request: RequestInstruments) -> anyhow::Result<()> {
        request_instruments(self, request);
        Ok(())
    }

    fn request_instrument(&self, request: RequestInstrument) -> anyhow::Result<()> {
        request_instrument(self, request);
        Ok(())
    }

    fn request_book_snapshot(&self, request: RequestBookSnapshot) -> anyhow::Result<()> {
        request_book_snapshot(self, request)
    }

    fn request_trades(&self, request: RequestTrades) -> anyhow::Result<()> {
        request_trades(self, request)
    }

    fn subscribe_instruments(&mut self, _cmd: SubscribeInstruments) -> anyhow::Result<()> {
        log::debug!("subscribe_instruments: subscribed individually via data subscription methods");
        Ok(())
    }

    fn subscribe(&mut self, cmd: SubscribeCustomData) -> anyhow::Result<()> {
        if !is_supported_rtds_data_type(&cmd.data_type) {
            log::debug!(
                "Ignoring unsupported Polymarket custom data subscription: {}",
                cmd.data_type
            );
            return Ok(());
        }

        log::debug!(
            "Tracking Polymarket RTDS custom data subscription: {}",
            cmd.data_type
        );
        let Some(wire) = self.rtds_feed.track_subscribe(cmd.data_type)? else {
            return Ok(());
        };

        if !self.is_connected() {
            return Ok(());
        }

        let feed = self.rtds_feed.clone();
        get_runtime().spawn(async move {
            if let Err(e) = feed.subscribe_live(wire).await {
                log::error!("Failed to subscribe RTDS custom data: {e}");
            }
        });

        Ok(())
    }

    fn subscribe_book_deltas(&mut self, cmd: SubscribeBookDeltas) -> anyhow::Result<()> {
        if cmd.book_type != BookType::L2_MBP {
            anyhow::bail!(
                "Polymarket only supports L2_MBP order book deltas, received {:?}",
                cmd.book_type
            );
        }

        let instrument_id = cmd.instrument_id;
        let cached = self.instruments.load().contains_key(&instrument_id);

        if !cached && !self.config.auto_load_missing_instruments {
            anyhow::bail!(
                "Instrument {instrument_id} not found, and `auto_load_missing_instruments` is disabled"
            );
        }

        // Mark intent before routing so unsubscribe can race-safely clear it.
        self.active_delta_subs.insert(instrument_id);
        self.order_books
            .entry(instrument_id)
            .or_insert_with(|| OrderBook::new(instrument_id, BookType::L2_MBP));

        if !cached {
            self.queue_pending_load(instrument_id);
            return Ok(());
        }

        self.sync_ws_subscription(instrument_id);
        log::debug!("Subscribed to book deltas for {instrument_id}");
        Ok(())
    }

    fn subscribe_quotes(&mut self, cmd: SubscribeQuotes) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;
        let cached = self.instruments.load().contains_key(&instrument_id);

        if !cached && !self.config.auto_load_missing_instruments {
            anyhow::bail!(
                "Instrument {instrument_id} not found, and `auto_load_missing_instruments` is disabled"
            );
        }

        self.active_quote_subs.insert(instrument_id);

        if !cached {
            self.queue_pending_load(instrument_id);
            return Ok(());
        }

        self.sync_ws_subscription(instrument_id);
        log::debug!("Subscribed to quotes for {instrument_id}");
        Ok(())
    }

    fn subscribe_trades(&mut self, cmd: SubscribeTrades) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;
        let cached = self.instruments.load().contains_key(&instrument_id);

        if !cached && !self.config.auto_load_missing_instruments {
            anyhow::bail!(
                "Instrument {instrument_id} not found, and `auto_load_missing_instruments` is disabled"
            );
        }

        self.active_trade_subs.insert(instrument_id);

        if !cached {
            self.queue_pending_load(instrument_id);
            return Ok(());
        }

        self.sync_ws_subscription(instrument_id);
        log::debug!("Subscribed to trades for {instrument_id}");
        Ok(())
    }

    fn unsubscribe_book_deltas(&mut self, cmd: &UnsubscribeBookDeltas) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;
        self.active_delta_subs.remove(&instrument_id);
        self.pending_snapshot_after_tick_change
            .remove(&instrument_id);
        self.drop_pending_if_unwanted(instrument_id);
        self.drop_local_book_state_if_unwanted(instrument_id);
        self.sync_ws_subscription(instrument_id);
        log::debug!("Unsubscribed from book deltas for {instrument_id}");
        Ok(())
    }

    fn unsubscribe_quotes(&mut self, cmd: &UnsubscribeQuotes) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;
        self.active_quote_subs.remove(&instrument_id);
        self.drop_pending_if_unwanted(instrument_id);
        self.drop_local_book_state_if_unwanted(instrument_id);
        self.sync_ws_subscription(instrument_id);
        log::debug!("Unsubscribed from quotes for {instrument_id}");
        Ok(())
    }

    fn unsubscribe_trades(&mut self, cmd: &UnsubscribeTrades) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;
        self.active_trade_subs.remove(&instrument_id);
        self.drop_pending_if_unwanted(instrument_id);
        self.sync_ws_subscription(instrument_id);
        log::debug!("Unsubscribed from trades for {instrument_id}");
        Ok(())
    }

    fn unsubscribe(&mut self, cmd: &UnsubscribeCustomData) -> anyhow::Result<()> {
        if !is_supported_rtds_data_type(&cmd.data_type) {
            log::debug!(
                "Ignoring unsupported Polymarket custom data unsubscription: {}",
                cmd.data_type
            );
            return Ok(());
        }

        log::debug!(
            "Tracking Polymarket RTDS custom data unsubscription: {}",
            cmd.data_type
        );
        let Some(wire) = self.rtds_feed.track_unsubscribe(&cmd.data_type)? else {
            return Ok(());
        };

        if !self.is_connected() {
            return Ok(());
        }

        let feed = self.rtds_feed.clone();
        get_runtime().spawn(async move {
            if let Err(e) = feed.unsubscribe_live(wire).await {
                log::error!("Failed to unsubscribe RTDS custom data: {e}");
            }
        });

        Ok(())
    }
}
