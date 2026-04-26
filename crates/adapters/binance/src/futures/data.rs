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

//! Live market data client implementation for the Binance Futures adapter.

use std::sync::{
    Arc, RwLock,
    atomic::{AtomicBool, Ordering},
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
            BarsResponse, DataResponse, InstrumentResponse, InstrumentsResponse, RequestBars,
            RequestInstrument, RequestInstruments, RequestTrades, SubscribeBars,
            SubscribeBookDeltas, SubscribeFundingRates, SubscribeIndexPrices, SubscribeInstrument,
            SubscribeInstruments, SubscribeMarkPrices, SubscribeQuotes, SubscribeTrades,
            TradesResponse, UnsubscribeBars, UnsubscribeBookDeltas, UnsubscribeFundingRates,
            UnsubscribeIndexPrices, UnsubscribeMarkPrices, UnsubscribeQuotes, UnsubscribeTrades,
            subscribe::SubscribeInstrumentStatus, unsubscribe::UnsubscribeInstrumentStatus,
        },
    },
};
use nautilus_core::{
    AtomicMap, MUTEX_POISONED,
    datetime::{NANOSECONDS_IN_MILLISECOND, datetime_to_unix_nanos},
    nanos::UnixNanos,
    time::{AtomicTime, get_atomic_clock_realtime},
};
use nautilus_model::{
    data::{BookOrder, Data, OrderBookDelta, OrderBookDeltas, OrderBookDeltas_API},
    enums::{BookAction, BookType, MarketStatusAction, OrderSide, RecordFlag},
    identifiers::{ClientId, InstrumentId, Venue},
    instruments::{Instrument, InstrumentAny},
    types::{Price, Quantity},
};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use ustr::Ustr;

use crate::{
    common::{
        consts::{BINANCE_BOOK_DEPTHS, BINANCE_VENUE},
        enums::{BinanceEnvironment, BinanceProductType},
        parse::bar_spec_to_binance_interval,
        status::diff_and_emit_statuses,
        symbol::format_binance_stream_symbol,
        urls::{get_usdm_ws_route_base_url, get_ws_public_base_url},
    },
    config::BinanceDataClientConfig,
    futures::{
        http::{
            client::BinanceFuturesHttpClient, models::BinanceOrderBook, query::BinanceDepthParams,
        },
        websocket::streams::{
            client::BinanceFuturesWebSocketClient,
            messages::BinanceFuturesWsStreamsMessage,
            parse_data::{
                parse_agg_trade, parse_book_ticker, parse_depth_update, parse_kline,
                parse_mark_price, parse_trade,
            },
        },
    },
};

#[derive(Debug, Clone)]
struct BufferedDepthUpdate {
    deltas: OrderBookDeltas,
    first_update_id: u64,
    final_update_id: u64,
    prev_final_update_id: u64,
}

#[derive(Debug, Clone)]
struct BookBuffer {
    updates: Vec<BufferedDepthUpdate>,
    epoch: u64,
}

impl BookBuffer {
    fn new(epoch: u64) -> Self {
        Self {
            updates: Vec::new(),
            epoch,
        }
    }
}

/// Binance Futures data client for USD-M and COIN-M markets.
#[derive(Debug)]
pub struct BinanceFuturesDataClient {
    clock: &'static AtomicTime,
    client_id: ClientId,
    config: BinanceDataClientConfig,
    product_type: BinanceProductType,
    http_client: BinanceFuturesHttpClient,
    ws_client: BinanceFuturesWebSocketClient,
    ws_public_client: BinanceFuturesWebSocketClient,
    data_sender: tokio::sync::mpsc::UnboundedSender<DataEvent>,
    is_connected: AtomicBool,
    cancellation_token: CancellationToken,
    tasks: Vec<JoinHandle<()>>,
    instruments: Arc<AtomicMap<InstrumentId, InstrumentAny>>,
    status_cache: Arc<AtomicMap<InstrumentId, MarketStatusAction>>,
    book_buffers: Arc<AtomicMap<InstrumentId, BookBuffer>>,
    book_subscriptions: Arc<AtomicMap<InstrumentId, u32>>,
    mark_price_refs: Arc<AtomicMap<InstrumentId, u32>>,
    book_epoch: Arc<RwLock<u64>>,
}

