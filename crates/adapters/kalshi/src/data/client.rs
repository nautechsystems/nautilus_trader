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

//! Data client for the Kalshi exchange.
//!
//! The exchange publishes market data over REST, so this client polls. One task refreshes the top
//! of book and the book snapshot of every subscribed market, and a second task fetches trades after
//! the newest timestamp it has already seen.
//!
//! Polling cannot observe every intermediate book state, so a book update is emitted as a snapshot
//! that clears and rebuilds the book rather than as an incremental delta. Between two polls a book
//! can print through levels that a consumer never sees, which is why the emitted snapshot is
//! authoritative and the sequence only has to increase.

use std::{collections::HashMap, sync::Arc, time::Duration};

use ahash::AHashSet;
use nautilus_common::{
    clients::DataClient,
    live::runner::get_data_event_sender,
    messages::{
        DataEvent,
        data::{
            SubscribeBookDeltas, SubscribeInstruments, SubscribeQuotes, SubscribeTrades,
            UnsubscribeBookDeltas, UnsubscribeQuotes, UnsubscribeTrades,
        },
    },
    providers::InstrumentProvider,
};
use nautilus_core::{UnixNanos, time::get_atomic_clock_realtime};
use nautilus_live::task::TaskGroup;
use nautilus_model::{
    data::{Data as NautilusData, OrderBookDeltas},
    enums::BookType,
    identifiers::{ClientId, InstrumentId, OutcomeGroupId, Venue},
};
use parking_lot::Mutex;

use crate::{
    common::consts::KALSHI_VENUE,
    data::subscriptions::KalshiSubscriptions,
    http::{
        client::KalshiHttpClient,
        models::{KalshiEvent, KalshiMarket},
        parse::{
            create_instrument_close_from_market, create_market_status,
            create_order_book_deltas_from_market, create_quote_tick_from_market,
            create_resolution_from_event, create_trade_tick_from_trade, instrument_id_for,
            price_precision_and_increment,
        },
    },
    providers::KalshiInstrumentProvider,
};

/// The default interval between market polls.
pub const DEFAULT_UPDATE_INTERVAL: Duration = Duration::from_secs(2);

/// The interval between trade polls.
pub const TRADE_POLL_INTERVAL: Duration = Duration::from_secs(5);

/// The most trades a single poll fetches per instrument.
pub const TRADE_POLL_LIMIT: u32 = 100;

/// State the client carries about markets the exchange has settled.
#[derive(Debug, Default)]
pub struct SettlementState {
    /// The resolution version emitted per outcome group.
    pub versions: HashMap<OutcomeGroupId, u32>,
    /// Instruments whose close has already been emitted.
    pub closed: AHashSet<InstrumentId>,
}

/// A data client for the Kalshi exchange.
#[derive(Debug)]
pub struct KalshiDataClient {
    client_id: ClientId,
    venue: Venue,
    http_client: Arc<KalshiHttpClient>,
    provider: KalshiInstrumentProvider,
    subscriptions: Arc<Mutex<KalshiSubscriptions>>,
    settlement_state: Arc<Mutex<SettlementState>>,
    last_trade_ts: Arc<Mutex<HashMap<InstrumentId, i64>>>,
    data_sender: tokio::sync::mpsc::UnboundedSender<DataEvent>,
    tasks: TaskGroup,
    update_interval: Duration,
    is_connected: bool,
}

impl KalshiDataClient {
    /// Creates a new [`KalshiDataClient`].
    #[must_use]
    pub fn new(
        client_id: ClientId,
        http_client: KalshiHttpClient,
        provider: KalshiInstrumentProvider,
        update_interval: Option<Duration>,
    ) -> Self {
        Self {
            client_id,
            venue: Venue::from(KALSHI_VENUE),
            http_client: Arc::new(http_client),
            provider,
            subscriptions: Arc::new(Mutex::new(KalshiSubscriptions::new())),
            settlement_state: Arc::new(Mutex::new(SettlementState::default())),
            last_trade_ts: Arc::new(Mutex::new(HashMap::new())),
            data_sender: get_data_event_sender(),
            tasks: TaskGroup::new(),
            update_interval: update_interval.unwrap_or(DEFAULT_UPDATE_INTERVAL),
            is_connected: false,
        }
    }

