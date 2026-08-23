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

//! Live market data client for the Betfair adapter.

use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

use ahash::{AHashMap, AHashSet};
use async_trait::async_trait;
use nautilus_common::{
    clients::{DataClient, SocketReconnectRegistration, SocketReconnectRegistry},
    live::{get_runtime, runner::get_data_event_sender},
    messages::{
        DataEvent,
        data::{
            SubscribeBookDeltas, SubscribeInstrumentClose, SubscribeInstrumentStatus,
            SubscribeTrades, UnsubscribeBars, UnsubscribeBookDeltas, UnsubscribeCustomData,
            UnsubscribeInstrument, UnsubscribeInstrumentClose, UnsubscribeInstrumentStatus,
            UnsubscribeInstruments, UnsubscribeQuotes, UnsubscribeTrades,
        },
    },
    providers::InstrumentProvider,
};
use nautilus_core::{
    AtomicMap, Params,
    time::{AtomicTime, get_atomic_clock_realtime},
};
use nautilus_model::{
    data::{CustomData, CustomDataTrait, Data, DataType, OrderBookDeltas, TradeTick},
    identifiers::{ClientId, InstrumentId, TradeId, Venue},
    instruments::{Instrument, InstrumentAny},
    types::{Currency, Money},
};
use nautilus_network::socket::TcpMessageHandler;
use rust_decimal::Decimal;
use tokio::task::JoinHandle;

use crate::{
    common::{
        consts::{BETFAIR_RACE_STREAM_HOST, BETFAIR_VENUE},
        credential::BetfairCredential,
        enums::{MarketDataFilterField, MarketStatus},
        parse::{
            extract_market_id, make_instrument_id, parse_betfair_price, parse_betfair_quantity,
            parse_market_definition, parse_millis_timestamp,
        },
        socket::{DATA_STREAMS_ENDPOINT, SocketControl, SocketStatePublisher},
    },
    config::BetfairDataConfig,
    data_types::{BetfairSequenceCompleted, register_betfair_custom_data},
    http::client::BetfairHttpClient,
    provider::{BetfairInstrumentProvider, NavigationFilter},
    stream::{
        client::{BetfairRaceStreamClient, BetfairStreamClient},
        config::BetfairStreamConfig,
        messages::{MarketDataFilter, StreamMarketFilter, StreamMessage, stream_decode},
        parse::{
            make_trade_tick, parse_betfair_starting_prices, parse_betfair_ticker,
            parse_bsp_book_deltas, parse_cricket_match, parse_instrument_closes,
            parse_instrument_statuses, parse_race_progress, parse_race_runner_data,
            parse_runner_book_deltas,
        },
    },
};

/// Keep-alive interval in seconds (10 hours, matching Python default).
const KEEP_ALIVE_INTERVAL_SECS: u64 = 36_000;

/// Betfair live data client.
#[derive(Debug)]
pub struct BetfairDataClient {
    clock: &'static AtomicTime,
    client_id: ClientId,
    http_client: Arc<BetfairHttpClient>,
    provider: BetfairInstrumentProvider,
    stream_client: Option<Arc<BetfairStreamClient>>,
    socket_registry: SocketReconnectRegistry,
    socket_control: Option<SocketControl>,
    socket_registration: Option<SocketReconnectRegistration>,
    race_stream_client: Option<Arc<BetfairRaceStreamClient>>,
    cricket_stream_client: Option<Arc<BetfairRaceStreamClient>>,
    credential: BetfairCredential,
    stream_config: BetfairStreamConfig,
    config: BetfairDataConfig,
    currency: Currency,
    is_connected: AtomicBool,
    data_sender: tokio::sync::mpsc::UnboundedSender<DataEvent>,
    instruments: Arc<AtomicMap<InstrumentId, InstrumentAny>>,
    subscribed_market_ids: AHashSet<String>,
    keep_alive_handle: Option<JoinHandle<()>>,
    reconnect_handle: Option<JoinHandle<()>>,
    race_fatal_handle: Option<JoinHandle<()>>,
    cricket_fatal_handle: Option<JoinHandle<()>>,
}

/// Wraps a custom data value with its instrument_id in both metadata (for
/// topic routing) and identifier (for catalog partitioning).
pub(crate) fn custom_data_with_instrument(
    value: Arc<dyn CustomDataTrait>,
    instrument_id: InstrumentId,
) -> CustomData {
    let mut metadata = Params::new();
    metadata.insert(
        "instrument_id".to_string(),
        serde_json::Value::String(instrument_id.to_string()),
    );
    let data_type = DataType::new(
        value.type_name(),
        Some(metadata),
        Some(instrument_id.to_string()),
    );
    CustomData::new(value, data_type)
}

