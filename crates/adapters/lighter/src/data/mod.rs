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

//! Live data client for the Lighter adapter.

use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Duration,
};

use ahash::AHashMap;
use anyhow::Context;
use dashmap::{DashMap, DashSet, mapref::entry::Entry};
use nautilus_common::{
    cache::InstrumentLookupError,
    clients::DataClient,
    live::runner::get_data_event_sender,
    messages::{
        DataEvent,
        data::{
            BarsResponse, BookResponse, DataResponse, FundingRatesResponse, InstrumentResponse,
            InstrumentsResponse, RequestBars, RequestBookDepth, RequestBookSnapshot,
            RequestFundingRates, RequestInstrument, RequestInstruments, RequestQuotes,
            RequestTrades, SubscribeBars, SubscribeBookDeltas, SubscribeBookDepth10,
            SubscribeFundingRates, SubscribeIndexPrices, SubscribeInstrument,
            SubscribeInstrumentStatus, SubscribeMarkPrices, SubscribeQuotes, SubscribeTrades,
            TradesResponse, UnsubscribeBars, UnsubscribeBookDeltas, UnsubscribeBookDepth10,
            UnsubscribeFundingRates, UnsubscribeIndexPrices, UnsubscribeInstrument,
            UnsubscribeInstrumentStatus, UnsubscribeMarkPrices, UnsubscribeQuotes,
            UnsubscribeTrades,
        },
    },
};
use nautilus_core::{
    AtomicMap, UnixNanos,
    datetime::datetime_to_unix_nanos,
    time::{AtomicTime, get_atomic_clock_realtime},
};
use nautilus_live::{
    SocketControlFactory,
    task::{TaskGroup, TaskGroupGuard, TaskJoinOutcome, TaskSlot, finish_task},
};
use nautilus_model::{
    data::{Data, InstrumentStatus, TradeTick},
    enums::{BookType, MarketStatusAction},
    identifiers::{ClientId, InstrumentId, Venue},
    instruments::{Instrument, InstrumentAny},
};
use tokio_util::sync::CancellationToken;

use crate::{
    common::{
        consts::DISCONNECT_TIMEOUT,
        credential::Credential,
        enums::{LighterCandleResolution, LighterMarketStatus},
        rate_limit::resolve_quota,
        symbol::MarketRegistry,
    },
    config::LighterDataClientConfig,
    http::{
        client::{LighterHttpClient, LighterRawHttpClient},
        parse::parse_l2_order_book_snapshot,
        query::LighterOrderBookOrdersQuery,
    },
    websocket::{
        DATA_STREAMS_ENDPOINT, LighterWsError,
        client::{LighterWebSocketClient, RetainedTaskSlot, TaskRetentionGuard},
        messages::{LighterMarketSelection, LighterWsChannel, NautilusWsMessage},
    },
};

mod limits;
mod market_stats;

use self::{
    limits::{clamp_book_snapshot_limit, clamp_recent_trades_limit},
    market_stats::{
        MarketStatsKind, MarketStatsSubscription, emit_ws_message as emit_market_stats_ws_message,
        subscribe_channel as subscribe_market_stats_channel,
        unsubscribe_channel as unsubscribe_market_stats_channel,
    },
};

#[derive(Debug)]
pub struct LighterDataClient {
    clock: &'static AtomicTime,
    client_id: ClientId,
    config: LighterDataClientConfig,
    credential: Option<Credential>,
    http_client: LighterHttpClient,
    ws_client: LighterWebSocketClient,
    registry: Arc<MarketRegistry>,
    socket_factory: SocketControlFactory,
    is_connected: AtomicBool,
    cancellation_token: CancellationToken,
    tasks: TaskGroup,
    ws_disconnect_handle: TaskSlot<Result<(), LighterWsError>>,
    ws_handler_retained: Arc<RetainedTaskSlot>,
    shutdown_errors: Vec<String>,
    data_sender: tokio::sync::mpsc::UnboundedSender<DataEvent>,
    instruments: Arc<AtomicMap<InstrumentId, InstrumentAny>>,
    instrument_statuses: Arc<DashMap<InstrumentId, LighterMarketStatus>>,
    instrument_status_subscriptions: Arc<DashSet<InstrumentId>>,
    market_stats_subscriptions: Arc<DashMap<InstrumentId, MarketStatsSubscription>>,
    market_stats_subscription_generations: Arc<DashMap<InstrumentId, u64>>,
    next_market_stats_subscription_generation: AtomicU64,
}

impl LighterDataClient {
    /// Creates a new [`LighterDataClient`] instance.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP client fails to initialize.
    pub fn new(client_id: ClientId, config: LighterDataClientConfig) -> anyhow::Result<Self> {
        let clock = get_atomic_clock_realtime();
        let data_sender = get_data_event_sender();
        let venue = config.resolved_venue();
        let settlement_currency = config.settlement_currency();
        let socket_factory = SocketControlFactory::new(client_id, Some(venue));

        let credential = if config.has_credentials() {
            // Mirror `has_credentials()`: a blank or whitespace-only `private_key`
            // config value falls back to the env var rather than overriding it.
            let private_key = config
                .private_key
                .as_ref()
                .map(|value| value.expose_secret())
                .filter(|s| !s.trim().is_empty())
                .map(str::to_string);
            Credential::resolve_for_deployment(
                private_key,
                config.account_index,
                config.api_key_index,
                config.deployment,
                config.environment,
            )
            .context("failed to resolve Lighter data credentials")?
        } else {
            None
        };

        let registry = Arc::new(MarketRegistry::new_with_venue_and_settlement_currency(
            venue,
            settlement_currency,
        ));

        let raw_http = LighterRawHttpClient::new_with_quotas(
            config.environment,
            Some(config.http_url()),
            config.http_timeout_secs,
            config
                .proxy_url
                .as_ref()
                .map(|value| value.expose_secret().to_owned()),
            resolve_quota(config.rest_quota_per_min),
            None,
        )
        .context("failed to construct Lighter raw HTTP client")?;

        let http_client =
            LighterHttpClient::from_raw_with_registry(raw_http, Arc::clone(&registry));

        let ws_client = Self::create_ws_client(&config, Arc::clone(&registry), &socket_factory);

        let tasks = TaskGroup::new();

        Ok(Self {
            clock,
            client_id,
            config,
            credential,
            http_client,
            ws_client,
            registry,
            socket_factory,
            is_connected: AtomicBool::new(false),
            cancellation_token: tasks.cancellation_token(),
            tasks,
            ws_disconnect_handle: TaskSlot::new(),
            ws_handler_retained: Arc::new(RetainedTaskSlot::new()),
            shutdown_errors: Vec::new(),
            data_sender,
            instruments: Arc::new(AtomicMap::new()),
            instrument_statuses: Arc::new(DashMap::new()),
            instrument_status_subscriptions: Arc::new(DashSet::new()),
            market_stats_subscriptions: Arc::new(DashMap::new()),
            market_stats_subscription_generations: Arc::new(DashMap::new()),
            next_market_stats_subscription_generation: AtomicU64::new(1),
        })
    }

    fn venue(&self) -> Venue {
        self.config.resolved_venue()
    }

    /// Returns `true` when the data client holds resolved Lighter credentials.
    #[must_use]
    pub fn has_credentials(&self) -> bool {
        self.credential.is_some()
    }

    fn create_ws_client(
        config: &LighterDataClientConfig,
        registry: Arc<MarketRegistry>,
        socket_factory: &SocketControlFactory,
    ) -> LighterWebSocketClient {
        let ws_client = LighterWebSocketClient::new(
            Some(config.ws_url()),
            config.environment,
            registry,
            config.transport_backend,
            config.ws_timeout_secs,
            config
                .proxy_url
                .as_ref()
                .map(|value| value.expose_secret().to_owned()),
        );

        ws_client.with_socket_control(socket_factory.control(DATA_STREAMS_ENDPOINT))
    }

    fn take_ws_client(&mut self) -> LighterWebSocketClient {
        std::mem::replace(
            &mut self.ws_client,
            Self::create_ws_client(
                &self.config,
                Arc::clone(&self.registry),
                &self.socket_factory,
            ),
        )
    }

    fn spawn_ws_disconnect(&mut self) {
        if self.ws_disconnect_handle.is_some() {
            return;
        }
        self.ws_client.begin_shutdown();
        let ws_client = self.take_ws_client();
        let retained = Arc::clone(&self.ws_handler_retained);

        if let Err(e) = self
            .ws_disconnect_handle
            .spawn(ws_client.disconnect_with_task_retention(retained))
        {
            log::error!("Failed to start Lighter WebSocket disconnect task: {e}");
        }
    }

    // Biased select drops an in-flight task on cancellation before it emits a late DataEvent
    fn spawn_task<F>(&self, fut: F)
    where
        F: std::future::Future<Output = ()> + Send + 'static,
    {
        let cancellation_token = self.cancellation_token.clone();

        let future = async move {
            tokio::select! {
                biased;
                () = cancellation_token.cancelled() => {}
                () = fut => {}
            }
        };

        if let Err(e) = self.tasks.spawn(future) {
            log::debug!("Skipping Lighter data task after shutdown began: {e}");
        }
    }

    fn abort_tasks(&self) {
        self.tasks.begin_shutdown();
    }

    async fn shutdown_tasks(&mut self) -> anyhow::Result<()> {
        self.tasks.begin_shutdown();
        if let Err(e) = self
            .tasks
            .finish_shutdown(Duration::from_secs(1), DISCONNECT_TIMEOUT)
            .await
        {
            self.shutdown_errors.push(format!("data tasks failed: {e}"));
        }

        Self::finish_owned_task(
            &mut self.ws_disconnect_handle,
            "WebSocket disconnect",
            &mut self.shutdown_errors,
        )
        .await;

        if let Err(e) = self.ws_handler_retained.finish().await {
            self.shutdown_errors.push(e.to_string());
        }

        self.take_shutdown_result("Failed to terminate Lighter tasks")
    }

    fn take_shutdown_result(&mut self, context: &str) -> anyhow::Result<()> {
        if self.shutdown_errors.is_empty() {
            Ok(())
        } else {
            let errors = std::mem::take(&mut self.shutdown_errors);
            anyhow::bail!("{context}: {}", errors.join("; "))
        }
    }

    async fn finish_owned_task(
        slot: &mut TaskSlot<Result<(), LighterWsError>>,
        description: &str,
        errors: &mut Vec<String>,
    ) {
        let Some(outcome) = finish_task(slot, DISCONNECT_TIMEOUT, DISCONNECT_TIMEOUT).await else {
            return;
        };

        match outcome {
            TaskJoinOutcome::Completed(Ok(())) | TaskJoinOutcome::Aborted => {}
            TaskJoinOutcome::Completed(Err(e)) => {
                errors.push(format!("{description} failed: {e}"));
            }
            TaskJoinOutcome::Failed(e) => {
                errors.push(format!("{description} task failed: {e}"));
            }
            TaskJoinOutcome::Incomplete => {
                errors.push(format!("{description} task did not stop after abort"));
            }
        }
    }

    async fn bootstrap_instruments(&self) -> anyhow::Result<Vec<InstrumentAny>> {
        let instruments_with_status = self
            .http_client
            .request_instruments_with_status()
            .await
            .context("failed to fetch instruments during bootstrap")?;
        let instruments: Vec<InstrumentAny> = instruments_with_status
            .iter()
            .map(|(instrument, _)| instrument.clone())
            .collect();

        let mut ws_cache: Vec<(i16, InstrumentAny)> = Vec::with_capacity(instruments.len());
        self.instruments.rcu(|m| {
            for instrument in &instruments {
                m.insert(instrument.id(), instrument.clone());
            }
        });

        for instrument in &instruments {
            if let Some(market_index) = self.registry.market_index(&instrument.id()) {
                ws_cache.push((market_index, instrument.clone()));
            } else {
                log::warn!(
                    "No market_index registered for instrument {} during bootstrap",
                    instrument.id(),
                );
            }
        }

        self.instrument_statuses.clear();
        for (instrument, status) in &instruments_with_status {
            cache_lighter_instrument_status(&self.instrument_statuses, instrument.id(), *status);
        }

        self.ws_client.cache_instruments(ws_cache);

        log::debug!(
            "Bootstrapped {} Lighter instruments ({} registry entries)",
            self.instruments.len(),
            self.registry.len(),
        );
        Ok(instruments)
    }