    /// Returns the interval between market polls.
    #[must_use]
    pub const fn update_interval(&self) -> Duration {
        self.update_interval
    }

    /// Returns the number of instruments subscribed to any market data.
    #[must_use]
    pub fn subscribed_instruments(&self) -> usize {
        self.subscriptions.lock().len()
    }

    /// Returns the subscriptions held by the client.
    #[must_use]
    pub fn subscriptions(&self) -> Arc<Mutex<KalshiSubscriptions>> {
        Arc::clone(&self.subscriptions)
    }

    /// Loads the provider's instruments and publishes every one of them to the engine.
    ///
    /// # Errors
    ///
    /// Returns an error if the instruments cannot be loaded.
    pub async fn refresh_instruments(&mut self) -> anyhow::Result<usize> {
        self.provider.load_all(None).await?;
        let instruments: Vec<_> = self
            .provider
            .store()
            .get_all()
            .into_iter()
            .map(|(_, instrument)| instrument.clone())
            .collect();
        let count = instruments.len();

        for instrument in instruments {
            if let Err(e) = self.data_sender.send(DataEvent::Instrument(instrument)) {
                log::error!("Failed to publish a Kalshi instrument: {e}");
            }
        }

        Ok(count)
    }

    /// Polls the subscribed markets once, emitting quotes, book snapshots, and settlements.
    ///
    /// This is the same work the client's poll task performs on an interval; it is public so a
    /// caller (or a test) can drive one poll.
    ///
    /// # Errors
    ///
    /// Returns an error if a market cannot be fetched or a settlement cannot be parsed.
    pub async fn poll_markets(&mut self) -> anyhow::Result<usize> {
        poll_markets(
            &self.http_client,
            &self.subscriptions,
            &self.settlement_state,
            &self.data_sender,
        )
        .await
    }

    /// Polls the subscribed markets' trades once.
    ///
    /// # Errors
    ///
    /// Returns an error if a market's trades cannot be fetched.
    pub async fn poll_trades(&mut self) -> anyhow::Result<usize> {
        poll_trades(
            &self.http_client,
            &self.subscriptions,
            &self.last_trade_ts,
            &self.data_sender,
        )
        .await
    }

    /// Spawns the market poll task.
    fn spawn_market_poll(&self) {
        let interval = self.update_interval;
        let http_client = Arc::clone(&self.http_client);
        let subscriptions = Arc::clone(&self.subscriptions);
        let settlement_state = Arc::clone(&self.settlement_state);
        let data_sender = self.data_sender.clone();

        if let Err(e) = self.tasks.spawn(async move {
            loop {
                tokio::time::sleep(interval).await;

                if let Err(e) = poll_markets(
                    &http_client,
                    &subscriptions,
                    &settlement_state,
                    &data_sender,
                )
                .await
                {
                    log::warn!("Kalshi market poll failed: {e}");
                }
            }
        }) {
            log::error!("Failed to start the Kalshi market poll: {e}");
        }
    }

    /// Spawns the trade poll task.
    fn spawn_trade_poll(&self) {
        let http_client = Arc::clone(&self.http_client);
        let subscriptions = Arc::clone(&self.subscriptions);
        let last_trade_ts = Arc::clone(&self.last_trade_ts);
        let data_sender = self.data_sender.clone();

        if let Err(e) = self.tasks.spawn(async move {
            loop {
                tokio::time::sleep(TRADE_POLL_INTERVAL).await;

                if let Err(e) =
                    poll_trades(&http_client, &subscriptions, &last_trade_ts, &data_sender).await
                {
                    log::warn!("Kalshi trade poll failed: {e}");
                }
            }
        }) {
            log::error!("Failed to start the Kalshi trade poll: {e}");
        }
    }
}

