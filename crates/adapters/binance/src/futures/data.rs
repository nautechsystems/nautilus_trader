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
        },
    },
};
use nautilus_core::{
    MUTEX_POISONED,
    datetime::{NANOSECONDS_IN_MILLISECOND, datetime_to_unix_nanos},
    nanos::UnixNanos,
    time::{AtomicTime, get_atomic_clock_realtime},
};
use nautilus_model::{
    data::{BookOrder, Data, OrderBookDelta, OrderBookDeltas, OrderBookDeltas_API},
    enums::{BookAction, BookType, OrderSide, RecordFlag},
    identifiers::{ClientId, InstrumentId, Venue},
    instruments::{Instrument, InstrumentAny},
    types::{Price, Quantity},
};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::{
    common::{
        consts::{BINANCE_BOOK_DEPTHS, BINANCE_VENUE},
        enums::BinanceProductType,
        parse::bar_spec_to_binance_interval,
        symbol::format_binance_stream_symbol,
    },
    config::BinanceDataClientConfig,
    futures::{
        http::{
            client::BinanceFuturesHttpClient, models::BinanceOrderBook, query::BinanceDepthParams,
        },
        websocket::{
            client::BinanceFuturesWebSocketClient,
            messages::{NautilusDataWsMessage, NautilusWsMessage},
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

#[derive(Debug)]
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
    data_sender: tokio::sync::mpsc::UnboundedSender<DataEvent>,
    is_connected: AtomicBool,
    cancellation_token: CancellationToken,
    tasks: Vec<JoinHandle<()>>,
    instruments: Arc<RwLock<AHashMap<InstrumentId, InstrumentAny>>>,
    book_buffers: Arc<RwLock<AHashMap<InstrumentId, BookBuffer>>>,
    book_subscriptions: Arc<RwLock<AHashMap<InstrumentId, u32>>>,
    mark_price_refs: Arc<RwLock<AHashMap<InstrumentId, u32>>>,
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
            config.api_key.clone(),
            config.api_secret.clone(),
            config.base_url_http.clone(),
            None, // recv_window
            None, // timeout_secs
            None, // proxy_url
        )?;

        let ws_client = BinanceFuturesWebSocketClient::new(
            product_type,
            config.environment,
            config.api_key.clone(),
            config.api_secret.clone(),
            config.base_url_ws.clone(),
            Some(20), // Heartbeat interval
        )?;

        Ok(Self {
            clock,
            client_id,
            config,
            product_type,
            http_client,
            ws_client,
            data_sender,
            is_connected: AtomicBool::new(false),
            cancellation_token: CancellationToken::new(),
            tasks: Vec::new(),
            instruments: Arc::new(RwLock::new(AHashMap::new())),
            book_buffers: Arc::new(RwLock::new(AHashMap::new())),
            book_subscriptions: Arc::new(RwLock::new(AHashMap::new())),
            mark_price_refs: Arc::new(RwLock::new(AHashMap::new())),
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

    #[allow(clippy::too_many_arguments)]
    fn handle_ws_message(
        msg: NautilusWsMessage,
        data_sender: &tokio::sync::mpsc::UnboundedSender<DataEvent>,
        instruments: &Arc<RwLock<AHashMap<InstrumentId, InstrumentAny>>>,
        book_buffers: &Arc<RwLock<AHashMap<InstrumentId, BookBuffer>>>,
        book_subscriptions: &Arc<RwLock<AHashMap<InstrumentId, u32>>>,
        book_epoch: &Arc<RwLock<u64>>,
        http_client: &BinanceFuturesHttpClient,
        clock: &'static AtomicTime,
    ) {
        match msg {
            NautilusWsMessage::Data(data_msg) => match data_msg {
                NautilusDataWsMessage::Data(payloads) => {
                    for data in payloads {
                        Self::send_data(data_sender, data);
                    }
                }
                NautilusDataWsMessage::DepthUpdate {
                    deltas,
                    first_update_id,
                    prev_final_update_id,
                } => {
                    let instrument_id = deltas.instrument_id;
                    let final_update_id = deltas.sequence;

                    // Check if we're buffering for this instrument
                    {
                        let mut buffers = book_buffers.write().expect(MUTEX_POISONED);
                        if let Some(buffer) = buffers.get_mut(&instrument_id) {
                            buffer.updates.push(BufferedDepthUpdate {
                                deltas,
                                first_update_id,
                                final_update_id,
                                prev_final_update_id,
                            });
                            return;
                        }
                    }

                    // Not buffering, emit directly
                    Self::send_data(data_sender, Data::Deltas(OrderBookDeltas_API::new(deltas)));
                }
                NautilusDataWsMessage::Instrument(instrument) => {
                    upsert_instrument(instruments, *instrument);
                }
                NautilusDataWsMessage::RawJson(value) => {
                    log::debug!("Unhandled JSON message: {value:?}");
                }
            },
            NautilusWsMessage::Exec(exec_msg) => {
                log::debug!("Received exec message in data client (ignored): {exec_msg:?}");
            }
            NautilusWsMessage::ExecRaw(raw_msg) => {
                log::debug!("Received raw exec message in data client (ignored): {raw_msg:?}");
            }
            NautilusWsMessage::Error(e) => {
                log::error!(
                    "Binance Futures WebSocket error: code={}, msg={}",
                    e.code,
                    e.msg
                );
            }
            NautilusWsMessage::Reconnected => {
                log::info!("WebSocket reconnected, rebuilding order book snapshots");

                // Increment epoch to invalidate any in-flight snapshot tasks
                let epoch = {
                    let mut guard = book_epoch.write().expect(MUTEX_POISONED);
                    *guard = guard.wrapping_add(1);
                    *guard
                };

                // Get all active book subscriptions
                let subs: Vec<(InstrumentId, u32)> = {
                    let guard = book_subscriptions.read().expect(MUTEX_POISONED);
                    guard.iter().map(|(k, v)| (*k, *v)).collect()
                };

                // Trigger snapshot rebuild for each active subscription
                for (instrument_id, depth) in subs {
                    // Start buffering deltas with new epoch
                    {
                        let mut buffers = book_buffers.write().expect(MUTEX_POISONED);
                        buffers.insert(instrument_id, BookBuffer::new(epoch));
                    }

                    log::info!(
                        "OrderBook snapshot rebuild for {instrument_id} @ depth {depth} \
                        starting (reconnect, epoch={epoch})"
                    );

                    // Spawn snapshot fetch task
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

    #[allow(clippy::too_many_arguments)]
    async fn fetch_and_emit_snapshot(
        http: BinanceFuturesHttpClient,
        sender: tokio::sync::mpsc::UnboundedSender<DataEvent>,
        buffers: Arc<RwLock<AHashMap<InstrumentId, BookBuffer>>>,
        instruments: Arc<RwLock<AHashMap<InstrumentId, InstrumentAny>>>,
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

    #[allow(clippy::too_many_arguments)]
    async fn fetch_and_emit_snapshot_inner(
        http: BinanceFuturesHttpClient,
        sender: tokio::sync::mpsc::UnboundedSender<DataEvent>,
        buffers: Arc<RwLock<AHashMap<InstrumentId, BookBuffer>>>,
        instruments: Arc<RwLock<AHashMap<InstrumentId, InstrumentAny>>>,
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
                    let guard = buffers.read().expect(MUTEX_POISONED);
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
                    let guard = instruments.read().expect(MUTEX_POISONED);
                    match guard.get(&instrument_id) {
                        Some(inst) => (inst.price_precision(), inst.size_precision()),
                        None => {
                            log::error!("No instrument in cache for snapshot: {instrument_id}");
                            let mut buffers = buffers.write().expect(MUTEX_POISONED);
                            buffers.remove(&instrument_id);
                            return;
                        }
                    }
                };

                // Validate first applicable update per Binance spec:
                // First update must satisfy: U <= lastUpdateId+1 AND u >= lastUpdateId+1
                let first_valid = {
                    let guard = buffers.read().expect(MUTEX_POISONED);
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

                            {
                                let mut buffers = buffers.write().expect(MUTEX_POISONED);
                                if let Some(buffer) = buffers.get_mut(&instrument_id)
                                    && buffer.epoch == epoch
                                {
                                    buffer.updates.clear();
                                }
                            }

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
                            {MAX_RETRIES} retries. Book may be inconsistent."
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
                    let mut buffers = buffers.write().expect(MUTEX_POISONED);
                    if let Some(buffer) = buffers.get_mut(&instrument_id) {
                        if buffer.epoch != epoch {
                            return;
                        }
                        std::mem::take(&mut buffer.updates)
                    } else {
                        return;
                    }
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

                            {
                                let mut buffers = buffers.write().expect(MUTEX_POISONED);
                                if let Some(buffer) = buffers.get_mut(&instrument_id)
                                    && buffer.epoch == epoch
                                {
                                    buffer.updates.clear();
                                }
                            }

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
                            retries: expected pu={last_final_update_id}, was pu={}. \
                            Book may be inconsistent.",
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
                        let mut buffers = buffers.write().expect(MUTEX_POISONED);
                        if let Some(buffer) = buffers.get_mut(&instrument_id) {
                            if buffer.epoch != epoch {
                                break;
                            }
                            if buffer.updates.is_empty() {
                                buffers.remove(&instrument_id);
                                break;
                            }
                            std::mem::take(&mut buffer.updates)
                        } else {
                            break;
                        }
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

                                {
                                    let mut buffers = buffers.write().expect(MUTEX_POISONED);
                                    if let Some(buffer) = buffers.get_mut(&instrument_id)
                                        && buffer.epoch == epoch
                                    {
                                        buffer.updates.clear();
                                    }
                                }

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
                                {MAX_RETRIES} retries. Book may be inconsistent."
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
                let mut buffers = buffers.write().expect(MUTEX_POISONED);
                buffers.remove(&instrument_id);
            }
        }
    }
}

fn upsert_instrument(
    cache: &Arc<RwLock<AHashMap<InstrumentId, InstrumentAny>>>,
    instrument: InstrumentAny,
) {
    let mut guard = cache.write().expect(MUTEX_POISONED);
    guard.insert(instrument.id(), instrument);
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
        {
            let mut refs = self.mark_price_refs.write().expect(MUTEX_POISONED);
            refs.clear();
        }
        {
            let mut subs = self.book_subscriptions.write().expect(MUTEX_POISONED);
            subs.clear();
        }
        {
            let mut buffers = self.book_buffers.write().expect(MUTEX_POISONED);
            buffers.clear();
        }

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

        {
            let mut guard = self.instruments.write().expect(MUTEX_POISONED);
            for instrument in &instruments {
                guard.insert(instrument.id(), instrument.clone());
            }
        }

        for instrument in instruments.clone() {
            if let Err(e) = self.data_sender.send(DataEvent::Instrument(instrument)) {
                log::warn!("Failed to send instrument: {e}");
            }
        }

        self.ws_client.cache_instruments(instruments);

        log::info!("Connecting to Binance Futures WebSocket...");
        self.ws_client.connect().await.map_err(|e| {
            log::error!("Binance Futures WebSocket connection failed: {e:?}");
            anyhow::anyhow!("failed to connect Binance Futures WebSocket: {e}")
        })?;
        log::info!("Binance Futures WebSocket connected");

        let stream = self.ws_client.stream();
        let sender = self.data_sender.clone();
        let insts = self.instruments.clone();
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
                            &buffers,
                            &book_subs,
                            &book_epoch,
                            &http,
                            clock,
                        );
                    }
                    () = cancel.cancelled() => {
                        log::debug!("WebSocket stream task cancelled");
                        break;
                    }
                }
            }
        });
        self.tasks.push(handle);

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

        let handles: Vec<_> = self.tasks.drain(..).collect();
        for handle in handles {
            if let Err(e) = handle.await {
                log::error!("Error joining WebSocket task: {e}");
            }
        }

        // Clear subscription state so resubscribes issue fresh WS subscribes
        {
            let mut refs = self.mark_price_refs.write().expect(MUTEX_POISONED);
            refs.clear();
        }
        {
            let mut subs = self.book_subscriptions.write().expect(MUTEX_POISONED);
            subs.clear();
        }
        {
            let mut buffers = self.book_buffers.write().expect(MUTEX_POISONED);
            buffers.clear();
        }

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

    fn subscribe_instruments(&mut self, _cmd: &SubscribeInstruments) -> anyhow::Result<()> {
        log::debug!(
            "subscribe_instruments: Binance Futures instruments are fetched via HTTP on connect"
        );
        Ok(())
    }

    fn subscribe_instrument(&mut self, _cmd: &SubscribeInstrument) -> anyhow::Result<()> {
        log::debug!(
            "subscribe_instrument: Binance Futures instruments are fetched via HTTP on connect"
        );
        Ok(())
    }

    fn subscribe_book_deltas(&mut self, cmd: &SubscribeBookDeltas) -> anyhow::Result<()> {
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
        {
            let mut subs = self.book_subscriptions.write().expect(MUTEX_POISONED);
            subs.insert(instrument_id, depth);
        }

        // Bump epoch to invalidate any in-flight snapshot from a prior subscription
        let epoch = {
            let mut guard = self.book_epoch.write().expect(MUTEX_POISONED);
            *guard = guard.wrapping_add(1);
            *guard
        };

        // Start buffering deltas for this instrument
        {
            let mut buffers = self.book_buffers.write().expect(MUTEX_POISONED);
            buffers.insert(instrument_id, BookBuffer::new(epoch));
        }

        log::info!("OrderBook snapshot rebuild for {instrument_id} @ depth {depth} starting");

        // Subscribe to WebSocket depth stream (0ms = unthrottled for Futures)
        let ws = self.ws_client.clone();
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

    fn subscribe_quotes(&mut self, cmd: &SubscribeQuotes) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;
        let ws = self.ws_client.clone();

        // Binance Futures uses bookTicker for best bid/ask
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

    fn subscribe_trades(&mut self, cmd: &SubscribeTrades) -> anyhow::Result<()> {
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

    fn subscribe_bars(&mut self, cmd: &SubscribeBars) -> anyhow::Result<()> {
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

    fn subscribe_mark_prices(&mut self, cmd: &SubscribeMarkPrices) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;

        // Mark/index/funding share the same stream - use ref counting
        let should_subscribe = {
            let mut refs = self.mark_price_refs.write().expect(MUTEX_POISONED);
            let count = refs.entry(instrument_id).or_insert(0);
            *count += 1;
            *count == 1
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

    fn subscribe_index_prices(&mut self, cmd: &SubscribeIndexPrices) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;

        // Mark/index/funding share the same stream - use ref counting
        let should_subscribe = {
            let mut refs = self.mark_price_refs.write().expect(MUTEX_POISONED);
            let count = refs.entry(instrument_id).or_insert(0);
            *count += 1;
            *count == 1
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

    fn subscribe_funding_rates(&mut self, _cmd: &SubscribeFundingRates) -> anyhow::Result<()> {
        // FundingRateUpdate is not a variant of the Data enum, so we cannot emit funding rates
        // through the standard data channel. This requires custom data handling.
        anyhow::bail!(
            "Funding rate subscriptions are not yet supported for Binance Futures. \
            The Data enum does not have a FundingRateUpdate variant."
        )
    }

    fn unsubscribe_book_deltas(&mut self, cmd: &UnsubscribeBookDeltas) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;
        let ws = self.ws_client.clone();

        // Remove subscription tracking
        {
            let mut subs = self.book_subscriptions.write().expect(MUTEX_POISONED);
            subs.remove(&instrument_id);
        }

        // Remove buffer to prevent snapshot task from emitting after unsubscribe
        {
            let mut buffers = self.book_buffers.write().expect(MUTEX_POISONED);
            buffers.remove(&instrument_id);
        }

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
        let ws = self.ws_client.clone();

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
            let mut refs = self.mark_price_refs.write().expect(MUTEX_POISONED);
            if let Some(count) = refs.get_mut(&instrument_id) {
                *count = count.saturating_sub(1);
                if *count == 0 {
                    refs.remove(&instrument_id);
                    true
                } else {
                    false
                }
            } else {
                false
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
            let mut refs = self.mark_price_refs.write().expect(MUTEX_POISONED);
            if let Some(count) = refs.get_mut(&instrument_id) {
                *count = count.saturating_sub(1);
                if *count == 0 {
                    refs.remove(&instrument_id);
                    true
                } else {
                    false
                }
            } else {
                false
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

    fn unsubscribe_funding_rates(&mut self, _cmd: &UnsubscribeFundingRates) -> anyhow::Result<()> {
        // Funding rate subscriptions are not supported (see subscribe_funding_rates)
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
            {
                let guard = instruments.read().expect(MUTEX_POISONED);
                if let Some(instrument) = guard.get(&instrument_id) {
                    let response = DataResponse::Instrument(Box::new(InstrumentResponse::new(
                        request_id,
                        client_id,
                        instrument.id(),
                        instrument.clone(),
                        start_nanos,
                        end_nanos,
                        clock.get_time_ns(),
                        params,
                    )));

                    if let Err(e) = sender.send(DataEvent::Response(response)) {
                        log::error!("Failed to send instrument response: {e}");
                    }
                    return;
                }
            }

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