impl BetfairDataClient {
    /// Creates a new [`BetfairDataClient`] instance.
    #[must_use]
    #[expect(clippy::too_many_arguments)]
    pub fn new(
        client_id: ClientId,
        http_client: BetfairHttpClient,
        credential: BetfairCredential,
        stream_config: BetfairStreamConfig,
        config: BetfairDataConfig,
        nav_filter: NavigationFilter,
        currency: Currency,
        min_notional: Option<Money>,
    ) -> Self {
        let data_sender = get_data_event_sender();
        let http_client = Arc::new(http_client);
        let socket_registry = SocketReconnectRegistry::default();
        let socket_control = SocketStatePublisher::new(client_id, socket_registry.clone())
            .map(|publisher| publisher.control(DATA_STREAMS_ENDPOINT));
        let provider = BetfairInstrumentProvider::new(
            Arc::clone(&http_client),
            nav_filter,
            currency,
            min_notional,
        );

        Self {
            clock: get_atomic_clock_realtime(),
            client_id,
            http_client,
            provider,
            stream_client: None,
            socket_registry,
            socket_control,
            socket_registration: None,
            race_stream_client: None,
            cricket_stream_client: None,
            credential,
            stream_config,
            config,
            currency,
            is_connected: AtomicBool::new(false),
            data_sender,
            instruments: Arc::new(AtomicMap::new()),
            subscribed_market_ids: AHashSet::new(),
            keep_alive_handle: None,
            reconnect_handle: None,
            race_fatal_handle: None,
            cricket_fatal_handle: None,
        }
    }

