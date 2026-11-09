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

use std::{
    collections::{BTreeMap, HashMap},
    sync::{Arc, Mutex},
    time::Duration,
};

use anyhow::Context;
use nautilus_common::{
    live::{dst::time, sender::EventSender},
    messages::DataEvent,
};
use nautilus_core::UnixNanos;
use nautilus_live::{
    book::{
        recovery::BookRecovery,
        snapshot::SnapshotGate,
        sync::{BookPhase, BookSync},
    },
    task::TaskSpawner,
};
use nautilus_model::{
    data::{BookOrder, Data, OrderBookDelta, OrderBookDeltas},
    enums::{BookAction, OrderSide, RecordFlag},
    identifiers::InstrumentId,
    instruments::BinaryOption,
    types::Quantity,
};
use rust_decimal::Decimal;
use serde::Deserialize;
use tokio_util::sync::CancellationToken;

use crate::{
    data::parse,
    http::models::{FixedPoint, Orderbook, OutcomeSide},
    websocket::{
        client::{KalshiWebSocketClient, KalshiWsEvent},
        error::KalshiWsError,
        messages::{KalshiChannel, KalshiDataMessage, KalshiSubscription, KalshiWsMessage},
    },
};

/// Applies order-book protocol events for the markets a data client has subscribed to.
#[derive(Debug, Clone)]
pub struct BookSyncTracker {
    state: Arc<Mutex<State>>,
    subscription: KalshiSubscription,
    snapshot_timeout: Duration,
}

#[allow(
    clippy::missing_panics_doc,
    reason = "book state mutex poisoning is not expected"
)]
impl BookSyncTracker {
    /// Creates a tracker for the selected markets.
    ///
    /// `snapshot_timeout` bounds how long an initial book may wait before recovery. Zero starts that
    /// wait immediately.
    #[must_use]
    pub fn new(instruments: &[InstrumentId], snapshot_timeout: Duration) -> Self {
        let mut market_tickers: Vec<_> =
            instruments.iter().map(|id| id.symbol.to_string()).collect();
        market_tickers.sort();

        Self {
            state: Arc::default(),
            snapshot_timeout,
            subscription: KalshiSubscription {
                channel: KalshiChannel::OrderbookDelta,
                market_tickers,
            },
        }
    }

    /// Adds one local subscription reference.
    ///
    /// The first reference records WebSocket intent when `ws_client` is present. Later references
    /// only increase demand.
    ///
    /// # Errors
    ///
    /// Returns an error if the WebSocket client rejects the subscription or the reference count
    /// overflows.
    pub fn subscribe(
        &self,
        id: InstrumentId,
        ws_client: Option<&KalshiWebSocketClient>,
    ) -> anyhow::Result<()> {
        let mut state = self.state.lock().expect("book state lock is not poisoned");
        let first = state.books.is_empty();
        if first && let Some(ws_client) = ws_client {
            ws_client.subscribe(self.subscription.clone())?;
        }

        let initial = state.sid.is_none();

        let book = state.books.entry(id).or_insert_with(|| {
            let mut book = Book::new(time::Instant::now());
            book.initial = initial;
            book
        });

        book.demand = book
            .demand
            .checked_add(1)
            .context("book subscription reference count overflow")?;
        Ok(())
    }

    pub(crate) fn unsubscribe(
        &self,
        id: InstrumentId,
        ws_client: Option<&KalshiWebSocketClient>,
    ) -> anyhow::Result<()> {
        let mut state = self.state.lock().expect("book state lock is not poisoned");
        if let Some(book) = state.books.get_mut(&id) {
            book.demand -= 1;
            if book.demand == 0 {
                state.books.remove(&id);
            }
        }

        if state.books.is_empty()
            && let Some(ws_client) = ws_client
        {
            ws_client.unsubscribe(&self.subscription)?;
        }

        Ok(())
    }

    /// Restores book subscription intent and marks the session running.
    ///
    /// # Errors
    ///
    /// Returns an error if the WebSocket client rejects the retained subscription.
    pub fn replay(&self, ws_client: &KalshiWebSocketClient) -> anyhow::Result<()> {
        let mut state = self.state.lock().expect("book state lock is not poisoned");
        if !state.books.is_empty() {
            ws_client.subscribe(self.subscription.clone())?;
        }

        state.running = true;
        Ok(())
    }

    pub(crate) fn stop(&self, reset: bool) {
        let mut state = self.state.lock().expect("book state lock is not poisoned");
        state.running = false;
        state.generation = None;
        state.sid = None;
        if reset {
            state.books.clear();
        }

        for book in state.books.values_mut() {
            let demand = book.demand;
            *book = Book::new(time::Instant::now());
            book.demand = demand;
        }
    }

    /// Applies one book protocol event and sends any resulting deltas.
    ///
    /// Ticker and trade frames are ignored. Events are ignored until [`Self::replay`] marks the
    /// tracker running, and an older generation is ignored. A newer generation resets stream state
    /// before the event is applied. Invalid snapshots and deltas gap the affected book and do not
    /// emit a batch.
    ///
    /// # Errors
    ///
    /// Returns an error if the data receiver is closed.
    pub fn event(
        &self,
        event: &KalshiWsEvent,
        instruments: &HashMap<InstrumentId, BinaryOption>,
        sender: &EventSender<DataEvent>,
        now: UnixNanos,
    ) -> anyhow::Result<()> {
        if matches!(
            event,
            KalshiWsEvent::Message {
                message: KalshiWsMessage::Ticker(_) | KalshiWsMessage::Trade(_),
                ..
            }
        ) {
            return Ok(());
        }

        let generation = event.generation();
        let mut state = self.state.lock().expect("book state lock is not poisoned");
        if !state.advance_generation(generation) {
            return Ok(());
        }

        match event {
            KalshiWsEvent::Message {
                message: KalshiWsMessage::Subscribed { msg, .. },
                ..
            } if msg.channel == KalshiChannel::OrderbookDelta => {
                state.subscribed(msg.sid, self.snapshot_timeout);
            }
            KalshiWsEvent::Message {
                message: KalshiWsMessage::Unsubscribed { sid, .. },
                ..
            } if state.sid == Some(*sid) => {
                state.reset_stream();
            }
            KalshiWsEvent::BookSequenceGap { sid, .. } => {
                state.gap(*sid);
            }
            KalshiWsEvent::BookSnapshotRequested {
                sid, id, attempt, ..
            } => {
                for book in state.books.values_mut() {
                    if let Some(current) = &mut book.attempt
                        && current.token == *attempt
                        && !current.cancel.is_cancelled()
                    {
                        current.correlation = Some((generation, *sid, *id));
                        current.gate.open();
                    }
                }
            }
            KalshiWsEvent::Message {
                message: KalshiWsMessage::OrderbookSnapshot(data),
                ..
            } => {
                self.snapshot(&mut state, data, instruments, sender, generation, now)?;
            }
            KalshiWsEvent::Message {
                message: KalshiWsMessage::OrderbookDelta(data),
                ..
            } => {
                self.delta(&mut state, data, instruments, sender, now)?;
            }
            KalshiWsEvent::Message {
                message:
                    KalshiWsMessage::Error {
                        id: Some(id),
                        sid,
                        msg,
                    },
                ..
            } => {
                state.reject_snapshot(generation, *id, *sid, msg.code);
            }
            KalshiWsEvent::Error { message, .. } => {
                for book in state.books.values_mut() {
                    book.sync.reject(KalshiWsError::Client(message.clone()));
                    book.gap();
                    book.initial = false;
                }
            }
            _ => {}
        }

        Ok(())
    }

    fn snapshot(
        &self,
        state: &mut State,
        data: &KalshiDataMessage,
        instruments: &HashMap<InstrumentId, BinaryOption>,
        sender: &EventSender<DataEvent>,
        generation: u64,
        now: UnixNanos,
    ) -> anyhow::Result<()> {
        if state.sid != Some(data.sid) {
            return Ok(());
        }

        let Some((id, book)) = selected(state, data) else {
            return Ok(());
        };

        let initial = book.initial && data.id.is_none();

        let replacement = book.attempt.as_ref().is_some_and(|attempt| {
            !attempt.cancel.is_cancelled()
                && data
                    .id
                    .is_some_and(|id| attempt.correlation == Some((generation, data.sid, id)))
        });

        if !initial && !replacement {
            return Ok(());
        }

        let Some(instrument) = instruments.get(&id) else {
            return Ok(());
        };

        let parsed = (|| {
            let seq = data.seq.context("book snapshot sequence missing")?;
            let payload: Snapshot = serde_json::from_value(data.msg.clone())?;
            let levels = Levels::from_snapshot(
                &payload.yes_dollars_fp,
                &payload.no_dollars_fp,
                false,
                instrument,
            )?;
            let event_time = event_time(data, now)?;
            let batch = levels.snapshot(instrument, seq, event_time, now)?;
            anyhow::Ok((seq, levels, batch))
        })();

        match parsed {
            Ok((seq, levels, batch)) if book.sync.accept_snapshot(seq, time::Instant::now()) => {
                sender
                    .send(DataEvent::Data(Data::BookDeltas(batch.into())))
                    .map_err(|_| anyhow::anyhow!("book event receiver closed"))?;
                book.levels = levels;
                book.initial = false;
                book.gapped = false;
                book.attempt = None;
            }
            Ok(_) => {}
            Err(e) => {
                log::warn!("Invalid Kalshi book snapshot: {e}");
                book.gap();
                book.initial = false;
            }
        }

        Ok(())
    }

    fn delta(
        &self,
        state: &mut State,
        data: &KalshiDataMessage,
        instruments: &HashMap<InstrumentId, BinaryOption>,
        sender: &EventSender<DataEvent>,
        now: UnixNanos,
    ) -> anyhow::Result<()> {
        if state.sid != Some(data.sid) {
            return Ok(());
        }

        let Some((id, book)) = selected(state, data) else {
            return Ok(());
        };

        if !matches!(book.sync.phase(), BookPhase::Synced(_)) {
            book.gap();
            return Ok(());
        }

        let Some(instrument) = instruments.get(&id) else {
            return Ok(());
        };

        let parsed = (|| {
            let seq = data.seq.context("book delta sequence missing")?;
            let payload: Delta = serde_json::from_value(data.msg.clone())?;

            let event_time = match payload.ts_ms {
                Some(value) => UnixNanos::from(
                    value
                        .checked_mul(1_000_000)
                        .context("book timestamp overflow")?,
                ),
                None => event_time(data, now)?,
            };

            let batch = book
                .levels
                .delta(&payload, instrument, seq, event_time, now)?;
            anyhow::Ok((seq, batch))
        })();

        match parsed {
            Ok((seq, batch)) => {
                book.sync.advance(seq, time::Instant::now());
                sender
                    .send(DataEvent::Data(Data::BookDeltas(batch.into())))
                    .map_err(|_| anyhow::anyhow!("book event receiver closed"))?;
            }
            Err(e) => {
                log::warn!("Invalid Kalshi book delta: {e}");
                book.gap();
                book.initial = false;
            }
        }

        Ok(())
    }