/// Emits a data event, logging rather than failing when the receiver is gone.
fn emit(data_sender: &tokio::sync::mpsc::UnboundedSender<DataEvent>, data: NautilusData) {
    if let Err(e) = data_sender.send(DataEvent::Data(data)) {
        log::error!("Failed to emit Kalshi market data: {e}");
    }
}

/// Polls the subscribed markets once.
async fn poll_markets(
    http_client: &KalshiHttpClient,
    subscriptions: &Mutex<KalshiSubscriptions>,
    settlement_state: &Mutex<SettlementState>,
    data_sender: &tokio::sync::mpsc::UnboundedSender<DataEvent>,
) -> anyhow::Result<usize> {
    let instrument_ids: Vec<InstrumentId> = subscriptions.lock().instrument_ids().collect();
    let mut polled = 0;

    for instrument_id in instrument_ids {
        let Some(ticker) = KalshiInstrumentProvider::ticker_for(&instrument_id) else {
            continue;
        };
        let ticker = ticker.to_string();
        let market = http_client.get_market(&ticker).await?;
        let ts_init = get_atomic_clock_realtime().get_time_ns();
        let (wants_quotes, wants_book, depth) = {
            let subscriptions = subscriptions.lock();

            (
                subscriptions.is_quote_subscribed(&instrument_id),
                subscriptions.is_book_subscribed(&instrument_id),
                subscriptions.book_depth(&instrument_id),
            )
        };

        if wants_quotes && market.status.is_tradable() {
            match create_quote_tick_from_market(&market, ts_init, ts_init) {
                Ok(quote) => emit(data_sender, NautilusData::Quote(quote)),
                Err(e) => log::debug!("Skipping Kalshi quote for {ticker}: {e}"),
            }
        }

        if wants_book {
            let orderbook = http_client.get_market_orderbook(&ticker, depth).await?;
            let sequence = subscriptions.lock().next_sequence(&instrument_id);

            match create_order_book_deltas_from_market(&market, &orderbook, sequence, ts_init) {
                Ok(deltas) => {
                    let data = NautilusData::BookDeltas(Box::new(OrderBookDeltas::new(
                        instrument_id,
                        deltas,
                    )));
                    emit(data_sender, data);
                }
                Err(e) => log::warn!("Failed to build a Kalshi book for {ticker}: {e}"),
            }
        }

        poll_settlement(http_client, settlement_state, data_sender, &market, ts_init).await?;
        polled += 1;
    }

    Ok(polled)
}

/// Emits the close and resolution of a market the exchange has settled, once.
async fn poll_settlement(
    http_client: &KalshiHttpClient,
    settlement_state: &Mutex<SettlementState>,
    data_sender: &tokio::sync::mpsc::UnboundedSender<DataEvent>,
    market: &KalshiMarket,
    ts_init: UnixNanos,
) -> anyhow::Result<()> {
    if !(market.status.is_final() && market.result.is_binary_outcome()) {
        return Ok(());
    }

    let instrument_id = instrument_id_for(&market.ticker);

    if !settlement_state.lock().closed.insert(instrument_id) {
        return Ok(());
    }

    let status = create_market_status(instrument_id, market.status, ts_init, ts_init);
    if let Err(e) = data_sender.send(DataEvent::InstrumentStatus(status)) {
        log::error!("Failed to emit a Kalshi instrument status: {e}");
    }
    emit(
        data_sender,
        NautilusData::InstrumentClose(create_instrument_close_from_market(market, ts_init)?),
    );

    // A resolution is declared once per event version, and only from the exchange's own settlement
    // record for every leg of that event.
    let event = http_client.get_event_for_market(market).await?;
    let markets = if event.markets.is_empty() {
        vec![market.clone()]
    } else {
        event.markets.clone()
    };
    let group_id = outcome_group_id(&event)?;
    let version = {
        let state = settlement_state.lock();

        state
            .versions
            .get(&group_id)
            .copied()
            .map_or(1, |version| version + 1)
    };

    if let Some(resolution) = create_resolution_from_event(&event, &markets, version, ts_init)? {
        log::info!(
            "Kalshi event {} resolved with state '{}'",
            event.event_ticker,
            resolution.outcome.state()
        );
        settlement_state.lock().versions.insert(group_id, version);
        emit(data_sender, NautilusData::MarketResolution(resolution));
    }

    Ok(())
}

