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

//! Live market data client implementation for the OKX adapter.

use std::{
    future::Future,
    sync::{
        Arc, RwLock,
        atomic::{AtomicBool, Ordering},
    },
};

use ahash::AHashMap;
use anyhow::Context;
use chrono::{DateTime, Utc};
use futures_util::StreamExt;
use nautilus_common::{
    messages::{
        DataEvent,
        data::{
            BarsResponse, DataResponse, InstrumentResponse, InstrumentsResponse, RequestBars,
            RequestInstrument, RequestInstruments, RequestTrades, SubscribeBars,
            SubscribeBookDeltas, SubscribeBookSnapshots, SubscribeFundingRates,
            SubscribeIndexPrices, SubscribeMarkPrices, SubscribeQuotes, SubscribeTrades,
            TradesResponse, UnsubscribeBars, UnsubscribeBookDeltas, UnsubscribeBookSnapshots,
            UnsubscribeFundingRates, UnsubscribeIndexPrices, UnsubscribeMarkPrices,
            UnsubscribeQuotes, UnsubscribeTrades,
        },
    },
    runner::get_data_event_sender,
};
use nautilus_core::{
    MUTEX_POISONED, UnixNanos,
    time::{AtomicTime, get_atomic_clock_realtime},
};
use nautilus_data::client::DataClient;
use nautilus_model::{
    data::{Data, FundingRateUpdate, OrderBookDeltas_API},
    enums::BookType,
    identifiers::{ClientId, InstrumentId, Venue},
    instruments::{Instrument, InstrumentAny},
};
use tokio::{task::JoinHandle, time::Duration};
use tokio_util::sync::CancellationToken;

use crate::{
    common::{
        consts::OKX_VENUE,
        enums::{OKXBookChannel, OKXContractType, OKXInstrumentType, OKXVipLevel},
    },
    config::OKXDataClientConfig,
    http::client::OKXHttpClient,
    websocket::{client::OKXWebSocketClient, messages::NautilusWsMessage},
};

#[derive(Debug)]
pub struct OKXDataClient {
    client_id: ClientId,
    config: OKXDataClientConfig,
    http_client: OKXHttpClient,
    ws_public: Option<OKXWebSocketClient>,
    ws_business: Option<OKXWebSocketClient>,
    is_connected: AtomicBool,
    cancellation_token: CancellationToken,
    tasks: Vec<JoinHandle<()>>,
    data_sender: tokio::sync::mpsc::UnboundedSender<DataEvent>,
    instruments: Arc<RwLock<AHashMap<InstrumentId, InstrumentAny>>>,
    book_channels: Arc<RwLock<AHashMap<InstrumentId, OKXBookChannel>>>,
    clock: &'static AtomicTime,
    instrument_refresh_active: bool,
}

impl OKXDataClient {
    /// Creates a new [`OKXDataClient`] instance.
    ///
    /// # Errors
    ///
    /// Returns an error if the client fails to initialize.
    pub fn new(client_id: ClientId, config: OKXDataClientConfig) -> anyhow::Result<Self> {
        let clock = get_atomic_clock_realtime();
        let data_sender = get_data_event_sender();

        let http_client = if config.has_api_credentials() {
            OKXHttpClient::with_credentials(
                config.api_key.clone(),
                config.api_secret.clone(),
                config.api_passphrase.clone(),
                config.base_url_http.clone(),
                config.http_timeout_secs,
                config.max_retries,
                config.retry_delay_initial_ms,
                config.retry_delay_max_ms,
                config.is_demo,
            )?
        } else {
            OKXHttpClient::new(
                config.base_url_http.clone(),
                config.http_timeout_secs,
                config.max_retries,
                config.retry_delay_initial_ms,
                config.retry_delay_max_ms,
                config.is_demo,
            )?
        };

        let ws_public =
            OKXWebSocketClient::new(Some(config.ws_public_url()), None, None, None, None, None)
                .context("failed to construct OKX public websocket client")?;

        let ws_business = if config.requires_business_ws() {
            Some(
                OKXWebSocketClient::new(
                    Some(config.ws_business_url()),
                    config.api_key.clone(),
                    config.api_secret.clone(),
                    config.api_passphrase.clone(),
                    None,
                    None,
                )
                .context("failed to construct OKX business websocket client")?,
            )
        } else {
            None
        };

        if let Some(vip_level) = config.vip_level {
            ws_public.set_vip_level(vip_level);
            if let Some(ref ws) = ws_business {
                ws.set_vip_level(vip_level);
            }
        }

        Ok(Self {
            client_id,
            config,
            http_client,
            ws_public: Some(ws_public),
            ws_business,
            is_connected: AtomicBool::new(false),
            cancellation_token: CancellationToken::new(),
            tasks: Vec::new(),
            data_sender,
            instruments: Arc::new(RwLock::new(AHashMap::new())),
            book_channels: Arc::new(RwLock::new(AHashMap::new())),
            clock,
            instrument_refresh_active: false,
        })
    }