impl BinanceFuturesDataClient {
    /// Creates a new [`BinanceFuturesDataClient`] instance.
    ///
    /// # Errors
    ///
    /// Returns an error if the client fails to initialize or if the product type
    /// is not a futures type (UsdM or CoinM).
    pub fn new(
        client_id: ClientId,
        config: BinanceDataClientConfig,
        product_type: BinanceProductType,
    ) -> anyhow::Result<Self> {
        match product_type {
            BinanceProductType::UsdM | BinanceProductType::CoinM => {}
            _ => {
                anyhow::bail!(
                    "BinanceFuturesDataClient requires UsdM or CoinM product type, was {product_type:?}"
                );
            }
        }

        let clock = get_atomic_clock_realtime();
        let data_sender = get_data_event_sender();

        let http_client = BinanceFuturesHttpClient::new(
            product_type,
            config.environment,
            clock,
            config.api_key.clone(),
            config.api_secret.clone(),
            config.base_url_http.clone(),
            None,  // recv_window
            None,  // timeout_secs
            None,  // proxy_url
            false, // treat_expired_as_canceled
        )?;

        let market_url = config.base_url_ws.clone().map(|url| {
            if product_type == BinanceProductType::UsdM
                && config.environment == BinanceEnvironment::Mainnet
            {
                get_usdm_ws_route_base_url(&url, "market")
            } else {
                url
            }
        });

        let ws_client = BinanceFuturesWebSocketClient::new(
            product_type,
            config.environment,
            config.api_key.clone(),
            config.api_secret.clone(),
            market_url,
            Some(20), // Heartbeat interval
            config.transport_backend,
        )?;

        let public_url = config.base_url_ws.clone().map_or_else(
            || get_ws_public_base_url(product_type, config.environment).to_string(),
            |url| {
                if product_type == BinanceProductType::UsdM
                    && config.environment == BinanceEnvironment::Mainnet
                {
                    get_usdm_ws_route_base_url(&url, "public")
                } else {
                    url
                }
            },
        );
        let ws_public_client = BinanceFuturesWebSocketClient::new(
            product_type,
            config.environment,
            None,
            None,
            Some(public_url),
            Some(20),
            config.transport_backend,
        )?;

        Ok(Self {
            clock,
            client_id,
            config,
            product_type,
            http_client,
            ws_client,
            ws_public_client,
            data_sender,
            is_connected: AtomicBool::new(false),
            cancellation_token: CancellationToken::new(),
            tasks: Vec::new(),
            instruments: Arc::new(AtomicMap::new()),
            status_cache: Arc::new(AtomicMap::new()),
            book_buffers: Arc::new(AtomicMap::new()),
            book_subscriptions: Arc::new(AtomicMap::new()),
            mark_price_refs: Arc::new(AtomicMap::new()),
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
        F: Future<Output = anyhow::Result<()>> + Send + 'static,
    {
        get_runtime().spawn(async move {
            if let Err(e) = fut.await {
                log::error!("{context}: {e:?}");
            }
        });
    }

    #[expect(clippy::too_many_arguments)]
    fn handle_ws_message(
        msg: BinanceFuturesWsStreamsMessage,
        data_sender: &tokio::sync::mpsc::UnboundedSender<DataEvent>,
        instruments: &Arc<AtomicMap<InstrumentId, InstrumentAny>>,
        ws_instruments: &Arc<AtomicMap<Ustr, InstrumentAny>>,
        book_buffers: &Arc<AtomicMap<InstrumentId, BookBuffer>>,
        book_subscriptions: &Arc<AtomicMap<InstrumentId, u32>>,
        book_epoch: &Arc<RwLock<u64>>,
        http_client: &BinanceFuturesHttpClient,
        clock: &'static AtomicTime,
    ) {
        let ts_init = clock.get_time_ns();
        let cache = ws_instruments.load();

        match msg {
            BinanceFuturesWsStreamsMessage::AggTrade(ref trade_msg) => {
                if let Some(instrument) = cache.get(&trade_msg.symbol) {
                    match parse_agg_trade(trade_msg, instrument, ts_init) {
                        Ok(trade) => Self::send_data(data_sender, Data::Trade(trade)),
                        Err(e) => log::warn!("Failed to parse aggregate trade: {e}"),
                    }
                }
            }
            BinanceFuturesWsStreamsMessage::Trade(ref trade_msg) => {
                if let Some(instrument) = cache.get(&trade_msg.symbol) {
                    match parse_trade(trade_msg, instrument, ts_init) {
                        Ok(trade) => Self::send_data(data_sender, Data::Trade(trade)),
                        Err(e) => log::warn!("Failed to parse trade: {e}"),
                    }
                }
            }
            BinanceFuturesWsStreamsMessage::BookTicker(ref ticker_msg) => {
                if let Some(instrument) = cache.get(&ticker_msg.symbol) {
                    match parse_book_ticker(ticker_msg, instrument, ts_init) {
                        Ok(quote) => Self::send_data(data_sender, Data::Quote(quote)),
                        Err(e) => log::warn!("Failed to parse book ticker: {e}"),
                    }
                }
            }
            BinanceFuturesWsStreamsMessage::DepthUpdate(ref depth_msg) => {
                if let Some(instrument) = cache.get(&depth_msg.symbol) {
                    match parse_depth_update(depth_msg, instrument, ts_init) {
                        Ok(deltas) => {
                            let instrument_id = deltas.instrument_id;
                            let final_update_id = deltas.sequence;
                            let first_update_id = depth_msg.first_update_id;
                            let prev_final_update_id = depth_msg.prev_final_update_id;

                            if book_buffers.contains_key(&instrument_id) {
                                let mut was_buffered = false;
                                book_buffers.rcu(|m| {
                                    was_buffered = false;

                                    if let Some(buffer) = m.get_mut(&instrument_id) {
                                        buffer.updates.push(BufferedDepthUpdate {
                                            deltas: deltas.clone(),
                                            first_update_id,
                                            final_update_id,
                                            prev_final_update_id,
                                        });
                                        was_buffered = true;
                                    }
                                });

                                if was_buffered {
                                    return;
                                }
                            }

                            Self::send_data(
                                data_sender,
                                Data::Deltas(OrderBookDeltas_API::new(deltas)),
                            );
                        }
                        Err(e) => log::warn!("Failed to parse depth update: {e}"),
                    }
                }
            }
            BinanceFuturesWsStreamsMessage::MarkPrice(ref mark_msg) => {
                if let Some(instrument) = cache.get(&mark_msg.symbol) {
                    match parse_mark_price(mark_msg, instrument, ts_init) {
                        Ok((mark_update, index_update, funding_update)) => {
                            Self::send_data(data_sender, Data::MarkPriceUpdate(mark_update));
                            Self::send_data(data_sender, Data::IndexPriceUpdate(index_update));
                            if let Err(e) = data_sender.send(DataEvent::FundingRate(funding_update))
                            {
                                log::error!("Failed to emit funding rate: {e}");
                            }
                        }
                        Err(e) => log::warn!("Failed to parse mark price: {e}"),
                    }
                }
            }
            BinanceFuturesWsStreamsMessage::Kline(ref kline_msg) => {
                if let Some(instrument) = cache.get(&kline_msg.symbol) {
                    match parse_kline(kline_msg, instrument, ts_init) {
                        Ok(Some(bar)) => Self::send_data(data_sender, Data::Bar(bar)),
                        Ok(None) => {} // Kline not closed yet
                        Err(e) => log::warn!("Failed to parse kline: {e}"),
                    }
                }
            }
            BinanceFuturesWsStreamsMessage::ForceOrder(ref liq_msg) => {
                log::info!(
                    "Liquidation: {} {:?} {:?} qty={} at price={}",
                    liq_msg.order.symbol,
                    liq_msg.order.side,
                    liq_msg.order.status,
                    liq_msg.order.original_qty,
                    liq_msg.order.average_price,
                );
            }
            BinanceFuturesWsStreamsMessage::Ticker(ref ticker_msg) => {
                log::debug!(
                    "Ticker: {} last={} vol={}",
                    ticker_msg.symbol,
                    ticker_msg.last_price,
                    ticker_msg.volume,
                );
            }
            // Execution messages ignored by data client
            BinanceFuturesWsStreamsMessage::AccountUpdate(_)
            | BinanceFuturesWsStreamsMessage::OrderUpdate(_)
            | BinanceFuturesWsStreamsMessage::TradeLite(_)
            | BinanceFuturesWsStreamsMessage::AlgoUpdate(_)
            | BinanceFuturesWsStreamsMessage::MarginCall(_)
            | BinanceFuturesWsStreamsMessage::AccountConfigUpdate(_)
            | BinanceFuturesWsStreamsMessage::ListenKeyExpired => {}
            BinanceFuturesWsStreamsMessage::Error(e) => {
                log::error!(
                    "Binance Futures WebSocket error: code={}, msg={}",
                    e.code,
                    e.msg
                );
            }
            BinanceFuturesWsStreamsMessage::Reconnected => {
                log::info!("WebSocket reconnected, rebuilding order book snapshots");

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
                    book_buffers.insert(instrument_id, BookBuffer::new(epoch));

                    log::info!(
                        "OrderBook snapshot rebuild for {instrument_id} @ depth {depth} \
                        starting (reconnect, epoch={epoch})"
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
                            depth,
                            epoch,
                            clock,
                        )
                        .await;
                    });
                }
            }
        }
    }

