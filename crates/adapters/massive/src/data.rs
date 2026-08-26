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

//! Massive data client for NautilusTrader.
//!
//! Implements the [`DataClient`] trait, providing US equity market data
//! subscriptions (trades, quotes, second/minute bars) and historical data
//! requests through the Massive REST and WebSocket APIs.

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use anyhow::Context;
use nautilus_common::{
    clients::DataClient,
    live::{runner::get_data_event_sender, runtime::get_runtime},
    messages::{
        DataEvent,
        data::{
            BarsResponse, DataResponse, InstrumentResponse, InstrumentsResponse, QuotesResponse,
            RequestBars, RequestInstrument, RequestInstruments, RequestQuotes, RequestTrades,
            SubscribeBars, SubscribeInstrument, SubscribeQuotes, SubscribeTrades, TradesResponse,
            UnsubscribeBars, UnsubscribeInstrument, UnsubscribeQuotes, UnsubscribeTrades,
        },
    },
};
use nautilus_core::{
    AtomicMap,
    datetime::datetime_to_unix_nanos,
    time::{AtomicTime, get_atomic_clock_realtime},
};
use nautilus_live::SocketControl;
use nautilus_model::{
    data::{Data, bar::BarSpecification},
    enums::{BarAggregation, PriceType},
    identifiers::{ClientId, InstrumentId, Venue},
    instruments::{Instrument, InstrumentAny},
};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use ustr::Ustr;

use crate::{
    common::{consts::MASSIVE_VENUE, credential::MassiveCredential, enums::MassiveWsChannel},
    config::MassiveDataClientConfig,
    http::client::MassiveHttpClient,
    provider::MassiveInstrumentProvider,
    websocket::{client::MassiveWebSocketClient, handler::NautilusWsMessage},
};

/// Data client for Massive US equity market data.
///
/// Owns an HTTP client, WebSocket client, and instrument provider. Bootstraps
/// instruments on connect, subscribes to WS channels for live data, and
/// handles historical data requests through the REST API.
#[derive(Debug)]
pub struct MassiveDataClient {
    client_id: ClientId,
    config: MassiveDataClientConfig,
    http_client: MassiveHttpClient,
    ws_client: MassiveWebSocketClient,
    provider: MassiveInstrumentProvider,
    is_connected: AtomicBool,
    cancellation_token: CancellationToken,
    tasks: Vec<JoinHandle<()>>,
    data_sender: tokio::sync::mpsc::UnboundedSender<DataEvent>,
    instruments: Arc<AtomicMap<InstrumentId, InstrumentAny>>,
    clock: &'static AtomicTime,
}

impl MassiveDataClient {
    /// Creates a new [`MassiveDataClient`] instance.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP client fails to initialize.
    pub fn new(client_id: ClientId, config: MassiveDataClientConfig) -> anyhow::Result<Self> {
        let clock = get_atomic_clock_realtime();
        let data_sender = get_data_event_sender();

        let credential = MassiveCredential::resolve(config.api_key.as_deref());
        if credential.is_none() {
            log::warn!("No Massive API key configured (set `MASSIVE_API_KEY`); requests will fail");
        }

        let http_client = MassiveHttpClient::new(
            credential.clone(),
            config.base_url_rest.clone(),
            config.http_timeout_secs,
            None,
            None,
        )
        .map_err(|e| anyhow::anyhow!("Failed to create HTTP client: {e}"))?;

        let ws_url = config.ws_url();
        let ws_client = MassiveWebSocketClient::new(
            &ws_url,
            credential,
            config.bars_timestamp_on_close,
            config.transport_backend,
            None,
        )
        .with_socket_control(SocketControl::new(
            client_id,
            Some(*MASSIVE_VENUE),
            "massive-data-streams",
        ));

        let provider = MassiveInstrumentProvider::new(http_client.clone(), config.symbols.clone());

        Ok(Self {
            client_id,
            config,
            http_client,
            ws_client,
            provider,
            is_connected: AtomicBool::new(false),
            cancellation_token: CancellationToken::new(),
            tasks: Vec::new(),
            data_sender,
            instruments: Arc::new(AtomicMap::new()),
            clock,
        })
    }

    fn venue(&self) -> Venue {
        *MASSIVE_VENUE
    }

    async fn bootstrap_instruments(&self) -> anyhow::Result<Vec<InstrumentAny>> {
        let instruments = self
            .provider
            .load_all()
            .await
            .context("failed to fetch instruments during bootstrap")?;

        self.instruments.rcu(|m| {
            for instrument in &instruments {
                m.insert(instrument.id(), instrument.clone());
            }
        });

        log::debug!("Bootstrapped {} instruments", instruments.len());
        Ok(instruments)
    }

