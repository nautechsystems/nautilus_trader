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

// Included by data.rs as a unit-test module to exercise private recovery internals

use std::sync::atomic::{AtomicU32, AtomicUsize};

use axum::{
    Router,
    extract::{
        State, WebSocketUpgrade,
        ws::{Message, WebSocket},
    },
    response::IntoResponse,
    routing::get,
};
use nautilus_common::testing::wait_until_async;
use nautilus_model::{
    enums::RecordFlag, instruments::stubs::currency_pair_btcusdt, orderbook::OrderBook,
};
use nautilus_network::websocket::TransportBackend;
use rstest::rstest;
use serde_json::Value;

use super::*;

#[derive(Default)]
struct Faults {
    drop_snapshots: AtomicUsize,
    snapshots: AtomicUsize,
    rejections: AtomicUsize,
}

struct BookSession {
    ws: OKXWebSocketClient,
    tracker: BookSyncTracker,
    channels: Arc<AtomicMap<InstrumentId, OKXBookChannel>>,
    tasks: TaskGroup,
    events: tokio::sync::mpsc::UnboundedReceiver<DataEvent>,
    faults: Arc<Faults>,
}

impl BookSession {
    async fn connect(url: String, instrument: InstrumentAny, snapshot_timeout: Duration) -> Self {
        let mut ws = OKXWebSocketClient::new(
            Some(url),
            None,
            None,
            None,
            None,
            Some(20),
            None,
            TransportBackend::default(),
            None,
        )
        .unwrap();
        ws.cache_instruments(std::slice::from_ref(&instrument));
        ws.connect().await.unwrap();
        let stream = ws.stream();
        let channels = Arc::new(AtomicMap::new());
        let tracker = BookSyncTracker::default();
        let tasks = TaskGroup::new();
        let spawner = tasks.spawner().unwrap();
        let cancel = spawner.cancellation_token();
        let instrument_id = instrument.id();
        let instruments = Arc::new(AtomicMap::new());
        instruments.insert(instrument_id.symbol.inner(), instrument);
        let (sender, events) = tokio::sync::mpsc::unbounded_channel();
        let sender = EventSender::from(sender);
        let faults = Arc::new(Faults::default());
        let stream_faults = Arc::clone(&faults);
        let stream_ws = ws.clone();
        let stream_tracker = tracker.clone();
        let stream_channels = Arc::clone(&channels);
        spawner.clone().spawn(async move {
            let http = OKXHttpClient::default();
            let config = OKXDataClientConfig::default();
            let update_lock = InstrumentUpdateLock::default();
            let mut quotes = QuoteCache::new();
            let mut funding = AHashMap::new();
            let indices = Arc::new(AtomicMap::new());
            let greeks = Arc::new(AtomicMap::new());
            pin_mut!(stream);

            loop {
                let message = tokio::select! {
                    biased;
                    () = cancel.cancelled() => break,
                    message = stream.next() => match message { Some(message) => message, None => break },
                };

                if let OKXWsMessage::BookData { action: OKXBookAction::Snapshot, .. } = &message {
                    if stream_faults.drop_snapshots.try_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1)).is_ok() {
                        continue;
                    }

                    stream_faults.snapshots.fetch_add(1, Ordering::SeqCst);
                }

                let rejected = matches!(message, OKXWsMessage::SubscriptionFailed { .. });
                OKXDataClient::handle_ws_message(message, &sender, &instruments, &http, &config, &update_lock, &stream_channels, &stream_tracker, Some(&stream_ws), None, &mut quotes, &mut funding, &indices, &greeks, BookChannelScope::Public, snapshot_timeout, &spawner, get_atomic_clock_realtime());

                if rejected { stream_faults.rejections.fetch_add(1, Ordering::SeqCst); }
            }
        }).unwrap();

        channels.insert(instrument_id, OKXBookChannel::Book);
        tracker.record_subscription(instrument_id, Instant::now(), SnapshotGate::default());
        ws.subscribe_book(instrument_id).await.unwrap();

        Self {
            ws,
            tracker,
            channels,
            tasks,
            events,
            faults,
        }
    }

    async fn snapshot(&mut self, book: &mut OrderBook) {
        tokio::time::timeout(Duration::from_secs(30), async {
            loop {
                if let Some(DataEvent::Data(Data::BookDeltas(deltas))) = self.events.recv().await {
                    let snapshot = deltas
                        .deltas
                        .first()
                        .is_some_and(|delta| RecordFlag::F_SNAPSHOT.matches(delta.flags));

                    book.apply_deltas(&deltas).unwrap();

                    if snapshot {
                        break;
                    }
                }
            }
        })
        .await
        .expect("fresh accepted snapshot");
    }

    async fn stop(mut self) {
        self.tasks.begin_shutdown();
        self.tracker.clear();
        self.ws.close().await.unwrap();
        self.tasks
            .finish_shutdown(Duration::from_secs(3), Duration::from_secs(3))
            .await
            .unwrap();
    }
}

