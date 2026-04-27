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

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use ahash::{AHashMap, AHashSet};
use anyhow::Context;
use futures_util::{StreamExt, pin_mut};
use nautilus_common::{
    cache::quote::QuoteCache,
    clients::DataClient,
    live::{runner::get_data_event_sender, runtime::get_runtime},
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
            UnsubscribeFundingRates, UnsubscribeIndexPrices, UnsubscribeInstrumentStatus,
            UnsubscribeMarkPrices, UnsubscribeOptionGreeks, UnsubscribeQuotes, UnsubscribeTrades,
        },
    },
};
use nautilus_core::{
    AtomicMap, Params, UnixNanos,
    datetime::datetime_to_unix_nanos,
    time::{AtomicTime, get_atomic_clock_realtime},
};
use nautilus_model::{
    data::{Data, FundingRateUpdate, InstrumentStatus, OrderBookDeltas_API},
    enums::{BookType, GreeksConvention, MarketStatusAction},
    identifiers::{ClientId, InstrumentId, Venue},
    instruments::{Instrument, InstrumentAny},
};
use tokio::{task::JoinHandle, time::Duration};
use tokio_util::sync::CancellationToken;
use ustr::Ustr;

use crate::{
    common::{
        consts::{
            OKX_VENUE, OKX_WS_HEARTBEAT_SECS, resolve_book_depth, resolve_instrument_families,
        },
        enums::{
            OKXBookChannel, OKXContractType, OKXGreeksType, OKXInstrumentStatus, OKXInstrumentType,
            OKXVipLevel,
        },
        parse::{
            extract_inst_family, okx_instrument_type_from_symbol, okx_status_to_market_action,
            parse_base_quote_from_symbol, parse_instrument_any, parse_instrument_id,
            parse_millisecond_timestamp, parse_price, parse_quantity,
        },
    },
    config::OKXDataClientConfig,
    http::client::OKXHttpClient,
    websocket::{
        client::OKXWebSocketClient,
        enums::OKXWsChannel,
        messages::{NautilusWsMessage, OKXBookMsg, OKXOptionSummaryMsg, OKXWsMessage},
        parse::{
            extract_fees_from_cached_instrument, parse_book_msg_vec, parse_index_price_msg_vec,
            parse_option_summary_greeks, parse_ws_message_data,
        },
    },
};

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
    instruments: Arc<AtomicMap<InstrumentId, InstrumentAny>>,
    book_channels: Arc<AtomicMap<InstrumentId, OKXBookChannel>>,
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
                config.base_url_http.clone(),
                config.http_timeout_secs,
                config.max_retries,
                config.retry_delay_initial_ms,
                config.retry_delay_max_ms,
                config.environment,
                config.proxy_url.clone(),
            )?
        } else {
            OKXHttpClient::new(
                config.base_url_http.clone(),
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
        .context("failed to construct OKX public websocket client")?;

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
            .context("failed to construct OKX business websocket client")?;
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
            cancellation_token: CancellationToken::new(),
            tasks: Vec::new(),
            data_sender,
            instruments: Arc::new(AtomicMap::new()),
            book_channels: Arc::new(AtomicMap::new()),
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
        get_runtime().spawn(async move {
            if let Err(e) = fut.await {
                log::error!("{context}: {e:?}");
            }
        });
    }

    #[expect(clippy::too_many_arguments)]
    fn handle_ws_message(
        message: OKXWsMessage,
        data_sender: &tokio::sync::mpsc::UnboundedSender<DataEvent>,
        instruments: &Arc<AtomicMap<InstrumentId, InstrumentAny>>,
        instruments_by_symbol: &mut AHashMap<Ustr, InstrumentAny>,
        quote_cache: &mut QuoteCache,
        funding_cache: &mut AHashMap<Ustr, (Ustr, u64)>,
        index_ticker_map: &Arc<AtomicMap<Ustr, AHashSet<Ustr>>>,
        option_greeks_subs: &Arc<AtomicMap<InstrumentId, AHashSet<OKXGreeksType>>>,
        clock: &AtomicTime,
    ) {
        match message {
            OKXWsMessage::BookData { arg, action, data } => {
                let Some(inst_id) = arg.inst_id else {
                    log::warn!("Book data without inst_id");
                    return;
                };
                let Some(instrument) = instruments_by_symbol.get(&inst_id) else {
                    log::warn!("No cached instrument for book data: {inst_id}");
                    return;
                };
                let ts_init = clock.get_time_ns();

                match parse_book_msg_vec(
                    data,
                    &instrument.id(),
                    instrument.price_precision(),
                    instrument.size_precision(),
                    action,
                    ts_init,
                ) {
                    Ok(data_vec) => {
                        for data in data_vec {
                            Self::send_data(data_sender, data);
                        }
                    }
                    Err(e) => log::error!("Failed to parse book data: {e}"),
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

                            for msg in &msgs {
                                let Some(instrument) = instruments_by_symbol.get(&msg.inst_id)
                                else {
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

                    for sym in &symbols {
                        let Some(instrument) = instruments_by_symbol.get(sym) else {
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

                let Some(instrument) = instruments_by_symbol.get(&inst_id) else {
                    log::warn!("No cached instrument for {channel:?}: {inst_id}");
                    return;
                };
                let instrument_id = instrument.id();
                let price_precision = instrument.price_precision();
                let size_precision = instrument.size_precision();
                let ts_init = clock.get_time_ns();

                if matches!(channel, OKXWsChannel::BboTbt) {
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
                    instruments_by_symbol,
                ) {
                    Ok(Some(ws_msg)) => {
                        dispatch_parsed_data(
                            ws_msg,
                            data_sender,
                            instruments,
                            instruments_by_symbol,
                        );
                    }
                    Ok(None) => {}
                    Err(e) => log::error!("Failed to parse {channel:?} data: {e}"),
                }
            }
            OKXWsMessage::Instruments(okx_instruments) => {
                let ts_init = clock.get_time_ns();

                for okx_inst in okx_instruments {
                    let inst_key = Ustr::from(&okx_inst.inst_id);
                    let (margin_init, margin_maint, maker_fee, taker_fee) =
                        instruments_by_symbol.get(&inst_key).map_or(
                            (None, None, None, None),
                            extract_fees_from_cached_instrument,
                        );
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
                            instruments_by_symbol
                                .insert(inst_any.symbol().inner(), inst_any.clone());
                            upsert_instrument(instruments, inst_any);
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
                                .get(&inst_key)
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
                            log::error!("Failed to parse instrument: {e}");
                            let instrument_id = instruments_by_symbol
                                .get(&inst_key)
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
            | OKXWsMessage::AlgoOrders(_)
            | OKXWsMessage::OrderResponse { .. }
            | OKXWsMessage::Account(_)
            | OKXWsMessage::Positions(_)
            | OKXWsMessage::SendFailed { .. } => {
                log::debug!("Ignoring execution message on data client");
            }
            OKXWsMessage::Error(e) => {
                log::error!("OKX websocket error: {e:?}");
            }
            OKXWsMessage::Reconnected => {
                log::info!("Websocket reconnected");
            }
            OKXWsMessage::Authenticated => {
                log::debug!("Websocket authenticated");
            }
        }
    }
}

fn dispatch_parsed_data(
    msg: NautilusWsMessage,
    data_sender: &tokio::sync::mpsc::UnboundedSender<DataEvent>,
    instruments: &Arc<AtomicMap<InstrumentId, InstrumentAny>>,
    instruments_by_symbol: &mut AHashMap<Ustr, InstrumentAny>,
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
            let data = Data::Deltas(OrderBookDeltas_API::new(deltas));
            if let Err(e) = data_sender.send(DataEvent::Data(data)) {
                log::error!("Failed to emit data event: {e}");
            }
        }
        NautilusWsMessage::FundingRates(updates) => {
            emit_funding_rates(data_sender, updates);
        }
        NautilusWsMessage::Instrument(instrument, status) => {
            instruments_by_symbol.insert(instrument.symbol().inner(), (*instrument).clone());
            upsert_instrument(instruments, *instrument);

            if let Some(status) = status
                && let Err(e) = data_sender.send(DataEvent::InstrumentStatus(status))
            {
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

fn upsert_instrument(
    cache: &Arc<AtomicMap<InstrumentId, InstrumentAny>>,
    instrument: InstrumentAny,
) {
    cache.insert(instrument.id(), instrument);
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
        self.cancellation_token.cancel();
        self.is_connected.store(false, Ordering::Relaxed);
        Ok(())
    }

    fn reset(&mut self) -> anyhow::Result<()> {
        log::debug!("Resetting {id}", id = self.client_id);
        self.is_connected.store(false, Ordering::Relaxed);
        self.cancellation_token = CancellationToken::new();
        self.tasks.clear();
        self.book_channels.store(AHashMap::new());
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
        self.stop()
    }

    async fn connect(&mut self) -> anyhow::Result<()> {
        if self.is_connected() {
            return Ok(());
        }

        // Create fresh token so tasks from a previous connection cycle are not
        // immediately cancelled (the old token may already be in cancelled state)
        self.cancellation_token = CancellationToken::new();

        let instrument_types = if self.config.instrument_types.is_empty() {
            vec![OKXInstrumentType::Spot]
        } else {
            self.config.instrument_types.clone()
        };

        let mut all_instruments = Vec::new();

        for inst_type in &instrument_types {
            let Some(families) =
                resolve_instrument_families(&self.config.instrument_families, *inst_type)
            else {
                continue;
            };

            if families.is_empty() {
                let (mut fetched, _inst_id_codes) = self
                    .http_client
                    .request_instruments(*inst_type, None)
                    .await
                    .with_context(|| {
                        format!("failed to request OKX instruments for {inst_type:?}")
                    })?;

                fetched.retain(|instrument| contract_filter_with_config(&self.config, instrument));
                self.http_client.cache_instruments(&fetched);

                self.instruments.rcu(|m| {
                    for instrument in &fetched {
                        m.insert(instrument.id(), instrument.clone());
                    }
                });

                all_instruments.extend(fetched);
            } else {
                for family in &families {
                    let (mut fetched, _inst_id_codes) = self
                        .http_client
                        .request_instruments(*inst_type, Some(family.clone()))
                        .await
                        .with_context(|| {
                            format!(
                                "failed to request OKX instruments for {inst_type:?} family {family}"
                            )
                        })?;

                    fetched
                        .retain(|instrument| contract_filter_with_config(&self.config, instrument));
                    self.http_client.cache_instruments(&fetched);

                    self.instruments.rcu(|m| {
                        for instrument in &fetched {
                            m.insert(instrument.id(), instrument.clone());
                        }
                    });

                    all_instruments.extend(fetched);
                }
            }
        }

        for instrument in all_instruments {
            if let Err(e) = self.data_sender.send(DataEvent::Instrument(instrument)) {
                log::warn!("Failed to send instrument: {e}");
            }
        }

        if let Some(ref mut ws) = self.ws_public {
            // Cache instruments to websocket before connecting so handler has them
            let instruments: Vec<_> = self.instruments.load().values().cloned().collect();
            ws.cache_instruments(&instruments);

            ws.connect()
                .await
                .context("failed to connect OKX public websocket")?;
            ws.wait_until_active(10.0)
                .await
                .context("public websocket did not become active")?;

            let stream = ws.stream();
            let sender = self.data_sender.clone();
            let insts = self.instruments.clone();
            let idx_map = self.index_ticker_map.clone();
            let greeks_subs = self.option_greeks_subs.clone();
            let cancel = self.cancellation_token.clone();
            let clock = self.clock;

            let handle = get_runtime().spawn(async move {
                let mut instruments_by_symbol: AHashMap<Ustr, InstrumentAny> = insts
                    .load()
                    .values()
                    .map(|i| (i.symbol().inner(), i.clone()))
                    .collect();
                let mut quote_cache = QuoteCache::new();
                let mut funding_cache: AHashMap<Ustr, (Ustr, u64)> = AHashMap::new();
                pin_mut!(stream);

                loop {
                    tokio::select! {
                        Some(message) = stream.next() => {
                            Self::handle_ws_message(
                                message,
                                &sender,
                                &insts,
                                &mut instruments_by_symbol,
                                &mut quote_cache,
                                &mut funding_cache,
                                &idx_map,
                                &greeks_subs,
                                clock,
                            );
                        }
                        () = cancel.cancelled() => {
                            log::debug!("Public websocket stream task cancelled");
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
            let instruments: Vec<_> = self.instruments.load().values().cloned().collect();
            ws.cache_instruments(&instruments);

            ws.connect()
                .await
                .context("failed to connect OKX business websocket")?;
            ws.wait_until_active(10.0)
                .await
                .context("business websocket did not become active")?;

            let stream = ws.stream();
            let sender = self.data_sender.clone();
            let insts = self.instruments.clone();
            let idx_map = self.index_ticker_map.clone();
            let greeks_subs = self.option_greeks_subs.clone();
            let cancel = self.cancellation_token.clone();
            let clock = self.clock;

            let handle = get_runtime().spawn(async move {
                let mut instruments_by_symbol: AHashMap<Ustr, InstrumentAny> = insts
                    .load()
                    .values()
                    .map(|i| (i.symbol().inner(), i.clone()))
                    .collect();
                let mut quote_cache = QuoteCache::new();
                let mut funding_cache: AHashMap<Ustr, (Ustr, u64)> = AHashMap::new();
                pin_mut!(stream);

                loop {
                    tokio::select! {
                        Some(message) = stream.next() => {
                            Self::handle_ws_message(
                                message,
                                &sender,
                                &insts,
                                &mut instruments_by_symbol,
                                &mut quote_cache,
                                &mut funding_cache,
                                &idx_map,
                                &greeks_subs,
                                clock,
                            );
                        }
                        () = cancel.cancelled() => {
                            log::debug!("Business websocket stream task cancelled");
                            break;
                        }
                    }
                }
            });
            self.tasks.push(handle);
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

        if let Some(ref mut ws) = self.ws_public {
            let _ = ws.close().await;
        }

        if let Some(ref mut ws) = self.ws_business {
            let _ = ws.close().await;
        }

        let handles: Vec<_> = self.tasks.drain(..).collect();

        for handle in handles {
            if let Err(e) = handle.await {
                log::error!("Error joining websocket task: {e}");
            }
        }

        self.book_channels.store(AHashMap::new());
        self.option_greeks_subs
            .store(AHashMap::<InstrumentId, AHashSet<OKXGreeksType>>::new());
        self.option_summary_family_subs
            .lock()
            .expect("option_summary_family_subs mutex poisoned")
            .clear();
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

        let raw_depth = cmd.depth.map_or(0, |d| d.get());
        let depth = resolve_book_depth(raw_depth);
        if depth != raw_depth {
            log::info!("Clamped book depth {raw_depth} to {depth} (OKX supports 50 or 400)");
        }

        let vip = self.vip_level().unwrap_or(OKXVipLevel::Vip0);
        let channel = match depth {
            50 => {
                if vip < OKXVipLevel::Vip4 {
                    log::info!(
                        "VIP level {vip} insufficient for 50-depth channel, falling back to default"
                    );
                    OKXBookChannel::Book
                } else {
                    OKXBookChannel::Books50L2Tbt
                }
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
                book_channels.insert(instrument_id, channel);
                Ok(())
            },
            "order book delta subscription",
        );

        Ok(())
    }

    fn subscribe_quotes(&mut self, cmd: SubscribeQuotes) -> anyhow::Result<()> {
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

    fn subscribe_trades(&mut self, cmd: SubscribeTrades) -> anyhow::Result<()> {
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

    fn unsubscribe_book_deltas(&mut self, cmd: &UnsubscribeBookDeltas) -> anyhow::Result<()> {
        let ws = self.public_ws()?.clone();
        let instrument_id = cmd.instrument_id;
        let channel = self.book_channels.get_cloned(&instrument_id);
        self.book_channels.remove(&instrument_id);

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
        let instrument_types = if self.config.instrument_types.is_empty() {
            vec![OKXInstrumentType::Spot]
        } else {
            self.config.instrument_types.clone()
        };
        let contract_types = self.config.contract_types.clone();
        let instrument_families = self.config.instrument_families.clone();

        get_runtime().spawn(async move {
            let mut all_instruments = Vec::new();

            for inst_type in instrument_types {
                let Some(families) =
                    resolve_instrument_families(&instrument_families, inst_type)
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

                                upsert_instrument(&instruments_cache, instrument.clone());
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

                                    upsert_instrument(&instruments_cache, instrument.clone());
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
        let instrument_types = if self.config.instrument_types.is_empty() {
            vec![OKXInstrumentType::Spot]
        } else {
            self.config.instrument_types.clone()
        };
        let contract_types = self.config.contract_types.clone();

        get_runtime().spawn(async move {
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
                        log::error!(
                            "Instrument {instrument_id} type {inst_type:?} not in configured types {instrument_types:?}"
                        );
                        return;
                    }

                    if !contract_filter_with_config_types(contract_types.as_ref(), &instrument) {
                        log::error!(
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
                        log::error!("Failed to send instrument response: {e}");
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

        get_runtime().spawn(async move {
            match http
                .request_book_snapshot(instrument_id, depth)
                .await
                .context("failed to request book snapshot from OKX")
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

        get_runtime().spawn(async move {
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

        get_runtime().spawn(async move {
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

        get_runtime().spawn(async move {
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

        get_runtime().spawn(async move {
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

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use serde_json::json;

    use super::*;

    fn both() -> AHashSet<OKXGreeksType> {
        [OKXGreeksType::Bs, OKXGreeksType::Pa].into_iter().collect()
    }

    fn only(greeks_type: OKXGreeksType) -> AHashSet<OKXGreeksType> {
        [greeks_type].into_iter().collect()
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
}
