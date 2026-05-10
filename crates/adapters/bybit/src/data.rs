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

//! Live market data client implementation for the Bybit adapter.

use std::{
    future::Future,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use ahash::{AHashMap, AHashSet};
use anyhow::Context;
use futures_util::{StreamExt, pin_mut};
use nautilus_common::{
    clients::DataClient,
    live::{runner::get_data_event_sender, runtime::get_runtime},
    messages::{
        DataEvent,
        data::{
            BarsResponse, BookResponse, DataResponse, ForwardPricesResponse, FundingRatesResponse,
            InstrumentResponse, InstrumentsResponse, RequestBars, RequestBookSnapshot,
            RequestForwardPrices, RequestFundingRates, RequestInstrument, RequestInstruments,
            RequestTrades, SubscribeBars, SubscribeBookDeltas, SubscribeFundingRates,
            SubscribeIndexPrices, SubscribeInstrumentStatus, SubscribeMarkPrices,
            SubscribeOptionGreeks, SubscribeQuotes, SubscribeTrades, TradesResponse,
            UnsubscribeBars, UnsubscribeBookDeltas, UnsubscribeFundingRates,
            UnsubscribeIndexPrices, UnsubscribeInstrumentStatus, UnsubscribeMarkPrices,
            UnsubscribeOptionGreeks, UnsubscribeQuotes, UnsubscribeTrades,
        },
    },
};
use nautilus_core::{
    AtomicMap, AtomicSet,
    datetime::datetime_to_unix_nanos,
    time::{AtomicTime, get_atomic_clock_realtime},
};
use nautilus_model::{
    data::{BarType, Data, ForwardPrice, OrderBookDeltas_API, QuoteTick},
    enums::{BookType, MarketStatusAction},
    identifiers::{ClientId, InstrumentId, Venue},
    instruments::{Instrument, InstrumentAny},
    orderbook::book::OrderBook,
};
use rust_decimal::Decimal;
use tokio::{task::JoinHandle, time::Duration};
use tokio_util::sync::CancellationToken;
use ustr::Ustr;

use crate::{
    common::{
        consts::{BYBIT_DEFAULT_ORDERBOOK_DEPTH, BYBIT_VENUE},
        enums::BybitProductType,
        parse::{extract_raw_symbol, make_bybit_symbol},
        status::diff_and_emit_statuses,
        symbol::BybitSymbol,
    },
    config::BybitDataClientConfig,
    http::client::BybitHttpClient,
    websocket::{
        client::BybitWebSocketClient,
        messages::BybitWsMessage,
        parse::{
            parse_kline_topic, parse_millis_i64, parse_orderbook_deltas, parse_orderbook_quote,
            parse_ticker_linear_funding, parse_ticker_linear_index_price,
            parse_ticker_linear_mark_price, parse_ticker_linear_quote, parse_ticker_option_greeks,
            parse_ticker_option_index_price, parse_ticker_option_mark_price,
            parse_ticker_option_quote, parse_ws_kline_bar, parse_ws_trade_tick,
        },
    },
};

/// Live market data client for Bybit.
#[derive(Debug)]
pub struct BybitDataClient {
    client_id: ClientId,
    config: BybitDataClientConfig,
    http_client: BybitHttpClient,
    ws_clients: Vec<BybitWebSocketClient>,
    is_connected: AtomicBool,
    cancellation_token: CancellationToken,
    tasks: Vec<JoinHandle<()>>,
    data_sender: tokio::sync::mpsc::UnboundedSender<DataEvent>,
    instruments: Arc<AtomicMap<InstrumentId, InstrumentAny>>,
    book_depths: Arc<AtomicMap<InstrumentId, u32>>,
    quote_depths: Arc<AtomicMap<InstrumentId, u32>>,
    ticker_subs: Arc<AtomicMap<InstrumentId, AHashSet<&'static str>>>,
    trade_subs: Arc<AtomicSet<InstrumentId>>,
    option_greeks_subs: Arc<AtomicSet<InstrumentId>>,
    instrument_status_subs: Arc<AtomicSet<InstrumentId>>,
    status_cache: Arc<AtomicMap<InstrumentId, MarketStatusAction>>,
    clock: &'static AtomicTime,
}

impl BybitDataClient {
    /// Creates a new [`BybitDataClient`] instance.
    ///
    /// # Errors
    ///
    /// Returns an error if the client fails to initialize.
    pub fn new(client_id: ClientId, config: BybitDataClientConfig) -> anyhow::Result<Self> {
        let clock = get_atomic_clock_realtime();
        let data_sender = get_data_event_sender();

        let http_client = if let (Some(api_key), Some(api_secret)) =
            (config.api_key.clone(), config.api_secret.clone())
        {
            BybitHttpClient::with_credentials(
                api_key,
                api_secret,
                Some(config.http_base_url()),
                config.http_timeout_secs,
                config.max_retries,
                config.retry_delay_initial_ms,
                config.retry_delay_max_ms,
                config.recv_window_ms,
                config.proxy_url.clone(),
            )?
        } else {
            BybitHttpClient::new(
                Some(config.http_base_url()),
                config.http_timeout_secs,
                config.max_retries,
                config.retry_delay_initial_ms,
                config.retry_delay_max_ms,
                config.recv_window_ms,
                config.proxy_url.clone(),
            )?
        };

        // Create a WebSocket client for each product type (default to Linear if empty)
        let product_types = if config.product_types.is_empty() {
            vec![BybitProductType::Linear]
        } else {
            config.product_types.clone()
        };

        let ws_clients: Vec<BybitWebSocketClient> = product_types
            .iter()
            .map(|product_type| {
                BybitWebSocketClient::new_public_with(
                    *product_type,
                    config.environment,
                    Some(config.ws_public_url_for(*product_type)),
                    config.heartbeat_interval_secs,
                    config.transport_backend,
                    config.proxy_url.clone(),
                )
            })
            .collect();

        Ok(Self {
            client_id,
            config,
            http_client,
            ws_clients,
            is_connected: AtomicBool::new(false),
            cancellation_token: CancellationToken::new(),
            tasks: Vec::new(),
            data_sender,
            instruments: Arc::new(AtomicMap::new()),
            book_depths: Arc::new(AtomicMap::new()),
            quote_depths: Arc::new(AtomicMap::new()),
            ticker_subs: Arc::new(AtomicMap::new()),
            trade_subs: Arc::new(AtomicSet::new()),
            option_greeks_subs: Arc::new(AtomicSet::new()),
            instrument_status_subs: Arc::new(AtomicSet::new()),
            status_cache: Arc::new(AtomicMap::new()),
            clock,
        })
    }

    fn venue(&self) -> Venue {
        *BYBIT_VENUE
    }

    fn get_ws_client_for_product(
        &self,
        product_type: BybitProductType,
    ) -> Option<&BybitWebSocketClient> {
        self.ws_clients
            .iter()
            .find(|ws| ws.product_type() == Some(product_type))
    }

    fn get_product_type_for_instrument(
        &self,
        instrument_id: InstrumentId,
    ) -> Option<BybitProductType> {
        let guard = self.instruments.load();
        guard
            .get(&instrument_id)
            .and_then(|_| BybitProductType::from_suffix(instrument_id.symbol.as_str()))
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

    fn spawn_instrument_status_polling(
        &mut self,
        product_types: &[BybitProductType],
        poll_secs: u64,
    ) {
        let http = self.http_client.clone();
        let sender = self.data_sender.clone();
        let instruments = self.instruments.clone();
        let status_cache = self.status_cache.clone();
        let status_subs = self.instrument_status_subs.clone();
        let cancel = self.cancellation_token.clone();
        let clock = self.clock;
        let product_types = product_types.to_vec();

        let handle = get_runtime().spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(poll_secs));
            interval.tick().await; // Skip first immediate tick

            loop {
                tokio::select! {
                    _ = interval.tick() => {
                        if status_subs.is_empty() {
                            continue;
                        }

                        // Accumulate statuses from all product types before diffing
                        let mut all_statuses = AHashMap::new();

                        for &pt in &product_types {
                            match http.request_instrument_statuses(pt).await {
                                Ok(new_statuses) => {
                                    let inst_guard = instruments.load();
                                    for (id, action) in new_statuses {
                                        if inst_guard.contains_key(&id) {
                                            all_statuses.insert(id, action);
                                        }
                                    }
                                }
                                Err(e) => {
                                    log::warn!("Bybit instrument status poll failed for {pt:?}: {e}");
                                }
                            }
                        }

                        let ts = clock.get_time_ns();
                        let mut cache = (**status_cache.load()).clone();
                        let subs_guard = status_subs.load();
                        diff_and_emit_statuses(
                            &all_statuses, &mut cache, Some(&subs_guard), &sender, ts, ts,
                        );
                        status_cache.store(cache);
                    }
                    () = cancel.cancelled() => {
                        log::debug!("Bybit instrument status polling task cancelled");
                        break;
                    }
                }
            }
        });
        self.tasks.push(handle);
        log::info!("Instrument status polling started: interval={poll_secs}s");
    }
}

