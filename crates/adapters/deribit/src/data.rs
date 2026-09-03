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

//! Live market data client implementation for the Deribit adapter.

use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use ahash::{AHashMap, AHashSet};
use anyhow::Context;
use async_trait::async_trait;
use futures_util::StreamExt;
use nautilus_common::{
    clients::DataClient,
    live::runner::get_data_event_sender,
    log_debug, log_info,
    messages::{
        DataEvent, DataResponse,
        data::{
            BarsResponse, BookResponse, CustomDataResponse, ForwardPricesResponse,
            InstrumentResponse, InstrumentsResponse, RequestBars, RequestBookSnapshot,
            RequestCustomData, RequestForwardPrices, RequestInstrument, RequestInstruments,
            RequestTrades, SubscribeBars, SubscribeBookDeltas, SubscribeBookDepth10,
            SubscribeCustomData, SubscribeFundingRates, SubscribeIndexPrices, SubscribeInstrument,
            SubscribeInstrumentStatus, SubscribeInstruments, SubscribeMarkPrices,
            SubscribeOptionGreeks, SubscribeQuotes, SubscribeTrades, TradesResponse,
            UnsubscribeBars, UnsubscribeBookDeltas, UnsubscribeBookDepth10, UnsubscribeCustomData,
            UnsubscribeFundingRates, UnsubscribeIndexPrices, UnsubscribeInstrument,
            UnsubscribeInstrumentStatus, UnsubscribeInstruments, UnsubscribeMarkPrices,
            UnsubscribeOptionGreeks, UnsubscribeQuotes, UnsubscribeTrades,
        },
    },
};
use nautilus_core::{
    AtomicMap, AtomicSet, Params,
    datetime::datetime_to_unix_nanos,
    time::{AtomicTime, get_atomic_clock_realtime},
};
use nautilus_live::{
    SocketControl,
    task::{TaskGroup, TaskGroupGuard},
};
use nautilus_model::{
    data::{CustomData, Data, DataType, ForwardPrice},
    enums::BookType,
    identifiers::{ClientId, InstrumentId, Symbol, Venue},
    instruments::{Instrument, InstrumentAny},
};
use tokio_util::sync::CancellationToken;

use crate::{
    common::{
        consts::{
            DERIBIT_BOOK_DEFAULT_DEPTH, DERIBIT_BOOK_DEFAULT_GROUP, DERIBIT_BOOK_VALID_DEPTHS,
            DERIBIT_VENUE,
        },
        parse::{bar_spec_to_resolution, parse_instrument_kind_currency},
    },
    config::DeribitDataClientConfig,
    data_types::{DeribitBookSummary, register_deribit_custom_data},
    http::{
        client::DeribitHttpClient,
        models::{DeribitCurrency, DeribitProductType},
    },
    websocket::{
        auth::DERIBIT_DATA_SESSION_NAME, client::DeribitWebSocketClient,
        enums::DeribitUpdateInterval, messages::NautilusWsMessage,
    },
};

/// Deribit live data client.
#[derive(Debug)]
pub struct DeribitDataClient {
    client_id: ClientId,
    config: DeribitDataClientConfig,
    http_client: DeribitHttpClient,
    ws_client: Option<DeribitWebSocketClient>,
    is_connected: AtomicBool,
    cancellation_token: CancellationToken,
    session_tasks: TaskGroup,
    command_tasks: TaskGroup,
    data_sender: tokio::sync::mpsc::UnboundedSender<DataEvent>,
    instruments: Arc<AtomicMap<InstrumentId, InstrumentAny>>,
    mark_price_subs: Arc<AtomicSet<InstrumentId>>,
    index_price_subs: Arc<AtomicSet<InstrumentId>>,
    option_greeks_subs: Arc<AtomicSet<InstrumentId>>,
    combo_leg_trade_subs: Arc<AtomicMap<InstrumentId, AHashMap<InstrumentId, usize>>>,
    clock: &'static AtomicTime,
}

impl DeribitDataClient {
    const BOOK_SUMMARY_TYPE_NAME: &'static str = "DeribitBookSummary";

    /// Creates a new [`DeribitDataClient`] instance.
    ///
    /// # Errors
    ///
    /// Returns an error if the client fails to initialize.
    pub fn new(client_id: ClientId, config: DeribitDataClientConfig) -> anyhow::Result<Self> {
        let clock = get_atomic_clock_realtime();
        let data_sender = get_data_event_sender();
        let api_key = config
            .api_key
            .as_ref()
            .map(|value| value.expose_secret().to_owned());
        let api_secret = config
            .api_secret
            .as_ref()
            .map(|value| value.expose_secret().to_owned());
        let proxy_url = config
            .proxy_url
            .as_ref()
            .map(|value| value.expose_secret().to_owned());

        let http_client = if config.has_api_credentials() {
            DeribitHttpClient::new_with_env(
                api_key.clone(),
                api_secret.clone(),
                config.base_url_http.clone(),
                config.environment,
                config.http_timeout_secs,
                config.max_retries,
                config.retry_delay_initial_ms,
                config.retry_delay_max_ms,
                proxy_url.clone(),
            )?
        } else {
            DeribitHttpClient::new(
                config.base_url_http.clone(),
                config.environment,
                config.http_timeout_secs,
                config.max_retries,
                config.retry_delay_initial_ms,
                config.retry_delay_max_ms,
                proxy_url.clone(),
            )?
        };

        let ws_client = DeribitWebSocketClient::new(
            Some(config.ws_url()),
            api_key,
            api_secret,
            config.heartbeat_interval_secs,
            config.auth_timeout_secs,
            config.environment,
            config.transport_backend,
            proxy_url,
        )?
        .with_socket_control(SocketControl::new(
            client_id,
            Some(*DERIBIT_VENUE),
            "deribit-data-streams",
        ));

        let session_tasks = TaskGroup::new();
        let command_tasks = TaskGroup::new();

        Ok(Self {
            client_id,
            config,
            http_client,
            ws_client: Some(ws_client),
            is_connected: AtomicBool::new(false),
            cancellation_token: session_tasks.cancellation_token(),
            session_tasks,
            command_tasks,
            data_sender,
            instruments: Arc::new(AtomicMap::new()),
            mark_price_subs: Arc::new(AtomicSet::new()),
            index_price_subs: Arc::new(AtomicSet::new()),
            option_greeks_subs: Arc::new(AtomicSet::new()),
            combo_leg_trade_subs: Arc::new(AtomicMap::new()),
            clock,
        })
    }