    fn venue(&self) -> Venue {
        *OKX_VENUE
    }

    fn vip_level(&self) -> Option<OKXVipLevel> {
        self.ws_public.as_ref().map(|ws| ws.vip_level())
    }

    fn public_ws(&self) -> anyhow::Result<&OKXWebSocketClient> {
        self.ws_public
            .as_ref()
            .context("public websocket client not initialized")
    }

    fn public_ws_mut(&mut self) -> anyhow::Result<&mut OKXWebSocketClient> {
        self.ws_public
            .as_mut()
            .context("public websocket client not initialized")
    }

    fn business_ws(&self) -> anyhow::Result<&OKXWebSocketClient> {
        self.ws_business
            .as_ref()
            .context("business websocket client not available (credentials required)")
    }

    fn business_ws_mut(&mut self) -> anyhow::Result<&mut OKXWebSocketClient> {
        self.ws_business
            .as_mut()
            .context("business websocket client not available (credentials required)")
    }

    fn send_data(sender: &tokio::sync::mpsc::UnboundedSender<DataEvent>, data: Data) {
        if let Err(e) = sender.send(DataEvent::Data(data)) {
            tracing::error!("Failed to emit data event: {e}");
        }
    }

    fn spawn_ws<F>(&self, fut: F, context: &'static str)
    where
        F: Future<Output = anyhow::Result<()>> + Send + 'static,
    {
        tokio::spawn(async move {
            if let Err(e) = fut.await {
                tracing::error!("{context}: {e:?}");
            }
        });
    }

    async fn bootstrap_instruments(&mut self) -> anyhow::Result<Vec<InstrumentAny>> {
        let instrument_types = if self.config.instrument_types.is_empty() {
            vec![OKXInstrumentType::Spot]
        } else {
            self.config.instrument_types.clone()
        };

        let mut collected: Vec<InstrumentAny> = Vec::new();

        for inst_type in instrument_types {
            let mut instruments = self
                .http_client
                .request_instruments(inst_type, None)
                .await
                .with_context(|| format!("failed to load instruments for {inst_type:?}"))?;
            instruments.retain(|instrument| self.contract_filter(instrument));
            tracing::debug!(
                "loaded {count} instruments for {inst_type:?}",
                count = instruments.len()
            );
            collected.extend(instruments);
        }

        if collected.is_empty() {
            tracing::warn!("No OKX instruments were loaded");
            return Ok(collected);
        }

        self.http_client.add_instruments(collected.clone());

        if let Some(ws) = self.ws_public.as_mut() {
            ws.initialize_instruments_cache(collected.clone());
        }
        if let Some(ws) = self.ws_business.as_mut() {
            ws.initialize_instruments_cache(collected.clone());
        }

        {
            let mut guard = self
                .instruments
                .write()
                .expect("instrument cache lock poisoned");
            guard.clear();
            for instrument in &collected {
                guard.insert(instrument.id(), instrument.clone());
            }
        }

        Ok(collected)
    }

    fn contract_filter(&self, instrument: &InstrumentAny) -> bool {
        contract_filter_with_config(&self.config, instrument)
    }

    fn handle_ws_message(
        message: NautilusWsMessage,
        data_sender: &tokio::sync::mpsc::UnboundedSender<DataEvent>,
        instruments: &Arc<RwLock<AHashMap<InstrumentId, InstrumentAny>>>,
    ) {
        match message {
            NautilusWsMessage::Data(payloads) => {
                for data in payloads {
                    Self::send_data(data_sender, data);
                }
            }
            NautilusWsMessage::Deltas(deltas) => {
                Self::send_data(data_sender, Data::Deltas(OrderBookDeltas_API::new(deltas)));
            }
            NautilusWsMessage::FundingRates(updates) => {
                emit_funding_rates(updates);
            }
            NautilusWsMessage::Instrument(instrument) => {
                upsert_instrument(instruments, *instrument);
            }
            NautilusWsMessage::AccountUpdate(_)
            | NautilusWsMessage::OrderRejected(_)
            | NautilusWsMessage::OrderCancelRejected(_)
            | NautilusWsMessage::OrderModifyRejected(_)
            | NautilusWsMessage::ExecutionReports(_) => {
                tracing::debug!("Ignoring trading message on data client");
            }
            NautilusWsMessage::Error(e) => {
                tracing::error!("OKX websocket error: {e:?}");
            }
            NautilusWsMessage::Raw(value) => {
                tracing::debug!("Unhandled websocket payload: {value:?}");
            }
            NautilusWsMessage::Reconnected => {
                tracing::info!("Websocket reconnected");
            }
        }
    }

    fn spawn_public_stream(&mut self) -> anyhow::Result<()> {
        let ws = self.public_ws_mut()?;
        let stream = ws.stream();
        self.spawn_stream_task(stream)
    }

    fn spawn_business_stream(&mut self) -> anyhow::Result<()> {
        if self.ws_business.is_none() {
            return Ok(());
        }

        let ws = self.business_ws_mut()?;
        let stream = ws.stream();
        self.spawn_stream_task(stream)
    }

    fn spawn_stream_task(
        &mut self,
        stream: impl futures_util::Stream<Item = NautilusWsMessage> + Send + 'static,
    ) -> anyhow::Result<()> {
        let data_sender = self.data_sender.clone();
        let instruments = self.instruments.clone();
        let cancellation = self.cancellation_token.clone();

        let handle = tokio::spawn(async move {
            tokio::pin!(stream);

            loop {
                tokio::select! {
                    maybe_msg = stream.next() => {
                        match maybe_msg {
                            Some(msg) => {
                                Self::handle_ws_message(msg, &data_sender, &instruments);
                            }
                            None => {
                                tracing::debug!("Websocket stream ended");
                                break;
                            }
                        }
                    }
                    _ = cancellation.cancelled() => {
                        tracing::debug!("Websocket stream task cancelled");
                        break;
                    }
                }
            }
        });

        self.tasks.push(handle);
        Ok(())
    }

    fn maybe_spawn_instrument_refresh(&mut self) -> anyhow::Result<()> {
        let Some(minutes) = self.config.update_instruments_interval_mins else {
            return Ok(());
        };

        if minutes == 0 || self.instrument_refresh_active {
            return Ok(());
        }

        let interval_secs = minutes.saturating_mul(60);
        if interval_secs == 0 {
            return Ok(());
        }

        let interval = Duration::from_secs(interval_secs);
        let cancellation = self.cancellation_token.clone();
        let instruments_cache = Arc::clone(&self.instruments);
        let mut http_client = self.http_client.clone();
        let config = self.config.clone();
        let client_id = self.client_id;

        let handle = tokio::spawn(async move {
            loop {
                let sleep = tokio::time::sleep(interval);
                tokio::pin!(sleep);
                tokio::select! {
                    _ = cancellation.cancelled() => {
                        tracing::debug!("OKX instrument refresh task cancelled");
                        break;
                    }
                    _ = &mut sleep => {
                        let instrument_types = if config.instrument_types.is_empty() {
                            vec![OKXInstrumentType::Spot]
                        } else {
                            config.instrument_types.clone()
                        };

                        let mut collected: Vec<InstrumentAny> = Vec::new();

                        for inst_type in instrument_types {
                            match http_client.request_instruments(inst_type, None).await {
                                Ok(mut instruments) => {
                                    instruments.retain(|instrument| contract_filter_with_config(&config, instrument));
                                    collected.extend(instruments);
                                }
                                Err(e) => {
                                    tracing::warn!(client_id=%client_id, instrument_type=?inst_type, error=?e, "Failed to refresh OKX instruments for type");
                                }
                            }
                        }

                        if collected.is_empty() {
                            tracing::debug!(client_id=%client_id, "OKX instrument refresh yielded no instruments");
                            continue;
                        }

                        http_client.add_instruments(collected.clone());

                        {
                            let mut guard = instruments_cache
                                .write()
                                .expect("instrument cache lock poisoned");
                            guard.clear();
                            for instrument in &collected {
                                guard.insert(instrument.id(), instrument.clone());
                            }
                        }

                        tracing::debug!(client_id=%client_id, count=collected.len(), "OKX instruments refreshed");
                    }
                }
            }
        });

        self.tasks.push(handle);
        self.instrument_refresh_active = true;
        Ok(())
    }
}

fn emit_funding_rates(updates: Vec<FundingRateUpdate>) {
    if updates.is_empty() {
        return;
    }

    for update in updates {
        tracing::debug!(
            "Received funding rate update for {} but forwarding is not yet supported",
            update.instrument_id
        );
    }
}

fn upsert_instrument(
    cache: &Arc<RwLock<AHashMap<InstrumentId, InstrumentAny>>>,
    instrument: InstrumentAny,
) {
    let mut guard = cache.write().expect(MUTEX_POISONED);
    guard.insert(instrument.id(), instrument);
}

fn datetime_to_unix_nanos(value: Option<DateTime<Utc>>) -> Option<UnixNanos> {
    value
        .and_then(|dt| dt.timestamp_nanos_opt())
        .and_then(|nanos| u64::try_from(nanos).ok())
        .map(UnixNanos::from)
}

fn contract_filter_with_config(config: &OKXDataClientConfig, instrument: &InstrumentAny) -> bool {
    match config.contract_types.as_ref() {
        None => true,
        Some(filter) if filter.is_empty() => true,
        Some(filter) => {
            let is_inverse = instrument.is_inverse();
            (is_inverse && filter.contains(&OKXContractType::Inverse))
                || (!is_inverse && filter.contains(&OKXContractType::Linear))
        }
    }
}

#[async_trait::async_trait]
impl DataClient for OKXDataClient {
    fn client_id(&self) -> ClientId {
        self.client_id
    }

