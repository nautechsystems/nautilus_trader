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
mod effective_deltas;
mod instruments;
mod lifecycle;
mod requests;
mod runtime;
mod subscriptions;

use std::{
    future::Future,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use dashmap::DashMap;
use nautilus_common::{
    cache::InstrumentLookupError,
    clients::DataClient,
    live::{dst::time::Instant, runner::get_data_event_sender, sender::EventSender},
    messages::{
        DataEvent,
        data::{
            RequestBookSnapshot, RequestCustomData, RequestInstrument, RequestInstruments,
            RequestTrades, SubscribeBookDeltas, SubscribeBookDepth, SubscribeCustomData,
            SubscribeInstrument, SubscribeInstrumentClose, SubscribeInstrumentStatus,
            SubscribeInstruments, SubscribeQuotes, SubscribeTrades, UnsubscribeBookDeltas,
            UnsubscribeCustomData, UnsubscribeInstrument, UnsubscribeInstrumentClose,
            UnsubscribeInstrumentStatus, UnsubscribeQuotes, UnsubscribeTrades,
        },
    },
    msgbus::TypedHandler,
};
use nautilus_core::{
    AtomicMap,
    time::{AtomicTime, get_atomic_clock_realtime},
};
use nautilus_live::{
    SocketControl, SocketControlFactory,
    task::{TaskGroup, TaskSpawner},
};
use nautilus_model::{
    enums::BookType,
    events::PositionEvent,
    identifiers::{ClientId, InstrumentId, Venue},
    instruments::InstrumentAny,
};
use nautilus_network::websocket::proxy::ProxyUrl;
use parking_lot::Mutex;
use tokio_util::sync::CancellationToken;
use ustr::Ustr;

pub(crate) use self::subscriptions::DataSubscriptions;
use self::{
    instruments::InstrumentUpdateState,
    requests::{
        request_book_snapshot, request_data, request_instrument, request_instruments,
        request_trades,
    },
    runtime::is_instrument_expired_and_not_reported_open,
    subscriptions::{SubscriptionKind, resolve_token_id_from},
};
use crate::{
    book::{BookSequenceOutcome, recovery::start_recovery},
    common::{consts::POLYMARKET_VENUE, credential::Credential},
    config::PolymarketDataClientConfig,
    filters::InstrumentFilter,
    http::{
        clob::PolymarketClobPublicClient, data_api::PolymarketDataApiHttpClient,
        gamma::PolymarketGammaHttpClient,
    },
    polybolt::{PolymarketPolyBoltFeed, is_supported_polybolt_data_type},
    providers::PolymarketInstrumentProvider,
    resolve::{PendingResolution, ResolveContext, ResolveWatchEntry, StrictResolvedMarket},
    websocket::{POLYBOLT_STREAMS_ENDPOINT, pool::PolymarketMarketConnectionPool},
};

const NEW_MARKET_FETCH_MAX_CONCURRENCY_CAP: usize = 64;
pub(super) const NEW_MARKET_EMPTY_RECHECK_MAX_ATTEMPTS: usize = 1;
pub(super) const NEW_MARKET_EMPTY_RECHECK_DELAY: Duration = Duration::from_millis(500);

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
    ws_client: PolymarketMarketConnectionPool,
    is_connected: AtomicBool,
    cancellation_token: CancellationToken,
    tasks: TaskGroup,
    data_sender: EventSender<DataEvent>,
    instruments: Arc<AtomicMap<InstrumentId, InstrumentAny>>,
    subscriptions: DataSubscriptions,
    instrument_update_state: Arc<Mutex<InstrumentUpdateState>>,
    resolve_poll_watchlist: Arc<AtomicMap<String, ResolveWatchEntry>>,
    resolve_watch_apply_mutex: Arc<Mutex<()>>,
    pending_resolutions: Arc<DashMap<String, PendingResolution>>,
    deferred_resolutions: Arc<AtomicMap<InstrumentId, StrictResolvedMarket>>,
    new_market_inflight_keys: Arc<DashMap<String, ()>>,
    new_market_fetch_semaphore: Arc<tokio::sync::Semaphore>,
    auto_load_scheduled: Arc<AtomicBool>,
    position_event_handler: Option<TypedHandler<PositionEvent>>,
    polybolt_feed: PolymarketPolyBoltFeed,
    polybolt_socket_control: Option<SocketControl>,
    polybolt_credential: Option<Credential>,
    proxy_url: Option<ProxyUrl>,
    reset_pending: bool,
    shutdown_errors: Vec<String>,
}