#[derive(Default)]
struct Venue {
    subscriptions: AtomicUsize,
    reject: AtomicBool,
    ack_before_rejection: AtomicBool,
    transient_rejections: AtomicUsize,
    transient_code: AtomicU32,
}

async fn serve_book(ws: WebSocketUpgrade, State(venue): State<Arc<Venue>>) -> impl IntoResponse {
    ws.on_upgrade(move |socket| book_socket(socket, venue))
}

async fn book_socket(mut socket: WebSocket, venue: Arc<Venue>) {
    while let Some(Ok(Message::Text(text))) = socket.recv().await {
        if text == "ping" {
            socket.send(Message::Text("pong".into())).await.unwrap();
            continue;
        }

        let request: Value = serde_json::from_str(&text).unwrap();
        if request["op"] == "unsubscribe" {
            let ack = serde_json::json!({"event": "unsubscribe", "connId": "mock", "arg": request["args"][0]});
            if socket
                .send(Message::Text(ack.to_string().into()))
                .await
                .is_err()
            {
                break;
            }

            continue;
        }

        if request["op"] != "subscribe" {
            continue;
        }

        venue.subscriptions.fetch_add(1, Ordering::SeqCst);
        let mut snapshot: Value =
            serde_json::from_str(include_str!("../../test_data/ws_books_snapshot.json")).unwrap();

        if venue
            .transient_rejections
            .try_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1))
            .is_ok()
        {
            snapshot = serde_json::json!({"event": "error", "code": venue.transient_code.load(Ordering::SeqCst).to_string(), "msg": "Temporary subscription failure", "arg": request["args"][0]});
        } else if venue.reject.load(Ordering::SeqCst) {
            snapshot = serde_json::json!({"event": "error", "code": "60018", "msg": "Channel does not exist", "arg": request["args"][0]});
        }

        if snapshot["event"] != "error" || venue.ack_before_rejection.load(Ordering::SeqCst) {
            let ack = serde_json::json!({"event": "subscribe", "connId": "mock", "arg": request["args"][0]});
            if socket
                .send(Message::Text(ack.to_string().into()))
                .await
                .is_err()
            {
                break;
            }
        }

        if socket
            .send(Message::Text(snapshot.to_string().into()))
            .await
            .is_err()
        {
            break;
        }
    }
}

async fn venue() -> (String, Arc<Venue>, tokio::task::JoinHandle<()>) {
    let venue = Arc::new(Venue::default());
    let router = Router::new()
        .route("/", get(serve_book))
        .with_state(Arc::clone(&venue));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("ws://{}/", listener.local_addr().unwrap());

    let task = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });

    (url, venue, task)
}

fn instrument() -> InstrumentAny {
    let mut instrument = currency_pair_btcusdt();
    instrument.id = InstrumentId::from("BTC-USDT.OKX");
    InstrumentAny::CurrencyPair(instrument)
}

#[tokio::test]
async fn recovery_retries_missing_snapshot_and_preserves_intent() {
    let (url, venue, server) = venue().await;
    let mut session = BookSession::connect(url, instrument(), Duration::from_secs(3)).await;
    let instrument_id = instrument().id();
    let mut book = OrderBook::new(instrument_id, BookType::L2_MBP);
    session.snapshot(&mut book).await;
    session.faults.drop_snapshots.store(1, Ordering::SeqCst);

    for _ in 0..10 {
        start_recovery(
            instrument_id,
            &session.channels,
            &session.tracker,
            Some(&session.ws),
            Duration::from_secs(3),
            &session.tasks.spawner().unwrap(),
        );
    }

    session.snapshot(&mut book).await;
    assert_eq!(venue.subscriptions.load(Ordering::SeqCst), 3);
    assert_eq!(
        session.ws.get_subscriptions(instrument_id),
        vec![OKXWsChannel::Books]
    );
    assert_eq!(session.faults.snapshots.load(Ordering::SeqCst), 2);
    session.stop().await;
    server.abort();
}

