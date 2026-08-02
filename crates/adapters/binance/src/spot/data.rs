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

//! Live market data client implementation for the Binance Spot adapter.

use std::{
    str::FromStr,
    sync::{
        Arc, RwLock,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use ahash::AHashMap;
use anyhow::Context;
use futures_util::{StreamExt, pin_mut};
use nautilus_common::{
    clients::DataClient,
    live::{runner::get_data_event_sender, runtime::get_runtime},
    messages::{
        DataEvent,
        data::{
            BarsResponse, BookResponse, CustomDataResponse, DataResponse, InstrumentResponse,
            InstrumentsResponse, RequestBars, RequestBookSnapshot, RequestCustomData,
            RequestInstrument, RequestInstruments, RequestTrades, SubscribeBars,
            SubscribeBookDeltas, SubscribeCustomData, SubscribeInstrument, SubscribeInstruments,
            SubscribeQuotes, SubscribeTrades, TradesResponse, UnsubscribeBars,
            UnsubscribeBookDeltas, UnsubscribeCustomData, UnsubscribeQuotes, UnsubscribeTrades,
            subscribe::SubscribeInstrumentStatus, unsubscribe::UnsubscribeInstrumentStatus,
        },
    },
};
use nautilus_core::{
    AtomicMap, MUTEX_POISONED, Params,
    datetime::datetime_to_unix_nanos,
    nanos::UnixNanos,
    time::{AtomicTime, get_atomic_clock_realtime},
};
use nautilus_model::{
    data::{
        BookOrder, CustomData, Data, DataType, OrderBookDelta, OrderBookDeltas,
        OrderBookDeltas_API, QuoteTick,
    },
    enums::{
        AggregationSource, BookAction, BookType, MarketStatusAction, OrderSide, PriceType,
        RecordFlag,
    },
    identifiers::{ClientId, InstrumentId, Venue},
    instruments::{Instrument, InstrumentAny},
    types::{Price, Quantity},
};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use ustr::Ustr;

use crate::{
    common::{
        bar::{binance_bar_data_type, parse_binance_bar_type},
        consts::BINANCE_VENUE,
        credential::resolve_credentials,
        enums::{BinanceEnvironment, BinanceProductType},
        parse::{bar_spec_to_binance_interval, quote_to_l1_deltas},
        status::diff_and_emit_statuses,
        urls::{get_http_base_url_with_us, get_ws_base_url_with_us},
    },
    config::{BinanceDataClientConfig, BinanceSpotMarketDataMode},
    data_types::register_binance_custom_data,
    spot::{
        http::{BinanceDepth, DepthParams, client::BinanceSpotHttpClient},
        websocket::{
            public_json::{
                BinanceSpotPublicJsonWebSocketClient,
                messages::BinanceSpotPublicWsMessage,
                parse::{
                    parse_book_ticker as parse_json_book_ticker,
                    parse_depth_diff as parse_json_depth_diff,
                    parse_depth_snapshot as parse_json_depth_snapshot,
                    parse_kline as parse_json_kline, parse_ticker as parse_json_ticker,
                    parse_trade as parse_json_trade,
                },
            },
            streams::{
                client::BinanceSpotWebSocketClient,
                messages::BinanceSpotWsMessage,
                parse::{
                    parse_bbo_event, parse_depth_diff, parse_depth_snapshot, parse_trades_event,
                },
            },
        },
    },
};

const MAX_SNAPSHOT_RETRIES: u32 = 5;
const MAX_BUFFERED_DEPTH_UPDATES: usize = 10_000;
const SNAPSHOT_RETRY_BACKOFF_BASE_MS: u64 = 250;
const SNAPSHOT_RETRY_BACKOFF_CAP_MS: u64 = 3_000;

#[derive(Debug, Clone)]
struct BufferedDepthUpdate {
    deltas: OrderBookDeltas,
    first_update_id: u64,
    final_update_id: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BookSyncStatus {
    Buffering,
    Failed,
}

#[derive(Debug, Clone)]
struct BookBuffer {
    updates: Vec<BufferedDepthUpdate>,
    epoch: u64,
    status: BookSyncStatus,
}

impl BookBuffer {
    fn new(epoch: u64) -> Self {
        Self {
            updates: Vec::new(),
            epoch,
            status: BookSyncStatus::Buffering,
        }
    }
}

#[derive(Debug, Clone)]
enum SpotWsClient {
    Sbe(BinanceSpotWebSocketClient),
    JsonPublic(BinanceSpotPublicJsonWebSocketClient),
}

impl SpotWsClient {
    fn has_credentials(&self) -> bool {
        match self {
            Self::Sbe(client) => client.has_credentials(),
            Self::JsonPublic(_) => true, // Public JSON streams require no credentials
        }
    }

    fn replace_instruments(&self, instruments: &[InstrumentAny]) {
        match self {
            Self::Sbe(client) => client.replace_instruments(instruments),
            Self::JsonPublic(client) => client.replace_instruments(instruments),
        }
    }

    async fn subscribe(&self, streams: Vec<String>) -> anyhow::Result<()> {
        match self {
            Self::Sbe(client) => client.subscribe(streams).await.map_err(Into::into),
            Self::JsonPublic(client) => client.subscribe(streams).await,
        }
    }

    async fn unsubscribe(&self, streams: Vec<String>) -> anyhow::Result<()> {
        match self {
            Self::Sbe(client) => client.unsubscribe(streams).await.map_err(Into::into),
            Self::JsonPublic(client) => client.unsubscribe(streams).await,
        }
    }

    async fn close(&mut self) -> anyhow::Result<()> {
        match self {
            Self::Sbe(client) => client.close().await.map_err(Into::into),
            Self::JsonPublic(client) => client.close().await,
        }
    }
}

fn looks_like_spot_sbe_ws_url(base_url: &str) -> bool {
    let without_scheme = base_url
        .split_once("://")
        .map_or(base_url, |(_, rest)| rest);
    let host = without_scheme
        .split(['/', ':'])
        .next()
        .unwrap_or(without_scheme);
    host.starts_with("stream-sbe") || host.starts_with("demo-stream-sbe")
}

fn resolve_spot_json_ws_url(
    base_url_ws: Option<String>,
    environment: BinanceEnvironment,
    us: bool,
) -> String {
    let default_url =
        get_ws_base_url_with_us(BinanceProductType::Spot, environment, us).to_string();

    match base_url_ws {
        Some(url) if looks_like_spot_sbe_ws_url(&url) => {
            log::warn!(
                "Spot JSON market-data mode received an SBE WebSocket URL override (`{url}`); \
                 using Spot JSON WebSocket default for {environment:?}: {default_url}",
            );
            default_url
        }
        Some(url) => url,
        None => default_url,
    }
}

/// Binance Spot data client for SBE market data.
#[derive(Debug)]
pub struct BinanceSpotDataClient {
    clock: &'static AtomicTime,
    client_id: ClientId,
    config: BinanceDataClientConfig,
    http_client: BinanceSpotHttpClient,
    ws_client: SpotWsClient,
    spot_market_data_mode: BinanceSpotMarketDataMode,
    is_connected: AtomicBool,
    cancellation_token: CancellationToken,
    tasks: Vec<JoinHandle<()>>,
    data_sender: tokio::sync::mpsc::UnboundedSender<DataEvent>,
    instruments: Arc<AtomicMap<InstrumentId, InstrumentAny>>,
    status_cache: Arc<AtomicMap<InstrumentId, MarketStatusAction>>,
    book_buffers: Arc<AtomicMap<InstrumentId, BookBuffer>>,
    book_subscriptions: Arc<AtomicMap<InstrumentId, u32>>,
    l1_book_subscriptions: Arc<AtomicMap<InstrumentId, u32>>,
    quote_refs: Arc<AtomicMap<InstrumentId, u32>>,
    ticker_refs: Arc<AtomicMap<InstrumentId, u32>>,
    book_epoch: Arc<RwLock<u64>>,
}

impl BinanceSpotDataClient {
    /// Creates a new [`BinanceSpotDataClient`] instance.
    ///
    /// # Errors
    ///
    /// Returns an error if the client fails to initialize.
    pub fn new(client_id: ClientId, config: BinanceDataClientConfig) -> anyhow::Result<Self> {
        config.validate()?;
        let clock = get_atomic_clock_realtime();
        let spot_market_data_mode = config.spot_market_data_mode;
        let base_url_http = config.base_url_http.clone().or_else(|| {
            config.us.then(|| {
                get_http_base_url_with_us(config.product_type, config.environment, true).to_string()
            })
        });

        let http_client = BinanceSpotHttpClient::new_with_json_responses(
            config.environment,
            clock,
            config.api_key.clone(),
            config.api_secret.clone(),
            base_url_http,
            Some(config.recv_window_ms),
            None, // timeout_secs
            config.proxy_url.clone(),
            config.us,
        )?;

        let creds = if spot_market_data_mode == BinanceSpotMarketDataMode::Sbe {
            resolve_credentials(
                config.api_key.clone(),
                config.api_secret.clone(),
                config.environment,
                config.product_type,
            )
            .inspect_err(|e| {
                log::warn!(
                    "Failed to resolve Binance API credentials ({e}). \
                     Spot SBE WebSocket streams require an Ed25519 API key. \
                     Set the appropriate env vars for your environment, \
                     or provide api_key/api_secret in the data client config"
                );
            })
            .ok()
        } else {
            None
        };

        let ws_client = match spot_market_data_mode {
            // SBE streams require Ed25519 authentication
            BinanceSpotMarketDataMode::Sbe => SpotWsClient::Sbe(
                BinanceSpotWebSocketClient::new(
                    config.base_url_ws.clone(),
                    creds.as_ref().map(|(k, _)| k.clone()),
                    creds.as_ref().map(|(_, s)| s.clone()),
                    Some(20), // Heartbeat interval
                    config.transport_backend,
                )?
                .with_proxy(config.proxy_url.clone()),
            ),
            BinanceSpotMarketDataMode::Json => {
                SpotWsClient::JsonPublic(
                    BinanceSpotPublicJsonWebSocketClient::new(
                        Some(resolve_spot_json_ws_url(
                            config.base_url_ws.clone(),
                            config.environment,
                            config.us,
                        )),
                        Some(20), // Heartbeat interval
                        config.transport_backend,
                    )
                    .with_proxy(config.proxy_url.clone()),
                )
            }
        };
        let data_sender = get_data_event_sender();

        log::debug!("Configured Spot market data mode: {spot_market_data_mode:?}");

        Ok(Self {
            clock,
            client_id,
            config,
            http_client,
            ws_client,
            spot_market_data_mode,
            is_connected: AtomicBool::new(false),
            cancellation_token: CancellationToken::new(),
            tasks: Vec::new(),
            data_sender,
            instruments: Arc::new(AtomicMap::new()),
            status_cache: Arc::new(AtomicMap::new()),
            book_buffers: Arc::new(AtomicMap::new()),
            book_subscriptions: Arc::new(AtomicMap::new()),
            l1_book_subscriptions: Arc::new(AtomicMap::new()),
            quote_refs: Arc::new(AtomicMap::new()),
            ticker_refs: Arc::new(AtomicMap::new()),
            book_epoch: Arc::new(RwLock::new(0)),
        })
    }

    fn venue(&self) -> Venue {
        *BINANCE_VENUE
    }

    fn send_data(sender: &tokio::sync::mpsc::UnboundedSender<DataEvent>, data: Data) {
        if let Err(e) = sender.send(DataEvent::Data(data)) {
            log::error!("Failed to emit data event: {e}");
        }
    }

    fn spawn_ws<F>(&self, fut: F, context: &'static str)
    where
        F: std::future::Future<Output = anyhow::Result<()>> + Send + 'static,
    {
        get_runtime().spawn(async move {
            if let Err(e) = fut.await {
                log::error!("{context}: {e:?}");
            }
        });
    }

    #[expect(clippy::too_many_arguments)]
    async fn refresh_instrument_catalogue(
        http: &BinanceSpotHttpClient,
        provider: &crate::config::BinanceInstrumentProviderConfig,
        us: bool,
        instruments_cache: &Arc<AtomicMap<InstrumentId, InstrumentAny>>,
        status_cache: &Arc<AtomicMap<InstrumentId, MarketStatusAction>>,
        ws: &SpotWsClient,
        sender: &tokio::sync::mpsc::UnboundedSender<DataEvent>,
        clock: &'static AtomicTime,
        emit_status_changes: bool,
    ) -> anyhow::Result<Vec<InstrumentAny>> {
        let instruments = http
            .request_instruments_with_config(provider, us)
            .await
            .context("failed to request Binance Spot instruments")?;
        let venue_statuses = http
            .request_symbol_statuses(us)
            .await
            .context("failed to request Binance Spot instrument statuses")?;

        let instrument_map = instruments
            .iter()
            .map(|instrument| (instrument.id(), instrument.clone()))
            .collect::<AHashMap<_, _>>();
        let status_map = venue_statuses
            .into_iter()
            .filter(|(instrument_id, _)| instrument_map.contains_key(instrument_id))
            .collect::<AHashMap<_, _>>();

        instruments_cache.store(instrument_map);
        ws.replace_instruments(&instruments);

        if emit_status_changes {
            let mut cached_statuses = (**status_cache.load()).clone();
            let ts = clock.get_time_ns();
            diff_and_emit_statuses(&status_map, &mut cached_statuses, sender, ts, ts);
            status_cache.store(cached_statuses);
        } else {
            status_cache.store(status_map);
        }

        for instrument in &instruments {
            if let Err(e) = sender.send(DataEvent::Instrument(instrument.clone())) {
                log::warn!("Failed to send refreshed Binance Spot instrument: {e}");
            }
        }

        Ok(instruments)
    }

    #[expect(clippy::too_many_arguments)]
    fn handle_ws_message(
        msg: BinanceSpotWsMessage,
        data_sender: &tokio::sync::mpsc::UnboundedSender<DataEvent>,
        instruments: &Arc<AtomicMap<InstrumentId, InstrumentAny>>,
        ws_instruments: &Arc<AtomicMap<Ustr, InstrumentAny>>,
        book_buffers: &Arc<AtomicMap<InstrumentId, BookBuffer>>,
        book_subscriptions: &Arc<AtomicMap<InstrumentId, u32>>,
        l1_book_subscriptions: &Arc<AtomicMap<InstrumentId, u32>>,
        book_epoch: &Arc<RwLock<u64>>,
        http_client: &BinanceSpotHttpClient,
        clock: &'static AtomicTime,
    ) {
        let ts_init = clock.get_time_ns();

        match msg {
            BinanceSpotWsMessage::Trades(ref event) => {
                let symbol = Ustr::from(&event.symbol);
                let cache = ws_instruments.load();
                if let Some(instrument) = cache.get(&symbol) {
                    let trades = parse_trades_event(event, instrument, ts_init);
                    for data in trades {
                        Self::send_data(data_sender, data);
                    }
                }
            }
            BinanceSpotWsMessage::BestBidAsk(ref event) => {
                let symbol = Ustr::from(&event.symbol);
                let cache = ws_instruments.load();
                if let Some(instrument) = cache.get(&symbol) {
                    let quote = parse_bbo_event(event, instrument, ts_init);
                    Self::send_top_of_book(
                        data_sender,
                        l1_book_subscriptions,
                        quote,
                        event.book_update_id as u64,
                    );
                }
            }
            BinanceSpotWsMessage::DepthSnapshot(ref event) => {
                let symbol = Ustr::from(&event.symbol);
                let cache = ws_instruments.load();
                if let Some(instrument) = cache.get(&symbol)
                    && let Some(deltas) = parse_depth_snapshot(event, instrument, ts_init)
                {
                    Self::send_data(data_sender, Data::Deltas(OrderBookDeltas_API::new(deltas)));
                }
            }
            BinanceSpotWsMessage::DepthDiff(ref event) => {
                let symbol = Ustr::from(&event.symbol);
                let cache = ws_instruments.load();
                if let Some(instrument) = cache.get(&symbol)
                    && let Some(deltas) = parse_depth_diff(event, instrument, ts_init)
                {
                    let first_update_id = event.first_book_update_id as u64;
                    let final_update_id = event.last_book_update_id as u64;

                    Self::route_depth_diff(
                        data_sender,
                        book_buffers,
                        deltas,
                        first_update_id,
                        final_update_id,
                    );
                }
            }
            BinanceSpotWsMessage::ServerShutdown(ref msg) => {
                log::warn!(
                    "Binance server shutdown notice (event_time={}); disconnect expected within ~10 minutes",
                    msg.event_time,
                );
            }
            BinanceSpotWsMessage::RawBinary(data) => {
                log::debug!("Unhandled binary message: {} bytes", data.len());
            }
            BinanceSpotWsMessage::RawJson(value) => {
                log::debug!("Unhandled JSON message: {value:?}");
            }
            BinanceSpotWsMessage::Error(e) => {
                log::warn!("Binance WebSocket error: code={}, msg={}", e.code, e.msg);
            }
            BinanceSpotWsMessage::Reconnected => {
                log::info!("WebSocket reconnected, rebuilding order book snapshots");

                Self::rebuild_full_depth_books(
                    data_sender,
                    instruments,
                    book_buffers,
                    book_subscriptions,
                    book_epoch,
                    http_client,
                    clock,
                );
            }
        }
    }

    #[expect(clippy::too_many_arguments)]
    fn handle_public_json_ws_message(
        msg: BinanceSpotPublicWsMessage,
        data_sender: &tokio::sync::mpsc::UnboundedSender<DataEvent>,
        instruments: &Arc<AtomicMap<InstrumentId, InstrumentAny>>,
        ws_instruments: &Arc<AtomicMap<Ustr, InstrumentAny>>,
        book_buffers: &Arc<AtomicMap<InstrumentId, BookBuffer>>,
        book_subscriptions: &Arc<AtomicMap<InstrumentId, u32>>,
        l1_book_subscriptions: &Arc<AtomicMap<InstrumentId, u32>>,
        book_epoch: &Arc<RwLock<u64>>,
        http_client: &BinanceSpotHttpClient,
        clock: &'static AtomicTime,
    ) {
        let ts_init = clock.get_time_ns();

        match msg {
            BinanceSpotPublicWsMessage::Trade(ref event) => {
                let symbol = event.symbol;
                let cache = ws_instruments.load();
                if let Some(instrument) = cache.get(&symbol) {
                    match parse_json_trade(event, instrument, ts_init) {
                        Ok(trade) => Self::send_data(data_sender, Data::Trade(trade)),
                        Err(e) => log::warn!("Failed to parse Spot JSON trade: {e}"),
                    }
                }
            }
            BinanceSpotPublicWsMessage::BookTicker(ref event) => {
                let symbol = event.symbol;
                let cache = ws_instruments.load();
                if let Some(instrument) = cache.get(&symbol) {
                    match parse_json_book_ticker(event, instrument, ts_init) {
                        Ok(quote) => Self::send_top_of_book(
                            data_sender,
                            l1_book_subscriptions,
                            quote,
                            event.book_update_id,
                        ),
                        Err(e) => log::warn!("Failed to parse Spot JSON book ticker: {e}"),
                    }
                }
            }
            BinanceSpotPublicWsMessage::DepthSnapshot(ref event) => {
                let symbol = event.symbol;
                let cache = ws_instruments.load();
                if let Some(instrument) = cache.get(&symbol)
                    && let Some(deltas) = parse_json_depth_snapshot(event, instrument, ts_init)
                {
                    Self::send_data(data_sender, Data::Deltas(OrderBookDeltas_API::new(deltas)));
                }
            }
            BinanceSpotPublicWsMessage::DepthDiff(ref event) => {
                let symbol = event.symbol;
                let cache = ws_instruments.load();
                if let Some(instrument) = cache.get(&symbol) {
                    match parse_json_depth_diff(event, instrument, ts_init) {
                        Ok(Some(deltas)) => Self::route_depth_diff(
                            data_sender,
                            book_buffers,
                            deltas,
                            event.first_update_id,
                            event.final_update_id,
                        ),
                        Ok(None) => {}
                        Err(e) => log::warn!("Failed to parse Spot JSON depth update: {e}"),
                    }
                }
            }
            BinanceSpotPublicWsMessage::Kline(ref event) => {
                let symbol = event.symbol;
                let cache = ws_instruments.load();
                if let Some(instrument) = cache.get(&symbol) {
                    match parse_json_kline(event, instrument, ts_init) {
                        Ok(Some(bar)) => {
                            Self::send_data(data_sender, Data::Bar(bar.bar()));
                            let data_type = binance_bar_data_type(bar.bar_type);
                            Self::send_data(
                                data_sender,
                                Data::Custom(CustomData::new(Arc::new(bar), data_type)),
                            );
                        }
                        Ok(None) => {} // Kline not closed yet
                        Err(e) => log::warn!("Failed to parse Spot JSON kline: {e}"),
                    }
                }
            }
            BinanceSpotPublicWsMessage::Ticker(ref event) => {
                let symbol = event.symbol;
                let cache = ws_instruments.load();
                if let Some(instrument) = cache.get(&symbol) {
                    match parse_json_ticker(event, instrument, ts_init) {
                        Ok(ticker) => {
                            let data_type = spot_ticker_data_type(instrument.id());
                            Self::send_data(
                                data_sender,
                                Data::Custom(CustomData::new(Arc::new(ticker), data_type)),
                            );
                        }
                        Err(e) => log::warn!("Failed to parse Spot JSON ticker: {e}"),
                    }
                }
            }
            BinanceSpotPublicWsMessage::ServerShutdown(ref msg) => {
                log::warn!(
                    "Binance Spot JSON server shutdown notice (event_time={}); disconnect expected within ~10 minutes",
                    msg.event_time,
                );
            }
            BinanceSpotPublicWsMessage::RawJson(value) => {
                log::debug!("Unhandled Spot JSON message: {value:?}");
            }
            BinanceSpotPublicWsMessage::Error(e) => {
                log::warn!("Spot JSON WebSocket error: code={}, msg={}", e.code, e.msg);
            }
            BinanceSpotPublicWsMessage::Reconnected => {
                log::info!("Spot JSON WebSocket reconnected, rebuilding order book snapshots");

                Self::rebuild_full_depth_books(
                    data_sender,
                    instruments,
                    book_buffers,
                    book_subscriptions,
                    book_epoch,
                    http_client,
                    clock,
                );
            }
        }
    }

    fn send_top_of_book(
        data_sender: &tokio::sync::mpsc::UnboundedSender<DataEvent>,
        l1_book_subscriptions: &Arc<AtomicMap<InstrumentId, u32>>,
        quote: QuoteTick,
        sequence: u64,
    ) {
        Self::send_data(data_sender, Data::Quote(quote));
        if l1_book_subscriptions.contains_key(&quote.instrument_id) {
            let deltas = quote_to_l1_deltas(quote, sequence);
            Self::send_data(data_sender, Data::Deltas(OrderBookDeltas_API::new(deltas)));
        }
    }

    fn route_depth_diff(
        data_sender: &tokio::sync::mpsc::UnboundedSender<DataEvent>,
        book_buffers: &Arc<AtomicMap<InstrumentId, BookBuffer>>,
        deltas: OrderBookDeltas,
        first_update_id: u64,
        final_update_id: u64,
    ) {
        let instrument_id = deltas.instrument_id;

        if book_buffers.contains_key(&instrument_id) {
            let mut handled_by_sync = false;
            book_buffers.rcu(|m| {
                handled_by_sync = false;

                if let Some(buffer) = m.get_mut(&instrument_id) {
                    handled_by_sync = true;

                    if buffer.status == BookSyncStatus::Buffering {
                        buffer.updates.push(BufferedDepthUpdate {
                            deltas: deltas.clone(),
                            first_update_id,
                            final_update_id,
                        });
                        trim_buffered_depth_updates(&mut buffer.updates);
                    }
                }
            });

            if handled_by_sync {
                return;
            }
        }

        Self::send_data(data_sender, Data::Deltas(OrderBookDeltas_API::new(deltas)));
    }

    fn rebuild_full_depth_books(
        data_sender: &tokio::sync::mpsc::UnboundedSender<DataEvent>,
        instruments: &Arc<AtomicMap<InstrumentId, InstrumentAny>>,
        book_buffers: &Arc<AtomicMap<InstrumentId, BookBuffer>>,
        book_subscriptions: &Arc<AtomicMap<InstrumentId, u32>>,
        book_epoch: &Arc<RwLock<u64>>,
        http_client: &BinanceSpotHttpClient,
        clock: &'static AtomicTime,
    ) {
        let epoch = {
            let mut guard = book_epoch.write().expect(MUTEX_POISONED);
            *guard = guard.wrapping_add(1);
            *guard
        };

        let subs: Vec<(InstrumentId, u32)> = {
            let guard = book_subscriptions.load();
            guard.iter().map(|(k, v)| (*k, *v)).collect()
        };

        for (instrument_id, depth) in subs {
            if depth != 0 {
                continue;
            }

            book_buffers.insert(instrument_id, BookBuffer::new(epoch));

            log::debug!(
                "OrderBook snapshot rebuild for {instrument_id} starting \
                (reconnect, epoch={epoch})"
            );

            let http = http_client.clone();
            let sender = data_sender.clone();
            let buffers = book_buffers.clone();
            let insts = instruments.clone();

            get_runtime().spawn(async move {
                Self::fetch_and_emit_snapshot(
                    http,
                    sender,
                    buffers,
                    insts,
                    instrument_id,
                    epoch,
                    clock,
                )
                .await;
            });
        }
    }

    fn quote_stream_suffix(&self) -> &'static str {
        match self.spot_market_data_mode {
            BinanceSpotMarketDataMode::Sbe => "bestBidAsk",
            BinanceSpotMarketDataMode::Json => "bookTicker",
        }
    }

    fn required_instrument_id_metadata(data_type: &DataType) -> anyhow::Result<InstrumentId> {
        let raw = data_type
            .metadata()
            .as_ref()
            .and_then(|metadata| metadata.get("instrument_id"))
            .and_then(|value| value.as_str())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .context("custom data subscription requires `instrument_id` metadata")?;
        InstrumentId::from_str(raw)
            .with_context(|| format!("invalid instrument_id metadata `{raw}`"))
    }

    async fn fetch_and_emit_snapshot(
        http: BinanceSpotHttpClient,
        sender: tokio::sync::mpsc::UnboundedSender<DataEvent>,
        buffers: Arc<AtomicMap<InstrumentId, BookBuffer>>,
        instruments: Arc<AtomicMap<InstrumentId, InstrumentAny>>,
        instrument_id: InstrumentId,
        epoch: u64,
        clock: &'static AtomicTime,
    ) {
        Self::fetch_and_emit_snapshot_inner(
            http,
            sender,
            buffers,
            instruments,
            instrument_id,
            epoch,
            clock,
            0,
        )
        .await;
    }

    #[expect(clippy::too_many_arguments)]
    async fn fetch_and_emit_snapshot_inner(
        http: BinanceSpotHttpClient,
        sender: tokio::sync::mpsc::UnboundedSender<DataEvent>,
        buffers: Arc<AtomicMap<InstrumentId, BookBuffer>>,
        instruments: Arc<AtomicMap<InstrumentId, InstrumentAny>>,
        instrument_id: InstrumentId,
        epoch: u64,
        clock: &'static AtomicTime,
        retry_count: u32,
    ) {
        const SNAPSHOT_DEPTH: u32 = 5000;

        if Self::wait_for_buffered_update(&buffers, instrument_id, epoch)
            .await
            .is_none()
        {
            return;
        }

        let params = DepthParams {
            symbol: instrument_id.symbol.as_str().to_uppercase(),
            limit: Some(SNAPSHOT_DEPTH),
        };

        match http.inner().depth(&params).await {
            Ok(depth_snapshot) => {
                let ts_init = clock.get_time_ns();
                let last_update_id = depth_snapshot.last_update_id as u64;

                {
                    let guard = buffers.load();
                    match guard.get(&instrument_id) {
                        None => {
                            log::debug!(
                                "OrderBook subscription for {instrument_id} was cancelled, \
                                discarding snapshot"
                            );
                            return;
                        }
                        Some(buffer) if buffer.epoch != epoch => {
                            log::debug!(
                                "OrderBook snapshot for {instrument_id} is stale \
                                (epoch {epoch} != {}), discarding",
                                buffer.epoch
                            );
                            return;
                        }
                        Some(buffer) if buffer.status == BookSyncStatus::Failed => {
                            log::debug!(
                                "OrderBook snapshot for {instrument_id} belongs to a failed \
                                sync, discarding"
                            );
                            return;
                        }
                        _ => {}
                    }
                }

                let (price_precision, size_precision) = {
                    let guard = instruments.load();
                    match guard.get(&instrument_id) {
                        Some(inst) => (inst.price_precision(), inst.size_precision()),
                        None => {
                            log::error!("No instrument in cache for snapshot: {instrument_id}");
                            Self::mark_book_sync_failed(&buffers, instrument_id, epoch);
                            return;
                        }
                    }
                };

                let Some(first) = Self::wait_for_first_applicable_update(
                    &buffers,
                    instrument_id,
                    epoch,
                    last_update_id,
                )
                .await
                else {
                    return;
                };

                let target = last_update_id + 1;
                if !spot_overlap_valid(first.first_update_id, first.final_update_id, last_update_id)
                {
                    if retry_count < MAX_SNAPSHOT_RETRIES {
                        log::warn!(
                            "OrderBook overlap validation failed for {instrument_id}: \
                            lastUpdateId={last_update_id}, first_update_id={}, \
                            final_update_id={} (need U <= {} <= u), \
                            retrying snapshot (attempt {}/{})",
                            first.first_update_id,
                            first.final_update_id,
                            target,
                            retry_count + 1,
                            MAX_SNAPSHOT_RETRIES
                        );

                        tokio::time::sleep(spot_snapshot_retry_backoff(retry_count)).await;

                        Box::pin(Self::fetch_and_emit_snapshot_inner(
                            http,
                            sender,
                            buffers,
                            instruments,
                            instrument_id,
                            epoch,
                            clock,
                            retry_count + 1,
                        ))
                        .await;
                        return;
                    }

                    log::error!(
                        "OrderBook overlap validation failed for {instrument_id} after \
                        {MAX_SNAPSHOT_RETRIES} retries; no deltas will be emitted until \
                        resubscribe or reconnect"
                    );
                    Self::mark_book_sync_failed(&buffers, instrument_id, epoch);
                    return;
                }

                let Some(buffered) =
                    Self::take_buffered_depth_updates(&buffers, instrument_id, epoch)
                else {
                    return;
                };

                let mut replayed = 0;
                let mut last_final_update_id = last_update_id;
                let mut is_first = true;
                let mut replay_ready = Vec::with_capacity(buffered.len());

                for update in buffered {
                    if update.final_update_id <= last_update_id {
                        continue;
                    }

                    if !spot_continuity_ok(is_first, update.first_update_id, last_final_update_id) {
                        if retry_count < MAX_SNAPSHOT_RETRIES {
                            log::warn!(
                                "OrderBook continuity break for {instrument_id}: \
                                expected U={}, was U={}, triggering resync (attempt {}/{})",
                                last_final_update_id + 1,
                                update.first_update_id,
                                retry_count + 1,
                                MAX_SNAPSHOT_RETRIES
                            );

                            Self::reset_book_sync_buffer(&buffers, instrument_id, epoch);
                            tokio::time::sleep(spot_snapshot_retry_backoff(retry_count)).await;

                            Box::pin(Self::fetch_and_emit_snapshot_inner(
                                http,
                                sender,
                                buffers,
                                instruments,
                                instrument_id,
                                epoch,
                                clock,
                                retry_count + 1,
                            ))
                            .await;
                            return;
                        }

                        log::error!(
                            "OrderBook continuity break for {instrument_id} after \
                            {MAX_SNAPSHOT_RETRIES} retries; no deltas will be emitted until \
                            resubscribe or reconnect"
                        );
                        Self::mark_book_sync_failed(&buffers, instrument_id, epoch);
                        return;
                    }

                    last_final_update_id = update.final_update_id;
                    is_first = false;
                    replayed += 1;
                    replay_ready.push(update);
                }

                let snapshot_ts_event = replay_ready
                    .first()
                    .map_or(ts_init, |update| update.deltas.ts_event);

                let snapshot_deltas = match parse_spot_depth_snapshot(
                    &depth_snapshot,
                    instrument_id,
                    price_precision,
                    size_precision,
                    snapshot_ts_event,
                    ts_init,
                ) {
                    Ok(Some(deltas)) => deltas,
                    Ok(None) => {
                        if retry_count < MAX_SNAPSHOT_RETRIES {
                            log::warn!(
                                "OrderBook snapshot for {instrument_id} contained no levels; \
                                retrying snapshot (attempt {}/{})",
                                retry_count + 1,
                                MAX_SNAPSHOT_RETRIES
                            );

                            tokio::time::sleep(spot_snapshot_retry_backoff(retry_count)).await;

                            Box::pin(Self::fetch_and_emit_snapshot_inner(
                                http,
                                sender,
                                buffers,
                                instruments,
                                instrument_id,
                                epoch,
                                clock,
                                retry_count + 1,
                            ))
                            .await;
                            return;
                        }

                        log::error!(
                            "OrderBook snapshot for {instrument_id} contained no levels after \
                            {MAX_SNAPSHOT_RETRIES} retries; no deltas will be emitted until \
                            resubscribe or reconnect"
                        );
                        Self::mark_book_sync_failed(&buffers, instrument_id, epoch);
                        return;
                    }
                    Err(e) => {
                        if retry_count < MAX_SNAPSHOT_RETRIES {
                            log::warn!(
                                "Failed to parse order book snapshot for {instrument_id}: {e}; \
                                retrying snapshot (attempt {}/{})",
                                retry_count + 1,
                                MAX_SNAPSHOT_RETRIES
                            );

                            tokio::time::sleep(spot_snapshot_retry_backoff(retry_count)).await;

                            Box::pin(Self::fetch_and_emit_snapshot_inner(
                                http,
                                sender,
                                buffers,
                                instruments,
                                instrument_id,
                                epoch,
                                clock,
                                retry_count + 1,
                            ))
                            .await;
                            return;
                        }

                        log::error!(
                            "Failed to parse order book snapshot for {instrument_id} after \
                            {MAX_SNAPSHOT_RETRIES} retries: {e}; no deltas will be emitted \
                            until resubscribe or reconnect"
                        );
                        Self::mark_book_sync_failed(&buffers, instrument_id, epoch);
                        return;
                    }
                };

                if let Err(e) = sender.send(DataEvent::Data(Data::Deltas(
                    OrderBookDeltas_API::new(snapshot_deltas),
                ))) {
                    log::error!("Failed to send snapshot: {e}");
                }

                for update in replay_ready {
                    if let Err(e) = sender.send(DataEvent::Data(Data::Deltas(
                        OrderBookDeltas_API::new(update.deltas),
                    ))) {
                        log::error!("Failed to send replayed deltas: {e}");
                    }
                }

                while let Some(more) =
                    Self::drain_buffered_depth_updates(&buffers, instrument_id, epoch)
                {
                    for update in more {
                        if update.final_update_id <= last_update_id {
                            continue;
                        }

                        if !spot_continuity_ok(
                            is_first,
                            update.first_update_id,
                            last_final_update_id,
                        ) {
                            if retry_count < MAX_SNAPSHOT_RETRIES {
                                log::warn!(
                                    "OrderBook continuity break for {instrument_id}: \
                                    expected U={}, was U={}, triggering resync (attempt {}/{})",
                                    last_final_update_id + 1,
                                    update.first_update_id,
                                    retry_count + 1,
                                    MAX_SNAPSHOT_RETRIES
                                );

                                Self::reset_book_sync_buffer(&buffers, instrument_id, epoch);
                                tokio::time::sleep(spot_snapshot_retry_backoff(retry_count)).await;

                                Box::pin(Self::fetch_and_emit_snapshot_inner(
                                    http,
                                    sender,
                                    buffers,
                                    instruments,
                                    instrument_id,
                                    epoch,
                                    clock,
                                    retry_count + 1,
                                ))
                                .await;
                                return;
                            }
                            log::error!(
                                "OrderBook continuity break for {instrument_id} after \
                                {MAX_SNAPSHOT_RETRIES} retries; no deltas will be emitted \
                                until resubscribe or reconnect"
                            );
                            Self::mark_book_sync_failed(&buffers, instrument_id, epoch);
                            return;
                        }

                        last_final_update_id = update.final_update_id;
                        is_first = false;
                        replayed += 1;

                        if let Err(e) = sender.send(DataEvent::Data(Data::Deltas(
                            OrderBookDeltas_API::new(update.deltas),
                        ))) {
                            log::error!("Failed to send replayed deltas: {e}");
                        }
                    }
                }

                log::debug!(
                    "OrderBook snapshot rebuild for {instrument_id} completed \
                    (lastUpdateId={last_update_id}, replayed={replayed})"
                );
            }
            Err(e) => {
                if retry_count < MAX_SNAPSHOT_RETRIES {
                    log::warn!(
                        "Failed to request order book snapshot for {instrument_id}: {e}; \
                        retrying snapshot (attempt {}/{})",
                        retry_count + 1,
                        MAX_SNAPSHOT_RETRIES
                    );

                    tokio::time::sleep(spot_snapshot_retry_backoff(retry_count)).await;

                    Box::pin(Self::fetch_and_emit_snapshot_inner(
                        http,
                        sender,
                        buffers,
                        instruments,
                        instrument_id,
                        epoch,
                        clock,
                        retry_count + 1,
                    ))
                    .await;
                    return;
                }

                log::error!(
                    "Failed to request order book snapshot for {instrument_id} after \
                    {MAX_SNAPSHOT_RETRIES} retries: {e}; no deltas will be emitted until \
                    resubscribe or reconnect"
                );
                Self::mark_book_sync_failed(&buffers, instrument_id, epoch);
            }
        }
    }

    async fn wait_for_buffered_update(
        buffers: &Arc<AtomicMap<InstrumentId, BookBuffer>>,
        instrument_id: InstrumentId,
        epoch: u64,
    ) -> Option<()> {
        loop {
            let guard = buffers.load();
            match guard.get(&instrument_id) {
                Some(buffer)
                    if buffer.epoch == epoch
                        && buffer.status == BookSyncStatus::Buffering
                        && !buffer.updates.is_empty() =>
                {
                    return Some(());
                }
                Some(buffer)
                    if buffer.epoch == epoch && buffer.status == BookSyncStatus::Buffering => {}
                _ => return None,
            }

            drop(guard);
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    async fn wait_for_first_applicable_update(
        buffers: &Arc<AtomicMap<InstrumentId, BookBuffer>>,
        instrument_id: InstrumentId,
        epoch: u64,
        last_update_id: u64,
    ) -> Option<BufferedDepthUpdate> {
        loop {
            let mut first = None;
            let mut waiting = false;
            buffers.rcu(|m| {
                first = None;
                waiting = false;

                if let Some(buffer) = m.get_mut(&instrument_id)
                    && buffer.epoch == epoch
                    && buffer.status == BookSyncStatus::Buffering
                {
                    buffer
                        .updates
                        .retain(|update| update.final_update_id > last_update_id);
                    first = first_applicable_spot_update(&buffer.updates, last_update_id).cloned();
                    waiting = first.is_none();
                }
            });

            if first.is_some() {
                return first;
            }

            if !waiting {
                return None;
            }

            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    fn take_buffered_depth_updates(
        buffers: &Arc<AtomicMap<InstrumentId, BookBuffer>>,
        instrument_id: InstrumentId,
        epoch: u64,
    ) -> Option<Vec<BufferedDepthUpdate>> {
        let mut taken = None;
        buffers.rcu(|m| {
            taken = None;

            if let Some(buffer) = m.get_mut(&instrument_id)
                && buffer.epoch == epoch
                && buffer.status == BookSyncStatus::Buffering
            {
                taken = Some(std::mem::take(&mut buffer.updates));
            }
        });
        taken
    }

    fn drain_buffered_depth_updates(
        buffers: &Arc<AtomicMap<InstrumentId, BookBuffer>>,
        instrument_id: InstrumentId,
        epoch: u64,
    ) -> Option<Vec<BufferedDepthUpdate>> {
        let mut taken = None;
        buffers.rcu(|m| {
            taken = None;

            if let Some(buffer) = m.get_mut(&instrument_id)
                && buffer.epoch == epoch
                && buffer.status == BookSyncStatus::Buffering
            {
                if buffer.updates.is_empty() {
                    m.remove(&instrument_id);
                } else {
                    taken = Some(std::mem::take(&mut buffer.updates));
                }
            }
        });
        taken
    }

    fn reset_book_sync_buffer(
        buffers: &Arc<AtomicMap<InstrumentId, BookBuffer>>,
        instrument_id: InstrumentId,
        epoch: u64,
    ) {
        buffers.rcu(|m| {
            if let Some(buffer) = m.get_mut(&instrument_id)
                && buffer.epoch == epoch
            {
                buffer.updates.clear();
                buffer.status = BookSyncStatus::Buffering;
            }
        });
    }

    fn mark_book_sync_failed(
        buffers: &Arc<AtomicMap<InstrumentId, BookBuffer>>,
        instrument_id: InstrumentId,
        epoch: u64,
    ) {
        buffers.rcu(|m| {
            if let Some(buffer) = m.get_mut(&instrument_id)
                && buffer.epoch == epoch
            {
                buffer.updates.clear();
                buffer.status = BookSyncStatus::Failed;
            }
        });
    }
}

fn spot_ticker_data_type(instrument_id: InstrumentId) -> DataType {
    let mut metadata = Params::new();
    metadata.insert(
        "instrument_id".to_string(),
        serde_json::Value::String(instrument_id.to_string()),
    );
    DataType::new(
        "BinanceSpotTicker",
        Some(metadata),
        Some(instrument_id.to_string()),
    )
}

fn upsert_instrument(
    cache: &Arc<AtomicMap<InstrumentId, InstrumentAny>>,
    instrument: InstrumentAny,
) {
    cache.insert(instrument.id(), instrument);
}

// Spot requires first diff to overlap the REST snapshot: `U <= lastUpdateId + 1 <= u`.
fn spot_overlap_valid(first_update_id: u64, final_update_id: u64, last_update_id: u64) -> bool {
    let target = last_update_id + 1;
    first_update_id <= target && final_update_id >= target
}

// After the first applied diff, each spot update must satisfy `U == previous u + 1`.
fn spot_continuity_ok(is_first: bool, first_update_id: u64, prev_final_update_id: u64) -> bool {
    is_first || first_update_id == prev_final_update_id + 1
}

fn spot_snapshot_retry_backoff(retry_count: u32) -> Duration {
    let multiplier = 1_u64 << retry_count.min(4);
    let millis = SNAPSHOT_RETRY_BACKOFF_BASE_MS
        .saturating_mul(multiplier)
        .min(SNAPSHOT_RETRY_BACKOFF_CAP_MS);
    Duration::from_millis(millis)
}

fn first_applicable_spot_update(
    updates: &[BufferedDepthUpdate],
    last_update_id: u64,
) -> Option<&BufferedDepthUpdate> {
    updates
        .iter()
        .find(|update| update.final_update_id > last_update_id)
}

fn trim_buffered_depth_updates(updates: &mut Vec<BufferedDepthUpdate>) {
    let excess = updates.len().saturating_sub(MAX_BUFFERED_DEPTH_UPDATES);
    if excess > 0 {
        updates.drain(..excess);
    }
}

fn parse_spot_depth_snapshot(
    depth: &BinanceDepth,
    instrument_id: InstrumentId,
    price_precision: u8,
    size_precision: u8,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
) -> anyhow::Result<Option<OrderBookDeltas>> {
    let sequence = depth.last_update_id as u64;

    let total_levels = depth.bids.len() + depth.asks.len();
    let mut deltas = Vec::with_capacity(total_levels + 1);

    // REST snapshots carry no event time; use the caller's best venue-time estimate.
    deltas.push(OrderBookDelta::clear(
        instrument_id,
        sequence,
        ts_event,
        ts_init,
    ));

    for (i, level) in depth.bids.iter().enumerate() {
        let price = Price::from_mantissa_exponent_checked(
            level.price_mantissa,
            depth.price_exponent,
            price_precision,
        )?;
        let size = Quantity::from_mantissa_exponent_checked(
            level.qty_mantissa as u64,
            depth.qty_exponent,
            size_precision,
        )?;
        let flags = if i == depth.bids.len() - 1 && depth.asks.is_empty() {
            RecordFlag::F_LAST as u8
        } else {
            0
        };

        let order = BookOrder::new(OrderSide::Buy, price, size, 0);

        deltas.push(OrderBookDelta::new(
            instrument_id,
            BookAction::Add,
            order,
            flags,
            sequence,
            ts_event,
            ts_init,
        ));
    }

    for (i, level) in depth.asks.iter().enumerate() {
        let price = Price::from_mantissa_exponent_checked(
            level.price_mantissa,
            depth.price_exponent,
            price_precision,
        )?;
        let size = Quantity::from_mantissa_exponent_checked(
            level.qty_mantissa as u64,
            depth.qty_exponent,
            size_precision,
        )?;
        let flags = if i == depth.asks.len() - 1 {
            RecordFlag::F_LAST as u8
        } else {
            0
        };

        let order = BookOrder::new(OrderSide::Sell, price, size, 0);

        deltas.push(OrderBookDelta::new(
            instrument_id,
            BookAction::Add,
            order,
            flags,
            sequence,
            ts_event,
            ts_init,
        ));
    }

    if deltas.len() <= 1 {
        return Ok(None);
    }

    Ok(Some(OrderBookDeltas::new(instrument_id, deltas)))
}

#[async_trait::async_trait(?Send)]
impl DataClient for BinanceSpotDataClient {
    fn client_id(&self) -> ClientId {
        self.client_id
    }

    fn venue(&self) -> Option<Venue> {
        Some(self.venue())
    }

    fn start(&mut self) -> anyhow::Result<()> {
        log::info!(
            "Started: client_id={}, product_type={:?}, environment={:?}",
            self.client_id,
            self.config.product_type,
            self.config.environment,
        );
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        log::info!("Stopping {id}", id = self.client_id);
        self.cancellation_token.cancel();
        self.is_connected.store(false, Ordering::Relaxed);
        Ok(())
    }

    fn reset(&mut self) -> anyhow::Result<()> {
        log::debug!("Resetting {id}", id = self.client_id);

        self.cancellation_token.cancel();

        for task in self.tasks.drain(..) {
            task.abort();
        }

        let mut ws = self.ws_client.clone();
        get_runtime().spawn(async move {
            let _ = ws.close().await;
        });

        self.book_subscriptions.store(AHashMap::new());
        self.l1_book_subscriptions.store(AHashMap::new());
        self.quote_refs.store(AHashMap::new());
        self.ticker_refs.store(AHashMap::new());
        self.book_buffers.store(AHashMap::new());

        self.is_connected.store(false, Ordering::Relaxed);
        self.cancellation_token = CancellationToken::new();
        Ok(())
    }

    fn dispose(&mut self) -> anyhow::Result<()> {
        log::debug!("Disposing {id}", id = self.client_id);
        self.stop()
    }

    async fn connect(&mut self) -> anyhow::Result<()> {
        if self.is_connected() {
            return Ok(());
        }

        register_binance_custom_data();

        if self.spot_market_data_mode == BinanceSpotMarketDataMode::Sbe
            && !self.ws_client.has_credentials()
        {
            anyhow::bail!(
                "Binance Spot market data mode SBE requires Ed25519 API credentials. \
                 Set the appropriate env vars for your environment, \
                 or provide api_key/api_secret in the data client config"
            );
        }

        // Reinitialize token in case of reconnection after disconnect
        self.cancellation_token = CancellationToken::new();

        Self::refresh_instrument_catalogue(
            &self.http_client,
            &self.config.instrument_provider,
            self.config.us,
            &self.instruments,
            &self.status_cache,
            &self.ws_client,
            &self.data_sender,
            self.clock,
            false,
        )
        .await?;

        match &mut self.ws_client {
            SpotWsClient::Sbe(ws_client) => {
                log::info!("Connecting to Binance Spot SBE WebSocket...");
                ws_client.connect().await.map_err(|e| {
                    log::error!("Binance Spot SBE WebSocket connection failed: {e:?}");
                    anyhow::anyhow!("failed to connect Binance Spot SBE WebSocket: {e}")
                })?;
                log::info!("Binance Spot SBE WebSocket connected");

                let stream = ws_client.stream();
                let sender = self.data_sender.clone();
                let insts = self.instruments.clone();
                let ws_insts = ws_client.instruments_cache();
                let buffers = self.book_buffers.clone();
                let book_subs = self.book_subscriptions.clone();
                let l1_book_subs = self.l1_book_subscriptions.clone();
                let book_epoch = self.book_epoch.clone();
                let http = self.http_client.clone();
                let clock = self.clock;
                let cancel = self.cancellation_token.clone();

                let handle = get_runtime().spawn(async move {
                    pin_mut!(stream);

                    loop {
                        tokio::select! {
                            Some(message) = stream.next() => {
                                Self::handle_ws_message(
                                    message,
                                    &sender,
                                    &insts,
                                    &ws_insts,
                                    &buffers,
                                    &book_subs,
                                    &l1_book_subs,
                                    &book_epoch,
                                    &http,
                                    clock,
                                );
                            }
                            () = cancel.cancelled() => {
                                log::debug!("Spot SBE WebSocket stream task cancelled");
                                break;
                            }
                        }
                    }
                });
                self.tasks.push(handle);
            }
            SpotWsClient::JsonPublic(ws_client) => {
                log::info!("Connecting to Binance Spot public JSON WebSocket...");
                ws_client.connect().await.map_err(|e| {
                    log::error!("Binance Spot public JSON WebSocket connection failed: {e:?}");
                    anyhow::anyhow!("failed to connect Binance Spot public JSON WebSocket: {e}")
                })?;
                log::info!("Binance Spot public JSON WebSocket connected");

                let stream = ws_client.stream();
                let sender = self.data_sender.clone();
                let insts = self.instruments.clone();
                let ws_insts = ws_client.instruments_cache();
                let buffers = self.book_buffers.clone();
                let book_subs = self.book_subscriptions.clone();
                let l1_book_subs = self.l1_book_subscriptions.clone();
                let book_epoch = self.book_epoch.clone();
                let http = self.http_client.clone();
                let clock = self.clock;
                let cancel = self.cancellation_token.clone();

                let handle = get_runtime().spawn(async move {
                    pin_mut!(stream);

                    loop {
                        tokio::select! {
                            Some(message) = stream.next() => {
                                Self::handle_public_json_ws_message(
                                    message,
                                    &sender,
                                    &insts,
                                    &ws_insts,
                                    &buffers,
                                    &book_subs,
                                    &l1_book_subs,
                                    &book_epoch,
                                    &http,
                                    clock,
                                );
                            }
                            () = cancel.cancelled() => {
                                log::debug!("Spot JSON WebSocket stream task cancelled");
                                break;
                            }
                        }
                    }
                });
                self.tasks.push(handle);
            }
        }

        // Spawn instrument status polling task
        let poll_secs = self.config.instrument_status_poll_secs;
        if poll_secs > 0 {
            let http = self.http_client.clone();
            let poll_sender = self.data_sender.clone();
            let poll_instruments = self.instruments.clone();
            let poll_status_cache = self.status_cache.clone();
            let poll_cancel = self.cancellation_token.clone();
            let clock = self.clock;
            let us = self.config.us;

            let poll_handle = get_runtime().spawn(async move {
                let mut interval =
                    tokio::time::interval(tokio::time::Duration::from_secs(poll_secs));
                interval.tick().await; // Skip first immediate tick

                loop {
                    tokio::select! {
                        _ = interval.tick() => {
                            match http.request_symbol_statuses(us).await {
                                Ok(statuses) => {
                                    let ts = clock.get_time_ns();
                                    let inst_guard = poll_instruments.load();
                                    let new_statuses = statuses
                                        .into_iter()
                                        .filter(|(instrument_id, _)| {
                                            inst_guard.contains_key(instrument_id)
                                        })
                                        .collect();
                                    drop(inst_guard);

                                    let mut cache =
                                        (**poll_status_cache.load()).clone();
                                    diff_and_emit_statuses(
                                        &new_statuses, &mut cache, &poll_sender, ts, ts,
                                    );
                                    poll_status_cache.store(cache);
                                }
                                Err(e) => {
                                    log::warn!("Instrument status poll failed: {e}");
                                }
                            }
                        }
                        () = poll_cancel.cancelled() => {
                            log::debug!("Instrument status polling task cancelled");
                            break;
                        }
                    }
                }
            });
            self.tasks.push(poll_handle);
            log::debug!("Instrument status polling started: interval={poll_secs}s");
        }

        let refresh_secs = self.config.instrument_refresh_interval_secs;
        if refresh_secs > 0 {
            let http = self.http_client.clone();
            let provider = self.config.instrument_provider.clone();
            let us = self.config.us;
            let instruments = self.instruments.clone();
            let statuses = self.status_cache.clone();
            let ws = self.ws_client.clone();
            let sender = self.data_sender.clone();
            let clock = self.clock;
            let cancel = self.cancellation_token.clone();

            let refresh_handle = get_runtime().spawn(async move {
                let mut interval = tokio::time::interval(Duration::from_secs(refresh_secs));
                interval.tick().await;

                loop {
                    tokio::select! {
                        _ = interval.tick() => {
                            if let Err(e) = Self::refresh_instrument_catalogue(
                                &http,
                                &provider,
                                us,
                                &instruments,
                                &statuses,
                                &ws,
                                &sender,
                                clock,
                                true,
                            ).await {
                                log::warn!("Binance Spot instrument refresh failed: {e}");
                            }
                        }
                        () = cancel.cancelled() => {
                            log::debug!("Binance Spot instrument refresh task cancelled");
                            break;
                        }
                    }
                }
            });
            self.tasks.push(refresh_handle);
            log::debug!("Instrument refresh started: interval={refresh_secs}s");
        }

        self.is_connected.store(true, Ordering::Release);
        log::info!("Connected: client_id={}", self.client_id);
        Ok(())
    }

    async fn disconnect(&mut self) -> anyhow::Result<()> {
        if self.is_disconnected() {
            return Ok(());
        }

        self.cancellation_token.cancel();

        let _ = self.ws_client.close().await;

        let handles: Vec<_> = std::mem::take(&mut self.tasks);
        for handle in handles {
            if let Err(e) = handle.await {
                log::error!("Error joining WebSocket task: {e}");
            }
        }

        self.book_subscriptions.store(AHashMap::new());
        self.l1_book_subscriptions.store(AHashMap::new());
        self.quote_refs.store(AHashMap::new());
        self.ticker_refs.store(AHashMap::new());
        self.book_buffers.store(AHashMap::new());

        self.is_connected.store(false, Ordering::Release);
        log::info!("Disconnected: client_id={}", self.client_id);
        Ok(())
    }

    fn is_connected(&self) -> bool {
        self.is_connected.load(Ordering::Relaxed)
    }

    fn is_disconnected(&self) -> bool {
        !self.is_connected()
    }

    fn subscribe(&mut self, cmd: SubscribeCustomData) -> anyhow::Result<()> {
        if cmd.data_type.type_name() != "BinanceSpotTicker" {
            log::warn!(
                "Unsupported custom data subscription: {}",
                cmd.data_type.type_name()
            );
            return Ok(());
        }
        anyhow::ensure!(
            self.spot_market_data_mode == BinanceSpotMarketDataMode::Json,
            "Binance Spot 24-hour ticker custom data requires JSON market-data mode"
        );
        let instrument_id = Self::required_instrument_id_metadata(&cmd.data_type)?;
        anyhow::ensure!(
            instrument_id.venue == self.venue(),
            "Spot ticker requires a BINANCE instrument"
        );
        let should_subscribe = {
            let previous = self
                .ticker_refs
                .load()
                .get(&instrument_id)
                .copied()
                .unwrap_or(0);
            self.ticker_refs
                .rcu(|refs| *refs.entry(instrument_id).or_insert(0) += 1);
            previous == 0
        };

        if should_subscribe {
            let ws = self.ws_client.clone();
            let stream = format!("{}@ticker", instrument_id.symbol.as_str().to_lowercase());
            self.spawn_ws(
                async move {
                    ws.subscribe(vec![stream])
                        .await
                        .context("ticker subscription")
                },
                "ticker subscription",
            );
        }
        Ok(())
    }

    fn subscribe_instruments(&mut self, _cmd: SubscribeInstruments) -> anyhow::Result<()> {
        log::debug!("subscribe_instruments: Binance instruments are fetched via HTTP on connect");
        Ok(())
    }

    fn subscribe_instrument(&mut self, _cmd: SubscribeInstrument) -> anyhow::Result<()> {
        log::debug!("subscribe_instrument: Binance instruments are fetched via HTTP on connect");
        Ok(())
    }

    fn subscribe_book_deltas(&mut self, cmd: SubscribeBookDeltas) -> anyhow::Result<()> {
        if cmd.book_type == BookType::L1_MBP {
            anyhow::ensure!(
                cmd.depth.is_none_or(|depth| depth.get() == 1),
                "Binance Spot L1_MBP supports depth 1 only"
            );
            anyhow::ensure!(
                !self.book_subscriptions.contains_key(&cmd.instrument_id),
                "cannot subscribe L1_MBP and L2_MBP for the same Binance Spot instrument"
            );
            self.l1_book_subscriptions.rcu(|subscriptions| {
                *subscriptions.entry(cmd.instrument_id).or_insert(0) += 1;
            });
            self.subscribe_top_of_book(cmd.instrument_id);
            return Ok(());
        }

        if cmd.book_type != BookType::L2_MBP {
            anyhow::bail!("Binance Spot supports L1_MBP and L2_MBP order book subscriptions");
        }
        anyhow::ensure!(
            !self.l1_book_subscriptions.contains_key(&cmd.instrument_id),
            "cannot subscribe L1_MBP and L2_MBP for the same Binance Spot instrument"
        );

        let instrument_id = cmd.instrument_id;
        let ws = self.ws_client.clone();
        let symbol_lower = instrument_id.symbol.as_str().to_lowercase();

        if self.spot_market_data_mode == BinanceSpotMarketDataMode::Json && cmd.depth.is_some() {
            // Explicit depth requests use partial-book streams. Full-depth JSON
            // subscriptions fall through to the REST snapshot + @depth diff path.
            let depth_level = match cmd.depth.map(|d| d.get()) {
                Some(1..=5) => 5,
                Some(6..=10) => 10,
                _ => 20,
            };
            self.book_subscriptions.insert(instrument_id, depth_level);

            let stream = format!("{symbol_lower}@depth{depth_level}");
            self.spawn_ws(
                async move {
                    ws.subscribe(vec![stream])
                        .await
                        .context("book deltas subscription")
                },
                "order book subscription",
            );
            return Ok(());
        }

        match cmd.depth.map(|d| d.get()) {
            // Partial book streams are self-contained snapshots.
            Some(depth) => {
                let depth_level = match depth {
                    1..=5 => 5,
                    6..=10 => 10,
                    _ => 20,
                };
                self.book_subscriptions.insert(instrument_id, depth_level);

                let stream = format!("{symbol_lower}@depth{depth_level}");
                self.spawn_ws(
                    async move {
                        ws.subscribe(vec![stream])
                            .await
                            .context("book deltas subscription")
                    },
                    "order book subscription",
                );
            }
            // Full book diffs are seeded by a REST snapshot and replayed.
            None => {
                self.book_subscriptions.insert(instrument_id, 0);

                // Bump epoch to invalidate any in-flight snapshot from a prior subscription
                let epoch = {
                    let mut guard = self.book_epoch.write().expect(MUTEX_POISONED);
                    *guard = guard.wrapping_add(1);
                    *guard
                };

                // Start buffering diffs before the snapshot lands
                self.book_buffers
                    .insert(instrument_id, BookBuffer::new(epoch));

                log::debug!("OrderBook full snapshot rebuild for {instrument_id} starting");

                let stream = format!("{symbol_lower}@depth");
                self.spawn_ws(
                    async move {
                        ws.subscribe(vec![stream])
                            .await
                            .context("book deltas subscription")
                    },
                    "order book subscription",
                );

                let http = self.http_client.clone();
                let sender = self.data_sender.clone();
                let buffers = self.book_buffers.clone();
                let instruments = self.instruments.clone();
                let clock = self.clock;

                get_runtime().spawn(async move {
                    Self::fetch_and_emit_snapshot(
                        http,
                        sender,
                        buffers,
                        instruments,
                        instrument_id,
                        epoch,
                        clock,
                    )
                    .await;
                });
            }
        }
        Ok(())
    }

    fn subscribe_quotes(&mut self, cmd: SubscribeQuotes) -> anyhow::Result<()> {
        self.subscribe_top_of_book(cmd.instrument_id);
        Ok(())
    }

    fn subscribe_trades(&mut self, cmd: SubscribeTrades) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;
        let ws = self.ws_client.clone();

        let stream = format!("{}@trade", instrument_id.symbol.as_str().to_lowercase());

        self.spawn_ws(
            async move {
                ws.subscribe(vec![stream])
                    .await
                    .context("trades subscription")
            },
            "trade subscription",
        );
        Ok(())
    }

    fn subscribe_bars(&mut self, cmd: SubscribeBars) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.spot_market_data_mode == BinanceSpotMarketDataMode::Json,
            "Binance Spot kline subscriptions require JSON market-data mode"
        );
        let bar_type = cmd.bar_type;
        let ws = self.ws_client.clone();
        let interval = bar_spec_to_binance_interval(bar_type.spec())?;

        let stream = format!(
            "{}@kline_{}",
            bar_type.instrument_id().symbol.as_str().to_lowercase(),
            interval.as_str()
        );

        self.spawn_ws(
            async move {
                ws.subscribe(vec![stream])
                    .await
                    .context("bars subscription")
            },
            "bar subscription",
        );
        Ok(())
    }

    fn subscribe_instrument_status(
        &mut self,
        cmd: SubscribeInstrumentStatus,
    ) -> anyhow::Result<()> {
        log::debug!(
            "subscribe_instrument_status: {id} (status changes detected via periodic exchange info polling)",
            id = cmd.instrument_id,
        );
        Ok(())
    }

    fn unsubscribe_book_deltas(&mut self, cmd: &UnsubscribeBookDeltas) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;

        if let Some(count) = self
            .l1_book_subscriptions
            .load()
            .get(&instrument_id)
            .copied()
        {
            if count == 1 {
                self.l1_book_subscriptions.remove(&instrument_id);
            } else {
                self.l1_book_subscriptions.rcu(|subscriptions| {
                    if let Some(existing) = subscriptions.get_mut(&instrument_id) {
                        *existing -= 1;
                    }
                });
            }
            self.unsubscribe_top_of_book(instrument_id);
            return Ok(());
        }
        let ws = self.ws_client.clone();

        // Stop buffering/tracking so any in-flight snapshot task is discarded
        self.book_subscriptions.remove(&instrument_id);
        self.book_buffers.remove(&instrument_id);

        let symbol_lower = instrument_id.symbol.as_str().to_lowercase();
        let streams = vec![
            format!("{symbol_lower}@depth"),
            format!("{symbol_lower}@depth5"),
            format!("{symbol_lower}@depth10"),
            format!("{symbol_lower}@depth20"),
        ];

        self.spawn_ws(
            async move {
                ws.unsubscribe(streams)
                    .await
                    .context("book deltas unsubscribe")
            },
            "order book unsubscribe",
        );
        Ok(())
    }

    fn unsubscribe_quotes(&mut self, cmd: &UnsubscribeQuotes) -> anyhow::Result<()> {
        self.unsubscribe_top_of_book(cmd.instrument_id);
        Ok(())
    }

    fn unsubscribe(&mut self, cmd: &UnsubscribeCustomData) -> anyhow::Result<()> {
        if cmd.data_type.type_name() != "BinanceSpotTicker" {
            log::warn!(
                "Unsupported custom data unsubscription: {}",
                cmd.data_type.type_name()
            );
            return Ok(());
        }
        let instrument_id = Self::required_instrument_id_metadata(&cmd.data_type)?;
        let should_unsubscribe = match self.ticker_refs.load().get(&instrument_id).copied() {
            Some(1) => {
                self.ticker_refs.remove(&instrument_id);
                true
            }
            Some(count) if count > 1 => {
                self.ticker_refs.rcu(|refs| {
                    if let Some(existing) = refs.get_mut(&instrument_id) {
                        *existing -= 1;
                    }
                });
                false
            }
            _ => false,
        };

        if should_unsubscribe {
            let ws = self.ws_client.clone();
            let stream = format!("{}@ticker", instrument_id.symbol.as_str().to_lowercase());
            self.spawn_ws(
                async move {
                    ws.unsubscribe(vec![stream])
                        .await
                        .context("ticker unsubscribe")
                },
                "ticker unsubscribe",
            );
        }
        Ok(())
    }

    fn unsubscribe_trades(&mut self, cmd: &UnsubscribeTrades) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;
        let ws = self.ws_client.clone();

        let stream = format!("{}@trade", instrument_id.symbol.as_str().to_lowercase());

        self.spawn_ws(
            async move {
                ws.unsubscribe(vec![stream])
                    .await
                    .context("trades unsubscribe")
            },
            "trade unsubscribe",
        );
        Ok(())
    }

    fn unsubscribe_bars(&mut self, cmd: &UnsubscribeBars) -> anyhow::Result<()> {
        let bar_type = cmd.bar_type;
        let ws = self.ws_client.clone();
        let interval = bar_spec_to_binance_interval(bar_type.spec())?;

        let stream = format!(
            "{}@kline_{}",
            bar_type.instrument_id().symbol.as_str().to_lowercase(),
            interval.as_str()
        );

        self.spawn_ws(
            async move {
                ws.unsubscribe(vec![stream])
                    .await
                    .context("bars unsubscribe")
            },
            "bar unsubscribe",
        );
        Ok(())
    }

    fn unsubscribe_instrument_status(
        &mut self,
        cmd: &UnsubscribeInstrumentStatus,
    ) -> anyhow::Result<()> {
        log::debug!(
            "unsubscribe_instrument_status: {id}",
            id = cmd.instrument_id,
        );
        Ok(())
    }

    fn request_instruments(&self, request: RequestInstruments) -> anyhow::Result<()> {
        let http = self.http_client.clone();
        let sender = self.data_sender.clone();
        let instruments_cache = self.instruments.clone();
        let request_id = request.request_id;
        let client_id = request.client_id.unwrap_or(self.client_id);
        let venue = self.venue();
        let start = request.start;
        let end = request.end;
        let params = request.params;
        let clock = self.clock;
        let provider = self.config.instrument_provider.clone();
        let us = self.config.us;
        let start_nanos = datetime_to_unix_nanos(start);
        let end_nanos = datetime_to_unix_nanos(end);

        get_runtime().spawn(async move {
            match http.request_instruments_with_config(&provider, us).await {
                Ok(instruments) => {
                    for instrument in &instruments {
                        upsert_instrument(&instruments_cache, instrument.clone());
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
                Err(e) => log::error!("Instruments request failed: {e:?}"),
            }
        });

        Ok(())
    }

    fn request_instrument(&self, request: RequestInstrument) -> anyhow::Result<()> {
        let http = self.http_client.clone();
        let sender = self.data_sender.clone();
        let instruments = self.instruments.clone();
        let instrument_id = request.instrument_id;
        let request_id = request.request_id;
        let client_id = request.client_id.unwrap_or(self.client_id);
        let start = request.start;
        let end = request.end;
        let params = request.params;
        let clock = self.clock;
        let provider = self.config.instrument_provider.clone();
        let us = self.config.us;
        let start_nanos = datetime_to_unix_nanos(start);
        let end_nanos = datetime_to_unix_nanos(end);

        get_runtime().spawn(async move {
            match http.request_instruments_with_config(&provider, us).await {
                Ok(all_instruments) => {
                    for instrument in &all_instruments {
                        upsert_instrument(&instruments, instrument.clone());
                    }

                    let instrument = all_instruments
                        .into_iter()
                        .find(|i| i.id() == instrument_id);

                    if let Some(instrument) = instrument {
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
                    } else {
                        log::error!("Instrument not found: {instrument_id}");
                    }
                }
                Err(e) => log::error!("Instrument request failed: {e:?}"),
            }
        });

        Ok(())
    }

    fn request_data(&self, request: RequestCustomData) -> anyhow::Result<()> {
        if request.data_type.type_name() != "BinanceBar" {
            log::warn!(
                "Unsupported custom data request: {}",
                request.data_type.type_name()
            );
            return Ok(());
        }
        let bar_type = parse_binance_bar_type(&request.data_type)?;
        anyhow::ensure!(
            bar_type.aggregation_source() == AggregationSource::External,
            "historical BinanceBar requests require EXTERNAL aggregation"
        );
        anyhow::ensure!(
            bar_type.spec().price_type == PriceType::Last,
            "historical BinanceBar requests require LAST price type"
        );
        anyhow::ensure!(
            bar_type.spec().is_time_aggregated(),
            "historical BinanceBar requests require time aggregation"
        );
        let http = self.http_client.clone();
        let sender = self.data_sender.clone();
        let request_id = request.request_id;
        let client_id = request.client_id;
        let data_type = request.data_type;
        let start = request.start;
        let end = request.end;
        let limit = request.limit.map(|value| value.get() as u32);
        let params = request.params;
        let clock = self.clock;
        let venue = self.venue();
        let start_nanos = datetime_to_unix_nanos(start);
        let end_nanos = datetime_to_unix_nanos(end);

        get_runtime().spawn(async move {
            match http.request_binance_bars(bar_type, start, end, limit).await {
                Ok(bars) => {
                    let response = DataResponse::Data(CustomDataResponse::new(
                        request_id,
                        client_id,
                        Some(venue),
                        data_type,
                        bars,
                        start_nanos,
                        end_nanos,
                        clock.get_time_ns(),
                        params,
                    ));

                    if let Err(e) = sender.send(DataEvent::Response(response)) {
                        log::error!("Failed to send BinanceBar response: {e}");
                    }
                }
                Err(e) => log::error!("BinanceBar request failed for {bar_type}: {e:?}"),
            }
        });
        Ok(())
    }

    fn request_trades(&self, request: RequestTrades) -> anyhow::Result<()> {
        let http = self.http_client.clone();
        let sender = self.data_sender.clone();
        let instrument_id = request.instrument_id;
        let limit = request.limit.map(|n| n.get() as u32);
        let request_id = request.request_id;
        let client_id = request.client_id.unwrap_or(self.client_id);
        let params = request.params;
        let clock = self.clock;
        let start_nanos = datetime_to_unix_nanos(request.start);
        let end_nanos = datetime_to_unix_nanos(request.end);
        let start = request.start;
        let end = request.end;
        anyhow::ensure!(
            limit.is_none_or(|value| value <= 1000),
            "Binance Spot trade limit must not exceed 1000"
        );

        get_runtime().spawn(async move {
            let result = if start.is_some() || end.is_some() {
                http.request_agg_trades(instrument_id, start, end, limit)
                    .await
            } else {
                http.request_trades(instrument_id, limit).await
            };

            match result.context("failed to request trades from Binance") {
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
                Err(e) => log::error!("Trade request failed: {e:?}"),
            }
        });

        Ok(())
    }

    fn request_bars(&self, request: RequestBars) -> anyhow::Result<()> {
        let http = self.http_client.clone();
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
        anyhow::ensure!(
            bar_type.aggregation_source() == AggregationSource::External,
            "Binance historical bars require EXTERNAL aggregation"
        );
        anyhow::ensure!(
            bar_type.spec().price_type == PriceType::Last,
            "Binance historical bars require LAST price type"
        );
        anyhow::ensure!(
            bar_type.spec().is_time_aggregated(),
            "Binance historical bars require time aggregation"
        );

        get_runtime().spawn(async move {
            let result = http.request_bars(bar_type, start, end, limit).await;

            match result.context("failed to request bars from Binance") {
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
                Err(e) => log::error!("Bar request failed: {e:?}"),
            }
        });

        Ok(())
    }

    fn request_book_snapshot(&self, request: RequestBookSnapshot) -> anyhow::Result<()> {
        let depth = request.depth.map(|value| value.get() as u32);
        anyhow::ensure!(
            depth.is_none_or(|value| (1..=5000).contains(&value)),
            "Binance Spot order-book depth must be between 1 and 5000"
        );
        let http = self.http_client.clone();
        let sender = self.data_sender.clone();
        let instrument_id = request.instrument_id;
        let request_id = request.request_id;
        let client_id = request.client_id.unwrap_or(self.client_id);
        let params = request.params;
        let clock = self.clock;

        get_runtime().spawn(async move {
            match http.request_book_snapshot(instrument_id, depth).await {
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
                Err(e) => log::error!("Book snapshot request failed for {instrument_id}: {e:?}"),
            }
        });
        Ok(())
    }
}

impl BinanceSpotDataClient {
    fn subscribe_top_of_book(&self, instrument_id: InstrumentId) {
        let should_subscribe = {
            let previous = self
                .quote_refs
                .load()
                .get(&instrument_id)
                .copied()
                .unwrap_or(0);
            self.quote_refs.rcu(|refs| {
                *refs.entry(instrument_id).or_insert(0) += 1;
            });
            previous == 0
        };

        if should_subscribe {
            let ws = self.ws_client.clone();
            let suffix = self.quote_stream_suffix();
            let stream = format!("{}@{suffix}", instrument_id.symbol.as_str().to_lowercase());
            self.spawn_ws(
                async move {
                    ws.subscribe(vec![stream])
                        .await
                        .context("top-of-book subscription")
                },
                "top-of-book subscription",
            );
        }
    }

    fn unsubscribe_top_of_book(&self, instrument_id: InstrumentId) {
        let should_unsubscribe = match self.quote_refs.load().get(&instrument_id).copied() {
            Some(1) => {
                self.quote_refs.remove(&instrument_id);
                true
            }
            Some(count) if count > 1 => {
                self.quote_refs.rcu(|refs| {
                    if let Some(existing) = refs.get_mut(&instrument_id) {
                        *existing -= 1;
                    }
                });
                false
            }
            _ => false,
        };

        if should_unsubscribe {
            let ws = self.ws_client.clone();
            let suffix = self.quote_stream_suffix();
            let stream = format!("{}@{suffix}", instrument_id.symbol.as_str().to_lowercase());
            self.spawn_ws(
                async move {
                    ws.unsubscribe(vec![stream])
                        .await
                        .context("top-of-book unsubscribe")
                },
                "top-of-book unsubscribe",
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        sync::{Arc, RwLock},
        time::Duration,
    };

    use nautilus_common::messages::DataEvent;
    use nautilus_core::{AtomicMap, nanos::UnixNanos, time::AtomicTime};
    use nautilus_model::{
        data::{BookOrder, Data, OrderBookDelta, OrderBookDeltas},
        enums::{BookAction, OrderSide, RecordFlag},
        identifiers::InstrumentId,
        instruments::{Instrument, InstrumentAny, stubs::currency_pair_btcusdt},
        types::{Price, Quantity},
    };
    use rstest::rstest;
    use rust_decimal_macros::dec;
    use ustr::Ustr;

    use super::{
        BinanceDepth, BinanceEnvironment, BinanceSpotDataClient, BinanceSpotMarketDataMode,
        BookBuffer, BufferedDepthUpdate, first_applicable_spot_update, parse_spot_depth_snapshot,
        resolve_spot_json_ws_url, spot_continuity_ok, spot_overlap_valid,
        spot_snapshot_retry_backoff,
    };
    use crate::{
        common::consts::BINANCE_SPOT_WS_URL,
        spot::{
            http::{BinancePriceLevel, BinanceSpotHttpClient},
            sbe::stream::BestBidAskStreamEvent,
            websocket::streams::messages::BinanceSpotWsMessage,
        },
    };

    #[rstest]
    fn handle_ws_message_uses_clock_timestamp_for_sbe_bbo_ts_init() {
        let ts_init = UnixNanos::from(1_800_000_000_000_000_000u64);
        let clock = Box::leak(Box::new(AtomicTime::new(false, ts_init)));
        let instrument = InstrumentAny::CurrencyPair(currency_pair_btcusdt());
        let instruments = Arc::new(AtomicMap::new());
        instruments.insert(instrument.id(), instrument.clone());
        let ws_instruments = Arc::new(AtomicMap::new());
        ws_instruments.insert(Ustr::from("BTCUSDT"), instrument);
        let book_buffers = Arc::new(AtomicMap::<InstrumentId, BookBuffer>::new());
        let book_subscriptions = Arc::new(AtomicMap::<InstrumentId, u32>::new());
        let l1_book_subscriptions = Arc::new(AtomicMap::<InstrumentId, u32>::new());
        let book_epoch = Arc::new(RwLock::new(0));
        let http_client = BinanceSpotHttpClient::new(
            BinanceEnvironment::Testnet,
            clock,
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .unwrap();
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        let event_time_us = 1_700_000_000_000_000;
        let message = BinanceSpotWsMessage::BestBidAsk(BestBidAskStreamEvent {
            event_time_us,
            book_update_id: 123,
            price_exponent: -2,
            qty_exponent: -4,
            bid_price_mantissa: 12_345,
            bid_qty_mantissa: 25_000,
            ask_price_mantissa: 12_350,
            ask_qty_mantissa: 30_000,
            symbol: Ustr::from("BTCUSDT"),
        });

        BinanceSpotDataClient::handle_ws_message(
            message,
            &sender,
            &instruments,
            &ws_instruments,
            &book_buffers,
            &book_subscriptions,
            &l1_book_subscriptions,
            &book_epoch,
            &http_client,
            clock,
        );

        let DataEvent::Data(Data::Quote(quote)) = receiver.try_recv().unwrap() else {
            panic!("expected quote data event");
        };
        assert_eq!(quote.ts_event, UnixNanos::from_micros(event_time_us as u64));
        assert_eq!(quote.ts_init, ts_init);
    }

    #[rstest]
    fn overlap_accepts_first_diff_straddling_snapshot() {
        assert!(spot_overlap_valid(90, 110, 100));
        assert!(spot_overlap_valid(101, 101, 100));
        assert!(spot_overlap_valid(101, 200, 100));
    }

    #[rstest]
    fn overlap_rejects_gap_and_stale() {
        assert!(!spot_overlap_valid(103, 110, 100));
        assert!(!spot_overlap_valid(90, 100, 100));
    }

    #[rstest]
    fn continuity_skips_first_then_requires_contiguous_u() {
        assert!(spot_continuity_ok(true, 999, 100));
        assert!(spot_continuity_ok(false, 101, 100));
        assert!(!spot_continuity_ok(false, 102, 100));
        assert!(!spot_continuity_ok(false, 100, 100));
    }

    #[rstest]
    #[case(0, 250)]
    #[case(1, 500)]
    #[case(2, 1_000)]
    #[case(3, 2_000)]
    #[case(4, 3_000)]
    #[case(5, 3_000)]
    fn snapshot_retry_backoff_exponentially_increases_then_caps(
        #[case] retry_count: u32,
        #[case] expected_ms: u64,
    ) {
        assert_eq!(
            spot_snapshot_retry_backoff(retry_count),
            Duration::from_millis(expected_ms)
        );
    }

    #[rstest]
    fn first_applicable_update_skips_stale_diffs() {
        let updates = vec![
            buffered_update(90, 100),
            buffered_update(101, 101),
            buffered_update(102, 103),
        ];

        let update = first_applicable_spot_update(&updates, 100).unwrap();

        assert_eq!(update.first_update_id, 101);
        assert_eq!(update.final_update_id, 101);
        assert!(first_applicable_spot_update(&updates, 103).is_none());
    }

    #[rstest]
    fn parse_spot_depth_snapshot_sets_sequence_and_last_flag() {
        let instrument_id = InstrumentId::from("BTCUSDT.BINANCE");
        let depth = depth_snapshot(
            vec![price_level(10_000, 1_000)],
            vec![price_level(10_100, 2_000)],
        );

        let deltas = parse_spot_depth_snapshot(
            &depth,
            instrument_id,
            2,
            3,
            UnixNanos::from(1),
            UnixNanos::from(2),
        )
        .unwrap()
        .unwrap();

        assert_eq!(deltas.deltas.len(), 3);
        assert_eq!(deltas.deltas[0].sequence, 123);
        assert_eq!(deltas.deltas[1].sequence, 123);
        assert_eq!(deltas.deltas[2].sequence, 123);
        assert_eq!(deltas.ts_event, UnixNanos::from(1));
        assert_eq!(deltas.ts_init, UnixNanos::from(2));
        assert_eq!(deltas.deltas[1].order.price.as_decimal(), dec!(100.00));
        assert_eq!(deltas.deltas[1].order.size.as_decimal(), dec!(1.000));
        assert_eq!(deltas.deltas[1].flags, 0);
        assert_eq!(deltas.deltas[2].flags, RecordFlag::F_LAST as u8);
    }

    #[rstest]
    fn parse_spot_depth_snapshot_sets_last_flag_for_bid_only_snapshot() {
        let instrument_id = InstrumentId::from("BTCUSDT.BINANCE");
        let depth = depth_snapshot(vec![price_level(10_000, 1_000)], vec![]);

        let deltas = parse_spot_depth_snapshot(
            &depth,
            instrument_id,
            2,
            3,
            UnixNanos::from(1),
            UnixNanos::from(2),
        )
        .unwrap()
        .unwrap();

        assert_eq!(deltas.deltas.len(), 2);
        assert_eq!(deltas.deltas[1].flags, RecordFlag::F_LAST as u8);
    }

    #[rstest]
    fn parse_spot_depth_snapshot_returns_none_for_empty_book() {
        let instrument_id = InstrumentId::from("BTCUSDT.BINANCE");
        let depth = depth_snapshot(vec![], vec![]);

        let deltas = parse_spot_depth_snapshot(
            &depth,
            instrument_id,
            2,
            3,
            UnixNanos::from(1),
            UnixNanos::from(2),
        )
        .unwrap();

        assert!(deltas.is_none());
    }

    #[rstest]
    fn parse_spot_depth_snapshot_rejects_out_of_range_price() {
        let instrument_id = InstrumentId::from("BTCUSDT.BINANCE");
        let depth = BinanceDepth {
            last_update_id: 123,
            price_exponent: 100,
            qty_exponent: -3,
            bids: vec![price_level(i64::MAX, 1_000)],
            asks: vec![],
        };

        let result = parse_spot_depth_snapshot(
            &depth,
            instrument_id,
            2,
            3,
            UnixNanos::from(1),
            UnixNanos::from(2),
        );

        assert!(result.is_err());
    }

    #[rstest]
    fn parse_spot_depth_snapshot_rejects_out_of_range_quantity() {
        let instrument_id = InstrumentId::from("BTCUSDT.BINANCE");
        let depth = BinanceDepth {
            last_update_id: 123,
            price_exponent: -2,
            qty_exponent: 100,
            bids: vec![price_level(10_000, i64::MAX)],
            asks: vec![],
        };

        let result = parse_spot_depth_snapshot(
            &depth,
            instrument_id,
            2,
            3,
            UnixNanos::from(1),
            UnixNanos::from(2),
        );

        assert!(result.is_err());
    }

    fn buffered_update(first_update_id: u64, final_update_id: u64) -> BufferedDepthUpdate {
        let instrument_id = InstrumentId::from("BTCUSDT.BINANCE");
        let ts = UnixNanos::default();
        let order = BookOrder::new(
            OrderSide::Buy,
            Price::from_raw(1, 0),
            Quantity::from_raw(1, 0),
            0,
        );
        let delta = OrderBookDelta::new(
            instrument_id,
            BookAction::Update,
            order,
            0,
            final_update_id,
            ts,
            ts,
        );
        let deltas = OrderBookDeltas::new(instrument_id, vec![delta]);

        BufferedDepthUpdate {
            deltas,
            first_update_id,
            final_update_id,
        }
    }

    fn depth_snapshot(bids: Vec<BinancePriceLevel>, asks: Vec<BinancePriceLevel>) -> BinanceDepth {
        BinanceDepth {
            last_update_id: 123,
            price_exponent: -2,
            qty_exponent: -3,
            bids,
            asks,
        }
    }

    fn price_level(price_mantissa: i64, qty_mantissa: i64) -> BinancePriceLevel {
        BinancePriceLevel {
            price_mantissa,
            qty_mantissa,
        }
    }

    #[rstest]
    fn test_spot_market_data_mode_default_is_sbe() {
        assert_eq!(
            BinanceSpotMarketDataMode::default(),
            BinanceSpotMarketDataMode::Sbe
        );
    }

    #[rstest]
    fn test_resolve_spot_json_ws_url_uses_environment_default_without_override() {
        assert_eq!(
            resolve_spot_json_ws_url(None, BinanceEnvironment::Live, false),
            BINANCE_SPOT_WS_URL.to_string()
        );
    }

    #[rstest]
    fn test_resolve_spot_json_ws_url_rewrites_sbe_override_to_spot_default() {
        assert_eq!(
            resolve_spot_json_ws_url(
                Some("wss://stream-sbe.binance.com/ws".to_string()),
                BinanceEnvironment::Live,
                false,
            ),
            BINANCE_SPOT_WS_URL.to_string()
        );
    }

    #[rstest]
    fn test_resolve_spot_json_ws_url_preserves_non_sbe_override() {
        let custom = "wss://example.com/ws".to_string();
        assert_eq!(
            resolve_spot_json_ws_url(Some(custom.clone()), BinanceEnvironment::Live, false),
            custom
        );
    }

    #[rstest]
    fn test_resolve_spot_json_ws_url_uses_binance_us_default() {
        assert_eq!(
            resolve_spot_json_ws_url(None, BinanceEnvironment::Live, true),
            "wss://stream.binance.us:9443/ws"
        );
    }
}