/// Polls the trades of every subscribed market that are newer than the last trade seen.
async fn poll_trades(
    http_client: &KalshiHttpClient,
    subscriptions: &Mutex<KalshiSubscriptions>,
    last_trade_ts: &Mutex<HashMap<InstrumentId, i64>>,
    data_sender: &tokio::sync::mpsc::UnboundedSender<DataEvent>,
) -> anyhow::Result<usize> {
    let instrument_ids = subscriptions.lock().trade_instrument_ids();
    let mut emitted = 0;

    for instrument_id in instrument_ids {
        let Some(ticker) = KalshiInstrumentProvider::ticker_for(&instrument_id) else {
            continue;
        };
        let ticker = ticker.to_string();
        let min_ts = last_trade_ts.lock().get(&instrument_id).copied();
        let response = http_client
            .get_trades(Some(&ticker), min_ts, None, Some(TRADE_POLL_LIMIT), None)
            .await?;

        if response.trades.is_empty() {
            continue;
        }

        let ts_init = get_atomic_clock_realtime().get_time_ns();
        let market = http_client.get_market(&ticker).await?;
        let (price_precision, _) = price_precision_and_increment(&market)?;
        let mut newest_ts = min_ts;

        for trade in &response.trades {
            match create_trade_tick_from_trade(trade, price_precision, ts_init) {
                Ok(tick) => {
                    let ts_event = tick.ts_event.as_u64().cast_signed();
                    newest_ts = Some(newest_ts.map_or(ts_event, |current| current.max(ts_event)));
                    emit(data_sender, NautilusData::Trade(tick));
                    emitted += 1;
                }
                Err(e) => log::debug!("Skipping Kalshi trade {}: {e}", trade.trade_id),
            }
        }

        if let Some(newest_ts) = newest_ts {
            last_trade_ts.lock().insert(instrument_id, newest_ts);
        }
    }

    Ok(emitted)
}

/// Returns the outcome group an event resolves.
fn outcome_group_id(event: &KalshiEvent) -> anyhow::Result<OutcomeGroupId> {
    OutcomeGroupId::from_parts(Venue::from(KALSHI_VENUE), &event.event_ticker).map_err(|e| {
        anyhow::anyhow!(
            "Invalid outcome group id for Kalshi event {}: {e}",
            event.event_ticker
        )
    })
}

#[async_trait::async_trait(?Send)]
impl DataClient for KalshiDataClient {
    fn client_id(&self) -> ClientId {
        self.client_id
    }

    fn venue(&self) -> Option<Venue> {
        Some(self.venue)
    }

    fn start(&mut self) -> anyhow::Result<()> {
        log::debug!("Kalshi data client {} started", self.client_id);

        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        self.tasks.abort();
        self.is_connected = false;
        log::debug!("Kalshi data client {} stopped", self.client_id);

        Ok(())
    }

    fn reset(&mut self) -> anyhow::Result<()> {
        self.last_trade_ts.lock().clear();
        self.settlement_state.lock().closed.clear();
        self.subscriptions.lock().clear();

        Ok(())
    }

    fn dispose(&mut self) -> anyhow::Result<()> {
        self.stop()
    }

    fn is_connected(&self) -> bool {
        self.is_connected
    }

    fn is_disconnected(&self) -> bool {
        !self.is_connected
    }