    fn venue(&self) -> Option<Venue> {
        Some(self.venue())
    }

    fn start(&mut self) -> anyhow::Result<()> {
        tracing::info!(
            client_id = %self.client_id,
            vip_level = ?self.vip_level(),
            instrument_types = ?self.config.instrument_types,
            is_demo = self.config.is_demo,
            "Starting OKX data client"
        );
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        tracing::info!("Stopping OKX data client {id}", id = self.client_id);
        self.cancellation_token.cancel();
        self.is_connected.store(false, Ordering::Relaxed);
        self.instrument_refresh_active = false;
        Ok(())
    }

    fn reset(&mut self) -> anyhow::Result<()> {
        tracing::debug!("Resetting OKX data client {id}", id = self.client_id);
        self.is_connected.store(false, Ordering::Relaxed);
        self.cancellation_token = CancellationToken::new();
        self.tasks.clear();
        self.book_channels
            .write()
            .expect("book channel cache lock poisoned")
            .clear();
        self.instrument_refresh_active = false;
        Ok(())
    }

    fn dispose(&mut self) -> anyhow::Result<()> {
        tracing::debug!("Disposing OKX data client {id}", id = self.client_id);
        self.stop()
    }

    async fn connect(&mut self) -> anyhow::Result<()> {
        if self.is_connected() {
            return Ok(());
        }

        self.bootstrap_instruments().await?;

        {
            let ws_public = self.public_ws_mut()?;
            ws_public
                .connect()
                .await
                .context("failed to connect OKX public websocket")?;
            ws_public
                .wait_until_active(10.0)
                .await
                .context("public websocket did not become active")?;
        }

        let instrument_types = if self.config.instrument_types.is_empty() {
            vec![OKXInstrumentType::Spot]
        } else {
            self.config.instrument_types.clone()
        };

        let public_clone = self.public_ws()?.clone();
        self.spawn_ws(
            async move {
                for inst_type in instrument_types {
                    public_clone
                        .subscribe_instruments(inst_type)
                        .await
                        .with_context(|| {
                            format!("failed to subscribe to instrument type {inst_type:?}")
                        })?;
                }
                Ok(())
            },
            "instrument subscription",
        );

        self.spawn_public_stream()?;

        if self.ws_business.is_some() {
            {
                let ws_business = self.business_ws_mut()?;
                ws_business
                    .connect()
                    .await
                    .context("failed to connect OKX business websocket")?;
                ws_business
                    .wait_until_active(10.0)
                    .await
                    .context("business websocket did not become active")?;
            }
            self.spawn_business_stream()?;
        }

        self.maybe_spawn_instrument_refresh()?;

        self.is_connected.store(true, Ordering::Relaxed);
        tracing::info!("OKX data client connected");
        Ok(())
    }