    #[expect(clippy::too_many_arguments)]
    async fn fetch_and_emit_snapshot(
        http: BinanceFuturesHttpClient,
        sender: tokio::sync::mpsc::UnboundedSender<DataEvent>,
        buffers: Arc<AtomicMap<InstrumentId, BookBuffer>>,
        instruments: Arc<AtomicMap<InstrumentId, InstrumentAny>>,
        instrument_id: InstrumentId,
        depth: u32,
        epoch: u64,
        clock: &'static AtomicTime,
    ) {
        Self::fetch_and_emit_snapshot_inner(
            http,
            sender,
            buffers,
            instruments,
            instrument_id,
            depth,
            epoch,
            clock,
            0,
        )
        .await;
    }

    #[expect(clippy::too_many_arguments)]
    async fn fetch_and_emit_snapshot_inner(
        http: BinanceFuturesHttpClient,
        sender: tokio::sync::mpsc::UnboundedSender<DataEvent>,
        buffers: Arc<AtomicMap<InstrumentId, BookBuffer>>,
        instruments: Arc<AtomicMap<InstrumentId, InstrumentAny>>,
        instrument_id: InstrumentId,
        depth: u32,
        epoch: u64,
        clock: &'static AtomicTime,
        retry_count: u32,
    ) {
        const MAX_RETRIES: u32 = 3;

        let symbol = format_binance_stream_symbol(&instrument_id).to_uppercase();
        let params = BinanceDepthParams {
            symbol,
            limit: Some(depth),
        };

        match http.depth(&params).await {
            Ok(order_book) => {
                let ts_init = clock.get_time_ns();
                let last_update_id = order_book.last_update_id as u64;

                // Check if subscription was cancelled or epoch changed
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
                        _ => {}
                    }
                }

                // Get instrument for precision
                let (price_precision, size_precision) = {
                    let guard = instruments.load();
                    match guard.get(&instrument_id) {
                        Some(inst) => (inst.price_precision(), inst.size_precision()),
                        None => {
                            log::error!("No instrument in cache for snapshot: {instrument_id}");
                            buffers.remove(&instrument_id);
                            return;
                        }
                    }
                };

                // Validate first applicable update per Binance spec:
                // First update must satisfy: U <= lastUpdateId+1 AND u >= lastUpdateId+1
                let first_valid = {
                    let guard = buffers.load();
                    guard.get(&instrument_id).and_then(|buffer| {
                        buffer
                            .updates
                            .iter()
                            .find(|u| u.final_update_id > last_update_id)
                            .cloned()
                    })
                };

                if let Some(first) = &first_valid {
                    let target = last_update_id + 1;
                    let valid_overlap =
                        first.first_update_id <= target && first.final_update_id >= target;

                    if !valid_overlap {
                        if retry_count < MAX_RETRIES {
                            log::warn!(
                                "OrderBook overlap validation failed for {instrument_id}: \
                                lastUpdateId={last_update_id}, first_update_id={}, \
                                final_update_id={} (need U <= {} <= u), \
                                retrying snapshot (attempt {}/{})",
                                first.first_update_id,
                                first.final_update_id,
                                target,
                                retry_count + 1,
                                MAX_RETRIES
                            );

                            buffers.rcu(|m| {
                                if let Some(buffer) = m.get_mut(&instrument_id)
                                    && buffer.epoch == epoch
                                {
                                    buffer.updates.clear();
                                }
                            });

                            Box::pin(Self::fetch_and_emit_snapshot_inner(
                                http,
                                sender,
                                buffers,
                                instruments,
                                instrument_id,
                                depth,
                                epoch,
                                clock,
                                retry_count + 1,
                            ))
                            .await;
                            return;
                        }
                        log::error!(
                            "OrderBook overlap validation failed for {instrument_id} after \
                            {MAX_RETRIES} retries; book may be inconsistent"
                        );
                    }
                }

                let snapshot_deltas = parse_order_book_snapshot(
                    &order_book,
                    instrument_id,
                    price_precision,
                    size_precision,
                    ts_init,
                );

                if let Err(e) = sender.send(DataEvent::Data(Data::Deltas(
                    OrderBookDeltas_API::new(snapshot_deltas),
                ))) {
                    log::error!("Failed to send snapshot: {e}");
                }

                // Take buffered updates but keep buffer entry during replay
                let buffered = {
                    let mut taken = Vec::new();
                    let mut should_return = false;
                    buffers.rcu(|m| {
                        taken = Vec::new();
                        should_return = false;

                        match m.get_mut(&instrument_id) {
                            Some(buffer) if buffer.epoch == epoch => {
                                taken = std::mem::take(&mut buffer.updates);
                            }
                            _ => should_return = true,
                        }
                    });

                    if should_return {
                        return;
                    }
                    taken
                };

                // Replay buffered updates with continuity validation
                let mut replayed = 0;
                let mut last_final_update_id = last_update_id;

                for update in buffered {
                    // Drop updates where u <= lastUpdateId
                    if update.final_update_id <= last_update_id {
                        continue;
                    }

                    // Validate continuity: pu should equal last emitted final_update_id
                    // (for first update, this validates pu == snapshot lastUpdateId)
                    if update.prev_final_update_id != last_final_update_id {
                        if retry_count < MAX_RETRIES {
                            log::warn!(
                                "OrderBook continuity break for {instrument_id}: \
                                expected pu={last_final_update_id}, was pu={}, \
                                triggering resync (attempt {}/{})",
                                update.prev_final_update_id,
                                retry_count + 1,
                                MAX_RETRIES
                            );

                            buffers.rcu(|m| {
                                if let Some(buffer) = m.get_mut(&instrument_id)
                                    && buffer.epoch == epoch
                                {
                                    buffer.updates.clear();
                                }
                            });

                            Box::pin(Self::fetch_and_emit_snapshot_inner(
                                http,
                                sender,
                                buffers,
                                instruments,
                                instrument_id,
                                depth,
                                epoch,
                                clock,
                                retry_count + 1,
                            ))
                            .await;
                            return;
                        }
                        log::error!(
                            "OrderBook continuity break for {instrument_id} after {MAX_RETRIES} \
                            retries: expected pu={last_final_update_id}, was pu={}; \
                            book may be inconsistent",
                            update.prev_final_update_id
                        );
                    }

                    last_final_update_id = update.final_update_id;
                    replayed += 1;

                    if let Err(e) = sender.send(DataEvent::Data(Data::Deltas(
                        OrderBookDeltas_API::new(update.deltas),
                    ))) {
                        log::error!("Failed to send replayed deltas: {e}");
                    }
                }

                // Drain any updates that arrived during replay
                loop {
                    let more = {
                        let mut taken = Vec::new();
                        let mut should_break = false;
                        buffers.rcu(|m| {
                            taken = Vec::new();
                            should_break = false;

                            match m.get_mut(&instrument_id) {
                                Some(buffer) if buffer.epoch == epoch => {
                                    if buffer.updates.is_empty() {
                                        m.remove(&instrument_id);
                                        should_break = true;
                                    } else {
                                        taken = std::mem::take(&mut buffer.updates);
                                    }
                                }
                                _ => should_break = true,
                            }
                        });

                        if should_break {
                            break;
                        }
                        taken
                    };

                    for update in more {
                        if update.final_update_id <= last_update_id {
                            continue;
                        }

                        if update.prev_final_update_id != last_final_update_id {
                            if retry_count < MAX_RETRIES {
                                log::warn!(
                                    "OrderBook continuity break for {instrument_id}: \
                                    expected pu={last_final_update_id}, was pu={}, \
                                    triggering resync (attempt {}/{})",
                                    update.prev_final_update_id,
                                    retry_count + 1,
                                    MAX_RETRIES
                                );

                                buffers.rcu(|m| {
                                    if let Some(buffer) = m.get_mut(&instrument_id)
                                        && buffer.epoch == epoch
                                    {
                                        buffer.updates.clear();
                                    }
                                });

                                Box::pin(Self::fetch_and_emit_snapshot_inner(
                                    http,
                                    sender,
                                    buffers,
                                    instruments,
                                    instrument_id,
                                    depth,
                                    epoch,
                                    clock,
                                    retry_count + 1,
                                ))
                                .await;
                                return;
                            }
                            log::error!(
                                "OrderBook continuity break for {instrument_id} after \
                                {MAX_RETRIES} retries; book may be inconsistent"
                            );
                        }

                        last_final_update_id = update.final_update_id;
                        replayed += 1;

                        if let Err(e) = sender.send(DataEvent::Data(Data::Deltas(
                            OrderBookDeltas_API::new(update.deltas),
                        ))) {
                            log::error!("Failed to send replayed deltas: {e}");
                        }
                    }
                }

                log::info!(
                    "OrderBook snapshot rebuild for {instrument_id} completed \
                    (lastUpdateId={last_update_id}, replayed={replayed})"
                );
            }
            Err(e) => {
                log::error!("Failed to request order book snapshot for {instrument_id}: {e}");
                buffers.remove(&instrument_id);
            }
        }
    }
}

