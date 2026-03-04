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
    Arc, Mutex, RwLock,
    atomic::{AtomicBool, Ordering},
};

use ahash::{AHashMap, AHashSet};
use async_trait::async_trait;
use nautilus_common::{
    clients::DataClient,
    live::{get_runtime, runner::get_data_event_sender},
    messages::{
        DataEvent,
        data::{
            SubscribeBookDeltas, SubscribeInstrumentStatus, SubscribeTrades, UnsubscribeBookDeltas,
            UnsubscribeInstrumentStatus, UnsubscribeTrades,
        },
    },
    providers::InstrumentProvider,
};
use nautilus_model::{
    data::{Data, OrderBookDeltas_API, TradeTick},
    identifiers::{ClientId, InstrumentId, TradeId, Venue},
    instruments::{Instrument, InstrumentAny},
    types::{Currency, Money, Price, Quantity},
};
use nautilus_network::socket::TcpMessageHandler;
use rust_decimal::Decimal;
use tokio::task::JoinHandle;

use crate::{
    common::{
        consts::{BETFAIR_PRICE_PRECISION, BETFAIR_QUANTITY_PRECISION, BETFAIR_VENUE},
        credential::BetfairCredential,
        enums::{MarketDataFilterField, MarketStatus},
        parse::{
            extract_market_id, make_instrument_id, parse_market_definition, parse_millis_timestamp,
        },
    },
    http::client::BetfairHttpClient,
    provider::{BetfairInstrumentProvider, NavigationFilter},
    stream::{
        client::BetfairStreamClient,
        config::BetfairStreamConfig,
        messages::{MarketDataFilter, StreamMarketFilter, StreamMessage, stream_decode},
        parse::{make_trade_tick, parse_instrument_status, parse_runner_book_deltas},
    },
};

/// Keep-alive interval in seconds (10 hours, matching Python default).
const KEEP_ALIVE_INTERVAL_SECS: u64 = 36_000;

/// Betfair live data client.
#[derive(Debug)]
pub struct BetfairDataClient {
    client_id: ClientId,
    http_client: Arc<BetfairHttpClient>,
    provider: BetfairInstrumentProvider,
    stream_client: Option<Arc<BetfairStreamClient>>,
    credential: BetfairCredential,
    stream_config: BetfairStreamConfig,
    currency: Currency,
    is_connected: AtomicBool,
    data_sender: tokio::sync::mpsc::UnboundedSender<DataEvent>,
    instruments: Arc<RwLock<AHashMap<InstrumentId, InstrumentAny>>>,
    subscribed_market_ids: AHashSet<String>,
    keep_alive_handle: Option<JoinHandle<()>>,
}

impl BetfairDataClient {
    /// Creates a new [`BetfairDataClient`] instance.
    #[must_use]
    pub fn new(
        client_id: ClientId,
        http_client: BetfairHttpClient,
        credential: BetfairCredential,
        stream_config: BetfairStreamConfig,
        nav_filter: NavigationFilter,
        currency: Currency,
        min_notional: Option<Money>,
    ) -> Self {
        let data_sender = get_data_event_sender();
        let http_client = Arc::new(http_client);
        let provider = BetfairInstrumentProvider::new(
            Arc::clone(&http_client),
            nav_filter,
            currency,
            min_notional,
        );

        Self {
            client_id,
            http_client,
            provider,
            stream_client: None,
            credential,
            stream_config,
            currency,
            is_connected: AtomicBool::new(false),
            data_sender,
            instruments: Arc::new(RwLock::new(AHashMap::new())),
            subscribed_market_ids: AHashSet::new(),
            keep_alive_handle: None,
        }
    }