#[rstest]
#[case::normal_deadline(Duration::from_secs(1), false)]
#[case::disabled_deadline(Duration::ZERO, false)]
#[case::late_ack(Duration::from_secs(1), true)]
#[case::late_ack_disabled_deadline(Duration::ZERO, true)]
#[tokio::test]
async fn recovery_stops_on_permanent_rejection(#[case] timeout: Duration, #[case] late_ack: bool) {
    let (url, venue, server) = venue().await;
    let mut session = BookSession::connect(url, instrument(), timeout).await;
    let instrument_id = instrument().id();
    session
        .snapshot(&mut OrderBook::new(instrument_id, BookType::L2_MBP))
        .await;
    venue.reject.store(true, Ordering::SeqCst);
    venue.ack_before_rejection.store(late_ack, Ordering::SeqCst);
    start_recovery(
        instrument_id,
        &session.channels,
        &session.tracker,
        Some(&session.ws),
        timeout,
        &session.tasks.spawner().unwrap(),
    );
    wait_until_async(
        || async { session.faults.rejections.load(Ordering::SeqCst) == 1 },
        Duration::from_secs(3),
    )
    .await;

    assert_eq!(
        session.tracker.validate_sequence(
            instrument_id,
            true,
            &[(Some(-1), 20)],
            Duration::ZERO,
            Instant::now()
        ),
        BookSequenceOutcome::Suppress
    );
    assert_eq!(venue.subscriptions.load(Ordering::SeqCst), 2);
    assert_eq!(
        session.ws.get_subscriptions(instrument_id),
        vec![OKXWsChannel::Books]
    );
    assert!(session.tracker.claim_recovery(instrument_id).is_none());
    session.stop().await;
    server.abort();
}

#[rstest]
#[case::rate_limit(50011)]
#[case::websocket_rate_limit(60014)]
#[case::websocket_internal_error(64007)]
#[tokio::test]
async fn recovery_retries_transient_venue_rejection(#[case] code: u32) {
    let (url, venue, server) = venue().await;
    let mut session = BookSession::connect(url, instrument(), Duration::from_secs(3)).await;
    let instrument_id = instrument().id();
    let mut book = OrderBook::new(instrument_id, BookType::L2_MBP);
    session.snapshot(&mut book).await;
    venue.transient_code.store(code, Ordering::SeqCst);
    venue.transient_rejections.store(1, Ordering::SeqCst);
    start_recovery(
        instrument_id,
        &session.channels,
        &session.tracker,
        Some(&session.ws),
        Duration::from_secs(3),
        &session.tasks.spawner().unwrap(),
    );
    session.snapshot(&mut book).await;
    assert_eq!(venue.subscriptions.load(Ordering::SeqCst), 3);
    assert_eq!(session.faults.rejections.load(Ordering::SeqCst), 1);
    session.stop().await;
    server.abort();
}

#[tokio::test]
async fn recovery_exhaustion_suppresses_late_snapshot() {
    let (url, venue, server) = venue().await;
    let mut session = BookSession::connect(url, instrument(), Duration::from_millis(10)).await;
    let instrument_id = instrument().id();
    session
        .snapshot(&mut OrderBook::new(instrument_id, BookType::L2_MBP))
        .await;
    session
        .faults
        .drop_snapshots
        .store(usize::MAX, Ordering::SeqCst);
    let recovery_tasks = TaskGroup::new();
    start_recovery(
        instrument_id,
        &session.channels,
        &session.tracker,
        Some(&session.ws),
        Duration::from_millis(10),
        &recovery_tasks.spawner().unwrap(),
    );
    wait_until_async(
        || async { recovery_tasks.all_finished() },
        Duration::from_secs(90),
    )
    .await;

    assert_eq!(venue.subscriptions.load(Ordering::SeqCst), 9);
    assert_eq!(
        session.tracker.validate_sequence(
            instrument_id,
            true,
            &[(Some(-1), 20)],
            Duration::ZERO,
            Instant::now()
        ),
        BookSequenceOutcome::Suppress
    );
    assert!(matches!(
        session.events.try_recv(),
        Err(tokio::sync::mpsc::error::TryRecvError::Empty)
    ));
    assert_eq!(
        session.ws.get_subscriptions(instrument_id),
        vec![OKXWsChannel::Books]
    );
    session.stop().await;
    server.abort();
}

#[tokio::test]
async fn recovery_shutdown_cancels_snapshot_wait_without_losing_intent() {
    let (url, venue, server) = venue().await;
    let mut session = BookSession::connect(url, instrument(), Duration::ZERO).await;
    let instrument_id = instrument().id();
    session
        .snapshot(&mut OrderBook::new(instrument_id, BookType::L2_MBP))
        .await;
    session
        .faults
        .drop_snapshots
        .store(usize::MAX, Ordering::SeqCst);
    let recovery_tasks = TaskGroup::new();
    start_recovery(
        instrument_id,
        &session.channels,
        &session.tracker,
        Some(&session.ws),
        Duration::ZERO,
        &recovery_tasks.spawner().unwrap(),
    );
    wait_until_async(
        || async { venue.subscriptions.load(Ordering::SeqCst) == 2 },
        Duration::from_secs(3),
    )
    .await;

    recovery_tasks.begin_shutdown();
    recovery_tasks
        .finish_shutdown(Duration::from_secs(1), Duration::from_secs(1))
        .await
        .unwrap();
    assert_eq!(venue.subscriptions.load(Ordering::SeqCst), 2);
    assert_eq!(
        session.ws.get_subscriptions(instrument_id),
        vec![OKXWsChannel::Books]
    );
    session.stop().await;
    server.abort();
}
