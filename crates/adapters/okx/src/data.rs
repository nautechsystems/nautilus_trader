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

//! Live market data client implementation for the OKX adapter.

use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use ahash::{AHashMap, AHashSet};
use anyhow::Context;
use futures_util::{StreamExt, pin_mut};
use nautilus_common::{
    cache::quote::QuoteCache,
    clients::DataClient,
    live::{runner::get_data_event_sender, task::TaskHandles},
    messages::{
        DataEvent,
        data::{
            BarsResponse, BookResponse, DataResponse, ForwardPricesResponse, FundingRatesResponse,
            InstrumentResponse, InstrumentsResponse, RequestBars, RequestBookSnapshot,
            RequestForwardPrices, RequestFundingRates, RequestInstrument, RequestInstruments,
            RequestTrades, SubscribeBars, SubscribeBookDeltas, SubscribeFundingRates,
            SubscribeIndexPrices, SubscribeInstrument, SubscribeInstrumentStatus,
            SubscribeInstruments, SubscribeMarkPrices, SubscribeOptionGreeks, SubscribeQuotes,
            SubscribeTrades, TradesResponse, UnsubscribeBars, UnsubscribeBookDeltas,
            UnsubscribeFundingRates, UnsubscribeIndexPrices, UnsubscribeInstrument,
            UnsubscribeInstrumentStatus, UnsubscribeMarkPrices, UnsubscribeOptionGreeks,
            UnsubscribeQuotes, UnsubscribeTrades,
        },
    },
};
use nautilus_core::{
    AtomicMap, Params, UnixNanos,
    datetime::datetime_to_unix_nanos,
    time::{AtomicTime, get_atomic_clock_realtime},
};
use nautilus_live::SocketControl;
use nautilus_model::{
    data::{Data, FundingRateUpdate, InstrumentStatus},
    enums::{BookType, GreeksConvention, MarketStatusAction},
    identifiers::{ClientId, InstrumentId, Venue},
    instruments::{Instrument, InstrumentAny},
};
use tokio_util::sync::CancellationToken;
use ustr::Ustr;

use crate::{
    book_sync::{
        BookChannelScope, BookSequenceOutcome, BookSyncSignal, BookSyncSignalKind, BookSyncTracker,
    },
    common::{
        consts::{
            OKX_VENUE, OKX_WS_HEARTBEAT_SECS, resolve_book_depth, resolve_instrument_families,
            select_book_channel, should_retry_error_code,
        },
        enums::{
            OKXBookAction, OKXBookChannel, OKXContractType, OKXGreeksType, OKXInstrumentStatus,
            OKXInstrumentType, OKXVipLevel,
        },
        models::OKXInstrument,
        parse::{
            extract_inst_family, is_okx_spread_symbol, okx_instrument_type_from_symbol,
            okx_status_to_market_action, parse_base_quote_from_symbol, parse_instrument_any,
            parse_instrument_id, parse_millisecond_timestamp, parse_price, parse_quantity,
        },
        task::{spawn_task, terminate_tasks},
    },
    config::OKXDataClientConfig,
    http::{
        client::{OKXHttpClient, OKXInstrumentDefinitionError},
        query::GetSpreadsParams,
    },
    websocket::{
        client::OKXWebSocketClient,
        enums::OKXWsChannel,
        messages::{NautilusWsMessage, OKXBookMsg, OKXOptionSummaryMsg, OKXWsMessage},
        parse::{
            extract_fees_from_cached_instrument, parse_book_msg_vec, parse_index_price_msg_vec,
            parse_option_summary_greeks, parse_rpi_book_msg_vec, parse_ws_message_data,
        },
    },
};