    fn create_stream_handler(
        data_sender: tokio::sync::mpsc::UnboundedSender<DataEvent>,
        instruments: Arc<AtomicMap<InstrumentId, InstrumentAny>>,
        currency: Currency,
        min_notional: Option<Money>,
        reconnect_tx: tokio::sync::mpsc::UnboundedSender<()>,
        clock: &'static AtomicTime,
    ) -> TcpMessageHandler {
        // Track cumulative traded volumes per (instrument_id, price) to compute
        // incremental trade sizes. Betfair `trd` fields report totals, not deltas.
        let traded_volumes: Arc<Mutex<AHashMap<(InstrumentId, Decimal), Decimal>>> =
            Arc::new(Mutex::new(AHashMap::new()));
        let has_initial_connection = Arc::new(AtomicBool::new(false));

        Arc::new(move |data: &[u8]| {
            let ts_init = clock.get_time_ns();

            let msg = match stream_decode(data) {
                Ok(msg) => msg,
                Err(e) => {
                    log::warn!("Failed to decode stream message: {e}");
                    return;
                }
            };

            match msg {
                StreamMessage::MarketChange(mcm) => {
                    if mcm.is_heartbeat() {
                        return;
                    }

                    let Some(market_changes) = &mcm.mc else {
                        return;
                    };

                    let ts_event = parse_millis_timestamp(mcm.pt);

                    for mc in market_changes {
                        let is_snapshot = mc.img;
                        let mut market_closed = false;

                        if let Some(def) = &mc.market_definition {
                            // Emit instruments first so downstream consumers (DataEngine,
                            // BacktestExchange) have the instrument cached before any status
                            // or close event references it.
                            match parse_market_definition(
                                &mc.id,
                                def,
                                currency,
                                ts_event,
                                ts_init,
                                min_notional,
                            ) {
                                Ok(new_instruments) => {
                                    instruments.rcu(|m| {
                                        for inst in &new_instruments {
                                            m.insert(inst.id(), inst.clone());
                                        }
                                    });

                                    for inst in new_instruments {
                                        if let Err(e) =
                                            data_sender.send(DataEvent::Instrument(inst))
                                        {
                                            log::warn!("Failed to send instrument: {e}");
                                        }
                                    }
                                }
                                Err(e) => {
                                    log::warn!(
                                        "Failed to parse market definition for {}: {e}",
                                        mc.id
                                    );
                                }
                            }

                            if let Some(status) = &def.status {
                                market_closed = *status == MarketStatus::Closed;

                                for event in
                                    parse_instrument_statuses(&mc.id, def, ts_event, ts_init)
                                {
                                    if let Err(e) =
                                        data_sender.send(DataEvent::InstrumentStatus(event))
                                    {
                                        log::warn!("Failed to send instrument status: {e}");
                                    }
                                }
                            }

                            for sp in parse_betfair_starting_prices(&mc.id, def, ts_event, ts_init)
                            {
                                let instrument_id = sp.instrument_id;
                                let custom =
                                    custom_data_with_instrument(Arc::new(sp), instrument_id);

                                if let Err(e) =
                                    data_sender.send(DataEvent::Data(Data::Custom(custom)))
                                {
                                    log::warn!("Failed to send starting price: {e}");
                                }
                            }

                            for close in parse_instrument_closes(&mc.id, def, ts_event, ts_init) {
                                if let Err(e) =
                                    data_sender.send(DataEvent::Data(Data::InstrumentClose(close)))
                                {
                                    log::warn!("Failed to send instrument close: {e}");
                                }
                            }
                        }

                        // Non-snapshot deltas and BSP deltas are buffered and flushed after
                        // trades/tickers to mirror the Python `market_change_to_updates`
                        // ordering (book deltas first, then BSP). Snapshots go inline.
                        let mut buffered_deltas: Vec<OrderBookDeltas> = Vec::new();
                        let mut buffered_bsp_customs: Vec<CustomData> = Vec::new();

                        if let Some(runner_changes) = &mc.rc {
                            for rc in runner_changes {
                                let handicap = rc.hc.unwrap_or(Decimal::ZERO);
                                let instrument_id = make_instrument_id(&mc.id, rc.id, handicap);

                                match parse_runner_book_deltas(
                                    instrument_id,
                                    rc,
                                    is_snapshot,
                                    mcm.pt,
                                    ts_event,
                                    ts_init,
                                ) {
                                    Ok(Some(deltas)) => {
                                        if is_snapshot {
                                            if let Err(e) = data_sender.send(DataEvent::Data(
                                                Data::Deltas(Box::new(deltas)),
                                            )) {
                                                log::warn!("Failed to send book deltas: {e}");
                                            }
                                        } else {
                                            buffered_deltas.push(deltas);
                                        }
                                    }
                                    Ok(None) => {}
                                    Err(e) => {
                                        log::warn!(
                                            "Failed to parse book deltas for {instrument_id}: {e}"
                                        );
                                    }
                                }

                                if let Some(trades) = &rc.trd {
                                    let mut volumes = traded_volumes.lock().unwrap();

                                    for pv in trades {
                                        if pv.volume == Decimal::ZERO {
                                            continue;
                                        }

                                        let key = (instrument_id, pv.price);
                                        let prev_volume =
                                            volumes.get(&key).copied().unwrap_or(Decimal::ZERO);

                                        if pv.volume <= prev_volume {
                                            volumes.insert(key, pv.volume);
                                            continue;
                                        }

                                        let trade_volume = pv.volume - prev_volume;
                                        volumes.insert(key, pv.volume);

                                        let price = match parse_betfair_price(pv.price) {
                                            Ok(p) => p,
                                            Err(e) => {
                                                log::warn!("Invalid trade price: {e}");
                                                continue;
                                            }
                                        };
                                        let size = match parse_betfair_quantity(trade_volume) {
                                            Ok(q) => q,
                                            Err(e) => {
                                                log::warn!("Invalid trade size: {e}");
                                                continue;
                                            }
                                        };
                                        let trade_id = TradeId::new(format!(
                                            "{}-{}-{}",
                                            mcm.pt, rc.id, pv.price
                                        ));
                                        let tick: TradeTick = make_trade_tick(
                                            instrument_id,
                                            price,
                                            size,
                                            trade_id,
                                            ts_event,
                                            ts_init,
                                        );

                                        if let Err(e) =
                                            data_sender.send(DataEvent::Data(Data::Trade(tick)))
                                        {
                                            log::warn!("Failed to send trade tick: {e}");
                                        }
                                    }
                                }

                                if let Some(ticker) =
                                    parse_betfair_ticker(instrument_id, rc, ts_event, ts_init)
                                {
                                    let custom = custom_data_with_instrument(
                                        Arc::new(ticker),
                                        instrument_id,
                                    );

                                    if let Err(e) =
                                        data_sender.send(DataEvent::Data(Data::Custom(custom)))
                                    {
                                        log::warn!("Failed to send ticker: {e}");
                                    }
                                }

                                for bsp_delta in
                                    parse_bsp_book_deltas(instrument_id, rc, ts_event, ts_init)
                                {
                                    buffered_bsp_customs.push(custom_data_with_instrument(
                                        Arc::new(bsp_delta),
                                        instrument_id,
                                    ));
                                }
                            }
                        }

                        for deltas in buffered_deltas {
                            if let Err(e) =
                                data_sender.send(DataEvent::Data(Data::Deltas(Box::new(deltas))))
                            {
                                log::warn!("Failed to send book deltas: {e}");
                            }
                        }

                        for custom in buffered_bsp_customs {
                            if let Err(e) = data_sender.send(DataEvent::Data(Data::Custom(custom)))
                            {
                                log::warn!("Failed to send BSP book delta: {e}");
                            }
                        }

                        if market_closed {
                            let prefix = format!("{}-", mc.id);

                            if let Ok(mut volumes) = traded_volumes.lock() {
                                volumes.retain(|k, _| !k.0.symbol.as_str().starts_with(&prefix));
                            }
                        }
                    }

                    let completed = BetfairSequenceCompleted::new(ts_event, ts_init);
                    let custom = CustomData::from_arc(Arc::new(completed));
                    if let Err(e) = data_sender.send(DataEvent::Data(Data::Custom(custom))) {
                        log::warn!("Failed to send sequence completed: {e}");
                    }
                }
                StreamMessage::Connection(_) => {
                    if has_initial_connection.swap(true, Ordering::SeqCst) {
                        log::info!("Betfair data stream reconnected");
                        let _ = reconnect_tx.send(());
                    } else {
                        log::debug!("Betfair data stream connected");
                    }
                }
                StreamMessage::Status(status) => {
                    if status.connection_closed {
                        log::warn!(
                            "Betfair stream closed: {:?} - {:?}",
                            status.error_code,
                            status.error_message,
                        );
                    }
                }
                StreamMessage::RaceChange(rcm) => {
                    if let Some(race_changes) = &rcm.rc {
                        let ts_event_fallback = parse_millis_timestamp(rcm.pt);

                        for rc in race_changes {
                            let race_id = rc.id.as_deref().unwrap_or("");
                            let market_id = rc.mid.as_deref().unwrap_or("");

                            if let Some(runners) = &rc.rrc {
                                for rrc in runners {
                                    let ts_event =
                                        rrc.ft.map_or(ts_event_fallback, parse_millis_timestamp);

                                    if let Some(runner) = parse_race_runner_data(
                                        race_id, market_id, rrc, ts_event, ts_init,
                                    ) {
                                        let selection_id = rrc.id.unwrap_or(0);
                                        let mut metadata = Params::new();
                                        metadata.insert(
                                            "selection_id".to_string(),
                                            serde_json::Value::Number(selection_id.into()),
                                        );
                                        let value: Arc<dyn CustomDataTrait> = Arc::new(runner);
                                        let data_type =
                                            DataType::new(value.type_name(), Some(metadata), None);
                                        let custom = CustomData::new(value, data_type);

                                        if let Err(e) =
                                            data_sender.send(DataEvent::Data(Data::Custom(custom)))
                                        {
                                            log::warn!("Failed to send race runner data: {e}");
                                        }
                                    }
                                }
                            }

                            if let Some(rpc) = &rc.rpc {
                                let ts_event =
                                    rpc.ft.map_or(ts_event_fallback, parse_millis_timestamp);

                                let progress =
                                    parse_race_progress(race_id, market_id, rpc, ts_event, ts_init);
                                let mut metadata = Params::new();
                                metadata.insert(
                                    "race_id".to_string(),
                                    serde_json::Value::String(race_id.to_string()),
                                );
                                let value: Arc<dyn CustomDataTrait> = Arc::new(progress);
                                let data_type =
                                    DataType::new(value.type_name(), Some(metadata), None);
                                let custom = CustomData::new(value, data_type);

                                if let Err(e) =
                                    data_sender.send(DataEvent::Data(Data::Custom(custom)))
                                {
                                    log::warn!("Failed to send race progress: {e}");
                                }
                            }
                        }
                    }
                }
                StreamMessage::CricketChange(ccm) => {
                    if let Some(cricket_changes) = &ccm.cc {
                        let ts_event = parse_millis_timestamp(ccm.pt);

                        for cricket_change in cricket_changes {
                            if let Some(cricket) =
                                parse_cricket_match(cricket_change, ts_event, ts_init)
                            {
                                let mut metadata = Params::new();
                                metadata.insert(
                                    "event_id".to_string(),
                                    serde_json::Value::String(cricket.event_id.clone()),
                                );
                                let value: Arc<dyn CustomDataTrait> = Arc::new(cricket);
                                let data_type =
                                    DataType::new(value.type_name(), Some(metadata), None);
                                let custom = CustomData::new(value, data_type);

                                if let Err(e) =
                                    data_sender.send(DataEvent::Data(Data::Custom(custom)))
                                {
                                    log::warn!("Failed to send cricket match: {e}");
                                }
                            }
                        }
                    }
                }
                StreamMessage::OrderChange(_) => {}
            }
        })
    }
}