    async fn disconnect(&mut self) -> anyhow::Result<()> {
        if self.is_disconnected() {
            return Ok(());
        }

        self.cancellation_token.cancel();

        if let Some(ws) = self.ws_public.as_ref()
            && let Err(e) = ws.unsubscribe_all().await
        {
            tracing::warn!("Failed to unsubscribe all from public websocket: {e:?}");
        }
        if let Some(ws) = self.ws_business.as_ref()
            && let Err(e) = ws.unsubscribe_all().await
        {
            tracing::warn!("Failed to unsubscribe all from business websocket: {e:?}");
        }

        // Brief delay to allow unsubscribe confirmations to be processed
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        if let Some(ws) = self.ws_public.as_mut() {
            let _ = ws.close().await;
        }
        if let Some(ws) = self.ws_business.as_mut() {
            let _ = ws.close().await;
        }

        for handle in self.tasks.drain(..) {
            if let Err(e) = handle.await {
                tracing::error!("Error joining websocket task: {e}");
            }
        }

        self.cancellation_token = CancellationToken::new();
        self.is_connected.store(false, Ordering::Relaxed);
        self.book_channels
            .write()
            .expect("book channel cache lock poisoned")
            .clear();
        self.instrument_refresh_active = false;
        tracing::info!("OKX data client disconnected");
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
            anyhow::bail!("OKX only supports L2_MBP order book deltas");
        }