    /// Returns a mutable reference to the WebSocket client.
    fn ws_client_mut(&mut self) -> anyhow::Result<&mut DeribitWebSocketClient> {
        self.ws_client
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("WebSocket client not initialized"))
    }

    fn spawn_command<F>(&self, future: F)
    where
        F: std::future::Future<Output = ()> + Send + 'static,
    {
        if let Err(e) = self.command_tasks.spawn(future) {
            log::warn!("Skipping Deribit data command after shutdown began: {e}");
        }
    }

    async fn finish_tasks(&self) -> anyhow::Result<()> {
        let (session_result, command_result) = tokio::join!(
            self.session_tasks
                .finish_shutdown(Duration::from_secs(1), Duration::from_secs(2)),
            self.command_tasks
                .finish_shutdown(Duration::from_secs(1), Duration::from_secs(2)),
        );
        session_result.context("failed to finish Deribit data session tasks")?;
        command_result.context("failed to finish Deribit data command tasks")?;
        Ok(())
    }

    async fn prepare_task_groups(&mut self) -> anyhow::Result<()> {
        if !self.session_tasks.is_open() || !self.command_tasks.is_open() {
            self.session_tasks.begin_shutdown();
            self.command_tasks.begin_shutdown();
            self.finish_tasks().await?;
            self.session_tasks
                .start_generation()
                .context("failed to start Deribit data session task generation")?;
            self.command_tasks
                .start_generation()
                .context("failed to start Deribit data command task generation")?;
            self.cancellation_token = self.session_tasks.cancellation_token();
        }
        Ok(())
    }

    async fn teardown_partial_connect(&self) -> anyhow::Result<()> {
        self.session_tasks.begin_shutdown();
        self.command_tasks.begin_shutdown();
        if let Some(ws) = self.ws_client.as_ref() {
            ws.begin_shutdown();
        }

        let mut errors = Vec::new();

        if let Some(ws) = self.ws_client.as_ref()
            && let Err(e) = ws.close().await
        {
            errors.push(format!("WebSocket shutdown failed: {e}"));
        }

        if let Err(e) = self.finish_tasks().await {
            errors.push(e.to_string());
        }
        self.is_connected.store(false, Ordering::Release);

        if errors.is_empty() {
            Ok(())
        } else {
            anyhow::bail!(errors.join("; "))
        }
    }

    /// Gets the interval from params, defaulting to Raw if authenticated.
    ///
    /// If authenticated, we prefer Raw interval for best data quality.
    /// Users can still override via params if they want 100ms or agg2.
    fn get_interval(&self, params: &Option<Params>) -> Option<DeribitUpdateInterval> {
        if let Some(interval) = params
            .as_ref()
            .and_then(|p| p.get_str("interval"))
            .and_then(|s| s.parse::<DeribitUpdateInterval>().ok())
        {
            return Some(interval);
        }

        // Default to Raw if authenticated, otherwise None (100ms default)
        if let Some(ws) = self.ws_client.as_ref()
            && ws.is_authenticated()
        {
            return Some(DeribitUpdateInterval::Raw);
        }
        None
    }

    /// Spawns a task to process WebSocket messages.
    fn spawn_stream_task(
        &self,
        stream: impl futures_util::Stream<Item = NautilusWsMessage> + Send + 'static,
    ) -> anyhow::Result<()> {
        let data_sender = self.data_sender.clone();
        let instruments = Arc::clone(&self.instruments);
        let cancellation = self.cancellation_token.clone();

        let future = async move {
            tokio::pin!(stream);

            loop {
                tokio::select! {
                    maybe_msg = stream.next() => {
                        match maybe_msg {
                            Some(msg) => Self::handle_ws_message(msg, &data_sender, &instruments),
                            None => {
                                log::debug!("WebSocket stream ended");
                                break;
                            }
                        }
                    }
                    () = cancellation.cancelled() => {
                        log::debug!("WebSocket stream task cancelled");
                        break;
                    }
                }
            }
        };

        self.session_tasks
            .spawn(future)
            .context("failed to register Deribit WebSocket stream task")?;
        Ok(())
    }

    /// Handles incoming WebSocket messages.
    fn handle_ws_message(
        message: NautilusWsMessage,
        sender: &tokio::sync::mpsc::UnboundedSender<DataEvent>,
        instruments: &Arc<AtomicMap<InstrumentId, InstrumentAny>>,
    ) {
        match message {
            NautilusWsMessage::Data(payloads) => {
                for data in payloads {
                    Self::send_data(sender, data);
                }
            }
            NautilusWsMessage::Deltas(deltas) => {
                Self::send_data(sender, Data::Deltas(Box::new(deltas)));
            }
            NautilusWsMessage::Instrument(instrument) => {
                let instrument_any = *instrument;
                instruments.insert(instrument_any.id(), instrument_any.clone());

                if let Err(e) = sender.send(DataEvent::Instrument(instrument_any)) {
                    log::warn!("Failed to send instrument update: {e}");
                }
            }
            NautilusWsMessage::OptionGreeks(greeks) => {
                if let Err(e) = sender.send(DataEvent::OptionGreeks(greeks)) {
                    log::error!("Failed to send option greeks: {e}");
                }
            }
            NautilusWsMessage::Error(e) => {
                log::warn!("WebSocket error: {e:?}");
            }
            NautilusWsMessage::Raw(value) => {
                log::debug!("Unhandled raw message: {value}");
            }
            NautilusWsMessage::Reconnected => {
                log::info!("WebSocket reconnected");
            }
            NautilusWsMessage::Authenticated(auth) => {
                log::debug!("WebSocket authenticated: expires_in={}s", auth.expires_in);
            }
            NautilusWsMessage::FundingRates(funding_rates) => {
                for funding_rate in funding_rates {
                    if let Err(e) = sender.send(DataEvent::FundingRate(funding_rate)) {
                        log::error!("Failed to send funding rate: {e}");
                    }
                }
            }
            NautilusWsMessage::InstrumentStatus(status) => {
                if let Err(e) = sender.send(DataEvent::InstrumentStatus(status)) {
                    log::error!("Failed to send instrument status event: {e}");
                }
            }
            NautilusWsMessage::OrderStatusReports(reports) => {
                log::warn!(
                    "Data client received OrderStatusReports message (should be handled by execution client): {} reports",
                    reports.len()
                );
            }
            NautilusWsMessage::FillReports(reports) => {
                log::warn!(
                    "Data client received FillReports message (should be handled by execution client): {} reports",
                    reports.len()
                );
            }
            NautilusWsMessage::OrderFilled(order) => {
                log::warn!(
                    "Data client received OrderFilled message (should be handled by execution client): {order:?}"
                );
            }
            NautilusWsMessage::OrderRejected(order) => {
                log::warn!(
                    "Data client received OrderRejected message (should be handled by execution client): {order:?}"
                );
            }
            NautilusWsMessage::OrderAccepted(order) => {
                log::warn!(
                    "Data client received OrderAccepted message (should be handled by execution client): {order:?}"
                );
            }
            NautilusWsMessage::OrderCanceled(order) => {
                log::warn!(
                    "Data client received OrderCanceled message (should be handled by execution client): {order:?}"
                );
            }
            NautilusWsMessage::OrderExpired(order) => {
                log::warn!(
                    "Data client received OrderExpired message (should be handled by execution client): {order:?}"
                );
            }
            NautilusWsMessage::OrderUpdated(order) => {
                log::warn!(
                    "Data client received OrderUpdated message (should be handled by execution client): {order:?}"
                );
            }
            NautilusWsMessage::OrderCancelRejected(order) => {
                log::warn!(
                    "Data client received OrderCancelRejected message (should be handled by execution client): {order:?}"
                );
            }
            NautilusWsMessage::OrderModifyRejected(order) => {
                log::warn!(
                    "Data client received OrderModifyRejected message (should be handled by execution client): {order:?}"
                );
            }
            NautilusWsMessage::AccountState(state) => {
                log::warn!(
                    "Data client received AccountState message (should be handled by execution client): {state:?}"
                );
            }
            NautilusWsMessage::AuthenticationFailed(reason) => {
                log::error!("Authentication failed in data client: {reason}");
            }
        }
    }

    /// Sends data to the data channel.
    fn send_data(sender: &tokio::sync::mpsc::UnboundedSender<DataEvent>, data: Data) {
        if let Err(e) = sender.send(DataEvent::Data(data)) {
            log::error!("Failed to send data: {e}");
        }
    }

    // Returns whether a subscribe should lazy-load the instrument before sending,
    // erroring up front when the instrument is missing and the flag is disabled
    // (so the WebSocket handler does not silently drop later frames).
    fn prepare_subscribe(&self, instrument_id: InstrumentId) -> anyhow::Result<bool> {
        if self.instruments.contains_key(&instrument_id) {
            return Ok(false);
        }

        if !self.config.auto_load_missing_instruments {
            anyhow::bail!(
                "Instrument {instrument_id} not found and `auto_load_missing_instruments` is disabled"
            );
        }
        Ok(true)
    }

    // Fetches an instrument over HTTP and seeds the local, HTTP, and WebSocket caches.
    async fn lazy_load_instrument(
        http_client: &DeribitHttpClient,
        ws: &DeribitWebSocketClient,
        instruments: &AtomicMap<InstrumentId, InstrumentAny>,
        instrument_id: InstrumentId,
    ) -> anyhow::Result<()> {
        let instrument = http_client
            .request_instrument(instrument_id)
            .await
            .with_context(|| format!("failed to lazy-load instrument {instrument_id}"))?;
        instruments.insert(instrument.id(), instrument.clone());
        http_client.cache_instruments(std::slice::from_ref(&instrument));
        ws.cache_instruments(std::slice::from_ref(&instrument));
        Ok(())
    }

    fn subscribe_combo_legs(params: &Option<Params>) -> bool {
        params
            .as_ref()
            .and_then(|params| params.get_bool("subscribe_combo_legs"))
            .unwrap_or(false)
    }

    fn book_summary_metadata_currency(data_type: &DataType) -> anyhow::Result<String> {
        data_type
            .metadata()
            .and_then(|m| m.get("currency"))
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_ascii_uppercase)
            .ok_or_else(|| {
                anyhow::anyhow!("DeribitBookSummary requests require metadata['currency']")
            })
    }

    fn book_summary_metadata_kind(data_type: &DataType) -> Option<String> {
        data_type
            .metadata()
            .and_then(|m| m.get("kind"))
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_ascii_lowercase)
    }

    fn book_summary_data_type(currency: &str, kind: Option<&str>) -> DataType {
        let mut metadata = Params::new();
        metadata.insert(
            "currency".to_string(),
            serde_json::Value::String(currency.to_string()),
        );
        let kind = kind.unwrap_or("option");
        metadata.insert(
            "kind".to_string(),
            serde_json::Value::String(kind.to_string()),
        );
        DataType::new(
            Self::BOOK_SUMMARY_TYPE_NAME,
            Some(metadata),
            Some(format!("{currency}:{kind}")),
        )
    }

    fn combo_leg_trade_ids(
        instruments: &AtomicMap<InstrumentId, InstrumentAny>,
        instrument_id: InstrumentId,
    ) -> Vec<InstrumentId> {
        let Some(instrument) = instruments.get_cloned(&instrument_id) else {
            log::warn!("Cannot expand Deribit combo legs for missing instrument {instrument_id}");
            return Vec::new();
        };

        let info = match instrument {
            InstrumentAny::CryptoOptionSpread(spread) => spread.info,
            InstrumentAny::CryptoFuturesSpread(spread) => spread.info,
            _ => return Vec::new(),
        };
        let Some(info) = info else {
            return Vec::new();
        };
        let Some(legs) = info
            .get("deribit_combo_legs")
            .and_then(serde_json::Value::as_array)
        else {
            return Vec::new();
        };

        let mut leg_ids = Vec::new();
        let mut seen = AHashSet::new();

        for leg in legs {
            let Some(leg_id_str) = leg.get("instrument_id").and_then(serde_json::Value::as_str)
            else {
                continue;
            };

            match InstrumentId::from_as_ref(leg_id_str) {
                Ok(leg_id) if leg_id != instrument_id && seen.insert(leg_id) => {
                    leg_ids.push(leg_id);
                }
                Ok(_) => {}
                Err(e) => {
                    log::warn!(
                        "Skipping invalid Deribit combo leg instrument ID {leg_id_str}: {e}"
                    );
                }
            }
        }

        leg_ids
    }

    fn track_combo_leg_trade_subs(
        subscriptions: &AtomicMap<InstrumentId, AHashMap<InstrumentId, usize>>,
        instrument_id: InstrumentId,
        leg_ids: &[InstrumentId],
    ) {
        if leg_ids.is_empty() {
            return;
        }

        subscriptions.rcu(|subscriptions| {
            let counts = subscriptions.entry(instrument_id).or_default();

            for leg_id in leg_ids {
                counts
                    .entry(*leg_id)
                    .and_modify(|count| *count += 1)
                    .or_insert(1);
            }
        });
    }

    fn combo_leg_trade_unsubs(
        subscriptions: &AtomicMap<InstrumentId, AHashMap<InstrumentId, usize>>,
        instrument_id: InstrumentId,
    ) -> Vec<InstrumentId> {
        let mut leg_ids = Vec::new();

        subscriptions.rcu(|subscriptions| {
            let remove_instrument = if let Some(counts) = subscriptions.get_mut(&instrument_id) {
                leg_ids = counts.keys().copied().collect();
                counts.retain(|_, count| {
                    if *count > 1 {
                        *count -= 1;
                        true
                    } else {
                        false
                    }
                });
                counts.is_empty()
            } else {
                leg_ids = Vec::new();
                false
            };

            if remove_instrument {
                subscriptions.remove(&instrument_id);
            }
        });

        leg_ids
    }
}

