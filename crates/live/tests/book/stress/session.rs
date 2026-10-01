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

//! A stress session: one data client behind a [`FaultProxy`], with every emitted batch checked.

use std::{net::SocketAddr, sync::Arc};

use ahash::{AHashMap, AHashSet};
use axum::Router;
use nautilus_common::{
    clients::DataClient,
    live::{
        dst::{
            self,
            time::{Duration, Instant},
        },
        runner::replace_data_event_sender,
    },
    messages::{
        DataEvent,
        data::{SubscribeBookDeltas, UnsubscribeBookDeltas},
    },
};
use nautilus_core::{Params, UUID4, UnixNanos};
use nautilus_live::{SocketReconnectRegistry, book::conformance::BookStreamChecker};
use nautilus_model::{
    data::{Data, OrderBookDeltas},
    enums::{BookAction, BookType},
    identifiers::{ClientId, InstrumentId},
    instruments::Instrument,
};
use nautilus_network::mode::ReconnectRequestOutcome;
use parking_lot::MappedMutexGuard;
use ustr::Ustr;

use super::{
    args::{Flag, StressArgs},
    elapsed, emit,
    proxy::{Fault, FaultProxy, Route, WireCodec},
};

const POLL_INTERVAL: Duration = Duration::from_millis(50);

/// The venue-specific pieces of a book stress harness.
pub(crate) trait StressVenue: Sized + Send + 'static {
    /// Venue name for report lines, such as `okx`.
    const NAME: &'static str;
    /// Scenario names; the first is the default.
    const SCENARIOS: &'static [&'static str];
    /// Default number of stress rounds.
    const ROUNDS: usize;
    /// Venue flags beyond `--scenario`, `--timeout`, and `--rounds`.
    const FLAGS: &'static [Flag] = &[];
    /// Whether each incremental sequence must exceed the previous one within a snapshot episode.
    const SEQUENCED: bool;
    /// How oracle comparisons count toward the run.
    const COVERAGE: Coverage;
    /// Limit for [`Session::healthy`].
    const HEALTHY_LIMIT: Duration;

    /// Proves the harness's wire parsing and oracle before any venue traffic.
    fn self_check();

    /// Creates the venue state for one session.
    fn new(args: &StressArgs) -> Self;

    /// Returns the data client ID.
    fn client_id(&self) -> ClientId;

    /// Returns the WebSocket routes the proxy serves.
    fn routes(&self) -> Vec<Route>;

    /// Returns the wire handling the proxy opens for each socket.
    fn codec(&self) -> Arc<dyn WireCodec>;

    /// Returns proxy routes for other paths, such as a REST proxy.
    fn router(&self) -> Option<Router> {
        None
    }

    /// Builds the data client against the proxy at `proxy`.
    ///
    /// # Errors
    ///
    /// Returns an error if the client configuration is invalid.
    fn client(&self, proxy: SocketAddr, args: &StressArgs) -> anyhow::Result<Box<dyn DataClient>>;

    /// Returns the fault key the wire handling uses for a book.
    fn key(&self, instrument_id: &InstrumentId) -> String;

    /// Returns the reconnect endpoint that carries a book.
    fn endpoint(&self, _instrument_id: &InstrumentId) -> &'static str {
        self.routes()[0].endpoint
    }

    /// Returns subscription params for a book.
    fn params(&self, _instrument_id: &InstrumentId) -> Option<Params> {
        None
    }

    /// Checks one batch the checker accepted against the venue oracle.
    fn verify(&mut self, checker: &mut BookStreamChecker, deltas: &OrderBookDeltas);

    /// Returns whether a book streams as healthy, given its progress now and when the wait began.
    ///
    /// [`Session::healthy`] also requires the book to reach its expected snapshot count.
    fn streaming(
        &self,
        instrument_id: &InstrumentId,
        book: &BookProgress,
        start: &BookProgress,
    ) -> bool;

    /// Runs periodic oracle work while the session waits.
    fn poll(&mut self) {}

    /// Returns venue report fields.
    fn stats(&self) -> String {
        String::new()
    }

    /// Runs venue assertions after the client disconnects.
    fn finish(&mut self) {}
}

/// How a harness oracle counts toward a run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Coverage {
    /// Each batch is verified through [`BookStreamChecker::verify`], and every snapshot episode
    /// must be verified by the time the session stops.
    Episodes,
    /// Oracle samples are matched after the fact; the venue asserts its own sample counts.
    Samples,
}