    async fn spawn_ws(&mut self) -> anyhow::Result<()> {
        self.ws_client
            .connect()
            .await
            .context("failed to connect to Massive WebSocket")?;

        let mut out_rx = self
            .ws_client
            .take_out_rx()
            .ok_or_else(|| anyhow::anyhow!("WebSocket output receiver not available"))?;

        let data_sender = self.data_sender.clone();
        let cancellation_token = self.cancellation_token.clone();

        let task = get_runtime().spawn(async move {
            log::debug!("Massive WebSocket consumption loop started");

            loop {
                tokio::select! {
                    () = cancellation_token.cancelled() => {
                        log::debug!("WebSocket consumption loop cancelled");
                        break;
                    }
                    msg_opt = out_rx.recv() => {
                        match msg_opt {
                            Some(msg) => dispatch_ws_message(msg, &data_sender),
                            None => {
                                log::debug!("WebSocket output channel closed");
                                break;
                            }
                        }
                    }
                }
            }

            log::debug!("Massive WebSocket consumption loop finished");
        });

        self.tasks.push(task);
        Ok(())
    }

    fn ticker(instrument_id: InstrumentId) -> Ustr {
        instrument_id.symbol.inner()
    }
}

fn dispatch_ws_message(
    msg: NautilusWsMessage,
    data_sender: &tokio::sync::mpsc::UnboundedSender<DataEvent>,
) {
    match msg {
        NautilusWsMessage::Trade(trade) => {
            if let Err(e) = data_sender.send(DataEvent::Data(Data::Trade(trade))) {
                log::error!("Failed to send trade tick: {e}");
            }
        }
        NautilusWsMessage::Quote(quote) => {
            if let Err(e) = data_sender.send(DataEvent::Data(Data::Quote(quote))) {
                log::error!("Failed to send quote tick: {e}");
            }
        }
        NautilusWsMessage::Bar(bar) => {
            if let Err(e) = data_sender.send(DataEvent::Data(Data::Bar(bar))) {
                log::error!("Failed to send bar: {e}");
            }
        }
        NautilusWsMessage::Reconnected => {
            log::info!("WebSocket reconnected");
        }
        NautilusWsMessage::Error(e) => {
            log::warn!("WebSocket error: {e}");
        }
    }
}

/// Returns the live aggregate channel for a bar specification.
///
/// Massive streams fixed one-second (`A`) and one-minute (`AM`) windows.
///
/// # Errors
///
/// Returns an error for any other specification.
fn ws_channel_for_bar_spec(spec: &BarSpecification) -> anyhow::Result<MassiveWsChannel> {
    anyhow::ensure!(
        spec.price_type == PriceType::Last,
        "Massive only provides LAST price bars, was {}",
        spec.price_type
    );

    match (spec.step.get(), spec.aggregation) {
        (1, BarAggregation::Second) => Ok(MassiveWsChannel::AggregatesSecond),
        (1, BarAggregation::Minute) => Ok(MassiveWsChannel::AggregatesMinute),
        (step, aggregation) => anyhow::bail!(
            "Massive only streams 1-SECOND and 1-MINUTE bars, was {step}-{aggregation}"
        ),
    }
}

#[async_trait::async_trait(?Send)]
impl DataClient for MassiveDataClient {
    fn client_id(&self) -> ClientId {
        self.client_id
    }

    fn venue(&self) -> Option<Venue> {
        Some(Self::venue(self))
    }

    fn start(&mut self) -> anyhow::Result<()> {
        log::info!(
            "Starting Massive data client: client_id={}, feed={:?}",
            self.client_id,
            self.config.feed,
        );
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        log::info!("Stopping Massive data client {}", self.client_id);
        self.cancellation_token.cancel();
        self.is_connected.store(false, Ordering::Relaxed);
        Ok(())
    }

    fn reset(&mut self) -> anyhow::Result<()> {
        log::debug!("Resetting Massive data client {}", self.client_id);
        self.cancellation_token.cancel();
        self.is_connected.store(false, Ordering::Relaxed);
        self.cancellation_token = CancellationToken::new();
        self.tasks.clear();
        Ok(())
    }