#[async_trait(?Send)]
impl DataClient for BetfairDataClient {
    fn client_id(&self) -> ClientId {
        self.client_id
    }

    fn venue(&self) -> Option<Venue> {
        Some(*BETFAIR_VENUE)
    }

    fn socket_reconnect_registry(&self) -> Option<&SocketReconnectRegistry> {
        Some(&self.socket_registry)
    }

    fn start(&mut self) -> anyhow::Result<()> {
        log::info!("Starting Betfair data client: {}", self.client_id);
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        log::info!("Stopping Betfair data client: {}", self.client_id);

        if let Some(handle) = self.keep_alive_handle.take() {
            handle.abort();
        }

        if let Some(handle) = self.reconnect_handle.take() {
            handle.abort();
        }

        if let Some(handle) = self.race_fatal_handle.take() {
            handle.abort();
        }

        if let Some(handle) = self.cricket_fatal_handle.take() {
            handle.abort();
        }

        self.socket_registration = None;
        self.is_connected.store(false, Ordering::Relaxed);

        Ok(())
    }

    fn reset(&mut self) -> anyhow::Result<()> {
        log::info!("Resetting Betfair data client: {}", self.client_id);

        if let Some(handle) = self.keep_alive_handle.take() {
            handle.abort();
        }

        if let Some(handle) = self.reconnect_handle.take() {
            handle.abort();
        }

        if let Some(handle) = self.race_fatal_handle.take() {
            handle.abort();
        }

        if let Some(handle) = self.cricket_fatal_handle.take() {
            handle.abort();
        }

        self.socket_registration = None;
        self.is_connected.store(false, Ordering::Relaxed);
        self.stream_client = None;
        self.race_stream_client = None;
        self.cricket_stream_client = None;
        self.provider.store_mut().clear();
        self.subscribed_market_ids.clear();

        self.instruments.store(AHashMap::new());
        Ok(())
    }

    fn dispose(&mut self) -> anyhow::Result<()> {
        log::debug!("Disposing Betfair data client: {}", self.client_id);
        self.stop()
    }

    fn is_connected(&self) -> bool {
        self.is_connected.load(Ordering::SeqCst)
    }