#[async_trait(?Send)]
impl DataClient for DeribitDataClient {
    fn client_id(&self) -> ClientId {
        self.client_id
    }

    fn venue(&self) -> Option<Venue> {
        Some(*DERIBIT_VENUE)
    }

    fn start(&mut self) -> anyhow::Result<()> {
        log::info!(
            "Starting data client: client_id={}, environment={}",
            self.client_id,
            self.config.environment
        );
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        log::info!("Stopping data client: {}", self.client_id);
        self.session_tasks.begin_shutdown();
        self.command_tasks.begin_shutdown();
        if let Some(ws) = self.ws_client.as_ref() {
            ws.begin_shutdown();
        }
        self.is_connected.store(false, Ordering::Relaxed);
        Ok(())
    }

    fn reset(&mut self) -> anyhow::Result<()> {
        log::info!("Resetting data client: {}", self.client_id);
        self.session_tasks.begin_shutdown();
        self.command_tasks.begin_shutdown();
        if let Some(ws) = self.ws_client.as_ref() {
            ws.begin_shutdown();
        }
        self.is_connected.store(false, Ordering::Relaxed);

        self.instruments.store(AHashMap::new());
        self.combo_leg_trade_subs.store(AHashMap::new());
        Ok(())
    }

    fn dispose(&mut self) -> anyhow::Result<()> {
        log::debug!("Disposing data client: {}", self.client_id);
        self.stop()
    }

    fn is_connected(&self) -> bool {
        self.is_connected.load(Ordering::SeqCst)
    }

    fn is_disconnected(&self) -> bool {
        !self.is_connected()
    }

    async fn connect(&mut self) -> anyhow::Result<()> {
        if self.is_connected() && self.session_tasks.is_open() && self.command_tasks.is_open() {
            return Ok(());
        }

        self.prepare_task_groups().await?;
        let cancellation_token = self.cancellation_token.clone();
        let ws_client = self.ws_client.clone();
        let setup_guard =
            TaskGroupGuard::new(&[&self.session_tasks, &self.command_tasks], move || {
                cancellation_token.cancel();

                if let Some(ws) = ws_client {
                    ws.begin_shutdown();
                }
            });

        register_deribit_custom_data();

        // Fetch instruments for each configured product type
        let product_types = if self.config.product_types.is_empty() {
            vec![DeribitProductType::Future]
        } else {
            self.config.product_types.clone()
        };

        let mut all_instruments = Vec::new();

        for product_type in &product_types {
            let fetched = self
                .http_client
                .request_instruments(DeribitCurrency::ANY, Some(*product_type))
                .await
                .with_context(|| format!("failed to request instruments for {product_type:?}"))?;

            // Cache in http client
            self.http_client.cache_instruments(&fetched);

            // Cache locally
            self.instruments.rcu(|m| {
                for instrument in &fetched {
                    m.insert(instrument.id(), instrument.clone());
                }
            });

            all_instruments.extend(fetched);
        }

        log::debug!(
            "Cached instruments: client_id={}, total={}",
            self.client_id,
            all_instruments.len()
        );

        for instrument in &all_instruments {
            if let Err(e) = self
                .data_sender
                .send(DataEvent::Instrument(instrument.clone()))
            {
                log::warn!("Failed to send instrument: {e}");
            }
        }

        // Cache instruments and set subscription filters in WebSocket client before connecting
        let mark_price_subs = self.mark_price_subs.clone();
        let index_price_subs = self.index_price_subs.clone();
        let option_greeks_subs = self.option_greeks_subs.clone();
        let ws = self.ws_client_mut()?;
        ws.cache_instruments(&all_instruments);
        ws.set_mark_price_subs(mark_price_subs);
        ws.set_index_price_subs(index_price_subs);
        ws.set_option_greeks_subs(option_greeks_subs);

        // Connect WebSocket and wait until active
        ws.connect().await.context("failed to connect WebSocket")?;
        let activation_result = async {
            ws.wait_until_active(10.0)
                .await
                .context("WebSocket failed to become active")?;

            // Authenticate if credentials are configured (required for raw streams)
            if ws.has_credentials() {
                ws.authenticate_session(DERIBIT_DATA_SESSION_NAME)
                    .await
                    .context("failed to authenticate WebSocket")?;
                log_debug!("WebSocket authenticated");
            }
            Ok::<(), anyhow::Error>(())
        }
        .await;

        if let Err(e) = activation_result {
            if let Err(teardown_error) = self.teardown_partial_connect().await {
                return Err(e.context(format!(
                    "Deribit data startup teardown failed: {teardown_error}"
                )));
            }
            return Err(e);
        }

        // Get the stream and spawn processing task
        let stream_result = self.ws_client_mut().and_then(|ws| Ok(ws.stream()?));
        let stream = match stream_result {
            Ok(stream) => stream,
            Err(e) => {
                if let Err(teardown_error) = self.teardown_partial_connect().await {
                    return Err(e.context(format!(
                        "Deribit data startup teardown failed: {teardown_error}"
                    )));
                }
                return Err(e);
            }
        };

        if let Err(e) = self.spawn_stream_task(stream) {
            if let Err(teardown_error) = self.teardown_partial_connect().await {
                return Err(e.context(format!(
                    "Deribit data startup teardown failed: {teardown_error}"
                )));
            }
            return Err(e);
        }

        self.is_connected.store(true, Ordering::Release);
        setup_guard.disarm();
        log_info!("Connected ({})", self.config.environment);
        Ok(())
    }