impl PolymarketDataClient {
    /// Creates a new [`PolymarketDataClient`].
    pub fn new(
        client_id: ClientId,
        config: PolymarketDataClientConfig,
        gamma_client: PolymarketGammaHttpClient,
        clob_public_client: PolymarketClobPublicClient,
        data_api_client: PolymarketDataApiHttpClient,
        ws_client: PolymarketMarketConnectionPool,
    ) -> Self {
        Self::new_with_proxy(
            client_id,
            config,
            gamma_client,
            clob_public_client,
            data_api_client,
            ws_client,
            None,
        )
    }

    /// Creates a new data client with an optional validated proxy URL.
    pub fn new_with_proxy(
        client_id: ClientId,
        mut config: PolymarketDataClientConfig,
        mut gamma_client: PolymarketGammaHttpClient,
        clob_public_client: PolymarketClobPublicClient,
        data_api_client: PolymarketDataApiHttpClient,
        ws_client: PolymarketMarketConnectionPool,
        proxy_url: Option<ProxyUrl>,
    ) -> Self {
        let clock = get_atomic_clock_realtime();
        let data_sender = get_data_event_sender();
        let socket_factory = SocketControlFactory::new(client_id, Some(*POLYMARKET_VENUE));
        let ws_client = ws_client.with_socket_factory(socket_factory.clone());
        let polybolt_socket_control = Some(socket_factory.control(POLYBOLT_STREAMS_ENDPOINT));
        gamma_client.set_clob_client(clob_public_client.clone());
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

        let polybolt_url = config.reference_prices_url();
        let polybolt_transport_backend = config.transport_backend;
        let polybolt_data_sender = data_sender.clone();

        let polybolt_credential = config.polybolt_credential().unwrap_or_else(|e| {
            log::error!("{e}; PolyBolt price subscriptions will be rejected");
            None
        });

        let tasks = TaskGroup::new();
        let cancellation_token = tasks.cancellation_token();

        let instruments = Arc::new(AtomicMap::new());
        let resolve_poll_watchlist = Arc::new(AtomicMap::new());
        let resolve_watch_apply_mutex = Arc::new(Mutex::new(()));
        let deferred_resolutions = Arc::new(AtomicMap::new());

        let subscriptions = DataSubscriptions::new(
            instruments.clone(),
            resolve_poll_watchlist.clone(),
            deferred_resolutions.clone(),
            resolve_watch_apply_mutex.clone(),
            ws_client.handle(),
            config.subscribe_new_markets,
        );

        Self {
            clock,
            client_id,
            config,
            provider,
            clob_public_client,
            data_api_client,
            ws_client,
            is_connected: AtomicBool::new(false),
            cancellation_token,
            tasks,
            data_sender,
            instruments,
            subscriptions,
            instrument_update_state: Arc::new(Mutex::new(InstrumentUpdateState::default())),
            resolve_poll_watchlist,
            resolve_watch_apply_mutex,
            pending_resolutions: Arc::new(DashMap::new()),
            deferred_resolutions,
            new_market_inflight_keys: Arc::new(DashMap::new()),
            new_market_fetch_semaphore: Arc::new(tokio::sync::Semaphore::new(
                fetch_max_concurrency,
            )),
            auto_load_scheduled: Arc::new(AtomicBool::new(false)),
            position_event_handler: None,
            polybolt_feed: PolymarketPolyBoltFeed::new_with_proxy_and_socket_control(
                polybolt_url,
                polybolt_transport_backend,
                clock,
                polybolt_data_sender,
                proxy_url.clone(),
                polybolt_socket_control.clone(),
                polybolt_credential.clone(),
            ),
            polybolt_socket_control,
            polybolt_credential,
            proxy_url,
            reset_pending: false,
            shutdown_errors: Vec::new(),
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

    #[cfg(test)]
    pub(crate) fn clob_public_client(&self) -> &PolymarketClobPublicClient {
        &self.clob_public_client
    }

    #[cfg(test)]
    pub(crate) fn data_api_client(&self) -> &PolymarketDataApiHttpClient {
        &self.data_api_client
    }

    #[cfg(test)]
    pub(crate) fn ws_client(&self) -> &PolymarketMarketConnectionPool {
        &self.ws_client
    }

    #[cfg(test)]
    pub(crate) fn polybolt_feed(&self) -> &PolymarketPolyBoltFeed {
        &self.polybolt_feed
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

    fn resolution_context(&self) -> ResolveContext {
        ResolveContext {
            subscriptions: self.subscriptions.clone(),
            clock: self.clock,
            data_sender: self.data_sender.clone(),
            instruments: self.instruments.clone(),
            watchlist: self.resolve_poll_watchlist.clone(),
            apply_mutex: self.resolve_watch_apply_mutex.clone(),
            pending_resolutions: self.pending_resolutions.clone(),
            deferred_resolutions: self.deferred_resolutions.clone(),
            cancellation_token: self.cancellation_token.clone(),
        }
    }

    fn ensure_live_subscription_allowed(&self, instrument_id: InstrumentId) -> anyhow::Result<()> {
        let now_ns = self.clock.get_time_ns();
        let loaded = self.instruments.load();
        let Some(instrument) = loaded.get(&instrument_id) else {
            return Ok(());
        };

        if is_instrument_expired_and_not_reported_open(instrument, now_ns) {
            anyhow::bail!(
                "Instrument {instrument_id} is expired and no longer available for live subscription"
            );
        }

        Ok(())
    }

    fn ensure_market_data_request_allowed(
        &self,
        instrument_id: InstrumentId,
    ) -> anyhow::Result<InstrumentAny> {
        let loaded = self.instruments.load();
        let instrument = loaded
            .get(&instrument_id)
            .ok_or_else(|| InstrumentLookupError::not_found(instrument_id))?
            .clone();

        if is_instrument_expired_and_not_reported_open(&instrument, self.clock.get_time_ns()) {
            anyhow::bail!(
                "Instrument {instrument_id} is expired and no longer available for market data requests"
            );
        }

        Ok(instrument)
    }

    fn subscribe_resolution(
        &self,
        instrument_id: InstrumentId,
        kind: SubscriptionKind,
    ) -> anyhow::Result<()> {
        // Expiration bounds recovery; it does not disqualify metadata needed for settlement
        let cached = self.instruments.load().contains_key(&instrument_id);

        if !cached && !self.config.auto_load_missing_instruments {
            anyhow::bail!(
                "Instrument {instrument_id} not found, and `auto_load_missing_instruments` is disabled"
            );
        }

        if !self
            .subscriptions
            .add_resolution_subscription_intent(instrument_id, kind)
        {
            return Ok(());
        }

        if !cached {
            self.queue_pending_load(instrument_id);
            return Ok(());
        }

        self.sync_ws_subscription(instrument_id);
        Ok(())
    }

    // Spawns an async task that reconciles the WS subscription for
    // `instrument_id`. The task holds `ws_sub_mutex` across the wire send so
    // concurrent subscribe/unsubscribe calls deliver commands to the WS handler
    // in a consistent order with the final `active_*_subs` state.
    fn sync_ws_subscription(&self, instrument_id: InstrumentId) {
        if let Ok(token_id) = self.resolve_token_id(instrument_id) {
            self.sync_ws_subscription_for_token(instrument_id, token_id);
        }
    }

    fn sync_ws_subscription_for_token(&self, instrument_id: InstrumentId, token_id_str: String) {
        let subscriptions = self.subscriptions.clone();
        let future = async move {
            subscriptions.reconcile(instrument_id, token_id_str).await;
        };

        if let Err(e) = self.tasks.spawn(future) {
            log::debug!("Skipping Polymarket data task after shutdown began: {e}");
        }
    }

    // The venue ignores a duplicate subscribe, so a token another subscription already holds
    // sends no `book` for this one; a recovery cycle requests it instead.
    fn sync_book_subscription(&self, instrument_id: InstrumentId) {
        let Ok(token_id) = self.resolve_token_id(instrument_id) else {
            return;
        };

        let spawner = match self.tasks.spawner() {
            Ok(spawner) => spawner,
            Err(e) => {
                log::debug!("Skipping Polymarket data task after shutdown began: {e}");
                return;
            }
        };

        // Read on arrival so a subscription still queued behind the subscription lock does not
        // look open; its `book` reaches this one.
        let was_open = self
            .subscriptions
            .ws_open_tokens
            .contains(&Ustr::from(token_id.as_str()));
        let resolve_ctx = self.resolution_context();
        let book_sync = self.subscriptions.book_sync.clone();
        let snapshot_timeout = Duration::from_secs(self.config.book_snapshot_timeout_secs);

        // The sync waits out any subscribe write in flight, so recovery cycles an owned token
        let future = async move {
            resolve_ctx
                .sync_ws_subscription(instrument_id, token_id.clone())
                .await;

            if was_open
                && book_sync.request_snapshot_if_subscribed(
                    &resolve_ctx.subscriptions.active_delta_subs,
                    instrument_id,
                    Instant::now(),
                ) == BookSequenceOutcome::Recover
            {
                log::debug!("Requesting a book snapshot for {instrument_id} on an open token");
                start_recovery(
                    instrument_id,
                    Some(token_id),
                    &resolve_ctx.subscriptions.active_delta_subs,
                    &book_sync,
                    &resolve_ctx.subscriptions.ws_client,
                    snapshot_timeout,
                    &spawner,
                );
            }
        };

        if let Err(e) = self.tasks.spawn(future) {
            log::debug!("Skipping Polymarket data task after shutdown began: {e}");
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

    fn subscribe_instrument(&mut self, cmd: SubscribeInstrument) -> anyhow::Result<()> {
        log::debug!(
            "Subscribed to instrument definition updates for {}; shared instrument sources remain active",
            cmd.instrument_id
        );

        Ok(())
    }

    fn unsubscribe_instrument(&mut self, cmd: &UnsubscribeInstrument) -> anyhow::Result<()> {
        log::debug!(
            "Unsubscribed from instrument {}; shared instrument sources remain active",
            cmd.instrument_id
        );
        Ok(())
    }

    fn subscribe(&mut self, cmd: SubscribeCustomData) -> anyhow::Result<()> {
        if !is_supported_polybolt_data_type(&cmd.data_type) {
            log::debug!(
                "Ignoring unsupported Polymarket custom data subscription: {}",
                cmd.data_type
            );
            return Ok(());
        }

        log::debug!(
            "Tracking Polymarket PolyBolt custom data subscription: {}",
            cmd.data_type
        );
        let changed = self.polybolt_feed.track_subscribe(cmd.data_type)?;
        if !changed {
            return Ok(());
        }

        if !self.is_connected() {
            return Ok(());
        }

        self.polybolt_feed
            .request_reconcile(crate::polybolt::ReconcileReason::DesiredChanged);

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
        self.ensure_live_subscription_allowed(instrument_id)?;
        let cached = self.instruments.load().contains_key(&instrument_id);

        if !cached && !self.config.auto_load_missing_instruments {
            anyhow::bail!(
                "Instrument {instrument_id} not found, and `auto_load_missing_instruments` is disabled"
            );
        }

        // Mark intent before routing so unsubscribe can race-safely clear it.
        if !self
            .subscriptions
            .add_delta_subscription_intent(instrument_id, self.config.compute_effective_deltas)
        {
            return Ok(());
        }

        if !cached {
            self.queue_pending_load(instrument_id);
            return Ok(());
        }

        self.sync_book_subscription(instrument_id);
        Ok(())
    }

    fn subscribe_book_depth(&mut self, _cmd: SubscribeBookDepth) -> anyhow::Result<()> {
        anyhow::bail!(
            "Polymarket does not support OrderBookDepth subscriptions; use managed L2_MBP order book deltas"
        )
    }

    fn subscribe_quotes(&mut self, cmd: SubscribeQuotes) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;
        self.ensure_live_subscription_allowed(instrument_id)?;
        let cached = self.instruments.load().contains_key(&instrument_id);

        if !cached && !self.config.auto_load_missing_instruments {
            anyhow::bail!(
                "Instrument {instrument_id} not found, and `auto_load_missing_instruments` is disabled"
            );
        }

        if !self
            .subscriptions
            .add_live_subscription_intent(instrument_id, SubscriptionKind::Quotes)
        {
            return Ok(());
        }

        if !cached {
            self.queue_pending_load(instrument_id);
            return Ok(());
        }

        self.sync_ws_subscription(instrument_id);
        Ok(())
    }

    fn subscribe_trades(&mut self, cmd: SubscribeTrades) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;
        self.ensure_live_subscription_allowed(instrument_id)?;
        let cached = self.instruments.load().contains_key(&instrument_id);

        if !cached && !self.config.auto_load_missing_instruments {
            anyhow::bail!(
                "Instrument {instrument_id} not found, and `auto_load_missing_instruments` is disabled"
            );
        }

        if !self
            .subscriptions
            .add_live_subscription_intent(instrument_id, SubscriptionKind::Trades)
        {
            return Ok(());
        }

        if !cached {
            self.queue_pending_load(instrument_id);
            return Ok(());
        }

        self.sync_ws_subscription(instrument_id);
        Ok(())
    }

    fn subscribe_instrument_status(
        &mut self,
        cmd: SubscribeInstrumentStatus,
    ) -> anyhow::Result<()> {
        self.subscribe_resolution(cmd.instrument_id, SubscriptionKind::Status)
    }

    fn subscribe_instrument_close(&mut self, cmd: SubscribeInstrumentClose) -> anyhow::Result<()> {
        self.subscribe_resolution(cmd.instrument_id, SubscriptionKind::Close)
    }

    fn unsubscribe_book_deltas(&mut self, cmd: &UnsubscribeBookDeltas) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;
        let token_id = self
            .subscriptions
            .unsubscribe(instrument_id, SubscriptionKind::Deltas);
        if let Some(token_id) = token_id {
            self.sync_ws_subscription_for_token(instrument_id, token_id);
        }

        Ok(())
    }

    fn unsubscribe_quotes(&mut self, cmd: &UnsubscribeQuotes) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;
        let token_id = self
            .subscriptions
            .unsubscribe(instrument_id, SubscriptionKind::Quotes);
        if let Some(token_id) = token_id {
            self.sync_ws_subscription_for_token(instrument_id, token_id);
        }

        Ok(())
    }

    fn unsubscribe_trades(&mut self, cmd: &UnsubscribeTrades) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;
        let token_id = self
            .subscriptions
            .unsubscribe(instrument_id, SubscriptionKind::Trades);
        if let Some(token_id) = token_id {
            self.sync_ws_subscription_for_token(instrument_id, token_id);
        }

        Ok(())
    }