fn send_data(sender: &tokio::sync::mpsc::UnboundedSender<DataEvent>, data: Data) {
    if let Err(e) = sender.send(DataEvent::Data(data)) {
        log::error!("Failed to emit data event: {e}");
    }
}

/// Cached funding state per symbol: (funding_rate, next_funding_time, funding_interval_hour).
type FundingCacheEntry = (Option<String>, Option<String>, Option<String>);

#[expect(clippy::too_many_arguments)]
fn handle_ws_message(
    message: &BybitWsMessage,
    data_sender: &tokio::sync::mpsc::UnboundedSender<DataEvent>,
    instruments: &AHashMap<Ustr, InstrumentAny>,
    product_type: Option<BybitProductType>,
    trade_subs: &Arc<AtomicSet<InstrumentId>>,
    ticker_subs: &Arc<AtomicMap<InstrumentId, AHashSet<&'static str>>>,
    quote_depths: &Arc<AtomicMap<InstrumentId, u32>>,
    book_depths: &Arc<AtomicMap<InstrumentId, u32>>,
    option_greeks_subs: &Arc<AtomicSet<InstrumentId>>,
    bar_types_cache: &Arc<AtomicMap<String, BarType>>,
    quote_cache: &mut AHashMap<InstrumentId, QuoteTick>,
    funding_cache: &mut AHashMap<Ustr, FundingCacheEntry>,
    clock: &AtomicTime,
) {
    let ts_init = clock.get_time_ns();
    let resolve = |raw_symbol: &Ustr| -> Option<&InstrumentAny> {
        let key = product_type.map_or(*raw_symbol, |pt| make_bybit_symbol(raw_symbol, pt));
        instruments.get(&key)
    };

    match message {
        BybitWsMessage::Orderbook(msg) => {
            let Some(instrument) = resolve(&msg.data.s) else {
                log::warn!("Unknown symbol in orderbook update: {}", msg.data.s);
                return;
            };
            let instrument_id = instrument.id();

            // Emit deltas if subscribed to book
            let has_book_sub = book_depths.contains_key(&instrument_id);

            if has_book_sub {
                match parse_orderbook_deltas(msg, instrument, ts_init) {
                    Ok(deltas) => {
                        send_data(data_sender, Data::Deltas(OrderBookDeltas_API::new(deltas)));
                    }
                    Err(e) => log::error!("Failed to parse orderbook deltas: {e}"),
                }
            }

            // Emit quote from best bid/ask if subscribed
            let has_quote_sub = quote_depths.contains_key(&instrument_id);
            let has_ticker_quote_sub = ticker_subs
                .load()
                .get(&instrument_id)
                .is_some_and(|s| s.contains("quotes"));

            if has_quote_sub || has_ticker_quote_sub {
                let last_quote = quote_cache.get(&instrument_id);
                match parse_orderbook_quote(msg, instrument, last_quote, ts_init) {
                    Ok(quote) => {
                        quote_cache.insert(instrument_id, quote);
                        send_data(data_sender, Data::Quote(quote));
                    }
                    Err(e) => log::error!("Failed to parse orderbook quote: {e}"),
                }
            }
        }
        BybitWsMessage::Trade(msg) => {
            for trade in &msg.data {
                let Some(instrument) = resolve(&trade.s) else {
                    continue;
                };
                let instrument_id = instrument.id();
                if !trade_subs.contains(&instrument_id) {
                    continue;
                }

                match parse_ws_trade_tick(trade, instrument, ts_init) {
                    Ok(tick) => send_data(data_sender, Data::Trade(tick)),
                    Err(e) => log::error!("Failed to parse trade tick: {e}"),
                }
            }
        }
        BybitWsMessage::Kline(msg) => {
            let Ok((_, raw_symbol)) = parse_kline_topic(msg.topic.as_str()) else {
                log::warn!("Invalid kline topic: {}", msg.topic);
                return;
            };
            let ustr_symbol = Ustr::from(raw_symbol);
            let Some(instrument) = resolve(&ustr_symbol) else {
                log::warn!("Unknown symbol in kline update: {raw_symbol}");
                return;
            };
            let topic_key = msg.topic.as_str();
            let Some(bar_type) = bar_types_cache.load().get(topic_key).copied() else {
                log::warn!("No bar type cached for kline topic: {topic_key}");
                return;
            };

            for kline in &msg.data {
                if !kline.confirm {
                    continue;
                }

                match parse_ws_kline_bar(kline, instrument, bar_type, true, ts_init) {
                    Ok(bar) => send_data(data_sender, Data::Bar(bar)),
                    Err(e) => log::error!("Failed to parse kline bar: {e}"),
                }
            }
        }
        BybitWsMessage::TickerLinear(msg) => {
            let Some(instrument) = resolve(&msg.data.symbol) else {
                log::warn!("Unknown symbol in ticker update: {}", msg.data.symbol);
                return;
            };
            let instrument_id = instrument.id();
            let subs = ticker_subs.load();
            let sub_set = subs.get(&instrument_id);

            if sub_set.is_some_and(|s| s.contains("quotes")) && msg.data.bid1_price.is_some() {
                match parse_ticker_linear_quote(msg, instrument, ts_init) {
                    Ok(quote) => {
                        let last = quote_cache.get(&instrument_id);
                        if last.is_none_or(|q| *q != quote) {
                            quote_cache.insert(instrument_id, quote);
                            send_data(data_sender, Data::Quote(quote));
                        }
                    }
                    Err(e) => log::debug!("Skipping partial ticker update: {e}"),
                }
            }

            let ts_event = match parse_millis_i64(msg.ts, "ticker.ts") {
                Ok(ts) => ts,
                Err(e) => {
                    log::error!("Failed to parse ticker timestamp: {e}");
                    return;
                }
            };

            if sub_set.is_some_and(|s| s.contains("funding")) {
                let cache_entry = funding_cache
                    .entry(msg.data.symbol)
                    .or_insert((None, None, None));
                let mut changed = false;

                if let Some(rate) = &msg.data.funding_rate
                    && cache_entry.0.as_ref() != Some(rate)
                {
                    cache_entry.0 = Some(rate.clone());
                    changed = true;
                }

                if let Some(next_time) = &msg.data.next_funding_time
                    && cache_entry.1.as_ref() != Some(next_time)
                {
                    cache_entry.1 = Some(next_time.clone());
                    changed = true;
                }

                if let Some(interval) = &msg.data.funding_interval_hour {
                    cache_entry.2 = Some(interval.clone());
                }

                if changed && cache_entry.0.is_some() {
                    let mut merged = msg.data.clone();

                    if merged.funding_rate.is_none() {
                        merged.funding_rate.clone_from(&cache_entry.0);
                    }

                    if merged.next_funding_time.is_none() {
                        merged.next_funding_time.clone_from(&cache_entry.1);
                    }

                    if merged.funding_interval_hour.is_none() {
                        merged.funding_interval_hour.clone_from(&cache_entry.2);
                    }

                    match parse_ticker_linear_funding(&merged, instrument_id, ts_event, ts_init) {
                        Ok(update) => {
                            if let Err(e) = data_sender.send(DataEvent::FundingRate(update)) {
                                log::error!("Failed to emit funding rate event: {e}");
                            }
                        }
                        Err(e) => log::error!("Failed to parse ticker linear funding: {e}"),
                    }
                }
            }

            if sub_set.is_some_and(|s| s.contains("mark_prices")) && msg.data.mark_price.is_some() {
                match parse_ticker_linear_mark_price(&msg.data, instrument, ts_event, ts_init) {
                    Ok(update) => send_data(data_sender, Data::MarkPriceUpdate(update)),
                    Err(e) => log::debug!("Skipping mark price update: {e}"),
                }
            }

            if sub_set.is_some_and(|s| s.contains("index_prices")) && msg.data.index_price.is_some()
            {
                match parse_ticker_linear_index_price(&msg.data, instrument, ts_event, ts_init) {
                    Ok(update) => send_data(data_sender, Data::IndexPriceUpdate(update)),
                    Err(e) => log::debug!("Skipping index price update: {e}"),
                }
            }
        }
        BybitWsMessage::TickerOption(msg) => {
            let Some(instrument) = resolve(&msg.data.symbol) else {
                log::warn!(
                    "Unknown symbol in option ticker update: {}",
                    msg.data.symbol
                );
                return;
            };
            let instrument_id = instrument.id();
            let subs = ticker_subs.load();
            let sub_set = subs.get(&instrument_id);

            if sub_set.is_some_and(|s| s.contains("quotes")) {
                match parse_ticker_option_quote(msg, instrument, ts_init) {
                    Ok(quote) => {
                        let last = quote_cache.get(&instrument_id);
                        if last.is_none_or(|q| *q != quote) {
                            quote_cache.insert(instrument_id, quote);
                            send_data(data_sender, Data::Quote(quote));
                        }
                    }
                    Err(e) => log::error!("Failed to parse ticker option quote: {e}"),
                }
            }

            if sub_set.is_some_and(|s| s.contains("mark_prices")) {
                match parse_ticker_option_mark_price(msg, instrument, ts_init) {
                    Ok(update) => send_data(data_sender, Data::MarkPriceUpdate(update)),
                    Err(e) => log::error!("Failed to parse ticker option mark price: {e}"),
                }
            }

            if sub_set.is_some_and(|s| s.contains("index_prices")) {
                match parse_ticker_option_index_price(msg, instrument, ts_init) {
                    Ok(update) => send_data(data_sender, Data::IndexPriceUpdate(update)),
                    Err(e) => log::error!("Failed to parse ticker option index price: {e}"),
                }
            }

            if option_greeks_subs.contains(&instrument_id) {
                match parse_ticker_option_greeks(msg, instrument, ts_init) {
                    Ok(greeks) => {
                        if let Err(e) = data_sender.send(DataEvent::OptionGreeks(greeks)) {
                            log::error!("Failed to send option greeks: {e}");
                        }
                    }
                    Err(e) => log::error!("Failed to parse option greeks: {e}"),
                }
            }
        }
        BybitWsMessage::Reconnected => {
            quote_cache.clear();
            funding_cache.clear();
            log::info!("WebSocket reconnected, cleared caches");
        }
        BybitWsMessage::Error(e) => {
            log::error!(
                "Bybit WebSocket error: code={} message={}",
                e.code,
                e.message
            );
        }
        BybitWsMessage::Auth(_)
        | BybitWsMessage::OrderResponse(_)
        | BybitWsMessage::AccountOrder(_)
        | BybitWsMessage::AccountExecution(_)
        | BybitWsMessage::AccountWallet(_)
        | BybitWsMessage::AccountPosition(_) => {}
    }
}