    async fn spawn_ws(&mut self) -> anyhow::Result<()> {
        // Connect on a clone so the resulting `out_rx` (and inner handler
        // task handle) live on the consumer; transfer the handle back to
        // `self.ws_client` so disconnect() can await it.
        let mut ws_guard = TaskRetentionGuard::new(
            self.ws_client.clone(),
            Arc::clone(&self.ws_handler_retained),
        );
        ws_guard
            .client_mut()
            .connect_with_cancellation(self.cancellation_token.clone())
            .await
            .context("failed to connect to Lighter WebSocket")?;

        if let Err(e) = ws_guard.client_mut().wait_until_active().await {
            let ws_client = ws_guard.disarm();
            let mut rollback_errors = Vec::new();

            if let Err(e) = ws_client
                .disconnect_with_task_retention(Arc::clone(&self.ws_handler_retained))
                .await
            {
                rollback_errors.push(e.to_string());
            }

            if let Err(e) = self.ws_handler_retained.finish().await {
                rollback_errors.push(e.to_string());
            }

            let readiness_error =
                anyhow::Error::new(e).context("Lighter WebSocket did not reach active state");

            if rollback_errors.is_empty() {
                return Err(readiness_error);
            }
            return Err(readiness_error.context(format!(
                "Lighter WebSocket readiness rollback failed: {}",
                rollback_errors.join("; ")
            )));
        }

        let mut ws_client = ws_guard.disarm();
        self.ws_client.set_task_slot(ws_client.take_task_slot());

        let cancellation_token = self.cancellation_token.clone();
        let data_sender = self.data_sender.clone();
        let market_stats_subscriptions = Arc::clone(&self.market_stats_subscriptions);

        let future = async move {
            log::debug!("Lighter WebSocket consumption loop started");

            loop {
                tokio::select! {
                    // Prefer cancellation so a buffered frame is not forwarded after cancel
                    biased;
                    () = cancellation_token.cancelled() => {
                        log::debug!("Lighter WebSocket consumption loop cancelled");
                        break;
                    }
                    msg_opt = ws_client.next_event() => {
                        match msg_opt {
                            Some(NautilusWsMessage::Trades(trades)) => {
                                for trade in trades {
                                    if let Err(e) = data_sender
                                        .send(DataEvent::Data(Data::Trade(trade)))
                                    {
                                        log::error!("Failed to send trade tick: {e}");
                                    }
                                }
                            }
                            Some(NautilusWsMessage::Quote(quote)) => {
                                if let Err(e) = data_sender
                                    .send(DataEvent::Data(Data::Quote(quote)))
                                {
                                    log::error!("Failed to send quote tick: {e}");
                                }
                            }
                            Some(NautilusWsMessage::Deltas(deltas)) => {
                                let data = Data::BookDeltas(Box::new(deltas));
                                if let Err(e) = data_sender.send(DataEvent::Data(data)) {
                                    log::error!("Failed to send order book deltas: {e}");
                                }
                            }
                            Some(NautilusWsMessage::Depth10(depth)) => {
                                if let Err(e) =
                                    data_sender.send(DataEvent::Data(Data::BookDepth10(depth)))
                                {
                                    log::error!("Failed to send order book depth10: {e}");
                                }
                            }
                            Some(NautilusWsMessage::Bar(bar)) => {
                                if let Err(e) = data_sender.send(DataEvent::Data(Data::Bar(bar))) {
                                    log::error!("Failed to send bar: {e}");
                                }
                            }
                            Some(message @ (NautilusWsMessage::MarkPrice(_)
                                | NautilusWsMessage::IndexPrice(_)
                                | NautilusWsMessage::FundingRate(_))) =>
                            {
                                emit_market_stats_ws_message(
                                    &data_sender,
                                    &market_stats_subscriptions,
                                    &message,
                                );
                            }
                            Some(NautilusWsMessage::Raw(value)) => {
                                log::debug!("Unhandled Lighter raw frame: {value}");
                            }
                            // The data client does not consume execution-side
                            // reports; the execution client subscribes to its
                            // own clone of the WebSocket and routes them.
                            Some(
                                NautilusWsMessage::ExecutionReports(_)
                                | NautilusWsMessage::PositionSnapshot { .. }
                                | NautilusWsMessage::PositionUpdate { .. }
                                | NautilusWsMessage::AccountState(_)
                                | NautilusWsMessage::SendTxAck { .. }
                                | NautilusWsMessage::SendTxRejected { .. }
                                | NautilusWsMessage::AccountStreamFirstFrame(_),
                            ) => {}
                            Some(NautilusWsMessage::Reconnected { .. }) => {
                                log::debug!("Lighter WebSocket reconnected");
                            }
                            None => {
                                log::debug!("Lighter WebSocket next_event returned None");
                                tokio::select! {
                                    () = cancellation_token.cancelled() => {
                                        log::debug!(
                                            "Lighter WebSocket consumption loop cancelled"
                                        );
                                        break;
                                    }
                                    () = tokio::time::sleep(tokio::time::Duration::from_secs(1)) => {}
                                }
                            }
                        }
                    }
                }
            }

            log::debug!("Lighter WebSocket consumption loop finished");
        };

        self.tasks
            .spawn(future)
            .context("failed to register Lighter WebSocket consumption task")?;
        log::debug!("Lighter WebSocket consumption task spawned");

        Ok(())
    }

    fn spawn_instrument_refresh(&self) -> anyhow::Result<()> {
        let minutes = self.config.update_instruments_interval_mins;
        if minutes == 0 {
            log::debug!("Lighter instrument refresh disabled (interval=0)");
            return Ok(());
        }

        let interval = Duration::from_secs(minutes.saturating_mul(60));
        let cancellation = self.cancellation_token.clone();
        let http_client = self.http_client.clone();
        let instruments_cache = Arc::clone(&self.instruments);
        let statuses = Arc::clone(&self.instrument_statuses);
        let status_subscriptions = Arc::clone(&self.instrument_status_subscriptions);
        let registry = Arc::clone(&self.registry);
        let ws_client = self.ws_client.clone();
        let data_sender = self.data_sender.clone();
        let client_id = self.client_id;
        let clock = self.clock;

        let future = async move {
            loop {
                let sleep = tokio::time::sleep(interval);
                tokio::pin!(sleep);
                tokio::select! {
                    () = cancellation.cancelled() => {
                        log::debug!("Lighter instrument refresh task cancelled");
                        break;
                    }
                    () = &mut sleep => {
                        let Some(result) = await_instrument_refresh(
                            &cancellation,
                            http_client.request_instruments_with_status(),
                        ).await else {
                            log::debug!("Lighter instrument refresh task cancelled");
                            break;
                        };

                        match result {
                            Ok(items) => {
                                instruments_cache.rcu(|m| {
                                    for (instrument, _) in &items {
                                        m.insert(instrument.id(), instrument.clone());
                                    }
                                });

                                let ws_cache: Vec<(i16, InstrumentAny)> = items
                                    .iter()
                                    .filter_map(|(instrument, _)| {
                                        registry
                                            .market_index(&instrument.id())
                                            .map(|idx| (idx, instrument.clone()))
                                    })
                                    .collect();

                                if !ws_cache.is_empty() {
                                    ws_client.cache_instruments(ws_cache);
                                }

                                statuses.clear();
                                let ts_init = clock.get_time_ns();

                                for (instrument, status) in &items {
                                    cache_lighter_instrument_status(
                                        &statuses,
                                        instrument.id(),
                                        *status,
                                    );
                                    emit_lighter_instrument_status_if_subscribed(
                                        &data_sender,
                                        &status_subscriptions,
                                        instrument.id(),
                                        *status,
                                        ts_init,
                                        ts_init,
                                    );

                                    if let Err(e) = data_sender
                                        .send(DataEvent::Instrument(instrument.clone()))
                                    {
                                        log::warn!(
                                            "Failed to send refreshed Lighter instrument: {e}"
                                        );
                                    }
                                }

                                log::debug!(
                                    "Lighter instruments refreshed: client_id={client_id}, count={}",
                                    items.len(),
                                );
                            }
                            Err(e) => {
                                log::warn!(
                                    "Failed to refresh Lighter instruments: client_id={client_id}, error={e:?}",
                                );
                            }
                        }
                    }
                }
            }
        };

        self.tasks
            .spawn(future)
            .context("failed to register Lighter instrument refresh task")?;
        Ok(())
    }

    async fn teardown_partial_connect(&mut self) -> anyhow::Result<()> {
        self.tasks.begin_shutdown();
        self.ws_client.begin_shutdown();

        if let Err(e) = self.shutdown_tasks().await {
            self.shutdown_errors.push(e.to_string());
        }
        let ws_client = self.take_ws_client();
        if let Err(e) = ws_client
            .disconnect_with_task_retention(Arc::clone(&self.ws_handler_retained))
            .await
        {
            self.shutdown_errors.push(e.to_string());
        }

        if let Err(e) = self.ws_handler_retained.finish().await {
            self.shutdown_errors.push(e.to_string());
        }
        self.is_connected.store(false, Ordering::Release);

        self.take_shutdown_result("Failed to roll back Lighter data startup")
    }

    fn clear_market_stats_subscriptions(&self) {
        self.market_stats_subscriptions.clear();
        self.market_stats_subscription_generations.clear();
    }

    fn clear_instrument_status_subscriptions(&self) {
        self.instrument_status_subscriptions.clear();
    }

    fn emit_cached_instrument_status(&self, instrument_id: InstrumentId) -> bool {
        let Some(status) = self
            .instrument_statuses
            .get(&instrument_id)
            .map(|status| *status)
        else {
            return false;
        };

        let ts_init = self.clock.get_time_ns();
        emit_lighter_instrument_status(&self.data_sender, instrument_id, status, ts_init, ts_init);
        true
    }

    fn activate_market_stats_subscription(
        &self,
        instrument_id: InstrumentId,
        channel: LighterWsChannel,
        kind: MarketStatsKind,
        label: &'static str,
    ) {
        let generation_entry = self
            .market_stats_subscription_generations
            .entry(instrument_id)
            .or_insert_with(|| {
                self.next_market_stats_subscription_generation
                    .fetch_add(1, Ordering::Relaxed)
            });
        let generation = *generation_entry;

        let subscribe_channel = match self.market_stats_subscriptions.entry(instrument_id) {
            Entry::Occupied(mut entry) => {
                let subscription = entry.get_mut();
                let should_subscribe = subscription.flags.is_empty();
                subscription.flags.insert(kind);
                should_subscribe.then(|| subscription.channel.clone())
            }
            Entry::Vacant(entry) => {
                entry.insert(MarketStatsSubscription::new(channel.clone(), kind));
                Some(channel)
            }
        };
        drop(generation_entry);

        if let Some(channel) = subscribe_channel {
            let ws = self.ws_client.clone();
            let subscriptions = Arc::clone(&self.market_stats_subscriptions);
            let generations = Arc::clone(&self.market_stats_subscription_generations);
            self.spawn_task(async move {
                if let Err(e) = subscribe_market_stats_channel(ws, channel).await {
                    log::error!("Failed to subscribe to Lighter {label}: {e:?}");

                    // The underlying channel never became active, so clear every request
                    // piggybacked on this generation. A newer replacement is left intact.
                    rollback_market_stats_subscription(
                        &subscriptions,
                        &generations,
                        instrument_id,
                        generation,
                    );
                }
            });
        }
    }

    fn deactivate_market_stats_subscription(
        &self,
        instrument_id: InstrumentId,
        kind: MarketStatsKind,
        label: &'static str,
    ) {
        // Hold the shard lock across removal so a concurrent activate cannot re-add an erased flag
        let generation = self
            .market_stats_subscription_generations
            .entry(instrument_id);
        let unsubscribe_channel = match self.market_stats_subscriptions.entry(instrument_id) {
            Entry::Occupied(mut entry) => {
                entry.get_mut().flags.remove(kind);
                if entry.get().flags.is_empty() {
                    if let Entry::Occupied(generation) = generation {
                        generation.remove();
                    }
                    Some(entry.remove().channel)
                } else {
                    None
                }
            }
            Entry::Vacant(_) => None,
        };

        if let Some(channel) = unsubscribe_channel {
            let ws = self.ws_client.clone();
            self.spawn_task(async move {
                if let Err(e) = unsubscribe_market_stats_channel(ws, channel).await {
                    log::error!("Failed to unsubscribe from Lighter {label}: {e:?}");
                }
            });
        }
    }

