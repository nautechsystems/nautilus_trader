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
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use ahash::AHashMap;
use anyhow::Context;
use dashmap::{DashMap, DashSet, mapref::entry::Entry};
use nautilus_common::{
    clients::DataClient,
    live::{runner::get_data_event_sender, runtime::get_runtime},
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
use nautilus_model::{
    data::{Data, InstrumentStatus, OrderBookDeltas_API},
    enums::{BookType, MarketStatusAction},
    identifiers::{ClientId, InstrumentId, Venue},
    instruments::{Instrument, InstrumentAny},
};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::{
    common::{
        consts::LIGHTER_VENUE,
        credential::{Credential, scrub_auth},
        enums::{LighterCandleResolution, LighterMarketStatus},
        symbol::MarketRegistry,
    },
    config::LighterDataClientConfig,
    http::{
        client::{LighterHttpClient, LighterRawHttpClient},
        parse::parse_l2_order_book_snapshot,
        query::{LighterOrderBookOrdersQuery, LighterTradeSortBy, LighterTradesQuery},
    },
    signing::auth_token::build_auth_token_for,
    websocket::{
        client::LighterWebSocketClient,
        error::LighterWsError,
        messages::{LighterMarketSelection, LighterWsChannel, NautilusWsMessage},
    },
};

/// Maximum `limit` accepted by `GET /api/v1/orderBookOrders` (venue-imposed).
const LIGHTER_BOOK_ORDERS_MAX_LIMIT: u16 = 250;
const DEFAULT_BOOK_SNAPSHOT_LIMIT: u16 = LIGHTER_BOOK_ORDERS_MAX_LIMIT;
const DEFAULT_TRADES_LIMIT: u16 = 100;

/// Which slice of the Lighter `market_stats` payload a caller has subscribed to.
///
/// The venue streams mark price, index price, and funding rate through the same
/// `market_stats` channel, so a single subscription can fan out to up to three
/// Nautilus subscriptions. [`MarketStatsFlags`] tracks which ones are active.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum MarketStatsKind {
    MarkPrice,
    IndexPrice,
    FundingRate,
}

/// Per-instrument fan-out state for the shared `market_stats` subscription.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
struct MarketStatsFlags {
    mark_price: bool,
    index_price: bool,
    funding_rate: bool,
}

impl MarketStatsFlags {
    fn is_empty(self) -> bool {
        !self.mark_price && !self.index_price && !self.funding_rate
    }

    fn contains(self, kind: MarketStatsKind) -> bool {
        match kind {
            MarketStatsKind::MarkPrice => self.mark_price,
            MarketStatsKind::IndexPrice => self.index_price,
            MarketStatsKind::FundingRate => self.funding_rate,
        }
    }

    fn insert(&mut self, kind: MarketStatsKind) {
        match kind {
            MarketStatsKind::MarkPrice => self.mark_price = true,
            MarketStatsKind::IndexPrice => self.index_price = true,
            MarketStatsKind::FundingRate => self.funding_rate = true,
        }
    }

    fn remove(&mut self, kind: MarketStatsKind) {
        match kind {
            MarketStatsKind::MarkPrice => self.mark_price = false,
            MarketStatsKind::IndexPrice => self.index_price = false,
            MarketStatsKind::FundingRate => self.funding_rate = false,
        }
    }
}

impl From<MarketStatsKind> for MarketStatsFlags {
    fn from(kind: MarketStatsKind) -> Self {
        let mut flags = Self::default();
        flags.insert(kind);
        flags
    }
}

#[derive(Debug, Clone)]
struct MarketStatsSubscription {
    channel: LighterWsChannel,
    flags: MarketStatsFlags,
}