fn upsert_instrument(
    cache: &Arc<AtomicMap<InstrumentId, InstrumentAny>>,
    instrument: InstrumentAny,
) {
    cache.insert(instrument.id(), instrument);
}

#[async_trait::async_trait(?Send)]
impl DataClient for BybitDataClient {
    fn client_id(&self) -> ClientId {
        self.client_id
    }

    fn venue(&self) -> Option<Venue> {
        Some(self.venue())
    }

    fn start(&mut self) -> anyhow::Result<()> {
        log::info!(
            "Started: client_id={}, product_types={:?}, environment={:?}, proxy_url={:?}",
            self.client_id,
            self.config.product_types,
            self.config.environment,
            self.config.proxy_url,
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
        self.is_connected.store(false, Ordering::Relaxed);
        self.cancellation_token = CancellationToken::new();
        self.tasks.clear();
        self.book_depths.store(AHashMap::new());
        self.quote_depths.store(AHashMap::new());
        self.ticker_subs.store(AHashMap::new());
        self.option_greeks_subs.store(AHashSet::new());
        self.instrument_status_subs.store(AHashSet::new());
        self.status_cache.store(AHashMap::new());
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

        let product_types = if self.config.product_types.is_empty() {
            vec![BybitProductType::Linear]
        } else {
            self.config.product_types.clone()
        };

        let mut all_instruments = Vec::new();

        for product_type in &product_types {
            let fetched = self
                .http_client
                .request_instruments(*product_type, None, None)
                .await
                .with_context(|| {
                    format!("failed to request Bybit instruments for {product_type:?}")
                })?;

            self.http_client.cache_instruments(&fetched);

            self.instruments.rcu(|m| {
                for instrument in &fetched {
                    m.insert(instrument.id(), instrument.clone());
                }
            });

            all_instruments.extend(fetched);
        }

        // Seed instrument status cache from initial fetch
        if self
            .config
            .instrument_status_poll_secs
            .is_some_and(|s| s > 0)
        {
            // Collect all statuses first (without holding the lock across await)
            let mut collected_statuses = Vec::new();

            for product_type in &product_types {
                match self
                    .http_client
                    .request_instrument_statuses(*product_type)
                    .await
                {
                    Ok(statuses) => collected_statuses.push(statuses),
                    Err(e) => {
                        log::warn!(
                            "Failed to seed instrument status cache for {product_type:?}: {e}"
                        );
                    }
                }
            }

            let inst_guard = self.instruments.load();
            let mut status_map = AHashMap::new();

            for statuses in collected_statuses {
                for (id, action) in statuses {
                    if inst_guard.contains_key(&id) {
                        status_map.insert(id, action);
                    }
                }
            }
            log::info!(
                "Seeded instrument status cache with {} entries",
                status_map.len()
            );
            self.status_cache.store(status_map);
        }

        for instrument in all_instruments {
            if let Err(e) = self.data_sender.send(DataEvent::Instrument(instrument)) {
                log::warn!("Failed to send instrument: {e}");
            }
        }

        // Build instruments map keyed by full Nautilus symbol for parsing
        let instruments_by_symbol: Arc<AHashMap<Ustr, InstrumentAny>> = {
            let guard = self.instruments.load();
            let mut map = AHashMap::new();
            for instrument in guard.values() {
                map.insert(instrument.id().symbol.inner(), instrument.clone());
            }
            Arc::new(map)
        };

        for ws_client in &mut self.ws_clients {
            ws_client
                .connect()
                .await
                .context("failed to connect Bybit WebSocket")?;
            ws_client
                .wait_until_active(10.0)
                .await
                .context("WebSocket did not become active")?;

            let stream = ws_client.stream();
            let product_type = ws_client.product_type();
            let sender = self.data_sender.clone();
            let trade_subs = self.trade_subs.clone();
            let ticker_subs = self.ticker_subs.clone();
            let quote_depths = self.quote_depths.clone();
            let book_depths = self.book_depths.clone();
            let option_greeks_subs = self.option_greeks_subs.clone();
            let bar_types_cache = ws_client.bar_types_cache().clone();
            let instruments = Arc::clone(&instruments_by_symbol);
            let clock = self.clock;
            let cancel = self.cancellation_token.clone();

            let handle = get_runtime().spawn(async move {
                let mut quote_cache: AHashMap<InstrumentId, QuoteTick> = AHashMap::new();
                let mut funding_cache: AHashMap<Ustr, FundingCacheEntry> = AHashMap::new();

                pin_mut!(stream);

                loop {
                    tokio::select! {
                        Some(message) = stream.next() => {
                            handle_ws_message(
                                &message,
                                &sender,
                                &instruments,
                                product_type,
                                &trade_subs,
                                &ticker_subs,
                                &quote_depths,
                                &book_depths,
                                &option_greeks_subs,
                                &bar_types_cache,
                                &mut quote_cache,
                                &mut funding_cache,
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
        }

        // Spawn instrument status polling task
        if let Some(poll_secs) = self.config.instrument_status_poll_secs
            && poll_secs > 0
        {
            self.spawn_instrument_status_polling(&product_types, poll_secs);
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

        // Reinitialize token so reconnect can spawn new stream tasks
        self.cancellation_token = CancellationToken::new();

        for ws_client in &mut self.ws_clients {
            if let Err(e) = ws_client.close().await {
                log::warn!("Error closing WebSocket: {e:?}");
            }
        }

        // Allow time for unsubscribe confirmations
        tokio::time::sleep(Duration::from_millis(500)).await;

        let handles: Vec<_> = self.tasks.drain(..).collect();
        for handle in handles {
            if let Err(e) = handle.await {
                log::error!("Error joining WebSocket task: {e}");
            }
        }

        self.book_depths.store(AHashMap::new());
        self.quote_depths.store(AHashMap::new());
        self.ticker_subs.store(AHashMap::new());
        self.trade_subs.store(AHashSet::new());
        self.option_greeks_subs.store(AHashSet::new());
        self.instrument_status_subs.store(AHashSet::new());
        self.status_cache.store(AHashMap::new());
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

    fn subscribe_book_deltas(&mut self, cmd: SubscribeBookDeltas) -> anyhow::Result<()> {
        if cmd.book_type != BookType::L2_MBP {
            anyhow::bail!("Bybit only supports L2_MBP order book deltas");
        }

        let depth = cmd
            .depth
            .map_or(BYBIT_DEFAULT_ORDERBOOK_DEPTH, |d| d.get() as u32);

        if !matches!(depth, 1 | 50 | 200 | 500) {
            anyhow::bail!("invalid depth {depth}; valid values are 1, 50, 200, or 500");
        }

        let instrument_id = cmd.instrument_id;
        let product_type = self
            .get_product_type_for_instrument(instrument_id)
            .unwrap_or(BybitProductType::Linear);

        let ws = self
            .get_ws_client_for_product(product_type)
            .context("no WebSocket client for product type")?
            .clone();

        let book_depths = Arc::clone(&self.book_depths);

        self.spawn_ws(
            async move {
                ws.subscribe_orderbook(instrument_id, depth)
                    .await
                    .context("orderbook subscription")?;
                book_depths.insert(instrument_id, depth);
                Ok(())
            },
            "order book delta subscription",
        );

        Ok(())
    }

    fn subscribe_quotes(&mut self, cmd: SubscribeQuotes) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;
        let product_type = self
            .get_product_type_for_instrument(instrument_id)
            .unwrap_or(BybitProductType::Linear);

        let ws = self
            .get_ws_client_for_product(product_type)
            .context("no WebSocket client for product type")?
            .clone();

        // SPOT ticker channel doesn't include bid/ask, use orderbook depth=1
        if product_type == BybitProductType::Spot {
            let depth = 1;
            self.quote_depths.insert(instrument_id, depth);

            self.spawn_ws(
                async move {
                    ws.subscribe_orderbook(instrument_id, depth)
                        .await
                        .context("orderbook subscription for quotes")
                },
                "quote subscription (spot orderbook)",
            );
        } else {
            let mut should_subscribe = false;
            self.ticker_subs.rcu(|m| {
                let entry = m.entry(instrument_id).or_default();
                should_subscribe = entry.is_empty();
                entry.insert("quotes");
            });

            if should_subscribe {
                self.spawn_ws(
                    async move {
                        ws.subscribe_ticker(instrument_id)
                            .await
                            .context("ticker subscription")
                    },
                    "quote subscription",
                );
            }
        }
        Ok(())
    }

    fn subscribe_trades(&mut self, cmd: SubscribeTrades) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;
        let product_type = self
            .get_product_type_for_instrument(instrument_id)
            .unwrap_or(BybitProductType::Linear);

        self.trade_subs.insert(instrument_id);

        let ws = self
            .get_ws_client_for_product(product_type)
            .context("no WebSocket client for product type")?
            .clone();

        self.spawn_ws(
            async move {
                ws.subscribe_trades(instrument_id)
                    .await
                    .context("trades subscription")
            },
            "trade subscription",
        );
        Ok(())
    }

    fn subscribe_funding_rates(&mut self, cmd: SubscribeFundingRates) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;
        let product_type = self
            .get_product_type_for_instrument(instrument_id)
            .unwrap_or(BybitProductType::Linear);

        if product_type == BybitProductType::Spot || product_type == BybitProductType::Option {
            anyhow::bail!("Funding rates not available for {product_type:?} instruments");
        }

        let mut should_subscribe = false;
        self.ticker_subs.rcu(|m| {
            let entry = m.entry(instrument_id).or_default();
            should_subscribe = entry.is_empty();
            entry.insert("funding");
        });

        if should_subscribe {
            let ws = self
                .get_ws_client_for_product(product_type)
                .context("no WebSocket client for product type")?
                .clone();

            self.spawn_ws(
                async move {
                    ws.subscribe_ticker(instrument_id)
                        .await
                        .context("ticker subscription for funding rates")
                },
                "funding rate subscription",
            );
        }
        Ok(())
    }

    fn subscribe_mark_prices(&mut self, cmd: SubscribeMarkPrices) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;
        let product_type = self
            .get_product_type_for_instrument(instrument_id)
            .unwrap_or(BybitProductType::Linear);

        if product_type == BybitProductType::Spot {
            anyhow::bail!("Mark prices not available for Spot instruments");
        }

        let mut should_subscribe = false;
        self.ticker_subs.rcu(|m| {
            let entry = m.entry(instrument_id).or_default();
            should_subscribe = entry.is_empty();
            entry.insert("mark_prices");
        });

        if should_subscribe {
            let ws = self
                .get_ws_client_for_product(product_type)
                .context("no WebSocket client for product type")?
                .clone();

            self.spawn_ws(
                async move {
                    ws.subscribe_ticker(instrument_id)
                        .await
                        .context("ticker subscription for mark prices")
                },
                "mark price subscription",
            );
        }
        Ok(())
    }

    fn subscribe_index_prices(&mut self, cmd: SubscribeIndexPrices) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;
        let product_type = self
            .get_product_type_for_instrument(instrument_id)
            .unwrap_or(BybitProductType::Linear);

        if product_type == BybitProductType::Spot {
            anyhow::bail!("Index prices not available for Spot instruments");
        }

        let mut should_subscribe = false;
        self.ticker_subs.rcu(|m| {
            let entry = m.entry(instrument_id).or_default();
            should_subscribe = entry.is_empty();
            entry.insert("index_prices");
        });

        if should_subscribe {
            let ws = self
                .get_ws_client_for_product(product_type)
                .context("no WebSocket client for product type")?
                .clone();

            self.spawn_ws(
                async move {
                    ws.subscribe_ticker(instrument_id)
                        .await
                        .context("ticker subscription for index prices")
                },
                "index price subscription",
            );
        }
        Ok(())
    }

    fn subscribe_bars(&mut self, cmd: SubscribeBars) -> anyhow::Result<()> {
        let bar_type = cmd.bar_type;
        let instrument_id = bar_type.instrument_id();
        let product_type = self
            .get_product_type_for_instrument(instrument_id)
            .unwrap_or(BybitProductType::Linear);

        if product_type == BybitProductType::Option {
            anyhow::bail!("Bybit does not support kline/bar data for options");
        }

        let ws = self
            .get_ws_client_for_product(product_type)
            .context("no WebSocket client for product type")?
            .clone();

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
        let instrument_id = cmd.instrument_id;
        let depth = self
            .book_depths
            .load()
            .get(&instrument_id)
            .copied()
            .unwrap_or(BYBIT_DEFAULT_ORDERBOOK_DEPTH);
        self.book_depths.remove(&instrument_id);

        let product_type = self
            .get_product_type_for_instrument(instrument_id)
            .unwrap_or(BybitProductType::Linear);

        // Check if spot quote subscription is using the same depth
        let quote_using_same_depth = self
            .quote_depths
            .load()
            .get(&instrument_id)
            .is_some_and(|&d| d == depth);

        if quote_using_same_depth {
            return Ok(());
        }

        let ws = self
            .get_ws_client_for_product(product_type)
            .context("no WebSocket client for product type")?
            .clone();

        self.spawn_ws(
            async move {
                ws.unsubscribe_orderbook(instrument_id, depth)
                    .await
                    .context("orderbook unsubscribe")
            },
            "order book unsubscribe",
        );
        Ok(())
    }

    fn unsubscribe_quotes(&mut self, cmd: &UnsubscribeQuotes) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;
        let product_type = self
            .get_product_type_for_instrument(instrument_id)
            .unwrap_or(BybitProductType::Linear);

        let ws = self
            .get_ws_client_for_product(product_type)
            .context("no WebSocket client for product type")?
            .clone();

        if product_type == BybitProductType::Spot {
            let depth = self
                .quote_depths
                .load()
                .get(&instrument_id)
                .copied()
                .unwrap_or(1);
            self.quote_depths.remove(&instrument_id);

            // Check if book deltas subscription is using the same depth
            let book_using_same_depth = self
                .book_depths
                .load()
                .get(&instrument_id)
                .is_some_and(|&d| d == depth);

            if !book_using_same_depth {
                self.spawn_ws(
                    async move {
                        ws.unsubscribe_orderbook(instrument_id, depth)
                            .await
                            .context("orderbook unsubscribe for quotes")
                    },
                    "quote unsubscribe (spot orderbook)",
                );
            }
        } else {
            let mut should_unsubscribe = false;
            self.ticker_subs.rcu(|m| {
                if let Some(entry) = m.get_mut(&instrument_id) {
                    entry.remove("quotes");
                    if entry.is_empty() {
                        m.remove(&instrument_id);
                        should_unsubscribe = true;
                    } else {
                        should_unsubscribe = false;
                    }
                } else {
                    should_unsubscribe = false;
                }
            });

            if should_unsubscribe {
                self.spawn_ws(
                    async move {
                        ws.unsubscribe_ticker(instrument_id)
                            .await
                            .context("ticker unsubscribe")
                    },
                    "quote unsubscribe",
                );
            }
        }
        Ok(())
    }

    fn unsubscribe_trades(&mut self, cmd: &UnsubscribeTrades) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;
        let product_type = self
            .get_product_type_for_instrument(instrument_id)
            .unwrap_or(BybitProductType::Linear);

        self.trade_subs.remove(&instrument_id);

        let ws = self
            .get_ws_client_for_product(product_type)
            .context("no WebSocket client for product type")?
            .clone();

        self.spawn_ws(
            async move {
                ws.unsubscribe_trades(instrument_id)
                    .await
                    .context("trades unsubscribe")
            },
            "trade unsubscribe",
        );
        Ok(())
    }

    fn unsubscribe_funding_rates(&mut self, cmd: &UnsubscribeFundingRates) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;
        let product_type = self
            .get_product_type_for_instrument(instrument_id)
            .unwrap_or(BybitProductType::Linear);

        let mut should_unsubscribe = false;
        self.ticker_subs.rcu(|m| {
            if let Some(entry) = m.get_mut(&instrument_id) {
                entry.remove("funding");
                if entry.is_empty() {
                    m.remove(&instrument_id);
                    should_unsubscribe = true;
                } else {
                    should_unsubscribe = false;
                }
            } else {
                should_unsubscribe = false;
            }
        });

        if should_unsubscribe {
            let ws = self
                .get_ws_client_for_product(product_type)
                .context("no WebSocket client for product type")?
                .clone();

            self.spawn_ws(
                async move {
                    ws.unsubscribe_ticker(instrument_id)
                        .await
                        .context("ticker unsubscribe for funding rates")
                },
                "funding rate unsubscribe",
            );
        }
        Ok(())
    }

    fn unsubscribe_mark_prices(&mut self, cmd: &UnsubscribeMarkPrices) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;
        let product_type = self
            .get_product_type_for_instrument(instrument_id)
            .unwrap_or(BybitProductType::Linear);

        let mut should_unsubscribe = false;
        self.ticker_subs.rcu(|m| {
            if let Some(entry) = m.get_mut(&instrument_id) {
                entry.remove("mark_prices");
                if entry.is_empty() {
                    m.remove(&instrument_id);
                    should_unsubscribe = true;
                } else {
                    should_unsubscribe = false;
                }
            } else {
                should_unsubscribe = false;
            }
        });

        if should_unsubscribe {
            let ws = self
                .get_ws_client_for_product(product_type)
                .context("no WebSocket client for product type")?
                .clone();

            self.spawn_ws(
                async move {
                    ws.unsubscribe_ticker(instrument_id)
                        .await
                        .context("ticker unsubscribe for mark prices")
                },
                "mark price unsubscribe",
            );
        }
        Ok(())
    }