    async fn connect(&mut self) -> anyhow::Result<()> {
        anyhow::ensure!(
            !self.is_connected,
            "Kalshi data client {} is already connected",
            self.client_id
        );

        self.refresh_instruments().await?;
        self.spawn_market_poll();
        self.spawn_trade_poll();
        self.is_connected = true;
        log::info!(
            "Kalshi data client {} connected to {}",
            self.client_id,
            self.http_client.base_url()
        );

        Ok(())
    }

    async fn disconnect(&mut self) -> anyhow::Result<()> {
        self.tasks.abort();
        self.is_connected = false;
        log::info!("Kalshi data client {} disconnected", self.client_id);

        Ok(())
    }

    fn subscribe_instruments(&mut self, _cmd: SubscribeInstruments) -> anyhow::Result<()> {
        log::debug!(
            "Kalshi instruments are loaded from the configured events and published on refresh"
        );

        Ok(())
    }

    fn subscribe_quotes(&mut self, cmd: SubscribeQuotes) -> anyhow::Result<()> {
        self.subscriptions
            .lock()
            .subscribe_quotes(cmd.instrument_id);

        Ok(())
    }

    fn subscribe_trades(&mut self, cmd: SubscribeTrades) -> anyhow::Result<()> {
        self.subscriptions
            .lock()
            .subscribe_trades(cmd.instrument_id);

        Ok(())
    }

    fn subscribe_book_deltas(&mut self, cmd: SubscribeBookDeltas) -> anyhow::Result<()> {
        anyhow::ensure!(
            cmd.book_type == BookType::L2_MBP,
            "Kalshi publishes bid and ask levels only, so only L2_MBP books are supported, was {:?}",
            cmd.book_type
        );
        let depth = cmd
            .depth
            .map(|depth| u32::try_from(depth.get()).unwrap_or(u32::MAX));
        self.subscriptions
            .lock()
            .subscribe_book_deltas(cmd.instrument_id, depth);

        Ok(())
    }

    fn unsubscribe_quotes(&mut self, cmd: &UnsubscribeQuotes) -> anyhow::Result<()> {
        self.subscriptions
            .lock()
            .unsubscribe_quotes(cmd.instrument_id);

        Ok(())
    }

    fn unsubscribe_trades(&mut self, cmd: &UnsubscribeTrades) -> anyhow::Result<()> {
        self.subscriptions
            .lock()
            .unsubscribe_trades(cmd.instrument_id);

        Ok(())
    }