    fn is_disconnected(&self) -> bool {
        !self.is_connected()
    }

    async fn connect(&mut self) -> anyhow::Result<()> {
        if self.is_connected() {
            return Ok(());
        }

        register_betfair_custom_data();

        self.http_client
            .connect()
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))?;

        self.provider.load_all(None).await?;

        let loaded: Vec<InstrumentAny> = self
            .provider
            .store()
            .list_all()
            .into_iter()
            .cloned()
            .collect();

        self.instruments.rcu(|m| {
            for inst in &loaded {
                m.insert(inst.id(), inst.clone());
            }
        });

        for inst in &loaded {
            if let Err(e) = self.data_sender.send(DataEvent::Instrument(inst.clone())) {
                log::warn!("Failed to send instrument: {e}");
            }
        }

        log::debug!("Cached {} instruments for {}", loaded.len(), self.client_id,);

        let session_token = self
            .http_client
            .session_token()
            .await
            .ok_or_else(|| anyhow::anyhow!("No session token after login"))?;

        let (reconnect_tx, mut reconnect_rx) = tokio::sync::mpsc::unbounded_channel();

        let handler = Self::create_stream_handler(
            self.data_sender.clone(),
            Arc::clone(&self.instruments),
            self.currency,
            self.provider.min_notional(),
            reconnect_tx.clone(),
            self.clock,
        );

        let state_sink = self.socket_control.as_ref().map(SocketControl::sink);
        let stream_client = BetfairStreamClient::connect_with_state_sink(
            &self.credential,
            session_token,
            handler,
            self.stream_config.clone(),
            state_sink,
        )
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?;

        let stream_client = Arc::new(stream_client);
        self.socket_registration = self.socket_control.as_ref().map(|control| {
            let reconnect_stream = Arc::clone(&stream_client);
            control.register(move || reconnect_stream.request_reconnect_outcome())
        });
        self.stream_client = Some(stream_client);

        if self.config.subscribe_race_data {
            let race_config = BetfairStreamConfig {
                host: BETFAIR_RACE_STREAM_HOST.to_string(),
                ..self.stream_config.clone()
            };

            let race_session = self
                .http_client
                .session_token()
                .await
                .ok_or_else(|| anyhow::anyhow!("No session token for race stream"))?;

            let race_handler = Self::create_stream_handler(
                self.data_sender.clone(),
                Arc::clone(&self.instruments),
                self.currency,
                self.provider.min_notional(),
                reconnect_tx.clone(),
                self.clock,
            );

            let (race_fatal_tx, mut race_fatal_rx) = tokio::sync::mpsc::unbounded_channel();

            match BetfairRaceStreamClient::connect(
                &self.credential,
                race_session,
                race_handler,
                race_config,
                race_fatal_tx,
            )
            .await
            {
                Ok(client) => {
                    let race_client = Arc::new(client);
                    self.race_stream_client = Some(Arc::clone(&race_client));

                    if let Some(handle) = self.race_fatal_handle.take() {
                        handle.abort();
                    }

                    self.race_fatal_handle = Some(get_runtime().spawn(async move {
                        if race_fatal_rx.recv().await.is_some() {
                            log::error!(
                                "Betfair race stream permanently disabled due to fatal error"
                            );
                            race_client.close().await;
                        }
                    }));

                    log::debug!("Betfair race stream connected");
                }
                Err(e) => {
                    log::warn!("Betfair race stream connect failed: {e}");
                    self.race_stream_client = None;
                }
            }
        }

        if self.config.subscribe_cricket_data {
            let cricket_config = BetfairStreamConfig {
                host: BETFAIR_RACE_STREAM_HOST.to_string(),
                ..self.stream_config.clone()
            };

            let cricket_session = self
                .http_client
                .session_token()
                .await
                .ok_or_else(|| anyhow::anyhow!("No session token for cricket stream"))?;

            let cricket_handler = Self::create_stream_handler(
                self.data_sender.clone(),
                Arc::clone(&self.instruments),
                self.currency,
                self.provider.min_notional(),
                reconnect_tx.clone(),
                self.clock,
            );

            let (cricket_fatal_tx, mut cricket_fatal_rx) = tokio::sync::mpsc::unbounded_channel();

            match BetfairRaceStreamClient::connect_cricket(
                &self.credential,
                cricket_session,
                cricket_handler,
                cricket_config,
                cricket_fatal_tx,
            )
            .await
            {
                Ok(client) => {
                    let cricket_client = Arc::new(client);
                    self.cricket_stream_client = Some(Arc::clone(&cricket_client));

                    if let Some(handle) = self.cricket_fatal_handle.take() {
                        handle.abort();
                    }

                    self.cricket_fatal_handle = Some(get_runtime().spawn(async move {
                        if cricket_fatal_rx.recv().await.is_some() {
                            log::error!(
                                "Betfair cricket stream permanently disabled due to fatal error"
                            );
                            cricket_client.close().await;
                        }
                    }));

                    log::debug!("Betfair cricket stream connected");
                }
                Err(e) => {
                    log::warn!("Betfair cricket stream connect failed: {e}");
                    self.cricket_stream_client = None;
                }
            }
        }

        // Abort any existing keep-alive task before spawning a new one
        if let Some(handle) = self.keep_alive_handle.take() {
            handle.abort();
        }

        // Spawn periodic keep-alive to prevent session expiry
        let keep_alive_client = Arc::clone(&self.http_client);
        let keep_alive_stream = Arc::clone(self.stream_client.as_ref().unwrap());
        let keep_alive_race_stream = self.race_stream_client.as_ref().map(Arc::clone);
        let keep_alive_cricket_stream = self.cricket_stream_client.as_ref().map(Arc::clone);
        let keep_alive_app_key = self.credential.app_key().to_string();

        self.keep_alive_handle = Some(get_runtime().spawn(async move {
            let interval = tokio::time::Duration::from_secs(KEEP_ALIVE_INTERVAL_SECS);
            loop {
                tokio::time::sleep(interval).await;

                let session_replaced = match keep_alive_client.keep_alive_with_token().await {
                    Ok(_) => false,
                    Err(ref e) if e.is_login_failed() => {
                        log::warn!("Betfair session expired, attempting re-login: {e}");

                        match keep_alive_client.reconnect_with_token().await {
                            Ok(_) => true,
                            Err(e) => {
                                log::warn!("Betfair re-login failed: {e}");
                                continue;
                            }
                        }
                    }
                    Err(e) => {
                        log::warn!("Betfair keep-alive failed (transient): {e}");
                        continue;
                    }
                };

                let _ = keep_alive_client
                    .with_session_token(|token| {
                        refresh_stream_sessions(
                            keep_alive_stream.as_ref(),
                            keep_alive_race_stream.as_deref(),
                            keep_alive_cricket_stream.as_deref(),
                            &keep_alive_app_key,
                            token,
                            session_replaced,
                        );
                    })
                    .await;
                log::debug!("Betfair session keep-alive sent");
            }
        }));

        // Spawn reconnect handler to refresh session on stream reconnection
        let reconnect_http = Arc::clone(&self.http_client);
        let reconnect_stream = Arc::clone(self.stream_client.as_ref().unwrap());
        let reconnect_race_stream = self.race_stream_client.as_ref().map(Arc::clone);
        let reconnect_cricket_stream = self.cricket_stream_client.as_ref().map(Arc::clone);
        let reconnect_app_key = self.credential.app_key().to_string();

        self.reconnect_handle = Some(get_runtime().spawn(async move {
            while reconnect_rx.recv().await.is_some() {
                log::info!("Handling data stream reconnection");

                let session_replaced = match reconnect_http.keep_alive_with_token().await {
                    Ok(_) => false,
                    Err(ref e) if e.is_login_failed() => {
                        log::warn!("Session expired on reconnect, attempting re-login: {e}");

                        match reconnect_http.reconnect_with_token().await {
                            Ok(_) => true,
                            Err(e) => {
                                log::warn!("Re-login failed on reconnect: {e}");
                                continue;
                            }
                        }
                    }
                    Err(e) => {
                        log::warn!("Keep-alive failed on reconnect (transient): {e}");
                        continue;
                    }
                };

                let _ = reconnect_http
                    .with_session_token(|token| {
                        refresh_stream_sessions(
                            reconnect_stream.as_ref(),
                            reconnect_race_stream.as_deref(),
                            reconnect_cricket_stream.as_deref(),
                            &reconnect_app_key,
                            token,
                            session_replaced,
                        );
                    })
                    .await;
            }
        }));

        self.is_connected.store(true, Ordering::Release);

        log::info!("Betfair data client connected: {}", self.client_id);
        Ok(())
    }

    async fn disconnect(&mut self) -> anyhow::Result<()> {
        if self.is_disconnected() {
            return Ok(());
        }

        if let Some(handle) = self.keep_alive_handle.take() {
            handle.abort();
        }

        if let Some(handle) = self.reconnect_handle.take() {
            handle.abort();
        }

        if let Some(handle) = self.race_fatal_handle.take() {
            handle.abort();
        }

        if let Some(handle) = self.cricket_fatal_handle.take() {
            handle.abort();
        }

        if let Some(client) = &self.cricket_stream_client {
            client.close().await;
        }
        self.cricket_stream_client = None;

        if let Some(client) = &self.race_stream_client {
            client.close().await;
        }
        self.race_stream_client = None;

        if let Some(client) = &self.stream_client {
            client.close().await;
        }

        self.http_client.disconnect().await;
        self.is_connected.store(false, Ordering::Relaxed);
        self.subscribed_market_ids.clear();
        self.socket_registration = None;

        log::info!("Betfair data client disconnected: {}", self.client_id);
        Ok(())
    }

    fn subscribe_book_deltas(&mut self, cmd: SubscribeBookDeltas) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;
        let market_id = extract_market_id(&instrument_id)?;

        if !self.subscribed_market_ids.insert(market_id.clone()) {
            log::debug!("Book deltas already subscribed for market {market_id}");
            return Ok(());
        }

        let stream_client = Arc::clone(
            self.stream_client
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("Stream client not connected"))?,
        );

        let all_ids: Vec<String> = self.subscribed_market_ids.iter().cloned().collect();

        let market_filter = StreamMarketFilter {
            market_ids: Some(all_ids),
            ..Default::default()
        };

        let data_filter = MarketDataFilter {
            fields: Some(vec![
                MarketDataFilterField::ExAllOffers,
                MarketDataFilterField::ExTraded,
                MarketDataFilterField::ExTradedVol,
                MarketDataFilterField::ExLtp,
                MarketDataFilterField::ExMarketDef,
                MarketDataFilterField::SpTraded,
                MarketDataFilterField::SpProjected,
            ]),
            ladder_levels: None,
        };

        let conflate_ms = self.config.stream_conflate_ms;

        nautilus_common::live::get_runtime().spawn(async move {
            if let Err(e) = stream_client
                .subscribe_markets(market_filter, data_filter, None, conflate_ms)
                .await
            {
                log::warn!("Failed to subscribe to market data: {e}");
            }
        });

        Ok(())
    }

    fn unsubscribe_book_deltas(&mut self, cmd: &UnsubscribeBookDeltas) -> anyhow::Result<()> {
        log::debug!(
            "Skipping unsubscribe book deltas for Betfair: {}",
            cmd.instrument_id
        );
        Ok(())
    }

    fn subscribe_trades(&mut self, cmd: SubscribeTrades) -> anyhow::Result<()> {
        // Trades are included in market subscription via EX_TRADED
        log::debug!(
            "Trade data included in book subscription for {}",
            cmd.instrument_id
        );
        Ok(())
    }

    fn unsubscribe_trades(&mut self, cmd: &UnsubscribeTrades) -> anyhow::Result<()> {
        log::debug!(
            "Skipping unsubscribe trades for Betfair: {}",
            cmd.instrument_id
        );
        Ok(())
    }

    fn subscribe_instrument_status(
        &mut self,
        cmd: SubscribeInstrumentStatus,
    ) -> anyhow::Result<()> {
        // Instrument status is included in market subscription via EX_MARKET_DEF
        log::debug!(
            "Instrument status included in book subscription for {}",
            cmd.instrument_id
        );
        Ok(())
    }

    fn unsubscribe_instrument_status(
        &mut self,
        cmd: &UnsubscribeInstrumentStatus,
    ) -> anyhow::Result<()> {
        log::debug!(
            "Skipping unsubscribe instrument status for Betfair: {}",
            cmd.instrument_id
        );
        Ok(())
    }

    fn subscribe_instrument_close(&mut self, cmd: SubscribeInstrumentClose) -> anyhow::Result<()> {
        // Close transitions arrive via marketDefinition.status="CLOSED" on the
        // existing market subscription; no separate venue subscription exists.
        log::debug!(
            "Instrument close included in book subscription for {}",
            cmd.instrument_id
        );
        Ok(())
    }

    fn unsubscribe_instrument_close(
        &mut self,
        cmd: &UnsubscribeInstrumentClose,
    ) -> anyhow::Result<()> {
        log::debug!(
            "Skipping unsubscribe instrument close for Betfair: {}",
            cmd.instrument_id
        );
        Ok(())
    }

    fn unsubscribe(&mut self, _cmd: &UnsubscribeCustomData) -> anyhow::Result<()> {
        log::debug!("Skipping unsubscribe custom data for Betfair");
        Ok(())
    }

    fn unsubscribe_instrument(&mut self, cmd: &UnsubscribeInstrument) -> anyhow::Result<()> {
        log::debug!(
            "Skipping unsubscribe instrument for Betfair: {}",
            cmd.instrument_id
        );
        Ok(())
    }

    fn unsubscribe_instruments(&mut self, _cmd: &UnsubscribeInstruments) -> anyhow::Result<()> {
        log::debug!("Skipping unsubscribe instruments for Betfair");
        Ok(())
    }

    fn unsubscribe_quotes(&mut self, cmd: &UnsubscribeQuotes) -> anyhow::Result<()> {
        log::debug!(
            "Skipping unsubscribe quotes for Betfair: {}",
            cmd.instrument_id
        );
        Ok(())
    }

    fn unsubscribe_bars(&mut self, cmd: &UnsubscribeBars) -> anyhow::Result<()> {
        log::debug!("Skipping unsubscribe bars for Betfair: {}", cmd.bar_type);
        Ok(())
    }
}