        let depth = cmd.depth.map_or(0, |d| d.get());
        if !matches!(depth, 0 | 50 | 400) {
            anyhow::bail!("invalid depth {depth}; valid values are 50 or 400");
        }

        let vip = self.vip_level().unwrap_or(OKXVipLevel::Vip0);
        let channel = match depth {
            50 => {
                if vip < OKXVipLevel::Vip4 {
                    anyhow::bail!(
                        "VIP level {vip} insufficient for 50 depth subscription (requires VIP4)"
                    );
                }
                OKXBookChannel::Books50L2Tbt
            }
            0 | 400 => {
                if vip >= OKXVipLevel::Vip5 {
                    OKXBookChannel::BookL2Tbt
                } else {
                    OKXBookChannel::Book
                }
            }
            _ => unreachable!(),
        };

        let instrument_id = cmd.instrument_id;
        let ws = self.public_ws()?.clone();
        let book_channels = Arc::clone(&self.book_channels);
        self.spawn_ws(
            async move {
                match channel {
                    OKXBookChannel::Books50L2Tbt => ws
                        .subscribe_book50_l2_tbt(instrument_id)
                        .await
                        .context("books50-l2-tbt subscription")?,
                    OKXBookChannel::BookL2Tbt => ws
                        .subscribe_book_l2_tbt(instrument_id)
                        .await
                        .context("books-l2-tbt subscription")?,
                    OKXBookChannel::Book => ws
                        .subscribe_books_channel(instrument_id)
                        .await
                        .context("books subscription")?,
                }
                book_channels
                    .write()
                    .expect("book channel cache lock poisoned")
                    .insert(instrument_id, channel);
                Ok(())
            },
            "order book delta subscription",
        );