#[derive(Debug)]
pub struct OKXDataClient {
    client_id: ClientId,
    config: OKXDataClientConfig,
    http_client: OKXHttpClient,
    ws_public: Option<OKXWebSocketClient>,
    ws_business: Option<OKXWebSocketClient>,
    is_connected: AtomicBool,
    transports_started: bool,
    cancellation_token: CancellationToken,
    tasks: Arc<TaskHandles>,
    data_sender: tokio::sync::mpsc::UnboundedSender<DataEvent>,
    // Shared instrument cache keyed by raw symbol so stream tasks, reconciliation,
    // and request paths all read and write one source of truth
    instruments_by_symbol: Arc<AtomicMap<Ustr, InstrumentAny>>,
    // Serializes instrument diff-update-publish sequences between the stream
    // tasks and the refresh task; only held for synchronous sections
    instrument_update_lock: Arc<InstrumentUpdateLock>,
    book_channels: Arc<AtomicMap<InstrumentId, OKXBookChannel>>,
    book_sync: BookSyncTracker,
    index_ticker_map: Arc<AtomicMap<Ustr, AHashSet<Ustr>>>,
    option_greeks_subs: Arc<AtomicMap<InstrumentId, AHashSet<OKXGreeksType>>>,
    // `Mutex<AHashMap>` so the spawned subscribe task can roll back the
    // refcount on failure. A bare `AHashMap` would leave the count
    // permanently incremented and wedge future Greeks subscribes.
    option_summary_family_subs: Arc<std::sync::Mutex<AHashMap<Ustr, usize>>>,
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
                Some(config.http_base_url()),
                config.http_timeout_secs,
                config.max_retries,
                config.retry_delay_initial_ms,
                config.retry_delay_max_ms,
                config.environment,
                config.proxy_url.clone(),
            )?
        } else {
            OKXHttpClient::new(
                Some(config.http_base_url()),
                config.http_timeout_secs,
                config.max_retries,
                config.retry_delay_initial_ms,
                config.retry_delay_max_ms,
                config.environment,
                config.proxy_url.clone(),
            )?
        };

        let ws_public = OKXWebSocketClient::new(
            Some(config.ws_public_url()),
            None,
            None,
            None,
            None,
            Some(OKX_WS_HEARTBEAT_SECS),
            None,
            config.transport_backend,
            config.proxy_url.clone(),
        )
        .context("failed to construct OKX public websocket client")?
        .with_socket_control(SocketControl::new(
            client_id,
            Some(*OKX_VENUE),
            "okx-public-data-streams",
        ));

        let ws_business = if config.requires_business_ws() {
            let ws = OKXWebSocketClient::new(
                Some(config.ws_business_url()),
                None, // No auth needed for public business channels
                None,
                None,
                None,
                Some(OKX_WS_HEARTBEAT_SECS),
                None,
                config.transport_backend,
                config.proxy_url.clone(),
            )
            .context("failed to construct OKX business websocket client")?
            .with_socket_control(SocketControl::new(
                client_id,
                Some(*OKX_VENUE),
                "okx-business-data-streams",
            ));
            Some(ws)
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
            transports_started: false,
            cancellation_token: CancellationToken::new(),
            tasks: Arc::new(TaskHandles::default()),
            data_sender,
            instruments_by_symbol: Arc::new(AtomicMap::new()),
            instrument_update_lock: Arc::new(InstrumentUpdateLock::default()),
            book_channels: Arc::new(AtomicMap::new()),
            book_sync: BookSyncTracker::default(),
            index_ticker_map: Arc::new(AtomicMap::new()),
            option_greeks_subs: Arc::new(AtomicMap::new()),
            option_summary_family_subs: Arc::new(std::sync::Mutex::new(AHashMap::new())),
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
            log::error!("Failed to emit data event: {e}");
        }
    }

    fn spawn_ws<F>(&self, fut: F, context: &'static str)
    where
        F: Future<Output = anyhow::Result<()>> + Send + 'static,
    {
        let fut = async move {
            if let Err(e) = fut.await {
                log::error!("{context}: {e:?}");
            }
        };
        spawn_task(&self.tasks, &self.cancellation_token, fut);
    }

    fn terminate_generation(&self, abort_tasks: bool) {
        self.cancellation_token.cancel();

        if abort_tasks {
            self.tasks.abort_all_retained();
        }

        if let Some(ws) = self.ws_public.as_ref() {
            ws.abort();
        }

        if let Some(ws) = self.ws_business.as_ref() {
            ws.abort();
        }

        self.is_connected.store(false, Ordering::Release);
    }

    fn spawn_book_health_monitor(&self) {
        let interval_duration = Duration::from_secs(self.config.book_stale_check_interval_secs);
        let threshold = Duration::from_secs(self.config.book_stale_threshold_secs);

        if interval_duration.is_zero() || threshold.is_zero() {
            return;
        }

        let book_sync = self.book_sync.clone();
        let cancel = self.cancellation_token.clone();

        spawn_task(&self.tasks, &self.cancellation_token, async move {
            let mut interval = tokio::time::interval(interval_duration);

            loop {
                tokio::select! {
                    biased;
                    () = cancel.cancelled() => {
                        log::debug!("Book health monitor task cancelled");
                        break;
                    }
                    _ = interval.tick() => {
                        handle_book_sync_signals(
                            book_sync.stale_books(threshold, Instant::now())
                        );
                    }
                }
            }
        });
    }

    #[expect(clippy::too_many_arguments)]
    fn handle_ws_message(
        message: OKXWsMessage,
        data_sender: &tokio::sync::mpsc::UnboundedSender<DataEvent>,
        instruments_by_symbol: &Arc<AtomicMap<Ustr, InstrumentAny>>,
        http_client: &OKXHttpClient,
        config: &OKXDataClientConfig,
        instrument_update_lock: &InstrumentUpdateLock,
        book_channels: &Arc<AtomicMap<InstrumentId, OKXBookChannel>>,
        book_sync: &BookSyncTracker,
        recovery_ws: Option<&OKXWebSocketClient>,
        business_ws: Option<&OKXWebSocketClient>,
        quote_cache: &mut QuoteCache,
        funding_cache: &mut AHashMap<Ustr, (Ustr, u64)>,
        index_ticker_map: &Arc<AtomicMap<Ustr, AHashSet<Ustr>>>,
        option_greeks_subs: &Arc<AtomicMap<InstrumentId, AHashSet<OKXGreeksType>>>,
        book_channel_scope: BookChannelScope,
        snapshot_timeout: Duration,
        tasks: &TaskHandles,
        cancel: &CancellationToken,
        clock: &AtomicTime,
    ) {
        match message {
            OKXWsMessage::BookData { arg, action, data } => {
                let Some(inst_id) = arg.inst_id else {
                    log::warn!("Book data without inst_id");
                    return;
                };
                let instruments_guard = instruments_by_symbol.load();
                let Some(instrument) = instruments_guard.get(&inst_id) else {
                    log::warn!("No cached instrument for book data: {inst_id}");
                    return;
                };
                let ts_init = clock.get_time_ns();
                let sequences = data
                    .iter()
                    .map(|msg| (msg.prev_seq_id, msg.seq_id))
                    .collect::<Vec<_>>();

                match parse_book_msg_vec(
                    data,
                    &instrument.id(),
                    instrument.price_precision(),
                    instrument.size_precision(),
                    action,
                    ts_init,
                ) {
                    Ok(data_vec) => {
                        let outcome = book_sync.validate_sequence_if_subscribed(
                            book_channels,
                            instrument.id(),
                            action == OKXBookAction::Snapshot,
                            &sequences,
                            snapshot_timeout,
                            Instant::now(),
                        );

                        if !handle_book_sequence_outcome(
                            outcome,
                            instrument.id(),
                            book_channels,
                            book_sync,
                            recovery_ws,
                            snapshot_timeout,
                            tasks,
                            cancel,
                        ) {
                            return;
                        }

                        for data in data_vec {
                            Self::send_data(data_sender, data);
                        }
                    }
                    Err(e) => log::error!("Failed to parse book data: {e}"),
                }
            }
            OKXWsMessage::RpiBookData { arg, action, data } => {
                let Some(inst_id) = arg.inst_id else {
                    log::warn!("RPI book data without inst_id");
                    return;
                };
                let instruments_guard = instruments_by_symbol.load();
                let Some(instrument) = instruments_guard.get(&inst_id) else {
                    log::warn!("No cached instrument for RPI book data: {inst_id}");
                    return;
                };
                let ts_init = clock.get_time_ns();
                let sequences = data
                    .iter()
                    .map(|msg| (Some(msg.prev_seq_id), msg.seq_id))
                    .collect::<Vec<_>>();

                match parse_rpi_book_msg_vec(
                    data,
                    &instrument.id(),
                    instrument.price_precision(),
                    instrument.size_precision(),
                    action,
                    ts_init,
                ) {
                    Ok(data_vec) => {
                        let outcome = book_sync.validate_sequence_if_subscribed(
                            book_channels,
                            instrument.id(),
                            action == OKXBookAction::Snapshot,
                            &sequences,
                            snapshot_timeout,
                            Instant::now(),
                        );

                        if !handle_book_sequence_outcome(
                            outcome,
                            instrument.id(),
                            book_channels,
                            book_sync,
                            recovery_ws,
                            snapshot_timeout,
                            tasks,
                            cancel,
                        ) {
                            return;
                        }

                        for data in data_vec {
                            Self::send_data(data_sender, data);
                        }
                    }
                    Err(e) => log::error!("Failed to parse RPI book data: {e}"),
                }
            }
            OKXWsMessage::ChannelData {
                channel,
                inst_id,
                data,
            } => {
                // Option summary subscriptions use instFamily (not instId), so
                // the arg has inst_id: None. Each element in the data array carries
                // its own inst_id that we resolve per-message.
                if matches!(channel, OKXWsChannel::OptionSummary) {
                    let ts_init = clock.get_time_ns();

                    match serde_json::from_value::<Vec<OKXOptionSummaryMsg>>(data) {
                        Ok(msgs) => {
                            let subs = option_greeks_subs.load();
                            let instruments_guard = instruments_by_symbol.load();

                            for msg in &msgs {
                                let Some(instrument) = instruments_guard.get(&msg.inst_id) else {
                                    continue;
                                };
                                let instrument_id = instrument.id();
                                let Some(conventions) = subs.get(&instrument_id) else {
                                    continue;
                                };

                                for greeks_type in conventions {
                                    match parse_option_summary_greeks(
                                        msg,
                                        &instrument_id,
                                        *greeks_type,
                                        ts_init,
                                    ) {
                                        Ok(greeks) => {
                                            if let Err(e) =
                                                data_sender.send(DataEvent::OptionGreeks(greeks))
                                            {
                                                log::error!(
                                                    "Failed to emit option greeks event: {e}"
                                                );
                                            }
                                        }
                                        Err(e) => {
                                            log::error!(
                                                "Failed to parse option summary for {} ({greeks_type:?}): {e}",
                                                msg.inst_id
                                            );
                                        }
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            log::error!("Failed to deserialize option summary data: {e}");
                        }
                    }
                    return;
                }

                let Some(inst_id) = inst_id else {
                    log::debug!("Channel data without inst_id: {channel:?}");
                    return;
                };

                // Index tickers use base pair format (e.g., "BTC-USDT") but instruments
                // are keyed by full symbol (e.g., "BTC-USDT-SWAP"). Dispatch index price
                // updates only to instruments that subscribed via subscribe_index_prices.
                if matches!(channel, OKXWsChannel::IndexTickers) {
                    let ts_init = clock.get_time_ns();
                    let map_guard = index_ticker_map.load();
                    let Some(subscribed_symbols) = map_guard.get(&inst_id) else {
                        log::debug!("No subscribed instruments for index ticker: {inst_id}");
                        return;
                    };
                    let symbols: Vec<Ustr> = subscribed_symbols.iter().copied().collect();
                    drop(map_guard);

                    let instruments_guard = instruments_by_symbol.load();

                    for sym in &symbols {
                        let Some(instrument) = instruments_guard.get(sym) else {
                            log::warn!("No cached instrument for index ticker symbol: {sym}");
                            continue;
                        };

                        match parse_index_price_msg_vec(
                            data.clone(),
                            &instrument.id(),
                            instrument.price_precision(),
                            ts_init,
                        ) {
                            Ok(data_vec) => {
                                for d in data_vec {
                                    Self::send_data(data_sender, d);
                                }
                            }
                            Err(e) => log::error!("Failed to parse index price data: {e}"),
                        }
                    }
                    return;
                }

                let instruments_guard = instruments_by_symbol.load();
                let Some(instrument) = instruments_guard.get(&inst_id) else {
                    log::warn!("No cached instrument for {channel:?}: {inst_id}");
                    return;
                };
                let instrument_id = instrument.id();
                let price_precision = instrument.price_precision();
                let size_precision = instrument.size_precision();
                let ts_init = clock.get_time_ns();

                if matches!(channel, OKXWsChannel::SprdBooks5) {
                    let msgs: Vec<OKXBookMsg> = match serde_json::from_value(data) {
                        Ok(m) => m,
                        Err(e) => {
                            log::error!("Failed to deserialize spread book data: {e}");
                            return;
                        }
                    };

                    // sprd-books5 pushes a full 5-level snapshot each message.
                    match parse_book_msg_vec(
                        msgs,
                        &instrument_id,
                        price_precision,
                        size_precision,
                        OKXBookAction::Snapshot,
                        ts_init,
                    ) {
                        Ok(data_vec) => {
                            book_sync.record_update_if_subscribed(
                                book_channels,
                                instrument_id,
                                true,
                                Instant::now(),
                            );

                            for d in data_vec {
                                Self::send_data(data_sender, d);
                            }
                        }
                        Err(e) => log::error!("Failed to parse spread book data: {e}"),
                    }

                    return;
                }

                if matches!(channel, OKXWsChannel::BboTbt | OKXWsChannel::SprdBboTbt) {
                    let msgs: Vec<OKXBookMsg> = match serde_json::from_value(data) {
                        Ok(m) => m,
                        Err(e) => {
                            log::error!("Failed to deserialize BboTbt data: {e}");
                            return;
                        }
                    };

                    for msg in &msgs {
                        let bid = msg.bids.first();
                        let ask = msg.asks.first();
                        let bid_price =
                            bid.and_then(|e| parse_price(&e.price, price_precision).ok());
                        let bid_size =
                            bid.and_then(|e| parse_quantity(&e.size, size_precision).ok());
                        let ask_price =
                            ask.and_then(|e| parse_price(&e.price, price_precision).ok());
                        let ask_size =
                            ask.and_then(|e| parse_quantity(&e.size, size_precision).ok());
                        let ts_event = parse_millisecond_timestamp(msg.ts);

                        match quote_cache.process(
                            instrument_id,
                            bid_price,
                            ask_price,
                            bid_size,
                            ask_size,
                            ts_event,
                            ts_init,
                        ) {
                            Ok(quote) => Self::send_data(data_sender, Data::Quote(quote)),
                            Err(e) => {
                                log::debug!("Skipping partial BboTbt for {instrument_id}: {e}");
                            }
                        }
                    }

                    return;
                }

                match parse_ws_message_data(
                    &channel,
                    data,
                    &instrument_id,
                    price_precision,
                    size_precision,
                    ts_init,
                    funding_cache,
                    &instruments_guard,
                ) {
                    Ok(Some(ws_msg)) => {
                        dispatch_parsed_data(ws_msg, data_sender, instruments_by_symbol);
                    }
                    Ok(None) => {}
                    Err(e) => log::error!("Failed to parse {channel:?} data: {e}"),
                }
            }
            OKXWsMessage::Instruments(okx_instruments) => {
                let ts_init = clock.get_time_ns();
                // Hold the instrument lock for the batch so a concurrent
                // reconciliation cannot interleave diff, cache update, and publish
                let _update_guard = instrument_update_lock
                    .mutex
                    .lock()
                    .expect("instrument update lock poisoned");

                for okx_inst in okx_instruments {
                    let inst_key = okx_inst.inst_id;
                    let cached = instruments_by_symbol.get_cloned(&inst_key);
                    let (margin_init, margin_maint, maker_fee, taker_fee) = cached
                        .as_ref()
                        .map_or((None, None, None, None), |instrument| {
                            extract_fees_from_cached_instrument(instrument)
                        });
                    let status_action = okx_status_to_market_action(okx_inst.state);
                    let is_live = matches!(okx_inst.state, OKXInstrumentStatus::Live);
                    match parse_instrument_any(
                        &okx_inst,
                        margin_init,
                        margin_maint,
                        maker_fee,
                        taker_fee,
                        ts_init,
                    ) {
                        Ok(Some(inst_any)) => {
                            let instrument_id = inst_any.id();
                            let is_new_or_changed = cached.is_none_or(|cached| {
                                !instrument_definitions_match(&cached, &inst_any)
                            });

                            if is_new_or_changed
                                && definition_in_scope(config, &okx_inst, &inst_any)
                            {
                                publish_instrument_updates(
                                    std::slice::from_ref(&inst_any),
                                    instruments_by_symbol,
                                    http_client,
                                    recovery_ws,
                                    business_ws,
                                    instrument_update_lock,
                                    data_sender,
                                );
                            }

                            emit_instrument_status(
                                data_sender,
                                instrument_id,
                                status_action,
                                is_live,
                                ts_init,
                            );
                        }
                        Ok(None) => {
                            let instrument_id = instruments_by_symbol
                                .get_cloned(&inst_key)
                                .map_or_else(|| parse_instrument_id(inst_key), |i| i.id());
                            emit_instrument_status(
                                data_sender,
                                instrument_id,
                                status_action,
                                is_live,
                                ts_init,
                            );
                        }
                        Err(e) => {
                            log::warn!("Failed to parse instrument {}: {e}", okx_inst.inst_id);
                            let instrument_id = instruments_by_symbol
                                .get_cloned(&inst_key)
                                .map_or_else(|| parse_instrument_id(inst_key), |i| i.id());
                            emit_instrument_status(
                                data_sender,
                                instrument_id,
                                status_action,
                                is_live,
                                ts_init,
                            );
                        }
                    }
                }
            }
            OKXWsMessage::Orders(_)
            | OKXWsMessage::SpreadOrders(_)
            | OKXWsMessage::AlgoOrders(_)
            | OKXWsMessage::OrderResponse { .. }
            | OKXWsMessage::Account(_)
            | OKXWsMessage::Positions(_)
            | OKXWsMessage::LiquidationWarnings(_)
            | OKXWsMessage::SendFailed { .. } => {
                log::debug!("Ignoring execution message on data client");
            }
            OKXWsMessage::SubscriptionFailed {
                channel,
                inst_id,
                code,
                msg,
            } => {
                log::error!(
                    "OKX rejected {channel:?} subscription for {inst_id:?} \
                     (code={code}, msg={msg}); no data will flow for this subscription"
                );

                if let Some(inst_id) = inst_id
                    && channel.is_book()
                    && let Some(instrument) = instruments_by_symbol.get_cloned(&inst_id)
                {
                    let instrument_id = instrument.id();
                    book_sync.remove(instrument_id);
                }
            }
            OKXWsMessage::Error(e) => {
                if should_retry_error_code(&e.code) {
                    log::warn!("OKX websocket error: {e:?}");
                } else {
                    log::error!("OKX websocket error: {e:?}");
                }
            }
            OKXWsMessage::Reconnected => {
                log::info!("Websocket reconnected");

                if book_channel_scope == BookChannelScope::Public {
                    book_sync.reset_sequences(book_channels, book_channel_scope);
                }

                if !snapshot_timeout.is_zero() {
                    let pending_count = book_sync.seed_pending_snapshots(
                        book_channels,
                        book_channel_scope,
                        snapshot_timeout,
                        Instant::now(),
                    );

                    if pending_count > 0 {
                        spawn_snapshot_health_monitor(
                            book_sync.clone(),
                            tasks,
                            cancel,
                            snapshot_timeout,
                        );
                    }
                }
            }
            OKXWsMessage::Authenticated => {
                log::debug!("Websocket authenticated");
            }
        }
    }

    /// Establishes instrument context and both WebSocket transports.
    ///
    /// Any failure leaves partially started transports for [`Self::teardown_transports`].
    async fn connect_session(&mut self) -> anyhow::Result<()> {
        // Reset leaves the old generation canceled until this async boundary can drain it
        if self.transports_started
            || !self.tasks.is_empty()
            || self
                .ws_public
                .as_ref()
                .is_some_and(OKXWebSocketClient::has_task)
            || self
                .ws_business
                .as_ref()
                .is_some_and(OKXWebSocketClient::has_task)
        {
            self.teardown_transports().await?;
        }

        self.cancellation_token = CancellationToken::new();
        self.transports_started = true;

        let all_instruments = fetch_configured_instruments(&self.http_client, &self.config).await?;

        // Diff before updating the cache so reconnects do not republish
        // unchanged definitions; the writer tasks start after this point,
        // so no instrument lock is needed here
        let changed = changed_definitions(&all_instruments, &self.instruments_by_symbol);

        self.instruments_by_symbol.rcu(|m| {
            for instrument in &all_instruments {
                m.insert(instrument.symbol().inner(), instrument.clone());
            }
        });

        // Cache both websockets before connecting and before publishing, so
        // every cache holds a definition before it is emitted
        let instruments: Vec<_> = self
            .instruments_by_symbol
            .load()
            .values()
            .cloned()
            .collect();

        if let Some(ref ws) = self.ws_public {
            ws.cache_instruments(&instruments);
        }

        if let Some(ref ws) = self.ws_business {
            ws.cache_instruments(&instruments);
        }

        publish_instrument_updates(
            &changed,
            &self.instruments_by_symbol,
            &self.http_client,
            self.ws_public.as_ref(),
            self.ws_business.as_ref(),
            &self.instrument_update_lock,
            &self.data_sender,
        );

        let instrument_types = configured_instrument_types(&self.config);

        if let Some(ref mut ws) = self.ws_public {
            ws.connect()
                .await
                .context("failed to connect OKX public websocket")?;
            ws.wait_until_active(10.0)
                .await
                .context("public websocket did not become active")?;

            let stream = ws.stream();
            let sender = self.data_sender.clone();
            let insts = self.instruments_by_symbol.clone();
            let http = self.http_client.clone();
            let config = self.config.clone();
            let update_lock = self.instrument_update_lock.clone();
            let book_channels = self.book_channels.clone();
            let book_sync = self.book_sync.clone();
            let recovery_ws = ws.clone();
            let business_ws = self.ws_business.clone();
            let idx_map = self.index_ticker_map.clone();
            let greeks_subs = self.option_greeks_subs.clone();
            let tasks = Arc::clone(&self.tasks);
            let cancel = self.cancellation_token.clone();
            let snapshot_timeout = Duration::from_secs(self.config.book_snapshot_timeout_secs);
            let clock = self.clock;

            spawn_task(&self.tasks, &self.cancellation_token, async move {
                let mut quote_cache = QuoteCache::new();
                let mut funding_cache: AHashMap<Ustr, (Ustr, u64)> = AHashMap::new();

                pin_mut!(stream);

                loop {
                    tokio::select! {
                        biased;
                        () = cancel.cancelled() => {
                            log::debug!("Public websocket stream task cancelled");
                            break;
                        }
                        Some(message) = stream.next() => {
                            Self::handle_ws_message(
                                message,
                                &sender,
                                &insts,
                                &http,
                                &config,
                                &update_lock,
                                &book_channels,
                                &book_sync,
                                Some(&recovery_ws),
                                business_ws.as_ref(),
                                &mut quote_cache,
                                &mut funding_cache,
                                &idx_map,
                                &greeks_subs,
                                BookChannelScope::Public,
                                snapshot_timeout,
                                &tasks,
                                &cancel,
                                clock,
                            );
                        }
                    }
                }
            });

            for inst_type in &instrument_types {
                ws.subscribe_instruments(*inst_type)
                    .await
                    .with_context(|| {
                        format!("failed to subscribe to instrument type {inst_type:?}")
                    })?;
            }
        }

        if let Some(ref mut ws) = self.ws_business {
            ws.connect()
                .await
                .context("failed to connect OKX business websocket")?;
            ws.wait_until_active(10.0)
                .await
                .context("business websocket did not become active")?;

            let stream = ws.stream();
            let sender = self.data_sender.clone();
            let insts = self.instruments_by_symbol.clone();
            let http = self.http_client.clone();
            let config = self.config.clone();
            let update_lock = self.instrument_update_lock.clone();
            let book_channels = self.book_channels.clone();
            let book_sync = self.book_sync.clone();
            let business_ws = ws.clone();
            let idx_map = self.index_ticker_map.clone();
            let greeks_subs = self.option_greeks_subs.clone();
            let tasks = Arc::clone(&self.tasks);
            let cancel = self.cancellation_token.clone();
            let snapshot_timeout = Duration::from_secs(self.config.book_snapshot_timeout_secs);
            let clock = self.clock;

            spawn_task(&self.tasks, &self.cancellation_token, async move {
                let mut quote_cache = QuoteCache::new();
                let mut funding_cache: AHashMap<Ustr, (Ustr, u64)> = AHashMap::new();

                pin_mut!(stream);

                loop {
                    tokio::select! {
                        biased;
                        () = cancel.cancelled() => {
                            log::debug!("Business websocket stream task cancelled");
                            break;
                        }
                        Some(message) = stream.next() => {
                            Self::handle_ws_message(
                                message,
                                &sender,
                                &insts,
                                &http,
                                &config,
                                &update_lock,
                                &book_channels,
                                &book_sync,
                                None,
                                Some(&business_ws),
                                &mut quote_cache,
                                &mut funding_cache,
                                &idx_map,
                                &greeks_subs,
                                BookChannelScope::Business,
                                snapshot_timeout,
                                &tasks,
                                &cancel,
                                clock,
                            );
                        }
                    }
                }
            });
        }

        self.spawn_book_health_monitor();
        self.spawn_instrument_refresh();
        Ok(())
    }

    /// Spawns the periodic instrument reconciliation task, disabled when the
    /// configured interval is zero. The handle is tracked in `self.tasks`, so
    /// `teardown_transports` joins it and the cancellation token stops it on
    /// disconnect, failed connect, stop, and dispose.
    fn spawn_instrument_refresh(&self) {
        let minutes = self.config.update_instruments_interval_mins;

        if minutes == 0 {
            log::debug!("Instrument refresh disabled (update_instruments_interval_mins=0)");
            return;
        }

        let interval = Duration::from_secs(minutes.saturating_mul(60));
        let cancel = self.cancellation_token.clone();
        let http_client = self.http_client.clone();
        let config = self.config.clone();
        let instruments = self.instruments_by_symbol.clone();
        let update_lock = self.instrument_update_lock.clone();
        let ws_public = self.ws_public.clone();
        let ws_business = self.ws_business.clone();
        let data_sender = self.data_sender.clone();
        let client_id = self.client_id;

        spawn_task(&self.tasks, &self.cancellation_token, async move {
            loop {
                let sleep = tokio::time::sleep(interval);
                tokio::pin!(sleep);

                tokio::select! {
                    biased;
                    () = cancel.cancelled() => break,
                    () = &mut sleep => {}
                }

                let result = tokio::select! {
                    biased;
                    () = cancel.cancelled() => break,
                    result = reconcile_instruments(
                        &http_client,
                        &config,
                        &instruments,
                        &update_lock,
                        ws_public.as_ref(),
                        ws_business.as_ref(),
                        &data_sender,
                    ) => result,
                };

                match result {
                    Ok(summary) => {
                        log::debug!(
                            "OKX instruments refreshed: client_id={client_id}, fetched={}, changed={}, missing={}",
                            summary.fetched,
                            summary.changed,
                            summary.missing,
                        );
                    }
                    Err(e) => {
                        log::warn!(
                            "Failed to refresh OKX instruments: client_id={client_id}, error={e:?}"
                        );
                    }
                }
            }

            log::debug!("Instrument refresh task cancelled");
        });
    }

    /// Cancels stream tasks, closes both WebSocket transports, and clears
    /// transport-local subscription bookkeeping.
    ///
    /// Safe to call after a partially failed connect and idempotent.
    async fn teardown_transports(&mut self) -> anyhow::Result<()> {
        self.transports_started = false;
        self.cancellation_token.cancel();

        if let Some(ws) = self.ws_public.as_ref() {
            ws.request_close().await;
        }

        if let Some(ws) = self.ws_business.as_ref() {
            ws.request_close().await;
        }

        let task_result = terminate_tasks(&self.tasks, "OKX data client").await;

        let public_result = if let Some(ref mut ws) = self.ws_public {
            ws.close().await.context("failed to close public websocket")
        } else {
            Ok(())
        };

        let business_result = if let Some(ref mut ws) = self.ws_business {
            ws.close()
                .await
                .context("failed to close business websocket")
        } else {
            Ok(())
        };

        self.book_channels.store(AHashMap::new());
        self.book_sync.clear();
        self.option_greeks_subs
            .store(AHashMap::<InstrumentId, AHashSet<OKXGreeksType>>::new());
        self.option_summary_family_subs
            .lock()
            .expect("option_summary_family_subs mutex poisoned")
            .clear();
        self.is_connected.store(false, Ordering::Release);
        task_result?;
        public_result?;
        business_result
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "book recovery needs the complete subscription and task ownership context"
)]
fn handle_book_sequence_outcome(
    outcome: BookSequenceOutcome,
    instrument_id: InstrumentId,
    book_channels: &Arc<AtomicMap<InstrumentId, OKXBookChannel>>,
    book_sync: &BookSyncTracker,
    recovery_ws: Option<&OKXWebSocketClient>,
    snapshot_timeout: Duration,
    tasks: &TaskHandles,
    cancel: &CancellationToken,
) -> bool {
    match outcome {
        BookSequenceOutcome::Accept => true,
        BookSequenceOutcome::Suppress => false,
        BookSequenceOutcome::Recover {
            last_seq_id,
            prev_seq_id,
            seq_id,
        } => {
            log::warn!(
                "Book sequence gap for {instrument_id}: last_seq_id={last_seq_id:?}, \
                 prev_seq_id={prev_seq_id:?}, seq_id={seq_id}; requesting a fresh snapshot"
            );

            let Some(channel) = book_channels.get_cloned(&instrument_id) else {
                log::warn!("Cannot recover book sequence for unsubscribed {instrument_id}");
                return false;
            };
            let Some(ws) = recovery_ws.cloned() else {
                log::error!("No public websocket available to recover book for {instrument_id}");
                return false;
            };
            let channels = Arc::clone(book_channels);
            let recovery_cancel = cancel.clone();

            spawn_task(tasks, cancel, async move {
                if recovery_cancel.is_cancelled()
                    || channels.get_cloned(&instrument_id) != Some(channel)
                {
                    return;
                }

                if let Err(e) = ws.resubscribe_book_channel(instrument_id, channel).await {
                    log::error!("Failed to recover book sequence for {instrument_id}: {e}");
                }
            });

            if !snapshot_timeout.is_zero() {
                spawn_snapshot_health_monitor(book_sync.clone(), tasks, cancel, snapshot_timeout);
            }
            false
        }
    }
}

/// Guards instrument definitions: serializes diff-update-publish sequences
/// between writer tasks, and counts completed update batches so a pass can
/// detect a write that raced its fetch and skip publishing a stale snapshot.
#[derive(Debug, Default)]
struct InstrumentUpdateLock {
    mutex: std::sync::Mutex<()>,
    write_seq: AtomicU64,
}

fn dispatch_parsed_data(
    msg: NautilusWsMessage,
    data_sender: &tokio::sync::mpsc::UnboundedSender<DataEvent>,
    instruments_by_symbol: &Arc<AtomicMap<Ustr, InstrumentAny>>,
) {
    match msg {
        NautilusWsMessage::Data(payloads) => {
            for data in payloads {
                if let Err(e) = data_sender.send(DataEvent::Data(data)) {
                    log::error!("Failed to emit data event: {e}");
                }
            }
        }
        NautilusWsMessage::Deltas(deltas) => {
            let data = Data::Deltas(Box::new(deltas));
            if let Err(e) = data_sender.send(DataEvent::Data(data)) {
                log::error!("Failed to emit data event: {e}");
            }
        }
        NautilusWsMessage::FundingRates(updates) => {
            emit_funding_rates(data_sender, updates);
        }
        NautilusWsMessage::Instrument(instrument, status) => {
            instruments_by_symbol.insert(instrument.symbol().inner(), *instrument);

            if let Some(status) = status
                && let Err(e) = data_sender.send(DataEvent::InstrumentStatus(status))
            {
                log::error!("Failed to emit instrument status event: {e}");
            }
        }
        NautilusWsMessage::InstrumentStatus(status) => {
            if let Err(e) = data_sender.send(DataEvent::InstrumentStatus(status)) {
                log::error!("Failed to emit instrument status event: {e}");
            }
        }
        _ => {}
    }
}

fn emit_funding_rates(
    sender: &tokio::sync::mpsc::UnboundedSender<DataEvent>,
    updates: Vec<FundingRateUpdate>,
) {
    for update in updates {
        if let Err(e) = sender.send(DataEvent::FundingRate(update)) {
            log::error!("Failed to emit funding rate event: {e}");
        }
    }
}

fn emit_instrument_status(
    sender: &tokio::sync::mpsc::UnboundedSender<DataEvent>,
    instrument_id: InstrumentId,
    status_action: MarketStatusAction,
    is_live: bool,
    ts_init: UnixNanos,
) {
    let status = InstrumentStatus::new(
        instrument_id,
        status_action,
        ts_init,
        ts_init,
        None,
        None,
        Some(is_live),
        None,
        None,
    );

    if let Err(e) = sender.send(DataEvent::InstrumentStatus(status)) {
        log::error!("Failed to emit instrument status event: {e}");
    }
}

fn spawn_snapshot_health_monitor(
    book_sync: BookSyncTracker,
    tasks: &TaskHandles,
    cancel: &CancellationToken,
    timeout: Duration,
) {
    let task_cancel = cancel.clone();
    spawn_task(tasks, cancel, async move {
        tokio::select! {
            biased;
            () = task_cancel.cancelled() => {}
            () = tokio::time::sleep(timeout) => {
                handle_book_sync_signals(book_sync.expired_pending_snapshots(Instant::now()));
            }
        }
    });
}

fn handle_book_sync_signals(signals: Vec<BookSyncSignal>) {
    for signal in signals {
        match signal.kind {
            BookSyncSignalKind::Stale { elapsed } => {
                log::warn!(
                    "Book feed stale for {}: no update for {:.3}s",
                    signal.instrument_id,
                    elapsed.as_secs_f64()
                );
            }
            BookSyncSignalKind::SnapshotMissing => {
                log::warn!(
                    "Book snapshot not received for {} after recovery request",
                    signal.instrument_id
                );
            }
        }
    }
}

fn changed_definitions(
    fetched: &[InstrumentAny],
    instruments_by_symbol: &Arc<AtomicMap<Ustr, InstrumentAny>>,
) -> Vec<InstrumentAny> {
    fetched
        .iter()
        .filter(|instrument| {
            instruments_by_symbol
                .get_cloned(&instrument.symbol().inner())
                .is_none_or(|cached| !instrument_definitions_match(&cached, instrument))
        })
        .cloned()
        .collect()
}

/// Updates the data client cache, HTTP client cache, and both WebSocket
/// caches, then bumps the update sequence. Callers serialize diff-update
/// sequences through the instrument update lock.
fn cache_instrument_updates(
    changed: &[InstrumentAny],
    instruments_by_symbol: &Arc<AtomicMap<Ustr, InstrumentAny>>,
    http_client: &OKXHttpClient,
    ws_public: Option<&OKXWebSocketClient>,
    ws_business: Option<&OKXWebSocketClient>,
    instrument_update_lock: &InstrumentUpdateLock,
) {
    if changed.is_empty() {
        return;
    }

    instruments_by_symbol.rcu(|m| {
        for instrument in changed {
            m.insert(instrument.symbol().inner(), instrument.clone());
        }
    });
    http_client.cache_instruments(changed);

    if let Some(ws) = ws_public {
        ws.cache_instruments(changed);
    }

    if let Some(ws) = ws_business {
        ws.cache_instruments(changed);
    }

    instrument_update_lock
        .write_seq
        .fetch_add(1, Ordering::SeqCst);
}

/// Publishes new or changed definitions as [`DataEvent::Instrument`] after
/// updating every cache, so consumers never observe a definition the caches
/// do not yet hold. Callers serialize diff-update-publish sequences through
/// the instrument update lock.
fn publish_instrument_updates(
    changed: &[InstrumentAny],
    instruments_by_symbol: &Arc<AtomicMap<Ustr, InstrumentAny>>,
    http_client: &OKXHttpClient,
    ws_public: Option<&OKXWebSocketClient>,
    ws_business: Option<&OKXWebSocketClient>,
    instrument_update_lock: &InstrumentUpdateLock,
    data_sender: &tokio::sync::mpsc::UnboundedSender<DataEvent>,
) {
    cache_instrument_updates(
        changed,
        instruments_by_symbol,
        http_client,
        ws_public,
        ws_business,
        instrument_update_lock,
    );

    for instrument in changed {
        if let Err(e) = data_sender.send(DataEvent::Instrument(instrument.clone())) {
            log::error!("Failed to emit instrument event: {e}");
        }
    }
}

fn contract_filter_with_config(config: &OKXDataClientConfig, instrument: &InstrumentAny) -> bool {
    contract_filter_with_config_types(config.contract_types.as_ref(), instrument)
}

/// Returns `true` when a venue definition belongs to the configured scope:
/// the contract type filter, plus configured families for derivative types.
/// The instruments channel pushes the whole type, so updates outside the
/// configured families must not enter the cache or publish downstream.
fn definition_in_scope(
    config: &OKXDataClientConfig,
    okx_inst: &OKXInstrument,
    instrument: &InstrumentAny,
) -> bool {
    if !contract_filter_with_config(config, instrument) {
        return false;
    }

    let Some(families) = &config.instrument_families else {
        return true;
    };

    if families.is_empty()
        || !matches!(
            okx_inst.inst_type,
            OKXInstrumentType::Option
                | OKXInstrumentType::Futures
                | OKXInstrumentType::Swap
                | OKXInstrumentType::Events
        )
    {
        return true;
    }

    let family_key = if matches!(okx_inst.inst_type, OKXInstrumentType::Events) {
        // Events carry their family as the series ID, matching the REST path
        // which passes configured families as series_id
        okx_inst.series_id.map(|series| series.as_str())
    } else {
        Some(okx_inst.inst_family.as_str())
    };

    let Some(family_key) = family_key else {
        return false;
    };

    families.iter().any(|family| family.as_str() == family_key)
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

fn configured_instrument_types(config: &OKXDataClientConfig) -> Vec<OKXInstrumentType> {
    if config.instrument_types.is_empty() {
        vec![OKXInstrumentType::Spot]
    } else {
        // A type configured twice must not fetch or publish its instruments twice
        let mut seen = AHashSet::new();
        config
            .instrument_types
            .iter()
            .filter(|inst_type| seen.insert(**inst_type))
            .copied()
            .collect()
    }
}

/// Fetches every instrument covered by the configuration, applying the
/// contract type filter. Fails on the first type or family error; a spread
/// endpoint failure is logged and skipped because spread instruments are
/// supplemental. Does not touch any cache: a caller may discard a stale
/// snapshot, so cache updates happen only in guarded publish sections.
async fn fetch_configured_instruments(
    http_client: &OKXHttpClient,
    config: &OKXDataClientConfig,
) -> anyhow::Result<Vec<InstrumentAny>> {
    let instrument_types = configured_instrument_types(config);
    let mut all_instruments = Vec::new();

    for inst_type in &instrument_types {
        let Some(mut families) =
            resolve_instrument_families(&config.instrument_families, *inst_type)
        else {
            continue;
        };

        // A family configured twice must not fetch or publish its instruments twice
        let mut seen = AHashSet::new();
        families.retain(|family| seen.insert(family.clone()));

        if families.is_empty() {
            let (mut fetched, _inst_id_codes) = http_client
                .request_instruments(*inst_type, None)
                .await
                .with_context(|| format!("failed to request OKX instruments for {inst_type:?}"))?;

            fetched.retain(|instrument| contract_filter_with_config(config, instrument));
            all_instruments.extend(fetched);
        } else {
            for family in &families {
                let (mut fetched, _inst_id_codes) = http_client
                    .request_instruments(*inst_type, Some(family.clone()))
                    .await
                    .with_context(|| {
                        format!(
                            "failed to request OKX instruments for {inst_type:?} family {family}"
                        )
                    })?;

                fetched.retain(|instrument| contract_filter_with_config(config, instrument));
                all_instruments.extend(fetched);
            }
        }
    }

    if config.load_spreads {
        match http_client
            .request_spread_instruments(GetSpreadsParams {
                state: Some("live".to_string()),
                ..Default::default()
            })
            .await
        {
            Ok(mut fetched) => {
                fetched.retain(|instrument| contract_filter_with_config(config, instrument));
                all_instruments.extend(fetched);
            }
            Err(e) => {
                log::error!("Failed to fetch OKX spread instruments: {e:?}");
            }
        }
    }

    Ok(all_instruments)
}

/// Returns `true` when two instruments carry the same tradable definition,
/// ignoring event timestamps.
///
/// Comparison runs on the serialized form so every venue field, including
/// the free-form `info` metadata, participates without listing each field.
fn instrument_definitions_match(a: &InstrumentAny, b: &InstrumentAny) -> bool {
    fn normalized(instrument: &InstrumentAny) -> Option<serde_json::Value> {
        let mut value = serde_json::to_value(instrument).ok()?;

        if let Some(definition) = value
            .as_object_mut()
            .and_then(|obj| obj.values_mut().next())
            .and_then(serde_json::Value::as_object_mut)
        {
            definition.remove("ts_event");
            definition.remove("ts_init");
        }

        Some(value)
    }

    // A serialization failure compares as changed so updates are never suppressed
    match (normalized(a), normalized(b)) {
        (Some(a), Some(b)) => a == b,
        _ => false,
    }
}

/// Summary of a single instrument reconciliation pass.
#[derive(Debug)]
struct InstrumentReconciliation {
    /// Instruments returned by the REST API after filtering.
    fetched: usize,
    /// New or materially changed definitions published downstream.
    changed: usize,
    /// Cached instruments absent from the REST response, retained in place.
    missing: usize,
}

/// Reconciles the instrument cache against the REST API.
///
/// Fetches every configured instrument type and family, plus spread instruments
/// when `load_spreads` is set, then updates the data client, HTTP client, and
/// WebSocket caches with new or materially changed definitions before
/// publishing them as [`DataEvent::Instrument`]. Unchanged definitions are
/// not republished. Cached instruments missing from the response are retained
/// because they may still back open subscriptions; the instruments WebSocket
/// channel communicates state changes such as suspension or delisting.
async fn reconcile_instruments(
    http_client: &OKXHttpClient,
    config: &OKXDataClientConfig,
    instruments_by_symbol: &Arc<AtomicMap<Ustr, InstrumentAny>>,
    instrument_update_lock: &InstrumentUpdateLock,
    ws_public: Option<&OKXWebSocketClient>,
    ws_business: Option<&OKXWebSocketClient>,
    data_sender: &tokio::sync::mpsc::UnboundedSender<DataEvent>,
) -> anyhow::Result<InstrumentReconciliation> {
    let seq_before = instrument_update_lock.write_seq.load(Ordering::SeqCst);
    let fetched = fetch_configured_instruments(http_client, config).await?;

    // Hold the instrument lock from the diff through publication so a concurrent
    // instruments channel update cannot interleave with this pass
    let _update_guard = instrument_update_lock
        .mutex
        .lock()
        .expect("instrument update lock poisoned");

    // A write during the fetch means the snapshot is stale relative to the
    // instrument cache; skip publishing it and let the next pass reconcile fully
    let changed = if instrument_update_lock.write_seq.load(Ordering::SeqCst) == seq_before {
        changed_definitions(&fetched, instruments_by_symbol)
    } else {
        log::debug!("OKX instrument cache changed during refresh fetch, skipping publish");
        Vec::new()
    };

    if !changed.is_empty() {
        publish_instrument_updates(
            &changed,
            instruments_by_symbol,
            http_client,
            ws_public,
            ws_business,
            instrument_update_lock,
            data_sender,
        );
    }

    let fetched_symbols: AHashSet<Ustr> = fetched
        .iter()
        .map(|instrument| instrument.symbol().inner())
        .collect();
    let missing = instruments_by_symbol
        .load()
        .keys()
        .filter(|symbol| !fetched_symbols.contains(*symbol))
        .count();

    if missing > 0 {
        log::debug!(
            "{missing} cached instruments absent from OKX REST response, retaining cached definitions"
        );
    }

    Ok(InstrumentReconciliation {
        fetched: fetched.len(),
        changed: changed.len(),
        missing,
    })
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
        log::info!(
            "Started: client_id={}, vip_level={:?}, instrument_types={:?}, environment={}, proxy_url={:?}",
            self.client_id,
            self.vip_level(),
            self.config.instrument_types,
            self.config.environment,
            self.config.proxy_url,
        );
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        log::info!("Stopping {id}", id = self.client_id);
        self.terminate_generation(false);
        Ok(())
    }

    fn reset(&mut self) -> anyhow::Result<()> {
        log::debug!("Resetting {id}", id = self.client_id);
        self.terminate_generation(true);
        self.book_channels.store(AHashMap::new());
        self.book_sync.clear();
        self.option_greeks_subs
            .store(AHashMap::<InstrumentId, AHashSet<OKXGreeksType>>::new());
        self.option_summary_family_subs
            .lock()
            .expect("option_summary_family_subs mutex poisoned")
            .clear();
        Ok(())
    }

    fn dispose(&mut self) -> anyhow::Result<()> {
        log::debug!("Disposing {id}", id = self.client_id);
        self.terminate_generation(true);
        Ok(())
    }

    async fn connect(&mut self) -> anyhow::Result<()> {
        if self.is_connected() {
            return Ok(());
        }

        if let Err(e) = self.connect_session().await {
            if let Err(teardown_error) = self.teardown_transports().await {
                log::warn!("Error tearing down partial connection: {teardown_error:?}");
            }
            return Err(e);
        }

        self.is_connected.store(true, Ordering::Release);
        log::info!("Connected: client_id={}", self.client_id);
        Ok(())
    }

    async fn disconnect(&mut self) -> anyhow::Result<()> {
        if self.is_disconnected() && !self.transports_started && self.tasks.is_empty() {
            return Ok(());
        }

        if !self.is_disconnected() {
            if let Some(ref ws) = self.ws_public
                && let Err(e) = ws.unsubscribe_all().await
            {
                log::warn!("Failed to unsubscribe all from public websocket: {e:?}");
            }

            if let Some(ref ws) = self.ws_business
                && let Err(e) = ws.unsubscribe_all().await
            {
                log::warn!("Failed to unsubscribe all from business websocket: {e:?}");
            }

            // Allow time for unsubscribe confirmations
            tokio::time::sleep(Duration::from_millis(500)).await;
        }

        self.teardown_transports().await?;
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

    fn subscribe_instrument(&mut self, cmd: SubscribeInstrument) -> anyhow::Result<()> {
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

    fn subscribe_book_deltas(&mut self, cmd: SubscribeBookDeltas) -> anyhow::Result<()> {
        if cmd.book_type != BookType::L2_MBP {
            anyhow::bail!("OKX only supports L2_MBP order book deltas");
        }

        if is_okx_spread_symbol(cmd.instrument_id.symbol.as_str()) {
            // Spreads have no incremental book channel; sprd-books5 pushes a full
            // 5-level snapshot, emitted as F_SNAPSHOT deltas to feed the book.
            let instrument_id = cmd.instrument_id;
            let ws = self.business_ws()?.clone();
            let book_channels = Arc::clone(&self.book_channels);
            let book_sync = self.book_sync.clone();
            self.spawn_ws(
                async move {
                    ws.subscribe_spread_book(instrument_id)
                        .await
                        .context("spread book subscription")?;
                    book_channels.insert(instrument_id, OKXBookChannel::SprdBooks5);
                    book_sync.record_subscription(instrument_id, Instant::now());
                    Ok(())
                },
                "spread book subscription",
            );
            return Ok(());
        }

        let raw_depth = cmd.depth.map_or(0, |d| d.get());
        let depth = resolve_book_depth(raw_depth);
        if depth != raw_depth {
            log::debug!("Clamped book depth {raw_depth} to {depth} (OKX supports 50 or 400)");
        }

        let rpi = cmd
            .params
            .as_ref()
            .and_then(|params| params.get_bool("rpi"))
            .unwrap_or(false);
        let vip = self.vip_level().unwrap_or(OKXVipLevel::Vip0);
        let channel = if rpi {
            OKXBookChannel::BooksRpi
        } else {
            let channel = select_book_channel(depth, vip);
            if depth == 50 && channel == OKXBookChannel::Book {
                log::debug!(
                    "VIP level {vip} insufficient for 50-depth channel, falling back to default"
                );
            }
            channel
        };

        let instrument_id = cmd.instrument_id;
        let ws = self.public_ws()?.clone();
        let book_channels = Arc::clone(&self.book_channels);
        let book_sync = self.book_sync.clone();

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
                    OKXBookChannel::BooksRpi => ws
                        .subscribe_book_rpi(instrument_id)
                        .await
                        .context("books-rpi subscription")?,
                    OKXBookChannel::SprdBooks5 => unreachable!(),
                }
                book_channels.insert(instrument_id, channel);
                book_sync.record_subscription(instrument_id, Instant::now());
                Ok(())
            },
            "order book delta subscription",
        );

        Ok(())
    }

    fn subscribe_quotes(&mut self, cmd: SubscribeQuotes) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;

        if is_okx_spread_symbol(instrument_id.symbol.as_str()) {
            let ws = self.business_ws()?.clone();
            self.spawn_ws(
                async move {
                    ws.subscribe_spread_quotes(instrument_id)
                        .await
                        .context("spread quotes subscription")
                },
                "spread quote subscription",
            );
            return Ok(());
        }

        let ws = self.public_ws()?.clone();
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

    fn subscribe_trades(&mut self, cmd: SubscribeTrades) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;

        if is_okx_spread_symbol(instrument_id.symbol.as_str()) {
            let ws = self.business_ws()?.clone();
            self.spawn_ws(
                async move {
                    ws.subscribe_spread_trades(instrument_id)
                        .await
                        .context("spread trades subscription")
                },
                "spread trade subscription",
            );
            return Ok(());
        }

        let ws = self.public_ws()?.clone();
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

    fn subscribe_mark_prices(&mut self, cmd: SubscribeMarkPrices) -> anyhow::Result<()> {
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

    fn subscribe_index_prices(&mut self, cmd: SubscribeIndexPrices) -> anyhow::Result<()> {
        let ws = self.public_ws()?.clone();
        let instrument_id = cmd.instrument_id;
        let symbol = instrument_id.symbol.inner();

        let (base, quote) = parse_base_quote_from_symbol(symbol.as_str())?;
        let base_pair = Ustr::from(&format!("{base}-{quote}"));
        self.index_ticker_map.rcu(|m| {
            m.entry(base_pair).or_default().insert(symbol);
        });

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

    fn subscribe_bars(&mut self, cmd: SubscribeBars) -> anyhow::Result<()> {
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

    fn subscribe_funding_rates(&mut self, cmd: SubscribeFundingRates) -> anyhow::Result<()> {
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

    fn subscribe_option_greeks(&mut self, cmd: SubscribeOptionGreeks) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;
        let conventions = parse_greeks_conventions_from_params(&cmd.params);
        self.option_greeks_subs.insert(instrument_id, conventions);

        let family = extract_inst_family(instrument_id.symbol.inner().as_str())?;
        let is_first = {
            let mut family_subs = self
                .option_summary_family_subs
                .lock()
                .expect("option_summary_family_subs mutex poisoned");
            let count = family_subs.entry(family).or_default();
            *count += 1;
            *count == 1
        };

        if is_first {
            let ws = self.public_ws()?.clone();
            let family_subs = self.option_summary_family_subs.clone();
            self.spawn_ws(
                async move {
                    let result = ws
                        .subscribe_option_summary(family)
                        .await
                        .context("opt-summary subscription");

                    if result.is_err() {
                        // Roll back the refcount so a retry can re-arm the subscribe;
                        // otherwise the family wedges and Greeks stay dark.
                        let mut subs = family_subs
                            .lock()
                            .expect("option_summary_family_subs mutex poisoned");

                        if let Some(count) = subs.get_mut(&family) {
                            *count = count.saturating_sub(1);
                            if *count == 0 {
                                subs.remove(&family);
                            }
                        }
                    }
                    result
                },
                "option greeks subscription",
            );
        }
        Ok(())
    }

    fn subscribe_instrument_status(
        &mut self,
        cmd: SubscribeInstrumentStatus,
    ) -> anyhow::Result<()> {
        let ws = self.public_ws()?.clone();
        let instrument_id = cmd.instrument_id;

        self.spawn_ws(
            async move {
                ws.subscribe_instrument(instrument_id)
                    .await
                    .context("instrument status subscription")
            },
            "instrument status subscription",
        );
        Ok(())
    }

    fn unsubscribe_instrument(&mut self, cmd: &UnsubscribeInstrument) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;
        let ws = self.public_ws()?.clone();

        self.spawn_ws(
            async move {
                ws.unsubscribe_instrument(instrument_id)
                    .await
                    .context("instrument unsubscribe")?;
                Ok(())
            },
            "unsubscribe_instrument",
        );
        Ok(())
    }

    fn unsubscribe_book_deltas(&mut self, cmd: &UnsubscribeBookDeltas) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;

        if is_okx_spread_symbol(instrument_id.symbol.as_str()) {
            let ws = self.business_ws()?.clone();
            self.book_channels.remove(&instrument_id);
            self.book_sync.remove(instrument_id);
            self.spawn_ws(
                async move {
                    ws.unsubscribe_spread_book(instrument_id)
                        .await
                        .context("spread book unsubscribe")
                },
                "spread book unsubscribe",
            );
            return Ok(());
        }

        let ws = self.public_ws()?.clone();
        let channel = self.book_channels.get_cloned(&instrument_id);
        self.book_channels.remove(&instrument_id);
        self.book_sync.remove(instrument_id);

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
                    Some(OKXBookChannel::BooksRpi) => ws
                        .unsubscribe_book_rpi(instrument_id)
                        .await
                        .context("books-rpi unsubscribe")?,
                    Some(OKXBookChannel::SprdBooks5) => ws
                        .unsubscribe_book(instrument_id)
                        .await
                        .context("book unsubscribe")?,
                    None => {
                        log::warn!(
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

    fn unsubscribe_quotes(&mut self, cmd: &UnsubscribeQuotes) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;

        if is_okx_spread_symbol(instrument_id.symbol.as_str()) {
            let ws = self.business_ws()?.clone();
            self.spawn_ws(
                async move {
                    ws.unsubscribe_spread_quotes(instrument_id)
                        .await
                        .context("spread quotes unsubscribe")
                },
                "spread quote unsubscribe",
            );
            return Ok(());
        }

        let ws = self.public_ws()?.clone();
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
        let instrument_id = cmd.instrument_id;

        if is_okx_spread_symbol(instrument_id.symbol.as_str()) {
            let ws = self.business_ws()?.clone();
            self.spawn_ws(
                async move {
                    ws.unsubscribe_spread_trades(instrument_id)
                        .await
                        .context("spread trades unsubscribe")
                },
                "spread trade unsubscribe",
            );
            return Ok(());
        }

        let ws = self.public_ws()?.clone();
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
        let symbol = instrument_id.symbol.inner();

        // The OKX index-tickers channel is keyed by base pair, so multiple
        // instruments on the same pair share one subscription. Per-base-pair
        // refcounting lives on the WS client, so we always forward the
        // unsubscribe and let the WS layer fire the venue request only when
        // it knows the last subscriber dropped. Local routing in
        // `index_ticker_map` is still maintained for downstream emit fan-out.
        if let Ok((base, quote)) = parse_base_quote_from_symbol(symbol.as_str()) {
            let base_pair = Ustr::from(&format!("{base}-{quote}"));
            self.index_ticker_map.rcu(|m| {
                if let Some(set) = m.get_mut(&base_pair) {
                    set.remove(&symbol);
                    if set.is_empty() {
                        m.remove(&base_pair);
                    }
                }
            });
        }

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

    fn unsubscribe_option_greeks(&mut self, cmd: &UnsubscribeOptionGreeks) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;
        self.option_greeks_subs.remove(&instrument_id);

        let family = extract_inst_family(instrument_id.symbol.inner().as_str())?;
        let should_unsubscribe = {
            let mut family_subs = self
                .option_summary_family_subs
                .lock()
                .expect("option_summary_family_subs mutex poisoned");

            if let Some(count) = family_subs.get_mut(&family) {
                *count = count.saturating_sub(1);
                if *count == 0 {
                    family_subs.remove(&family);
                    true
                } else {
                    false
                }
            } else {
                false
            }
        };

        if should_unsubscribe {
            let ws = self.public_ws()?.clone();
            self.spawn_ws(
                async move {
                    ws.unsubscribe_option_summary(family)
                        .await
                        .context("opt-summary unsubscription")
                },
                "option greeks unsubscription",
            );
        }
        Ok(())
    }

    fn unsubscribe_instrument_status(
        &mut self,
        cmd: &UnsubscribeInstrumentStatus,
    ) -> anyhow::Result<()> {
        let ws = self.public_ws()?.clone();
        let instrument_id = cmd.instrument_id;

        self.spawn_ws(
            async move {
                ws.unsubscribe_instrument(instrument_id)
                    .await
                    .context("instrument status unsubscription")
            },
            "instrument status unsubscription",
        );
        Ok(())
    }

    fn request_instruments(&self, request: RequestInstruments) -> anyhow::Result<()> {
        let http = self.http_client.clone();
        let sender = self.data_sender.clone();
        let instruments_cache = self.instruments_by_symbol.clone();
        let update_lock = self.instrument_update_lock.clone();
        let ws_public = self.ws_public.clone();
        let ws_business = self.ws_business.clone();
        let request_id = request.request_id;
        let client_id = request.client_id.unwrap_or(self.client_id);
        let venue = self.venue();
        let start = request.start;
        let end = request.end;
        let params = request.params;
        let clock = self.clock;
        let start_nanos = datetime_to_unix_nanos(start);
        let end_nanos = datetime_to_unix_nanos(end);
        let instrument_types = configured_instrument_types(&self.config);
        let contract_types = self.config.contract_types.clone();
        let instrument_families = self.config.instrument_families.clone();
        let load_spreads = self.config.load_spreads;

        spawn_task(&self.tasks, &self.cancellation_token, async move {
            let seq_before = update_lock.write_seq.load(Ordering::SeqCst);
            let mut all_instruments = Vec::new();

            for inst_type in instrument_types {
                let Some(families) = resolve_instrument_families(&instrument_families, inst_type)
                else {
                    continue;
                };

                if families.is_empty() {
                    match http.request_instruments(inst_type, None).await {
                        Ok((instruments, _inst_id_codes)) => {
                            for instrument in instruments {
                                if !contract_filter_with_config_types(
                                    contract_types.as_ref(),
                                    &instrument,
                                ) {
                                    continue;
                                }

                                all_instruments.push(instrument);
                            }
                        }
                        Err(e) => {
                            log::error!("Failed to fetch instruments for {inst_type:?}: {e:?}");
                        }
                    }
                } else {
                    for family in families {
                        match http
                            .request_instruments(inst_type, Some(family.clone()))
                            .await
                        {
                            Ok((instruments, _inst_id_codes)) => {
                                for instrument in instruments {
                                    if !contract_filter_with_config_types(
                                        contract_types.as_ref(),
                                        &instrument,
                                    ) {
                                        continue;
                                    }

                                    all_instruments.push(instrument);
                                }
                            }
                            Err(e) => {
                                log::error!(
                                    "Failed to fetch instruments for {inst_type:?} family {family}: {e:?}"
                                );
                            }
                        }
                    }
                }
            }

            if load_spreads {
                match http
                    .request_spread_instruments(GetSpreadsParams {
                        state: Some("live".to_string()),
                        ..Default::default()
                    })
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

                            all_instruments.push(instrument);
                        }
                    }
                    Err(e) => {
                        log::error!("Failed to fetch OKX spread instruments: {e:?}");
                    }
                }
            }

            {
                let _update_guard = update_lock
                    .mutex
                    .lock()
                    .expect("instrument update lock poisoned");

                if update_lock.write_seq.load(Ordering::SeqCst) == seq_before {
                    cache_instrument_updates(
                        &all_instruments,
                        &instruments_cache,
                        &http,
                        ws_public.as_ref(),
                        ws_business.as_ref(),
                        &update_lock,
                    );
                } else {
                    log::debug!(
                        "OKX instrument cache changed during request fetch, skipping cache update"
                    );
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
        let instruments = self.instruments_by_symbol.clone();
        let update_lock = self.instrument_update_lock.clone();
        let ws_public = self.ws_public.clone();
        let ws_business = self.ws_business.clone();
        let instrument_id = request.instrument_id;
        let request_id = request.request_id;
        let client_id = request.client_id.unwrap_or(self.client_id);
        let start = request.start;
        let end = request.end;
        let params = request.params;
        let clock = self.clock;
        let start_nanos = datetime_to_unix_nanos(start);
        let end_nanos = datetime_to_unix_nanos(end);
        let instrument_types = configured_instrument_types(&self.config);
        let contract_types = self.config.contract_types.clone();
        let load_spreads = self.config.load_spreads;

        spawn_task(&self.tasks, &self.cancellation_token, async move {
            let seq_before = update_lock.write_seq.load(Ordering::SeqCst);

            match http
                .request_instrument(instrument_id)
                .await
                .context("fetch instrument from API")
            {
                Ok(instrument) => {
                    let inst_id = instrument.id();
                    let symbol = inst_id.symbol.as_str();
                    if is_okx_spread_symbol(symbol) {
                        if !load_spreads {
                            log::error!(
                                "Instrument {instrument_id} is a spread but load_spreads is false"
                            );
                            return;
                        }
                    } else {
                        let inst_type = okx_instrument_type_from_symbol(symbol);
                        if !instrument_types.contains(&inst_type) {
                            log::error!(
                                "Instrument {instrument_id} type {inst_type:?} not in configured types {instrument_types:?}"
                            );
                            return;
                        }
                    }

                    if !contract_filter_with_config_types(contract_types.as_ref(), &instrument) {
                        log::error!(
                            "Instrument {instrument_id} filtered out by contract_types config"
                        );
                        return;
                    }

                    {
                        let _update_guard = update_lock
                            .mutex
                            .lock()
                            .expect("instrument update lock poisoned");

                        if update_lock.write_seq.load(Ordering::SeqCst) == seq_before {
                            cache_instrument_updates(
                                std::slice::from_ref(&instrument),
                                &instruments,
                                &http,
                                ws_public.as_ref(),
                                ws_business.as_ref(),
                                &update_lock,
                            );
                        } else {
                            log::debug!(
                                "OKX instrument cache changed during request fetch, skipping cache update"
                            );
                        }
                    }

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
                Err(e) if e.downcast_ref::<OKXInstrumentDefinitionError>().is_some() => {
                    log::warn!("Instrument request skipped: {e:?}");
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
        let rpi = params
            .as_ref()
            .and_then(|params| params.get_bool("rpi"))
            .unwrap_or(false);
        let clock = self.clock;

        spawn_task(&self.tasks, &self.cancellation_token, async move {
            let result = if rpi {
                http.request_rpi_book_snapshot(instrument_id, depth).await
            } else {
                http.request_book_snapshot(instrument_id, depth).await
            };

            match result.context("failed to request book snapshot from OKX") {
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
                Err(e) => log::error!("Book snapshot request failed: {e:?}"),
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

        spawn_task(&self.tasks, &self.cancellation_token, async move {
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

        spawn_task(&self.tasks, &self.cancellation_token, async move {
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

        spawn_task(&self.tasks, &self.cancellation_token, async move {
            match http
                .request_funding_rates(instrument_id, start, end, limit)
                .await
                .context("failed to request funding rates from OKX")
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
                Err(e) => log::error!("Funding rates request failed: {e:?}"),
            }
        });

        Ok(())
    }

    fn request_forward_prices(&self, request: RequestForwardPrices) -> anyhow::Result<()> {
        let http = self.http_client.clone();
        let sender = self.data_sender.clone();
        let underlying = request.underlying.to_string();
        let instrument_id = request.instrument_id;
        let request_id = request.request_id;
        let client_id = request.client_id.unwrap_or(self.client_id);
        let params = request.params;
        let clock = self.clock;
        let venue = *OKX_VENUE;

        spawn_task(&self.tasks, &self.cancellation_token, async move {
            match http
                .request_forward_prices(&underlying, instrument_id)
                .await
                .context("failed to request forward prices from OKX")
            {
                Ok(forward_prices) => {
                    let response = DataResponse::ForwardPrices(ForwardPricesResponse::new(
                        request_id,
                        client_id,
                        venue,
                        forward_prices,
                        clock.get_time_ns(),
                        params,
                    ));

                    if let Err(e) = sender.send(DataEvent::Response(response)) {
                        log::error!("Failed to send forward prices response: {e}");
                    }
                }
                Err(e) => {
                    log::error!("Forward prices request failed for {underlying}: {e:?}");
                    let response = DataResponse::ForwardPrices(ForwardPricesResponse::new(
                        request_id,
                        client_id,
                        venue,
                        Vec::new(),
                        clock.get_time_ns(),
                        params,
                    ));

                    if let Err(e) = sender.send(DataEvent::Response(response)) {
                        log::error!("Failed to send forward prices response: {e}");
                    }
                }
            }
        });

        Ok(())
    }
}

/// Resolves the set of [`OKXGreeksType`] conventions for an option greeks subscription.
///
/// Reads the `greeks_convention` key from `params`, accepting either a single
/// [`GreeksConvention`] string (e.g. `"BLACK_SCHOLES"` or `"PRICE_ADJUSTED"`) or a
/// JSON array of such strings. Unrecognized entries log a warning and are skipped.
/// Returns the default set `{Bs, Pa}` when the key is absent, unparsable, or
/// yields no valid entries so every subscription defaults to both conventions.
pub(crate) fn parse_greeks_conventions_from_params(
    params: &Option<Params>,
) -> AHashSet<OKXGreeksType> {
    let default_set: AHashSet<OKXGreeksType> =
        [OKXGreeksType::Bs, OKXGreeksType::Pa].into_iter().collect();

    let Some(value) = params.as_ref().and_then(|p| p.get("greeks_convention")) else {
        return default_set;
    };

    let mut out = AHashSet::new();
    match value {
        serde_json::Value::String(s) => push_convention_str(&mut out, s),
        serde_json::Value::Array(items) => {
            for item in items {
                if let Some(s) = item.as_str() {
                    push_convention_str(&mut out, s);
                } else {
                    log::warn!("Ignoring non-string greeks_convention entry {item:?}");
                }
            }
        }
        other => {
            log::warn!(
                "Unsupported greeks_convention value {other:?}, defaulting to both conventions"
            );
        }
    }

    if out.is_empty() { default_set } else { out }
}

fn push_convention_str(out: &mut AHashSet<OKXGreeksType>, raw: &str) {
    match raw.parse::<GreeksConvention>() {
        Ok(convention) => {
            out.insert(convention.into());
        }
        Err(_) => log::warn!("Unrecognized greeks_convention {raw:?}, skipping"),
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, net::SocketAddr, sync::Arc};

    use axum::{Router, extract::Query, response::Json, routing::get};
    use nautilus_common::{live::runner::replace_data_event_sender, testing::wait_until_async};
    use nautilus_core::UUID4;
    use nautilus_model::{
        identifiers::Symbol,
        instruments::stubs::currency_pair_btcusdt,
        types::{Price, Quantity},
    };
    use nautilus_network::websocket::TransportBackend;
    use rstest::rstest;
    use serde_json::{Value, json};

    use super::*;
    use crate::{
        common::{
            consts::OKX_CLIENT_ID, enums::OKXEnvironment, models::OKXInstrument,
            testing::load_test_json,
        },
        websocket::{enums::OKXWsChannel, messages::OKXWsFrame},
    };

    struct DropSignal(Option<tokio::sync::oneshot::Sender<()>>);

    impl Drop for DropSignal {
        fn drop(&mut self) {
            if let Some(sender) = self.0.take() {
                let _ = sender.send(());
            }
        }
    }

    #[derive(Clone, Copy)]
    enum DataTaskBoundary {
        Reset,
        Dispose,
        RepeatedStop,
    }

    fn both() -> AHashSet<OKXGreeksType> {
        [OKXGreeksType::Bs, OKXGreeksType::Pa].into_iter().collect()
    }

    fn only(greeks_type: OKXGreeksType) -> AHashSet<OKXGreeksType> {
        [greeks_type].into_iter().collect()
    }

    #[rstest]
    fn dispatch_parsed_data_emits_instrument_status() {
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        let instruments_by_symbol = Arc::new(AtomicMap::new());
        let status = InstrumentStatus::new(
            InstrumentId::from("USDG-SGD.OKX"),
            MarketStatusAction::Trading,
            UnixNanos::from(1u64),
            UnixNanos::from(2u64),
            None,
            None,
            Some(true),
            None,
            None,
        );

        dispatch_parsed_data(
            NautilusWsMessage::InstrumentStatus(status),
            &sender,
            &instruments_by_symbol,
        );

        match receiver.try_recv().expect("instrument status event") {
            DataEvent::InstrumentStatus(received) => assert_eq!(received, status),
            other => panic!("Expected DataEvent::InstrumentStatus, was {other:?}"),
        }
        assert!(instruments_by_symbol.load().is_empty());
    }

    #[rstest]
    fn rejected_book_subscription_clears_sync_and_preserves_reconnect_intent() {
        let instrument_id = InstrumentId::from("OMI-USD.OKX");
        let mut pair = currency_pair_btcusdt();
        pair.id = instrument_id;
        pair.raw_symbol = Symbol::from("OMI-USD");
        let instrument = InstrumentAny::CurrencyPair(pair);

        let instruments_by_symbol = Arc::new(AtomicMap::new());
        instruments_by_symbol.insert(Ustr::from("OMI-USD"), instrument);
        let book_channels = Arc::new(AtomicMap::new());
        book_channels.insert(instrument_id, OKXBookChannel::Book);
        let book_sync = BookSyncTracker::default();
        book_sync.record_subscription(instrument_id, Instant::now());
        let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel();
        let http = offline_http_client();
        let update_lock = InstrumentUpdateLock::default();
        let mut quote_cache = QuoteCache::new();
        let mut funding_cache: AHashMap<Ustr, (Ustr, u64)> = AHashMap::new();
        let index_ticker_map = Arc::new(AtomicMap::new());
        let option_greeks_subs = Arc::new(AtomicMap::new());
        let tasks = TaskHandles::default();
        let cancel = CancellationToken::new();

        OKXDataClient::handle_ws_message(
            OKXWsMessage::SubscriptionFailed {
                channel: OKXWsChannel::Books,
                inst_id: Some(Ustr::from("OMI-USD")),
                code: "60018".to_string(),
                msg: "Channel does not exist".to_string(),
            },
            &sender,
            &instruments_by_symbol,
            &http,
            &OKXDataClientConfig::default(),
            &update_lock,
            &book_channels,
            &book_sync,
            None,
            None,
            &mut quote_cache,
            &mut funding_cache,
            &index_ticker_map,
            &option_greeks_subs,
            BookChannelScope::Public,
            Duration::ZERO,
            &tasks,
            &cancel,
            get_atomic_clock_realtime(),
        );

        assert_eq!(
            book_channels.load().get(&instrument_id),
            Some(&OKXBookChannel::Book),
            "rejected subscription must preserve the channel selected for reconnect"
        );
        assert!(
            book_sync
                .stale_books(Duration::ZERO, Instant::now())
                .is_empty(),
            "rejected subscription must remove book synchronization state"
        );
    }

    #[rstest]
    fn rpi_sequence_gap_suppresses_updates_until_fresh_snapshot() {
        let instrument_id = InstrumentId::from("OMI-USD.OKX");
        let mut pair = currency_pair_btcusdt();
        pair.id = instrument_id;
        pair.raw_symbol = Symbol::from("OMI-USD");
        pair.price_precision = 7;
        pair.size_precision = 3;
        pair.price_increment = Price::from("0.0000001");
        pair.size_increment = Quantity::from("0.001");
        let instrument = InstrumentAny::CurrencyPair(pair);

        let instruments_by_symbol = Arc::new(AtomicMap::new());
        instruments_by_symbol.insert(Ustr::from("OMI-USD"), instrument);
        let book_channels = Arc::new(AtomicMap::new());
        book_channels.insert(instrument_id, OKXBookChannel::BooksRpi);
        let book_sync = BookSyncTracker::default();
        book_sync.record_subscription(instrument_id, Instant::now());
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        let http = offline_http_client();
        let update_lock = InstrumentUpdateLock::default();
        let index_ticker_map = Arc::new(AtomicMap::new());
        let option_greeks_subs = Arc::new(AtomicMap::new());
        let tasks = TaskHandles::default();
        let cancel = CancellationToken::new();
        let mut quote_cache = QuoteCache::new();
        let mut funding_cache = AHashMap::new();

        let snapshot = rpi_book_message("ws_books_rpi_snapshot.json");
        let update = rpi_book_message("ws_books_rpi_update.json");
        let mut gap = rpi_book_message("ws_books_rpi_update.json");
        let OKXWsMessage::RpiBookData { data, .. } = &mut gap else {
            unreachable!()
        };
        data[0].prev_seq_id -= 1;

        let mut handle = |message| {
            OKXDataClient::handle_ws_message(
                message,
                &sender,
                &instruments_by_symbol,
                &http,
                &OKXDataClientConfig::default(),
                &update_lock,
                &book_channels,
                &book_sync,
                None,
                None,
                &mut quote_cache,
                &mut funding_cache,
                &index_ticker_map,
                &option_greeks_subs,
                BookChannelScope::Public,
                Duration::ZERO,
                &tasks,
                &cancel,
                get_atomic_clock_realtime(),
            );
        };

        handle(snapshot);
        assert!(matches!(receiver.try_recv(), Ok(DataEvent::Data(_))));
        assert!(matches!(
            receiver.try_recv(),
            Err(tokio::sync::mpsc::error::TryRecvError::Empty)
        ));

        handle(gap);
        handle(update);
        assert!(matches!(
            receiver.try_recv(),
            Err(tokio::sync::mpsc::error::TryRecvError::Empty)
        ));

        let mut recovered_snapshot = rpi_book_message("ws_books_rpi_snapshot.json");
        let OKXWsMessage::RpiBookData { data, .. } = &mut recovered_snapshot else {
            unreachable!()
        };
        data[0].seq_id = 2_000;
        handle(recovered_snapshot);
        assert!(matches!(receiver.try_recv(), Ok(DataEvent::Data(_))));
        assert!(matches!(
            receiver.try_recv(),
            Err(tokio::sync::mpsc::error::TryRecvError::Empty)
        ));
    }

    #[rstest]
    fn parse_conventions_returns_both_when_params_missing() {
        let result = parse_greeks_conventions_from_params(&None);
        assert_eq!(result, both());
    }

    #[rstest]
    fn parse_conventions_returns_both_when_key_absent() {
        let mut params = Params::new();
        params.insert("other_key".to_string(), json!("value"));
        let result = parse_greeks_conventions_from_params(&Some(params));
        assert_eq!(result, both());
    }

    #[rstest]
    #[case("BLACK_SCHOLES", OKXGreeksType::Bs)]
    #[case("PRICE_ADJUSTED", OKXGreeksType::Pa)]
    #[case("black_scholes", OKXGreeksType::Bs)]
    #[case("price_adjusted", OKXGreeksType::Pa)]
    fn parse_conventions_accepts_single_string(#[case] raw: &str, #[case] expected: OKXGreeksType) {
        let mut params = Params::new();
        params.insert("greeks_convention".to_string(), json!(raw));
        let result = parse_greeks_conventions_from_params(&Some(params));
        assert_eq!(result, only(expected));
    }

    #[rstest]
    fn parse_conventions_accepts_list_of_strings() {
        let mut params = Params::new();
        params.insert(
            "greeks_convention".to_string(),
            json!(["BLACK_SCHOLES", "PRICE_ADJUSTED"]),
        );
        let result = parse_greeks_conventions_from_params(&Some(params));
        assert_eq!(result, both());
    }

    #[rstest]
    fn parse_conventions_accepts_single_entry_list() {
        let mut params = Params::new();
        params.insert("greeks_convention".to_string(), json!(["PRICE_ADJUSTED"]));
        let result = parse_greeks_conventions_from_params(&Some(params));
        assert_eq!(result, only(OKXGreeksType::Pa));
    }

    #[rstest]
    fn parse_conventions_deduplicates_list_entries() {
        let mut params = Params::new();
        params.insert(
            "greeks_convention".to_string(),
            json!(["BLACK_SCHOLES", "black_scholes"]),
        );
        let result = parse_greeks_conventions_from_params(&Some(params));
        assert_eq!(result, only(OKXGreeksType::Bs));
    }

    #[rstest]
    fn parse_conventions_skips_unknown_list_entries() {
        let mut params = Params::new();
        params.insert(
            "greeks_convention".to_string(),
            json!(["BOGUS", "PRICE_ADJUSTED"]),
        );
        let result = parse_greeks_conventions_from_params(&Some(params));
        assert_eq!(result, only(OKXGreeksType::Pa));
    }

    #[rstest]
    fn parse_conventions_falls_back_to_both_on_all_unknown() {
        let mut params = Params::new();
        params.insert("greeks_convention".to_string(), json!(["BOGUS"]));
        let result = parse_greeks_conventions_from_params(&Some(params));
        assert_eq!(result, both());
    }

    #[rstest]
    #[case(json!(1))]
    #[case(json!(null))]
    #[case(json!(true))]
    #[case(json!({"nested": "object"}))]
    fn parse_conventions_falls_back_on_non_string_value(#[case] value: serde_json::Value) {
        let mut params = Params::new();
        params.insert("greeks_convention".to_string(), value);
        let result = parse_greeks_conventions_from_params(&Some(params));
        assert_eq!(result, both());
    }

    #[rstest]
    fn parse_conventions_falls_back_on_unknown_single_string() {
        let mut params = Params::new();
        params.insert("greeks_convention".to_string(), json!("BOGUS"));
        let result = parse_greeks_conventions_from_params(&Some(params));
        assert_eq!(result, both());
    }

    fn rpi_book_message(filename: &str) -> OKXWsMessage {
        let frame: OKXWsFrame = serde_json::from_str(&load_test_json(filename)).unwrap();
        let OKXWsFrame::RpiBookData { arg, action, data } = frame else {
            panic!("expected RPI book data");
        };
        OKXWsMessage::RpiBookData { arg, action, data }
    }

    fn swap_definition(tick_sz: &str) -> Value {
        json!({
            "alias": "",
            "baseCcy": "",
            "category": "1",
            "ctMult": "1",
            "ctType": "linear",
            "ctVal": "0.01",
            "ctValCcy": "BTC",
            "expTime": "",
            "instFamily": "BTC-USDT",
            "instId": "BTC-USDT-SWAP",
            "instType": "SWAP",
            "lever": "125",
            "listTime": "1611916828000",
            "lotSz": "1",
            "maxIcebergSz": "100000000.0000000000000000",
            "maxLmtAmt": "20000000",
            "maxLmtSz": "100000000",
            "maxMktAmt": "",
            "maxMktSz": "30000",
            "maxStopSz": "30000",
            "maxTriggerSz": "100000000.0000000000000000",
            "maxTwapSz": "100000000.0000000000000000",
            "minSz": "1",
            "optType": "",
            "quoteCcy": "",
            "ruleType": "normal",
            "settleCcy": "USDT",
            "state": "live",
            "stk": "",
            "tickSz": tick_sz,
            "uly": "BTC-USDT"
        })
    }

    fn ws_instruments_message(definition: Value) -> OKXWsMessage {
        let instrument: OKXInstrument =
            serde_json::from_value(definition).expect("valid OKXInstrument");
        OKXWsMessage::Instruments(vec![instrument])
    }

    fn offline_http_client() -> OKXHttpClient {
        OKXHttpClient::new(
            Some("http://127.0.0.1:9".to_string()),
            5,
            0,
            1,
            1,
            OKXEnvironment::Live,
            None,
        )
        .expect("http client")
    }

    fn offline_ws_client() -> OKXWebSocketClient {
        OKXWebSocketClient::new(
            Some("ws://127.0.0.1:9".to_string()),
            None,
            None,
            None,
            None,
            Some(OKX_WS_HEARTBEAT_SECS),
            None,
            TransportBackend::default(),
            None,
        )
        .expect("ws client")
    }

    fn handle_instruments_message(
        sender: &tokio::sync::mpsc::UnboundedSender<DataEvent>,
        instruments_by_symbol: &Arc<AtomicMap<Ustr, InstrumentAny>>,
        http_client: &OKXHttpClient,
        config: &OKXDataClientConfig,
        recovery_ws: Option<&OKXWebSocketClient>,
        business_ws: Option<&OKXWebSocketClient>,
        message: OKXWsMessage,
    ) {
        let book_channels = Arc::new(AtomicMap::new());
        let book_sync = BookSyncTracker::default();
        let update_lock = InstrumentUpdateLock::default();
        let mut quote_cache = QuoteCache::new();
        let mut funding_cache = AHashMap::new();
        let index_ticker_map = Arc::new(AtomicMap::new());
        let option_greeks_subs = Arc::new(AtomicMap::new());
        let tasks = TaskHandles::default();
        let cancel = CancellationToken::new();

        OKXDataClient::handle_ws_message(
            message,
            sender,
            instruments_by_symbol,
            http_client,
            config,
            &update_lock,
            &book_channels,
            &book_sync,
            recovery_ws,
            business_ws,
            &mut quote_cache,
            &mut funding_cache,
            &index_ticker_map,
            &option_greeks_subs,
            BookChannelScope::Public,
            Duration::ZERO,
            &tasks,
            &cancel,
            get_atomic_clock_realtime(),
        );
    }

    #[rstest]
    fn ws_instruments_publishes_new_definition_and_status() {
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        let instruments_by_symbol = Arc::new(AtomicMap::new());
        let http = offline_http_client();
        let ws_public = offline_ws_client();
        let ws_business = offline_ws_client();

        handle_instruments_message(
            &sender,
            &instruments_by_symbol,
            &http,
            &OKXDataClientConfig::default(),
            Some(&ws_public),
            Some(&ws_business),
            ws_instruments_message(swap_definition("0.1")),
        );

        match receiver.try_recv().expect("instrument event") {
            DataEvent::Instrument(instrument) => {
                assert_eq!(instrument.id(), InstrumentId::from("BTC-USDT-SWAP.OKX"));
                assert_eq!(instrument.price_increment(), Price::from("0.1"));
            }
            other => panic!("Expected DataEvent::Instrument, was {other:?}"),
        }

        match receiver.try_recv().expect("instrument status event") {
            DataEvent::InstrumentStatus(status) => {
                assert_eq!(
                    status.instrument_id,
                    InstrumentId::from("BTC-USDT-SWAP.OKX")
                );
                assert_eq!(status.action, MarketStatusAction::Trading);
            }
            other => panic!("Expected DataEvent::InstrumentStatus, was {other:?}"),
        }
        assert!(receiver.try_recv().is_err());

        let symbol = Ustr::from("BTC-USDT-SWAP");
        let cached = instruments_by_symbol
            .get_cloned(&symbol)
            .expect("instrument cached in the shared cache");
        assert_eq!(cached.id(), InstrumentId::from("BTC-USDT-SWAP.OKX"));
        assert_eq!(cached.price_increment(), Price::from("0.1"));
        assert!(
            http.get_instrument(&symbol).is_some(),
            "HTTP client cache must be updated before publishing"
        );
        assert!(
            ws_public.instruments_snapshot().contains_key(&symbol),
            "public WebSocket cache must be updated before publishing"
        );
        assert!(
            ws_business.instruments_snapshot().contains_key(&symbol),
            "business WebSocket cache must be updated before publishing"
        );
    }

    #[rstest]
    fn ws_instruments_unchanged_definition_emits_status_only() {
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        let instruments_by_symbol = Arc::new(AtomicMap::new());
        let http = offline_http_client();

        for _ in 0..2 {
            handle_instruments_message(
                &sender,
                &instruments_by_symbol,
                &http,
                &OKXDataClientConfig::default(),
                None,
                None,
                ws_instruments_message(swap_definition("0.1")),
            );
        }

        assert!(matches!(receiver.try_recv(), Ok(DataEvent::Instrument(_))));
        assert!(matches!(
            receiver.try_recv(),
            Ok(DataEvent::InstrumentStatus(_))
        ));

        match receiver
            .try_recv()
            .expect("status event for repeat definition")
        {
            DataEvent::InstrumentStatus(status) => {
                assert_eq!(
                    status.instrument_id,
                    InstrumentId::from("BTC-USDT-SWAP.OKX")
                );
            }
            other => panic!("Expected DataEvent::InstrumentStatus, was {other:?}"),
        }
        assert!(
            receiver.try_recv().is_err(),
            "unchanged definition must not be republished"
        );
    }

    #[rstest]
    fn ws_instruments_changed_definition_is_republished() {
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        let instruments_by_symbol = Arc::new(AtomicMap::new());
        let http = offline_http_client();

        handle_instruments_message(
            &sender,
            &instruments_by_symbol,
            &http,
            &OKXDataClientConfig::default(),
            None,
            None,
            ws_instruments_message(swap_definition("0.1")),
        );
        handle_instruments_message(
            &sender,
            &instruments_by_symbol,
            &http,
            &OKXDataClientConfig::default(),
            None,
            None,
            ws_instruments_message(swap_definition("0.5")),
        );

        assert!(matches!(receiver.try_recv(), Ok(DataEvent::Instrument(_))));
        assert!(matches!(
            receiver.try_recv(),
            Ok(DataEvent::InstrumentStatus(_))
        ));

        match receiver.try_recv().expect("republished instrument") {
            DataEvent::Instrument(instrument) => {
                assert_eq!(instrument.id(), InstrumentId::from("BTC-USDT-SWAP.OKX"));
                assert_eq!(instrument.price_increment(), Price::from("0.5"));
            }
            other => panic!("Expected DataEvent::Instrument, was {other:?}"),
        }

        match receiver
            .try_recv()
            .expect("status for republished instrument")
        {
            DataEvent::InstrumentStatus(status) => {
                assert_eq!(
                    status.instrument_id,
                    InstrumentId::from("BTC-USDT-SWAP.OKX")
                );
            }
            other => panic!("Expected DataEvent::InstrumentStatus, was {other:?}"),
        }
        assert!(receiver.try_recv().is_err());

        let cached = instruments_by_symbol
            .get_cloned(&Ustr::from("BTC-USDT-SWAP"))
            .expect("instrument cached in the shared cache");
        assert_eq!(cached.price_increment(), Price::from("0.5"));
    }

    #[rstest]
    fn ws_instruments_invalid_definition_emits_status_only() {
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        let instruments_by_symbol = Arc::new(AtomicMap::new());
        let http = offline_http_client();
        let mut definition = swap_definition("0.1");
        definition["uly"] = json!("");

        handle_instruments_message(
            &sender,
            &instruments_by_symbol,
            &http,
            &OKXDataClientConfig::default(),
            None,
            None,
            ws_instruments_message(definition),
        );

        match receiver
            .try_recv()
            .expect("status event for invalid definition")
        {
            DataEvent::InstrumentStatus(status) => {
                assert_eq!(
                    status.instrument_id,
                    InstrumentId::from("BTC-USDT-SWAP.OKX")
                );
            }
            other => panic!("Expected DataEvent::InstrumentStatus, was {other:?}"),
        }
        assert!(
            receiver.try_recv().is_err(),
            "invalid definition must not publish an instrument event"
        );
        assert!(instruments_by_symbol.load().is_empty());
    }

    #[rstest]
    fn ws_instruments_batch_publishes_each_valid_definition() {
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        let instruments_by_symbol = Arc::new(AtomicMap::new());
        let http = offline_http_client();
        let mut eth_definition = swap_definition("0.01");
        eth_definition["instId"] = json!("ETH-USDT-SWAP");
        eth_definition["instFamily"] = json!("ETH-USDT");
        eth_definition["uly"] = json!("ETH-USDT");
        let batch = OKXWsMessage::Instruments(vec![
            serde_json::from_value(swap_definition("0.1")).expect("valid OKXInstrument"),
            serde_json::from_value(eth_definition).expect("valid OKXInstrument"),
        ]);

        handle_instruments_message(
            &sender,
            &instruments_by_symbol,
            &http,
            &OKXDataClientConfig::default(),
            None,
            None,
            batch,
        );

        let mut published = Vec::new();
        let mut statuses = Vec::new();

        while let Ok(event) = receiver.try_recv() {
            match event {
                DataEvent::Instrument(instrument) => published.push(instrument.id()),
                DataEvent::InstrumentStatus(status) => statuses.push(status.instrument_id),
                other => panic!("Unexpected event {other:?}"),
            }
        }

        assert_eq!(
            published,
            vec![
                InstrumentId::from("BTC-USDT-SWAP.OKX"),
                InstrumentId::from("ETH-USDT-SWAP.OKX")
            ],
            "every valid batch item must publish its definition"
        );
        assert_eq!(
            statuses,
            vec![
                InstrumentId::from("BTC-USDT-SWAP.OKX"),
                InstrumentId::from("ETH-USDT-SWAP.OKX")
            ],
            "every batch item must keep its status event"
        );
        assert_eq!(instruments_by_symbol.load().len(), 2);
    }

    #[rstest]
    fn ws_instruments_respects_contract_type_filter() {
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        let instruments_by_symbol = Arc::new(AtomicMap::new());
        let http = offline_http_client();
        let config = OKXDataClientConfig::builder()
            .contract_types(vec![OKXContractType::Inverse])
            .build();

        handle_instruments_message(
            &sender,
            &instruments_by_symbol,
            &http,
            &config,
            None,
            None,
            ws_instruments_message(swap_definition("0.1")),
        );

        match receiver.try_recv().expect("status event") {
            DataEvent::InstrumentStatus(status) => {
                assert_eq!(
                    status.instrument_id,
                    InstrumentId::from("BTC-USDT-SWAP.OKX")
                );
            }
            other => panic!("Expected DataEvent::InstrumentStatus, was {other:?}"),
        }
        assert!(
            receiver.try_recv().is_err(),
            "a definition excluded by the contract type filter must not publish"
        );
        assert!(
            instruments_by_symbol.load().is_empty(),
            "a filtered definition must not enter the instrument cache"
        );

        let inverse_item = test_payload("http_get_instruments_swap.json")["data"][0].clone();
        assert_eq!(inverse_item["instId"], json!("BTC-USD-SWAP"));
        assert_eq!(inverse_item["ctType"], json!("inverse"));
        handle_instruments_message(
            &sender,
            &instruments_by_symbol,
            &http,
            &config,
            None,
            None,
            ws_instruments_message(inverse_item),
        );

        match receiver.try_recv().expect("instrument event") {
            DataEvent::Instrument(instrument) => {
                assert_eq!(instrument.id(), InstrumentId::from("BTC-USD-SWAP.OKX"));
            }
            other => panic!("Expected DataEvent::Instrument, was {other:?}"),
        }
        assert!(matches!(
            receiver.try_recv(),
            Ok(DataEvent::InstrumentStatus(_))
        ));
        assert_eq!(instruments_by_symbol.load().len(), 1);
    }

    #[rstest]
    fn ws_instruments_respects_family_filter() {
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        let instruments_by_symbol = Arc::new(AtomicMap::new());
        let http = offline_http_client();
        let config = OKXDataClientConfig::builder()
            .instrument_types(vec![OKXInstrumentType::Swap])
            .instrument_families(vec!["BTC-USDT".to_string()])
            .build();
        let mut eth_definition = swap_definition("0.01");
        eth_definition["instId"] = json!("ETH-USDT-SWAP");
        eth_definition["instFamily"] = json!("ETH-USDT");
        eth_definition["uly"] = json!("ETH-USDT");

        handle_instruments_message(
            &sender,
            &instruments_by_symbol,
            &http,
            &config,
            None,
            None,
            ws_instruments_message(eth_definition),
        );

        match receiver.try_recv().expect("status event") {
            DataEvent::InstrumentStatus(status) => {
                assert_eq!(
                    status.instrument_id,
                    InstrumentId::from("ETH-USDT-SWAP.OKX")
                );
            }
            other => panic!("Expected DataEvent::InstrumentStatus, was {other:?}"),
        }
        assert!(
            receiver.try_recv().is_err(),
            "a definition outside the configured families must not publish"
        );
        assert!(instruments_by_symbol.load().is_empty());

        handle_instruments_message(
            &sender,
            &instruments_by_symbol,
            &http,
            &config,
            None,
            None,
            ws_instruments_message(swap_definition("0.1")),
        );

        match receiver.try_recv().expect("instrument event") {
            DataEvent::Instrument(instrument) => {
                assert_eq!(instrument.id(), InstrumentId::from("BTC-USDT-SWAP.OKX"));
            }
            other => panic!("Expected DataEvent::Instrument, was {other:?}"),
        }
        assert!(matches!(
            receiver.try_recv(),
            Ok(DataEvent::InstrumentStatus(_))
        ));
        assert_eq!(instruments_by_symbol.load().len(), 1);
    }

    #[tokio::test]
    async fn reconcile_fetches_duplicate_configured_families_once() {
        let state = RefreshServerState {
            instruments_payload: Arc::new(tokio::sync::Mutex::new(test_payload(
                "http_get_instruments_swap.json",
            ))),
            ..RefreshServerState::default()
        };
        let addr = start_refresh_server(state.clone()).await;
        let http = refresh_http_client(addr);
        let config = OKXDataClientConfig::builder()
            .instrument_types(vec![OKXInstrumentType::Swap])
            .instrument_families(vec!["BTC-USD".to_string(), "BTC-USD".to_string()])
            .build();
        let instruments_by_symbol = Arc::new(AtomicMap::new());
        let update_lock = InstrumentUpdateLock::default();
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();

        let summary = reconcile_instruments(
            &http,
            &config,
            &instruments_by_symbol,
            &update_lock,
            None,
            None,
            &sender,
        )
        .await
        .expect("reconcile");

        let queries = state.instrument_queries.lock().await;
        assert_eq!(
            queries.len(),
            1,
            "a duplicated family must be fetched only once"
        );
        drop(queries);
        assert_eq!(summary.fetched, 1);
        assert_eq!(summary.changed, 1);
        assert_eq!(
            instrument_events(&mut receiver).len(),
            1,
            "a duplicated family must not publish its instruments twice"
        );
    }

    #[tokio::test]
    async fn reconcile_fetches_duplicate_configured_types_once() {
        let state = RefreshServerState {
            instruments_payload: Arc::new(tokio::sync::Mutex::new(test_payload(
                "http_get_instruments_swap.json",
            ))),
            ..RefreshServerState::default()
        };
        let addr = start_refresh_server(state.clone()).await;
        let http = refresh_http_client(addr);
        let config = OKXDataClientConfig::builder()
            .instrument_types(vec![OKXInstrumentType::Swap, OKXInstrumentType::Swap])
            .build();
        let instruments_by_symbol = Arc::new(AtomicMap::new());
        let update_lock = InstrumentUpdateLock::default();
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();

        let summary = reconcile_instruments(
            &http,
            &config,
            &instruments_by_symbol,
            &update_lock,
            None,
            None,
            &sender,
        )
        .await
        .expect("reconcile");

        let queries = state.instrument_queries.lock().await;
        assert_eq!(
            queries.len(),
            1,
            "a duplicated type must be fetched only once"
        );
        drop(queries);
        assert_eq!(summary.fetched, 3);
        assert_eq!(summary.changed, 3);
        assert_eq!(
            instrument_events(&mut receiver).len(),
            3,
            "a duplicated type must not publish its instruments twice"
        );
    }

    #[rstest]
    fn definition_in_scope_matches_events_family_on_series_id() {
        let okx_inst = OKXInstrument {
            inst_type: OKXInstrumentType::Events,
            inst_id: Ustr::from("BTC-ABOVE-DAILY-260224-1600-65000"),
            inst_id_code: Some(1000000001),
            uly: Ustr::from(""),
            inst_family: Ustr::from(""),
            series_id: Some(Ustr::from("BTC-ABOVE-DAILY")),
            inst_category: Some(crate::common::enums::OKXInstrumentCategory::Crypto),
            init_px_lmt_pct: String::new(),
            float_px_lmt_pct: String::new(),
            max_px_lmt_pct: String::new(),
            base_ccy: Ustr::from(""),
            quote_ccy: Ustr::from("USDT"),
            settle_ccy: Ustr::from("USDT"),
            ct_val: String::new(),
            ct_mult: String::new(),
            ct_val_ccy: String::new(),
            opt_type: crate::common::enums::OKXOptionType::None,
            stk: String::new(),
            list_time: Some(1769697132335),
            exp_time: Some(1769700732335),
            lever: String::new(),
            tick_sz: "0.001".to_string(),
            lot_sz: "1".to_string(),
            min_sz: "1".to_string(),
            ct_type: OKXContractType::None,
            state: OKXInstrumentStatus::Settling,
            rule_type: "normal".to_string(),
            max_lmt_sz: "1000000".to_string(),
            max_mkt_sz: "1000000".to_string(),
            max_lmt_amt: String::new(),
            max_mkt_amt: String::new(),
            max_twap_sz: String::new(),
            max_iceberg_sz: String::new(),
            max_trigger_sz: String::new(),
            max_stop_sz: String::new(),
            rpi: None,
            rpi_min_level: None,
            rpi_min_px_band: None,
        };
        let instrument = crate::common::parse::parse_event_contract_instrument(
            &okx_inst,
            None,
            None,
            None,
            None,
            UnixNanos::from(1u64),
        )
        .expect("parse events instrument");
        let matching = OKXDataClientConfig::builder()
            .instrument_types(vec![OKXInstrumentType::Events])
            .instrument_families(vec!["BTC-ABOVE-DAILY".to_string()])
            .build();
        let other = OKXDataClientConfig::builder()
            .instrument_types(vec![OKXInstrumentType::Events])
            .instrument_families(vec!["ETH-ABOVE-DAILY".to_string()])
            .build();

        assert!(definition_in_scope(&matching, &okx_inst, &instrument));
        assert!(!definition_in_scope(&other, &okx_inst, &instrument));
    }

    #[tokio::test]
    async fn ws_definition_matching_rest_fetch_is_not_republished() {
        let state = RefreshServerState {
            instruments_payload: Arc::new(tokio::sync::Mutex::new(test_payload(
                "http_get_instruments_swap.json",
            ))),
            ..RefreshServerState::default()
        };
        let addr = start_refresh_server(state).await;
        let http = refresh_http_client(addr);
        let config = OKXDataClientConfig::builder()
            .instrument_types(vec![OKXInstrumentType::Swap])
            .build();
        let instruments_by_symbol = Arc::new(AtomicMap::new());
        let update_lock = InstrumentUpdateLock::default();
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();

        reconcile_instruments(
            &http,
            &config,
            &instruments_by_symbol,
            &update_lock,
            None,
            None,
            &sender,
        )
        .await
        .expect("reconcile");
        assert_eq!(instrument_events(&mut receiver).len(), 3);

        // Feed the same venue definition back through the WebSocket handler:
        // cross-source parsing must compare equal, so nothing is republished
        let rest_item = test_payload("http_get_instruments_swap.json")["data"][2].clone();
        assert_eq!(rest_item["instId"], json!("BTC-USDT-SWAP"));
        handle_instruments_message(
            &sender,
            &instruments_by_symbol,
            &http,
            &OKXDataClientConfig::default(),
            None,
            None,
            ws_instruments_message(rest_item),
        );

        match receiver.try_recv().expect("status event") {
            DataEvent::InstrumentStatus(status) => {
                assert_eq!(
                    status.instrument_id,
                    InstrumentId::from("BTC-USDT-SWAP.OKX")
                );
            }
            other => panic!("Expected DataEvent::InstrumentStatus, was {other:?}"),
        }
        assert!(
            receiver.try_recv().is_err(),
            "a definition identical to the REST fetch must not be republished"
        );
    }

    #[rstest]
    fn definitions_match_ignores_event_timestamps() {
        let okx_instrument: OKXInstrument =
            serde_json::from_value(swap_definition("0.1")).expect("valid OKXInstrument");
        let first = parse_instrument_any(
            &okx_instrument,
            None,
            None,
            None,
            None,
            UnixNanos::from(1u64),
        )
        .expect("parse")
        .expect("instrument");
        let second = parse_instrument_any(
            &okx_instrument,
            None,
            None,
            None,
            None,
            UnixNanos::from(2u64),
        )
        .expect("parse")
        .expect("instrument");

        assert!(instrument_definitions_match(&first, &second));
    }

    #[rstest]
    fn definitions_match_detects_increment_changes() {
        let mut pair = currency_pair_btcusdt();
        let mut changed = pair.clone();
        changed.price_increment = Price::from("0.5");
        pair.ts_event = UnixNanos::from(1u64);
        changed.ts_event = UnixNanos::from(2u64);

        assert!(!instrument_definitions_match(
            &InstrumentAny::CurrencyPair(pair),
            &InstrumentAny::CurrencyPair(changed),
        ));
    }

    #[rstest]
    fn definitions_match_detects_info_changes() {
        let pair = currency_pair_btcusdt();
        let mut changed = pair.clone();
        let mut info = Params::new();
        info.insert("okx_rpi_min_level".to_string(), json!(5));
        changed.info = Some(info);

        assert!(!instrument_definitions_match(
            &InstrumentAny::CurrencyPair(pair),
            &InstrumentAny::CurrencyPair(changed),
        ));
    }

    #[rstest]
    fn definitions_match_detects_id_changes() {
        let pair = currency_pair_btcusdt();
        let mut other = pair.clone();
        other.id = InstrumentId::from("ETH-USDT.OKX");

        assert!(!instrument_definitions_match(
            &InstrumentAny::CurrencyPair(pair),
            &InstrumentAny::CurrencyPair(other),
        ));
    }

    #[derive(Clone, Default)]
    struct RefreshServerState {
        instruments_payload: Arc<tokio::sync::Mutex<Value>>,
        spreads_payload: Arc<tokio::sync::Mutex<Value>>,
        instrument_queries: Arc<tokio::sync::Mutex<Vec<HashMap<String, String>>>>,
        spread_queries: Arc<tokio::sync::Mutex<Vec<HashMap<String, String>>>>,
        fail_instruments: bool,
        gate_instruments: Option<Arc<tokio::sync::Semaphore>>,
    }

    async fn start_refresh_server(state: RefreshServerState) -> SocketAddr {
        let instruments_state = state.clone();
        let spreads_state = state;

        let router =
            Router::new()
                .route(
                    "/api/v5/public/instruments",
                    get(move |Query(params): Query<HashMap<String, String>>| {
                        let state = instruments_state.clone();
                        async move {
                            state.instrument_queries.lock().await.push(params.clone());

                            if let Some(gate) = &state.gate_instruments {
                                gate.acquire()
                                    .await
                                    .expect("instruments gate open")
                                    .forget();
                            }

                            if state.fail_instruments {
                                return (
                                    axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                                    Json(json!({
                                        "code": "50000",
                                        "msg": "instruments endpoint unavailable",
                                        "data": []
                                    })),
                                );
                            }

                            let family = params.get("instFamily").cloned();
                            let mut payload = state.instruments_payload.lock().await.clone();

                            if let Some(family) = family
                                && let Some(data) =
                                    payload.get_mut("data").and_then(Value::as_array_mut)
                            {
                                data.retain(|item| {
                                    item.get("instFamily").and_then(Value::as_str)
                                        == Some(family.as_str())
                                });
                            }
                            (axum::http::StatusCode::OK, Json(payload))
                        }
                    }),
                )
                .route(
                    "/api/v5/sprd/spreads",
                    get(move |Query(params): Query<HashMap<String, String>>| {
                        let state = spreads_state.clone();
                        async move {
                            state.spread_queries.lock().await.push(params);
                            Json(state.spreads_payload.lock().await.clone())
                        }
                    }),
                )
                .route(
                    "/ws/public",
                    get(|ws: axum::extract::ws::WebSocketUpgrade| async move {
                        ws.on_upgrade(drain_ws)
                    }),
                )
                .route(
                    "/ws/business",
                    get(|ws: axum::extract::ws::WebSocketUpgrade| async move {
                        ws.on_upgrade(drain_ws)
                    }),
                );

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let addr = listener.local_addr().expect("local_addr");
        tokio::spawn(async move { axum::serve(listener, router).await.expect("serve") });
        addr
    }

    async fn drain_ws(mut socket: axum::extract::ws::WebSocket) {
        while socket.next().await.is_some() {}
    }

    fn test_payload(filename: &str) -> Value {
        serde_json::from_str(&load_test_json(filename)).expect("valid json fixture")
    }

    fn refresh_http_client(addr: SocketAddr) -> OKXHttpClient {
        OKXHttpClient::new(
            Some(format!("http://{addr}")),
            5,
            0,
            1,
            1,
            OKXEnvironment::Live,
            None,
        )
        .expect("http client")
    }

    fn spot_refresh_state() -> RefreshServerState {
        RefreshServerState {
            instruments_payload: Arc::new(tokio::sync::Mutex::new(test_payload(
                "http_get_instruments_spot.json",
            ))),
            ..RefreshServerState::default()
        }
    }

    fn instrument_events(
        receiver: &mut tokio::sync::mpsc::UnboundedReceiver<DataEvent>,
    ) -> Vec<InstrumentAny> {
        let mut events = Vec::new();
        while let Ok(DataEvent::Instrument(instrument)) = receiver.try_recv() {
            events.push(instrument);
        }
        events
    }

    #[tokio::test]
    async fn reconcile_publishes_only_new_or_changed_and_retains_missing() {
        let state = spot_refresh_state();
        let addr = start_refresh_server(state.clone()).await;
        let http = refresh_http_client(addr);
        let config = OKXDataClientConfig::default();
        let instruments_by_symbol = Arc::new(AtomicMap::new());
        let update_lock = InstrumentUpdateLock::default();
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();

        let summary = reconcile_instruments(
            &http,
            &config,
            &instruments_by_symbol,
            &update_lock,
            None,
            None,
            &sender,
        )
        .await
        .expect("initial reconcile");
        assert_eq!(summary.fetched, 5);
        assert_eq!(summary.changed, 5);
        assert_eq!(summary.missing, 0);
        assert_eq!(instrument_events(&mut receiver).len(), 5);
        assert_eq!(instruments_by_symbol.load().len(), 5);

        let summary = reconcile_instruments(
            &http,
            &config,
            &instruments_by_symbol,
            &update_lock,
            None,
            None,
            &sender,
        )
        .await
        .expect("unchanged reconcile");
        assert_eq!(summary.fetched, 5);
        assert_eq!(summary.changed, 0);
        assert_eq!(summary.missing, 0);
        assert!(
            instrument_events(&mut receiver).is_empty(),
            "unchanged definitions must not be republished"
        );

        {
            let mut payload = state.instruments_payload.lock().await;
            payload["data"][0]["tickSz"] = json!("0.5");
        }
        let summary = reconcile_instruments(
            &http,
            &config,
            &instruments_by_symbol,
            &update_lock,
            None,
            None,
            &sender,
        )
        .await
        .expect("changed reconcile");
        assert_eq!(summary.changed, 1);
        let events = instrument_events(&mut receiver);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].id(), InstrumentId::from("BTC-USD.OKX"));
        assert_eq!(events[0].price_increment(), Price::from("0.5"));

        {
            let mut payload = state.instruments_payload.lock().await;
            let mut new_instrument = payload["data"][0].clone();
            new_instrument["instId"] = json!("ETH-USDT");
            new_instrument["baseCcy"] = json!("ETH");
            new_instrument["quoteCcy"] = json!("USDT");
            payload["data"]
                .as_array_mut()
                .expect("data array")
                .push(new_instrument);
        }
        let summary = reconcile_instruments(
            &http,
            &config,
            &instruments_by_symbol,
            &update_lock,
            None,
            None,
            &sender,
        )
        .await
        .expect("new listing reconcile");
        assert_eq!(summary.fetched, 6);
        assert_eq!(summary.changed, 1);
        let events = instrument_events(&mut receiver);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].id(), InstrumentId::from("ETH-USDT.OKX"));
        assert_eq!(instruments_by_symbol.load().len(), 6);

        {
            let mut payload = state.instruments_payload.lock().await;
            payload["data"]
                .as_array_mut()
                .expect("data array")
                .remove(0);
        }
        let summary = reconcile_instruments(
            &http,
            &config,
            &instruments_by_symbol,
            &update_lock,
            None,
            None,
            &sender,
        )
        .await
        .expect("removal reconcile");
        assert_eq!(summary.fetched, 5);
        assert_eq!(summary.changed, 0);
        assert_eq!(summary.missing, 1);
        assert!(
            instrument_events(&mut receiver).is_empty(),
            "removed instruments must not publish events"
        );
        assert!(
            instruments_by_symbol
                .get_cloned(&Ustr::from("BTC-USD"))
                .is_some(),
            "removed instruments are retained in the cache"
        );
    }

    #[tokio::test]
    async fn reconcile_surfaces_fetch_errors_without_publishing() {
        let state = RefreshServerState {
            fail_instruments: true,
            ..spot_refresh_state()
        };
        let addr = start_refresh_server(state).await;
        let http = refresh_http_client(addr);
        let config = OKXDataClientConfig::default();
        let instruments_by_symbol = Arc::new(AtomicMap::new());
        let update_lock = InstrumentUpdateLock::default();
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();

        let result = reconcile_instruments(
            &http,
            &config,
            &instruments_by_symbol,
            &update_lock,
            None,
            None,
            &sender,
        )
        .await;

        assert!(result.is_err(), "fetch failure must surface as an error");
        assert!(instruments_by_symbol.load().is_empty());
        assert!(
            receiver.try_recv().is_err(),
            "a failed reconcile must not publish events"
        );
    }

    #[tokio::test]
    async fn reconcile_requests_each_configured_family() {
        let state = RefreshServerState {
            instruments_payload: Arc::new(tokio::sync::Mutex::new(test_payload(
                "http_get_instruments_swap.json",
            ))),
            ..RefreshServerState::default()
        };
        let addr = start_refresh_server(state.clone()).await;
        let http = refresh_http_client(addr);
        let config = OKXDataClientConfig::builder()
            .instrument_types(vec![OKXInstrumentType::Swap])
            .instrument_families(vec!["BTC-USD".to_string(), "BTC-USDT".to_string()])
            .build();
        let instruments_by_symbol = Arc::new(AtomicMap::new());
        let update_lock = InstrumentUpdateLock::default();
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();

        let summary = reconcile_instruments(
            &http,
            &config,
            &instruments_by_symbol,
            &update_lock,
            None,
            None,
            &sender,
        )
        .await
        .expect("reconcile");

        let queries = state.instrument_queries.lock().await;
        let families: Vec<Option<String>> = queries
            .iter()
            .map(|query| query.get("instFamily").cloned())
            .collect();
        assert_eq!(queries.len(), 2);
        assert!(families.contains(&Some("BTC-USD".to_string())));
        assert!(families.contains(&Some("BTC-USDT".to_string())));
        drop(queries);

        assert_eq!(summary.fetched, 2);
        assert_eq!(summary.changed, 2);
        let ids: Vec<InstrumentId> = instrument_events(&mut receiver)
            .iter()
            .map(Instrument::id)
            .collect();
        assert!(ids.contains(&InstrumentId::from("BTC-USD-SWAP.OKX")));
        assert!(ids.contains(&InstrumentId::from("BTC-USDT-SWAP.OKX")));
    }

    #[rstest]
    #[case::inverse_keeps_inverse_only(vec![OKXContractType::Inverse], 1, "BTC-USD-SWAP.OKX")]
    #[case::linear_keeps_linear_only(vec![OKXContractType::Linear], 2, "BTC-USDT-SWAP.OKX")]
    #[tokio::test]
    async fn reconcile_applies_contract_type_filter(
        #[case] filter: Vec<OKXContractType>,
        #[case] expected_count: usize,
        #[case] expected_id: &str,
    ) {
        let state = RefreshServerState {
            instruments_payload: Arc::new(tokio::sync::Mutex::new(test_payload(
                "http_get_instruments_swap.json",
            ))),
            ..RefreshServerState::default()
        };
        let addr = start_refresh_server(state).await;
        let http = refresh_http_client(addr);
        let config = OKXDataClientConfig::builder()
            .instrument_types(vec![OKXInstrumentType::Swap])
            .contract_types(filter)
            .build();
        let instruments_by_symbol = Arc::new(AtomicMap::new());
        let update_lock = InstrumentUpdateLock::default();
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();

        let summary = reconcile_instruments(
            &http,
            &config,
            &instruments_by_symbol,
            &update_lock,
            None,
            None,
            &sender,
        )
        .await
        .expect("reconcile");

        assert_eq!(summary.fetched, expected_count);
        assert_eq!(summary.changed, expected_count);
        let events = instrument_events(&mut receiver);
        assert_eq!(events.len(), expected_count);
        assert!(
            events
                .iter()
                .any(|i| i.id() == InstrumentId::from(expected_id)),
            "expected {expected_id} in filtered results"
        );
    }

    #[tokio::test]
    async fn reconcile_includes_spreads_when_load_spreads_enabled() {
        let state = RefreshServerState {
            instruments_payload: Arc::new(tokio::sync::Mutex::new(test_payload(
                "http_get_instruments_spot.json",
            ))),
            spreads_payload: Arc::new(tokio::sync::Mutex::new(test_payload(
                "http_get_spreads.json",
            ))),
            ..RefreshServerState::default()
        };
        let addr = start_refresh_server(state.clone()).await;
        let http = refresh_http_client(addr);
        let config = OKXDataClientConfig::builder().load_spreads(true).build();
        let instruments_by_symbol = Arc::new(AtomicMap::new());
        let update_lock = InstrumentUpdateLock::default();
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();

        let summary = reconcile_instruments(
            &http,
            &config,
            &instruments_by_symbol,
            &update_lock,
            None,
            None,
            &sender,
        )
        .await
        .expect("reconcile");

        assert_eq!(state.spread_queries.lock().await.len(), 1);
        assert_eq!(summary.fetched, 7);
        assert_eq!(summary.changed, 7);
        let ids: Vec<InstrumentId> = instrument_events(&mut receiver)
            .iter()
            .map(Instrument::id)
            .collect();
        assert!(ids.contains(&InstrumentId::from("ETH-USD-SWAP_ETH-USD-231229.OKX")));
        assert!(ids.contains(&InstrumentId::from("BTC-USDT_BTC-USDT-SWAP.OKX")));
    }

    #[tokio::test]
    async fn reconcile_updates_all_caches_before_publishing() {
        let state = spot_refresh_state();
        let addr = start_refresh_server(state).await;
        let http = refresh_http_client(addr);
        let config = OKXDataClientConfig::default();
        let instruments_by_symbol = Arc::new(AtomicMap::new());
        let update_lock = Arc::new(InstrumentUpdateLock::default());
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        let ws = offline_ws_client();
        let ws_business = offline_ws_client();

        let instruments_task = instruments_by_symbol.clone();
        let update_lock_task = update_lock.clone();
        let http_task = http.clone();
        let ws_task = ws.clone();
        let ws_business_task = ws_business.clone();

        let reconcile = tokio::spawn(async move {
            reconcile_instruments(
                &http_task,
                &config,
                &instruments_task,
                &update_lock_task,
                Some(&ws_task),
                Some(&ws_business_task),
                &sender,
            )
            .await
        });

        let event = receiver.recv().await.expect("instrument event");
        let DataEvent::Instrument(instrument) = event else {
            panic!("Expected DataEvent::Instrument, was {event:?}");
        };

        assert!(
            instruments_by_symbol
                .load()
                .contains_key(&instrument.symbol().inner()),
            "data client cache must be updated before publishing"
        );
        assert!(
            http.get_instrument(&instrument.symbol().inner()).is_some(),
            "HTTP client cache must be updated before publishing"
        );
        assert!(
            ws.instruments_snapshot()
                .contains_key(&instrument.symbol().inner()),
            "public WebSocket cache must be updated before publishing"
        );
        assert!(
            ws_business
                .instruments_snapshot()
                .contains_key(&instrument.symbol().inner()),
            "business WebSocket cache must be updated before publishing"
        );
        reconcile.await.expect("reconcile task").expect("reconcile");
    }

    #[tokio::test]
    async fn reconcile_skips_publish_when_cache_changes_during_fetch() {
        let gate = Arc::new(tokio::sync::Semaphore::new(0));
        let state = RefreshServerState {
            gate_instruments: Some(gate.clone()),
            ..spot_refresh_state()
        };
        let addr = start_refresh_server(state.clone()).await;
        let http = refresh_http_client(addr);
        let config = OKXDataClientConfig::default();
        let instruments_by_symbol = Arc::new(AtomicMap::new());
        let update_lock = Arc::new(InstrumentUpdateLock::default());
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();

        gate.add_permits(1);
        let summary = reconcile_instruments(
            &http,
            &config,
            &instruments_by_symbol,
            &update_lock,
            None,
            None,
            &sender,
        )
        .await
        .expect("seed reconcile");
        assert_eq!(summary.changed, 5);
        assert_eq!(instrument_events(&mut receiver).len(), 5);

        let reconcile = {
            let http = http.clone();
            let instruments_by_symbol = instruments_by_symbol.clone();
            let update_lock = update_lock.clone();
            let sender = sender.clone();

            tokio::spawn(async move {
                reconcile_instruments(
                    &http,
                    &config,
                    &instruments_by_symbol,
                    &update_lock,
                    None,
                    None,
                    &sender,
                )
                .await
            })
        };

        let deadline = Instant::now() + Duration::from_secs(5);
        while state.instrument_queries.lock().await.len() < 2 {
            assert!(Instant::now() < deadline, "refresh fetch not in flight");
            tokio::time::sleep(Duration::from_millis(10)).await;
        }

        // A concurrent update lands while the refresh fetch is in flight
        let mut v2_item = test_payload("http_get_instruments_spot.json")["data"][0].clone();
        v2_item["tickSz"] = json!("0.5");
        let v2: OKXInstrument = serde_json::from_value(v2_item).expect("valid OKXInstrument");
        let v2 = parse_instrument_any(&v2, None, None, None, None, UnixNanos::from(1u64))
            .expect("parse")
            .expect("instrument");
        {
            let _guard = update_lock
                .mutex
                .lock()
                .expect("instrument update lock poisoned");
            publish_instrument_updates(
                std::slice::from_ref(&v2),
                &instruments_by_symbol,
                &http,
                None,
                None,
                &update_lock,
                &sender,
            );
        }

        match receiver.try_recv().expect("concurrent update event") {
            DataEvent::Instrument(instrument) => {
                assert_eq!(instrument.price_increment(), Price::from("0.5"));
            }
            other => panic!("Expected DataEvent::Instrument, was {other:?}"),
        }

        gate.add_permits(1);
        let summary = reconcile.await.expect("reconcile task").expect("reconcile");
        assert_eq!(
            summary.changed, 0,
            "a pass whose snapshot went stale mid-fetch must skip publishing"
        );
        assert!(
            instrument_events(&mut receiver).is_empty(),
            "the stale pass must not republish the older definition"
        );
        let cached = instruments_by_symbol
            .get_cloned(&Ustr::from("BTC-USD"))
            .expect("instrument cached");
        assert_eq!(
            cached.price_increment(),
            Price::from("0.5"),
            "the instrument cache keeps the fresher concurrent definition"
        );
        assert_eq!(
            http.get_instrument(&Ustr::from("BTC-USD"))
                .map(|instrument| instrument.price_increment()),
            Some(Price::from("0.5")),
            "the HTTP cache keeps the fresher concurrent definition"
        );
    }

    #[tokio::test]
    async fn spawn_instrument_refresh_skipped_when_interval_zero() {
        let config = OKXDataClientConfig::builder()
            .update_instruments_interval_mins(0)
            .build();
        let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel();
        replace_data_event_sender(sender);
        let client = OKXDataClient::new(*OKX_CLIENT_ID, config).expect("data client");

        client.spawn_instrument_refresh();
        assert!(client.tasks.is_empty());
    }

    #[rstest]
    #[case::reset(DataTaskBoundary::Reset)]
    #[case::dispose(DataTaskBoundary::Dispose)]
    #[case::repeated_stop(DataTaskBoundary::RepeatedStop)]
    #[tokio::test]
    async fn lifecycle_boundary_terminates_owned_data_task(#[case] boundary: DataTaskBoundary) {
        let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel();
        replace_data_event_sender(sender);
        let mut client = OKXDataClient::new(*OKX_CLIENT_ID, OKXDataClientConfig::default())
            .expect("data client");

        if matches!(boundary, DataTaskBoundary::RepeatedStop) {
            client.stop().expect("initial stop");
        }

        let (drop_tx, drop_rx) = tokio::sync::oneshot::channel();
        let signal = DropSignal(Some(drop_tx));
        client.spawn_ws(
            async move {
                let _signal = signal;
                std::future::pending::<anyhow::Result<()>>().await
            },
            "pending lifecycle task",
        );

        match boundary {
            DataTaskBoundary::Reset => client.reset().expect("reset"),
            DataTaskBoundary::Dispose => client.dispose().expect("dispose"),
            DataTaskBoundary::RepeatedStop => client.stop().expect("repeated stop"),
        }

        tokio::time::timeout(Duration::from_secs(1), drop_rx)
            .await
            .expect("lifecycle boundary must drop the owned task")
            .expect("drop signal");
        terminate_tasks(&client.tasks, "test data client")
            .await
            .expect("data task terminated");
        assert!(client.tasks.is_empty());
    }

    #[tokio::test]
    async fn reset_prevents_in_flight_request_from_publishing() {
        let gate = Arc::new(tokio::sync::Semaphore::new(0));
        let state = RefreshServerState {
            gate_instruments: Some(Arc::clone(&gate)),
            ..spot_refresh_state()
        };
        let addr = start_refresh_server(state.clone()).await;
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        replace_data_event_sender(sender);
        let config = OKXDataClientConfig {
            instrument_types: vec![OKXInstrumentType::Spot],
            base_url_http: Some(format!("http://{addr}")),
            http_timeout_secs: 5,
            max_retries: 0,
            retry_delay_initial_ms: 1,
            retry_delay_max_ms: 1,
            ..OKXDataClientConfig::default()
        };
        let mut client = OKXDataClient::new(*OKX_CLIENT_ID, config).expect("data client");
        let request = RequestInstruments::new(
            None,
            None,
            Some(*OKX_CLIENT_ID),
            None,
            UUID4::new(),
            UnixNanos::default(),
            None,
        );

        client
            .request_instruments(request)
            .expect("request instruments");
        wait_until_async(
            || {
                let state = state.clone();
                async move { !state.instrument_queries.lock().await.is_empty() }
            },
            Duration::from_secs(1),
        )
        .await;

        client.reset().expect("reset");
        wait_until_async(
            || async { client.tasks.all_finished() },
            Duration::from_secs(1),
        )
        .await;
        gate.add_permits(1);

        assert!(client.instruments_by_symbol.load().is_empty());
        assert!(matches!(
            receiver.try_recv(),
            Err(tokio::sync::mpsc::error::TryRecvError::Empty)
        ));
        terminate_tasks(&client.tasks, "test data client")
            .await
            .expect("data task terminated");
    }

    #[tokio::test]
    async fn spawn_instrument_refresh_registers_task() {
        let config = OKXDataClientConfig::builder()
            .update_instruments_interval_mins(60)
            .build();
        let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel();
        replace_data_event_sender(sender);
        let client = OKXDataClient::new(*OKX_CLIENT_ID, config).expect("data client");

        client.spawn_instrument_refresh();
        assert_eq!(client.tasks.len(), 1);

        client.cancellation_token.cancel();
        for handle in client.tasks.take_all() {
            handle.await.expect("refresh task joins after cancel");
        }
    }

    #[tokio::test]
    async fn reconnect_does_not_leak_refresh_tasks() {
        let state = spot_refresh_state();
        let addr = start_refresh_server(state).await;
        let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel();
        replace_data_event_sender(sender);
        let config = OKXDataClientConfig {
            instrument_types: vec![OKXInstrumentType::Spot],
            base_url_http: Some(format!("http://{addr}")),
            base_url_ws_public: Some(format!("ws://{addr}/ws/public")),
            base_url_ws_business: Some(format!("ws://{addr}/ws/business")),
            environment: OKXEnvironment::Live,
            http_timeout_secs: 5,
            max_retries: 0,
            retry_delay_initial_ms: 1,
            retry_delay_max_ms: 1,
            book_stale_check_interval_secs: 0,
            update_instruments_interval_mins: 60,
            ..OKXDataClientConfig::default()
        };
        let mut client = OKXDataClient::new(*OKX_CLIENT_ID, config).expect("data client");

        for cycle in 1..=2 {
            client.connect().await.expect("connect");
            assert_eq!(
                client.tasks.len(),
                3,
                "cycle {cycle}: two stream tasks and one refresh task"
            );
            client.disconnect().await.expect("disconnect");
            assert!(
                client.tasks.is_empty(),
                "cycle {cycle}: teardown must join every task"
            );
        }

        client.connect().await.expect("connect");
        client.connect().await.expect("repeated connect is a no-op");
        assert_eq!(client.tasks.len(), 3);
        client.disconnect().await.expect("disconnect");
    }

    #[tokio::test]
    async fn reset_drains_old_generation_before_reconnect() {
        let state = spot_refresh_state();
        let addr = start_refresh_server(state).await;
        let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel();
        replace_data_event_sender(sender);
        let config = OKXDataClientConfig {
            instrument_types: vec![OKXInstrumentType::Spot],
            base_url_http: Some(format!("http://{addr}")),
            base_url_ws_public: Some(format!("ws://{addr}/ws/public")),
            base_url_ws_business: Some(format!("ws://{addr}/ws/business")),
            environment: OKXEnvironment::Live,
            http_timeout_secs: 5,
            max_retries: 0,
            retry_delay_initial_ms: 1,
            retry_delay_max_ms: 1,
            book_stale_check_interval_secs: 0,
            update_instruments_interval_mins: 60,
            ..OKXDataClientConfig::default()
        };
        let mut client = OKXDataClient::new(*OKX_CLIENT_ID, config).expect("data client");

        client.connect().await.expect("initial connect");
        client.reset().expect("reset");
        client.connect().await.expect("reconnect after reset");

        assert_eq!(client.tasks.len(), 3);
        assert!(!client.tasks.all_finished());
        client.disconnect().await.expect("disconnect");
    }

    #[tokio::test]
    async fn reconnect_does_not_republish_unchanged_instruments() {
        let state = spot_refresh_state();
        let addr = start_refresh_server(state.clone()).await;
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        replace_data_event_sender(sender);
        let config = OKXDataClientConfig {
            instrument_types: vec![OKXInstrumentType::Spot],
            base_url_http: Some(format!("http://{addr}")),
            base_url_ws_public: Some(format!("ws://{addr}/ws/public")),
            base_url_ws_business: Some(format!("ws://{addr}/ws/business")),
            environment: OKXEnvironment::Live,
            http_timeout_secs: 5,
            max_retries: 0,
            retry_delay_initial_ms: 1,
            retry_delay_max_ms: 1,
            book_stale_check_interval_secs: 0,
            update_instruments_interval_mins: 60,
            ..OKXDataClientConfig::default()
        };
        let mut client = OKXDataClient::new(*OKX_CLIENT_ID, config).expect("data client");

        client.connect().await.expect("first connect");
        assert_eq!(
            instrument_events(&mut receiver).len(),
            5,
            "first connect publishes the full cache"
        );
        client.disconnect().await.expect("disconnect");

        client.connect().await.expect("reconnect");
        assert!(
            instrument_events(&mut receiver).is_empty(),
            "reconnect must not republish unchanged instruments"
        );
        client.disconnect().await.expect("disconnect");

        {
            let mut payload = state.instruments_payload.lock().await;
            payload["data"][0]["tickSz"] = json!("0.5");
        }
        client.connect().await.expect("third connect");
        let events = instrument_events(&mut receiver);
        assert_eq!(
            events.len(),
            1,
            "reconnect publishes only changed definitions"
        );
        assert_eq!(events[0].id(), InstrumentId::from("BTC-USD.OKX"));
        client.disconnect().await.expect("disconnect");
    }

    #[tokio::test]
    async fn stop_cancels_refresh_task() {
        let state = spot_refresh_state();
        let addr = start_refresh_server(state).await;
        let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel();
        replace_data_event_sender(sender);
        let config = OKXDataClientConfig {
            instrument_types: vec![OKXInstrumentType::Spot],
            base_url_http: Some(format!("http://{addr}")),
            base_url_ws_public: Some(format!("ws://{addr}/ws/public")),
            base_url_ws_business: Some(format!("ws://{addr}/ws/business")),
            environment: OKXEnvironment::Live,
            http_timeout_secs: 5,
            max_retries: 0,
            retry_delay_initial_ms: 1,
            retry_delay_max_ms: 1,
            book_stale_check_interval_secs: 0,
            update_instruments_interval_mins: 60,
            ..OKXDataClientConfig::default()
        };
        let mut client = OKXDataClient::new(*OKX_CLIENT_ID, config).expect("data client");

        client.connect().await.expect("connect");
        assert_eq!(client.tasks.len(), 3);

        client.stop().expect("stop");
        for handle in client.tasks.take_all() {
            tokio::time::timeout(Duration::from_secs(5), handle)
                .await
                .expect("stop must cancel every spawned task")
                .expect("task joins cleanly");
        }

        client.disconnect().await.expect("disconnect");
    }

    #[tokio::test]
    async fn zero_interval_disables_refresh_on_connect() {
        let addr = start_refresh_server(spot_refresh_state()).await;
        let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel();
        replace_data_event_sender(sender);
        let config = OKXDataClientConfig {
            instrument_types: vec![OKXInstrumentType::Spot],
            base_url_http: Some(format!("http://{addr}")),
            base_url_ws_public: Some(format!("ws://{addr}/ws/public")),
            base_url_ws_business: Some(format!("ws://{addr}/ws/business")),
            environment: OKXEnvironment::Live,
            http_timeout_secs: 5,
            max_retries: 0,
            retry_delay_initial_ms: 1,
            retry_delay_max_ms: 1,
            book_stale_check_interval_secs: 0,
            update_instruments_interval_mins: 0,
            ..OKXDataClientConfig::default()
        };
        let mut client = OKXDataClient::new(*OKX_CLIENT_ID, config).expect("data client");

        client.connect().await.expect("connect");
        assert_eq!(
            client.tasks.len(),
            2,
            "only the two stream tasks run when refresh is disabled"
        );

        client.disconnect().await.expect("disconnect");
    }
}