/// Emitted batches for one book.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct BookProgress {
    /// Snapshot batches.
    pub(crate) snapshots: usize,
    /// All batches.
    pub(crate) batches: usize,
    /// Incremental batches since the latest snapshot.
    pub(crate) updates: usize,
}

/// One data client behind a [`FaultProxy`], with every emitted batch checked.
pub(crate) struct Session<V: StressVenue> {
    args: StressArgs,
    venue: V,
    client: Box<dyn DataClient>,
    proxy: FaultProxy,
    registry: SocketReconnectRegistry,
    events: tokio::sync::mpsc::UnboundedReceiver<DataEvent>,
    checker: BookStreamChecker,
    instruments: AHashSet<InstrumentId>,
    requested: AHashSet<InstrumentId>,
    closed: AHashSet<InstrumentId>,
    books: AHashMap<InstrumentId, BookProgress>,
    expected: AHashMap<InstrumentId, usize>,
    heal_ms_max: u128,
}

impl<V: StressVenue> Session<V> {
    /// Starts a proxy, builds the venue data client against it, and connects.
    ///
    /// # Panics
    ///
    /// Panics if the client cannot be built or does not connect within 60 seconds.
    pub(crate) async fn connect(args: &StressArgs) -> Self {
        let venue = V::new(args);
        let proxy = FaultProxy::start(venue.routes(), venue.codec(), venue.router()).await;
        let (sender, events) = tokio::sync::mpsc::unbounded_channel();
        replace_data_event_sender(sender);
        let registry = SocketReconnectRegistry::default();
        let mut client = registry
            .scope(|| venue.client(proxy.addr(), args))
            .expect("data client builds");

        eprintln!(
            "Connecting {} market data through {}, snapshot_timeout={}",
            V::NAME,
            proxy.addr(),
            args.timeout_secs()
        );
        dst::time::timeout(Duration::from_secs(60), client.connect())
            .await
            .expect("client connects within 60 seconds")
            .expect("client connects");

        let mut session = Self {
            args: args.clone(),
            venue,
            client,
            proxy,
            registry,
            events,
            checker: BookStreamChecker::new(BookType::L2_MBP, V::SEQUENCED),
            instruments: AHashSet::new(),
            requested: AHashSet::new(),
            closed: AHashSet::new(),
            books: AHashMap::new(),
            expected: AHashMap::new(),
            heal_ms_max: 0,
        };

        session.drain();
        session
    }

    /// Returns the venue state.
    #[must_use]
    pub(crate) const fn venue(&self) -> &V {
        &self.venue
    }

    /// Returns the venue state mutably.
    pub(crate) const fn venue_mut(&mut self) -> &mut V {
        &mut self.venue
    }

    /// Returns the fault proxy.
    #[must_use]
    pub(crate) const fn proxy(&self) -> &FaultProxy {
        &self.proxy
    }