        Ok(())
    }

    fn subscribe_book_snapshots(&mut self, cmd: &SubscribeBookSnapshots) -> anyhow::Result<()> {
        if cmd.book_type != BookType::L2_MBP {
            anyhow::bail!("OKX only supports L2_MBP order book snapshots");
        }
        let depth = cmd.depth.map_or(5, |d| d.get());
        if depth != 5 {
            anyhow::bail!("OKX only supports depth=5 snapshots");
        }

        let ws = self.public_ws()?.clone();
        let instrument_id = cmd.instrument_id;
        self.spawn_ws(
            async move {
                ws.subscribe_book_depth5(instrument_id)
                    .await
                    .context("books5 subscription")
            },
            "order book snapshot subscription",
        );
        Ok(())
    }

    fn subscribe_quotes(&mut self, cmd: &SubscribeQuotes) -> anyhow::Result<()> {
        let ws = self.public_ws()?.clone();
        let instrument_id = cmd.instrument_id;
        self.spawn_ws(
            async move {
                ws.subscribe_quotes(instrument_id)
                    .await
                    .context("quotes subscription")
            },
            "quote subscription",
        );
        Ok(())
    }

    fn subscribe_trades(&mut self, cmd: &SubscribeTrades) -> anyhow::Result<()> {
        let ws = self.public_ws()?.clone();
        let instrument_id = cmd.instrument_id;
        self.spawn_ws(
            async move {
                ws.subscribe_trades(instrument_id, false)
                    .await
                    .context("trades subscription")
            },
            "trade subscription",
        );
        Ok(())
    }

    fn subscribe_mark_prices(&mut self, cmd: &SubscribeMarkPrices) -> anyhow::Result<()> {
        let ws = self.public_ws()?.clone();
        let instrument_id = cmd.instrument_id;
        self.spawn_ws(
            async move {
                ws.subscribe_mark_prices(instrument_id)
                    .await
                    .context("mark price subscription")
            },
            "mark price subscription",
        );
        Ok(())
    }

    fn subscribe_index_prices(&mut self, cmd: &SubscribeIndexPrices) -> anyhow::Result<()> {
        let ws = self.public_ws()?.clone();
        let instrument_id = cmd.instrument_id;
        self.spawn_ws(
            async move {
                ws.subscribe_index_prices(instrument_id)
                    .await
                    .context("index price subscription")
            },
            "index price subscription",
        );
        Ok(())
    }

    fn subscribe_funding_rates(&mut self, cmd: &SubscribeFundingRates) -> anyhow::Result<()> {
        let ws = self.public_ws()?.clone();
        let instrument_id = cmd.instrument_id;
        self.spawn_ws(
            async move {
                ws.subscribe_funding_rates(instrument_id)
                    .await
                    .context("funding rate subscription")
            },
            "funding rate subscription",
        );
        Ok(())
    }