    pub(crate) fn monitor(&self, ws_client: &KalshiWebSocketClient, tasks: &TaskSpawner) {
        let mut state = self.state.lock().expect("book state lock is not poisoned");
        if !state.running {
            return;
        }

        let confirmed = state.sid.is_some();
        for (id, book) in &mut state.books {
            if !confirmed && book.initial {
                continue;
            }

            let expired = book.sync.take_expired(time::Instant::now());
            if !expired
                && (matches!(book.sync.phase(), BookPhase::Synced(_))
                    || book.waits_for_snapshot(self.snapshot_timeout))
            {
                continue;
            }

            let Some(recovery) = book.sync.claim() else {
                continue;
            };

            if expired {
                log::warn!("Kalshi book snapshot deadline expired for {id}");
            }

            book.initial = false;
            let tracker = self.clone();
            let ws_client = ws_client.clone();
            let id = *id;
            let running = Arc::clone(&recovery);
            let shutdown = tasks.cancellation_token();
            let snapshot_timeout = self.snapshot_timeout;

            if let Err(e) = tasks.spawn(async move {
                let run = running.run(
                    id,
                    snapshot_timeout,
                    |cancel, gate| {
                        let tracker = tracker.clone();
                        let ws_client = ws_client.clone();
                        let owner = Arc::clone(&running);
                        async move {
                            let token = tracker.attempt(id, cancel.clone(), gate, &owner)?;
                            ws_client
                                .request_book_snapshot(
                                    &tracker.subscription,
                                    id.symbol.to_string(),
                                    token,
                                    cancel,
                                )
                                .await
                        }
                    },
                    |_| true,
                    KalshiWsError::Client,
                    || KalshiWsError::Client("book snapshot timed out".into()),
                );

                tokio::select! {
                    biased;
                    () = shutdown.cancelled() => {}
                    () = run => {}
                }
                running.cancellation.cancel();
            }) {
                recovery.cancellation.cancel();
                log::warn!("Kalshi book recovery task refused: {e}");
            }
        }
    }

    fn attempt(
        &self,
        id: InstrumentId,
        cancel: CancellationToken,
        gate: SnapshotGate,
        owner: &Arc<BookRecovery<KalshiWsError>>,
    ) -> Result<u64, KalshiWsError> {
        let mut state = self.state.lock().expect("book state lock is not poisoned");
        state.next_attempt = state
            .next_attempt
            .checked_add(1)
            .ok_or_else(|| KalshiWsError::Client("book attempt IDs exhausted".into()))?;
        let token = state.next_attempt;
        if !state.running {
            return Err(KalshiWsError::Client("book session stopped".into()));
        }

        let book = state
            .books
            .get_mut(&id)
            .ok_or_else(|| KalshiWsError::Client("book subscription retired".into()))?;

        if cancel.is_cancelled() || !book.sync.is_current(owner) {
            return Err(KalshiWsError::Client("book recovery owner retired".into()));
        }

        book.attempt = Some(Attempt {
            token,
            cancel,
            gate,
            correlation: None,
        });

        Ok(token)
    }
}

#[derive(Debug, Default)]
struct State {
    running: bool,
    generation: Option<u64>,
    sid: Option<u64>,
    next_attempt: u64,
    books: BTreeMap<InstrumentId, Book>,
}

impl State {
    fn advance_generation(&mut self, generation: u64) -> bool {
        if !self.running || self.generation.is_some_and(|current| generation < current) {
            return false;
        }

        if self.generation.is_some_and(|current| generation > current) {
            self.reset_stream();
        }

        self.generation = Some(generation);
        true
    }

    fn subscribed(&mut self, sid: u64, snapshot_timeout: Duration) {
        self.sid = Some(sid);

        for book in self.books.values_mut() {
            book.initial = true;
            book.gapped = false;

            if snapshot_timeout.is_zero() {
                let gate = SnapshotGate::default();
                gate.open();
                book.sync.expect_snapshot(gate);
            } else {
                book.sync
                    .arm_deadline(time::Instant::now() + snapshot_timeout);
            }
        }
    }

    fn reset_stream(&mut self) {
        self.sid = None;

        for book in self.books.values_mut() {
            book.sync.reset_on_reconnect();
            book.attempt = None;
            book.initial = true;
            book.gapped = false;
            book.levels = Levels::default();
        }
    }

    fn reject_snapshot(&mut self, generation: u64, id: u64, sid: Option<u64>, code: u32) {
        for book in self.books.values_mut() {
            if book.attempt.as_ref().is_some_and(|attempt| {
                !attempt.cancel.is_cancelled()
                    && attempt
                        .correlation
                        .is_some_and(|(current, subscription, request)| {
                            current == generation
                                && request == id
                                && sid.is_none_or(|sid| sid == subscription)
                        })
            }) {
                log::warn!("Kalshi book snapshot request {id} rejected with code {code}");
                book.sync.reject(KalshiWsError::Client(format!(
                    "book snapshot rejected with code {code}"
                )));
                book.gap();
                book.initial = false;
            }
        }
    }

    fn gap(&mut self, sid: u64) {
        if self.sid != Some(sid) {
            return;
        }

        for book in self.books.values_mut() {
            book.gap();
            book.initial = false;
        }
    }
}

#[derive(Debug)]
struct Book {
    demand: usize,
    sync: BookSync<KalshiWsError, u64>,
    levels: Levels,
    initial: bool,
    gapped: bool,
    attempt: Option<Attempt>,
}

impl Book {
    fn new(now: time::Instant) -> Self {
        Self {
            demand: 0,
            sync: BookSync::new(now),
            levels: Levels::default(),
            initial: true,
            gapped: false,
            attempt: None,
        }
    }

    fn gap(&mut self) {
        self.gapped = true;
        self.sync.gap();
    }

    fn waits_for_snapshot(&self, snapshot_timeout: Duration) -> bool {
        self.sync.has_pending_snapshot() && (!snapshot_timeout.is_zero() || !self.gapped)
    }
}

#[derive(Debug)]
struct Attempt {
    token: u64,
    cancel: CancellationToken,
    gate: SnapshotGate,
    correlation: Option<(u64, u64, u64)>,
}

fn selected<'a>(
    state: &'a mut State,
    data: &KalshiDataMessage,
) -> Option<(InstrumentId, &'a mut Book)> {
    let Some(ticker) = data
        .msg
        .get("market_ticker")
        .and_then(serde_json::Value::as_str)
    else {
        log::warn!("Kalshi book frame has no market identity");
        state.gap(data.sid);
        return None;
    };

    state
        .books
        .iter_mut()
        .find(|(id, _)| id.symbol.as_str() == ticker)
        .map(|(id, book)| (*id, book))
}

fn event_time(data: &KalshiDataMessage, now: UnixNanos) -> anyhow::Result<UnixNanos> {
    data.sending_ts_ms.map_or(Ok(now), |value| {
        value
            .checked_mul(1_000_000)
            .map(UnixNanos::from)
            .context("book timestamp overflow")
    })
}

#[derive(Debug, Default)]
pub(crate) struct Levels {
    bids: BTreeMap<Decimal, Decimal>,
    asks: BTreeMap<Decimal, Decimal>,
}

impl Levels {
    pub(crate) fn rest(
        book: &Orderbook,
        instrument: &BinaryOption,
        now: UnixNanos,
    ) -> anyhow::Result<OrderBookDeltas> {
        let levels = Self::from_snapshot(&book.yes_dollars, &book.no_dollars, true, instrument)?;
        levels.snapshot(instrument, 0, now, now)
    }

    fn from_snapshot(
        yes: &[[FixedPoint; 2]],
        no: &[[FixedPoint; 2]],
        complement_no: bool,
        instrument: &BinaryOption,
    ) -> anyhow::Result<Self> {
        let mut levels = Self::default();

        for (side, values) in [(OrderSide::Buy, yes), (OrderSide::Sell, no)] {
            let target = if side == OrderSide::Buy {
                &mut levels.bids
            } else {
                &mut levels.asks
            };

            for [price, qty] in values {
                let price = if side == OrderSide::Sell && complement_no {
                    Decimal::ONE
                        .checked_sub(price.0)
                        .context("book price overflow")?
                } else {
                    price.0
                };

                parse::price(FixedPoint(price), instrument.price_precision)?;
                parse::size(*qty, instrument.size_precision)?;
                anyhow::ensure!(
                    target.insert(price, qty.0).is_none(),
                    "duplicate book price level"
                );
            }
        }

        levels.integrity()?;
        Ok(levels)
    }

    fn snapshot(
        &self,
        instrument: &BinaryOption,
        seq: u64,
        event_time: UnixNanos,
        now: UnixNanos,
    ) -> anyhow::Result<OrderBookDeltas> {
        let mut deltas = vec![OrderBookDelta::clear(instrument.id, seq, event_time, now)];

        for (side, levels) in [(OrderSide::Buy, &self.bids), (OrderSide::Sell, &self.asks)] {
            for (price, qty) in levels {
                let price = parse::price(FixedPoint(*price), instrument.price_precision)?;
                let qty = parse::size(FixedPoint(*qty), instrument.size_precision)?;
                let order = BookOrder::new(side, price, qty, 0);
                deltas.push(OrderBookDelta::new_checked(
                    instrument.id,
                    BookAction::Add,
                    order,
                    RecordFlag::F_SNAPSHOT as u8,
                    seq,
                    event_time,
                    now,
                )?);
            }
        }

        deltas.last_mut().expect("clear is present").flags |= RecordFlag::F_LAST as u8;
        Ok(OrderBookDeltas::new(instrument.id, deltas))
    }