    /// Returns the fault rules and counters for a book, creating them if absent.
    pub(crate) fn fault(&self, instrument_id: &InstrumentId) -> MappedMutexGuard<'_, Fault> {
        self.proxy.fault(&self.venue.key(instrument_id))
    }

    /// Returns the batches emitted for a book.
    #[must_use]
    pub(crate) fn book(&self, instrument_id: &InstrumentId) -> BookProgress {
        self.books.get(instrument_id).copied().unwrap_or_default()
    }

    /// Returns the batches emitted in this session.
    #[must_use]
    pub(crate) fn batches(&self) -> usize {
        self.books.values().map(|book| book.batches).sum()
    }

    /// Returns the instruments the client has loaded.
    #[must_use]
    pub(crate) const fn instruments(&self) -> &AHashSet<InstrumentId> {
        &self.instruments
    }

    /// Subscribes a book, which must then emit a new snapshot.
    ///
    /// # Panics
    ///
    /// Panics if the client rejects the command.
    pub(crate) fn subscribe(&mut self, instrument_id: InstrumentId) {
        self.closed.remove(&instrument_id);
        self.expect_snapshot(instrument_id);
        self.requested.insert(instrument_id);
        self.checker.open(instrument_id);
        let params = self.venue.params(&instrument_id);

        self.client
            .subscribe_book_deltas(SubscribeBookDeltas::new(
                instrument_id,
                BookType::L2_MBP,
                Some(self.venue.client_id()),
                None,
                UUID4::new(),
                UnixNanos::default(),
                None,
                true,
                None,
                params,
            ))
            .expect("subscribe command accepted");
    }

    /// Sends an unsubscribe command without closing the book to further output.
    ///
    /// # Panics
    ///
    /// Panics if the client rejects the command.
    pub(crate) fn unsubscribe(&mut self, instrument_id: InstrumentId) {
        self.client
            .unsubscribe_book_deltas(&UnsubscribeBookDeltas::new(
                instrument_id,
                Some(self.venue.client_id()),
                None,
                UUID4::new(),
                UnixNanos::default(),
                None,
                None,
            ))
            .expect("unsubscribe command accepted");
    }

    /// Treats a book's unsubscribe as settled, after which the book must emit nothing.
    pub(crate) fn close(&mut self, instrument_id: InstrumentId) {
        self.drain();
        self.closed.insert(instrument_id);
        self.checker.close(instrument_id);
    }

    /// Requires a book to emit one more snapshot than it has so far.
    pub(crate) fn expect_snapshot(&mut self, instrument_id: InstrumentId) {
        let snapshots = self.book(&instrument_id).snapshots;
        self.expected.insert(instrument_id, snapshots + 1);
    }

    /// Requires every open book to emit a new snapshot.
    pub(crate) fn expect_all(&mut self) {
        let open = self.open_books().collect::<Vec<_>>();

        for instrument_id in open {
            self.expect_snapshot(instrument_id);
        }
    }

    /// Requests a reconnect of `endpoint`, requiring a new snapshot from each open book on it.
    ///
    /// A request during an in-flight reconnect joins it.
    ///
    /// # Panics
    ///
    /// Panics if the endpoint has no reconnect handle or refuses the request.
    pub(crate) fn reconnect(&mut self, endpoint: &'static str) -> ReconnectRequestOutcome {
        let books = self
            .open_books()
            .filter(|instrument_id| self.venue.endpoint(instrument_id) == endpoint)
            .collect::<Vec<_>>();

        for instrument_id in books {
            self.expect_snapshot(instrument_id);
        }

        self.reconnect_resuming(endpoint)
    }

    /// Requests a reconnect of `endpoint` whose venue resumes each book in place, so no book on it
    /// needs a new snapshot.
    ///
    /// A request during an in-flight reconnect joins it.
    ///
    /// # Panics
    ///
    /// Panics if the endpoint has no reconnect handle or refuses the request.
    pub(crate) fn reconnect_resuming(&self, endpoint: &'static str) -> ReconnectRequestOutcome {
        let handle = self
            .registry
            .handle(self.venue.client_id(), Ustr::from(endpoint))
            .unwrap_or_else(|| panic!("reconnect handle registered for {endpoint}"));
        let outcome = handle.request_reconnect();
        assert!(
            matches!(
                outcome,
                ReconnectRequestOutcome::Accepted | ReconnectRequestOutcome::AlreadyReconnecting
            ),
            "reconnect request refused: {outcome:?}"
        );
        outcome
    }

    /// Applies emitted batches until `predicate` holds.
    ///
    /// # Panics
    ///
    /// Panics with the book and fault state if `limit` elapses first.
    pub(crate) async fn until(
        &mut self,
        limit: Duration,
        label: &str,
        predicate: impl Fn(&Self) -> bool,
    ) {
        let result = dst::time::timeout(limit, async {
            let mut polled = Instant::now();

            while !predicate(self) {
                tokio::select! {
                    biased;

                    event = self.events.recv() => {
                        self.apply(event.expect("data event stream stays open"));
                    }
                    () = dst::time::sleep(Duration::from_millis(10)) => {}
                }

                // Busy streams must not starve the oracle comparisons
                if polled.elapsed() >= POLL_INTERVAL {
                    self.venue.poll();
                    polled = Instant::now();
                }
            }
        })
        .await;

        if result.is_err() {
            self.venue.poll();

            // The fault guard must drop before `stats` locks the faults again
            let faults = format!("{:?}", *self.proxy.faults());
            let stats = self.stats();
            panic!(
                "deadline exceeded: {label}, books={:?}, expected={:?}, faults={faults}, \
                 upstream_frames={:?}, {stats}",
                self.books,
                self.expected,
                self.proxy.frames(),
            );
        }

        self.drain();
    }

    /// Applies emitted batches for `duration`.
    pub(crate) async fn observe(&mut self, duration: Duration) {
        let end = Instant::now() + duration;
        self.until(
            duration + Duration::from_secs(2),
            "observation window",
            |_| Instant::now() >= end,
        )
        .await;
    }

    /// Waits up to the venue's healthy limit for each open book in `instrument_ids` to heal.
    ///
    /// A book heals once it reaches its expected snapshot count and the venue's
    /// [`StressVenue::streaming`] condition holds.
    pub(crate) async fn healthy(&mut self, instrument_ids: &[InstrumentId]) {
        self.healthy_within(instrument_ids, V::HEALTHY_LIMIT).await;
    }

    /// Waits up to `limit` for each open book in `instrument_ids` to heal.
    ///
    /// # Panics
    ///
    /// Panics if a book in `instrument_ids` was never subscribed or `limit` elapses first.
    pub(crate) async fn healthy_within(
        &mut self,
        instrument_ids: &[InstrumentId],
        limit: Duration,
    ) {
        let started = Instant::now();
        let books = instrument_ids
            .iter()
            .filter(|instrument_id| !self.closed.contains(instrument_id))
            .map(|instrument_id| (*instrument_id, self.book(instrument_id)))
            .collect::<Vec<_>>();

        self.until(limit, "all intended books recover", |s| {
            books.iter().all(|(instrument_id, start)| {
                let book = s.book(instrument_id);
                book.snapshots >= s.expected[instrument_id]
                    && s.venue.streaming(instrument_id, &book, start)
            })
        })
        .await;

        self.heal_ms_max = self.heal_ms_max.max(started.elapsed().as_millis());
    }

    /// Prints a `ROUND` line for round `index`, counting from zero.
    pub(crate) fn round(&self, index: usize, fields: &str) {
        emit(&[
            &format!("ROUND round={}/{}", index + 1, self.args.rounds()),
            fields,
            &self.stats(),
            &elapsed(),
        ]);
    }

    /// Disconnects, checks that every socket and reconnect handle is released, prints a
    /// `SHUTDOWN` line, and runs the coverage and venue assertions.
    ///
    /// Returns the final report fields.
    ///
    /// # Panics
    ///
    /// Panics if shutdown leaves a socket or handle behind, or a coverage assertion fails.
    pub(crate) async fn stop(mut self) -> String {
        let started = Instant::now();
        dst::time::timeout(Duration::from_secs(10), self.client.disconnect())
            .await
            .expect("client disconnects within 10 seconds")
            .expect("client disconnects");
        assert!(self.client.is_disconnected());

        let deadline = Instant::now() + Duration::from_secs(5);

        while self.proxy.active() > 0 {
            assert!(
                Instant::now() < deadline,
                "proxied sockets close after disconnect"
            );
            dst::time::sleep(POLL_INTERVAL).await;
        }

        let client_id = self.venue.client_id();

        for route in self.proxy.routes() {
            assert!(
                self.registry
                    .handle(client_id, Ustr::from(route.endpoint))
                    .is_none(),
                "reconnect handle for {} released",
                route.endpoint
            );
        }

        let (episodes, verified) = self.checker.coverage();

        let coverage = match V::COVERAGE {
            Coverage::Episodes => format!("episodes={episodes} verified_episodes={verified}"),
            Coverage::Samples => format!("episodes={episodes}"),
        };

        let stats = format!("{} {coverage}", self.stats());
        emit(&[
            "SHUTDOWN",
            &format!(
                "shutdown_ms={} active_proxies=0",
                started.elapsed().as_millis()
            ),
            &stats,
        ]);

        if V::COVERAGE == Coverage::Episodes {
            assert_eq!(
                verified, episodes,
                "every snapshot episode must be verified against the oracle"
            );
        }

        self.venue.finish();
        self.proxy.stop();
        stats
    }

    /// Stops this session and connects a new one with the same arguments.
    pub(crate) async fn restart(self) -> Self {
        let args = self.args.clone();
        self.stop().await;
        Self::connect(&args).await
    }

    fn open_books(&self) -> impl Iterator<Item = InstrumentId> + '_ {
        self.requested
            .iter()
            .filter(|instrument_id| !self.closed.contains(instrument_id))
            .copied()
    }

    fn drain(&mut self) {
        while let Ok(event) = self.events.try_recv() {
            self.apply(event);
        }

        self.venue.poll();
    }

    fn apply(&mut self, event: DataEvent) {
        let deltas = match event {
            DataEvent::Instrument(instrument) => {
                self.instruments.insert(instrument.id());
                return;
            }
            DataEvent::Data(Data::BookDeltas(deltas)) => deltas,
            _ => return,
        };

        let instrument_id = deltas.instrument_id;

        if let Err(violation) = self.checker.apply(&deltas) {
            panic!(
                "book contract violation {instrument_id} seq={} ts={}: {violation}",
                deltas.sequence, deltas.ts_event
            );
        }

        let book = self.books.entry(instrument_id).or_default();
        book.batches += 1;

        if deltas
            .deltas
            .first()
            .is_some_and(|delta| delta.action == BookAction::Clear)
        {
            book.snapshots += 1;
            book.updates = 0;
        } else {
            book.updates += 1;
        }

        self.venue.verify(&mut self.checker, &deltas);
    }

    fn stats(&self) -> String {
        let snapshots = self
            .books
            .values()
            .map(|book| book.snapshots)
            .sum::<usize>();

        [
            format!("batches={} snapshots={snapshots}", self.batches()),
            self.proxy.stats(),
            format!("heal_ms_max={}", self.heal_ms_max),
            self.venue.stats(),
        ]
        .into_iter()
        .filter(|fields| !fields.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
    }
}