    fn subscribe_bars(&mut self, cmd: &SubscribeBars) -> anyhow::Result<()> {
        let ws = self.business_ws()?.clone();
        let bar_type = cmd.bar_type;
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
        let ws = self.public_ws()?.clone();
        let instrument_id = cmd.instrument_id;
        let channel = self
            .book_channels
            .write()
            .expect("book channel cache lock poisoned")
            .remove(&instrument_id);
        self.spawn_ws(
            async move {
                match channel {
                    Some(OKXBookChannel::Books50L2Tbt) => ws
                        .unsubscribe_book50_l2_tbt(instrument_id)
                        .await
                        .context("books50-l2-tbt unsubscribe")?,
                    Some(OKXBookChannel::BookL2Tbt) => ws
                        .unsubscribe_book_l2_tbt(instrument_id)
                        .await
                        .context("books-l2-tbt unsubscribe")?,
                    Some(OKXBookChannel::Book) => ws
                        .unsubscribe_book(instrument_id)
                        .await
                        .context("book unsubscribe")?,
                    None => {
                        tracing::warn!(
                            "Book channel not found for {instrument_id}; unsubscribing fallback channel"
                        );
                        ws.unsubscribe_book(instrument_id)
                            .await
                            .context("book fallback unsubscribe")?;
                    }
                }
                Ok(())
            },
            "order book unsubscribe",
        );
        Ok(())
    }

    fn unsubscribe_book_snapshots(&mut self, cmd: &UnsubscribeBookSnapshots) -> anyhow::Result<()> {
        let ws = self.public_ws()?.clone();
        let instrument_id = cmd.instrument_id;
        self.spawn_ws(
            async move {
                ws.unsubscribe_book_depth5(instrument_id)
                    .await
                    .context("book depth5 unsubscribe")
            },
            "order book snapshot unsubscribe",
        );
        Ok(())
    }

    fn unsubscribe_quotes(&mut self, cmd: &UnsubscribeQuotes) -> anyhow::Result<()> {
        let ws = self.public_ws()?.clone();
        let instrument_id = cmd.instrument_id;
        self.spawn_ws(
            async move {
                ws.unsubscribe_quotes(instrument_id)
                    .await
                    .context("quotes unsubscribe")
            },
            "quote unsubscribe",
        );
        Ok(())
    }

    fn unsubscribe_trades(&mut self, cmd: &UnsubscribeTrades) -> anyhow::Result<()> {
        let ws = self.public_ws()?.clone();
        let instrument_id = cmd.instrument_id;
        self.spawn_ws(
            async move {
                ws.unsubscribe_trades(instrument_id, false) // TODO: Aggregated trades?
                    .await
                    .context("trades unsubscribe")
            },
            "trade unsubscribe",
        );
        Ok(())
    }

    fn unsubscribe_mark_prices(&mut self, cmd: &UnsubscribeMarkPrices) -> anyhow::Result<()> {
        let ws = self.public_ws()?.clone();
        let instrument_id = cmd.instrument_id;
        self.spawn_ws(
            async move {
                ws.unsubscribe_mark_prices(instrument_id)
                    .await
                    .context("mark price unsubscribe")
            },
            "mark price unsubscribe",
        );
        Ok(())
    }

    fn unsubscribe_index_prices(&mut self, cmd: &UnsubscribeIndexPrices) -> anyhow::Result<()> {
        let ws = self.public_ws()?.clone();
        let instrument_id = cmd.instrument_id;
        self.spawn_ws(
            async move {
                ws.unsubscribe_index_prices(instrument_id)
                    .await
                    .context("index price unsubscribe")
            },
            "index price unsubscribe",
        );
        Ok(())
    }

    fn unsubscribe_funding_rates(&mut self, cmd: &UnsubscribeFundingRates) -> anyhow::Result<()> {
        let ws = self.public_ws()?.clone();
        let instrument_id = cmd.instrument_id;
        self.spawn_ws(
            async move {
                ws.unsubscribe_funding_rates(instrument_id)
                    .await
                    .context("funding rate unsubscribe")
            },
            "funding rate unsubscribe",
        );
        Ok(())
    }