    fn create_stream_handler(
        data_sender: tokio::sync::mpsc::UnboundedSender<DataEvent>,
        instruments: Arc<RwLock<AHashMap<InstrumentId, InstrumentAny>>>,
        currency: Currency,
        min_notional: Option<Money>,
    ) -> TcpMessageHandler {
        // Track cumulative traded volumes per (instrument_id, price) to compute
        // incremental trade sizes. Betfair `trd` fields report totals, not deltas.
        let traded_volumes: Arc<Mutex<AHashMap<(InstrumentId, Decimal), Decimal>>> =
            Arc::new(Mutex::new(AHashMap::new()));

        Arc::new(move |data: &[u8]| {
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
                    let ts_init = ts_event;

                    for mc in market_changes {
                        let is_snapshot = mc.img;
                        let mut market_closed = false;

                        if let Some(def) = &mc.market_definition {
                            if let Some(status) = &def.status {
                                market_closed = *status == MarketStatus::Closed;
                                let in_play = def.in_play.unwrap_or(false);
                                let guard = instruments.read().ok();
                                if let Some(guard) = &guard {
                                    for inst in guard.values() {
                                        let prefix = format!("{}-", mc.id);
                                        if inst.id().symbol.as_str().starts_with(&prefix) {
                                            let event = parse_instrument_status(
                                                inst.id(),
                                                *status,
                                                in_play,
                                                ts_event,
                                                ts_init,
                                            );

                                            if let Err(e) =
                                                data_sender.send(DataEvent::InstrumentStatus(event))
                                            {
                                                log::warn!("Failed to send instrument status: {e}");
                                            }
                                        }
                                    }
                                }
                            }

                            match parse_market_definition(
                                &mc.id,
                                def,
                                currency,
                                ts_init,
                                min_notional,
                            ) {
                                Ok(new_instruments) => {
                                    if let Ok(mut guard) = instruments.write() {
                                        for inst in &new_instruments {
                                            guard.insert(inst.id(), inst.clone());
                                        }
                                    }
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
                        }

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
                                        if let Err(e) = data_sender.send(DataEvent::Data(
                                            Data::Deltas(OrderBookDeltas_API::new(deltas)),
                                        )) {
                                            log::warn!("Failed to send book deltas: {e}");
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
                                            continue;
                                        }

                                        let trade_volume = pv.volume - prev_volume;
                                        volumes.insert(key, pv.volume);

                                        let price = match Price::from_decimal_dp(
                                            pv.price,
                                            BETFAIR_PRICE_PRECISION,
                                        ) {
                                            Ok(p) => p,
                                            Err(e) => {
                                                log::warn!("Invalid trade price: {e}");
                                                continue;
                                            }
                                        };
                                        let size = match Quantity::from_decimal_dp(
                                            trade_volume,
                                            BETFAIR_QUANTITY_PRECISION,
                                        ) {
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
                            }
                        }

                        if market_closed {
                            let prefix = format!("{}-", mc.id);

                            if let Ok(mut volumes) = traded_volumes.lock() {
                                volumes.retain(|k, _| !k.0.symbol.as_str().starts_with(&prefix));
                            }
                        }
                    }
                }
                StreamMessage::Connection(_) => {
                    log::info!("Betfair stream connected");
                }
                StreamMessage::Status(status) => {
                    if status.connection_closed {
                        log::error!(
                            "Betfair stream closed: {:?} - {:?}",
                            status.error_code,
                            status.error_message,
                        );
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

    fn start(&mut self) -> anyhow::Result<()> {
        log::info!("Starting Betfair data client: {}", self.client_id);
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        log::info!("Stopping Betfair data client: {}", self.client_id);

        if let Some(handle) = self.keep_alive_handle.take() {
            handle.abort();
        }
        self.is_connected.store(false, Ordering::Relaxed);
        Ok(())
    }

    fn reset(&mut self) -> anyhow::Result<()> {
        log::info!("Resetting Betfair data client: {}", self.client_id);

        if let Some(handle) = self.keep_alive_handle.take() {
            handle.abort();
        }
        self.is_connected.store(false, Ordering::Relaxed);
        self.stream_client = None;
        self.provider.store_mut().clear();

        if let Ok(mut guard) = self.instruments.write() {
            guard.clear();
        }
        Ok(())
    }

    fn dispose(&mut self) -> anyhow::Result<()> {
        log::info!("Disposing Betfair data client: {}", self.client_id);
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

        {
            let mut guard = self
                .instruments
                .write()
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            for inst in &loaded {
                guard.insert(inst.id(), inst.clone());
            }
        }

        for inst in &loaded {
            if let Err(e) = self.data_sender.send(DataEvent::Instrument(inst.clone())) {
                log::warn!("Failed to send instrument: {e}");
            }
        }

        log::info!("Cached {} instruments for {}", loaded.len(), self.client_id,);

        let session_token = self
            .http_client
            .session_token()
            .await
            .ok_or_else(|| anyhow::anyhow!("No session token after login"))?;

        let handler = Self::create_stream_handler(
            self.data_sender.clone(),
            Arc::clone(&self.instruments),
            self.currency,
            self.provider.min_notional(),
        );

        let stream_client = BetfairStreamClient::connect(
            &self.credential,
            session_token,
            handler,
            self.stream_config.clone(),
        )
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?;

        self.stream_client = Some(Arc::new(stream_client));

        // Abort any existing keep-alive task before spawning a new one
        if let Some(handle) = self.keep_alive_handle.take() {
            handle.abort();
        }

        // Spawn periodic keep-alive to prevent session expiry
        let keep_alive_client = Arc::clone(&self.http_client);
        self.keep_alive_handle = Some(get_runtime().spawn(async move {
            let interval = tokio::time::Duration::from_secs(KEEP_ALIVE_INTERVAL_SECS);
            loop {
                tokio::time::sleep(interval).await;

                if let Err(e) = keep_alive_client.keep_alive().await {
                    log::warn!("Betfair keep-alive failed: {e}");
                } else {
                    log::debug!("Betfair session keep-alive sent");
                }
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

        if let Some(client) = &self.stream_client {
            client.close().await;
        }

        self.http_client.disconnect().await;
        self.is_connected.store(false, Ordering::Relaxed);

        log::info!("Betfair data client disconnected: {}", self.client_id);
        Ok(())
    }

    fn subscribe_book_deltas(&mut self, cmd: &SubscribeBookDeltas) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;
        let market_id = extract_market_id(&instrument_id)?;

        let stream_client = Arc::clone(
            self.stream_client
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("Stream client not connected"))?,
        );

        // Accumulate market IDs so each subscription includes all prior markets
        self.subscribed_market_ids.insert(market_id);
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
                MarketDataFilterField::ExMarketDef,
            ]),
            ladder_levels: None,
        };

        nautilus_common::live::get_runtime().spawn(async move {
            if let Err(e) = stream_client
                .subscribe_markets(market_filter, data_filter, None, None)
                .await
            {
                log::error!("Failed to subscribe to market data: {e}");
            }
        });

        log::info!("Subscribing to book deltas for {instrument_id}");
        Ok(())
    }

    fn unsubscribe_book_deltas(&mut self, cmd: &UnsubscribeBookDeltas) -> anyhow::Result<()> {
        log::info!(
            "Unsubscribe book deltas not supported for Betfair: {}",
            cmd.instrument_id
        );
        Ok(())
    }

    fn subscribe_trades(&mut self, cmd: &SubscribeTrades) -> anyhow::Result<()> {
        // Trades are included in market subscription via EX_TRADED
        log::info!(
            "Trade data included in book subscription for {}",
            cmd.instrument_id
        );
        Ok(())
    }

    fn unsubscribe_trades(&mut self, cmd: &UnsubscribeTrades) -> anyhow::Result<()> {
        log::info!(
            "Unsubscribe trades not supported for Betfair: {}",
            cmd.instrument_id
        );
        Ok(())
    }

    fn subscribe_instrument_status(
        &mut self,
        cmd: &SubscribeInstrumentStatus,
    ) -> anyhow::Result<()> {
        // Instrument status is included in market subscription via EX_MARKET_DEF
        log::info!(
            "Instrument status included in book subscription for {}",
            cmd.instrument_id
        );
        Ok(())
    }

    fn unsubscribe_instrument_status(
        &mut self,
        cmd: &UnsubscribeInstrumentStatus,
    ) -> anyhow::Result<()> {
        log::info!(
            "Unsubscribe instrument status not supported for Betfair: {}",
            cmd.instrument_id
        );
        Ok(())
    }
}