#[derive(Debug)]
pub struct LighterDataClient {
    clock: &'static AtomicTime,
    client_id: ClientId,
    config: LighterDataClientConfig,
    credential: Option<Credential>,
    http_client: LighterHttpClient,
    ws_client: LighterWebSocketClient,
    registry: Arc<MarketRegistry>,
    is_connected: AtomicBool,
    cancellation_token: CancellationToken,
    tasks: Vec<JoinHandle<()>>,
    data_sender: tokio::sync::mpsc::UnboundedSender<DataEvent>,
    instruments: Arc<AtomicMap<InstrumentId, InstrumentAny>>,
    instrument_statuses: Arc<DashMap<InstrumentId, LighterMarketStatus>>,
    instrument_status_subscriptions: Arc<DashSet<InstrumentId>>,
    market_stats_subscriptions: Arc<DashMap<InstrumentId, MarketStatsSubscription>>,
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

        let credential = if config.has_credentials() {
            // Mirror `has_credentials()`: a blank or whitespace-only `private_key`
            // config value falls back to the env var rather than overriding it.
            let private_key = config
                .private_key
                .as_deref()
                .filter(|s| !s.trim().is_empty())
                .map(str::to_string);
            Credential::resolve(
                private_key,
                config.account_index,
                config.api_key_index,
                config.environment,
            )
            .context("failed to resolve Lighter data credentials")?
        } else {
            None
        };

        let registry = Arc::new(MarketRegistry::new());

        let raw_http = LighterRawHttpClient::new(
            config.environment,
            config.base_url_http.clone(),
            config.http_timeout_secs,
            config.proxy_url.clone(),
        )
        .context("failed to construct Lighter raw HTTP client")?;
        let http_client =
            LighterHttpClient::from_raw_with_registry(raw_http, Arc::clone(&registry));

        let ws_client = LighterWebSocketClient::new(
            Some(config.ws_url()),
            config.environment,
            Arc::clone(&registry),
            config.transport_backend,
            config.proxy_url.clone(),
        );