#[cfg(test)]
mod tests {
    use std::panic::AssertUnwindSafe;

    use futures_util::FutureExt;
    use nautilus_common::live::runner::get_data_event_sender;
    use nautilus_model::{
        data::{BookOrder, OrderBookDelta},
        enums::{OrderSide, RecordFlag},
        identifiers::Venue,
        types::{Price, Quantity},
    };
    use tokio_tungstenite::tungstenite::Message;

    use super::*;
    use crate::stress::{Upstream, WireConnection};

    // A venue whose proxy never relays a frame; its client emits scripted batches on subscribe
    struct Silent;

    impl StressVenue for Silent {
        const NAME: &'static str = "silent";
        const SCENARIOS: &'static [&'static str] = &["idle"];
        const ROUNDS: usize = 1;
        const SEQUENCED: bool = true;
        const COVERAGE: Coverage = Coverage::Samples;
        const HEALTHY_LIMIT: Duration = Duration::from_secs(1);

        fn self_check() {}

        fn new(_args: &StressArgs) -> Self {
            Self
        }

        fn client_id(&self) -> ClientId {
            ClientId::from("SILENT")
        }

        fn routes(&self) -> Vec<Route> {
            vec![Route {
                name: "silent",
                path: "/silent",
                upstream: "ws://127.0.0.1:9/silent".to_string(),
                endpoint: "silent",
                headers: &[],
            }]
        }