    fn delta(
        &mut self,
        delta: &Delta,
        instrument: &BinaryOption,
        seq: u64,
        event_time: UnixNanos,
        now: UnixNanos,
    ) -> anyhow::Result<OrderBookDeltas> {
        let side = match delta.side {
            OutcomeSide::Yes => OrderSide::Buy,
            OutcomeSide::No => OrderSide::Sell,
        };

        let price = parse::price(delta.price_dollars, instrument.price_precision)?;

        let levels = if side == OrderSide::Buy {
            &mut self.bids
        } else {
            &mut self.asks
        };

        let previous = levels
            .get(&delta.price_dollars.0)
            .copied()
            .unwrap_or_default();
        let qty = previous
            .checked_add(delta.delta_fp.0)
            .context("book size overflow")?;
        anyhow::ensure!(qty >= Decimal::ZERO, "book delta produces negative size");
        anyhow::ensure!(
            !delta.delta_fp.0.is_zero(),
            "book delta must change a level"
        );

        let size = if qty.is_zero() {
            Quantity::zero(instrument.size_precision)
        } else {
            parse::size(FixedPoint(qty), instrument.size_precision)?
        };

        let action = if qty.is_zero() {
            BookAction::Delete
        } else if previous.is_zero() {
            BookAction::Add
        } else {
            BookAction::Update
        };

        let order = BookOrder::new(side, price, size, 0);
        let delta = OrderBookDelta::new_checked(
            instrument.id,
            action,
            order,
            RecordFlag::F_LAST as u8,
            seq,
            event_time,
            now,
        )?;

        if qty.is_zero() {
            levels.remove(&price.as_decimal());
        } else {
            levels.insert(price.as_decimal(), qty);
        }

        self.integrity()?;
        Ok(OrderBookDeltas::new(instrument.id, vec![delta]))
    }

    fn integrity(&self) -> anyhow::Result<()> {
        if let (Some((bid, _)), Some((ask, _))) =
            (self.bids.last_key_value(), self.asks.first_key_value())
        {
            anyhow::ensure!(bid < ask, "crossed or locked book");
        }

        Ok(())
    }
}

#[derive(Deserialize)]
struct Snapshot {
    #[serde(default)]
    yes_dollars_fp: Vec<[FixedPoint; 2]>,
    #[serde(default)]
    no_dollars_fp: Vec<[FixedPoint; 2]>,
}

#[derive(Deserialize)]
struct Delta {
    price_dollars: FixedPoint,
    delta_fp: FixedPoint,
    side: OutcomeSide,
    ts_ms: Option<u64>,
}

#[cfg(test)]
mod tests {
    use nautilus_live::book::conformance::BookStreamChecker;
    #[cfg(not(all(feature = "simulation", madsim)))]
    use nautilus_live::task::TaskGroup;
    use nautilus_model::{
        enums::BookType,
        types::{Price, Quantity},
    };
    use rstest::rstest;
    use serde_json::Value;