fn upsert_instrument(
    cache: &Arc<AtomicMap<InstrumentId, InstrumentAny>>,
    instrument: InstrumentAny,
) {
    cache.insert(instrument.id(), instrument);
}

fn parse_order_book_snapshot(
    order_book: &BinanceOrderBook,
    instrument_id: InstrumentId,
    price_precision: u8,
    size_precision: u8,
    ts_init: UnixNanos,
) -> OrderBookDeltas {
    let sequence = order_book.last_update_id as u64;
    let ts_event = order_book.transaction_time.map_or(ts_init, |t| {
        UnixNanos::from((t as u64) * NANOSECONDS_IN_MILLISECOND)
    });

    let total_levels = order_book.bids.len() + order_book.asks.len();
    let mut deltas = Vec::with_capacity(total_levels + 1);

    // First delta is CLEAR to reset the book
    deltas.push(OrderBookDelta::clear(
        instrument_id,
        sequence,
        ts_event,
        ts_init,
    ));

    for (i, (price_str, qty_str)) in order_book.bids.iter().enumerate() {
        let price: f64 = price_str.parse().unwrap_or(0.0);
        let size: f64 = qty_str.parse().unwrap_or(0.0);

        let is_last = i == order_book.bids.len() - 1 && order_book.asks.is_empty();
        let flags = if is_last { RecordFlag::F_LAST as u8 } else { 0 };

        let order = BookOrder::new(
            OrderSide::Buy,
            Price::new(price, price_precision),
            Quantity::new(size, size_precision),
            0,
        );

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

    for (i, (price_str, qty_str)) in order_book.asks.iter().enumerate() {
        let price: f64 = price_str.parse().unwrap_or(0.0);
        let size: f64 = qty_str.parse().unwrap_or(0.0);

        let is_last = i == order_book.asks.len() - 1;
        let flags = if is_last { RecordFlag::F_LAST as u8 } else { 0 };

        let order = BookOrder::new(
            OrderSide::Sell,
            Price::new(price, price_precision),
            Quantity::new(size, size_precision),
            0,
        );

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

    OrderBookDeltas::new(instrument_id, deltas)
}

#[async_trait::async_trait(?Send)]
impl DataClient for BinanceFuturesDataClient {
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
            self.product_type,
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

        // Clear subscription state so resubscribes issue fresh WS subscribes
        self.mark_price_refs.store(AHashMap::new());
        self.book_subscriptions.store(AHashMap::new());
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

        // Reinitialize token in case of reconnection after disconnect
        self.cancellation_token = CancellationToken::new();

        let instruments = self
            .http_client
            .request_instruments()
            .await
            .context("failed to request Binance Futures instruments")?;

        // Seed the status cache from the HTTP client's instruments cache
        {
            let mut inst_map = AHashMap::new();
            let mut status_map = AHashMap::new();

            for instrument in &instruments {
                inst_map.insert(instrument.id(), instrument.clone());
            }

            let http_instruments = self.http_client.instruments_cache();
            for entry in http_instruments.iter() {
                let raw_symbol = entry.key();
                let action = match entry.value() {
                    crate::futures::http::client::BinanceFuturesInstrument::UsdM(s) => {
                        MarketStatusAction::from(s.status)
                    }
                    crate::futures::http::client::BinanceFuturesInstrument::CoinM(s) => s
                        .contract_status
                        .map_or(MarketStatusAction::NotAvailableForTrading, Into::into),
                };

                for instrument in &instruments {
                    if instrument.raw_symbol().as_str() == raw_symbol.as_str() {
                        status_map.insert(instrument.id(), action);
                        break;
                    }
                }
            }

            self.instruments.store(inst_map);
            self.status_cache.store(status_map);
        }

        for instrument in instruments.clone() {
            if let Err(e) = self.data_sender.send(DataEvent::Instrument(instrument)) {
                log::warn!("Failed to send instrument: {e}");
            }
        }

        self.ws_client.cache_instruments(&instruments);
        self.ws_public_client.cache_instruments(&instruments);

        log::info!("Connecting to Binance Futures market WebSocket...");
        self.ws_client.connect().await.map_err(|e| {
            log::error!("Binance Futures market WebSocket connection failed: {e:?}");
            anyhow::anyhow!("failed to connect Binance Futures market WebSocket: {e}")
        })?;
        log::info!("Binance Futures market WebSocket connected");

        log::info!("Connecting to Binance Futures public WebSocket...");
        self.ws_public_client.connect().await.map_err(|e| {
            log::error!("Binance Futures public WebSocket connection failed: {e:?}");
            anyhow::anyhow!("failed to connect Binance Futures public WebSocket: {e}")
        })?;
        log::info!("Binance Futures public WebSocket connected");

        // Spawn market stream handler
        let stream = self.ws_client.stream();
        let sender = self.data_sender.clone();
        let insts = self.instruments.clone();
        let ws_insts = self.ws_client.instruments_cache();
        let buffers = self.book_buffers.clone();
        let book_subs = self.book_subscriptions.clone();
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
                            &book_epoch,
                            &http,
                            clock,
                        );
                    }
                    () = cancel.cancelled() => {
                        log::debug!("Market WebSocket stream task cancelled");
                        break;
                    }
                }
            }
        });
        self.tasks.push(handle);

        // Spawn public stream handler (book data)
        let pub_stream = self.ws_public_client.stream();
        let pub_sender = self.data_sender.clone();
        let pub_insts = self.instruments.clone();
        let pub_ws_insts = self.ws_public_client.instruments_cache();
        let pub_buffers = self.book_buffers.clone();
        let pub_book_subs = self.book_subscriptions.clone();
        let pub_book_epoch = self.book_epoch.clone();
        let pub_http = self.http_client.clone();
        let pub_cancel = self.cancellation_token.clone();

        let pub_handle = get_runtime().spawn(async move {
            pin_mut!(pub_stream);

            loop {
                tokio::select! {
                    Some(message) = pub_stream.next() => {
                        Self::handle_ws_message(
                            message,
                            &pub_sender,
                            &pub_insts,
                            &pub_ws_insts,
                            &pub_buffers,
                            &pub_book_subs,
                            &pub_book_epoch,
                            &pub_http,
                            clock,
                        );
                    }
                    () = pub_cancel.cancelled() => {
                        log::debug!("Public WebSocket stream task cancelled");
                        break;
                    }
                }
            }
        });
        self.tasks.push(pub_handle);

        // Spawn instrument status polling task
        let poll_secs = self.config.instrument_status_poll_secs;
        if poll_secs > 0 {
            let poll_http = self.http_client.clone();
            let poll_sender = self.data_sender.clone();
            let poll_instruments = self.instruments.clone();
            let poll_status_cache = self.status_cache.clone();
            let poll_cancel = self.cancellation_token.clone();
            let poll_clock = self.clock;

            let poll_handle = get_runtime().spawn(async move {
                let mut interval =
                    tokio::time::interval(tokio::time::Duration::from_secs(poll_secs));
                interval.tick().await; // Skip first immediate tick

                loop {
                    tokio::select! {
                        _ = interval.tick() => {
                            match poll_http.request_symbol_statuses().await {
                                Ok(symbol_statuses) => {
                                    let ts = poll_clock.get_time_ns();
                                    let inst_guard = poll_instruments.load();

                                    // Build raw_symbol -> InstrumentId lookup
                                    let raw_to_id: AHashMap<Ustr, InstrumentId> = inst_guard
                                        .values()
                                        .map(|inst| (inst.raw_symbol().inner(), inst.id()))
                                        .collect();

                                    let mut new_statuses = AHashMap::new();

                                    for (raw_symbol, action) in &symbol_statuses {
                                        if let Some(&id) = raw_to_id.get(raw_symbol) {
                                            new_statuses.insert(id, *action);
                                        }
                                    }
                                    drop(inst_guard);

                                    let mut cache = (**poll_status_cache.load()).clone();
                                    diff_and_emit_statuses(
                                        &new_statuses, &mut cache, &poll_sender, ts, ts,
                                    );
                                    poll_status_cache.store(cache);
                                }
                                Err(e) => {
                                    log::warn!("Futures instrument status poll failed: {e}");
                                }
                            }
                        }
                        () = poll_cancel.cancelled() => {
                            log::debug!("Futures instrument status polling task cancelled");
                            break;
                        }
                    }
                }
            });
            self.tasks.push(poll_handle);
            log::info!("Futures instrument status polling started: interval={poll_secs}s");
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
        let _ = self.ws_public_client.close().await;

        let handles: Vec<_> = self.tasks.drain(..).collect();
        for handle in handles {
            if let Err(e) = handle.await {
                log::error!("Error joining WebSocket task: {e}");
            }
        }

        // Clear subscription state so resubscribes issue fresh WS subscribes
        self.mark_price_refs.store(AHashMap::new());
        self.book_subscriptions.store(AHashMap::new());
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

    fn subscribe_instruments(&mut self, _cmd: SubscribeInstruments) -> anyhow::Result<()> {
        log::debug!(
            "subscribe_instruments: Binance Futures instruments are fetched via HTTP on connect"
        );
        Ok(())
    }

    fn subscribe_instrument(&mut self, _cmd: SubscribeInstrument) -> anyhow::Result<()> {
        log::debug!(
            "subscribe_instrument: Binance Futures instruments are fetched via HTTP on connect"
        );
        Ok(())
    }

    fn subscribe_book_deltas(&mut self, cmd: SubscribeBookDeltas) -> anyhow::Result<()> {
        if cmd.book_type != BookType::L2_MBP {
            anyhow::bail!("Binance Futures only supports L2_MBP order book deltas");
        }

        let instrument_id = cmd.instrument_id;
        let depth = cmd.depth.map_or(1000, |d| d.get() as u32);

        if !BINANCE_BOOK_DEPTHS.contains(&depth) {
            anyhow::bail!(
                "Invalid depth {depth} for Binance Futures order book. \
                Valid values: {BINANCE_BOOK_DEPTHS:?}"
            );
        }

        // Track subscription for reconnect handling
        self.book_subscriptions.insert(instrument_id, depth);

        // Bump epoch to invalidate any in-flight snapshot from a prior subscription
        let epoch = {
            let mut guard = self.book_epoch.write().expect(MUTEX_POISONED);
            *guard = guard.wrapping_add(1);
            *guard
        };

        // Start buffering deltas for this instrument
        self.book_buffers
            .insert(instrument_id, BookBuffer::new(epoch));

        log::info!("OrderBook snapshot rebuild for {instrument_id} @ depth {depth} starting");

        // Subscribe to WebSocket depth stream (0ms = unthrottled for Futures)
        let ws = self.ws_public_client.clone();
        let stream = format!("{}@depth@0ms", format_binance_stream_symbol(&instrument_id));

        self.spawn_ws(
            async move {
                ws.subscribe(vec![stream])
                    .await
                    .context("book deltas subscription")
            },
            "order book subscription",
        );

        // Spawn task to fetch HTTP snapshot and replay buffered deltas
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
                depth,
                epoch,
                clock,
            )
            .await;
        });

        Ok(())
    }

    fn subscribe_quotes(&mut self, cmd: SubscribeQuotes) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;
        let ws = self.ws_public_client.clone();

        // Binance Futures uses bookTicker for best bid/ask (public endpoint)
        let stream = format!(
            "{}@bookTicker",
            format_binance_stream_symbol(&instrument_id)
        );

        self.spawn_ws(
            async move {
                ws.subscribe(vec![stream])
                    .await
                    .context("quotes subscription")
            },
            "quote subscription",
        );
        Ok(())
    }

    fn subscribe_trades(&mut self, cmd: SubscribeTrades) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;
        let ws = self.ws_client.clone();

        // Binance Futures uses aggTrade for aggregate trades
        let stream = format!("{}@aggTrade", format_binance_stream_symbol(&instrument_id));

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
        let bar_type = cmd.bar_type;
        let ws = self.ws_client.clone();
        let interval = bar_spec_to_binance_interval(bar_type.spec())?;

        let stream = format!(
            "{}@kline_{}",
            format_binance_stream_symbol(&bar_type.instrument_id()),
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

    fn subscribe_mark_prices(&mut self, cmd: SubscribeMarkPrices) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;

        // Mark/index/funding share the same stream - use ref counting
        let should_subscribe = {
            let prev = self
                .mark_price_refs
                .load()
                .get(&instrument_id)
                .copied()
                .unwrap_or(0);
            self.mark_price_refs.rcu(|m| {
                let count = m.entry(instrument_id).or_insert(0);
                *count += 1;
            });
            prev == 0
        };

        if should_subscribe {
            let ws = self.ws_client.clone();
            let stream = format!(
                "{}@markPrice@1s",
                format_binance_stream_symbol(&instrument_id)
            );

            self.spawn_ws(
                async move {
                    ws.subscribe(vec![stream])
                        .await
                        .context("mark prices subscription")
                },
                "mark prices subscription",
            );
        }
        Ok(())
    }

    fn subscribe_index_prices(&mut self, cmd: SubscribeIndexPrices) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;

        // Mark/index/funding share the same stream - use ref counting
        let should_subscribe = {
            let prev = self
                .mark_price_refs
                .load()
                .get(&instrument_id)
                .copied()
                .unwrap_or(0);
            self.mark_price_refs.rcu(|m| {
                let count = m.entry(instrument_id).or_insert(0);
                *count += 1;
            });
            prev == 0
        };

        if should_subscribe {
            let ws = self.ws_client.clone();
            let stream = format!(
                "{}@markPrice@1s",
                format_binance_stream_symbol(&instrument_id)
            );

            self.spawn_ws(
                async move {
                    ws.subscribe(vec![stream])
                        .await
                        .context("index prices subscription")
                },
                "index prices subscription",
            );
        }
        Ok(())
    }

    fn subscribe_funding_rates(&mut self, cmd: SubscribeFundingRates) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;

        let should_subscribe = {
            let prev = self
                .mark_price_refs
                .load()
                .get(&instrument_id)
                .copied()
                .unwrap_or(0);
            self.mark_price_refs.rcu(|m| {
                let count = m.entry(instrument_id).or_insert(0);
                *count += 1;
            });
            prev == 0
        };

        if should_subscribe {
            let ws = self.ws_client.clone();
            let stream = format!(
                "{}@markPrice@1s",
                format_binance_stream_symbol(&instrument_id)
            );

            self.spawn_ws(
                async move {
                    ws.subscribe(vec![stream])
                        .await
                        .context("funding rates subscription")
                },
                "funding rates subscription",
            );
        }
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
        let ws = self.ws_public_client.clone();

        // Remove subscription tracking
        self.book_subscriptions.remove(&instrument_id);

        // Remove buffer to prevent snapshot task from emitting after unsubscribe
        self.book_buffers.remove(&instrument_id);

        let symbol_lower = format_binance_stream_symbol(&instrument_id);
        let streams = vec![
            format!("{symbol_lower}@depth"),
            format!("{symbol_lower}@depth@0ms"),
            format!("{symbol_lower}@depth@100ms"),
            format!("{symbol_lower}@depth@250ms"),
            format!("{symbol_lower}@depth@500ms"),
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
        let instrument_id = cmd.instrument_id;
        let ws = self.ws_public_client.clone();

        let stream = format!(
            "{}@bookTicker",
            format_binance_stream_symbol(&instrument_id)
        );

        self.spawn_ws(
            async move {
                ws.unsubscribe(vec![stream])
                    .await
                    .context("quotes unsubscribe")
            },
            "quote unsubscribe",
        );
        Ok(())
    }

    fn unsubscribe_trades(&mut self, cmd: &UnsubscribeTrades) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;
        let ws = self.ws_client.clone();

        let stream = format!("{}@aggTrade", format_binance_stream_symbol(&instrument_id));

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
            format_binance_stream_symbol(&bar_type.instrument_id()),
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

    fn unsubscribe_mark_prices(&mut self, cmd: &UnsubscribeMarkPrices) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;

        // Mark/index/funding share the same stream - use ref counting
        let should_unsubscribe = {
            let prev = self.mark_price_refs.load().get(&instrument_id).copied();
            match prev {
                Some(count) if count <= 1 => {
                    self.mark_price_refs.remove(&instrument_id);
                    true
                }
                Some(_) => {
                    self.mark_price_refs.rcu(|m| {
                        if let Some(count) = m.get_mut(&instrument_id) {
                            *count = count.saturating_sub(1);
                        }
                    });
                    false
                }
                None => false,
            }
        };

        if should_unsubscribe {
            let ws = self.ws_client.clone();
            let symbol_lower = format_binance_stream_symbol(&instrument_id);
            let streams = vec![
                format!("{symbol_lower}@markPrice"),
                format!("{symbol_lower}@markPrice@1s"),
                format!("{symbol_lower}@markPrice@3s"),
            ];

            self.spawn_ws(
                async move {
                    ws.unsubscribe(streams)
                        .await
                        .context("mark prices unsubscribe")
                },
                "mark prices unsubscribe",
            );
        }
        Ok(())
    }

    fn unsubscribe_index_prices(&mut self, cmd: &UnsubscribeIndexPrices) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;

        // Mark/index/funding share the same stream - use ref counting
        let should_unsubscribe = {
            let prev = self.mark_price_refs.load().get(&instrument_id).copied();
            match prev {
                Some(count) if count <= 1 => {
                    self.mark_price_refs.remove(&instrument_id);
                    true
                }
                Some(_) => {
                    self.mark_price_refs.rcu(|m| {
                        if let Some(count) = m.get_mut(&instrument_id) {
                            *count = count.saturating_sub(1);
                        }
                    });
                    false
                }
                None => false,
            }
        };

        if should_unsubscribe {
            let ws = self.ws_client.clone();
            let symbol_lower = format_binance_stream_symbol(&instrument_id);
            let streams = vec![
                format!("{symbol_lower}@markPrice"),
                format!("{symbol_lower}@markPrice@1s"),
                format!("{symbol_lower}@markPrice@3s"),
            ];

            self.spawn_ws(
                async move {
                    ws.unsubscribe(streams)
                        .await
                        .context("index prices unsubscribe")
                },
                "index prices unsubscribe",
            );
        }
        Ok(())
    }

    fn unsubscribe_funding_rates(&mut self, cmd: &UnsubscribeFundingRates) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;

        let should_unsubscribe = {
            let prev = self.mark_price_refs.load().get(&instrument_id).copied();
            match prev {
                Some(count) if count <= 1 => {
                    self.mark_price_refs.remove(&instrument_id);
                    true
                }
                Some(_) => {
                    self.mark_price_refs.rcu(|m| {
                        if let Some(count) = m.get_mut(&instrument_id) {
                            *count = count.saturating_sub(1);
                        }
                    });
                    false
                }
                None => false,
            }
        };

        if should_unsubscribe {
            let ws = self.ws_client.clone();
            let symbol_lower = format_binance_stream_symbol(&instrument_id);
            let streams = vec![
                format!("{symbol_lower}@markPrice"),
                format!("{symbol_lower}@markPrice@1s"),
                format!("{symbol_lower}@markPrice@3s"),
            ];

            self.spawn_ws(
                async move {
                    ws.unsubscribe(streams)
                        .await
                        .context("funding rates unsubscribe")
                },
                "funding rates unsubscribe",
            );
        }
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
        let start_nanos = datetime_to_unix_nanos(start);
        let end_nanos = datetime_to_unix_nanos(end);

        get_runtime().spawn(async move {
            match http.request_instruments().await {
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
        let start_nanos = datetime_to_unix_nanos(start);
        let end_nanos = datetime_to_unix_nanos(end);

        get_runtime().spawn(async move {
            match http.request_instruments().await {
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

        get_runtime().spawn(async move {
            match http
                .request_trades(instrument_id, limit)
                .await
                .context("failed to request trades from Binance Futures")
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

        get_runtime().spawn(async move {
            match http
                .request_bars(bar_type, start, end, limit)
                .await
                .context("failed to request bars from Binance Futures")
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
                Err(e) => log::error!("Bar request failed: {e:?}"),
            }
        });

        Ok(())
    }
}