        fn codec(&self) -> Arc<dyn WireCodec> {
            Arc::new(SilentCodec)
        }

        fn client(
            &self,
            _proxy: SocketAddr,
            _args: &StressArgs,
        ) -> anyhow::Result<Box<dyn DataClient>> {
            Ok(Box::new(ScriptedClient))
        }

        fn key(&self, instrument_id: &InstrumentId) -> String {
            instrument_id.to_string()
        }

        fn verify(&mut self, _checker: &mut BookStreamChecker, _deltas: &OrderBookDeltas) {}

        fn streaming(
            &self,
            _instrument_id: &InstrumentId,
            _book: &BookProgress,
            _start: &BookProgress,
        ) -> bool {
            true
        }
    }

    struct SilentCodec;

    impl WireCodec for SilentCodec {
        fn open(&self, _route: &Route, _number: usize) -> Box<dyn WireConnection> {
            Box::new(SilentConnection)
        }
    }

    struct SilentConnection;

    impl WireConnection for SilentConnection {
        fn upstream(&mut self, _message: &Message) -> Upstream {
            Upstream::Other
        }
    }

    // Emits a snapshot followed by three updates for each subscribed book
    struct ScriptedClient;

    impl DataClient for ScriptedClient {
        fn client_id(&self) -> ClientId {
            ClientId::from("SILENT")
        }

        fn venue(&self) -> Option<Venue> {
            None
        }

        fn start(&mut self) -> anyhow::Result<()> {
            Ok(())
        }

        fn stop(&mut self) -> anyhow::Result<()> {
            Ok(())
        }

        fn reset(&mut self) -> anyhow::Result<()> {
            Ok(())
        }

        fn dispose(&mut self) -> anyhow::Result<()> {
            Ok(())
        }

        fn is_connected(&self) -> bool {
            true
        }

        fn is_disconnected(&self) -> bool {
            false
        }

        fn subscribe_book_deltas(&mut self, cmd: SubscribeBookDeltas) -> anyhow::Result<()> {
            let sender = get_data_event_sender();
            sender.send(snapshot(cmd.instrument_id, 1))?;

            for sequence in 2..=4 {
                sender.send(update(cmd.instrument_id, sequence))?;
            }

            Ok(())
        }
    }

    const LAST: u8 = RecordFlag::F_LAST as u8;
    const SNAPSHOT: u8 = RecordFlag::F_SNAPSHOT as u8;

    fn delta(
        instrument_id: InstrumentId,
        action: BookAction,
        side: OrderSide,
        price: &str,
        flags: u8,
        sequence: u64,
    ) -> OrderBookDelta {
        let order = BookOrder::new(side, Price::from(price), Quantity::from("1.0"), 0);
        let ts = UnixNanos::from(sequence);
        OrderBookDelta::new(instrument_id, action, order, flags, sequence, ts, ts)
    }

    fn event(instrument_id: InstrumentId, deltas: Vec<OrderBookDelta>) -> DataEvent {
        let deltas = OrderBookDeltas::new(instrument_id, deltas);
        DataEvent::Data(Data::BookDeltas(Box::new(deltas)))
    }

    fn snapshot(instrument_id: InstrumentId, sequence: u64) -> DataEvent {
        let ts = UnixNanos::from(sequence);
        let mut clear = OrderBookDelta::clear(instrument_id, sequence, ts, ts);
        clear.flags = SNAPSHOT;

        event(
            instrument_id,
            vec![
                clear,
                delta(
                    instrument_id,
                    BookAction::Add,
                    OrderSide::Buy,
                    "100.0",
                    SNAPSHOT,
                    sequence,
                ),
                delta(
                    instrument_id,
                    BookAction::Add,
                    OrderSide::Sell,
                    "101.0",
                    SNAPSHOT | LAST,
                    sequence,
                ),
            ],
        )
    }

    fn update(instrument_id: InstrumentId, sequence: u64) -> DataEvent {
        event(
            instrument_id,
            vec![delta(
                instrument_id,
                BookAction::Update,
                OrderSide::Buy,
                "100.0",
                LAST,
                sequence,
            )],
        )
    }

    fn args() -> StressArgs {
        StressArgs::parse(Vec::new(), Silent::SCENARIOS, 1, &[]).unwrap()
    }

    #[tokio::test]
    async fn subscribed_book_heals_and_rejects_output_after_close() {
        let mut session = Session::<Silent>::connect(&args()).await;
        let instrument_id = InstrumentId::from("BTCUSDT.SILENT");

        session.subscribe(instrument_id);
        session.healthy(&[instrument_id]).await;
        let healed = session.book(&instrument_id);
        session.close(instrument_id);
        get_data_event_sender()
            .send(update(instrument_id, 5))
            .unwrap();
        let late = std::panic::catch_unwind(AssertUnwindSafe(|| session.drain()));

        assert_eq!(
            healed,
            BookProgress {
                snapshots: 1,
                batches: 4,
                updates: 3,
            }
        );
        assert_eq!(session.batches(), 4);
        assert_eq!(
            *late
                .expect_err("output after close fails the checker")
                .downcast::<String>()
                .unwrap(),
            format!(
                "book contract violation {instrument_id} seq=5 ts={}: output while the book is \
                 closed",
                UnixNanos::from(5)
            )
        );
    }

    // Runs on its own thread so a deadlocked deadline fails the test instead of hanging it
    #[rstest::rstest]
    fn deadline_panics_with_the_session_state() {
        let (sender, receiver) = std::sync::mpsc::channel();

        std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();

            let outcome = runtime.block_on(async {
                let mut session = Session::<Silent>::connect(&args()).await;
                session.proxy().fault("A").dropped = 1;

                AssertUnwindSafe(session.until(Duration::from_millis(20), "idle wait", |_| false))
                    .catch_unwind()
                    .await
            });

            let message = outcome
                .expect_err("the deadline panics")
                .downcast::<String>()
                .map(|message| *message);
            let _ = sender.send(message);
        });

        let message = receiver
            .recv_timeout(std::time::Duration::from_secs(10))
            .expect("the deadline reports instead of blocking")
            .expect("the panic carries a formatted message");

        let fault = Fault {
            dropped: 1,
            ..Fault::default()
        };

        assert_eq!(
            message,
            format!(
                "deadline exceeded: idle wait, books={{}}, expected={{}}, \
                 faults={{\"A\": {fault:?}}}, upstream_frames=[(\"silent\", 0)], batches=0 \
                 snapshots=0 connections_silent=0 cuts=0 dropped=1 held=0 corrupted=0 rejected=0 \
                 upstream_failures=0 heal_ms_max=0"
            )
        );
    }
}