    fn unsubscribe_index_prices(&mut self, cmd: &UnsubscribeIndexPrices) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;
        let product_type = self
            .get_product_type_for_instrument(instrument_id)
            .unwrap_or(BybitProductType::Linear);

        let mut should_unsubscribe = false;
        self.ticker_subs.rcu(|m| {
            if let Some(entry) = m.get_mut(&instrument_id) {
                entry.remove("index_prices");
                if entry.is_empty() {
                    m.remove(&instrument_id);
                    should_unsubscribe = true;
                } else {
                    should_unsubscribe = false;
                }
            } else {
                should_unsubscribe = false;
            }
        });

        if should_unsubscribe {
            let ws = self
                .get_ws_client_for_product(product_type)
                .context("no WebSocket client for product type")?
                .clone();

            self.spawn_ws(
                async move {
                    ws.unsubscribe_ticker(instrument_id)
                        .await
                        .context("ticker unsubscribe for index prices")
                },
                "index price unsubscribe",
            );
        }
        Ok(())
    }

    fn unsubscribe_bars(&mut self, cmd: &UnsubscribeBars) -> anyhow::Result<()> {
        let bar_type = cmd.bar_type;
        let instrument_id = bar_type.instrument_id();
        let product_type = self
            .get_product_type_for_instrument(instrument_id)
            .unwrap_or(BybitProductType::Linear);

        let ws = self
            .get_ws_client_for_product(product_type)
            .context("no WebSocket client for product type")?
            .clone();

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

    fn subscribe_option_greeks(&mut self, cmd: SubscribeOptionGreeks) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;
        self.option_greeks_subs.insert(instrument_id);

        let mut should_subscribe = false;
        self.ticker_subs.rcu(|m| {
            let entry = m.entry(instrument_id).or_default();
            should_subscribe = entry.is_empty();
            entry.insert("option_greeks");
        });

        if should_subscribe {
            let product_type = self
                .get_product_type_for_instrument(instrument_id)
                .unwrap_or(BybitProductType::Option);

            let ws = self
                .get_ws_client_for_product(product_type)
                .context("no WebSocket client for product type")?
                .clone();

            self.spawn_ws(
                async move {
                    ws.subscribe_ticker(instrument_id)
                        .await
                        .context("ticker subscription for option greeks")
                },
                "option greeks subscription",
            );
        }
        Ok(())
    }

    fn unsubscribe_option_greeks(&mut self, cmd: &UnsubscribeOptionGreeks) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;
        self.option_greeks_subs.remove(&instrument_id);

        let mut should_unsubscribe = false;
        self.ticker_subs.rcu(|m| {
            if let Some(entry) = m.get_mut(&instrument_id) {
                entry.remove("option_greeks");
                if entry.is_empty() {
                    m.remove(&instrument_id);
                    should_unsubscribe = true;
                } else {
                    should_unsubscribe = false;
                }
            } else {
                should_unsubscribe = false;
            }
        });

        if should_unsubscribe {
            let product_type = self
                .get_product_type_for_instrument(instrument_id)
                .unwrap_or(BybitProductType::Option);

            let ws = self
                .get_ws_client_for_product(product_type)
                .context("no WebSocket client for product type")?
                .clone();

            self.spawn_ws(
                async move {
                    ws.unsubscribe_ticker(instrument_id)
                        .await
                        .context("ticker unsubscribe for option greeks")
                },
                "option greeks unsubscribe",
            );
        }
        Ok(())
    }

    fn subscribe_instrument_status(
        &mut self,
        cmd: SubscribeInstrumentStatus,
    ) -> anyhow::Result<()> {
        log::debug!(
            "subscribe_instrument_status: {id} (status changes detected via periodic instrument info polling)",
            id = cmd.instrument_id,
        );
        self.instrument_status_subs.insert(cmd.instrument_id);
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
        self.instrument_status_subs.remove(&cmd.instrument_id);
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
        let product_types = if self.config.product_types.is_empty() {
            vec![BybitProductType::Linear]
        } else {
            self.config.product_types.clone()
        };

        get_runtime().spawn(async move {
            let mut all_instruments = Vec::new();

            for product_type in product_types {
                match http.request_instruments(product_type, None, None).await {
                    Ok(instruments) => {
                        for instrument in instruments {
                            upsert_instrument(&instruments_cache, instrument.clone());
                            all_instruments.push(instrument);
                        }
                    }
                    Err(e) => {
                        log::error!("Failed to fetch instruments for {product_type:?}: {e:?}");
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
                log::error!("Failed to send instruments response: {e}");
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

        let product_type = BybitProductType::from_suffix(instrument_id.symbol.as_str())
            .unwrap_or(BybitProductType::Linear);
        let raw_symbol = extract_raw_symbol(instrument_id.symbol.as_str()).to_string();

        get_runtime().spawn(async move {
            match http
                .request_instruments(product_type, Some(raw_symbol), None)
                .await
                .context("fetch instrument from API")
            {
                Ok(fetched) => {
                    if let Some(instrument) = fetched.into_iter().find(|i| i.id() == instrument_id)
                    {
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

    fn request_book_snapshot(&self, request: RequestBookSnapshot) -> anyhow::Result<()> {
        let http = self.http_client.clone();
        let sender = self.data_sender.clone();
        let instrument_id = request.instrument_id;
        let depth = request.depth.map(|n| n.get() as u32);
        let request_id = request.request_id;
        let client_id = request.client_id.unwrap_or(self.client_id);
        let params = request.params;
        let clock = self.clock;

        let product_type = BybitProductType::from_suffix(instrument_id.symbol.as_str())
            .unwrap_or(BybitProductType::Linear);

        get_runtime().spawn(async move {
            match http
                .request_orderbook_snapshot(product_type, instrument_id, depth)
                .await
                .context("failed to request book snapshot from Bybit")
            {
                Ok(deltas) => {
                    let mut book = OrderBook::new(instrument_id, BookType::L2_MBP);
                    if let Err(e) = book.apply_deltas(&deltas) {
                        log::error!("Failed to apply book deltas for {instrument_id}: {e}");
                        return;
                    }

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

    fn request_trades(&self, request: RequestTrades) -> anyhow::Result<()> {
        let http = self.http_client.clone();
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

        let product_type = BybitProductType::from_suffix(instrument_id.symbol.as_str())
            .unwrap_or(BybitProductType::Linear);

        get_runtime().spawn(async move {
            match http
                .request_trades(product_type, instrument_id, limit)
                .await
                .context("failed to request trades from Bybit")
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

        let instrument_id = bar_type.instrument_id();
        let product_type = BybitProductType::from_suffix(instrument_id.symbol.as_str())
            .unwrap_or(BybitProductType::Linear);

        get_runtime().spawn(async move {
            match http
                .request_bars(product_type, bar_type, start, end, limit, true)
                .await
                .context("failed to request bars from Bybit")
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

    fn request_funding_rates(&self, request: RequestFundingRates) -> anyhow::Result<()> {
        let http = self.http_client.clone();
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

        let product_type = BybitProductType::from_suffix(instrument_id.symbol.as_str())
            .unwrap_or(BybitProductType::Linear);

        if product_type == BybitProductType::Spot || product_type == BybitProductType::Option {
            anyhow::bail!("Funding rates not available for {product_type} instruments");
        }

        get_runtime().spawn(async move {
            match http
                .request_funding_rates(product_type, instrument_id, start, end, limit)
                .await
                .context("failed to request funding rates from Bybit")
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
                Err(e) => log::error!("Funding rates request failed for {instrument_id}: {e:?}"),
            }
        });

        Ok(())
    }

    fn request_forward_prices(&self, request: RequestForwardPrices) -> anyhow::Result<()> {
        let underlying = request.underlying.to_string();
        let instrument_id = request.instrument_id;
        let http_client = self.http_client.clone();
        let sender = self.data_sender.clone();
        let request_id = request.request_id;
        let client_id = self.client_id();
        let params = request.params;
        let clock = self.clock;
        let venue = *BYBIT_VENUE;

        get_runtime().spawn(async move {
            let result = if let Some(inst_id) = instrument_id {
                // Single-instrument path: fetch ticker for one symbol
                let raw_symbol = extract_raw_symbol(inst_id.symbol.as_str()).to_string();
                log::info!(
                    "Requesting forward price for {underlying} (single instrument: {raw_symbol})"
                );

                let params = crate::http::query::BybitTickersParams {
                    category: BybitProductType::Option,
                    symbol: Some(raw_symbol.clone()),
                    base_coin: None,
                    exp_date: None,
                };

                match http_client.request_option_tickers_raw_with_params(&params).await {
                    Ok(tickers) => {
                        let ts = clock.get_time_ns();
                        let forward_prices: Vec<ForwardPrice> = tickers
                            .into_iter()
                            .filter_map(|t| {
                                let up: Decimal = t.underlying_price.parse().ok()?;
                                if up.is_zero() {
                                    return None;
                                }
                                Some(ForwardPrice::new(inst_id, up, None, ts, ts))
                            })
                            .collect();

                        log::info!(
                            "Fetched {} forward price for {underlying} (single instrument: {raw_symbol})",
                            forward_prices.len(),
                        );
                        Ok((forward_prices, ts))
                    }
                    Err(e) => Err(e),
                }
            } else {
                // Bulk path: fetch all option tickers
                log::info!("Requesting option forward prices for base_coin={underlying} (bulk)");

                match http_client.request_option_tickers_raw(&underlying).await {
                    Ok(tickers) => {
                        let ts = clock.get_time_ns();

                        // Deduplicate: all options at the same expiry share the same
                        // forward price. Extract expiry prefix (e.g. "BTC-28FEB26" from
                        // "BTC-28FEB26-65000-C") and keep only one entry per expiry.
                        let mut seen_expiries = std::collections::HashSet::new();
                        let forward_prices: Vec<ForwardPrice> = tickers
                            .into_iter()
                            .filter_map(|t| {
                                let up: Decimal = t.underlying_price.parse().ok()?;
                                if up.is_zero() {
                                    return None;
                                }
                                let parts: Vec<&str> = t.symbol.splitn(3, '-').collect();
                                let expiry_key = if parts.len() >= 2 {
                                    format!("{}-{}", parts[0], parts[1])
                                } else {
                                    t.symbol.to_string()
                                };

                                if !seen_expiries.insert(expiry_key) {
                                    return None;
                                }
                                Some(ForwardPrice::new(
                                    BybitSymbol::new(format!("{}-OPTION", t.symbol))
                                        .map(|s| s.to_instrument_id())
                                        .ok()?,
                                    up,
                                    None,
                                    ts,
                                    ts,
                                ))
                            })
                            .collect();

                        log::info!(
                            "Fetched {} forward prices (per-expiry) for {underlying}",
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
                    log::error!("Forward prices request failed for {underlying}: {e:?}");
                }
            }
        });

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use ahash::{AHashMap, AHashSet};
    use nautilus_common::messages::DataEvent;
    use nautilus_core::{AtomicMap, AtomicSet, UnixNanos, time::get_atomic_clock_realtime};
    use nautilus_model::{
        data::{BarType, Data, QuoteTick},
        enums::AggressorSide,
        identifiers::InstrumentId,
        instruments::{Instrument, InstrumentAny},
        types::{Price, Quantity},
    };
    use rstest::rstest;
    use ustr::Ustr;

    use super::handle_ws_message;
    use crate::{
        common::{
            enums::BybitProductType,
            parse::{parse_linear_instrument, parse_option_instrument},
            testing::load_test_json,
        },
        http::models::{
            BybitFeeRate, BybitInstrumentLinearResponse, BybitInstrumentOptionResponse,
        },
        websocket::messages::{
            BybitWsMessage, BybitWsOrderbookDepthMsg, BybitWsTickerLinearMsg,
            BybitWsTickerOptionMsg, BybitWsTradeMsg,
        },
    };

    fn sample_fee_rate(
        symbol: &str,
        taker: &str,
        maker: &str,
        base_coin: Option<&str>,
    ) -> BybitFeeRate {
        BybitFeeRate {
            symbol: Ustr::from(symbol),
            taker_fee_rate: taker.to_string(),
            maker_fee_rate: maker.to_string(),
            base_coin: base_coin.map(Ustr::from),
        }
    }

    fn linear_instrument() -> InstrumentAny {
        let json = load_test_json("http_get_instruments_linear.json");
        let response: BybitInstrumentLinearResponse = serde_json::from_str(&json).unwrap();
        let instrument = &response.result.list[0];
        let fee_rate = sample_fee_rate("BTCUSDT", "0.00055", "0.0001", Some("BTC"));
        let ts = UnixNanos::new(1_700_000_000_000_000_000);
        parse_linear_instrument(instrument, &fee_rate, ts, ts).unwrap()
    }

    fn option_instrument() -> InstrumentAny {
        let json = load_test_json("http_get_instruments_option.json");
        let response: BybitInstrumentOptionResponse = serde_json::from_str(&json).unwrap();
        let instrument = &response.result.list[0];
        let ts = UnixNanos::new(1_700_000_000_000_000_000);
        parse_option_instrument(instrument, None, ts, ts).unwrap()
    }

    fn build_instruments(instruments: &[InstrumentAny]) -> AHashMap<Ustr, InstrumentAny> {
        let mut map = AHashMap::new();
        for inst in instruments {
            map.insert(inst.id().symbol.inner(), inst.clone());
        }
        map
    }

    #[expect(clippy::type_complexity)]
    fn empty_subs() -> (
        Arc<AtomicSet<InstrumentId>>,
        Arc<AtomicMap<InstrumentId, AHashSet<&'static str>>>,
        Arc<AtomicMap<InstrumentId, u32>>,
        Arc<AtomicMap<InstrumentId, u32>>,
        Arc<AtomicSet<InstrumentId>>,
        Arc<AtomicMap<String, BarType>>,
    ) {
        (
            Arc::new(AtomicSet::new()),
            Arc::new(AtomicMap::new()),
            Arc::new(AtomicMap::new()),
            Arc::new(AtomicMap::new()),
            Arc::new(AtomicSet::new()),
            Arc::new(AtomicMap::new()),
        )
    }

    #[rstest]
    fn test_handle_trade_message_emits_trade_tick() {
        let instrument = linear_instrument();
        let instruments = build_instruments(std::slice::from_ref(&instrument));
        let (trade_subs, ticker_subs, quote_depths, book_depths, greeks_subs, bar_types) =
            empty_subs();
        trade_subs.insert(instrument.id());
        let mut quote_cache = AHashMap::new();
        let mut funding_cache = AHashMap::new();
        let clock = get_atomic_clock_realtime();

        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();

        let json = load_test_json("ws_public_trade.json");
        let msg: BybitWsTradeMsg = serde_json::from_str(&json).unwrap();
        let ws_msg = BybitWsMessage::Trade(msg);

        handle_ws_message(
            &ws_msg,
            &tx,
            &instruments,
            Some(BybitProductType::Linear),
            &trade_subs,
            &ticker_subs,
            &quote_depths,
            &book_depths,
            &greeks_subs,
            &bar_types,
            &mut quote_cache,
            &mut funding_cache,
            clock,
        );

        let event = rx.try_recv().unwrap();
        match event {
            DataEvent::Data(Data::Trade(tick)) => {
                assert_eq!(tick.instrument_id, instrument.id());
                assert_eq!(tick.price, instrument.make_price(27451.00));
                assert_eq!(tick.size, instrument.make_qty(0.010, None));
                assert_eq!(tick.aggressor_side, AggressorSide::Buyer);
            }
            other => panic!("Expected Trade data event, found {other:?}"),
        }
    }

    #[rstest]
    fn test_handle_trade_message_unknown_symbol_no_event() {
        let instruments = AHashMap::new();
        let (trade_subs, ticker_subs, quote_depths, book_depths, greeks_subs, bar_types) =
            empty_subs();
        let mut quote_cache = AHashMap::new();
        let mut funding_cache = AHashMap::new();
        let clock = get_atomic_clock_realtime();

        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();

        let json = load_test_json("ws_public_trade.json");
        let msg: BybitWsTradeMsg = serde_json::from_str(&json).unwrap();
        let ws_msg = BybitWsMessage::Trade(msg);

        handle_ws_message(
            &ws_msg,
            &tx,
            &instruments,
            Some(BybitProductType::Linear),
            &trade_subs,
            &ticker_subs,
            &quote_depths,
            &book_depths,
            &greeks_subs,
            &bar_types,
            &mut quote_cache,
            &mut funding_cache,
            clock,
        );

        assert!(rx.try_recv().is_err());
    }

    #[rstest]
    fn test_handle_orderbook_message_emits_deltas_and_quote() {
        let instrument = linear_instrument();
        let instrument_id = instrument.id();
        let instruments = build_instruments(&[instrument]);
        let (trade_subs, ticker_subs, quote_depths, book_depths, greeks_subs, bar_types) =
            empty_subs();

        book_depths.insert(instrument_id, 1);
        quote_depths.insert(instrument_id, 1);

        let mut quote_cache = AHashMap::new();
        let mut funding_cache = AHashMap::new();
        let clock = get_atomic_clock_realtime();

        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();

        let json = load_test_json("ws_orderbook_snapshot.json");
        let msg: BybitWsOrderbookDepthMsg = serde_json::from_str(&json).unwrap();
        let ws_msg = BybitWsMessage::Orderbook(msg);

        handle_ws_message(
            &ws_msg,
            &tx,
            &instruments,
            Some(BybitProductType::Linear),
            &trade_subs,
            &ticker_subs,
            &quote_depths,
            &book_depths,
            &greeks_subs,
            &bar_types,
            &mut quote_cache,
            &mut funding_cache,
            clock,
        );

        let event1 = rx.try_recv().unwrap();
        assert!(matches!(event1, DataEvent::Data(Data::Deltas(_))));

        let event2 = rx.try_recv().unwrap();
        assert!(matches!(event2, DataEvent::Data(Data::Quote(_))));
    }

    #[rstest]
    fn test_handle_orderbook_message_no_sub_no_event() {
        let instrument = linear_instrument();
        let instruments = build_instruments(&[instrument]);
        let (trade_subs, ticker_subs, quote_depths, book_depths, greeks_subs, bar_types) =
            empty_subs();
        let mut quote_cache = AHashMap::new();
        let mut funding_cache = AHashMap::new();
        let clock = get_atomic_clock_realtime();

        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();

        let json = load_test_json("ws_orderbook_snapshot.json");
        let msg: BybitWsOrderbookDepthMsg = serde_json::from_str(&json).unwrap();
        let ws_msg = BybitWsMessage::Orderbook(msg);

        handle_ws_message(
            &ws_msg,
            &tx,
            &instruments,
            Some(BybitProductType::Linear),
            &trade_subs,
            &ticker_subs,
            &quote_depths,
            &book_depths,
            &greeks_subs,
            &bar_types,
            &mut quote_cache,
            &mut funding_cache,
            clock,
        );

        assert!(rx.try_recv().is_err());
    }

    #[rstest]
    fn test_handle_ticker_linear_emits_quote() {
        let instrument = linear_instrument();
        let instrument_id = instrument.id();
        let instruments = build_instruments(&[instrument]);
        let (trade_subs, ticker_subs, quote_depths, book_depths, greeks_subs, bar_types) =
            empty_subs();

        let mut subs = AHashSet::new();
        subs.insert("quotes");
        ticker_subs.insert(instrument_id, subs);

        let mut quote_cache = AHashMap::new();
        let mut funding_cache = AHashMap::new();
        let clock = get_atomic_clock_realtime();

        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();

        let json = load_test_json("ws_ticker_linear.json");
        let msg: BybitWsTickerLinearMsg = serde_json::from_str(&json).unwrap();
        let ws_msg = BybitWsMessage::TickerLinear(msg);

        handle_ws_message(
            &ws_msg,
            &tx,
            &instruments,
            Some(BybitProductType::Linear),
            &trade_subs,
            &ticker_subs,
            &quote_depths,
            &book_depths,
            &greeks_subs,
            &bar_types,
            &mut quote_cache,
            &mut funding_cache,
            clock,
        );

        let event = rx.try_recv().unwrap();
        assert!(matches!(event, DataEvent::Data(Data::Quote(_))));
        assert!(quote_cache.contains_key(&instrument_id));
    }

    #[rstest]
    fn test_handle_ticker_linear_funding_dedup() {
        let instrument = linear_instrument();
        let instrument_id = instrument.id();
        let instruments = build_instruments(&[instrument]);
        let (trade_subs, ticker_subs, quote_depths, book_depths, greeks_subs, bar_types) =
            empty_subs();

        let mut subs = AHashSet::new();
        subs.insert("funding");
        ticker_subs.insert(instrument_id, subs);

        let mut quote_cache = AHashMap::new();
        let mut funding_cache = AHashMap::new();
        let clock = get_atomic_clock_realtime();

        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();

        let json = load_test_json("ws_ticker_linear.json");
        let msg: BybitWsTickerLinearMsg = serde_json::from_str(&json).unwrap();
        let ws_msg = BybitWsMessage::TickerLinear(msg.clone());

        handle_ws_message(
            &ws_msg,
            &tx,
            &instruments,
            Some(BybitProductType::Linear),
            &trade_subs,
            &ticker_subs,
            &quote_depths,
            &book_depths,
            &greeks_subs,
            &bar_types,
            &mut quote_cache,
            &mut funding_cache,
            clock,
        );

        let event = rx.try_recv().unwrap();
        assert!(matches!(event, DataEvent::FundingRate(_)));

        // Send same message again, funding unchanged so should be deduped
        let ws_msg2 = BybitWsMessage::TickerLinear(msg);
        handle_ws_message(
            &ws_msg2,
            &tx,
            &instruments,
            Some(BybitProductType::Linear),
            &trade_subs,
            &ticker_subs,
            &quote_depths,
            &book_depths,
            &greeks_subs,
            &bar_types,
            &mut quote_cache,
            &mut funding_cache,
            clock,
        );

        assert!(rx.try_recv().is_err());
    }

    #[rstest]
    fn test_handle_ticker_linear_mark_and_index_prices() {
        let instrument = linear_instrument();
        let instrument_id = instrument.id();
        let instruments = build_instruments(&[instrument]);
        let (trade_subs, ticker_subs, quote_depths, book_depths, greeks_subs, bar_types) =
            empty_subs();

        let mut subs = AHashSet::new();
        subs.insert("mark_prices");
        subs.insert("index_prices");
        ticker_subs.insert(instrument_id, subs);

        let mut quote_cache = AHashMap::new();
        let mut funding_cache = AHashMap::new();
        let clock = get_atomic_clock_realtime();

        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();

        let json = load_test_json("ws_ticker_linear.json");
        let msg: BybitWsTickerLinearMsg = serde_json::from_str(&json).unwrap();
        let ws_msg = BybitWsMessage::TickerLinear(msg);

        handle_ws_message(
            &ws_msg,
            &tx,
            &instruments,
            Some(BybitProductType::Linear),
            &trade_subs,
            &ticker_subs,
            &quote_depths,
            &book_depths,
            &greeks_subs,
            &bar_types,
            &mut quote_cache,
            &mut funding_cache,
            clock,
        );

        let event1 = rx.try_recv().unwrap();
        assert!(matches!(event1, DataEvent::Data(Data::MarkPriceUpdate(_))));

        let event2 = rx.try_recv().unwrap();
        assert!(matches!(event2, DataEvent::Data(Data::IndexPriceUpdate(_))));
    }

    #[rstest]
    fn test_handle_reconnected_clears_caches() {
        let instruments = AHashMap::new();
        let (trade_subs, ticker_subs, quote_depths, book_depths, greeks_subs, bar_types) =
            empty_subs();
        let mut quote_cache = AHashMap::new();
        let mut funding_cache = AHashMap::new();
        let clock = get_atomic_clock_realtime();

        let instrument_id = InstrumentId::from("BTCUSDT-LINEAR.BYBIT");
        quote_cache.insert(
            instrument_id,
            QuoteTick::new(
                instrument_id,
                Price::from("100.00"),
                Price::from("101.00"),
                Quantity::from("1.0"),
                Quantity::from("1.0"),
                UnixNanos::default(),
                UnixNanos::default(),
            ),
        );
        funding_cache.insert(
            Ustr::from("BTCUSDT"),
            (
                Some("-0.001".to_string()),
                Some("1000".to_string()),
                Some("8".to_string()),
            ),
        );

        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();

        handle_ws_message(
            &BybitWsMessage::Reconnected,
            &tx,
            &instruments,
            None,
            &trade_subs,
            &ticker_subs,
            &quote_depths,
            &book_depths,
            &greeks_subs,
            &bar_types,
            &mut quote_cache,
            &mut funding_cache,
            clock,
        );

        assert!(quote_cache.is_empty());
        assert!(funding_cache.is_empty());
    }

    #[rstest]
    fn test_handle_ticker_option_greeks() {
        // Use the option instrument but key it by the ticker fixture symbol
        // (fixture instrument is ETH-26JUN26-16000-P, ticker fixture is BTC-6JAN23-17500-C)
        let instrument = option_instrument();
        let instrument_id = instrument.id();

        // Key the instrument by the fixture ticker symbol with OPTION suffix
        let ticker_key = Ustr::from("BTC-6JAN23-17500-C-OPTION");
        let mut instruments = AHashMap::new();
        instruments.insert(ticker_key, instrument);

        let (trade_subs, ticker_subs, quote_depths, book_depths, greeks_subs, bar_types) =
            empty_subs();
        greeks_subs.insert(instrument_id);

        let mut quote_cache = AHashMap::new();
        let mut funding_cache = AHashMap::new();
        let clock = get_atomic_clock_realtime();

        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();

        let json = load_test_json("ws_ticker_option.json");
        let msg: BybitWsTickerOptionMsg = serde_json::from_str(&json).unwrap();
        let ws_msg = BybitWsMessage::TickerOption(msg);

        handle_ws_message(
            &ws_msg,
            &tx,
            &instruments,
            Some(BybitProductType::Option),
            &trade_subs,
            &ticker_subs,
            &quote_depths,
            &book_depths,
            &greeks_subs,
            &bar_types,
            &mut quote_cache,
            &mut funding_cache,
            clock,
        );

        let event = rx.try_recv().unwrap();
        assert!(matches!(event, DataEvent::OptionGreeks(_)));
    }

    #[rstest]
    fn test_handle_execution_message_ignored_by_data() {
        let instruments = AHashMap::new();
        let (trade_subs, ticker_subs, quote_depths, book_depths, greeks_subs, bar_types) =
            empty_subs();
        let mut quote_cache = AHashMap::new();
        let mut funding_cache = AHashMap::new();
        let clock = get_atomic_clock_realtime();

        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();

        let json = load_test_json("ws_account_order.json");
        let msg: crate::websocket::messages::BybitWsAccountOrderMsg =
            serde_json::from_str(&json).unwrap();
        let ws_msg = BybitWsMessage::AccountOrder(msg);

        handle_ws_message(
            &ws_msg,
            &tx,
            &instruments,
            None,
            &trade_subs,
            &ticker_subs,
            &quote_depths,
            &book_depths,
            &greeks_subs,
            &bar_types,
            &mut quote_cache,
            &mut funding_cache,
            clock,
        );

        assert!(rx.try_recv().is_err());
    }

    #[rstest]
    fn test_instrument_resolution_with_product_type() {
        let instrument = linear_instrument();

        let mut map = AHashMap::new();
        map.insert(instrument.id().symbol.inner(), instrument.clone());

        let (trade_subs, ticker_subs, quote_depths, book_depths, greeks_subs, bar_types) =
            empty_subs();
        trade_subs.insert(instrument.id());
        let mut quote_cache = AHashMap::new();
        let mut funding_cache = AHashMap::new();
        let clock = get_atomic_clock_realtime();
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();

        let json = load_test_json("ws_public_trade.json");
        let msg: BybitWsTradeMsg = serde_json::from_str(&json).unwrap();

        // With None product_type, raw symbol "BTCUSDT" does not match "BTCUSDT-LINEAR"
        handle_ws_message(
            &BybitWsMessage::Trade(msg.clone()),
            &tx,
            &map,
            None,
            &trade_subs,
            &ticker_subs,
            &quote_depths,
            &book_depths,
            &greeks_subs,
            &bar_types,
            &mut quote_cache,
            &mut funding_cache,
            clock,
        );
        assert!(rx.try_recv().is_err());

        // With product_type=Linear, "BTCUSDT" -> "BTCUSDT-LINEAR" matches
        handle_ws_message(
            &BybitWsMessage::Trade(msg),
            &tx,
            &map,
            Some(BybitProductType::Linear),
            &trade_subs,
            &ticker_subs,
            &quote_depths,
            &book_depths,
            &greeks_subs,
            &bar_types,
            &mut quote_cache,
            &mut funding_cache,
            clock,
        );

        let event = rx.try_recv().unwrap();
        assert!(matches!(event, DataEvent::Data(Data::Trade(_))));
    }

    #[rstest]
    fn test_handle_trade_filters_by_subscription() {
        let instrument = linear_instrument();
        let instruments = build_instruments(std::slice::from_ref(&instrument));
        let (trade_subs, ticker_subs, quote_depths, book_depths, greeks_subs, bar_types) =
            empty_subs();
        let mut quote_cache = AHashMap::new();
        let mut funding_cache = AHashMap::new();
        let clock = get_atomic_clock_realtime();
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();

        let json = load_test_json("ws_public_trade.json");
        let msg: BybitWsTradeMsg = serde_json::from_str(&json).unwrap();

        // Without subscription, trade should be filtered out
        handle_ws_message(
            &BybitWsMessage::Trade(msg.clone()),
            &tx,
            &instruments,
            Some(BybitProductType::Linear),
            &trade_subs,
            &ticker_subs,
            &quote_depths,
            &book_depths,
            &greeks_subs,
            &bar_types,
            &mut quote_cache,
            &mut funding_cache,
            clock,
        );
        assert!(rx.try_recv().is_err());

        // With subscription, trade should be emitted
        trade_subs.insert(instrument.id());
        handle_ws_message(
            &BybitWsMessage::Trade(msg),
            &tx,
            &instruments,
            Some(BybitProductType::Linear),
            &trade_subs,
            &ticker_subs,
            &quote_depths,
            &book_depths,
            &greeks_subs,
            &bar_types,
            &mut quote_cache,
            &mut funding_cache,
            clock,
        );
        let event = rx.try_recv().unwrap();
        assert!(matches!(event, DataEvent::Data(Data::Trade(_))));
    }
}