fn refresh_stream_sessions(
    stream: &BetfairStreamClient,
    race_stream: Option<&BetfairRaceStreamClient>,
    cricket_stream: Option<&BetfairRaceStreamClient>,
    app_key: &str,
    token: &str,
    session_replaced: bool,
) {
    stream.update_auth(app_key, token.to_string());

    if let Some(race_stream) = race_stream {
        race_stream.update_auth(app_key, token.to_string());
    }

    if let Some(cricket_stream) = cricket_stream {
        cricket_stream.update_auth(app_key, token.to_string());
    }

    if !session_replaced {
        return;
    }

    let _ = stream.request_reconnect();

    if let Some(race_stream) = race_stream {
        let _ = race_stream.request_reconnect();
    }

    if let Some(cricket_stream) = cricket_stream {
        let _ = cricket_stream.request_reconnect();
    }
}

#[cfg(test)]
mod tests {
    use nautilus_core::UnixNanos;
    use rstest::rstest;

    use super::*;
    use crate::{
        common::testing::load_test_json,
        data_types::{BetfairCricketMatch, BetfairRaceRunnerData, BetfairSequenceCompleted},
    };

    fn stream_handler_at(
        ts_init: UnixNanos,
    ) -> (
        TcpMessageHandler,
        tokio::sync::mpsc::UnboundedReceiver<DataEvent>,
    ) {
        let (data_tx, data_rx) = tokio::sync::mpsc::unbounded_channel();
        let (reconnect_tx, _reconnect_rx) = tokio::sync::mpsc::unbounded_channel();
        let clock = Box::leak(Box::new(AtomicTime::new(false, ts_init)));
        let handler = BetfairDataClient::create_stream_handler(
            data_tx,
            Arc::new(AtomicMap::new()),
            Currency::GBP(),
            None,
            reconnect_tx,
            clock,
        );

        (handler, data_rx)
    }