    fn perp_market_stats_channel(
        &self,
        instrument_id: InstrumentId,
        label: &str,
    ) -> anyhow::Result<LighterWsChannel> {
        let instrument = self
            .instruments
            .get_cloned(&instrument_id)
            .ok_or_else(|| InstrumentLookupError::not_found(instrument_id))?;

        anyhow::ensure!(
            matches!(instrument, InstrumentAny::CryptoPerpetual(_)),
            "Lighter {label} subscriptions require a perpetual instrument: {instrument_id}",
        );

        let market_index = self.registry.market_index(&instrument_id).ok_or_else(|| {
            anyhow::anyhow!("No Lighter market_index registered for {instrument_id}")
        })?;

        Ok(LighterWsChannel::MarketStats(
            LighterMarketSelection::Market(market_index),
        ))
    }

    fn index_market_stats_channel(
        &self,
        instrument_id: InstrumentId,
    ) -> anyhow::Result<LighterWsChannel> {
        let instrument = self
            .instruments
            .get_cloned(&instrument_id)
            .ok_or_else(|| InstrumentLookupError::not_found(instrument_id))?;
        let market_index = self.registry.market_index(&instrument_id).ok_or_else(|| {
            anyhow::anyhow!("No Lighter market_index registered for {instrument_id}")
        })?;

        match instrument {
            InstrumentAny::CryptoPerpetual(_) => Ok(LighterWsChannel::MarketStats(
                LighterMarketSelection::Market(market_index),
            )),
            InstrumentAny::CurrencyPair(_) => Ok(LighterWsChannel::SpotMarketStats(
                LighterMarketSelection::Market(market_index),
            )),
            _ => anyhow::bail!(
                "Lighter index price subscriptions require a perpetual or spot instrument: {instrument_id}",
            ),
        }
    }
}

async fn await_instrument_refresh<T>(
    cancellation: &CancellationToken,
    request: impl std::future::Future<Output = T>,
) -> Option<T> {
    tokio::select! {
        biased;
        () = cancellation.cancelled() => None,
        result = request => (!cancellation.is_cancelled()).then_some(result),
    }
}

fn cache_lighter_instrument_status(
    statuses: &DashMap<InstrumentId, LighterMarketStatus>,
    instrument_id: InstrumentId,
    status: LighterMarketStatus,
) {
    statuses.insert(instrument_id, status);
}

fn rollback_market_stats_subscription(
    subscriptions: &DashMap<InstrumentId, MarketStatsSubscription>,
    generations: &DashMap<InstrumentId, u64>,
    instrument_id: InstrumentId,
    failed_generation: u64,
) {
    let Entry::Occupied(generation) = generations.entry(instrument_id) else {
        return;
    };

    if *generation.get() != failed_generation {
        return;
    }

    subscriptions.remove(&instrument_id);
    generation.remove();
}

fn emit_lighter_instrument_status_if_subscribed(
    sender: &tokio::sync::mpsc::UnboundedSender<DataEvent>,
    subscriptions: &DashSet<InstrumentId>,
    instrument_id: InstrumentId,
    status: LighterMarketStatus,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
) {
    if subscriptions.contains(&instrument_id) {
        emit_lighter_instrument_status(sender, instrument_id, status, ts_event, ts_init);
    }
}

fn emit_lighter_instrument_status(
    sender: &tokio::sync::mpsc::UnboundedSender<DataEvent>,
    instrument_id: InstrumentId,
    status: LighterMarketStatus,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
) {
    let action = lighter_market_status_action(status);
    let is_trading = Some(matches!(action, MarketStatusAction::Trading));
    let status = InstrumentStatus::new(
        instrument_id,
        action,
        ts_event,
        ts_init,
        None,
        None,
        is_trading,
        None,
        None,
    );

    if let Err(e) = sender.send(DataEvent::InstrumentStatus(status)) {
        log::error!("Failed to send Lighter instrument status: {e}");
    }
}

fn lighter_market_status_action(status: LighterMarketStatus) -> MarketStatusAction {
    match status {
        LighterMarketStatus::Active => MarketStatusAction::Trading,
        LighterMarketStatus::Inactive => MarketStatusAction::NotAvailableForTrading,
    }
}

#[async_trait::async_trait(?Send)]
impl DataClient for LighterDataClient {
    fn client_id(&self) -> ClientId {
        self.client_id
    }

    fn venue(&self) -> Option<Venue> {
        Some(self.venue())
    }

    fn start(&mut self) -> anyhow::Result<()> {
        log::info!(
            "Starting Lighter data client: client_id={}, environment={:?}, has_credentials={}",
            self.client_id,
            self.config.environment,
            self.has_credentials(),
        );
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        log::info!("Stopping Lighter data client {}", self.client_id);
        self.abort_tasks();
        self.spawn_ws_disconnect();
        self.is_connected.store(false, Ordering::Release);
        self.clear_instrument_status_subscriptions();
        self.clear_market_stats_subscriptions();
        Ok(())
    }

    fn reset(&mut self) -> anyhow::Result<()> {
        log::debug!("Resetting Lighter data client {}", self.client_id);
        self.abort_tasks();
        self.spawn_ws_disconnect();
        self.is_connected.store(false, Ordering::Release);
        self.clear_instrument_status_subscriptions();
        self.clear_market_stats_subscriptions();
        Ok(())
    }

    fn dispose(&mut self) -> anyhow::Result<()> {
        log::debug!("Disposing Lighter data client {}", self.client_id);
        self.stop()
    }

    fn is_connected(&self) -> bool {
        self.is_connected.load(Ordering::Acquire)
    }

    fn is_disconnected(&self) -> bool {
        !self.is_connected()
    }

    async fn connect(&mut self) -> anyhow::Result<()> {
        if self.is_connected()
            && self.tasks.is_open()
            && self.ws_disconnect_handle.is_none()
            && self.ws_handler_retained.is_empty()
        {
            return Ok(());
        }

        if !self.tasks.is_open()
            || !self.tasks.is_empty()
            || self.ws_disconnect_handle.is_some()
            || !self.ws_handler_retained.is_empty()
        {
            self.teardown_partial_connect().await?;
        }

        if !self.tasks.is_open() {
            self.tasks.start_generation().map_err(|e| {
                anyhow::anyhow!("Failed to start Lighter data task generation: {e}")
            })?;
            self.cancellation_token = self.tasks.cancellation_token();
        }

        let ws_client = self.ws_client.clone();
        let setup_guard = TaskGroupGuard::new(&[&self.tasks], move || {
            ws_client.begin_shutdown();
        });

        let instruments = self
            .bootstrap_instruments()
            .await
            .context("failed to bootstrap Lighter instruments")?;

        for instrument in instruments {
            if let Err(e) = self.data_sender.send(DataEvent::Instrument(instrument)) {
                log::warn!("Failed to send instrument: {e}");
            }
        }

        let session_result = async {
            self.spawn_ws()
                .await
                .context("failed to spawn Lighter WebSocket consumer")?;
            self.spawn_instrument_refresh()?;
            Ok::<(), anyhow::Error>(())
        }
        .await;

        if let Err(e) = session_result {
            if let Err(teardown_error) = self.teardown_partial_connect().await {
                return Err(e.context(format!(
                    "Lighter data startup teardown failed: {teardown_error}"
                )));
            }
            return Err(e);
        }

        setup_guard.disarm();
        self.is_connected.store(true, Ordering::Relaxed);
        log::info!("Connected: client_id={}", self.client_id);

        Ok(())
    }