        Ok(Self {
            clock,
            client_id,
            config,
            credential,
            http_client,
            ws_client,
            registry,
            is_connected: AtomicBool::new(false),
            cancellation_token: CancellationToken::new(),
            tasks: Vec::new(),
            data_sender,
            instruments: Arc::new(AtomicMap::new()),
            instrument_statuses: Arc::new(DashMap::new()),
            instrument_status_subscriptions: Arc::new(DashSet::new()),
            market_stats_subscriptions: Arc::new(DashMap::new()),
        })
    }

    fn venue(&self) -> Venue {
        *LIGHTER_VENUE
    }

    /// Returns `true` when the data client holds resolved Lighter credentials.
    #[must_use]
    pub fn has_credentials(&self) -> bool {
        self.credential.is_some()
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
        let mut ws_client = self.ws_client.clone();
        ws_client
            .connect()
            .await
            .context("failed to connect to Lighter WebSocket")?;

        if let Some(handle) = ws_client.take_task_handle() {
            self.ws_client.set_task_handle(handle);
        }

        let cancellation_token = self.cancellation_token.clone();
        let data_sender = self.data_sender.clone();
        let market_stats_subscriptions = Arc::clone(&self.market_stats_subscriptions);

        let task = get_runtime().spawn(async move {
            log::debug!("Lighter WebSocket consumption loop started");

            loop {
                tokio::select! {
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
                                let data = Data::Deltas(OrderBookDeltas_API::new(deltas));
                                if let Err(e) = data_sender.send(DataEvent::Data(data)) {
                                    log::error!("Failed to send order book deltas: {e}");
                                }
                            }
                            Some(NautilusWsMessage::Depth10(depth)) => {
                                if let Err(e) =
                                    data_sender.send(DataEvent::Data(Data::Depth10(depth)))
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
                                | NautilusWsMessage::PositionSnapshot(_)
                                | NautilusWsMessage::AccountState(_)
                                | NautilusWsMessage::SendTxAck { .. }
                                | NautilusWsMessage::SendTxRejected { .. }
                                | NautilusWsMessage::AccountStreamFirstFrame(_),
                            ) => {}
                            Some(NautilusWsMessage::Reconnected) => {
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
        });

        self.tasks.push(task);
        log::debug!("Lighter WebSocket consumption task spawned");

        Ok(())
    }

    fn spawn_instrument_refresh(&mut self) {
        let minutes = self.config.update_instruments_interval_mins;
        if minutes == 0 {
            log::debug!("Lighter instrument refresh disabled (interval=0)");
            return;
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

        let handle = get_runtime().spawn(async move {
            loop {
                let sleep = tokio::time::sleep(interval);
                tokio::pin!(sleep);
                tokio::select! {
                    () = cancellation.cancelled() => {
                        log::debug!("Lighter instrument refresh task cancelled");
                        break;
                    }
                    () = &mut sleep => {
                        match http_client.request_instruments_with_status().await {
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
        });

        self.tasks.push(handle);
    }

    fn clear_market_stats_subscriptions(&self) {
        self.market_stats_subscriptions.clear();
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
        let subscribe_channel = match self.market_stats_subscriptions.entry(instrument_id) {
            Entry::Occupied(mut entry) => {
                let subscription = entry.get_mut();
                let should_subscribe = subscription.flags.is_empty();
                subscription.flags.insert(kind);
                should_subscribe.then(|| subscription.channel.clone())
            }
            Entry::Vacant(entry) => {
                entry.insert(MarketStatsSubscription {
                    channel: channel.clone(),
                    flags: kind.into(),
                });
                Some(channel)
            }
        };

        if let Some(channel) = subscribe_channel {
            let ws = self.ws_client.clone();
            get_runtime().spawn(async move {
                if let Err(e) = subscribe_market_stats_channel(ws, channel).await {
                    log::error!("Failed to subscribe to Lighter {label}: {e:?}");
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
        let unsubscribe_channel = if let Some(mut subscription) =
            self.market_stats_subscriptions.get_mut(&instrument_id)
        {
            subscription.flags.remove(kind);
            subscription
                .flags
                .is_empty()
                .then(|| subscription.channel.clone())
        } else {
            None
        };

        if let Some(channel) = unsubscribe_channel {
            self.market_stats_subscriptions.remove(&instrument_id);

            let ws = self.ws_client.clone();
            get_runtime().spawn(async move {
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
            .ok_or_else(|| anyhow::anyhow!("Instrument {instrument_id} not found in cache"))?;

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
            .ok_or_else(|| anyhow::anyhow!("Instrument {instrument_id} not found in cache"))?;
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

fn cache_lighter_instrument_status(
    statuses: &DashMap<InstrumentId, LighterMarketStatus>,
    instrument_id: InstrumentId,
    status: LighterMarketStatus,
) {
    statuses.insert(instrument_id, status);
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

async fn subscribe_market_stats_channel(
    ws: LighterWebSocketClient,
    channel: LighterWsChannel,
) -> Result<(), LighterWsError> {
    match channel {
        LighterWsChannel::MarketStats(selection) => ws.subscribe_market_stats(selection).await,
        LighterWsChannel::SpotMarketStats(selection) => {
            ws.subscribe_spot_market_stats(selection).await
        }
        _ => unreachable!("market-stats subscription called with non-market-stats channel"),
    }
}

async fn unsubscribe_market_stats_channel(
    ws: LighterWebSocketClient,
    channel: LighterWsChannel,
) -> Result<(), LighterWsError> {
    match channel {
        LighterWsChannel::MarketStats(selection) => ws.unsubscribe_market_stats(selection).await,
        LighterWsChannel::SpotMarketStats(selection) => {
            ws.unsubscribe_spot_market_stats(selection).await
        }
        _ => unreachable!("market-stats unsubscription called with non-market-stats channel"),
    }
}

fn emit_market_stats_ws_message(
    sender: &tokio::sync::mpsc::UnboundedSender<DataEvent>,
    subscriptions: &DashMap<InstrumentId, MarketStatsSubscription>,
    message: &NautilusWsMessage,
) -> bool {
    match message {
        NautilusWsMessage::MarkPrice(mark_price) => {
            if !market_stats_is_subscribed(
                subscriptions,
                &mark_price.instrument_id,
                MarketStatsKind::MarkPrice,
            ) {
                return false;
            }

            if let Err(e) = sender.send(DataEvent::Data(Data::MarkPriceUpdate(*mark_price))) {
                log::error!("Failed to send mark price: {e}");
            }
            true
        }
        NautilusWsMessage::IndexPrice(index_price) => {
            if !market_stats_is_subscribed(
                subscriptions,
                &index_price.instrument_id,
                MarketStatsKind::IndexPrice,
            ) {
                return false;
            }

            if let Err(e) = sender.send(DataEvent::Data(Data::IndexPriceUpdate(*index_price))) {
                log::error!("Failed to send index price: {e}");
            }
            true
        }
        NautilusWsMessage::FundingRate(funding_rate) => {
            if !market_stats_is_subscribed(
                subscriptions,
                &funding_rate.instrument_id,
                MarketStatsKind::FundingRate,
            ) {
                return false;
            }

            if let Err(e) = sender.send(DataEvent::FundingRate(*funding_rate)) {
                log::error!("Failed to send funding rate: {e}");
            }
            true
        }
        _ => false,
    }
}

fn market_stats_is_subscribed(
    subscriptions: &DashMap<InstrumentId, MarketStatsSubscription>,
    instrument_id: &InstrumentId,
    kind: MarketStatsKind,
) -> bool {
    subscriptions
        .get(instrument_id)
        .is_some_and(|subscription| subscription.flags.contains(kind))
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
        self.cancellation_token.cancel();
        self.clear_instrument_status_subscriptions();
        self.clear_market_stats_subscriptions();
        self.is_connected.store(false, Ordering::Relaxed);
        Ok(())
    }

    fn reset(&mut self) -> anyhow::Result<()> {
        log::debug!("Resetting Lighter data client {}", self.client_id);
        self.clear_instrument_status_subscriptions();
        self.clear_market_stats_subscriptions();
        self.is_connected.store(false, Ordering::Relaxed);
        self.cancellation_token = CancellationToken::new();
        self.tasks.clear();
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
        if self.is_connected() {
            return Ok(());
        }

        // `stop()` and `disconnect()` cancel `cancellation_token` to tear down
        // the consumer task. Without rotating it here, a subsequent connect()
        // would clone an already-cancelled token into the new consumer, which
        // would exit immediately while we still mark the client connected.
        if self.cancellation_token.is_cancelled() {
            self.cancellation_token = CancellationToken::new();
        }

        let instruments = self
            .bootstrap_instruments()
            .await
            .context("failed to bootstrap Lighter instruments")?;

        for instrument in instruments {
            if let Err(e) = self.data_sender.send(DataEvent::Instrument(instrument)) {
                log::warn!("Failed to send instrument: {e}");
            }
        }

        self.spawn_ws()
            .await
            .context("failed to spawn Lighter WebSocket consumer")?;
        self.spawn_instrument_refresh();

        self.is_connected.store(true, Ordering::Relaxed);
        log::info!("Connected: client_id={}", self.client_id);

        Ok(())
    }

    async fn disconnect(&mut self) -> anyhow::Result<()> {
        if !self.is_connected() {
            return Ok(());
        }

        self.cancellation_token.cancel();
        self.clear_instrument_status_subscriptions();
        self.clear_market_stats_subscriptions();

        for task in self.tasks.drain(..) {
            if let Err(e) = task.await {
                log::error!("Error waiting for Lighter task to complete: {e}");
            }
        }

        if let Err(e) = self.ws_client.disconnect().await {
            log::error!("Error disconnecting Lighter WebSocket client: {e}");
        }

        self.instruments.store(AHashMap::new());
        self.instrument_statuses.clear();
        self.registry.clear();

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
        log::debug!("Subscribing to instrument status: {instrument_id}");

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

        get_runtime().spawn(async move {
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
        log::debug!("Subscribing to book deltas: {}", subscription.instrument_id);

        validate_book_deltas_subscription(subscription.book_type)?;

        let ws = self.ws_client.clone();
        let instrument_id = subscription.instrument_id;

        get_runtime().spawn(async move {
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

        get_runtime().spawn(async move {
            if let Err(e) = ws.subscribe_book_depth10(instrument_id).await {
                log::error!("Failed to subscribe to Lighter book depth10: {e:?}");
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
                log::error!("Failed to subscribe to Lighter quotes: {e:?}");
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
                log::error!("Failed to subscribe to Lighter trades: {e:?}");
            }
        });

        Ok(())
    }

    fn subscribe_mark_prices(&mut self, subscription: SubscribeMarkPrices) -> anyhow::Result<()> {
        let instrument_id = subscription.instrument_id;
        log::debug!("Subscribing to mark prices: {instrument_id}");

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
        log::debug!("Subscribing to index prices: {instrument_id}");

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
        log::debug!("Subscribing to funding rates: {instrument_id}");

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
        log::debug!("Subscribing to bars: {bar_type}");

        let resolution = LighterCandleResolution::try_from(&bar_type)?;
        anyhow::ensure!(
            resolution.is_ws_streamable(),
            "Lighter does not offer {bar_type} on the candle WebSocket stream",
        );

        let instrument_id = bar_type.instrument_id();
        anyhow::ensure!(
            self.instruments.contains_key(&instrument_id),
            "Instrument {instrument_id} not found in cache",
        );

        let ws = self.ws_client.clone();
        get_runtime().spawn(async move {
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

        get_runtime().spawn(async move {
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

        get_runtime().spawn(async move {
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

        get_runtime().spawn(async move {
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

        get_runtime().spawn(async move {
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
        log::debug!("Unsubscribing from instrument status: {instrument_id}");

        self.instrument_status_subscriptions.remove(&instrument_id);

        Ok(())
    }

    fn unsubscribe_mark_prices(
        &mut self,
        unsubscription: &UnsubscribeMarkPrices,
    ) -> anyhow::Result<()> {
        let instrument_id = unsubscription.instrument_id;
        log::debug!("Unsubscribing from mark prices: {instrument_id}");

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
        log::debug!("Unsubscribing from index prices: {instrument_id}");

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
        log::debug!("Unsubscribing from funding rates: {instrument_id}");

        self.deactivate_market_stats_subscription(
            instrument_id,
            MarketStatsKind::FundingRate,
            "funding rate",
        );

        Ok(())
    }

    fn unsubscribe_bars(&mut self, unsubscription: &UnsubscribeBars) -> anyhow::Result<()> {
        let bar_type = unsubscription.bar_type;
        log::debug!("Unsubscribing from bars: {bar_type}");

        let resolution = match LighterCandleResolution::try_from(&bar_type) {
            Ok(resolution) => resolution,
            Err(e) => {
                log::warn!("Skipping Lighter candle unsubscribe for {bar_type}: {e}");
                return Ok(());
            }
        };

        let instrument_id = bar_type.instrument_id();
        let ws = self.ws_client.clone();
        get_runtime().spawn(async move {
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

        get_runtime().spawn(async move {
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

        get_runtime().spawn(async move {
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
            .ok_or_else(|| anyhow::anyhow!("Instrument {instrument_id} not found in cache"))?;

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

        get_runtime().spawn(async move {
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
            .ok_or_else(|| anyhow::anyhow!("Instrument {instrument_id} not found in cache"))?;

        let http = self.http_client.clone();
        let sender = self.data_sender.clone();
        let request_id = request.request_id;
        let client_id = request.client_id.unwrap_or(self.client_id);
        let limit = request.limit.map_or(DEFAULT_TRADES_LIMIT, |n| {
            u16::try_from(n.get()).unwrap_or(u16::MAX)
        });
        let from_timestamp = request.start.map(|dt| dt.timestamp_millis());
        let start_nanos = datetime_to_unix_nanos(request.start);
        let end_nanos = datetime_to_unix_nanos(request.end);
        let params = request.params;
        let clock = self.clock;
        let auth = match &self.credential {
            Some(credential) => Some(
                build_auth_token_for(credential)
                    .context("failed to mint Lighter auth token for trades request")?,
            ),
            None => {
                let message = "Lighter historical trade requests require credentials; \
                               configure Lighter data credentials to call /api/v1/trades";
                log::warn!("{message}");
                anyhow::bail!("{message}");
            }
        };

        // Marketwide trade history: omit `account_index` (it filters server-side).
        let query = LighterTradesQuery {
            auth,
            sort_by: LighterTradeSortBy::Timestamp,
            from_timestamp,
            limit,
            ..Default::default()
        };

        get_runtime().spawn(async move {
            match http.request_trades(&instrument, query).await {
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
                Err(e) => {
                    log::error!(
                        "Lighter trades request failed for {instrument_id}: {}",
                        scrub_auth(&format!("{e:#}")),
                    );
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
            .ok_or_else(|| anyhow::anyhow!("Instrument {instrument_id} not found in cache"))?;

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

        get_runtime().spawn(async move {
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
            .ok_or_else(|| anyhow::anyhow!("Instrument {instrument_id} not found in cache"))?;

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

        get_runtime().spawn(async move {
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

/// Clamps a `RequestBookSnapshot.depth` to a `limit` value the venue accepts.
///
/// Lighter's `GET /api/v1/orderBookOrders` rejects `limit` above
/// [`LIGHTER_BOOK_ORDERS_MAX_LIMIT`] with venue error 20001. `None` defaults
/// to the cap.
fn clamp_book_snapshot_limit(depth: Option<std::num::NonZeroUsize>) -> u16 {
    depth
        .map_or(DEFAULT_BOOK_SNAPSHOT_LIMIT, |n| {
            u16::try_from(n.get()).unwrap_or(u16::MAX)
        })
        .min(LIGHTER_BOOK_ORDERS_MAX_LIMIT)
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
    use chrono::DateTime;
    use nautilus_common::live::runner::replace_data_event_sender;
    use nautilus_core::UUID4;
    use nautilus_model::{
        data::{BarSpecification, BarType, FundingRateUpdate, IndexPriceUpdate, MarkPriceUpdate},
        enums::{AggregationSource, AggressorSide, BarAggregation, PriceType},
        identifiers::{InstrumentId, Symbol},
        instruments::{CryptoPerpetual, CurrencyPair},
        types::{Currency, Price, Quantity},
    };
    use rstest::rstest;
    use rust_decimal::Decimal;

    use super::*;
    use crate::{
        common::enums::{LighterFundingResolution, LighterProductType},
        http::query::LighterFundingsQuery,
    };

    const HTTP_ORDER_BOOK_DETAILS: &str = include_str!("../test_data/http_order_book_details.json");
    const HTTP_FUNDINGS: &str = include_str!("../test_data/http_fundings.json");
    const HTTP_RECENT_TRADES: &str = include_str!("../test_data/http_recent_trades.json");
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
            DataEvent::Data(Data::MarkPriceUpdate(update)) => {
                assert_eq!(update.instrument_id, instrument_id);
                assert_eq!(update.value, Price::from("2000.00"));
            }
            event => panic!("expected mark price update, was {event:?}"),
        }

        match receiver.try_recv().unwrap() {
            DataEvent::Data(Data::IndexPriceUpdate(update)) => {
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
        let start = DateTime::from_timestamp(1_778_702_400, 0).unwrap();
        let end = DateTime::from_timestamp(1_778_706_000, 0).unwrap();
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
    async fn test_request_trades_uses_auth_when_credentials_available() {
        let base_url = spawn_trades_server().await;
        let config = LighterDataClientConfig {
            base_url_http: Some(base_url),
            account_index: Some(12_345),
            api_key_index: Some(5),
            private_key: Some(PRIVATE_KEY_HEX.to_string()),
            ..Default::default()
        };
        let (client, mut receiver) = create_data_client_with_receiver_and_config_for_test(config);
        let instrument_id = cache_test_instrument(&client, 0, "ETH", LighterProductType::Perp);
        let start = DateTime::from_timestamp(1_700_000_000, 0).unwrap();
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
                assert_eq!(tick.aggressor_side, AggressorSide::Seller);
                assert_eq!(tick.trade_id.to_string(), "19211490282");
            }
            event => panic!("expected trades response, was {event:?}"),
        }
    }

    #[tokio::test]
    async fn test_request_trades_requires_credentials() {
        let (mut client, mut receiver) = create_data_client_with_receiver_for_test();
        client.credential = None;
        let instrument_id = cache_test_instrument(&client, 0, "ETH", LighterProductType::Perp);
        let request = RequestTrades::new(
            instrument_id,
            None,
            None,
            NonZeroUsize::new(50),
            Some(ClientId::new("LIGHTER")),
            UUID4::new(),
            UnixNanos::default(),
            None,
        );

        let err = DataClient::request_trades(&client, request).unwrap_err();

        assert!(
            err.to_string()
                .contains("historical trade requests require credentials"),
        );
        assert!(receiver.try_recv().is_err());
    }

    #[tokio::test]
    async fn test_spawn_instrument_refresh_skipped_when_interval_zero() {
        let config = LighterDataClientConfig {
            update_instruments_interval_mins: 0,
            ..Default::default()
        };
        let (mut client, _receiver) = create_data_client_with_receiver_and_config_for_test(config);

        assert!(client.tasks.is_empty());
        client.spawn_instrument_refresh();
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
        client.spawn_instrument_refresh();
        assert_eq!(client.tasks.len(), 1);

        client.cancellation_token.cancel();
        for task in client.tasks.drain(..) {
            task.await.unwrap();
        }
    }

    // Tests that observe `has_credentials()` semantics under controlled env
    // state. Pinned to the workspace `serial_tests` group (see
    // `.config/nextest.toml`) so env-var mutation runs single-threaded.
    #[allow(unsafe_code)] // env-var mutation in tests; restored via `EnvGuard`.
    mod serial_tests {
        use super::*;

        const LIGHTER_ENV_VARS: &[&str] = &[
            "LIGHTER_API_KEY_INDEX",
            "LIGHTER_API_SECRET",
            "LIGHTER_ACCOUNT_INDEX",
            "LIGHTER_TESTNET_API_KEY_INDEX",
            "LIGHTER_TESTNET_API_SECRET",
            "LIGHTER_TESTNET_ACCOUNT_INDEX",
        ];

        struct EnvGuard {
            saved: Vec<(&'static str, Option<String>)>,
        }

        impl EnvGuard {
            fn clear_lighter() -> Self {
                let saved = LIGHTER_ENV_VARS
                    .iter()
                    .map(|&name| (name, std::env::var(name).ok()))
                    .collect::<Vec<_>>();
                for &(name, _) in &saved {
                    // SAFETY: the `serial_tests` nextest group serializes
                    // these tests, and no other lighter test reads or writes
                    // the LIGHTER_* env vars.
                    unsafe { std::env::remove_var(name) };
                }
                Self { saved }
            }
        }

        impl Drop for EnvGuard {
            fn drop(&mut self) {
                for (name, original) in &self.saved {
                    match original {
                        // SAFETY: see `EnvGuard::clear_lighter`.
                        Some(value) => unsafe { std::env::set_var(name, value) },
                        None => unsafe { std::env::remove_var(name) },
                    }
                }
            }
        }

        #[tokio::test]
        async fn new_data_client_with_partial_config_skips_credential_resolution() {
            // With `account_index` missing and the env cleared,
            // `LighterDataClientConfig::has_credentials()` must short-circuit
            // to `false` so `Credential::resolve` is never called. Regressing
            // the `&&` in `has_credentials()` to `||` would route this case
            // through `credential_from_resolved_values` and fail construction
            // with "incomplete Lighter credentials".
            let _guard = EnvGuard::clear_lighter();
            let config = LighterDataClientConfig {
                api_key_index: Some(5),
                private_key: Some(PRIVATE_KEY_HEX.to_string()),
                account_index: None,
                ..Default::default()
            };
            let (client, _receiver) = create_data_client_with_receiver_and_config_for_test(config);

            assert!(!client.has_credentials());
        }

        #[tokio::test]
        async fn new_data_client_with_all_config_fields_resolves_credential() {
            let _guard = EnvGuard::clear_lighter();
            let config = LighterDataClientConfig {
                api_key_index: Some(5),
                account_index: Some(12_345),
                private_key: Some(PRIVATE_KEY_HEX.to_string()),
                ..Default::default()
            };
            let (client, _receiver) = create_data_client_with_receiver_and_config_for_test(config);

            assert!(client.has_credentials());
        }

        #[tokio::test]
        async fn new_data_client_blank_private_key_falls_back_to_env() {
            // `has_credentials()` and `Credential::resolve` must agree on
            // precedence: when the config holds a blank `private_key` and the
            // env secret is set, resolution must succeed via the env value
            // rather than failing with "incomplete Lighter credentials".
            let _guard = EnvGuard::clear_lighter();
            // SAFETY: see `EnvGuard::clear_lighter`; the guard restores values on drop.
            unsafe {
                std::env::set_var("LIGHTER_API_SECRET", PRIVATE_KEY_HEX);
            }
            let config = LighterDataClientConfig {
                api_key_index: Some(5),
                account_index: Some(12_345),
                private_key: Some("   ".to_string()),
                ..Default::default()
            };
            let (client, _receiver) = create_data_client_with_receiver_and_config_for_test(config);

            assert!(client.has_credentials());
        }
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
        config: LighterDataClientConfig,
    ) -> (
        LighterDataClient,
        tokio::sync::mpsc::UnboundedReceiver<DataEvent>,
    ) {
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
        let app = Router::new().route("/api/v1/trades", get(trades));
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
        assert_eq!(query.count_back, 2);
        (StatusCode::OK, HTTP_FUNDINGS).into_response()
    }

    async fn trades(Query(query): Query<LighterTradesQuery>) -> Response {
        let token = query
            .auth
            .as_deref()
            .expect("auth token must be present on /api/v1/trades");
        // Token format: `<deadline>:<account_index>:<api_key_index>:<sig_hex>`.
        let parts: Vec<&str> = token.split(':').collect();
        assert_eq!(parts.len(), 4, "unexpected token shape: `{token}`");
        assert_eq!(parts[1], "12345", "embedded account_index mismatch");
        assert_eq!(parts[2], "5", "embedded api_key_index mismatch");
        assert!(!parts[3].is_empty(), "signature segment is empty");
        assert_eq!(query.authorization, None);
        assert_eq!(query.market_id, Some(0));
        assert_eq!(
            query.account_index, None,
            "request_trades must be marketwide (no account_index filter)",
        );
        assert_eq!(query.sort_by, LighterTradeSortBy::Timestamp);
        assert_eq!(query.from_timestamp, Some(1_700_000_000_000));
        assert_eq!(query.limit, 50);
        (StatusCode::OK, HTTP_RECENT_TRADES).into_response()
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
        InstrumentAny::CryptoPerpetual(CryptoPerpetual::new(
            instrument_id,
            Symbol::new(format!("{venue_symbol}-PERP")),
            Currency::from(venue_symbol),
            Currency::from("USDC"),
            Currency::from("USDC"),
            false,
            2,
            4,
            Price::from("0.01"),
            Quantity::from("0.0001"),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            UnixNanos::default(),
            UnixNanos::default(),
        ))
    }

    fn test_spot_instrument(instrument_id: InstrumentId, venue_symbol: &str) -> InstrumentAny {
        InstrumentAny::CurrencyPair(CurrencyPair::new(
            instrument_id,
            Symbol::new(format!("{venue_symbol}-SPOT")),
            Currency::from(venue_symbol),
            Currency::from("USDC"),
            2,
            4,
            Price::from("0.01"),
            Quantity::from("0.0001"),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            UnixNanos::default(),
            UnixNanos::default(),
        ))
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