    fn unsubscribe_bars(&mut self, cmd: &UnsubscribeBars) -> anyhow::Result<()> {
        let ws = self.business_ws()?.clone();
        let bar_type = cmd.bar_type;
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

    fn request_instruments(&self, request: &RequestInstruments) -> anyhow::Result<()> {
        let instruments = {
            let guard = self
                .instruments
                .read()
                .expect("instrument cache lock poisoned");
            guard.values().cloned().collect::<Vec<_>>()
        };

        let response = DataResponse::Instruments(InstrumentsResponse::new(
            request.request_id,
            request.client_id.unwrap_or(self.client_id),
            self.venue(),
            instruments,
            datetime_to_unix_nanos(request.start),
            datetime_to_unix_nanos(request.end),
            self.clock.get_time_ns(),
            request.params.clone(),
        ));

        if let Err(e) = self.data_sender.send(DataEvent::Response(response)) {
            tracing::error!("Failed to send instruments response: {e}");
        }

        Ok(())
    }

    fn request_instrument(&self, request: &RequestInstrument) -> anyhow::Result<()> {
        let instrument = {
            let guard = self
                .instruments
                .read()
                .expect("instrument cache lock poisoned");
            guard
                .get(&request.instrument_id)
                .cloned()
                .context("instrument not found in cache")?
        };

        let response = DataResponse::Instrument(Box::new(InstrumentResponse::new(
            request.request_id,
            request.client_id.unwrap_or(self.client_id),
            instrument.id(),
            instrument,
            datetime_to_unix_nanos(request.start),
            datetime_to_unix_nanos(request.end),
            self.clock.get_time_ns(),
            request.params.clone(),
        )));

        if let Err(e) = self.data_sender.send(DataEvent::Response(response)) {
            tracing::error!("Failed to send instrument response: {e}");
        }

        Ok(())
    }

    fn request_trades(&self, request: &RequestTrades) -> anyhow::Result<()> {
        let http = self.http_client.clone();
        let sender = self.data_sender.clone();
        let instrument_id = request.instrument_id;
        let start = request.start;
        let end = request.end;
        let limit = request.limit.map(|n| n.get() as u32);
        let request_id = request.request_id;
        let client_id = request.client_id.unwrap_or(self.client_id);
        let params = request.params.clone();
        let clock = self.clock;
        let start_nanos = datetime_to_unix_nanos(start);
        let end_nanos = datetime_to_unix_nanos(end);

        tokio::spawn(async move {
            match http
                .request_trades(instrument_id, start, end, limit)
                .await
                .context("failed to request trades from OKX")
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
                        tracing::error!("Failed to send trades response: {e}");
                    }
                }
                Err(e) => tracing::error!("Trade request failed: {e:?}"),
            }
        });

        Ok(())
    }

    fn request_bars(&self, request: &RequestBars) -> anyhow::Result<()> {
        let http = self.http_client.clone();
        let sender = self.data_sender.clone();
        let bar_type = request.bar_type;
        let start = request.start;
        let end = request.end;
        let limit = request.limit.map(|n| n.get() as u32);
        let request_id = request.request_id;
        let client_id = request.client_id.unwrap_or(self.client_id);
        let params = request.params.clone();
        let clock = self.clock;
        let start_nanos = datetime_to_unix_nanos(start);
        let end_nanos = datetime_to_unix_nanos(end);

        tokio::spawn(async move {
            match http
                .request_bars(bar_type, start, end, limit)
                .await
                .context("failed to request bars from OKX")
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
                        tracing::error!("Failed to send bars response: {e}");
                    }
                }
                Err(e) => tracing::error!("Bar request failed: {e:?}"),
            }
        });

        Ok(())
    }
}