    fn receive_custom<T: 'static>(
        data_rx: &mut tokio::sync::mpsc::UnboundedReceiver<DataEvent>,
    ) -> Arc<dyn CustomDataTrait> {
        while let Ok(event) = data_rx.try_recv() {
            if let DataEvent::Data(Data::Custom(custom)) = event
                && custom.data.as_any().is::<T>()
            {
                return custom.data;
            }
        }

        panic!("expected {} custom data", std::any::type_name::<T>());
    }

    #[rstest]
    fn test_stream_handler_sets_mcm_init_from_clock() {
        let ts_init = UnixNanos::from(1_800_000_000_000_000_001);

        let (handler, mut data_rx) = stream_handler_at(ts_init);
        let data = load_test_json("stream/mcm_UPDATE.json");

        handler(data.as_bytes());

        let custom = receive_custom::<BetfairSequenceCompleted>(&mut data_rx);
        let completed = custom
            .as_any()
            .downcast_ref::<BetfairSequenceCompleted>()
            .unwrap();

        assert_eq!(
            completed.ts_event,
            UnixNanos::from(1_471_370_160_471_000_000)
        );
        assert_eq!(completed.ts_init, ts_init);
    }

    #[rstest]
    fn test_stream_handler_sets_rcm_init_from_clock() {
        let ts_init = UnixNanos::from(1_800_000_000_000_000_002);

        let (handler, mut data_rx) = stream_handler_at(ts_init);
        let data = load_test_json("stream/rcm_single.json");

        handler(data.as_bytes());

        let custom = receive_custom::<BetfairRaceRunnerData>(&mut data_rx);
        let runner = custom
            .as_any()
            .downcast_ref::<BetfairRaceRunnerData>()
            .unwrap();

        assert_eq!(runner.ts_event, UnixNanos::from(1_518_626_674_000_000_000));
        assert_eq!(runner.ts_init, ts_init);
    }

    #[rstest]
    fn test_stream_handler_uses_rcm_publish_time_without_feed_time() {
        let ts_init = UnixNanos::from(1_800_000_000_000_000_003);

        let (handler, mut data_rx) = stream_handler_at(ts_init);
        let data = load_test_json("stream/rcm_single.json");
        let mut message: serde_json::Value = serde_json::from_str(&data).unwrap();
        message
            .pointer_mut("/rc/0/rrc/0")
            .unwrap()
            .as_object_mut()
            .unwrap()
            .remove("ft");
        let data = message.to_string();

        handler(data.as_bytes());

        let custom = receive_custom::<BetfairRaceRunnerData>(&mut data_rx);
        let runner = custom
            .as_any()
            .downcast_ref::<BetfairRaceRunnerData>()
            .unwrap();

        assert_eq!(runner.ts_event, UnixNanos::from(1_518_626_764_000_000_000));
        assert_eq!(runner.ts_init, ts_init);
    }

    #[rstest]
    fn test_stream_handler_emits_cricket_match_custom_data() {
        let ts_init = UnixNanos::from(1_800_000_000_000_000_004);

        let (handler, mut data_rx) = stream_handler_at(ts_init);
        let data = load_test_json("stream/ccm_single.json");

        handler(data.as_bytes());

        let event = data_rx.try_recv().expect("expected cricket custom data");
        let DataEvent::Data(Data::Custom(custom)) = event else {
            panic!("expected cricket custom data event, was {event:?}");
        };
        let cricket = custom
            .data
            .as_any()
            .downcast_ref::<BetfairCricketMatch>()
            .expect("custom data must be BetfairCricketMatch");
        let metadata = custom.data_type.metadata().expect("event metadata");

        assert_eq!(cricket.event_id, "35741575");
        assert_eq!(cricket.market_id, "1.259334639");
        assert_eq!(cricket.ts_event, UnixNanos::from(1_700_000_000_000_000_000));
        assert_eq!(cricket.ts_init, ts_init);
        assert_eq!(
            metadata.get("event_id"),
            Some(&serde_json::Value::String("35741575".to_string())),
        );
        assert!(
            data_rx.try_recv().is_err(),
            "CCM fixture must emit exactly one event"
        );
    }
}