    fn unsubscribe_book_deltas(&mut self, cmd: &UnsubscribeBookDeltas) -> anyhow::Result<()> {
        self.subscriptions
            .lock()
            .unsubscribe_book_deltas(cmd.instrument_id);

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroUsize;

    use nautilus_core::uuid::UUID4;
    use rstest::rstest;

    use super::*;
    use crate::{common::credential::KalshiCredential, http::auth::KalshiAuth};

    fn subscribe_quotes(instrument_id: InstrumentId) -> SubscribeQuotes {
        SubscribeQuotes::new(
            instrument_id,
            None,
            Some(Venue::from(KALSHI_VENUE)),
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        )
    }

    fn subscribe_book_deltas(
        instrument_id: InstrumentId,
        book_type: BookType,
        depth: Option<usize>,
    ) -> SubscribeBookDeltas {
        SubscribeBookDeltas::new(
            instrument_id,
            book_type,
            None,
            Some(Venue::from(KALSHI_VENUE)),
            UUID4::new(),
            UnixNanos::default(),
            depth.and_then(NonZeroUsize::new),
            false,
            None,
            None,
        )
    }

    fn client() -> KalshiDataClient {
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        nautilus_common::live::runner::replace_data_event_sender(tx);

        // A port nothing listens on, so a request fails immediately rather than reaching the
        // exchange or blocking on DNS.
        let base_url = Some("http://127.0.0.1:9/trade-api/v2".to_string());
        let http_client = KalshiHttpClient::new(
            base_url,
            Some(5),
            None,
            Some(KalshiAuth::new(KalshiCredential::new(
                "key".to_string(),
                "not a pem".to_string(),
            ))),
        )
        .unwrap();
        let provider = KalshiInstrumentProvider::new(
            http_client.clone(),
            vec!["KXHIGHNY-25JAN01".to_string()],
            None,
        );

        KalshiDataClient::new(
            ClientId::from("KALSHI-DATA"),
            http_client,
            provider,
            Some(Duration::from_millis(50)),
        )
    }

    #[rstest]
    fn test_client_reports_its_identity_venue_and_interval() {
        let client = client();

        assert_eq!(client.client_id(), ClientId::from("KALSHI-DATA"));
        assert_eq!(client.venue(), Some(Venue::from("KALSHI")));
        assert_eq!(client.update_interval(), Duration::from_millis(50));
        assert!(client.is_disconnected());
    }

    #[rstest]
    fn test_subscriptions_are_tracked_per_data_type() {
        let mut client = client();
        let quote_instrument = InstrumentId::from("KXHIGHNY-25JAN01-T50.KALSHI");

        client
            .subscribe_quotes(subscribe_quotes(quote_instrument))
            .unwrap();
        client
            .subscribe_book_deltas(subscribe_book_deltas(
                quote_instrument,
                BookType::L2_MBP,
                Some(25),
            ))
            .unwrap();

        // Read the subscription state through a guard that is released before the client is asked
        // again, because the registry mutex is not reentrant.
        let (quotes, book, trades, depth) = {
            let subscriptions = client.subscriptions();
            let subscriptions = subscriptions.lock();

            (
                subscriptions.is_quote_subscribed(&quote_instrument),
                subscriptions.is_book_subscribed(&quote_instrument),
                subscriptions.is_trade_subscribed(&quote_instrument),
                subscriptions.book_depth(&quote_instrument),
            )
        };

        assert!(quotes);
        assert!(book);
        assert!(!trades);
        assert_eq!(depth, Some(25));
        assert_eq!(client.subscribed_instruments(), 1);
    }

    #[rstest]
    fn test_book_subscription_rejects_other_book_types() {
        let mut client = client();
        let cmd = subscribe_book_deltas(
            InstrumentId::from("KXHIGHNY-25JAN01-T50.KALSHI"),
            BookType::L1_MBP,
            None,
        );

        let error = client.subscribe_book_deltas(cmd).unwrap_err();

        assert!(error.to_string().contains("L2_MBP"), "{error}");
    }

    #[rstest]
    fn test_reset_clears_subscriptions_and_settlement_state() {
        let mut client = client();
        let instrument_id = InstrumentId::from("KXHIGHNY-25JAN01-T50.KALSHI");

        client
            .subscribe_quotes(subscribe_quotes(instrument_id))
            .unwrap();
        client.settlement_state.lock().closed.insert(instrument_id);

        client.reset().unwrap();

        assert_eq!(client.subscribed_instruments(), 0);
        assert!(client.settlement_state.lock().closed.is_empty());
    }

    #[rstest]
    fn test_stop_clears_the_connected_flag() {
        let mut client = client();

        client.stop().unwrap();

        assert!(client.is_disconnected());
    }

    #[rstest]
    fn test_settlement_state_versions_start_at_one_and_advance() {
        let mut state = SettlementState::default();
        let group_id =
            OutcomeGroupId::from_parts(Venue::from("KALSHI"), "KXHIGHNY-25JAN01").unwrap();

        assert_eq!(state.versions.get(&group_id).copied().unwrap_or(1), 1);

        state.versions.insert(group_id.clone(), 1);

        assert_eq!(
            state.versions.get(&group_id).copied().map_or(1, |v| v + 1),
            2
        );
    }

    #[tokio::test]
    async fn test_connect_reports_a_failure_and_stays_disconnected() {
        let mut client = client();

        // The configured base URL is not reachable from a unit test, so loading instruments fails
        // and the client must not claim to be connected.
        assert!(client.connect().await.is_err());
        assert!(client.is_disconnected());
    }
}
