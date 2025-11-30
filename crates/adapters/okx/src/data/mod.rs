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

use std::sync::{
    Arc, RwLock,
    atomic::{AtomicBool, Ordering},
};

use ahash::AHashMap;
use anyhow::Context;
use chrono::{DateTime, Utc};
use futures_util::{StreamExt, pin_mut};
use nautilus_common::{
    live::runner::get_data_event_sender,
    messages::{
        DataEvent,
        data::{
            BarsResponse, DataResponse, InstrumentResponse, InstrumentsResponse, RequestBars,
            RequestInstrument, RequestInstruments, RequestTrades, SubscribeBars,
            SubscribeBookDeltas, SubscribeBookSnapshots, SubscribeFundingRates,
            SubscribeIndexPrices, SubscribeInstrument, SubscribeInstruments, SubscribeMarkPrices,
            SubscribeQuotes, SubscribeTrades, TradesResponse, UnsubscribeBars,
            UnsubscribeBookDeltas, UnsubscribeBookSnapshots, UnsubscribeFundingRates,
            UnsubscribeIndexPrices, UnsubscribeMarkPrices, UnsubscribeQuotes, UnsubscribeTrades,
        },
    },
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
        parse::okx_instrument_type_from_symbol,
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
                config.http_proxy_url.clone(),
            )?
        } else {
            OKXHttpClient::new(
                config.base_url_http.clone(),
                config.http_timeout_secs,
                config.max_retries,
                config.retry_delay_initial_ms,
                config.retry_delay_max_ms,
                config.is_demo,
                config.http_proxy_url.clone(),
            )?
        };

        let ws_public = OKXWebSocketClient::new(
            Some(config.ws_public_url()),
            None,
            None,
            None,
            None,
            Some(20), // Heartbeat
        )
        .context("failed to construct OKX public websocket client")?;

        let ws_business = if config.requires_business_ws() {
            Some(
                OKXWebSocketClient::new(
                    Some(config.ws_business_url()),
                    config.api_key.clone(),
                    config.api_secret.clone(),
                    config.api_passphrase.clone(),
                    None,
                    Some(20), // Heartbeat
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

    fn business_ws(&self) -> anyhow::Result<&OKXWebSocketClient> {
        self.ws_business
            .as_ref()
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
            | NautilusWsMessage::PositionUpdate(_)
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
            NautilusWsMessage::Authenticated => {
                tracing::debug!("Websocket authenticated");
            }
        }
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
    contract_filter_with_config_types(config.contract_types.as_ref(), instrument)
}

fn contract_filter_with_config_types(
    contract_types: Option<&Vec<OKXContractType>>,
    instrument: &InstrumentAny,
) -> bool {
    match contract_types {
        None => true,
        Some(filter) if filter.is_empty() => true,
        Some(filter) => {
            let is_inverse = instrument.is_inverse();
            (is_inverse && filter.contains(&OKXContractType::Inverse))
                || (!is_inverse && filter.contains(&OKXContractType::Linear))
        }
    }
}

#[async_trait::async_trait(?Send)]
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
            http_proxy_url = ?self.config.http_proxy_url,
            ws_proxy_url = ?self.config.ws_proxy_url,
            "Starting OKX data client"
        );
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        tracing::info!("Stopping OKX data client {id}", id = self.client_id);
        self.cancellation_token.cancel();
        self.is_connected.store(false, Ordering::Relaxed);
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

        let instrument_types = if self.config.instrument_types.is_empty() {
            vec![OKXInstrumentType::Spot]
        } else {
            self.config.instrument_types.clone()
        };

        let mut all_instruments = Vec::new();
        for inst_type in &instrument_types {
            let mut fetched = self
                .http_client
                .request_instruments(*inst_type, None)
                .await
                .with_context(|| format!("failed to request OKX instruments for {inst_type:?}"))?;

            fetched.retain(|instrument| contract_filter_with_config(&self.config, instrument));
            self.http_client.cache_instruments(fetched.clone());

            let mut guard = self.instruments.write().expect(MUTEX_POISONED);
            for instrument in &fetched {
                guard.insert(instrument.id(), instrument.clone());
            }
            drop(guard);

            all_instruments.extend(fetched);
        }

        for instrument in all_instruments {
            if let Err(e) = self.data_sender.send(DataEvent::Instrument(instrument)) {
                tracing::warn!("Failed to send instrument: {e}");
            }
        }

        if let Some(ref mut ws) = self.ws_public {
            // Cache instruments to websocket before connecting so handler has them
            let instruments: Vec<_> = self
                .instruments
                .read()
                .expect(MUTEX_POISONED)
                .values()
                .cloned()
                .collect();
            ws.cache_instruments(instruments);

            ws.connect()
                .await
                .context("failed to connect OKX public websocket")?;
            ws.wait_until_active(10.0)
                .await
                .context("public websocket did not become active")?;

            let stream = ws.stream();
            let sender = self.data_sender.clone();
            let insts = self.instruments.clone();
            let cancel = self.cancellation_token.clone();
            let handle = tokio::spawn(async move {
                pin_mut!(stream);
                loop {
                    tokio::select! {
                        Some(message) = stream.next() => {
                            Self::handle_ws_message(message, &sender, &insts);
                        }
                        _ = cancel.cancelled() => {
                            tracing::debug!("Public websocket stream task cancelled");
                            break;
                        }
                    }
                }
            });
            self.tasks.push(handle);

            for inst_type in &instrument_types {
                ws.subscribe_instruments(*inst_type)
                    .await
                    .with_context(|| {
                        format!("failed to subscribe to instrument type {inst_type:?}")
                    })?;
            }
        }

        if let Some(ref mut ws) = self.ws_business {
            // Cache instruments to websocket before connecting so handler has them
            let instruments: Vec<_> = self
                .instruments
                .read()
                .expect(MUTEX_POISONED)
                .values()
                .cloned()
                .collect();
            ws.cache_instruments(instruments);

            ws.connect()
                .await
                .context("failed to connect OKX business websocket")?;
            ws.wait_until_active(10.0)
                .await
                .context("business websocket did not become active")?;

            let stream = ws.stream();
            let sender = self.data_sender.clone();
            let insts = self.instruments.clone();
            let cancel = self.cancellation_token.clone();
            let handle = tokio::spawn(async move {
                pin_mut!(stream);
                loop {
                    tokio::select! {
                        Some(message) = stream.next() => {
                            Self::handle_ws_message(message, &sender, &insts);
                        }
                        _ = cancel.cancelled() => {
                            tracing::debug!("Business websocket stream task cancelled");
                            break;
                        }
                    }
                }
            });
            self.tasks.push(handle);
        }

        self.is_connected.store(true, Ordering::Release);
        tracing::info!(client_id = %self.client_id, "Connected");
        Ok(())
    }

    async fn disconnect(&mut self) -> anyhow::Result<()> {
        if self.is_disconnected() {
            return Ok(());
        }

        self.cancellation_token.cancel();

        if let Some(ref ws) = self.ws_public
            && let Err(e) = ws.unsubscribe_all().await
        {
            tracing::warn!("Failed to unsubscribe all from public websocket: {e:?}");
        }
        if let Some(ref ws) = self.ws_business
            && let Err(e) = ws.unsubscribe_all().await
        {
            tracing::warn!("Failed to unsubscribe all from business websocket: {e:?}");
        }

        // Allow time for unsubscribe confirmations
        tokio::time::sleep(Duration::from_millis(500)).await;

        if let Some(ref mut ws) = self.ws_public {
            let _ = ws.close().await;
        }
        if let Some(ref mut ws) = self.ws_business {
            let _ = ws.close().await;
        }

        let handles: Vec<_> = self.tasks.drain(..).collect();
        for handle in handles {
            if let Err(e) = handle.await {
                tracing::error!("Error joining websocket task: {e}");
            }
        }

        self.book_channels.write().expect(MUTEX_POISONED).clear();
        self.is_connected.store(false, Ordering::Release);
        tracing::info!(client_id = %self.client_id, "Disconnected");
        Ok(())
    }

    fn is_connected(&self) -> bool {
        self.is_connected.load(Ordering::Relaxed)
    }

    fn is_disconnected(&self) -> bool {
        !self.is_connected()
    }

    fn subscribe_instruments(&mut self, _cmd: &SubscribeInstruments) -> anyhow::Result<()> {
        for inst_type in &self.config.instrument_types {
            let ws = self.public_ws()?.clone();
            let inst_type = *inst_type;

            self.spawn_ws(
                async move {
                    ws.subscribe_instruments(inst_type)
                        .await
                        .context("instruments subscription")?;
                    Ok(())
                },
                "subscribe_instruments",
            );
        }
        Ok(())
    }

    fn subscribe_instrument(&mut self, cmd: &SubscribeInstrument) -> anyhow::Result<()> {
        // OKX instruments channel doesn't support subscribing to individual instruments via instId
        // Instead, subscribe to the instrument type if not already subscribed
        let instrument_id = cmd.instrument_id;
        let ws = self.public_ws()?.clone();

        self.spawn_ws(
            async move {
                ws.subscribe_instrument(instrument_id)
                    .await
                    .context("instrument type subscription")?;
                Ok(())
            },
            "subscribe_instrument",
        );
        Ok(())
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
        let http = self.http_client.clone();
        let sender = self.data_sender.clone();
        let instruments_cache = self.instruments.clone();
        let request_id = request.request_id;
        let client_id = request.client_id.unwrap_or(self.client_id);
        let venue = self.venue();
        let start = request.start;
        let end = request.end;
        let params = request.params.clone();
        let clock = self.clock;
        let start_nanos = datetime_to_unix_nanos(start);
        let end_nanos = datetime_to_unix_nanos(end);
        let instrument_types = if self.config.instrument_types.is_empty() {
            vec![OKXInstrumentType::Spot]
        } else {
            self.config.instrument_types.clone()
        };
        let contract_types = self.config.contract_types.clone();
        let instrument_families = self.config.instrument_families.clone();

        tokio::spawn(async move {
            let mut all_instruments = Vec::new();

            for inst_type in instrument_types {
                let supports_family = matches!(
                    inst_type,
                    OKXInstrumentType::Futures
                        | OKXInstrumentType::Swap
                        | OKXInstrumentType::Option
                );

                let families = match (&instrument_families, inst_type, supports_family) {
                    (Some(families), OKXInstrumentType::Option, true) => families.clone(),
                    (Some(families), _, true) => families.clone(),
                    (None, OKXInstrumentType::Option, _) => {
                        tracing::warn!(
                            "Skipping OPTION type: instrument_families required but not configured"
                        );
                        continue;
                    }
                    _ => vec![],
                };

                if families.is_empty() {
                    match http.request_instruments(inst_type, None).await {
                        Ok(instruments) => {
                            for instrument in instruments {
                                if !contract_filter_with_config_types(
                                    contract_types.as_ref(),
                                    &instrument,
                                ) {
                                    continue;
                                }

                                upsert_instrument(&instruments_cache, instrument.clone());
                                all_instruments.push(instrument);
                            }
                        }
                        Err(e) => {
                            tracing::error!("Failed to fetch instruments for {inst_type:?}: {e:?}");
                        }
                    }
                } else {
                    for family in families {
                        match http
                            .request_instruments(inst_type, Some(family.clone()))
                            .await
                        {
                            Ok(instruments) => {
                                for instrument in instruments {
                                    if !contract_filter_with_config_types(
                                        contract_types.as_ref(),
                                        &instrument,
                                    ) {
                                        continue;
                                    }

                                    upsert_instrument(&instruments_cache, instrument.clone());
                                    all_instruments.push(instrument);
                                }
                            }
                            Err(e) => {
                                tracing::error!(
                                    "Failed to fetch instruments for {inst_type:?} family {family}: {e:?}"
                                );
                            }
                        }
                    }
                }
            }

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
                tracing::error!("Failed to send instruments response: {e}");
            }
        });

        Ok(())
    }

    fn request_instrument(&self, request: &RequestInstrument) -> anyhow::Result<()> {
        let http = self.http_client.clone();
        let sender = self.data_sender.clone();
        let instruments = self.instruments.clone();
        let instrument_id = request.instrument_id;
        let request_id = request.request_id;
        let client_id = request.client_id.unwrap_or(self.client_id);
        let start = request.start;
        let end = request.end;
        let params = request.params.clone();
        let clock = self.clock;
        let start_nanos = datetime_to_unix_nanos(start);
        let end_nanos = datetime_to_unix_nanos(end);
        let instrument_types = if self.config.instrument_types.is_empty() {
            vec![OKXInstrumentType::Spot]
        } else {
            self.config.instrument_types.clone()
        };
        let contract_types = self.config.contract_types.clone();

        tokio::spawn(async move {
            match http
                .request_instrument(instrument_id)
                .await
                .context("fetch instrument from API")
            {
                Ok(instrument) => {
                    let inst_id = instrument.id();
                    let symbol = inst_id.symbol.as_str();
                    let inst_type = okx_instrument_type_from_symbol(symbol);
                    if !instrument_types.contains(&inst_type) {
                        tracing::error!(
                            "Instrument {instrument_id} type {inst_type:?} not in configured types {instrument_types:?}"
                        );
                        return;
                    }

                    if !contract_filter_with_config_types(contract_types.as_ref(), &instrument) {
                        tracing::error!(
                            "Instrument {instrument_id} filtered out by contract_types config"
                        );
                        return;
                    }

                    upsert_instrument(&instruments, instrument.clone());

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
                        tracing::error!("Failed to send instrument response: {e}");
                    }
                }
                Err(e) => tracing::error!("Instrument request failed: {e:?}"),
            }
        });

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