    async fn disconnect(&mut self) -> anyhow::Result<()> {
        if !self.is_connected()
            && self.tasks.is_empty()
            && self.tasks.is_open()
            && self.ws_disconnect_handle.is_none()
            && self.ws_handler_retained.is_empty()
            && self.shutdown_errors.is_empty()
        {
            return Ok(());
        }

        self.tasks.begin_shutdown();
        self.ws_client.begin_shutdown();
        self.clear_instrument_status_subscriptions();
        self.clear_market_stats_subscriptions();

        if let Err(e) = self.shutdown_tasks().await {
            self.shutdown_errors.push(e.to_string());
        }

        let ws_client = self.take_ws_client();
        if let Err(e) = ws_client
            .disconnect_with_task_retention(Arc::clone(&self.ws_handler_retained))
            .await
        {
            self.shutdown_errors.push(e.to_string());
        }

        if let Err(e) = self.ws_handler_retained.finish().await {
            self.shutdown_errors.push(e.to_string());
        }

        self.instruments.store(AHashMap::new());
        self.instrument_statuses.clear();
        self.registry.clear();

        self.is_connected.store(false, Ordering::Relaxed);
        log::info!("Disconnected: client_id={}", self.client_id);

        self.take_shutdown_result("Failed to disconnect Lighter data client")
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

    fn unsubscribe_instrument(&mut self, cmd: &UnsubscribeInstrument) -> anyhow::Result<()> {
        log::debug!(
            "Unsubscribing from instrument: {} (cache replay only)",
            cmd.instrument_id,
        );
        Ok(())
    }

    fn subscribe_instrument_status(
        &mut self,
        subscription: SubscribeInstrumentStatus,
    ) -> anyhow::Result<()> {
        let instrument_id = subscription.instrument_id;

        self.instrument_status_subscriptions.insert(instrument_id);
        if self.emit_cached_instrument_status(instrument_id) {
            return Ok(());
        }

        let http = self.http_client.clone();
        let ws = self.ws_client.clone();
        let registry = Arc::clone(&self.registry);
        let sender = self.data_sender.clone();
        let instruments_cache = Arc::clone(&self.instruments);
        let statuses = Arc::clone(&self.instrument_statuses);
        let subscriptions = Arc::clone(&self.instrument_status_subscriptions);
        let clock = self.clock;

        self.spawn_task(async move {
            match http.request_instrument_with_status(instrument_id).await {
                Ok((instrument, status)) => {
                    instruments_cache.rcu(|map| {
                        map.insert(instrument.id(), instrument.clone());
                    });

                    if let Some(market_index) = registry.market_index(&instrument.id()) {
                        ws.cache_instrument(market_index, instrument.clone());
                    }

                    cache_lighter_instrument_status(&statuses, instrument.id(), status);
                    let ts_init = clock.get_time_ns();
                    emit_lighter_instrument_status_if_subscribed(
                        &sender,
                        &subscriptions,
                        instrument.id(),
                        status,
                        ts_init,
                        ts_init,
                    );
                }
                Err(e) => {
                    log::error!(
                        "Failed to fetch Lighter instrument status for {instrument_id}: {e:?}"
                    );
                }
            }
        });

        Ok(())
    }

    fn subscribe_book_deltas(&mut self, subscription: SubscribeBookDeltas) -> anyhow::Result<()> {
        validate_book_deltas_subscription(subscription.book_type)?;

        let ws = self.ws_client.clone();
        let instrument_id = subscription.instrument_id;

        self.spawn_task(async move {
            if let Err(e) = ws.subscribe_book(instrument_id).await {
                log::error!("Failed to subscribe to Lighter book deltas: {e:?}");
            }
        });

        Ok(())
    }

    fn subscribe_book_depth10(&mut self, subscription: SubscribeBookDepth10) -> anyhow::Result<()> {
        log::debug!(
            "Subscribing to book depth10: {}",
            subscription.instrument_id
        );

        validate_book_depth10_subscription(subscription.book_type)?;

        let ws = self.ws_client.clone();
        let instrument_id = subscription.instrument_id;

        self.spawn_task(async move {
            if let Err(e) = ws.subscribe_book_depth10(instrument_id).await {
                log::error!("Failed to subscribe to Lighter book depth10: {e:?}");
            }
        });

        Ok(())
    }

    fn subscribe_quotes(&mut self, subscription: SubscribeQuotes) -> anyhow::Result<()> {
        let ws = self.ws_client.clone();
        let instrument_id = subscription.instrument_id;

        self.spawn_task(async move {
            if let Err(e) = ws.subscribe_quotes(instrument_id).await {
                log::error!("Failed to subscribe to Lighter quotes: {e:?}");
            }
        });

        Ok(())
    }

    fn subscribe_trades(&mut self, subscription: SubscribeTrades) -> anyhow::Result<()> {
        let ws = self.ws_client.clone();
        let instrument_id = subscription.instrument_id;

        self.spawn_task(async move {
            if let Err(e) = ws.subscribe_trades(instrument_id).await {
                log::error!("Failed to subscribe to Lighter trades: {e:?}");
            }
        });

        Ok(())
    }

    fn subscribe_mark_prices(&mut self, subscription: SubscribeMarkPrices) -> anyhow::Result<()> {
        let instrument_id = subscription.instrument_id;

        let channel = self.perp_market_stats_channel(instrument_id, "mark price")?;
        self.activate_market_stats_subscription(
            instrument_id,
            channel,
            MarketStatsKind::MarkPrice,
            "mark price",
        );

        Ok(())
    }

    fn subscribe_index_prices(&mut self, subscription: SubscribeIndexPrices) -> anyhow::Result<()> {
        let instrument_id = subscription.instrument_id;

        let channel = self.index_market_stats_channel(instrument_id)?;
        self.activate_market_stats_subscription(
            instrument_id,
            channel,
            MarketStatsKind::IndexPrice,
            "index price",
        );

        Ok(())
    }

    fn subscribe_funding_rates(
        &mut self,
        subscription: SubscribeFundingRates,
    ) -> anyhow::Result<()> {
        let instrument_id = subscription.instrument_id;

        let channel = self.perp_market_stats_channel(instrument_id, "funding rate")?;
        self.activate_market_stats_subscription(
            instrument_id,
            channel,
            MarketStatsKind::FundingRate,
            "funding rate",
        );

        Ok(())
    }

    fn subscribe_bars(&mut self, subscription: SubscribeBars) -> anyhow::Result<()> {
        let bar_type = subscription.bar_type;

        let resolution = LighterCandleResolution::try_from(&bar_type)?;
        anyhow::ensure!(
            resolution.is_ws_streamable(),
            "Lighter does not offer {bar_type} on the candle WebSocket stream",
        );

        let instrument_id = bar_type.instrument_id();
        if !self.instruments.contains_key(&instrument_id) {
            return Err(InstrumentLookupError::not_found(instrument_id).into());
        }

        let ws = self.ws_client.clone();
        self.spawn_task(async move {
            if let Err(e) = ws.subscribe_candles(instrument_id, resolution).await {
                log::error!("Failed to subscribe to Lighter candles for {bar_type}: {e:?}");
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

        self.spawn_task(async move {
            if let Err(e) = ws.unsubscribe_book(instrument_id).await {
                log::error!("Failed to unsubscribe from Lighter book deltas: {e:?}");
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

        self.spawn_task(async move {
            if let Err(e) = ws.unsubscribe_book_depth10(instrument_id).await {
                log::error!("Failed to unsubscribe from Lighter book depth10: {e:?}");
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

        self.spawn_task(async move {
            if let Err(e) = ws.unsubscribe_quotes(instrument_id).await {
                log::error!("Failed to unsubscribe from Lighter quotes: {e:?}");
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

        self.spawn_task(async move {
            if let Err(e) = ws.unsubscribe_trades(instrument_id).await {
                log::error!("Failed to unsubscribe from Lighter trades: {e:?}");
            }
        });

        Ok(())
    }

    fn unsubscribe_instrument_status(
        &mut self,
        unsubscription: &UnsubscribeInstrumentStatus,
    ) -> anyhow::Result<()> {
        let instrument_id = unsubscription.instrument_id;

        self.instrument_status_subscriptions.remove(&instrument_id);

        Ok(())
    }

    fn unsubscribe_mark_prices(
        &mut self,
        unsubscription: &UnsubscribeMarkPrices,
    ) -> anyhow::Result<()> {
        let instrument_id = unsubscription.instrument_id;

        self.deactivate_market_stats_subscription(
            instrument_id,
            MarketStatsKind::MarkPrice,
            "mark price",
        );

        Ok(())
    }

    fn unsubscribe_index_prices(
        &mut self,
        unsubscription: &UnsubscribeIndexPrices,
    ) -> anyhow::Result<()> {
        let instrument_id = unsubscription.instrument_id;

        self.deactivate_market_stats_subscription(
            instrument_id,
            MarketStatsKind::IndexPrice,
            "index price",
        );

        Ok(())
    }

    fn unsubscribe_funding_rates(
        &mut self,
        unsubscription: &UnsubscribeFundingRates,
    ) -> anyhow::Result<()> {
        let instrument_id = unsubscription.instrument_id;

        self.deactivate_market_stats_subscription(
            instrument_id,
            MarketStatsKind::FundingRate,
            "funding rate",
        );

        Ok(())
    }

    fn unsubscribe_bars(&mut self, unsubscription: &UnsubscribeBars) -> anyhow::Result<()> {
        let bar_type = unsubscription.bar_type;

        let resolution = match LighterCandleResolution::try_from(&bar_type) {
            Ok(resolution) => resolution,
            Err(e) => {
                log::warn!("Skipping Lighter candle unsubscribe for {bar_type}: {e}");
                return Ok(());
            }
        };

        let instrument_id = bar_type.instrument_id();
        let ws = self.ws_client.clone();
        self.spawn_task(async move {
            if let Err(e) = ws.unsubscribe_candles(instrument_id, resolution).await {
                log::error!("Failed to unsubscribe from Lighter candles for {bar_type}: {e:?}");
            }
        });

        Ok(())
    }

    fn request_instruments(&self, request: RequestInstruments) -> anyhow::Result<()> {
        log::debug!("Requesting Lighter instruments");

        let http = self.http_client.clone();
        let ws = self.ws_client.clone();
        let registry = Arc::clone(&self.registry);
        let sender = self.data_sender.clone();
        let instruments_cache = Arc::clone(&self.instruments);
        let status_cache = Arc::clone(&self.instrument_statuses);
        let status_subscriptions = Arc::clone(&self.instrument_status_subscriptions);
        let request_id = request.request_id;
        let client_id = request.client_id.unwrap_or(self.client_id);
        let venue = self.venue();
        let start_nanos = datetime_to_unix_nanos(request.start);
        let end_nanos = datetime_to_unix_nanos(request.end);
        let params = request.params;
        let clock = self.clock;

        self.spawn_task(async move {
            match http.request_instruments_with_status().await {
                Ok(instruments_with_status) => {
                    let instruments: Vec<InstrumentAny> = instruments_with_status
                        .iter()
                        .map(|(instrument, _)| instrument.clone())
                        .collect();

                    instruments_cache.rcu(|map| {
                        for instrument in &instruments {
                            map.insert(instrument.id(), instrument.clone());
                        }
                    });

                    let ws_cache: Vec<(i16, InstrumentAny)> = instruments
                        .iter()
                        .filter_map(|i| registry.market_index(&i.id()).map(|idx| (idx, i.clone())))
                        .collect();

                    if !ws_cache.is_empty() {
                        ws.cache_instruments(ws_cache);
                    }

                    status_cache.clear();
                    let ts_init = clock.get_time_ns();

                    for (instrument, status) in &instruments_with_status {
                        cache_lighter_instrument_status(&status_cache, instrument.id(), *status);
                        emit_lighter_instrument_status_if_subscribed(
                            &sender,
                            &status_subscriptions,
                            instrument.id(),
                            *status,
                            ts_init,
                            ts_init,
                        );
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
                    log::error!("Failed to fetch Lighter instruments: {e:?}");
                }
            }
        });

        Ok(())
    }

    fn request_instrument(&self, request: RequestInstrument) -> anyhow::Result<()> {
        log::debug!("Requesting Lighter instrument: {}", request.instrument_id);

        let http = self.http_client.clone();
        let ws = self.ws_client.clone();
        let registry = Arc::clone(&self.registry);
        let sender = self.data_sender.clone();
        let instruments_cache = Arc::clone(&self.instruments);
        let status_cache = Arc::clone(&self.instrument_statuses);
        let status_subscriptions = Arc::clone(&self.instrument_status_subscriptions);
        let instrument_id = request.instrument_id;
        let request_id = request.request_id;
        let client_id = request.client_id.unwrap_or(self.client_id);
        let start_nanos = datetime_to_unix_nanos(request.start);
        let end_nanos = datetime_to_unix_nanos(request.end);
        let params = request.params;
        let clock = self.clock;

        self.spawn_task(async move {
            match http.request_instrument_with_status(instrument_id).await {
                Ok((instrument, status)) => {
                    instruments_cache.rcu(|map| {
                        map.insert(instrument.id(), instrument.clone());
                    });

                    if let Some(market_index) = registry.market_index(&instrument.id()) {
                        ws.cache_instrument(market_index, instrument.clone());
                    }

                    cache_lighter_instrument_status(&status_cache, instrument.id(), status);
                    let ts_init = clock.get_time_ns();
                    emit_lighter_instrument_status_if_subscribed(
                        &sender,
                        &status_subscriptions,
                        instrument.id(),
                        status,
                        ts_init,
                        ts_init,
                    );

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
                    log::error!("Failed to fetch Lighter instrument {instrument_id}: {e:?}");
                }
            }
        });

        Ok(())
    }

    fn request_bars(&self, request: RequestBars) -> anyhow::Result<()> {
        let bar_type = request.bar_type;
        log::debug!("Requesting Lighter bars for {bar_type}");

        LighterCandleResolution::try_from(&bar_type)?;

        let instrument_id = bar_type.instrument_id();
        let instrument = self
            .instruments
            .get_cloned(&instrument_id)
            .ok_or_else(|| InstrumentLookupError::not_found(instrument_id))?;

        let http = self.http_client.clone();
        let sender = self.data_sender.clone();
        let start = request.start;
        let end = request.end;
        let limit = request.limit.map(|n| n.get() as u32);
        let request_id = request.request_id;
        let client_id = request.client_id.unwrap_or(self.client_id);
        let params = request.params;
        let clock = self.clock;
        let start_nanos = datetime_to_unix_nanos(start);
        let end_nanos = datetime_to_unix_nanos(end);

        self.spawn_task(async move {
            match http
                .request_bars(&instrument, bar_type, start, end, limit)
                .await
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
                Err(e) => {
                    log::error!("Lighter bars request failed for {instrument_id}: {e:?}");
                }
            }
        });

        Ok(())
    }

    fn request_quotes(&self, request: RequestQuotes) -> anyhow::Result<()> {
        anyhow::bail!(
            "Lighter does not support historical quote requests for {}; \
             subscribe to quotes via WebSocket for live BBO ticks",
            request.instrument_id,
        )
    }

    fn request_trades(&self, request: RequestTrades) -> anyhow::Result<()> {
        let instrument_id = request.instrument_id;
        log::debug!("Requesting Lighter trades for {instrument_id}");

        let instrument = self
            .instruments
            .get_cloned(&instrument_id)
            .ok_or_else(|| InstrumentLookupError::not_found(instrument_id))?;

        let http = self.http_client.clone();
        let sender = self.data_sender.clone();
        let request_id = request.request_id;
        let client_id = request.client_id.unwrap_or(self.client_id);
        let limit = clamp_recent_trades_limit(request.limit);
        let start_nanos = datetime_to_unix_nanos(request.start);
        let end_nanos = datetime_to_unix_nanos(request.end);
        let params = request.params;
        let clock = self.clock;

        self.spawn_task(async move {
            match http.request_recent_trades(&instrument, limit).await {
                Ok(mut trades) => {
                    retain_trade_ticks_in_range(&mut trades, start_nanos, end_nanos);

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
                Err(e) => {
                    log::error!("Lighter trades request failed for {instrument_id}: {e}");
                }
            }
        });

        Ok(())
    }

    fn request_funding_rates(&self, request: RequestFundingRates) -> anyhow::Result<()> {
        let instrument_id = request.instrument_id;
        log::debug!("Requesting Lighter funding rates for {instrument_id}");

        let instrument = self
            .instruments
            .get_cloned(&instrument_id)
            .ok_or_else(|| InstrumentLookupError::not_found(instrument_id))?;

        anyhow::ensure!(
            matches!(instrument, InstrumentAny::CryptoPerpetual(_)),
            "Lighter funding-rate requests require a perpetual instrument: {instrument_id}",
        );

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

        self.spawn_task(async move {
            match http
                .request_funding_rates(&instrument, start, end, limit)
                .await
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
                Err(e) => {
                    log::error!("Lighter funding rates request failed for {instrument_id}: {e:?}");
                }
            }
        });

        Ok(())
    }

    fn request_book_snapshot(&self, request: RequestBookSnapshot) -> anyhow::Result<()> {
        let instrument_id = request.instrument_id;
        log::debug!("Requesting Lighter book snapshot for {instrument_id}");

        let instrument = self
            .instruments
            .get_cloned(&instrument_id)
            .ok_or_else(|| InstrumentLookupError::not_found(instrument_id))?;

        let market_index = self.registry.market_index(&instrument_id).ok_or_else(|| {
            anyhow::anyhow!("No Lighter market_index registered for {instrument_id}")
        })?;

        let http = self.http_client.clone();
        let sender = self.data_sender.clone();
        let request_id = request.request_id;
        let client_id = request.client_id.unwrap_or(self.client_id);
        let limit = clamp_book_snapshot_limit(request.depth);
        let params = request.params;
        let clock = self.clock;
        let price_precision = instrument.price_precision();
        let size_precision = instrument.size_precision();

        let query = LighterOrderBookOrdersQuery {
            market_id: market_index,
            limit,
        };

        self.spawn_task(async move {
            match http.inner.get_order_book_orders(&query).await {
                Ok(snapshot) => {
                    let ts_init = clock.get_time_ns();
                    let book = parse_l2_order_book_snapshot(
                        &snapshot,
                        instrument_id,
                        price_precision,
                        size_precision,
                    );

                    let response = DataResponse::Book(BookResponse::new(
                        request_id,
                        client_id,
                        instrument_id,
                        book,
                        None,
                        None,
                        ts_init,
                        params,
                    ));

                    if let Err(e) = sender.send(DataEvent::Response(response)) {
                        log::error!("Failed to send book snapshot response: {e}");
                    }
                }
                Err(e) => {
                    log::error!("Lighter book snapshot request failed for {instrument_id}: {e:?}");
                }
            }
        });

        Ok(())
    }

    fn request_book_depth(&self, request: RequestBookDepth) -> anyhow::Result<()> {
        anyhow::bail!(
            "Lighter does not support historical order book depth requests for {}; \
             use request_book_snapshot for an L2 snapshot or subscribe_book_depth10 for live depth10",
            request.instrument_id,
        )
    }
}

fn retain_trade_ticks_in_range(
    trades: &mut Vec<TradeTick>,
    start_nanos: Option<UnixNanos>,
    end_nanos: Option<UnixNanos>,
) {
    trades.retain(|trade| trade_tick_in_range(trade.ts_event, start_nanos, end_nanos));
    trades.sort_by_key(|trade| trade.ts_event);
}

fn trade_tick_in_range(
    ts_event: UnixNanos,
    start_nanos: Option<UnixNanos>,
    end_nanos: Option<UnixNanos>,
) -> bool {
    start_nanos.is_none_or(|start| ts_event >= start) && end_nanos.is_none_or(|end| ts_event <= end)
}

/// Returns an error if `book_type` is not [`BookType::L2_MBP`].
///
/// Lighter publishes only level-aggregated book updates, so any other book
/// type cannot be served by the WebSocket feed.
fn validate_book_deltas_subscription(book_type: BookType) -> anyhow::Result<()> {
    validate_l2_mbp_book_type(book_type, "deltas")
}

fn validate_book_depth10_subscription(book_type: BookType) -> anyhow::Result<()> {
    validate_l2_mbp_book_type(book_type, "depth10")
}

fn validate_l2_mbp_book_type(book_type: BookType, label: &str) -> anyhow::Result<()> {
    anyhow::ensure!(
        book_type == BookType::L2_MBP,
        "Lighter only supports L2_MBP order book {label}",
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{num::NonZeroUsize, time::Duration};

    use axum::{
        Router,
        extract::Query,
        http::StatusCode,
        response::{IntoResponse, Response},
        routing::get,
    };
    use jiff::Timestamp;
    use nautilus_common::live::runner::replace_data_event_sender;
    use nautilus_core::UUID4;
    use nautilus_model::{
        data::{
            BarSpecification, BarType, FundingRateUpdate, IndexPriceUpdate, MarkPriceUpdate,
            TradeTick,
        },
        enums::{AggregationSource, AggressorSide, BarAggregation, PriceType},
        identifiers::{InstrumentId, Symbol, TradeId},
        instruments::{CryptoPerpetual, CurrencyPair},
        types::{Currency, Price, Quantity},
    };
    use rstest::rstest;
    use rust_decimal::Decimal;

    use super::{
        limits::{LIGHTER_BOOK_ORDERS_MAX_LIMIT, LIGHTER_RECENT_TRADES_MAX_LIMIT},
        market_stats::{MarketStatsFlags, MarketStatsSubscription},
        *,
    };
    use crate::{
        common::{
            consts::LIGHTER_VENUE,
            enums::{LighterFundingResolution, LighterProductType},
        },
        http::query::{LighterFundingsQuery, LighterRecentTradesQuery},
    };

    struct DropSignal(Option<tokio::sync::oneshot::Sender<()>>);

    impl Drop for DropSignal {
        fn drop(&mut self) {
            if let Some(sender) = self.0.take() {
                let _ = sender.send(());
            }
        }
    }

    const HTTP_ORDER_BOOK_DETAILS: &str =
        include_str!("../../test_data/http_order_book_details.json");
    const HTTP_FUNDINGS: &str = include_str!("../../test_data/http_fundings.json");
    const HTTP_RECENT_TRADES: &str = include_str!("../../test_data/http_recent_trades.json");
    const HTTP_RECENT_TRADES_NULL: &str =
        include_str!("../../test_data/http_recent_trades_null.json");
    const HTTP_RECENT_TRADES_UNORDERED: &str =
        include_str!("../../test_data/http_recent_trades_unordered.json");
    const PRIVATE_KEY_HEX: &str =
        "0b8e0f63c24d8baacd9d29ad4e9a4b73c4a8d2bb8b16dc4fa9d7c2e1d3a8b1f0e8d3a4c5b6e7f001";

    #[rstest]
    #[case::none_defaults_to_cap(None, LIGHTER_BOOK_ORDERS_MAX_LIMIT)]
    #[case::below_cap_passes_through(Some(10), 10)]
    #[case::at_cap_passes_through(
        Some(LIGHTER_BOOK_ORDERS_MAX_LIMIT as usize),
        LIGHTER_BOOK_ORDERS_MAX_LIMIT
    )]
    #[case::above_cap_clamps(Some(500), LIGHTER_BOOK_ORDERS_MAX_LIMIT)]
    #[case::usize_max_clamps(Some(usize::MAX), LIGHTER_BOOK_ORDERS_MAX_LIMIT)]
    fn test_clamp_book_snapshot_limit(#[case] depth: Option<usize>, #[case] expected: u16) {
        let depth = depth.map(|n| NonZeroUsize::new(n).expect("non-zero"));
        assert_eq!(clamp_book_snapshot_limit(depth), expected);
    }

    #[rstest]
    #[case::none_defaults_to_cap(None, LIGHTER_RECENT_TRADES_MAX_LIMIT)]
    #[case::below_cap_passes_through(Some(10), 10)]
    #[case::at_cap_passes_through(
        Some(LIGHTER_RECENT_TRADES_MAX_LIMIT as usize),
        LIGHTER_RECENT_TRADES_MAX_LIMIT
    )]
    #[case::above_cap_clamps(Some(500), LIGHTER_RECENT_TRADES_MAX_LIMIT)]
    #[case::usize_max_clamps(Some(usize::MAX), LIGHTER_RECENT_TRADES_MAX_LIMIT)]
    fn test_clamp_recent_trades_limit(#[case] limit: Option<usize>, #[case] expected: u16) {
        let limit = limit.map(|n| NonZeroUsize::new(n).expect("non-zero"));
        assert_eq!(clamp_recent_trades_limit(limit), expected);
    }

    #[rstest]
    fn test_new_uses_readonly_websocket_url() {
        let client = create_data_client_for_test();

        assert_eq!(
            client.ws_client.url(),
            "wss://mainnet.zklighter.elliot.ai/stream?readonly=true",
        );
    }

    #[rstest]
    fn test_validate_book_deltas_accepts_l2_mbp() {
        assert!(validate_book_deltas_subscription(BookType::L2_MBP).is_ok());
    }

    #[rstest]
    #[case(BookType::L1_MBP)]
    #[case(BookType::L3_MBO)]
    fn test_validate_book_deltas_rejects_other_book_types(#[case] book_type: BookType) {
        let err = validate_book_deltas_subscription(book_type).unwrap_err();
        assert!(
            err.to_string().contains("L2_MBP"),
            "expected error to cite L2_MBP, was: {err}",
        );
    }

    #[rstest]
    fn test_validate_book_depth10_accepts_l2_mbp() {
        assert!(validate_book_depth10_subscription(BookType::L2_MBP).is_ok());
    }

    #[rstest]
    #[case(BookType::L1_MBP)]
    #[case(BookType::L3_MBO)]
    fn test_validate_book_depth10_rejects_other_book_types(#[case] book_type: BookType) {
        let err = validate_book_depth10_subscription(book_type).unwrap_err();
        assert!(
            err.to_string().contains("depth10"),
            "expected error to cite depth10, was: {err}",
        );
    }

    #[rstest]
    #[case(LighterMarketStatus::Active, MarketStatusAction::Trading)]
    #[case(
        LighterMarketStatus::Inactive,
        MarketStatusAction::NotAvailableForTrading
    )]
    fn test_lighter_market_status_action(
        #[case] status: LighterMarketStatus,
        #[case] expected: MarketStatusAction,
    ) {
        assert_eq!(lighter_market_status_action(status), expected);
    }

    #[tokio::test]
    async fn test_subscribe_instrument_status_replays_cached_status() {
        let (mut client, mut receiver) = create_data_client_with_receiver_for_test();
        let instrument_id = cache_test_instrument(&client, 0, "ETH", LighterProductType::Perp);
        cache_lighter_instrument_status(
            &client.instrument_statuses,
            instrument_id,
            LighterMarketStatus::Active,
        );

        DataClient::subscribe_instrument_status(
            &mut client,
            SubscribeInstrumentStatus::new(
                instrument_id,
                Some(ClientId::new("LIGHTER")),
                None,
                UUID4::new(),
                UnixNanos::default(),
                None,
                None,
            ),
        )
        .unwrap();

        let event = receiver.recv().await.expect("instrument status event");
        match event {
            DataEvent::InstrumentStatus(status) => {
                assert_eq!(status.instrument_id, instrument_id);
                assert_eq!(status.action, MarketStatusAction::Trading);
                assert_eq!(status.is_trading, Some(true));
            }
            event => panic!("expected instrument status, was {event:?}"),
        }
    }

    #[tokio::test]
    async fn test_subscribe_instrument_status_fetches_when_cache_is_empty() {
        let base_url = spawn_order_book_details_server().await;
        let config = LighterDataClientConfig {
            base_url_http: Some(base_url),
            ..Default::default()
        };
        let (mut client, mut receiver) =
            create_data_client_with_receiver_and_config_for_test(config);
        let instrument_id = client.registry.insert(0, "ETH", LighterProductType::Perp);

        DataClient::subscribe_instrument_status(
            &mut client,
            SubscribeInstrumentStatus::new(
                instrument_id,
                Some(ClientId::new("LIGHTER")),
                None,
                UUID4::new(),
                UnixNanos::default(),
                None,
                None,
            ),
        )
        .unwrap();

        let event = tokio::time::timeout(Duration::from_secs(2), receiver.recv())
            .await
            .expect("instrument status response")
            .expect("instrument status event");

        match event {
            DataEvent::InstrumentStatus(status) => {
                assert_eq!(status.instrument_id, instrument_id);
                assert_eq!(status.action, MarketStatusAction::Trading);
                assert_eq!(status.is_trading, Some(true));
            }
            event => panic!("expected instrument status, was {event:?}"),
        }
        assert!(client.instruments.get_cloned(&instrument_id).is_some());
        assert_eq!(
            client
                .instrument_statuses
                .get(&instrument_id)
                .map(|status| *status),
            Some(LighterMarketStatus::Active),
        );
    }

    #[tokio::test]
    async fn test_market_stats_subscriptions_share_perp_channel_until_last_unsub() {
        let mut client = create_data_client_for_test();
        // Prevent the unconnected test client from asynchronously rolling back local flags
        client.cancellation_token.cancel();
        let instrument_id = cache_test_instrument(&client, 0, "ETH", LighterProductType::Perp);

        DataClient::subscribe_mark_prices(
            &mut client,
            SubscribeMarkPrices::new(
                instrument_id,
                Some(ClientId::new("LIGHTER")),
                None,
                UUID4::new(),
                UnixNanos::default(),
                None,
                None,
            ),
        )
        .unwrap();
        DataClient::subscribe_index_prices(
            &mut client,
            SubscribeIndexPrices::new(
                instrument_id,
                Some(ClientId::new("LIGHTER")),
                None,
                UUID4::new(),
                UnixNanos::default(),
                None,
                None,
            ),
        )
        .unwrap();
        DataClient::subscribe_funding_rates(
            &mut client,
            SubscribeFundingRates::new(
                instrument_id,
                Some(ClientId::new("LIGHTER")),
                None,
                UUID4::new(),
                UnixNanos::default(),
                None,
                None,
            ),
        )
        .unwrap();

        let subscription = client
            .market_stats_subscriptions
            .get(&instrument_id)
            .expect("market stats subscription");
        assert_eq!(
            subscription.flags,
            MarketStatsFlags {
                mark_price: true,
                index_price: true,
                funding_rate: true,
            },
        );
        assert!(matches!(
            subscription.channel,
            LighterWsChannel::MarketStats(LighterMarketSelection::Market(0)),
        ));
        drop(subscription);
        assert!(
            client
                .market_stats_subscription_generations
                .contains_key(&instrument_id),
        );

        DataClient::unsubscribe_mark_prices(
            &mut client,
            &UnsubscribeMarkPrices::new(
                instrument_id,
                Some(ClientId::new("LIGHTER")),
                None,
                UUID4::new(),
                UnixNanos::default(),
                None,
                None,
            ),
        )
        .unwrap();
        assert_eq!(
            client
                .market_stats_subscriptions
                .get(&instrument_id)
                .expect("index and funding still active")
                .flags,
            MarketStatsFlags {
                index_price: true,
                funding_rate: true,
                ..Default::default()
            },
        );

        DataClient::unsubscribe_index_prices(
            &mut client,
            &UnsubscribeIndexPrices::new(
                instrument_id,
                Some(ClientId::new("LIGHTER")),
                None,
                UUID4::new(),
                UnixNanos::default(),
                None,
                None,
            ),
        )
        .unwrap();
        assert_eq!(
            client
                .market_stats_subscriptions
                .get(&instrument_id)
                .expect("funding still active")
                .flags,
            MarketStatsFlags {
                funding_rate: true,
                ..Default::default()
            },
        );

        DataClient::unsubscribe_funding_rates(
            &mut client,
            &UnsubscribeFundingRates::new(
                instrument_id,
                Some(ClientId::new("LIGHTER")),
                None,
                UUID4::new(),
                UnixNanos::default(),
                None,
                None,
            ),
        )
        .unwrap();
        assert!(
            !client
                .market_stats_subscriptions
                .contains_key(&instrument_id)
        );
        assert!(
            !client
                .market_stats_subscription_generations
                .contains_key(&instrument_id),
        );
    }

    #[rstest]
    fn test_market_stats_ws_forwarding_requires_matching_subscription() {
        let subscriptions = DashMap::new();
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        let instrument_id = InstrumentId::new(Symbol::new("ETH-PERP"), *LIGHTER_VENUE);
        let other_instrument_id = InstrumentId::new(Symbol::new("BTC-PERP"), *LIGHTER_VENUE);

        subscriptions.insert(
            instrument_id,
            MarketStatsSubscription {
                channel: LighterWsChannel::MarketStats(LighterMarketSelection::Market(0)),
                flags: MarketStatsFlags {
                    mark_price: true,
                    index_price: true,
                    funding_rate: true,
                },
            },
        );

        assert!(emit_market_stats_ws_message(
            &sender,
            &subscriptions,
            &NautilusWsMessage::MarkPrice(MarkPriceUpdate::new(
                instrument_id,
                Price::from("2000.00"),
                UnixNanos::from(10),
                UnixNanos::from(1),
            )),
        ));
        assert!(emit_market_stats_ws_message(
            &sender,
            &subscriptions,
            &NautilusWsMessage::IndexPrice(IndexPriceUpdate::new(
                instrument_id,
                Price::from("1999.50"),
                UnixNanos::from(11),
                UnixNanos::from(1),
            )),
        ));
        assert!(emit_market_stats_ws_message(
            &sender,
            &subscriptions,
            &NautilusWsMessage::FundingRate(FundingRateUpdate::new(
                instrument_id,
                Decimal::new(12, 6),
                None,
                Some(UnixNanos::from(100)),
                UnixNanos::from(12),
                UnixNanos::from(1),
            )),
        ));

        match receiver.try_recv().unwrap() {
            DataEvent::Data(Data::MarkPrice(update)) => {
                assert_eq!(update.instrument_id, instrument_id);
                assert_eq!(update.value, Price::from("2000.00"));
            }
            event => panic!("expected mark price update, was {event:?}"),
        }

        match receiver.try_recv().unwrap() {
            DataEvent::Data(Data::IndexPrice(update)) => {
                assert_eq!(update.instrument_id, instrument_id);
                assert_eq!(update.value, Price::from("1999.50"));
            }
            event => panic!("expected index price update, was {event:?}"),
        }

        match receiver.try_recv().unwrap() {
            DataEvent::FundingRate(update) => {
                assert_eq!(update.instrument_id, instrument_id);
                assert_eq!(update.rate, Decimal::new(12, 6));
            }
            event => panic!("expected funding rate update, was {event:?}"),
        }

        assert!(!emit_market_stats_ws_message(
            &sender,
            &subscriptions,
            &NautilusWsMessage::MarkPrice(MarkPriceUpdate::new(
                other_instrument_id,
                Price::from("1.00"),
                UnixNanos::from(13),
                UnixNanos::from(1),
            )),
        ));
        assert!(receiver.try_recv().is_err());
    }

    #[rstest]
    fn test_index_market_stats_channel_uses_spot_stream_for_spot_instrument() {
        let client = create_data_client_for_test();
        let instrument_id = cache_test_instrument(&client, 2048, "ETH", LighterProductType::Spot);

        let channel = client.index_market_stats_channel(instrument_id).unwrap();

        assert!(matches!(
            channel,
            LighterWsChannel::SpotMarketStats(LighterMarketSelection::Market(2048)),
        ));
    }

    #[rstest]
    fn test_mark_price_channel_rejects_spot_instrument() {
        let client = create_data_client_for_test();
        let instrument_id = cache_test_instrument(&client, 2048, "ETH", LighterProductType::Spot);

        let err = client
            .perp_market_stats_channel(instrument_id, "mark price")
            .unwrap_err();

        assert!(
            err.to_string()
                .contains("mark price subscriptions require a perpetual instrument"),
        );
    }

    #[rstest]
    fn test_request_bars_rejects_unsupported_bar_type() {
        let client = create_data_client_for_test();
        let request = RequestBars::new(
            unsupported_three_minute_bar_type(),
            None,
            None,
            None,
            Some(ClientId::new("LIGHTER")),
            UUID4::new(),
            UnixNanos::default(),
            None,
        );

        let err = DataClient::request_bars(&client, request).unwrap_err();

        assert_eq!(err.to_string(), "unsupported Lighter candle minute step: 3");
    }

    #[rstest]
    fn test_subscribe_bars_rejects_unsupported_bar_type() {
        let mut client = create_data_client_for_test();
        let subscription = SubscribeBars::new(
            unsupported_three_minute_bar_type(),
            Some(ClientId::new("LIGHTER")),
            None,
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        );

        let err = DataClient::subscribe_bars(&mut client, subscription).unwrap_err();

        assert_eq!(err.to_string(), "unsupported Lighter candle minute step: 3");
    }

    #[rstest]
    fn test_subscribe_bars_accepts_ws_streamable_resolution() {
        let mut client = create_data_client_for_test();
        let instrument_id = cache_test_instrument(&client, 0, "ETH", LighterProductType::Perp);
        let bar_type = BarType::new(
            instrument_id,
            BarSpecification::new(1, BarAggregation::Minute, PriceType::Last),
            AggregationSource::External,
        );
        let subscription = SubscribeBars::new(
            bar_type,
            Some(ClientId::new("LIGHTER")),
            None,
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        );

        DataClient::subscribe_bars(&mut client, subscription).unwrap();
    }

    #[rstest]
    fn test_subscribe_bars_missing_cached_instrument_returns_lookup_error() {
        let mut client = create_data_client_for_test();
        let instrument_id = InstrumentId::new(Symbol::new("ETH-PERP"), *LIGHTER_VENUE);
        let bar_type = BarType::new(
            instrument_id,
            BarSpecification::new(1, BarAggregation::Minute, PriceType::Last),
            AggregationSource::External,
        );
        let subscription = SubscribeBars::new(
            bar_type,
            Some(ClientId::new("LIGHTER")),
            None,
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        );

        let err = DataClient::subscribe_bars(&mut client, subscription).unwrap_err();

        assert_eq!(
            err.to_string(),
            InstrumentLookupError::not_found(instrument_id).to_string()
        );
    }

    #[rstest]
    fn test_subscribe_bars_rejects_one_week_with_ws_message() {
        let mut client = create_data_client_for_test();
        let instrument_id = cache_test_instrument(&client, 0, "ETH", LighterProductType::Perp);
        let bar_type = BarType::new(
            instrument_id,
            BarSpecification::new(1, BarAggregation::Week, PriceType::Last),
            AggregationSource::External,
        );
        let subscription = SubscribeBars::new(
            bar_type,
            Some(ClientId::new("LIGHTER")),
            None,
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        );

        let err = DataClient::subscribe_bars(&mut client, subscription).unwrap_err();

        assert!(
            err.to_string().contains("does not offer")
                && err.to_string().contains("candle WebSocket stream"),
            "expected WS-streamable rejection, was: {err}",
        );
    }

    #[rstest]
    fn test_unsubscribe_bars_returns_ok_for_unsupported_bar_type() {
        let mut client = create_data_client_for_test();
        let unsubscription = UnsubscribeBars::new(
            unsupported_three_minute_bar_type(),
            Some(ClientId::new("LIGHTER")),
            None,
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        );

        DataClient::unsubscribe_bars(&mut client, &unsubscription).unwrap();
    }

    #[rstest]
    fn test_subscribe_book_depth10_rejects_unsupported_book_type() {
        let mut client = create_data_client_for_test();
        let instrument_id = InstrumentId::new(Symbol::new("ETH-PERP"), *LIGHTER_VENUE);
        let subscription = SubscribeBookDepth10::new(
            instrument_id,
            BookType::L1_MBP,
            Some(ClientId::new("LIGHTER")),
            None,
            UUID4::new(),
            UnixNanos::default(),
            None,
            false,
            None,
            None,
        );

        let err = DataClient::subscribe_book_depth10(&mut client, subscription).unwrap_err();

        assert!(err.to_string().contains("L2_MBP"));
    }

    #[rstest]
    fn test_request_quotes_rejects_unsupported_rest_quotes() {
        let client = create_data_client_for_test();
        let instrument_id = InstrumentId::new(Symbol::new("ETH-PERP"), *LIGHTER_VENUE);
        let request = RequestQuotes::new(
            instrument_id,
            None,
            None,
            None,
            Some(ClientId::new("LIGHTER")),
            UUID4::new(),
            UnixNanos::default(),
            None,
        );

        let err = DataClient::request_quotes(&client, request).unwrap_err();

        assert!(
            err.to_string()
                .contains("does not support historical quote requests"),
        );
    }

    #[rstest]
    fn test_request_book_depth_rejects_unsupported_rest_depth() {
        let client = create_data_client_for_test();
        let instrument_id = InstrumentId::new(Symbol::new("ETH-PERP"), *LIGHTER_VENUE);
        let request = RequestBookDepth::new(
            instrument_id,
            None,
            None,
            None,
            NonZeroUsize::new(10),
            Some(ClientId::new("LIGHTER")),
            UUID4::new(),
            UnixNanos::default(),
            None,
        );

        let err = DataClient::request_book_depth(&client, request).unwrap_err();

        assert!(
            err.to_string()
                .contains("does not support historical order book depth requests"),
        );
    }

    #[rstest]
    fn test_request_funding_rates_rejects_spot_instrument() {
        let client = create_data_client_for_test();
        let instrument_id = cache_test_instrument(&client, 2048, "ETH", LighterProductType::Spot);
        let request = RequestFundingRates::new(
            instrument_id,
            None,
            None,
            None,
            Some(ClientId::new("LIGHTER")),
            UUID4::new(),
            UnixNanos::default(),
            None,
        );

        let err = DataClient::request_funding_rates(&client, request).unwrap_err();

        assert!(
            err.to_string()
                .contains("funding-rate requests require a perpetual instrument"),
        );
    }

    #[tokio::test]
    async fn test_request_funding_rates_emits_response() {
        let base_url = spawn_fundings_server().await;
        let config = LighterDataClientConfig {
            base_url_http: Some(base_url),
            ..Default::default()
        };
        let (client, mut receiver) = create_data_client_with_receiver_and_config_for_test(config);
        let instrument_id = cache_test_instrument(&client, 0, "ETH", LighterProductType::Perp);
        let start = Timestamp::from_second(1_778_702_400).unwrap();
        let end = Timestamp::from_second(1_778_706_000).unwrap();
        let request = RequestFundingRates::new(
            instrument_id,
            Some(start),
            Some(end),
            NonZeroUsize::new(2),
            Some(ClientId::new("LIGHTER")),
            UUID4::new(),
            UnixNanos::default(),
            None,
        );

        DataClient::request_funding_rates(&client, request).unwrap();

        let event = tokio::time::timeout(Duration::from_secs(2), receiver.recv())
            .await
            .expect("funding rates response")
            .expect("funding rates event");

        match event {
            DataEvent::Response(DataResponse::FundingRates(response)) => {
                assert_eq!(response.instrument_id, instrument_id);
                assert_eq!(response.data.len(), 2);
                assert_eq!(response.data[0].rate, Decimal::new(12, 4));
                assert_eq!(response.data[0].interval, Some(60));
                assert_eq!(
                    response.data[0].ts_event,
                    UnixNanos::from(1_778_702_400_000_000_000)
                );
                assert_eq!(response.data[1].rate, Decimal::new(-2, 4));
                assert_eq!(response.data[1].interval, Some(60));
            }
            event => panic!("expected funding rates response, was {event:?}"),
        }
    }

    #[tokio::test]
    async fn test_request_trades_uses_recent_trades_endpoint() {
        let base_url = spawn_trades_server().await;
        let config = LighterDataClientConfig {
            base_url_http: Some(base_url),
            ..Default::default()
        };
        let (client, mut receiver) = create_data_client_with_receiver_and_config_for_test(config);
        let instrument_id = cache_test_instrument(&client, 0, "ETH", LighterProductType::Perp);
        let start = Timestamp::from_second(1_700_000_000).unwrap();
        let request = RequestTrades::new(
            instrument_id,
            Some(start),
            None,
            NonZeroUsize::new(50),
            Some(ClientId::new("LIGHTER")),
            UUID4::new(),
            UnixNanos::default(),
            None,
        );

        DataClient::request_trades(&client, request).unwrap();

        let event = tokio::time::timeout(Duration::from_secs(2), receiver.recv())
            .await
            .expect("trades response")
            .expect("trades event");

        match event {
            DataEvent::Response(DataResponse::Trades(response)) => {
                assert_eq!(response.instrument_id, instrument_id);
                assert_eq!(response.data.len(), 1);
                let tick = &response.data[0];
                assert_eq!(tick.instrument_id, instrument_id);
                assert_eq!(tick.price, Price::from("2361.31"));
                assert_eq!(tick.size, Quantity::from("0.0005"));
                assert_eq!(tick.aggressor_side, AggressorSide::Sell);
                assert_eq!(tick.trade_id.to_string(), "19211490282");
            }
            event => panic!("expected trades response, was {event:?}"),
        }
    }

    #[tokio::test]
    async fn test_request_trades_clamps_limit_to_venue_cap() {
        let base_url = spawn_trades_server_with_response_and_limit(
            HTTP_RECENT_TRADES,
            LIGHTER_RECENT_TRADES_MAX_LIMIT,
        )
        .await;
        let config = LighterDataClientConfig {
            base_url_http: Some(base_url),
            ..Default::default()
        };
        let (client, mut receiver) = create_data_client_with_receiver_and_config_for_test(config);
        let instrument_id = cache_test_instrument(&client, 0, "ETH", LighterProductType::Perp);
        let request = RequestTrades::new(
            instrument_id,
            None,
            None,
            NonZeroUsize::new(usize::from(LIGHTER_RECENT_TRADES_MAX_LIMIT) + 1),
            Some(ClientId::new("LIGHTER")),
            UUID4::new(),
            UnixNanos::default(),
            None,
        );

        DataClient::request_trades(&client, request).unwrap();

        let event = tokio::time::timeout(Duration::from_secs(2), receiver.recv())
            .await
            .expect("trades response")
            .expect("trades event");

        assert!(
            matches!(event, DataEvent::Response(DataResponse::Trades(_))),
            "expected trades response, was {event:?}",
        );
    }

    #[tokio::test]
    async fn test_request_trades_emits_empty_response_for_null_recent_trades() {
        let base_url = spawn_trades_server_with_response(HTTP_RECENT_TRADES_NULL).await;
        let config = LighterDataClientConfig {
            base_url_http: Some(base_url),
            ..Default::default()
        };
        let (client, mut receiver) = create_data_client_with_receiver_and_config_for_test(config);
        let instrument_id = cache_test_instrument(&client, 0, "ETH", LighterProductType::Perp);
        let start = Timestamp::from_second(1_700_000_000).unwrap();
        let request = RequestTrades::new(
            instrument_id,
            Some(start),
            None,
            NonZeroUsize::new(50),
            Some(ClientId::new("LIGHTER")),
            UUID4::new(),
            UnixNanos::default(),
            None,
        );

        DataClient::request_trades(&client, request).unwrap();

        let event = tokio::time::timeout(Duration::from_secs(2), receiver.recv())
            .await
            .expect("trades response")
            .expect("trades event");

        match event {
            DataEvent::Response(DataResponse::Trades(response)) => {
                assert_eq!(response.instrument_id, instrument_id);
                assert!(response.data.is_empty());
            }
            event => panic!("expected trades response, was {event:?}"),
        }
    }

    #[tokio::test]
    async fn test_request_trades_filters_recent_trades_to_requested_range() {
        let base_url = spawn_trades_server().await;
        let config = LighterDataClientConfig {
            base_url_http: Some(base_url),
            ..Default::default()
        };
        let (client, mut receiver) = create_data_client_with_receiver_and_config_for_test(config);
        let instrument_id = cache_test_instrument(&client, 0, "ETH", LighterProductType::Perp);
        let end = Timestamp::from_second(1_700_000_000).unwrap();
        let request = RequestTrades::new(
            instrument_id,
            None,
            Some(end),
            NonZeroUsize::new(50),
            Some(ClientId::new("LIGHTER")),
            UUID4::new(),
            UnixNanos::default(),
            None,
        );

        DataClient::request_trades(&client, request).unwrap();

        let event = tokio::time::timeout(Duration::from_secs(2), receiver.recv())
            .await
            .expect("trades response")
            .expect("trades event");

        match event {
            DataEvent::Response(DataResponse::Trades(response)) => {
                assert_eq!(response.instrument_id, instrument_id);
                assert!(response.data.is_empty());
            }
            event => panic!("expected trades response, was {event:?}"),
        }
    }

    #[tokio::test]
    async fn test_request_trades_returns_recent_trades_in_timestamp_order() {
        let base_url = spawn_trades_server_with_response(HTTP_RECENT_TRADES_UNORDERED).await;
        let config = LighterDataClientConfig {
            base_url_http: Some(base_url),
            ..Default::default()
        };
        let (client, mut receiver) = create_data_client_with_receiver_and_config_for_test(config);
        let instrument_id = cache_test_instrument(&client, 0, "ETH", LighterProductType::Perp);
        let start = Timestamp::from_millisecond(1_777_945_103_092).unwrap();
        let end = Timestamp::from_millisecond(1_777_945_103_094).unwrap();
        let request = RequestTrades::new(
            instrument_id,
            Some(start),
            Some(end),
            NonZeroUsize::new(50),
            Some(ClientId::new("LIGHTER")),
            UUID4::new(),
            UnixNanos::default(),
            None,
        );

        DataClient::request_trades(&client, request).unwrap();

        let event = tokio::time::timeout(Duration::from_secs(2), receiver.recv())
            .await
            .expect("trades response")
            .expect("trades event");

        match event {
            DataEvent::Response(DataResponse::Trades(response)) => {
                assert_eq!(response.instrument_id, instrument_id);
                assert_eq!(
                    response
                        .data
                        .iter()
                        .map(|trade| trade.trade_id.to_string())
                        .collect::<Vec<_>>(),
                    vec!["19211490282", "19211490283", "19211490284"],
                );
                assert_eq!(
                    response
                        .data
                        .iter()
                        .map(|trade| trade.ts_event.as_u64())
                        .collect::<Vec<_>>(),
                    vec![
                        1_777_945_103_092_000_000,
                        1_777_945_103_093_000_000,
                        1_777_945_103_094_000_000,
                    ],
                );
            }
            event => panic!("expected trades response, was {event:?}"),
        }
    }

    #[rstest]
    fn test_retain_trade_ticks_in_range_sorts_ascending() {
        let instrument_id = InstrumentId::new(Symbol::new("ETH-PERP"), *LIGHTER_VENUE);
        let tick = |ts_event, trade_id| {
            TradeTick::new(
                instrument_id,
                Price::from("1.0"),
                Quantity::from("1.0"),
                AggressorSide::Buy,
                TradeId::new(trade_id),
                UnixNanos::from(ts_event),
                UnixNanos::from(ts_event + 1),
            )
        };
        let mut trades = vec![tick(4, "4"), tick(1, "1"), tick(3, "3"), tick(2, "2")];

        retain_trade_ticks_in_range(
            &mut trades,
            Some(UnixNanos::from(2)),
            Some(UnixNanos::from(4)),
        );

        assert_eq!(
            trades
                .iter()
                .map(|trade| trade.ts_event.as_u64())
                .collect::<Vec<_>>(),
            vec![2, 3, 4],
        );
    }

    #[tokio::test]
    async fn test_spawn_instrument_refresh_skipped_when_interval_zero() {
        let config = LighterDataClientConfig {
            update_instruments_interval_mins: 0,
            ..Default::default()
        };
        let (client, _receiver) = create_data_client_with_receiver_and_config_for_test(config);

        assert!(client.tasks.is_empty());
        client
            .spawn_instrument_refresh()
            .expect("instrument refresh remains disabled");
        assert!(client.tasks.is_empty());
    }

    #[tokio::test]
    async fn test_spawn_instrument_refresh_registers_task() {
        let config = LighterDataClientConfig {
            update_instruments_interval_mins: 60,
            ..Default::default()
        };
        let (mut client, _receiver) = create_data_client_with_receiver_and_config_for_test(config);

        assert!(client.tasks.is_empty());
        client
            .spawn_instrument_refresh()
            .expect("instrument refresh task registration");
        assert_eq!(client.tasks.len(), 1);

        client.tasks.begin_shutdown();
        client.shutdown_tasks().await.expect("task shutdown");
    }

    #[tokio::test]
    async fn test_await_instrument_refresh_drops_result_when_request_cancels() {
        let cancellation = CancellationToken::new();
        let request_cancellation = cancellation.clone();

        let result = await_instrument_refresh(&cancellation, async move {
            request_cancellation.cancel();
            42
        })
        .await;

        assert_eq!(result, None);
    }

    #[tokio::test]
    async fn test_reset_closes_registered_task_generation_until_drain() {
        let (mut client, _receiver) = create_data_client_with_receiver_for_test();
        let old_token = client.cancellation_token.clone();
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (dropped_tx, dropped_rx) = tokio::sync::oneshot::channel();

        client
            .tasks
            .spawn(async move {
                let _drop_signal = DropSignal(Some(dropped_tx));
                let _ = started_tx.send(());
                std::future::pending::<()>().await;
            })
            .expect("registered task spawn");
        started_rx.await.expect("registered task started");

        client.reset().expect("reset");

        assert!(old_token.is_cancelled());
        assert_eq!(client.tasks.len(), 1);
        assert!(!client.tasks.is_open());
        assert!(client.cancellation_token.is_cancelled());
        client.shutdown_tasks().await.expect("reset task shutdown");
        assert!(client.tasks.is_empty());
        tokio::time::timeout(Duration::from_secs(2), dropped_rx)
            .await
            .expect("registered task was not aborted")
            .expect("drop signal sender dropped");
    }

    #[tokio::test]
    async fn test_spawn_task_suppresses_output_after_cancellation() {
        let (client, mut receiver) = create_data_client_with_receiver_for_test();
        let instrument_id = cache_test_instrument(&client, 0, "ETH", LighterProductType::Perp);
        let instrument = client
            .instruments
            .get_cloned(&instrument_id)
            .expect("cached instrument");

        // Cancel before the spawn so the biased select drops the future before it can send
        client.cancellation_token.cancel();

        let sender = client.data_sender.clone();
        client.spawn_task(async move {
            let _ = sender.send(DataEvent::Instrument(instrument));
        });

        let result = tokio::time::timeout(Duration::from_millis(200), receiver.recv()).await;
        assert!(
            result.is_err(),
            "expected no DataEvent after cancellation, was {result:?}",
        );
    }

    #[tokio::test]
    async fn test_connect_is_idempotent_when_already_connected() {
        let (mut client, _receiver) = create_data_client_with_receiver_for_test();
        client.is_connected.store(true, Ordering::Release);

        client
            .connect()
            .await
            .expect("connect returns Ok when already connected");

        assert!(
            client.tasks.is_empty(),
            "an already-connected client must not spawn duplicate tasks",
        );
        assert!(client.is_connected());
    }

    #[tokio::test]
    async fn test_disconnect_drains_in_flight_task_and_suppresses_late_event() {
        let (mut client, mut receiver) = create_data_client_with_receiver_for_test();
        let instrument_id = cache_test_instrument(&client, 0, "ETH", LighterProductType::Perp);
        let instrument = client
            .instruments
            .get_cloned(&instrument_id)
            .expect("cached instrument");
        client.is_connected.store(true, Ordering::Release);

        // In-flight task (never-released barrier) that would emit; disconnect must drop it first
        let sender = client.data_sender.clone();
        let (hold_tx, hold_rx) = tokio::sync::oneshot::channel::<()>();
        client.spawn_task(async move {
            let _ = hold_rx.await;
            let _ = sender.send(DataEvent::Instrument(instrument));
        });
        assert_eq!(client.tasks.len(), 1);

        client.disconnect().await.expect("disconnect");

        assert!(
            client.tasks.is_empty(),
            "disconnect must drain tracked tasks",
        );
        assert!(!client.is_connected());
        let result = tokio::time::timeout(Duration::from_millis(200), receiver.recv()).await;
        assert!(
            result.is_err(),
            "expected no DataEvent after disconnect, was {result:?}",
        );

        drop(hold_tx);
    }

    #[tokio::test]
    async fn test_disconnect_aborts_task_that_ignores_cancellation() {
        let (mut client, _receiver) = create_data_client_with_receiver_for_test();
        client.is_connected.store(true, Ordering::Release);

        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (dropped_tx, dropped_rx) = tokio::sync::oneshot::channel();

        // The task ignores cancellation, so shutdown_tasks must abort it after the grace timeout.
        client
            .tasks
            .spawn(async move {
                let _drop_signal = DropSignal(Some(dropped_tx));
                let _ = started_tx.send(());
                std::future::pending::<()>().await;
            })
            .expect("uncancellable task spawn");
        started_rx.await.expect("task started");

        client.disconnect().await.expect("disconnect");

        assert!(client.tasks.is_empty());
        assert!(!client.is_connected());
        tokio::time::timeout(Duration::from_secs(5), dropped_rx)
            .await
            .expect("task aborted after timeout")
            .expect("drop signal sender dropped");
    }

    #[rstest]
    fn test_rollback_market_stats_subscription_clears_piggybacked_flags() {
        let subscriptions = DashMap::new();
        let generations = DashMap::new();
        let instrument_id = InstrumentId::new(Symbol::new("ETH-PERP"), *LIGHTER_VENUE);
        subscriptions.insert(
            instrument_id,
            MarketStatsSubscription {
                channel: LighterWsChannel::MarketStats(LighterMarketSelection::Market(0)),
                flags: MarketStatsFlags {
                    mark_price: true,
                    index_price: true,
                    ..Default::default()
                },
            },
        );
        generations.insert(instrument_id, 7);

        rollback_market_stats_subscription(&subscriptions, &generations, instrument_id, 7);

        assert!(
            !subscriptions.contains_key(&instrument_id),
            "all flags share the failed underlying channel",
        );
        assert!(!generations.contains_key(&instrument_id));
    }

    #[rstest]
    fn test_rollback_market_stats_subscription_keeps_replacement_generation() {
        let subscriptions = DashMap::new();
        let generations = DashMap::new();
        let instrument_id = InstrumentId::new(Symbol::new("ETH-PERP"), *LIGHTER_VENUE);
        let replacement = MarketStatsSubscription {
            channel: LighterWsChannel::MarketStats(LighterMarketSelection::Market(0)),
            flags: MarketStatsFlags {
                funding_rate: true,
                ..Default::default()
            },
        };
        subscriptions.insert(instrument_id, replacement.clone());
        generations.insert(instrument_id, 8);

        rollback_market_stats_subscription(&subscriptions, &generations, instrument_id, 7);

        assert_eq!(
            subscriptions
                .get(&instrument_id)
                .expect("replacement retained")
                .flags,
            replacement.flags,
        );
        assert_eq!(generations.get(&instrument_id).map(|value| *value), Some(8));
    }

    fn create_data_client_for_test() -> LighterDataClient {
        create_data_client_with_receiver_for_test().0
    }

    fn create_data_client_with_receiver_for_test() -> (
        LighterDataClient,
        tokio::sync::mpsc::UnboundedReceiver<DataEvent>,
    ) {
        create_data_client_with_receiver_and_config_for_test(LighterDataClientConfig::default())
    }

    fn create_data_client_with_receiver_and_config_for_test(
        mut config: LighterDataClientConfig,
    ) -> (
        LighterDataClient,
        tokio::sync::mpsc::UnboundedReceiver<DataEvent>,
    ) {
        config.api_key_index = Some(5);
        config.account_index = Some(12_345);
        config.private_key = Some(PRIVATE_KEY_HEX.into());
        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
        replace_data_event_sender(sender);
        let client = LighterDataClient::new(ClientId::new("LIGHTER"), config).unwrap();
        (client, receiver)
    }

    async fn spawn_order_book_details_server() -> String {
        let app = Router::new().route("/api/v1/orderBookDetails", get(order_book_details));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        format!("http://{addr}")
    }

    async fn spawn_fundings_server() -> String {
        let app = Router::new().route("/api/v1/fundings", get(fundings));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        format!("http://{addr}")
    }

    async fn spawn_trades_server() -> String {
        spawn_trades_server_with_response(HTTP_RECENT_TRADES).await
    }

    async fn spawn_trades_server_with_response(response_body: &'static str) -> String {
        spawn_trades_server_with_response_and_limit(response_body, 50).await
    }

    async fn spawn_trades_server_with_response_and_limit(
        response_body: &'static str,
        expected_limit: u16,
    ) -> String {
        let app = Router::new().route(
            "/api/v1/recentTrades",
            get(
                move |Query(query): Query<LighterRecentTradesQuery>| async move {
                    recent_trades_response(&query, response_body, expected_limit)
                },
            ),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        format!("http://{addr}")
    }

    async fn order_book_details() -> Response {
        (StatusCode::OK, HTTP_ORDER_BOOK_DETAILS).into_response()
    }

    async fn fundings(Query(query): Query<LighterFundingsQuery>) -> Response {
        assert_eq!(query.market_id, 0);
        assert_eq!(query.resolution, LighterFundingResolution::OneHour);
        assert_eq!(query.start_timestamp, 1_778_702_400_000);
        assert_eq!(query.end_timestamp, 1_778_706_000_000);
        assert_eq!(
            query.count_back,
            i64::from(crate::http::client::LIGHTER_FUNDINGS_MAX_LIMIT)
        );
        (StatusCode::OK, HTTP_FUNDINGS).into_response()
    }

    fn recent_trades_response(
        query: &LighterRecentTradesQuery,
        response_body: &'static str,
        expected_limit: u16,
    ) -> Response {
        assert_eq!(query.market_id, 0);
        assert_eq!(query.limit, expected_limit);
        (StatusCode::OK, response_body).into_response()
    }

    fn cache_test_instrument(
        client: &LighterDataClient,
        market_index: i16,
        venue_symbol: &str,
        product_type: LighterProductType,
    ) -> InstrumentId {
        let instrument_id = client
            .registry
            .insert(market_index, venue_symbol, product_type);
        let instrument = match product_type {
            LighterProductType::Perp => test_perp_instrument(instrument_id, venue_symbol),
            LighterProductType::Spot => test_spot_instrument(instrument_id, venue_symbol),
        };

        client.instruments.rcu(|m| {
            m.insert(instrument_id, instrument.clone());
        });

        instrument_id
    }

    fn test_perp_instrument(instrument_id: InstrumentId, venue_symbol: &str) -> InstrumentAny {
        InstrumentAny::CryptoPerpetual(
            CryptoPerpetual::builder()
                .instrument_id(instrument_id)
                .raw_symbol(Symbol::new(format!("{venue_symbol}-PERP")))
                .base_currency(Currency::from(venue_symbol))
                .quote_currency(Currency::from("USDC"))
                .settlement_currency(Currency::from("USDC"))
                .is_inverse(false)
                .price_precision(2)
                .size_precision(4)
                .price_increment(Price::from("0.01"))
                .size_increment(Quantity::from("0.0001"))
                .ts_event(UnixNanos::default())
                .ts_init(UnixNanos::default())
                .build()
                .unwrap(),
        )
    }

    fn test_spot_instrument(instrument_id: InstrumentId, venue_symbol: &str) -> InstrumentAny {
        InstrumentAny::CurrencyPair(
            CurrencyPair::builder()
                .instrument_id(instrument_id)
                .raw_symbol(Symbol::new(format!("{venue_symbol}-SPOT")))
                .base_currency(Currency::from(venue_symbol))
                .quote_currency(Currency::from("USDC"))
                .price_precision(2)
                .size_precision(4)
                .price_increment(Price::from("0.01"))
                .size_increment(Quantity::from("0.0001"))
                .ts_event(UnixNanos::default())
                .ts_init(UnixNanos::default())
                .build()
                .unwrap(),
        )
    }

    fn unsupported_three_minute_bar_type() -> BarType {
        let instrument_id = InstrumentId::new(Symbol::new("ETH-PERP"), *LIGHTER_VENUE);
        BarType::new(
            instrument_id,
            BarSpecification::new(3, BarAggregation::Minute, PriceType::Last),
            AggregationSource::External,
        )
    }
}