    fn unsubscribe_instrument_status(
        &mut self,
        cmd: &UnsubscribeInstrumentStatus,
    ) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;
        let token_id = self
            .subscriptions
            .unsubscribe(instrument_id, SubscriptionKind::Status);
        if let Some(token_id) = token_id {
            self.sync_ws_subscription_for_token(instrument_id, token_id);
        }
        Ok(())
    }

    fn unsubscribe_instrument_close(
        &mut self,
        cmd: &UnsubscribeInstrumentClose,
    ) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;
        let token_id = self
            .subscriptions
            .unsubscribe(instrument_id, SubscriptionKind::Close);
        if let Some(token_id) = token_id {
            self.sync_ws_subscription_for_token(instrument_id, token_id);
        }
        Ok(())
    }

    fn unsubscribe(&mut self, cmd: &UnsubscribeCustomData) -> anyhow::Result<()> {
        if !is_supported_polybolt_data_type(&cmd.data_type) {
            log::debug!(
                "Ignoring unsupported Polymarket custom data unsubscription: {}",
                cmd.data_type
            );
            return Ok(());
        }

        log::debug!(
            "Tracking Polymarket PolyBolt custom data unsubscription: {}",
            cmd.data_type
        );
        let changed = self.polybolt_feed.track_unsubscribe(&cmd.data_type)?;
        if !changed {
            return Ok(());
        }

        if !self.is_connected() {
            return Ok(());
        }

        self.polybolt_feed
            .request_reconcile(crate::polybolt::ReconcileReason::DesiredChanged);

        Ok(())
    }
}

fn clamp_new_market_fetch_max_concurrency(value: usize) -> usize {
    value.clamp(1, NEW_MARKET_FETCH_MAX_CONCURRENCY_CAP)
}

pub(super) fn spawn_task<F>(tasks: &TaskSpawner, future: F)
where
    F: Future<Output = ()> + Send + 'static,
{
    if let Err(e) = tasks.spawn(future) {
        log::debug!("Skipping Polymarket data task after shutdown began: {e}");
    }
}