    async fn disconnect(&mut self) -> anyhow::Result<()> {
        self.teardown_partial_connect().await?;

        log_info!("Disconnected");
        Ok(())
    }

    fn subscribe_instruments(&mut self, cmd: SubscribeInstruments) -> anyhow::Result<()> {
        // Extract kind and currency from params, defaulting to "any.any" (all instruments)
        let kind = cmd
            .params
            .as_ref()
            .and_then(|p| p.get_str("kind"))
            .unwrap_or("any")
            .to_string();
        let currency = cmd
            .params
            .as_ref()
            .and_then(|p| p.get_str("currency"))
            .unwrap_or("any")
            .to_string();

        let ws = self
            .ws_client
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("WebSocket client not initialized"))?
            .clone();

        log::debug!("Subscribing to instrument state changes for {kind}.{currency}");

        self.spawn_command(async move {
            if let Err(e) = ws.subscribe_instrument_status(&kind, &currency).await {
                log::error!("Failed to subscribe to instrument status for {kind}.{currency}: {e}");
            }
        });

        Ok(())
    }

    fn subscribe_instrument(&mut self, cmd: SubscribeInstrument) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;

        // Check if instrument is in cache (should be from connect())
        if !self.instruments.contains_key(&instrument_id) {
            log::warn!(
                "Instrument {instrument_id} not in cache - it may have been created after connect()"
            );
        }

        // Determine kind and currency from instrument_id
        let (kind, currency) = parse_instrument_kind_currency(&instrument_id);

        let ws = self
            .ws_client
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("WebSocket client not initialized"))?
            .clone();

        log::debug!(
            "Subscribing to instrument state for {instrument_id} (channel: {kind}.{currency})"
        );

        // Subscribe to broader kind/currency channel (filter in handler)
        self.spawn_command(async move {
            if let Err(e) = ws.subscribe_instrument_status(&kind, &currency).await {
                log::error!("Failed to subscribe to instrument status for {instrument_id}: {e}");
            }
        });

        Ok(())
    }

    fn subscribe_book_deltas(&mut self, cmd: SubscribeBookDeltas) -> anyhow::Result<()> {
        if cmd.book_type != BookType::L2_MBP {
            anyhow::bail!("Deribit only supports L2_MBP order book deltas");
        }

        let instrument_id = cmd.instrument_id;
        let needs_load = self.prepare_subscribe(instrument_id)?;

        let ws = self
            .ws_client
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("WebSocket client not initialized"))?
            .clone();
        let http_client = self.http_client.clone();
        let instruments = Arc::clone(&self.instruments);
        let interval = self.get_interval(&cmd.params);

        let depth = cmd
            .depth
            .map(|d| d.get() as u32)
            .or_else(|| {
                cmd.params
                    .as_ref()
                    .and_then(|p| p.get_u64("depth"))
                    .map(|n| n as u32)
            })
            .unwrap_or(DERIBIT_BOOK_DEFAULT_DEPTH);

        if !DERIBIT_BOOK_VALID_DEPTHS.contains(&depth) {
            anyhow::bail!("invalid depth {depth}; supported depths: {DERIBIT_BOOK_VALID_DEPTHS:?}");
        }

        let group = cmd
            .params
            .as_ref()
            .and_then(|p| p.get_str("group"))
            .unwrap_or(DERIBIT_BOOK_DEFAULT_GROUP)
            .to_string();

        log::debug!(
            "Subscribing to book deltas for {} (group: {}, depth: {}, interval: {}, book_type: {:?})",
            instrument_id,
            group,
            depth,
            interval.map_or("100ms (default)".to_string(), |i| i.to_string()),
            cmd.book_type
        );

        self.spawn_command(async move {
            if needs_load
                && let Err(e) =
                    Self::lazy_load_instrument(&http_client, &ws, &instruments, instrument_id).await
            {
                log::error!("Lazy-load failed for {instrument_id} (book deltas): {e}");
                return;
            }

            let result = if interval == Some(DeribitUpdateInterval::Raw) {
                ws.subscribe_book(instrument_id, interval).await
            } else {
                ws.subscribe_book_grouped(instrument_id, &group, depth, interval)
                    .await
            };

            if let Err(e) = result {
                log::error!("Failed to subscribe to book deltas for {instrument_id}: {e}");
            }
        });

        Ok(())
    }

    fn subscribe_book_depth10(&mut self, cmd: SubscribeBookDepth10) -> anyhow::Result<()> {
        if cmd.book_type != BookType::L2_MBP {
            anyhow::bail!("Deribit only supports L2_MBP order book depth");
        }

        let instrument_id = cmd.instrument_id;
        let needs_load = self.prepare_subscribe(instrument_id)?;

        let ws = self
            .ws_client
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("WebSocket client not initialized"))?
            .clone();
        let http_client = self.http_client.clone();
        let instruments = Arc::clone(&self.instruments);
        let interval = self.get_interval(&cmd.params);
        let group = cmd
            .params
            .as_ref()
            .and_then(|p| p.get_str("group"))
            .unwrap_or(DERIBIT_BOOK_DEFAULT_GROUP)
            .to_string();

        log::debug!(
            "Subscribing to book depth10 for {} (group: {}, interval: {}, book_type: {:?})",
            instrument_id,
            group,
            interval.map_or("100ms (default)".to_string(), |i| i.to_string()),
            cmd.book_type
        );

        self.spawn_command(async move {
            if needs_load
                && let Err(e) =
                    Self::lazy_load_instrument(&http_client, &ws, &instruments, instrument_id).await
            {
                log::error!("Lazy-load failed for {instrument_id} (book depth10): {e}");
                return;
            }

            if let Err(e) = ws
                .subscribe_book_grouped(instrument_id, &group, 10, interval)
                .await
            {
                log::error!("Failed to subscribe to book depth10 for {instrument_id}: {e}");
            }
        });

        Ok(())
    }

    fn subscribe_quotes(&mut self, cmd: SubscribeQuotes) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;
        let command_id = cmd.command_id;
        let needs_load = self.prepare_subscribe(instrument_id)?;

        let ws = self
            .ws_client
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("WebSocket client not initialized"))?
            .clone();
        let http_client = self.http_client.clone();
        let instruments = Arc::clone(&self.instruments);

        self.spawn_command(async move {
            if needs_load
                && let Err(e) =
                    Self::lazy_load_instrument(&http_client, &ws, &instruments, instrument_id).await
            {
                log::error!(
                    "Lazy-load failed for {instrument_id} (quotes, command_id={command_id}): {e}"
                );
                return;
            }

            if let Err(e) = ws.subscribe_quotes(instrument_id).await {
                log::error!("Failed to subscribe to quotes for {instrument_id}: {e}");
            }
        });

        Ok(())
    }

    fn subscribe_trades(&mut self, cmd: SubscribeTrades) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;
        let command_id = cmd.command_id;
        let needs_load = self.prepare_subscribe(instrument_id)?;
        let subscribe_combo_legs = Self::subscribe_combo_legs(&cmd.params);

        let ws = self
            .ws_client
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("WebSocket client not initialized"))?
            .clone();
        let http_client = self.http_client.clone();
        let instruments = Arc::clone(&self.instruments);
        let combo_leg_trade_subs = Arc::clone(&self.combo_leg_trade_subs);
        let auto_load_missing_instruments = self.config.auto_load_missing_instruments;
        let interval = self.get_interval(&cmd.params);

        log::debug!(
            "Subscribing to trades for {} (interval: {})",
            instrument_id,
            interval.map_or("100ms (default)".to_string(), |i| i.to_string())
        );

        self.spawn_command(async move {
            if needs_load
                && let Err(e) =
                    Self::lazy_load_instrument(&http_client, &ws, &instruments, instrument_id).await
            {
                log::error!("Lazy-load failed for {instrument_id} (trades): {e}");
                return;
            }

            let mut subscription_ids = vec![instrument_id];

            if subscribe_combo_legs {
                let leg_ids = Self::combo_leg_trade_ids(&instruments, instrument_id);
                if leg_ids.is_empty() {
                    log::warn!(
                        "No Deribit combo legs found for trade subscription opt-in on {instrument_id}"
                    );
                }

                for leg_id in leg_ids {
                    if !instruments.contains_key(&leg_id) {
                        if !auto_load_missing_instruments {
                            log::error!(
                                "Instrument {leg_id} not found and `auto_load_missing_instruments` is disabled"
                            );
                            continue;
                        }

                        if let Err(e) =
                            Self::lazy_load_instrument(&http_client, &ws, &instruments, leg_id)
                                .await
                        {
                            log::error!("Lazy-load failed for {leg_id} (combo leg trades): {e}");
                            continue;
                        }
                    }

                    subscription_ids.push(leg_id);
                }
            }

            let subscription_count = subscription_ids.len();
            let mut opened_leg_ids = Vec::new();

            for subscription_id in subscription_ids {
                if let Err(e) = ws.subscribe_trades(subscription_id, interval).await {
                    log::error!("Failed to subscribe to trades for {subscription_id}: {e}");
                    continue;
                }

                if subscription_id != instrument_id {
                    opened_leg_ids.push(subscription_id);
                }
            }

            Self::track_combo_leg_trade_subs(
                &combo_leg_trade_subs,
                instrument_id,
                &opened_leg_ids,
            );

            log::debug!(
                "Processed trade subscription batch: command_id={command_id}, requests={subscription_count}, instrument={instrument_id}"
            );
        });

        Ok(())
    }

    fn subscribe_mark_prices(&mut self, cmd: SubscribeMarkPrices) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;
        let needs_load = self.prepare_subscribe(instrument_id)?;

        let ws = self
            .ws_client
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("WebSocket client not initialized"))?
            .clone();
        let http_client = self.http_client.clone();
        let instruments = Arc::clone(&self.instruments);
        let interval = self.get_interval(&cmd.params);

        // Track subscription so handler gates MarkPriceUpdate emission
        self.mark_price_subs.insert(instrument_id);

        log::debug!(
            "Subscribing to mark prices for {} (via ticker channel, interval: {})",
            instrument_id,
            interval.map_or("100ms (default)".to_string(), |i| i.to_string())
        );

        self.spawn_command(async move {
            if needs_load
                && let Err(e) =
                    Self::lazy_load_instrument(&http_client, &ws, &instruments, instrument_id).await
            {
                log::error!("Lazy-load failed for {instrument_id} (mark prices): {e}");
                return;
            }

            if let Err(e) = ws.subscribe_ticker(instrument_id, interval).await {
                log::error!("Failed to subscribe to mark prices for {instrument_id}: {e}");
            }
        });

        Ok(())
    }

    fn subscribe_index_prices(&mut self, cmd: SubscribeIndexPrices) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;
        let needs_load = self.prepare_subscribe(instrument_id)?;

        let ws = self
            .ws_client
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("WebSocket client not initialized"))?
            .clone();
        let http_client = self.http_client.clone();
        let instruments = Arc::clone(&self.instruments);
        let interval = self.get_interval(&cmd.params);

        // Track subscription so handler gates IndexPriceUpdate emission
        self.index_price_subs.insert(instrument_id);

        log::debug!(
            "Subscribing to index prices for {} (via ticker channel, interval: {})",
            instrument_id,
            interval.map_or("100ms (default)".to_string(), |i| i.to_string())
        );

        self.spawn_command(async move {
            if needs_load
                && let Err(e) =
                    Self::lazy_load_instrument(&http_client, &ws, &instruments, instrument_id).await
            {
                log::error!("Lazy-load failed for {instrument_id} (index prices): {e}");
                return;
            }

            if let Err(e) = ws.subscribe_ticker(instrument_id, interval).await {
                log::error!("Failed to subscribe to index prices for {instrument_id}: {e}");
            }
        });

        Ok(())
    }

    fn subscribe_bars(&mut self, cmd: SubscribeBars) -> anyhow::Result<()> {
        let instrument_id = cmd.bar_type.instrument_id();
        let needs_load = self.prepare_subscribe(instrument_id)?;

        let ws = self
            .ws_client
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("WebSocket client not initialized"))?
            .clone();
        let http_client = self.http_client.clone();
        let instruments = Arc::clone(&self.instruments);
        let resolution = bar_spec_to_resolution(&cmd.bar_type);

        self.spawn_command(async move {
            if needs_load
                && let Err(e) =
                    Self::lazy_load_instrument(&http_client, &ws, &instruments, instrument_id).await
            {
                log::error!("Lazy-load failed for {instrument_id} (bars): {e}");
                return;
            }

            if let Err(e) = ws.subscribe_chart(instrument_id, &resolution).await {
                log::error!("Failed to subscribe to bars for {instrument_id}: {e}");
            }
        });

        Ok(())
    }

    fn subscribe_funding_rates(&mut self, cmd: SubscribeFundingRates) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;
        let command_id = cmd.command_id;
        let needs_load = self.prepare_subscribe(instrument_id)?;

        let ws = self
            .ws_client
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("WebSocket client not initialized"))?
            .clone();
        let http_client = self.http_client.clone();
        let instruments = Arc::clone(&self.instruments);
        let interval = self.get_interval(&cmd.params);

        log::debug!(
            "Subscribing to funding rates for {} (perpetual channel, interval: {})",
            instrument_id,
            interval.map_or("100ms (default)".to_string(), |i| i.to_string())
        );

        self.spawn_command(async move {
            if needs_load
                && let Err(e) =
                    Self::lazy_load_instrument(&http_client, &ws, &instruments, instrument_id).await
            {
                log::error!("Lazy-load failed for {instrument_id} (funding rates): {e}");
                return;
            }

            // Funding rates only apply to perpetual contracts; check after any lazy-load
            let is_perpetual = instruments
                .load()
                .get(&instrument_id)
                .is_some_and(|inst| matches!(inst, InstrumentAny::CryptoPerpetual(_)));

            if !is_perpetual {
                log::warn!(
                    "Funding rates subscription rejected for {instrument_id} (command_id={command_id}): only available for perpetual instruments"
                );
                return;
            }

            if let Err(e) = ws
                .subscribe_perpetual_interests_rates_updates(instrument_id, interval)
                .await
            {
                log::error!("Failed to subscribe to funding rates for {instrument_id}: {e}");
            }
        });

        Ok(())
    }

    fn subscribe_instrument_status(
        &mut self,
        cmd: SubscribeInstrumentStatus,
    ) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;
        let (kind, currency) = parse_instrument_kind_currency(&instrument_id);

        let ws = self
            .ws_client
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("WebSocket client not initialized"))?
            .clone();

        log::debug!("Subscribing to instrument status for {instrument_id} ({kind}.{currency})");

        self.spawn_command(async move {
            if let Err(e) = ws.subscribe_instrument_status(&kind, &currency).await {
                log::error!("Failed to subscribe to instrument status for {instrument_id}: {e}");
            }
        });

        Ok(())
    }

    fn subscribe_option_greeks(&mut self, cmd: SubscribeOptionGreeks) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;
        let needs_load = self.prepare_subscribe(instrument_id)?;

        let ws = self
            .ws_client
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("WebSocket client not initialized"))?
            .clone();
        let http_client = self.http_client.clone();
        let instruments = Arc::clone(&self.instruments);
        let interval = self.get_interval(&cmd.params);

        // Track subscription so handler gates OptionGreeks emission
        self.option_greeks_subs.insert(instrument_id);

        log::debug!(
            "Subscribing to option greeks for {} (via ticker channel, interval: {})",
            instrument_id,
            interval.map_or("100ms (default)".to_string(), |i| i.to_string())
        );

        self.spawn_command(async move {
            if needs_load
                && let Err(e) =
                    Self::lazy_load_instrument(&http_client, &ws, &instruments, instrument_id).await
            {
                log::error!("Lazy-load failed for {instrument_id} (option greeks): {e}");
                return;
            }

            if let Err(e) = ws.subscribe_ticker(instrument_id, interval).await {
                log::error!("Failed to subscribe to option greeks for {instrument_id}: {e}");
            }
        });

        Ok(())
    }

    fn subscribe(&mut self, cmd: SubscribeCustomData) -> anyhow::Result<()> {
        let data_type = cmd.data_type.type_name();
        if data_type != "DeribitVolatilityIndex" {
            log::warn!("Unsupported custom data subscription: {data_type}");
            return Ok(());
        }

        let Some(index_name) = cmd
            .data_type
            .metadata()
            .as_ref()
            .and_then(|m| m.get("index_name"))
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string)
        else {
            log::warn!(
                "Rejected Deribit volatility index subscription: missing required metadata `index_name`"
            );
            return Ok(());
        };

        log::debug!("Subscribing to Deribit volatility index: {index_name}");

        let ws = self
            .ws_client
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("WebSocket client not initialized"))?
            .clone();

        self.spawn_command(async move {
            if let Err(e) = ws.subscribe_volatility_index(&index_name).await {
                log::error!("Failed to subscribe to volatility index {index_name}: {e}");
            }
        });

        Ok(())
    }

    fn unsubscribe_instrument_status(
        &mut self,
        cmd: &UnsubscribeInstrumentStatus,
    ) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;
        let (kind, currency) = parse_instrument_kind_currency(&instrument_id);

        let ws = self
            .ws_client
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("WebSocket client not initialized"))?
            .clone();

        log::debug!("Unsubscribing from instrument status for {instrument_id} ({kind}.{currency})");

        self.spawn_command(async move {
            if let Err(e) = ws.unsubscribe_instrument_status(&kind, &currency).await {
                log::error!(
                    "Failed to unsubscribe from instrument status for {instrument_id}: {e}"
                );
            }
        });

        Ok(())
    }

    fn unsubscribe_instruments(&mut self, cmd: &UnsubscribeInstruments) -> anyhow::Result<()> {
        let kind = cmd
            .params
            .as_ref()
            .and_then(|p| p.get_str("kind"))
            .unwrap_or("any")
            .to_string();
        let currency = cmd
            .params
            .as_ref()
            .and_then(|p| p.get_str("currency"))
            .unwrap_or("any")
            .to_string();

        let ws = self
            .ws_client
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("WebSocket client not initialized"))?
            .clone();

        log::debug!("Unsubscribing from instrument state changes for {kind}.{currency}");

        self.spawn_command(async move {
            if let Err(e) = ws.unsubscribe_instrument_status(&kind, &currency).await {
                log::error!(
                    "Failed to unsubscribe from instrument status for {kind}.{currency}: {e}"
                );
            }
        });

        Ok(())
    }

    fn unsubscribe_instrument(&mut self, cmd: &UnsubscribeInstrument) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;

        // Determine kind and currency from instrument_id
        let (kind, currency) = parse_instrument_kind_currency(&instrument_id);

        let ws = self
            .ws_client
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("WebSocket client not initialized"))?
            .clone();

        log::debug!(
            "Unsubscribing from instrument state for {instrument_id} (channel: {kind}.{currency})"
        );

        self.spawn_command(async move {
            if let Err(e) = ws.unsubscribe_instrument_status(&kind, &currency).await {
                log::error!(
                    "Failed to unsubscribe from instrument status for {instrument_id}: {e}"
                );
            }
        });

        Ok(())
    }

    fn unsubscribe_book_deltas(&mut self, cmd: &UnsubscribeBookDeltas) -> anyhow::Result<()> {
        let ws = self
            .ws_client
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("WebSocket client not initialized"))?
            .clone();
        let instrument_id = cmd.instrument_id;
        let interval = self.get_interval(&cmd.params);

        let depth = cmd
            .params
            .as_ref()
            .and_then(|p| p.get_u64("depth"))
            .map_or(DERIBIT_BOOK_DEFAULT_DEPTH, |n| n as u32);

        if !DERIBIT_BOOK_VALID_DEPTHS.contains(&depth) {
            anyhow::bail!("invalid depth {depth}; supported depths: {DERIBIT_BOOK_VALID_DEPTHS:?}");
        }

        let group = cmd
            .params
            .as_ref()
            .and_then(|p| p.get_str("group"))
            .unwrap_or(DERIBIT_BOOK_DEFAULT_GROUP)
            .to_string();

        log::debug!(
            "Unsubscribing from book deltas for {} (group: {}, depth: {}, interval: {})",
            instrument_id,
            group,
            depth,
            interval.map_or("100ms (default)".to_string(), |i| i.to_string())
        );

        self.spawn_command(async move {
            let result = if interval == Some(DeribitUpdateInterval::Raw) {
                ws.unsubscribe_book(instrument_id, interval).await
            } else {
                ws.unsubscribe_book_grouped(instrument_id, &group, depth, interval)
                    .await
            };

            if let Err(e) = result {
                log::error!("Failed to unsubscribe from book deltas for {instrument_id}: {e}");
            }
        });

        Ok(())
    }

    fn unsubscribe_book_depth10(&mut self, cmd: &UnsubscribeBookDepth10) -> anyhow::Result<()> {
        let ws = self
            .ws_client
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("WebSocket client not initialized"))?
            .clone();
        let instrument_id = cmd.instrument_id;
        let interval = self.get_interval(&cmd.params);
        let group = cmd
            .params
            .as_ref()
            .and_then(|p| p.get_str("group"))
            .unwrap_or(DERIBIT_BOOK_DEFAULT_GROUP)
            .to_string();

        log::debug!(
            "Unsubscribing from book depth10 for {} (group: {}, interval: {})",
            instrument_id,
            group,
            interval.map_or("100ms (default)".to_string(), |i| i.to_string())
        );

        self.spawn_command(async move {
            if let Err(e) = ws
                .unsubscribe_book_grouped(instrument_id, &group, 10, interval)
                .await
            {
                log::error!("Failed to unsubscribe from book depth10 for {instrument_id}: {e}");
            }
        });

        Ok(())
    }

    fn unsubscribe_quotes(&mut self, cmd: &UnsubscribeQuotes) -> anyhow::Result<()> {
        let ws = self
            .ws_client
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("WebSocket client not initialized"))?
            .clone();
        let instrument_id = cmd.instrument_id;

        self.spawn_command(async move {
            if let Err(e) = ws.unsubscribe_quotes(instrument_id).await {
                log::error!("Failed to unsubscribe from quotes for {instrument_id}: {e}");
            }
        });

        Ok(())
    }

    fn unsubscribe_trades(&mut self, cmd: &UnsubscribeTrades) -> anyhow::Result<()> {
        let ws = self
            .ws_client
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("WebSocket client not initialized"))?
            .clone();
        let instrument_id = cmd.instrument_id;
        let command_id = cmd.command_id;
        let interval = self.get_interval(&cmd.params);
        let mut subscription_ids = vec![instrument_id];
        subscription_ids.extend(Self::combo_leg_trade_unsubs(
            &self.combo_leg_trade_subs,
            instrument_id,
        ));
        let subscription_count = subscription_ids.len();

        log::debug!(
            "Unsubscribing from trades for {} instruments from {} (interval: {})",
            subscription_ids.len(),
            instrument_id,
            interval.map_or("100ms (default)".to_string(), |i| i.to_string())
        );

        self.spawn_command(async move {
            for subscription_id in subscription_ids {
                if let Err(e) = ws.unsubscribe_trades(subscription_id, interval).await {
                    log::error!("Failed to unsubscribe from trades for {subscription_id}: {e}");
                }
            }

            log::debug!(
                "Processed trade unsubscription batch: command_id={command_id}, requests={subscription_count}, instrument={instrument_id}"
            );
        });

        Ok(())
    }

    fn unsubscribe_mark_prices(&mut self, cmd: &UnsubscribeMarkPrices) -> anyhow::Result<()> {
        let ws = self
            .ws_client
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("WebSocket client not initialized"))?
            .clone();
        let instrument_id = cmd.instrument_id;
        let interval = self.get_interval(&cmd.params);

        // Remove subscription tracking so handler stops emitting MarkPriceUpdate
        self.mark_price_subs.remove(&instrument_id);

        log::debug!(
            "Unsubscribing from mark prices for {} (via ticker channel, interval: {})",
            instrument_id,
            interval.map_or("100ms (default)".to_string(), |i| i.to_string())
        );

        self.spawn_command(async move {
            if let Err(e) = ws.unsubscribe_ticker(instrument_id, interval).await {
                log::error!("Failed to unsubscribe from mark prices for {instrument_id}: {e}");
            }
        });

        Ok(())
    }

    fn unsubscribe_index_prices(&mut self, cmd: &UnsubscribeIndexPrices) -> anyhow::Result<()> {
        let ws = self
            .ws_client
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("WebSocket client not initialized"))?
            .clone();
        let instrument_id = cmd.instrument_id;
        let interval = self.get_interval(&cmd.params);

        // Remove subscription tracking so handler stops emitting IndexPriceUpdate
        self.index_price_subs.remove(&instrument_id);

        log::debug!(
            "Unsubscribing from index prices for {} (via ticker channel, interval: {})",
            instrument_id,
            interval.map_or("100ms (default)".to_string(), |i| i.to_string())
        );

        self.spawn_command(async move {
            if let Err(e) = ws.unsubscribe_ticker(instrument_id, interval).await {
                log::error!("Failed to unsubscribe from index prices for {instrument_id}: {e}");
            }
        });

        Ok(())
    }

    fn unsubscribe_bars(&mut self, cmd: &UnsubscribeBars) -> anyhow::Result<()> {
        let ws = self
            .ws_client
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("WebSocket client not initialized"))?
            .clone();
        let instrument_id = cmd.bar_type.instrument_id();
        let resolution = bar_spec_to_resolution(&cmd.bar_type);

        self.spawn_command(async move {
            if let Err(e) = ws.unsubscribe_chart(instrument_id, &resolution).await {
                log::error!("Failed to unsubscribe from bars for {instrument_id}: {e}");
            }
        });

        Ok(())
    }

    fn unsubscribe_funding_rates(&mut self, cmd: &UnsubscribeFundingRates) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;

        // Validate instrument is a perpetual - funding rates only apply to perpetual contracts
        let is_perpetual = self
            .instruments
            .load()
            .get(&instrument_id)
            .is_some_and(|inst| matches!(inst, InstrumentAny::CryptoPerpetual(_)));

        if !is_perpetual {
            log::warn!(
                "Funding rates unsubscription rejected for {instrument_id}: only available for perpetual instruments"
            );
            return Ok(());
        }

        let ws = self
            .ws_client
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("WebSocket client not initialized"))?
            .clone();
        let interval = self.get_interval(&cmd.params);

        log::debug!(
            "Unsubscribing from funding rates for {} (perpetual channel, interval: {})",
            instrument_id,
            interval.map_or("100ms (default)".to_string(), |i| i.to_string())
        );

        self.spawn_command(async move {
            if let Err(e) = ws
                .unsubscribe_perpetual_interest_rates_updates(instrument_id, interval)
                .await
            {
                log::error!("Failed to unsubscribe from funding rates for {instrument_id}: {e}");
            }
        });

        Ok(())
    }

    fn unsubscribe_option_greeks(&mut self, cmd: &UnsubscribeOptionGreeks) -> anyhow::Result<()> {
        let ws = self
            .ws_client
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("WebSocket client not initialized"))?
            .clone();
        let instrument_id = cmd.instrument_id;
        let interval = self.get_interval(&cmd.params);

        // Remove subscription tracking so handler stops emitting OptionGreeks
        self.option_greeks_subs.remove(&instrument_id);

        log::debug!(
            "Unsubscribing from option greeks for {} (via ticker channel, interval: {})",
            instrument_id,
            interval.map_or("100ms (default)".to_string(), |i| i.to_string())
        );

        self.spawn_command(async move {
            if let Err(e) = ws.unsubscribe_ticker(instrument_id, interval).await {
                log::error!("Failed to unsubscribe from option greeks for {instrument_id}: {e}");
            }
        });

        Ok(())
    }

    fn unsubscribe(&mut self, cmd: &UnsubscribeCustomData) -> anyhow::Result<()> {
        let data_type = cmd.data_type.type_name();
        if data_type != "DeribitVolatilityIndex" {
            log::warn!("Unsupported custom data unsubscription: {data_type}");
            return Ok(());
        }

        let Some(index_name) = cmd
            .data_type
            .metadata()
            .as_ref()
            .and_then(|m| m.get("index_name"))
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string)
        else {
            log::warn!(
                "Rejected Deribit volatility index unsubscription: missing required metadata `index_name`"
            );
            return Ok(());
        };

        log::debug!("Unsubscribing from Deribit volatility index: {index_name}");

        let ws = self
            .ws_client
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("WebSocket client not initialized"))?
            .clone();

        self.spawn_command(async move {
            if let Err(e) = ws.unsubscribe_volatility_index(&index_name).await {
                log::error!("Failed to unsubscribe from volatility index {index_name}: {e}");
            }
        });

        Ok(())
    }

    fn request_instruments(&self, request: RequestInstruments) -> anyhow::Result<()> {
        if request.start.is_some() {
            log::warn!(
                "Requesting instruments for {:?} with specified `start` which has no effect",
                request.venue
            );
        }

        if request.end.is_some() {
            log::warn!(
                "Requesting instruments for {:?} with specified `end` which has no effect",
                request.venue
            );
        }

        let http_client = self.http_client.clone();
        let ws_client = self.ws_client.clone();
        let instruments_cache = Arc::clone(&self.instruments);
        let sender = self.data_sender.clone();
        let request_id = request.request_id;
        let client_id = request.client_id.unwrap_or(self.client_id);
        let start_nanos = datetime_to_unix_nanos(request.start);
        let end_nanos = datetime_to_unix_nanos(request.end);
        let params = request.params;
        let clock = self.clock;
        let venue = *DERIBIT_VENUE;

        // Get product types from config, default to Future if empty
        let product_types = if self.config.product_types.is_empty() {
            vec![crate::http::models::DeribitProductType::Future]
        } else {
            self.config.product_types.clone()
        };

        self.spawn_command(async move {
            let mut all_instruments = Vec::new();

            for product_type in &product_types {
                log::debug!(
                    "Requesting instruments for currency=ANY, product_type={product_type:?}"
                );

                match http_client
                    .request_instruments(DeribitCurrency::ANY, Some(*product_type))
                    .await
                {
                    Ok(instruments) => {
                        log::debug!(
                            "Fetched {} instruments for ANY/{:?}",
                            instruments.len(),
                            product_type
                        );

                        instruments_cache.rcu(|m| {
                            for instrument in &instruments {
                                m.insert(instrument.id(), instrument.clone());
                            }
                        });
                        all_instruments.extend(instruments);
                    }
                    Err(e) => {
                        log::error!("Failed to fetch instruments for ANY/{product_type:?}: {e:?}");
                    }
                }
            }

            // Propagate to HTTP and WebSocket caches so downstream
            // requests use correct precisions.
            if !all_instruments.is_empty() {
                http_client.cache_instruments(&all_instruments);

                if let Some(ws) = &ws_client {
                    ws.cache_instruments(&all_instruments);
                }
            }

            // Send response with all collected instruments
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
        if request.start.is_some() {
            log::warn!(
                "Requesting instrument {} with specified `start` which has no effect",
                request.instrument_id
            );
        }

        if request.end.is_some() {
            log::warn!(
                "Requesting instrument {} with specified `end` which has no effect",
                request.instrument_id
            );
        }

        log::debug!("Fetching instrument {} from API", request.instrument_id);

        let http_client = self.http_client.clone();
        let ws_client = self.ws_client.clone();
        let instruments_cache = Arc::clone(&self.instruments);
        let sender = self.data_sender.clone();
        let instrument_id = request.instrument_id;
        let request_id = request.request_id;
        let client_id = request.client_id.unwrap_or(self.client_id);
        let start_nanos = datetime_to_unix_nanos(request.start);
        let end_nanos = datetime_to_unix_nanos(request.end);
        let params = request.params;
        let clock = self.clock;

        self.spawn_command(async move {
            match http_client
                .request_instrument(instrument_id)
                .await
                .context("failed to request instrument from Deribit")
            {
                Ok(instrument) => {
                    log::debug!("Successfully fetched instrument: {instrument_id}");

                    instruments_cache.insert(instrument.id(), instrument.clone());
                    http_client.cache_instruments(std::slice::from_ref(&instrument));

                    if let Some(ws) = &ws_client {
                        ws.cache_instruments(std::slice::from_ref(&instrument));
                    }

                    // Send response
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
                    log::error!("Instrument request failed for {instrument_id}: {e:?}");
                }
            }
        });

        Ok(())
    }

    fn request_trades(&self, request: RequestTrades) -> anyhow::Result<()> {
        let http_client = self.http_client.clone();
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

        self.spawn_command(async move {
            match http_client
                .request_trades(instrument_id, start, end, limit)
                .await
                .context("failed to request trades from Deribit")
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
                Err(e) => log::error!("Trades request failed for {instrument_id}: {e:?}"),
            }
        });

        Ok(())
    }

    fn request_bars(&self, request: RequestBars) -> anyhow::Result<()> {
        let http_client = self.http_client.clone();
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

        self.spawn_command(async move {
            match http_client
                .request_bars(bar_type, start, end, limit)
                .await
                .context("failed to request bars from Deribit")
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
                Err(e) => log::error!("Bars request failed for {bar_type}: {e:?}"),
            }
        });

        Ok(())
    }

    fn request_book_snapshot(&self, request: RequestBookSnapshot) -> anyhow::Result<()> {
        let http_client = self.http_client.clone();
        let sender = self.data_sender.clone();
        let instrument_id = request.instrument_id;
        let depth = request.depth.map(|n| n.get() as u32);
        let request_id = request.request_id;
        let client_id = request.client_id.unwrap_or(self.client_id);
        let params = request.params;
        let clock = self.clock;

        self.spawn_command(async move {
            match http_client
                .request_book_snapshot(instrument_id, depth)
                .await
                .context("failed to request book snapshot from Deribit")
            {
                Ok(book) => {
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
                    log::error!("Book snapshot request failed for {instrument_id}: {e:?}");
                }
            }
        });

        Ok(())
    }

    fn request_data(&self, request: RequestCustomData) -> anyhow::Result<()> {
        if request.data_type.type_name() != Self::BOOK_SUMMARY_TYPE_NAME {
            log::warn!(
                "Unsupported custom data request: {}",
                request.data_type.type_name()
            );
            return Ok(());
        }

        let currency = Self::book_summary_metadata_currency(&request.data_type)?;
        let kind = Self::book_summary_metadata_kind(&request.data_type);
        let kind_str = kind.as_deref().unwrap_or("option").to_string();
        let data_type = Self::book_summary_data_type(&currency, Some(&kind_str));
        let http_client = self.http_client.clone();
        let sender = self.data_sender.clone();
        let request_id = request.request_id;
        let client_id = request.client_id;
        let params = request.params;
        let clock = self.clock;
        let venue = *DERIBIT_VENUE;
        let start = request.start;
        let end = request.end;
        let start_nanos = datetime_to_unix_nanos(start);
        let end_nanos = datetime_to_unix_nanos(end);

        self.spawn_command(async move {
            log::debug!(
                "Requesting Deribit book summaries for currency={currency} kind={kind_str}"
            );

            match http_client
                .request_book_summaries_kind(&currency, Some(&kind_str))
                .await
            {
                Ok(summaries) => {
                    let ts = clock.get_time_ns();
                    let data: Vec<CustomData> = summaries
                        .into_iter()
                        .map(|raw| {
                            CustomData::new(
                                Arc::new(DeribitBookSummary::from_raw(raw, ts)),
                                data_type.clone(),
                            )
                        })
                        .collect();

                    let response = DataResponse::Data(CustomDataResponse::new(
                        request_id,
                        client_id,
                        Some(venue),
                        data_type,
                        data,
                        start_nanos,
                        end_nanos,
                        ts,
                        params,
                    ));

                    if let Err(e) = sender.send(DataEvent::Response(response)) {
                        log::error!("Failed to send book summary response: {e}");
                    }
                }
                Err(e) => {
                    // Empty response keeps request correlation closed for strategy waiters.
                    log::error!(
                        "Book summary request failed for currency={currency} kind={kind_str}: {e:?}"
                    );
                    let ts = clock.get_time_ns();
                    let response = DataResponse::Data(CustomDataResponse::new(
                        request_id,
                        client_id,
                        Some(venue),
                        data_type,
                        Vec::<CustomData>::new(),
                        start_nanos,
                        end_nanos,
                        ts,
                        params,
                    ));

                    if let Err(send_err) = sender.send(DataEvent::Response(response)) {
                        log::error!("Failed to send empty book summary response: {send_err}");
                    }
                }
            }
        });

        Ok(())
    }

    fn request_forward_prices(&self, request: RequestForwardPrices) -> anyhow::Result<()> {
        let currency = request.underlying.to_string();
        let instrument_id = request.instrument_id;
        let http_client = self.http_client.clone();
        let sender = self.data_sender.clone();
        let request_id = request.request_id;
        let client_id = request.client_id.unwrap_or(self.client_id());
        let params = request.params;
        let clock = self.clock;
        let venue = *DERIBIT_VENUE;

        self.spawn_command(async move {
            let result = if let Some(inst_id) = instrument_id {
                // Single-instrument path: 1 HTTP call to public/ticker
                let instrument_name = inst_id.symbol.to_string();
                log::debug!(
                    "Requesting forward price for {currency} (single instrument: {instrument_name})"
                );

                match http_client.request_ticker(&instrument_name).await {
                    Ok(ticker) => {
                        let ts = clock.get_time_ns();
                        let forward_prices: Vec<ForwardPrice> = ticker
                            .underlying_price
                            .map(|up| {
                                vec![ForwardPrice::new(
                                    inst_id,
                                    up,
                                    ticker.underlying_index.filter(|s| !s.is_empty()),
                                    ts,
                                    ts,
                                )]
                            })
                            .unwrap_or_default();

                        log::debug!(
                            "Fetched {} forward price for {currency} (single instrument: {instrument_name})",
                            forward_prices.len(),
                        );
                        Ok((forward_prices, ts))
                    }
                    Err(e) => Err(e),
                }
            } else {
                // Bulk path: fetch all book summaries
                log::debug!("Requesting option forward prices for currency={currency} (bulk)");

                match http_client.request_book_summaries(&currency).await {
                    Ok(summaries) => {
                        let ts = clock.get_time_ns();

                        // Deduplicate: all options at the same expiry share the same
                        // forward price, so keep only one entry per underlying_index.
                        let mut seen_indices = std::collections::HashSet::new();
                        let forward_prices: Vec<ForwardPrice> = summaries
                            .into_iter()
                            .filter_map(|s| {
                                let up = s.underlying_price?;
                                let idx = s.underlying_index.clone().unwrap_or_default();
                                if !seen_indices.insert(idx.clone()) {
                                    return None;
                                }
                                Some(ForwardPrice::new(
                                    InstrumentId::new(
                                        Symbol::new(&s.instrument_name),
                                        *DERIBIT_VENUE,
                                    ),
                                    up,
                                    Some(idx).filter(|s| !s.is_empty()),
                                    ts,
                                    ts,
                                ))
                            })
                            .collect();

                        log::debug!(
                            "Fetched {} forward prices (per-expiry) for {currency}",
                            forward_prices.len(),
                        );
                        Ok((forward_prices, ts))
                    }
                    Err(e) => Err(e),
                }
            };

            match result {
                Ok((forward_prices, ts)) => {
                    let response = DataResponse::ForwardPrices(ForwardPricesResponse::new(
                        request_id,
                        client_id,
                        venue,
                        forward_prices,
                        ts,
                        params,
                    ));

                    if let Err(e) = sender.send(DataEvent::Response(response)) {
                        log::error!("Failed to send forward prices response: {e}");
                    }
                }
                Err(e) => {
                    log::error!("Forward prices request failed for {currency}: {e:?}");
                }
            }
        });

        Ok(())
    }
}