    fn dispose(&mut self) -> anyhow::Result<()> {
        log::debug!("Disposing Massive data client {}", self.client_id);
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

        self.cancellation_token = CancellationToken::new();

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

        self.ws_client.disconnect().await;
        self.instruments.store(ahash::AHashMap::new());
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

    fn subscribe_quotes(&mut self, subscription: SubscribeQuotes) -> anyhow::Result<()> {
        let ws = self.ws_client.clone();
        let ticker = Self::ticker(subscription.instrument_id);

        get_runtime().spawn(async move {
            if let Err(e) = ws.subscribe(MassiveWsChannel::Quotes, &[ticker]).await {
                log::error!("Failed to subscribe to quotes: {e:?}");
            }
        });

        Ok(())
    }

    fn subscribe_trades(&mut self, subscription: SubscribeTrades) -> anyhow::Result<()> {
        let ws = self.ws_client.clone();
        let ticker = Self::ticker(subscription.instrument_id);

        get_runtime().spawn(async move {
            if let Err(e) = ws.subscribe(MassiveWsChannel::Trades, &[ticker]).await {
                log::error!("Failed to subscribe to trades: {e:?}");
            }
        });

        Ok(())
    }

    fn subscribe_bars(&mut self, subscription: SubscribeBars) -> anyhow::Result<()> {
        let bar_type = subscription.bar_type;
        let channel = ws_channel_for_bar_spec(&bar_type.spec())?;
        let ticker = Self::ticker(bar_type.instrument_id());
        let key = format!("{}.{ticker}", channel.as_ref());

        // Register on the original client so the bar type persists across clones
        self.ws_client.register_bar_type(key.clone(), bar_type);

        let mut ws = self.ws_client.clone();

        get_runtime().spawn(async move {
            ws.add_bar_type(key, bar_type).await;

            if let Err(e) = ws.subscribe(channel, &[ticker]).await {
                log::error!("Failed to subscribe to bars: {e:?}");
            }
        });

        Ok(())
    }

    fn unsubscribe_instrument(
        &mut self,
        _unsubscription: &UnsubscribeInstrument,
    ) -> anyhow::Result<()> {
        // `subscribe_instrument` only replays cached state; no venue subscription to tear down.
        Ok(())
    }

    fn unsubscribe_quotes(&mut self, unsubscription: &UnsubscribeQuotes) -> anyhow::Result<()> {
        let ws = self.ws_client.clone();
        let ticker = Self::ticker(unsubscription.instrument_id);

        get_runtime().spawn(async move {
            if let Err(e) = ws.unsubscribe(MassiveWsChannel::Quotes, &[ticker]).await {
                log::error!("Failed to unsubscribe from quotes: {e:?}");
            }
        });

        Ok(())
    }

    fn unsubscribe_trades(&mut self, unsubscription: &UnsubscribeTrades) -> anyhow::Result<()> {
        let ws = self.ws_client.clone();
        let ticker = Self::ticker(unsubscription.instrument_id);

        get_runtime().spawn(async move {
            if let Err(e) = ws.unsubscribe(MassiveWsChannel::Trades, &[ticker]).await {
                log::error!("Failed to unsubscribe from trades: {e:?}");
            }
        });

        Ok(())
    }

    fn unsubscribe_bars(&mut self, unsubscription: &UnsubscribeBars) -> anyhow::Result<()> {
        let bar_type = unsubscription.bar_type;
        let channel = ws_channel_for_bar_spec(&bar_type.spec())?;
        let ticker = Self::ticker(bar_type.instrument_id());
        let key = format!("{}.{ticker}", channel.as_ref());

        let mut ws = self.ws_client.clone();

        get_runtime().spawn(async move {
            if let Err(e) = ws.unsubscribe(channel, &[ticker]).await {
                log::error!("Failed to unsubscribe from bars: {e:?}");
            }
            ws.remove_bar_type(&key).await;
        });

        Ok(())
    }

    fn request_instruments(&self, request: RequestInstruments) -> anyhow::Result<()> {
        log::debug!("Requesting all instruments");

        let provider = self.provider.clone();
        let sender = self.data_sender.clone();
        let instruments_cache = self.instruments.clone();
        let request_id = request.request_id;
        let client_id = request.client_id.unwrap_or(self.client_id);
        let venue = Self::venue(self);
        let start_nanos = datetime_to_unix_nanos(request.start);
        let end_nanos = datetime_to_unix_nanos(request.end);
        let params = request.params;
        let clock = self.clock;

        get_runtime().spawn(async move {
            match provider.load_all().await {
                Ok(instruments) => {
                    instruments_cache.rcu(|m| {
                        for instrument in &instruments {
                            m.insert(instrument.id(), instrument.clone());
                        }
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
                    log::error!("Failed to fetch instruments: {e:?}");
                }
            }
        });

        Ok(())
    }

    fn request_instrument(&self, request: RequestInstrument) -> anyhow::Result<()> {
        log::debug!("Requesting instrument: {}", request.instrument_id);

        let provider = self.provider.clone();
        let sender = self.data_sender.clone();
        let instruments_cache = self.instruments.clone();
        let instrument_id = request.instrument_id;
        let ticker = instrument_id.symbol.to_string();
        let request_id = request.request_id;
        let client_id = request.client_id.unwrap_or(self.client_id);
        let start_nanos = datetime_to_unix_nanos(request.start);
        let end_nanos = datetime_to_unix_nanos(request.end);
        let params = request.params;
        let clock = self.clock;

        get_runtime().spawn(async move {
            match provider.load(&ticker).await {
                Ok(instrument) => {
                    instruments_cache.rcu(|m| {
                        m.insert(instrument.id(), instrument.clone());
                    });

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
                    log::error!("Failed to fetch instrument {instrument_id}: {e:?}");
                }
            }
        });

        Ok(())
    }

    fn request_trades(&self, request: RequestTrades) -> anyhow::Result<()> {
        log::debug!("Requesting trades for {}", request.instrument_id);

        let http = self.http_client.clone();
        let sender = self.data_sender.clone();
        let instrument_id = request.instrument_id;
        let request_id = request.request_id;
        let client_id = request.client_id.unwrap_or(self.client_id);
        let limit = request.limit.map(std::num::NonZeroUsize::get);
        let start_nanos = datetime_to_unix_nanos(request.start);
        let end_nanos = datetime_to_unix_nanos(request.end);
        let params = request.params;
        let clock = self.clock;

        get_runtime().spawn(async move {
            let start_ns = start_nanos.map(|ts| ts.as_u64());
            let end_ns = end_nanos.map(|ts| ts.as_u64());

            match http
                .request_trades(instrument_id, start_ns, end_ns, limit)
                .await
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

    fn request_quotes(&self, request: RequestQuotes) -> anyhow::Result<()> {
        log::debug!("Requesting quotes for {}", request.instrument_id);

        let http = self.http_client.clone();
        let sender = self.data_sender.clone();
        let instrument_id = request.instrument_id;
        let request_id = request.request_id;
        let client_id = request.client_id.unwrap_or(self.client_id);
        let limit = request.limit.map(std::num::NonZeroUsize::get);
        let start_nanos = datetime_to_unix_nanos(request.start);
        let end_nanos = datetime_to_unix_nanos(request.end);
        let params = request.params;
        let clock = self.clock;

        get_runtime().spawn(async move {
            let start_ns = start_nanos.map(|ts| ts.as_u64());
            let end_ns = end_nanos.map(|ts| ts.as_u64());

            match http
                .request_quotes(instrument_id, start_ns, end_ns, limit)
                .await
            {
                Ok(quotes) => {
                    let response = DataResponse::Quotes(QuotesResponse::new(
                        request_id,
                        client_id,
                        instrument_id,
                        quotes,
                        start_nanos,
                        end_nanos,
                        clock.get_time_ns(),
                        params,
                    ));

                    if let Err(e) = sender.send(DataEvent::Response(response)) {
                        log::error!("Failed to send quotes response: {e}");
                    }
                }
                Err(e) => log::error!("Quotes request failed for {instrument_id}: {e:?}"),
            }
        });

        Ok(())
    }

    fn request_bars(&self, request: RequestBars) -> anyhow::Result<()> {
        log::debug!("Requesting bars for {}", request.bar_type);

        let bar_type = request.bar_type;
        // Validate the specification up front so callers get an immediate error
        let window_nanos = crate::http::parse::bar_window_nanos(&bar_type.spec())?;
        crate::http::parse::bar_spec_to_aggs_params(&bar_type.spec())?;

        let http = self.http_client.clone();
        let sender = self.data_sender.clone();
        let request_id = request.request_id;
        let client_id = request.client_id.unwrap_or(self.client_id);
        let limit = request.limit.map(std::num::NonZeroUsize::get);
        let start = request.start;
        let end = request.end;
        let start_nanos = datetime_to_unix_nanos(start);
        let end_nanos = datetime_to_unix_nanos(end);
        let params = request.params;
        let adjusted = self.config.adjusted_bars;
        let timestamp_on_close = self.config.bars_timestamp_on_close;
        let clock = self.clock;

        get_runtime().spawn(async move {
            let now = jiff::Timestamp::now();
            let end_ms = end.unwrap_or(now).as_millisecond();
            let start_ms = match start {
                Some(s) => s.as_millisecond(),
                None => {
                    // Default lookback covers `limit` windows (or 300)
                    let count = limit.unwrap_or(300) as i64;
                    let window_ms = (window_nanos / 1_000_000) as i64;
                    end_ms.saturating_sub(count.saturating_mul(window_ms))
                }
            };

            match http
                .request_bars(
                    bar_type,
                    start_ms,
                    end_ms,
                    limit,
                    adjusted,
                    timestamp_on_close,
                )
                .await
            {
                Ok(mut bars) => {
                    if let Some(limit) = limit
                        && bars.len() > limit
                    {
                        bars.drain(..bars.len() - limit);
                    }

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
}

#[cfg(test)]
mod tests {
    use nautilus_common::live::runner::set_data_event_sender;
    use nautilus_core::{UUID4, UnixNanos};
    use nautilus_model::{data::bar::BarType, enums::AggregationSource};
    use rstest::rstest;

    use super::*;
    use crate::common::{consts::MASSIVE_CLIENT_ID, parse::instrument_id_from_ticker};

    fn make_client() -> MassiveDataClient {
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        set_data_event_sender(tx);
        MassiveDataClient::new(*MASSIVE_CLIENT_ID, MassiveDataClientConfig::default())
            .expect("client construction")
    }

    #[rstest]
    #[case(1, BarAggregation::Second, Some(MassiveWsChannel::AggregatesSecond))]
    #[case(1, BarAggregation::Minute, Some(MassiveWsChannel::AggregatesMinute))]
    #[case(5, BarAggregation::Minute, None)]
    #[case(1, BarAggregation::Hour, None)]
    #[case(100, BarAggregation::Tick, None)]
    fn test_ws_channel_for_bar_spec(
        #[case] step: usize,
        #[case] aggregation: BarAggregation,
        #[case] expected: Option<MassiveWsChannel>,
    ) {
        let spec = BarSpecification::new(step, aggregation, PriceType::Last);
        let result = ws_channel_for_bar_spec(&spec);
        match expected {
            Some(channel) => assert_eq!(result.unwrap(), channel),
            None => assert!(result.is_err()),
        }
    }

    #[rstest]
    fn test_ws_channel_rejects_non_last_price_type() {
        let spec = BarSpecification::new(1, BarAggregation::Minute, PriceType::Mid);
        assert!(ws_channel_for_bar_spec(&spec).is_err());
    }

    #[rstest]
    #[tokio::test]
    async fn test_client_construction_and_venue() {
        let client = make_client();
        assert_eq!(client.client_id(), *MASSIVE_CLIENT_ID);
        assert_eq!(DataClient::venue(&client), Some(*MASSIVE_VENUE));
        assert!(!DataClient::is_connected(&client));
        assert!(client.is_disconnected());
    }

    #[rstest]
    #[tokio::test]
    async fn test_subscribe_bars_rejects_unsupported_spec() {
        let mut client = make_client();

        let bar_type = BarType::new(
            instrument_id_from_ticker("AAPL"),
            BarSpecification::new(5, BarAggregation::Minute, PriceType::Last),
            AggregationSource::External,
        );
        let cmd = SubscribeBars::new(
            bar_type,
            Some(*MASSIVE_CLIENT_ID),
            None,
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        );

        let err = client
            .subscribe_bars(cmd)
            .expect_err("5-minute live bars must be rejected");
        assert!(err.to_string().contains("1-SECOND and 1-MINUTE"));
    }

    #[rstest]
    fn test_dispatch_ws_message_trade() {
        use nautilus_model::{
            data::TradeTick,
            enums::AggressorSide,
            identifiers::TradeId,
            types::{Price, Quantity},
        };

        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        let tick = TradeTick::new(
            instrument_id_from_ticker("AAPL"),
            Price::from("100.00"),
            Quantity::from(10),
            AggressorSide::NoAggressor,
            TradeId::new("1"),
            UnixNanos::from(1),
            UnixNanos::from(2),
        );

        dispatch_ws_message(NautilusWsMessage::Trade(tick), &tx);

        match rx.try_recv() {
            Ok(DataEvent::Data(Data::Trade(received))) => {
                assert_eq!(received.instrument_id, tick.instrument_id);
            }
            other => panic!("expected trade data event, was {other:?}"),
        }
    }
}