    use super::*;
    #[cfg(not(all(feature = "simulation", madsim)))]
    use crate::config::KalshiDataClientConfig;
    use crate::{
        http::{
            models::{Event, Market},
            parse::parse_instrument,
        },
        websocket::messages::KalshiSubscribed,
    };
    #[cfg(not(all(feature = "simulation", madsim)))]
    #[allow(dead_code, reason = "shared key fixtures serve multiple suites")]
    mod keys {
        include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/integration/keys.rs"
        ));
    }
    #[cfg(not(all(feature = "simulation", madsim)))]
    mod http_request {
        include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/common/http_request.rs"
        ));
    }
    #[cfg(not(all(feature = "simulation", madsim)))]
    #[allow(
        dead_code,
        reason = "peer fields also serve other data lifecycle tests"
    )]
    mod peer {
        include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/common/data_peer.rs"
        ));
    }

    fn fixtures() -> Value {
        serde_json::from_str(include_str!("../../test_data/books.json")).unwrap()
    }

    fn instrument() -> BinaryOption {
        let fixtures: Value =
            serde_json::from_str(include_str!("../../test_data/instruments.json")).unwrap();
        let mut market: Market =
            serde_json::from_value(fixtures["definitions"]["live"]["market"].clone()).unwrap();
        market.ticker = "MARKET-A".into();
        let event: Event =
            serde_json::from_value(fixtures["definitions"]["live"]["event"].clone()).unwrap();
        parse_instrument(&market, &event, UnixNanos::from(5)).unwrap()
    }

    fn decimal(value: &str) -> Decimal {
        value.parse().unwrap()
    }

    fn message(payload: Value, seq: u64, id: Option<u64>) -> KalshiDataMessage {
        KalshiDataMessage {
            id,
            sid: 7,
            seq: Some(seq),
            sending_ts_ms: Some(1700000000001),
            msg: payload,
        }
    }

    fn subscribed(generation: u64, sid: u64) -> KalshiWsEvent {
        KalshiWsEvent::Message {
            generation,
            message: KalshiWsMessage::Subscribed {
                id: 1,
                msg: KalshiSubscribed {
                    channel: KalshiChannel::OrderbookDelta,
                    sid,
                },
            },
        }
    }

    fn snapshot(generation: u64, seq: u64, id: Option<u64>) -> KalshiWsEvent {
        KalshiWsEvent::Message {
            generation,
            message: KalshiWsMessage::OrderbookSnapshot(message(
                fixtures()["snapshot"].clone(),
                seq,
                id,
            )),
        }
    }

    fn snapshot_ticker(generation: u64, seq: u64, ticker: &str) -> KalshiWsEvent {
        let mut payload = fixtures()["snapshot"].clone();
        payload["market_ticker"] = ticker.into();

        KalshiWsEvent::Message {
            generation,
            message: KalshiWsMessage::OrderbookSnapshot(message(payload, seq, None)),
        }
    }

    fn snapshot_sid(generation: u64, seq: u64, sid: u64, ticker: &str) -> KalshiWsEvent {
        let mut event = snapshot_ticker(generation, seq, ticker);

        let KalshiWsEvent::Message {
            message: KalshiWsMessage::OrderbookSnapshot(data),
            ..
        } = &mut event
        else {
            unreachable!("snapshot event");
        };

        data.sid = sid;
        event
    }

    fn delta_ticker(
        generation: u64,
        seq: u64,
        sid: u64,
        ticker: &str,
        mut payload: Value,
    ) -> KalshiWsEvent {
        payload["market_ticker"] = ticker.into();
        let mut data = message(payload, seq, None);
        data.sid = sid;

        KalshiWsEvent::Message {
            generation,
            message: KalshiWsMessage::OrderbookDelta(data),
        }
    }

    fn oracle_levels() -> (BTreeMap<Decimal, Decimal>, BTreeMap<Decimal, Decimal>) {
        let snapshot = fixtures()["snapshot"].clone();
        let mut bids = BTreeMap::new();
        let mut asks = BTreeMap::new();

        for row in snapshot["yes_dollars_fp"].as_array().unwrap() {
            bids.insert(
                decimal(row[0].as_str().unwrap()),
                decimal(row[1].as_str().unwrap()),
            );
        }

        for row in snapshot["no_dollars_fp"].as_array().unwrap() {
            asks.insert(
                decimal(row[0].as_str().unwrap()),
                decimal(row[1].as_str().unwrap()),
            );
        }

        (bids, asks)
    }

    fn batch(receiver: &mut tokio::sync::mpsc::UnboundedReceiver<DataEvent>) -> OrderBookDeltas {
        let DataEvent::Data(Data::BookDeltas(batch)) = receiver.try_recv().unwrap() else {
            panic!("expected book batch");
        };

        *batch
    }

    #[rstest]
    fn exact_levels_actions_flags_and_empty_replacement() {
        let instrument = instrument();
        let fixture = fixtures();
        let snapshot: Snapshot = serde_json::from_value(fixture["snapshot"].clone()).unwrap();
        let mut levels = Levels::from_snapshot(
            &snapshot.yes_dollars_fp,
            &snapshot.no_dollars_fp,
            false,
            &instrument,
        )
        .unwrap();
        let now = UnixNanos::from(123);
        let event_time = UnixNanos::from(91);
        let output = levels.snapshot(&instrument, 17, event_time, now).unwrap();
        let mut expected = vec![OrderBookDelta::clear(instrument.id, 17, event_time, now)];

        for (side, price, qty, flags) in [
            (OrderSide::Buy, "0.123456", "7.25", 32),
            (OrderSide::Buy, "0.213456", "4.50", 32),
            (OrderSide::Sell, "0.713456", "9.75", 32),
            (OrderSide::Sell, "0.813456", "6.25", 160),
        ] {
            let price = Price::from_decimal_dp(decimal(price), instrument.price_precision).unwrap();
            let qty = Quantity::from_decimal_dp(decimal(qty), instrument.size_precision).unwrap();
            let order = BookOrder::new(side, price, qty, 0);
            expected.push(OrderBookDelta::new(
                instrument.id,
                BookAction::Add,
                order,
                flags,
                17,
                event_time,
                now,
            ));
        }

        assert_eq!(output, OrderBookDeltas::new(instrument.id, expected));
        let mut checker = BookStreamChecker::new(BookType::L2_MBP, true);
        checker.open(instrument.id);
        checker.apply(&output).unwrap();

        let cases = [
            (BookAction::Update, OrderSide::Buy, "0.123456", "9.75"),
            (BookAction::Update, OrderSide::Sell, "0.713456", "6.50"),
            (BookAction::Delete, OrderSide::Buy, "0.213456", "0"),
            (BookAction::Add, OrderSide::Sell, "0.613456", "8.25"),
        ];

        let mut bids = BTreeMap::from([
            (decimal("0.123456"), decimal("7.25")),
            (decimal("0.213456"), decimal("4.50")),
        ]);
        let mut asks = BTreeMap::from([
            (decimal("0.713456"), decimal("9.75")),
            (decimal("0.813456"), decimal("6.25")),
        ]);

        for (index, (action, side, price, qty)) in cases.into_iter().enumerate() {
            let payload: Delta = serde_json::from_value(fixture["steps"][index].clone()).unwrap();
            let seq = 18 + index as u64;
            let output = levels
                .delta(&payload, &instrument, seq, event_time, now)
                .unwrap();
            let price = Price::from_decimal_dp(decimal(price), instrument.price_precision).unwrap();
            let qty = Quantity::from_decimal_dp(decimal(qty), instrument.size_precision).unwrap();
            let order = BookOrder::new(side, price, qty, 0);
            assert_eq!(
                output,
                OrderBookDeltas::new(
                    instrument.id,
                    vec![OrderBookDelta::new(
                        instrument.id,
                        action,
                        order,
                        128,
                        seq,
                        event_time,
                        now
                    )]
                )
            );

            let truth = if side == OrderSide::Buy {
                &mut bids
            } else {
                &mut asks
            };

            if qty.is_zero() {
                truth.remove(&price.as_decimal());
            } else {
                truth.insert(price.as_decimal(), qty.as_decimal());
            }

            checker.apply(&output).unwrap();
            checker.verify(instrument.id, 100, &bids, &asks).unwrap();
        }

        let empty = Levels::default()
            .snapshot(&instrument, 30, event_time, now)
            .unwrap();
        let mut clear = OrderBookDelta::clear(instrument.id, 30, event_time, now);
        clear.flags = 160;
        assert_eq!(empty, OrderBookDeltas::new(instrument.id, vec![clear]));
        checker.apply(&empty).unwrap();
        checker
            .verify(instrument.id, 100, &BTreeMap::new(), &BTreeMap::new())
            .unwrap();
    }

    #[rstest]
    fn rest_no_bids_complement_once_and_carry_no_watermark() {
        let instrument = instrument();
        let rest: Orderbook = serde_json::from_value(fixtures()["rest"].clone()).unwrap();
        let output = Levels::rest(&rest, &instrument, UnixNanos::from(13)).unwrap();
        assert_eq!(
            output.deltas[2].order.price.as_decimal(),
            decimal("0.713456")
        );
        assert_eq!(output.deltas[2].order.size.as_decimal(), decimal("9.75"));
        assert!(output.deltas.iter().all(|delta| delta.sequence == 0));
    }

    #[rstest]
    #[case("delta_fp", "-100.00")]
    #[case("delta_fp", "0.0000001")]
    #[case("price_dollars", "0.1234567")]
    #[case("side", "invalid")]
    #[case("price_dollars", "1.5")]
    fn invalid_delta_cannot_publish(#[case] field: &str, #[case] value: &str) {
        let instrument = instrument();
        let snapshot: Snapshot = serde_json::from_value(fixtures()["snapshot"].clone()).unwrap();
        let mut levels = Levels::from_snapshot(
            &snapshot.yes_dollars_fp,
            &snapshot.no_dollars_fp,
            false,
            &instrument,
        )
        .unwrap();
        let mut payload = fixtures()["steps"][0].clone();
        payload[field] = value.into();

        let result = serde_json::from_value::<Delta>(payload)
            .map_err(anyhow::Error::from)
            .and_then(|payload| {
                levels.delta(
                    &payload,
                    &instrument,
                    2,
                    UnixNanos::from(1),
                    UnixNanos::from(2),
                )
            });

        assert!(result.is_err());
    }

    #[rstest]
    fn canceled_confirmation_cannot_open_new_attempt_or_accept_late_snapshot() {
        let instrument = instrument();
        let id = instrument.id;
        let instruments = std::collections::HashMap::from([(id, instrument)]);
        let books = BookSyncTracker::new(&[id], Duration::from_secs(10));
        books.subscribe(id, None).unwrap();
        books.state.lock().unwrap().running = true;
        let (sender, mut output) = tokio::sync::mpsc::unbounded_channel();
        let sender = sender.into();
        let now = UnixNanos::from(9);
        books
            .event(&subscribed(0, 7), &instruments, &sender, now)
            .unwrap();
        books
            .event(&snapshot(0, 1, None), &instruments, &sender, now)
            .unwrap();
        let _ = batch(&mut output);
        let recovery = books
            .state
            .lock()
            .unwrap()
            .books
            .get_mut(&id)
            .unwrap()
            .sync
            .claim()
            .unwrap();
        assert!(recovery.begin_replacement());
        let cancel = recovery.cancellation.child_token();
        let token = books
            .attempt(id, cancel.clone(), recovery.gate.clone(), &recovery)
            .unwrap();
        cancel.cancel();
        assert!(recovery.begin_replacement());
        books
            .event(
                &KalshiWsEvent::BookSnapshotRequested {
                    generation: 0,
                    sid: 7,
                    id: 20,
                    attempt: token,
                },
                &instruments,
                &sender,
                now,
            )
            .unwrap();

        assert!(recovery.gate.lock().is_closed());
        books
            .event(&snapshot(0, 2, Some(20)), &instruments, &sender, now)
            .unwrap();
        assert!(output.try_recv().is_err());
        let token = books
            .attempt(
                id,
                recovery.cancellation.child_token(),
                recovery.gate.clone(),
                &recovery,
            )
            .unwrap();
        books
            .event(
                &KalshiWsEvent::BookSnapshotRequested {
                    generation: 0,
                    sid: 7,
                    id: 21,
                    attempt: token,
                },
                &instruments,
                &sender,
                now,
            )
            .unwrap();

        books
            .event(&snapshot(0, 3, Some(20)), &instruments, &sender, now)
            .unwrap();
        assert!(output.try_recv().is_err());
        books
            .event(&snapshot(0, 4, Some(21)), &instruments, &sender, now)
            .unwrap();
        assert_eq!(batch(&mut output).sequence, 4);
        assert!(recovery.is_accepted());
        books.unsubscribe(id, None).unwrap();
        assert!(recovery.cancellation.is_cancelled());
        books
            .event(&snapshot(0, 5, Some(21)), &instruments, &sender, now)
            .unwrap();
        assert!(output.try_recv().is_err());
    }

    #[rstest]
    fn reconnect_preserves_owner_and_rejects_stale_generation() {
        let instrument = instrument();
        let id = instrument.id;
        let instruments = std::collections::HashMap::from([(id, instrument)]);
        let books = BookSyncTracker::new(&[id], Duration::from_secs(10));
        books.subscribe(id, None).unwrap();
        books.state.lock().unwrap().running = true;
        let (sender, mut output) = tokio::sync::mpsc::unbounded_channel();
        let sender = sender.into();
        let now = UnixNanos::from(9);
        books
            .event(&subscribed(0, 7), &instruments, &sender, now)
            .unwrap();
        let recovery = books
            .state
            .lock()
            .unwrap()
            .books
            .get_mut(&id)
            .unwrap()
            .sync
            .claim()
            .unwrap();
        assert!(recovery.begin_replacement());
        books
            .event(&subscribed(1, 7), &instruments, &sender, now)
            .unwrap();
        assert!(
            books.state.lock().unwrap().books[&id]
                .sync
                .is_current(&recovery)
        );
        assert!(
            books
                .state
                .lock()
                .unwrap()
                .books
                .get_mut(&id)
                .unwrap()
                .sync
                .claim()
                .is_none()
        );
        books
            .event(&snapshot(0, 1, None), &instruments, &sender, now)
            .unwrap();
        assert!(output.try_recv().is_err());
        let token = books
            .attempt(
                id,
                recovery.cancellation.child_token(),
                recovery.gate.clone(),
                &recovery,
            )
            .unwrap();
        books
            .event(
                &KalshiWsEvent::BookSnapshotRequested {
                    generation: 1,
                    sid: 7,
                    id: 21,
                    attempt: token,
                },
                &instruments,
                &sender,
                now,
            )
            .unwrap();

        books
            .event(&snapshot(1, 2, Some(21)), &instruments, &sender, now)
            .unwrap();
        assert_eq!(batch(&mut output).sequence, 2);
        assert!(recovery.is_accepted());
        books.stop(false);
        books
            .event(&snapshot(1, 3, Some(21)), &instruments, &sender, now)
            .unwrap();
        assert!(output.try_recv().is_err());
    }

    #[rstest]
    fn schedules_match_independent_publication_model() {
        let instrument = instrument();
        let id = instrument.id;
        let instruments = std::collections::HashMap::from([(id, instrument)]);
        let fixture = fixtures();
        let now = UnixNanos::from(9);

        for schedule in 0_u32..6_u32.pow(5) {
            let books = BookSyncTracker::new(&[id], Duration::from_secs(10));
            books.state.lock().unwrap().running = true;
            let (sender, mut output) = tokio::sync::mpsc::unbounded_channel();
            let sender = sender.into();
            let mut checker = BookStreamChecker::new(BookType::L2_MBP, true);
            let mut model = PublicationModel::default();
            let mut generation = 0;
            let mut seq = 0;
            let mut choices = schedule;
            for _ in 0..5 {
                let choice = choices % 6;
                choices /= 6;
                seq += 1;

                match choice {
                    0 if !model.active => {
                        books.subscribe(id, None).unwrap();
                        books
                            .event(&subscribed(generation, 7), &instruments, &sender, now)
                            .unwrap();
                        checker.open(id);
                    }
                    1 => {
                        books
                            .event(&snapshot(generation, seq, None), &instruments, &sender, now)
                            .unwrap();
                    }
                    2 => {
                        let data = message(fixture["steps"][0].clone(), seq, None);
                        books
                            .event(
                                &KalshiWsEvent::Message {
                                    generation,
                                    message: KalshiWsMessage::OrderbookDelta(data),
                                },
                                &instruments,
                                &sender,
                                now,
                            )
                            .unwrap();
                    }
                    3 => {
                        books
                            .event(
                                &KalshiWsEvent::BookSequenceGap { generation, sid: 7 },
                                &instruments,
                                &sender,
                                now,
                            )
                            .unwrap();
                    }
                    4 => {
                        generation += 1;
                        books
                            .event(&subscribed(generation, 7), &instruments, &sender, now)
                            .unwrap();
                    }
                    5 if model.active => {
                        books.unsubscribe(id, None).unwrap();
                        checker.close(id);
                    }
                    _ => {}
                }

                if model.step(choice) {
                    let batch = batch(&mut output);
                    assert_eq!(batch.sequence, seq, "schedule {schedule}, choice {choice}");
                    checker.apply(&batch).unwrap();
                    let bids = BTreeMap::from([
                        (decimal("0.123456"), model.bid_qty),
                        (decimal("0.213456"), decimal("4.50")),
                    ]);
                    let asks = BTreeMap::from([
                        (decimal("0.713456"), decimal("9.75")),
                        (decimal("0.813456"), decimal("6.25")),
                    ]);
                    checker.verify(id, 100, &bids, &asks).unwrap();
                }

                assert!(
                    output.try_recv().is_err(),
                    "unexpected output: schedule {schedule}, choice {choice}"
                );
                assert_eq!(
                    books
                        .state
                        .lock()
                        .unwrap()
                        .books
                        .get(&id)
                        .is_some_and(|book| matches!(book.sync.phase(), BookPhase::Synced(_))),
                    model.synced,
                    "schedule {schedule}, choice {choice}"
                );
            }
        }
    }

    #[rstest]
    fn multi_market_connection_schedule_matches_independent_oracle() {
        let first = instrument();
        let first_id = first.id;
        let mut second = first.clone();
        second.id = InstrumentId::from("MARKET-B.KALSHI");
        let second_id = second.id;
        let instruments = HashMap::from([(first_id, first), (second_id, second)]);
        let books = BookSyncTracker::new(&[first_id, second_id], Duration::from_secs(10));
        books.state.lock().unwrap().running = true;
        let (sender, mut output) = tokio::sync::mpsc::unbounded_channel();
        let sender = sender.into();
        let mut checker = BookStreamChecker::new(BookType::L2_MBP, true);
        checker.open(first_id);
        checker.open(second_id);
        let now = UnixNanos::from(9);
        let (mut bids, asks) = oracle_levels();

        books.subscribe(first_id, None).unwrap();
        books.subscribe(second_id, None).unwrap();
        books
            .event(&subscribed(0, 7), &instruments, &sender, now)
            .unwrap();
        books
            .event(
                &snapshot_sid(0, 1, 99, "MARKET-A"),
                &instruments,
                &sender,
                now,
            )
            .unwrap();
        assert!(output.try_recv().is_err());

        for (seq, ticker) in [(1, "MARKET-A"), (2, "MARKET-B")] {
            books
                .event(&snapshot_ticker(0, seq, ticker), &instruments, &sender, now)
                .unwrap();
            let emitted = batch(&mut output);
            checker.apply(&emitted).unwrap();
            checker
                .verify(emitted.instrument_id, 100, &bids, &asks)
                .unwrap();
        }

        books
            .event(
                &delta_ticker(0, 3, 7, "MARKET-A", fixtures()["steps"][0].clone()),
                &instruments,
                &sender,
                now,
            )
            .unwrap();
        let emitted = batch(&mut output);
        assert_eq!(emitted.instrument_id, first_id);
        checker.apply(&emitted).unwrap();
        *bids.get_mut(&decimal("0.123456")).unwrap() += decimal("2.50");
        checker.verify(first_id, 100, &bids, &asks).unwrap();
        assert!(output.try_recv().is_err());

        books
            .event(
                &KalshiWsEvent::BookSequenceGap {
                    generation: 0,
                    sid: 7,
                },
                &instruments,
                &sender,
                now,
            )
            .unwrap();

        books
            .event(
                &delta_ticker(0, 4, 7, "MARKET-B", fixtures()["steps"][0].clone()),
                &instruments,
                &sender,
                now,
            )
            .unwrap();
        assert!(output.try_recv().is_err());

        books
            .event(&subscribed(1, 8), &instruments, &sender, now)
            .unwrap();
        books
            .event(
                &snapshot_ticker(0, 5, "MARKET-A"),
                &instruments,
                &sender,
                now,
            )
            .unwrap();
        assert!(output.try_recv().is_err());

        let (bids, asks) = oracle_levels();

        for (seq, ticker, id) in [(1, "MARKET-A", first_id), (2, "MARKET-B", second_id)] {
            books
                .event(&snapshot_sid(1, seq, 8, ticker), &instruments, &sender, now)
                .unwrap();
            let emitted = batch(&mut output);
            assert_eq!(emitted.instrument_id, id);
            checker.apply(&emitted).unwrap();
            checker.verify(id, 100, &bids, &asks).unwrap();
        }

        books.unsubscribe(first_id, None).unwrap();
        checker.close(first_id);
        books
            .event(
                &delta_ticker(1, 3, 8, "MARKET-B", fixtures()["steps"][1].clone()),
                &instruments,
                &sender,
                now,
            )
            .unwrap();
        let emitted = batch(&mut output);
        assert_eq!(emitted.instrument_id, second_id);
        checker.apply(&emitted).unwrap();
        let mut reduced_asks = asks.clone();
        *reduced_asks.get_mut(&decimal("0.713456")).unwrap() -= decimal("3.25");
        checker
            .verify(second_id, 100, &bids, &reduced_asks)
            .unwrap();
        assert!(output.try_recv().is_err());

        books
            .event(
                &delta_ticker(1, 4, 8, "MARKET-A", fixtures()["steps"][0].clone()),
                &instruments,
                &sender,
                now,
            )
            .unwrap();
        assert!(output.try_recv().is_err());
        books.unsubscribe(second_id, None).unwrap();
        checker.close(second_id);
        books
            .event(
                &snapshot_sid(1, 5, 8, "MARKET-B"),
                &instruments,
                &sender,
                now,
            )
            .unwrap();
        assert!(output.try_recv().is_err());
    }

    #[cfg(not(all(feature = "simulation", madsim)))]
    #[tokio::test]
    async fn expired_initial_wait_and_refused_task_are_retried_without_another_delta() {
        time::timeout(Duration::from_secs(8), async {
            let mut peer = peer::Peer::new().await;
            let instrument = instrument();
            let id = instrument.id;
            let books = BookSyncTracker::new(&[id], Duration::from_secs(10));
            books.subscribe(id, None).unwrap();
            let (ws_client, mut input) = KalshiWebSocketClient::new(peer.config.clone()).unwrap();
            books.replay(&ws_client).unwrap();
            ws_client.connect().await.unwrap();
            let _ = peer.command().await;
            let instruments = std::collections::HashMap::from([(id, instrument)]);
            let (sender, mut output) = tokio::sync::mpsc::unbounded_channel();
            let sender = sender.into();
            let now = UnixNanos::from(9);
            let initial_tasks = TaskGroup::new();
            books.monitor(&ws_client, &initial_tasks.spawner().unwrap());
            assert_eq!(
                *books.state.lock().unwrap().books[&id].sync.phase(),
                BookPhase::Waiting
            );
            initial_tasks.begin_shutdown();
            initial_tasks
                .finish_shutdown(Duration::from_secs(1), Duration::from_secs(1))
                .await
                .unwrap();

            loop {
                let event = input.recv().await.unwrap();

                if matches!(
                    event,
                    KalshiWsEvent::Message {
                        message: KalshiWsMessage::OrderbookSnapshot(_),
                        ..
                    }
                ) {
                    break;
                }

                books.event(&event, &instruments, &sender, now).unwrap();
            }

            books
                .state
                .lock()
                .unwrap()
                .books
                .get_mut(&id)
                .unwrap()
                .sync
                .arm_deadline(time::Instant::now() - Duration::from_secs(1));
            let tasks = TaskGroup::new();
            let refused = tasks.spawner().unwrap();
            tasks.begin_shutdown();
            books.monitor(&ws_client, &refused);
            {
                let mut state = books.state.lock().unwrap();
                let recovery = state
                    .books
                    .get_mut(&id)
                    .unwrap()
                    .sync
                    .claim()
                    .expect("refused task must release ownership");
                recovery.cancellation.cancel();
            }

            tasks
                .finish_shutdown(Duration::from_secs(1), Duration::from_secs(1))
                .await
                .unwrap();
            tasks.start_generation().unwrap();
            books.monitor(&ws_client, &tasks.spawner().unwrap());
            let command = peer.command().await;
            assert_eq!(command["params"]["action"], "get_snapshot");

            loop {
                books
                    .event(&input.recv().await.unwrap(), &instruments, &sender, now)
                    .unwrap();

                if let Ok(DataEvent::Data(Data::BookDeltas(batch))) = output.try_recv() {
                    assert_eq!(batch.sequence, 2);
                    break;
                }
            }

            books.unsubscribe(id, None).unwrap();
            tasks.begin_shutdown();
            tasks
                .finish_shutdown(Duration::from_secs(1), Duration::from_secs(1))
                .await
                .unwrap();
            ws_client.dispose().await.unwrap();
        })
        .await
        .unwrap();
    }

    #[rstest]
    #[case::missing_identity("market_ticker", None)]
    #[case::timestamp_overflow("ts_ms", Some(Value::from(u64::MAX)))]
    #[case::crossed("price_dollars", Some(Value::from("0.900000")))]
    #[case::negative_size("delta_fp", Some(Value::from("-100.00")))]
    #[case::invalid_side("side", Some(Value::from("invalid")))]
    fn invalid_frame_stops_publication(#[case] field: &str, #[case] value: Option<Value>) {
        let instrument = instrument();
        let id = instrument.id;
        let instruments = std::collections::HashMap::from([(id, instrument)]);
        let books = BookSyncTracker::new(&[id], Duration::from_secs(10));
        books.subscribe(id, None).unwrap();
        books.state.lock().unwrap().running = true;
        let (sender, mut output) = tokio::sync::mpsc::unbounded_channel();
        let sender = sender.into();
        let now = UnixNanos::from(9);
        books
            .event(&subscribed(0, 7), &instruments, &sender, now)
            .unwrap();
        books
            .event(&snapshot(0, 1, None), &instruments, &sender, now)
            .unwrap();
        let _ = batch(&mut output);
        let mut payload = fixtures()["steps"][0].clone();
        if let Some(value) = value {
            payload[field] = value;
        } else {
            payload.as_object_mut().unwrap().remove(field);
        }

        books
            .event(
                &KalshiWsEvent::Message {
                    generation: 0,
                    message: KalshiWsMessage::OrderbookDelta(message(payload, 2, None)),
                },
                &instruments,
                &sender,
                now,
            )
            .unwrap();

        books
            .event(
                &KalshiWsEvent::Message {
                    generation: 0,
                    message: KalshiWsMessage::OrderbookDelta(message(
                        fixtures()["steps"][0].clone(),
                        3,
                        None,
                    )),
                },
                &instruments,
                &sender,
                now,
            )
            .unwrap();

        assert_eq!(
            *books.state.lock().unwrap().books[&id].sync.phase(),
            BookPhase::Recovering
        );
        assert!(output.try_recv().is_err());
    }

    #[derive(Default)]
    struct PublicationModel {
        active: bool,
        synced: bool,
        initial: bool,
        bid_qty: Decimal,
    }

    impl PublicationModel {
        fn step(&mut self, choice: u32) -> bool {
            match choice {
                0 if !self.active => {
                    self.active = true;
                    self.synced = false;
                    self.initial = true;
                }
                1 if self.active && self.initial => {
                    self.synced = true;
                    self.initial = false;
                    self.bid_qty = decimal("7.25");
                    return true;
                }
                2 if self.active && self.synced => {
                    self.bid_qty += decimal("2.50");
                    return true;
                }
                3 if self.active => {
                    self.synced = false;
                    self.initial = false;
                }
                4 if self.active => {
                    self.synced = false;
                    self.initial = true;
                }
                5 if self.active => {
                    self.active = false;
                    self.synced = false;
                    self.initial = false;
                }
                _ => {}
            }

            false
        }
    }

    #[rstest]
    #[case::unsubscribe(0)]
    #[case::stop(1)]
    #[case::reset(2)]
    #[case::resubscribe(3)]
    fn retirement_before_confirmation_cannot_open_gate(#[case] action: u8) {
        let instrument = instrument();
        let id = instrument.id;
        let instruments = std::collections::HashMap::from([(id, instrument)]);
        let books = BookSyncTracker::new(&[id], Duration::from_secs(10));
        books.subscribe(id, None).unwrap();
        books.state.lock().unwrap().running = true;
        let (sender, mut output) = tokio::sync::mpsc::unbounded_channel();
        let sender = sender.into();
        let now = UnixNanos::from(9);
        books
            .event(&subscribed(0, 7), &instruments, &sender, now)
            .unwrap();
        let owner = books
            .state
            .lock()
            .unwrap()
            .books
            .get_mut(&id)
            .unwrap()
            .sync
            .claim()
            .unwrap();
        assert!(owner.begin_replacement());
        let token = books
            .attempt(
                id,
                owner.cancellation.child_token(),
                owner.gate.clone(),
                &owner,
            )
            .unwrap();

        match action {
            0 => books.unsubscribe(id, None).unwrap(),
            1 => books.stop(false),
            2 => books.stop(true),
            3 => {
                books.unsubscribe(id, None).unwrap();
                books.subscribe(id, None).unwrap();
            }
            _ => unreachable!(),
        }

        books
            .event(
                &KalshiWsEvent::BookSnapshotRequested {
                    generation: 0,
                    sid: 7,
                    id: 21,
                    attempt: token,
                },
                &instruments,
                &sender,
                now,
            )
            .unwrap();

        books
            .event(&snapshot(0, 2, Some(21)), &instruments, &sender, now)
            .unwrap();
        assert!(owner.cancellation.is_cancelled());
        assert!(owner.gate.lock().is_closed());
        assert!(output.try_recv().is_err());

        if action == 3 {
            let result = books.attempt(id, CancellationToken::new(), owner.gate.clone(), &owner);
            assert_eq!(
                result.unwrap_err().to_string(),
                "client error: book recovery owner retired"
            );
        }
    }

    #[cfg(not(all(feature = "simulation", madsim)))]
    #[tokio::test]
    async fn native_reconnect_retains_pending_recovery_owner() {
        time::timeout(Duration::from_secs(10), async {
            let mut peer = peer::Peer::new().await;
            let instrument = instrument();
            let id = instrument.id;
            let instruments = std::collections::HashMap::from([(id, instrument)]);
            let books = BookSyncTracker::new(&[id], Duration::from_secs(10));
            books.subscribe(id, None).unwrap();
            let (ws_client, mut input) = KalshiWebSocketClient::new(peer.config.clone()).unwrap();
            books.replay(&ws_client).unwrap();
            ws_client.connect().await.unwrap();
            let _ = peer.command().await;
            let (sender, mut output) = tokio::sync::mpsc::unbounded_channel();
            let sender = sender.into();
            let now = UnixNanos::from(9);

            loop {
                books
                    .event(&input.recv().await.unwrap(), &instruments, &sender, now)
                    .unwrap();

                if output.try_recv().is_ok() {
                    break;
                }
            }

            let owner = books
                .state
                .lock()
                .unwrap()
                .books
                .get_mut(&id)
                .unwrap()
                .sync
                .claim()
                .unwrap();
            assert!(owner.begin_replacement());
            let cancel = owner.cancellation.child_token();
            let token = books
                .attempt(id, cancel.clone(), owner.gate.clone(), &owner)
                .unwrap();
            peer.confirm
                .store(false, std::sync::atomic::Ordering::SeqCst);
            ws_client
                .request_book_snapshot(&books.subscription, id.symbol.to_string(), token, cancel)
                .await
                .unwrap();
            let request = peer.command().await;
            assert_eq!(request["params"]["action"], "get_snapshot");

            loop {
                let event = input.recv().await.unwrap();
                books.event(&event, &instruments, &sender, now).unwrap();

                if matches!(event, KalshiWsEvent::BookSnapshotRequested { .. }) {
                    break;
                }
            }

            peer.confirm
                .store(true, std::sync::atomic::Ordering::SeqCst);
            peer.actions.send(("close".into(), Value::Null)).unwrap();
            ws_client.stop().await.unwrap();
            ws_client.connect().await.unwrap();
            let tasks = TaskGroup::new();

            loop {
                let event = input.recv().await.unwrap();
                books.event(&event, &instruments, &sender, now).unwrap();
                books.monitor(&ws_client, &tasks.spawner().unwrap());

                if let Ok(DataEvent::Data(Data::BookDeltas(batch))) = output.try_recv() {
                    assert_eq!(batch.sequence, 1);
                    assert_eq!(
                        batch.flags,
                        RecordFlag::F_SNAPSHOT as u8 | RecordFlag::F_LAST as u8
                    );
                    break;
                }

                let mut state = books.state.lock().unwrap();
                assert!(state.books[&id].sync.is_current(&owner));
                assert!(state.books.get_mut(&id).unwrap().sync.claim().is_none());
            }

            assert!(owner.is_accepted());
            assert!(!owner.cancellation.is_cancelled());
            books
                .event(
                    &snapshot(0, 999, Some(request["id"].as_u64().unwrap())),
                    &instruments,
                    &sender,
                    now,
                )
                .unwrap();
            assert!(output.try_recv().is_err());
            tasks.begin_shutdown();
            tasks
                .finish_shutdown(Duration::from_secs(1), Duration::from_secs(1))
                .await
                .unwrap();
            books.stop(false);
            ws_client.dispose().await.unwrap();
        })
        .await
        .unwrap();
    }

    #[rstest]
    fn book_demand_survives_until_the_last_reference_retires() {
        let instrument = instrument();
        let id = instrument.id;
        let instruments = std::collections::HashMap::from([(id, instrument)]);
        let books = BookSyncTracker::new(&[id], Duration::from_secs(10));
        books.subscribe(id, None).unwrap();
        books.subscribe(id, None).unwrap();
        books.state.lock().unwrap().running = true;
        let (sender, mut output) = tokio::sync::mpsc::unbounded_channel();
        let sender = sender.into();
        let now = UnixNanos::from(9);
        books
            .event(&subscribed(0, 7), &instruments, &sender, now)
            .unwrap();
        books
            .event(&snapshot(0, 1, None), &instruments, &sender, now)
            .unwrap();
        let _ = batch(&mut output);
        books.unsubscribe(id, None).unwrap();

        let delta = KalshiWsEvent::Message {
            generation: 0,
            message: KalshiWsMessage::OrderbookDelta(message(
                fixtures()["steps"][0].clone(),
                2,
                None,
            )),
        };

        books.event(&delta, &instruments, &sender, now).unwrap();
        let output_delta = batch(&mut output);
        assert_eq!(output_delta.sequence, 2);
        assert_eq!(
            output_delta.deltas[0].order.size.as_decimal(),
            decimal("9.75")
        );
        books.unsubscribe(id, None).unwrap();
        books.event(&delta, &instruments, &sender, now).unwrap();
        assert!(output.try_recv().is_err());
    }

    #[cfg(not(all(feature = "simulation", madsim)))]
    #[tokio::test]
    async fn last_book_readd_after_rejected_unsubscribe_recovers() {
        time::timeout(Duration::from_secs(8), async {
            let mut peer = peer::Peer::new().await;
            let instrument = instrument();
            let id = instrument.id;
            let instruments = std::collections::HashMap::from([(id, instrument)]);
            let books = BookSyncTracker::new(&[id], Duration::from_secs(10));
            books.subscribe(id, None).unwrap();
            let (ws_client, mut input) = KalshiWebSocketClient::new(peer.config.clone()).unwrap();
            books.replay(&ws_client).unwrap();
            ws_client.connect().await.unwrap();
            let _ = peer.command().await;
            let (sender, mut output) = tokio::sync::mpsc::unbounded_channel();
            let sender = sender.into();
            let now = UnixNanos::from(9);

            loop {
                books
                    .event(&input.recv().await.unwrap(), &instruments, &sender, now)
                    .unwrap();

                if output.try_recv().is_ok() {
                    break;
                }
            }

            peer.confirm
                .store(false, std::sync::atomic::Ordering::SeqCst);
            books.unsubscribe(id, Some(&ws_client)).unwrap();
            let unsubscribe = peer.command().await;
            assert_eq!(unsubscribe["cmd"], "unsubscribe");
            books.subscribe(id, Some(&ws_client)).unwrap();
            peer.confirm
                .store(true, std::sync::atomic::Ordering::SeqCst);
            let mut frame = serde_json::from_str::<Value>(include_str!(
                "../../test_data/websocket_messages.json"
            ))
            .unwrap()["unsubscribe_rate_limit"]
                .clone();
            frame["id"] = serde_json::json!(unsubscribe["id"]);
            peer.actions.send(("raw".into(), frame)).unwrap();

            loop {
                let event = input.recv().await.unwrap();
                books.event(&event, &instruments, &sender, now).unwrap();

                if matches!(event, KalshiWsEvent::Ready { .. }) {
                    break;
                }
            }

            assert!(ws_client.is_active());
            let tasks = TaskGroup::new();
            books.monitor(&ws_client, &tasks.spawner().unwrap());
            let extra_owner = books
                .state
                .lock()
                .unwrap()
                .books
                .get_mut(&id)
                .unwrap()
                .sync
                .claim();

            if let Some(owner) = extra_owner {
                owner.cancellation.cancel();
                panic!("ready retained SID left the re-added book without a recovery owner");
            }

            let request = peer.command().await;
            assert_eq!(request["params"]["action"], "get_snapshot");

            loop {
                books
                    .event(&input.recv().await.unwrap(), &instruments, &sender, now)
                    .unwrap();

                if let Ok(DataEvent::Data(Data::BookDeltas(batch))) = output.try_recv() {
                    assert_eq!(batch.sequence, 2);
                    break;
                }
            }

            books.stop(false);
            tasks.begin_shutdown();
            tasks
                .finish_shutdown(Duration::from_secs(1), Duration::from_secs(1))
                .await
                .unwrap();
            ws_client.dispose().await.unwrap();
        })
        .await
        .unwrap();
    }

    #[rstest]
    fn unsubscribe_ack_retires_sid_before_new_subscription() {
        let instrument = instrument();
        let id = instrument.id;
        let instruments = HashMap::from([(id, instrument)]);
        let books = BookSyncTracker::new(&[id], Duration::from_secs(10));
        books.subscribe(id, None).unwrap();
        books.state.lock().unwrap().running = true;
        let (sender, mut output) = tokio::sync::mpsc::unbounded_channel();
        let sender = sender.into();
        let now = UnixNanos::from(9);
        books
            .event(&subscribed(0, 7), &instruments, &sender, now)
            .unwrap();
        books
            .event(&snapshot(0, 1, None), &instruments, &sender, now)
            .unwrap();
        let _ = batch(&mut output);
        books.unsubscribe(id, None).unwrap();
        books
            .event(
                &KalshiWsEvent::Message {
                    generation: 0,
                    message: KalshiWsMessage::Unsubscribed {
                        id: Some(2),
                        sid: 7,
                        seq: 2,
                    },
                },
                &instruments,
                &sender,
                now,
            )
            .unwrap();

        assert_eq!(books.state.lock().unwrap().sid, None);
        books.subscribe(id, None).unwrap();
        books
            .event(&snapshot(0, 3, None), &instruments, &sender, now)
            .unwrap();
        assert!(output.try_recv().is_err());
        books
            .event(&subscribed(0, 9), &instruments, &sender, now)
            .unwrap();
        books
            .event(&snapshot(0, 4, None), &instruments, &sender, now)
            .unwrap();
        assert!(output.try_recv().is_err());

        let KalshiWsEvent::Message {
            message: KalshiWsMessage::OrderbookSnapshot(mut data),
            ..
        } = snapshot(0, 1, None)
        else {
            unreachable!()
        };

        data.sid = 9;
        books
            .event(
                &KalshiWsEvent::Message {
                    generation: 0,
                    message: KalshiWsMessage::OrderbookSnapshot(data),
                },
                &instruments,
                &sender,
                now,
            )
            .unwrap();

        assert_eq!(batch(&mut output).sequence, 1);
        assert_eq!(books.state.lock().unwrap().sid, Some(9));
    }

    #[rstest]
    fn last_book_readd_without_unsubscribe_ack_requires_correlated_snapshot() {
        let instrument = instrument();
        let id = instrument.id;
        let instruments = HashMap::from([(id, instrument)]);
        let books = BookSyncTracker::new(&[id], Duration::from_secs(10));
        books.subscribe(id, None).unwrap();
        books.state.lock().unwrap().running = true;
        let (sender, mut output) = tokio::sync::mpsc::unbounded_channel();
        let sender = sender.into();
        let now = UnixNanos::from(9);
        books
            .event(&subscribed(0, 7), &instruments, &sender, now)
            .unwrap();
        books
            .event(&snapshot(0, 1, None), &instruments, &sender, now)
            .unwrap();
        let _ = batch(&mut output);
        books.unsubscribe(id, None).unwrap();
        books.subscribe(id, None).unwrap();
        books
            .event(&snapshot(0, 2, None), &instruments, &sender, now)
            .unwrap();
        assert!(output.try_recv().is_err());
        assert_eq!(books.state.lock().unwrap().sid, Some(7));
        let owner = books
            .state
            .lock()
            .unwrap()
            .books
            .get_mut(&id)
            .unwrap()
            .sync
            .claim()
            .unwrap();
        assert!(owner.begin_replacement());
        let token = books
            .attempt(
                id,
                owner.cancellation.child_token(),
                owner.gate.clone(),
                &owner,
            )
            .unwrap();
        books
            .event(
                &KalshiWsEvent::BookSnapshotRequested {
                    generation: 0,
                    sid: 7,
                    id: 21,
                    attempt: token,
                },
                &instruments,
                &sender,
                now,
            )
            .unwrap();

        books
            .event(&snapshot(0, 3, Some(21)), &instruments, &sender, now)
            .unwrap();
        assert_eq!(batch(&mut output).sequence, 3);
        assert!(owner.is_accepted());
    }

    #[cfg(not(all(feature = "simulation", madsim)))]
    #[tokio::test]
    async fn non_book_rejection_does_not_block_book_recovery() {
        time::timeout(Duration::from_secs(8), async {
            let mut peer = peer::Peer::new().await;
            let instrument = instrument();
            let id = instrument.id;
            let instruments = HashMap::from([(id, instrument)]);
            let books = BookSyncTracker::new(&[id], Duration::from_secs(10));
            books.subscribe(id, None).unwrap();
            let (ws_client, mut input) = KalshiWebSocketClient::new(peer.config.clone()).unwrap();
            books.replay(&ws_client).unwrap();
            ws_client.connect().await.unwrap();
            let _ = peer.command().await;
            let (sender, mut output) = tokio::sync::mpsc::unbounded_channel();
            let sender = sender.into();
            let now = UnixNanos::from(9);

            loop {
                books
                    .event(&input.recv().await.unwrap(), &instruments, &sender, now)
                    .unwrap();

                if output.try_recv().is_ok() {
                    break;
                }
            }

            peer.confirm
                .store(false, std::sync::atomic::Ordering::SeqCst);
            ws_client
                .subscribe(KalshiSubscription {
                    channel: KalshiChannel::Ticker,
                    market_tickers: vec!["MARKET-A".into()],
                })
                .unwrap();

            let rejected = peer.command().await;
            assert_eq!(rejected["params"]["channels"][0], "ticker");
            let mut frame = serde_json::from_str::<Value>(include_str!(
                "../../test_data/websocket_messages.json"
            ))
            .unwrap()["command_rate_limit"]
                .clone();
            frame["id"] = serde_json::json!(rejected["id"]);
            peer.actions.send(("raw".into(), frame)).unwrap();

            loop {
                let event = input.recv().await.unwrap();
                books.event(&event, &instruments, &sender, now).unwrap();

                if matches!(event, KalshiWsEvent::Error { .. }) {
                    break;
                }
            }

            assert!(!ws_client.is_active());
            peer.confirm
                .store(true, std::sync::atomic::Ordering::SeqCst);
            let tasks = TaskGroup::new();
            books.monitor(&ws_client, &tasks.spawner().unwrap());
            let extra = books
                .state
                .lock()
                .unwrap()
                .books
                .get_mut(&id)
                .unwrap()
                .sync
                .claim();
            assert!(
                extra.is_none(),
                "unrelated command rejection prevented the book recovery owner"
            );
            let request = peer.command().await;
            assert_eq!(request["params"]["action"], "get_snapshot");

            loop {
                books
                    .event(&input.recv().await.unwrap(), &instruments, &sender, now)
                    .unwrap();

                if let Ok(DataEvent::Data(Data::BookDeltas(batch))) = output.try_recv() {
                    assert_eq!(batch.sequence, 2);
                    break;
                }
            }

            assert!(!ws_client.is_active());
            books.stop(false);
            tasks.begin_shutdown();
            tasks
                .finish_shutdown(Duration::from_secs(1), Duration::from_secs(1))
                .await
                .unwrap();
            ws_client.dispose().await.unwrap();
        })
        .await
        .unwrap();
    }

    #[rstest]
    #[case::empty_arrays(0)]
    #[case::both_omitted(1)]
    #[case::yes_omitted(2)]
    #[case::no_omitted(3)]
    fn empty_and_one_sided_replacements_clear_the_published_book(#[case] shape: u8) {
        let instrument = instrument();
        let id = instrument.id;
        let instruments = HashMap::from([(id, instrument)]);
        let books = BookSyncTracker::new(&[id], Duration::from_secs(10));
        books.subscribe(id, None).unwrap();
        books.state.lock().unwrap().running = true;
        let (sender, mut output) = tokio::sync::mpsc::unbounded_channel();
        let sender = sender.into();
        let now = UnixNanos::from(9);
        let mut checker = BookStreamChecker::new(BookType::L2_MBP, true);
        checker.open(id);
        books
            .event(&subscribed(0, 7), &instruments, &sender, now)
            .unwrap();
        books
            .event(&snapshot(0, 1, None), &instruments, &sender, now)
            .unwrap();
        checker.apply(&batch(&mut output)).unwrap();
        books
            .event(
                &KalshiWsEvent::BookSequenceGap {
                    generation: 0,
                    sid: 7,
                },
                &instruments,
                &sender,
                now,
            )
            .unwrap();

        let owner = books
            .state
            .lock()
            .unwrap()
            .books
            .get_mut(&id)
            .unwrap()
            .sync
            .claim()
            .unwrap();
        assert!(owner.begin_replacement());
        let token = books
            .attempt(
                id,
                owner.cancellation.child_token(),
                owner.gate.clone(),
                &owner,
            )
            .unwrap();
        books
            .event(
                &KalshiWsEvent::BookSnapshotRequested {
                    generation: 0,
                    sid: 7,
                    id: 21,
                    attempt: token,
                },
                &instruments,
                &sender,
                now,
            )
            .unwrap();

        let mut payload = fixtures()[if shape <= 1 { "empty" } else { "snapshot" }].clone();

        for (side, remove) in [
            ("yes_dollars_fp", shape == 1 || shape == 2),
            ("no_dollars_fp", shape == 1 || shape == 3),
        ] {
            if remove {
                payload.as_object_mut().unwrap().remove(side);
            }
        }

        books
            .event(
                &KalshiWsEvent::Message {
                    generation: 0,
                    message: KalshiWsMessage::OrderbookSnapshot(message(payload, 2, Some(21))),
                },
                &instruments,
                &sender,
                now,
            )
            .unwrap();

        let replacement = batch(&mut output);
        assert_eq!(replacement.sequence, 2);
        let mut clear = OrderBookDelta::clear(id, 2, UnixNanos::from(1700000000001000000), now);
        if shape <= 1 {
            clear.flags |= RecordFlag::F_LAST as u8;
        }

        assert_eq!(replacement.deltas[0], clear);
        assert_eq!(
            replacement.flags,
            RecordFlag::F_SNAPSHOT as u8 | RecordFlag::F_LAST as u8
        );
        checker.apply(&replacement).unwrap();

        let bids = if shape == 3 {
            BTreeMap::from([
                (decimal("0.123456"), decimal("7.25")),
                (decimal("0.213456"), decimal("4.50")),
            ])
        } else {
            BTreeMap::new()
        };

        let asks = if shape == 2 {
            BTreeMap::from([
                (decimal("0.713456"), decimal("9.75")),
                (decimal("0.813456"), decimal("6.25")),
            ])
        } else {
            BTreeMap::new()
        };

        checker.verify(id, 100, &bids, &asks).unwrap();
        assert_eq!(replacement.deltas.len(), 1 + bids.len() + asks.len());
        assert!(owner.is_accepted());
        assert!(output.try_recv().is_err());
    }

    #[rstest]
    fn snapshot_rejection_reaches_only_its_correlated_recovery() {
        use nautilus_live::book::recovery::BookRecoveryOutcome;

        use crate::websocket::messages::KalshiServerError;
        let instrument = instrument();
        let id = instrument.id;
        let mut other = instrument.clone();
        other.id = InstrumentId::from("MARKET-B.KALSHI");
        let other_id = other.id;
        let instruments = HashMap::from([(id, instrument), (other_id, other)]);
        let books = BookSyncTracker::new(&[id, other_id], Duration::from_secs(10));
        let (sender, _output) = tokio::sync::mpsc::unbounded_channel();
        let sender = sender.into();
        books.state.lock().unwrap().running = true;
        let now = UnixNanos::from(9);
        books.subscribe(id, None).unwrap();
        books.subscribe(other_id, None).unwrap();
        books
            .event(&subscribed(0, 7), &instruments, &sender, now)
            .unwrap();
        let mut owners = Vec::new();

        for (id, request) in [(id, 21), (other_id, 22)] {
            let owner = books
                .state
                .lock()
                .unwrap()
                .books
                .get_mut(&id)
                .unwrap()
                .sync
                .claim()
                .unwrap();
            assert!(owner.begin_replacement());
            let token = books
                .attempt(
                    id,
                    owner.cancellation.child_token(),
                    owner.gate.clone(),
                    &owner,
                )
                .unwrap();
            books
                .event(
                    &KalshiWsEvent::BookSnapshotRequested {
                        generation: 0,
                        sid: 7,
                        id: request,
                        attempt: token,
                    },
                    &instruments,
                    &sender,
                    now,
                )
                .unwrap();

            owners.push(owner);
        }

        books
            .event(
                &KalshiWsEvent::Message {
                    generation: 0,
                    message: KalshiWsMessage::Error {
                        id: Some(21),
                        sid: Some(7),
                        msg: KalshiServerError {
                            code: 27,
                            msg: "rate limit exceeded".into(),
                        },
                    },
                },
                &instruments,
                &sender,
                now,
            )
            .unwrap();

        assert!(matches!(
            &*owners[0].outcome.borrow(),
            BookRecoveryOutcome::Rejected(_)
        ));
        assert!(matches!(
            &*owners[1].outcome.borrow(),
            BookRecoveryOutcome::Pending
        ));
        assert!(!owners[1].gate.lock().is_closed());
        assert!(!owners[1].cancellation.is_cancelled());
    }

    #[rstest]
    #[case::disabled(0)]
    #[case::short(1)]
    #[case::nondefault(31)]
    fn snapshot_deadline_uses_the_configured_duration(#[case] seconds: u64) {
        let instrument = instrument();
        let id = instrument.id;
        let instruments = HashMap::from([(id, instrument)]);
        let books = BookSyncTracker::new(&[id], Duration::from_secs(seconds));
        books.subscribe(id, None).unwrap();
        books.state.lock().unwrap().running = true;
        let (sender, _output) = tokio::sync::mpsc::unbounded_channel();
        let sender = sender.into();
        let start = time::Instant::now();
        books
            .event(&subscribed(0, 7), &instruments, &sender, UnixNanos::from(9))
            .unwrap();
        let end = time::Instant::now();
        let mut state = books.state.lock().unwrap();
        let book = state.books.get_mut(&id).unwrap();
        assert!(book.waits_for_snapshot(Duration::from_secs(seconds)));
        book.gap();
        assert_eq!(
            book.waits_for_snapshot(Duration::from_secs(seconds)),
            seconds != 0
        );

        if seconds == 0 {
            assert!(book.sync.has_pending_snapshot());
            assert!(!book.sync.is_send_pending());
            assert!(!book.sync.take_expired(end + Duration::from_secs(1000)));
        } else {
            assert!(
                !book
                    .sync
                    .take_expired(start + Duration::from_millis(seconds * 1000 - 1))
            );
            assert!(book.sync.take_expired(end + Duration::from_secs(seconds)));
        }
    }

    #[cfg(not(all(feature = "simulation", madsim)))]
    #[tokio::test]
    async fn rejected_replay_keeps_a_recovery_owner() {
        time::timeout(Duration::from_secs(10), async {
            let mut peer = peer::Peer::new().await;
            let instrument = instrument();
            let id = instrument.id;
            let instruments = HashMap::from([(id, instrument)]);
            let books = BookSyncTracker::new(&[id], Duration::from_secs(10));
            books.subscribe(id, None).unwrap();
            let (ws_client, mut input) = KalshiWebSocketClient::new(peer.config.clone()).unwrap();
            books.replay(&ws_client).unwrap();
            ws_client.connect().await.unwrap();
            let _ = peer.command().await;
            let (sender, mut output) = tokio::sync::mpsc::unbounded_channel();
            let sender = sender.into();
            let now = UnixNanos::from(9);

            loop {
                books
                    .event(&input.recv().await.unwrap(), &instruments, &sender, now)
                    .unwrap();

                if output.try_recv().is_ok() {
                    break;
                }
            }

            peer.confirm
                .store(false, std::sync::atomic::Ordering::SeqCst);
            peer.actions.send(("close".into(), Value::Null)).unwrap();
            ws_client.stop().await.unwrap();
            let connect = ws_client.connect();
            tokio::pin!(connect);
            let rejected = tokio::select! {
                result = &mut connect => panic!("replayed connection finished before rejection: {result:?}"),
                command = peer.command() => command,
            };
            assert_eq!(rejected["cmd"], "subscribe");
            let mut frame = serde_json::from_str::<Value>(include_str!(
                "../../test_data/websocket_messages.json"
            ))
            .unwrap()["command_rate_limit"]
                .clone();
            frame["id"] = serde_json::json!(rejected["id"]);
            peer.actions.send(("raw".into(), frame)).unwrap();

            loop {
                let event = input.recv().await.unwrap();
                books.event(&event, &instruments, &sender, now).unwrap();

                if matches!(event, KalshiWsEvent::Error { generation: 1, .. }) {
                    break;
                }
            }

            assert_eq!(
                connect.await.unwrap_err().to_string(),
                "Kalshi WebSocket subscription restoration timed out"
            );
            peer.confirm
                .store(true, std::sync::atomic::Ordering::SeqCst);
            let tasks = TaskGroup::new();
            books.monitor(&ws_client, &tasks.spawner().unwrap());
            let extra = books
                .state
                .lock()
                .unwrap()
                .books
                .get_mut(&id)
                .unwrap()
                .sync
                .claim();

            if let Some(owner) = extra {
                owner.cancellation.cancel();
                panic!("rejected replay left the book unowned");
            }

            ws_client.stop().await.unwrap();
            ws_client.connect().await.unwrap();
            let replay = peer.command().await;
            assert_eq!(replay["cmd"], "subscribe");

            loop {
                let event = input.recv().await.unwrap();
                books.event(&event, &instruments, &sender, now).unwrap();

                if let Ok(DataEvent::Data(Data::BookDeltas(batch))) = output.try_recv() {
                    assert_eq!(batch.sequence, 2);
                    assert_eq!(
                        batch.flags,
                        RecordFlag::F_SNAPSHOT as u8 | RecordFlag::F_LAST as u8
                    );
                    break;
                }

                let extra = books
                    .state
                    .lock()
                    .unwrap()
                    .books
                    .get_mut(&id)
                    .unwrap()
                    .sync
                    .claim();

                if let Some(owner) = extra {
                    owner.cancellation.cancel();
                    panic!("reconnect replaced the running owner");
                }
            }

            books.stop(false);
            tasks.begin_shutdown();
            tasks
                .finish_shutdown(Duration::from_secs(1), Duration::from_secs(1))
                .await
                .unwrap();
            ws_client.dispose().await.unwrap();
        })
        .await
        .unwrap();
    }

    #[rstest]
    #[case::initial(0)]
    #[case::resubscribe(1)]
    #[case::reconnect(2)]
    fn zero_timeout_waits_for_initial_snapshot_after_subscription(#[case] transition: u8) {
        let instrument = instrument();
        let id = instrument.id;
        let instruments = HashMap::from([(id, instrument)]);
        let books = BookSyncTracker::new(&[id], Duration::ZERO);
        books.subscribe(id, None).unwrap();
        books.state.lock().unwrap().running = true;
        let (sender, _output) = tokio::sync::mpsc::unbounded_channel();
        let sender = sender.into();
        let now = UnixNanos::from(9);
        books
            .event(&subscribed(0, 7), &instruments, &sender, now)
            .unwrap();

        if transition != 0 {
            books
                .event(
                    &KalshiWsEvent::Message {
                        generation: 0,
                        message: KalshiWsMessage::OrderbookSnapshot(message(
                            fixtures()["snapshot"].clone(),
                            1,
                            None,
                        )),
                    },
                    &instruments,
                    &sender,
                    now,
                )
                .unwrap();

            assert_eq!(
                books.state.lock().unwrap().books[&id].sync.position(),
                Some(&1)
            );

            let event = if transition == 1 {
                KalshiWsEvent::Message {
                    generation: 0,
                    message: KalshiWsMessage::Unsubscribed {
                        id: Some(2),
                        sid: 7,
                        seq: 1,
                    },
                }
            } else {
                KalshiWsEvent::Ready { generation: 1 }
            };

            books.event(&event, &instruments, &sender, now).unwrap();
            books
                .event(
                    &subscribed(u64::from(transition == 2), 7),
                    &instruments,
                    &sender,
                    now,
                )
                .unwrap();
        }

        {
            let mut state = books.state.lock().unwrap();
            let book = state.books.get_mut(&id).unwrap();
            assert!(
                book.waits_for_snapshot(Duration::ZERO),
                "confirmed initial wait was treated as a gap"
            );
            assert!(
                !book
                    .sync
                    .take_expired(time::Instant::now() + Duration::from_secs(1000))
            );
        }

        books
            .event(
                &KalshiWsEvent::BookSequenceGap {
                    generation: u64::from(transition == 2),
                    sid: 7,
                },
                &instruments,
                &sender,
                now,
            )
            .unwrap();

        let state = books.state.lock().unwrap();
        assert!(
            !state.books[&id].waits_for_snapshot(Duration::ZERO),
            "a real gap failed to end the initial wait"
        );
    }
}
