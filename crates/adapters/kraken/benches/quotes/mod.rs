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

use std::hint::black_box;

use criterion::{BatchSize, Criterion, Throughput};

use super::*;

const MESSAGE_COUNT: usize = 128;

#[derive(Debug)]
struct QuoteStream {
    sender: EventSender<DataEvent>,
    receiver: tokio::sync::mpsc::UnboundedReceiver<DataEvent>,
    instruments: Arc<AtomicMap<InstrumentId, InstrumentAny>>,
    quotes_subscribed: Arc<AtomicSet<InstrumentId>>,
    deltas_subscribed: Arc<AtomicSet<InstrumentId>>,
    books: AHashMap<InstrumentId, OrderBook>,
    quotes: DashMap<InstrumentId, QuoteTick>,
    sequence: Arc<AtomicU64>,
    clock: &'static AtomicTime,
}

pub(crate) fn bench_quotes(c: &mut Criterion) {
    let instrument = futures_instrument();
    let instrument_id = instrument.id();
    let initial: KrakenFuturesBookSnapshot = serde_json::from_str(include_str!(
        "../../test_data/ws_futures_book_snapshot.json"
    ))
    .unwrap();
    let mut group = c.benchmark_group("kraken/futures_book_quotes");
    group.throughput(Throughput::Elements(MESSAGE_COUNT as u64));

    for (name, quotes, deltas, changed) in [
        ("depth_only", true, false, false),
        ("top_changed", true, false, true),
        ("depth_only_with_deltas", true, true, false),
        ("top_changed_with_deltas", true, true, true),
        ("book_only", false, true, true),
    ] {
        let messages: Vec<_> = (0..MESSAGE_COUNT)
            .map(|index| {
                KrakenFuturesWsMessage::BookDelta(KrakenFuturesBookDelta {
                    feed: KrakenFuturesFeed::Book,
                    product_id: initial.product_id,
                    side: KrakenOrderSide::Buy,
                    seq: initial.seq + (index + 1) as i64,
                    price: if changed { dec!(34892.5) } else { dec!(34892) },
                    qty: if index % 2 == 0 {
                        dec!(7000)
                    } else {
                        dec!(7100)
                    },
                    timestamp: initial.timestamp + (index + 1) as i64,
                })
            })
            .collect();

        let mut stream = QuoteStream::new(instrument.clone(), quotes, deltas);
        stream.handle(KrakenFuturesWsMessage::BookSnapshot(initial.clone()));
        assert_eq!(stream.drain(), (usize::from(quotes), usize::from(deltas)));
        assert_eq!(
            stream.run(&mut messages.clone()),
            (
                usize::from(changed && quotes) * MESSAGE_COUNT,
                usize::from(deltas) * MESSAGE_COUNT
            )
        );
        let bid_size = Quantity::from(if changed { "7100" } else { "6385" });
        let timestamp = UnixNanos::new(if changed { MESSAGE_COUNT as u64 + 1 } else { 1 });

        let expected = QuoteTick::new(
            instrument_id,
            Price::from("34892.5"),
            Price::from("34911.5"),
            bid_size,
            Quantity::from("20598"),
            timestamp,
            timestamp,
        );
        let book = stream.books.get(&instrument_id).unwrap();
        assert_eq!(
            (
                book.best_bid_price(),
                book.best_ask_price(),
                book.best_bid_size(),
                book.best_ask_size()
            ),
            (
                Some(expected.bid_price),
                Some(expected.ask_price),
                Some(expected.bid_size),
                Some(expected.ask_size)
            ),
        );
        assert_eq!(
            stream.quotes.get(&instrument_id).map(|quote| *quote),
            quotes.then_some(expected)
        );

        // Include wire conversion, book application, deduplication, dispatch, and draining
        group.bench_function(name, |b| {
            b.iter_batched_ref(
                || messages.clone(),
                |messages| black_box(stream.run(black_box(messages))),
                BatchSize::SmallInput,
            );
        });
    }

    group.finish();
}

pub(crate) fn bench_snapshots(c: &mut Criterion) {
    let instrument = futures_instrument();
    let instrument_id = instrument.id();
    let initial: KrakenFuturesBookSnapshot = serde_json::from_str(include_str!(
        "../../test_data/ws_futures_book_snapshot.json"
    ))
    .unwrap();
    let mut group = c.benchmark_group("kraken/futures_book_snapshots");
    group.throughput(Throughput::Elements(MESSAGE_COUNT as u64));

    for depth in [2, 100] {
        let mut snapshot = initial.clone();
        snapshot.bids = (0..depth)
            .map(|index| {
                let mut level = initial.bids[0].clone();
                level.price -= Decimal::from(index) * dec!(0.5);
                level.qty += Decimal::from(index);
                level
            })
            .collect();

        snapshot.asks = (0..depth)
            .map(|index| {
                let mut level = initial.asks[0].clone();
                level.price += Decimal::from(index) * dec!(0.5);
                level.qty += Decimal::from(index);
                level
            })
            .collect();

        let messages = vec![KrakenFuturesWsMessage::BookSnapshot(snapshot.clone()); MESSAGE_COUNT];
        let mut stream = QuoteStream::new(instrument.clone(), true, true);
        stream.handle(KrakenFuturesWsMessage::BookSnapshot(snapshot));
        assert_eq!(stream.drain(), (1, 1));
        assert_eq!(stream.run(&mut messages.clone()), (0, MESSAGE_COUNT));
        let book = stream.books.get(&instrument_id).unwrap();
        assert_eq!(
            (book.bids(None).count(), book.asks(None).count()),
            (depth, depth)
        );
        let timestamp = UnixNanos::new(1);

        let expected = QuoteTick::new(
            instrument_id,
            Price::from("34892.5"),
            Price::from("34911.5"),
            Quantity::from("6385"),
            Quantity::from("20598"),
            timestamp,
            timestamp,
        );
        assert_eq!(
            stream.quotes.get(&instrument_id).map(|quote| *quote),
            Some(expected)
        );

        // Exclude input cloning; measure replacement and delivery at fixture and live-like depth
        group.bench_function(format!("depth_{depth}"), |b| {
            b.iter_batched_ref(
                || messages.clone(),
                |messages| black_box(stream.run(black_box(messages))),
                BatchSize::SmallInput,
            );
        });
    }

    group.finish();
}

impl QuoteStream {
    fn new(instrument: InstrumentAny, quotes: bool, deltas: bool) -> Self {
        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
        let instrument_id = instrument.id();
        let instruments = Arc::new(AtomicMap::new());
        instruments.insert(instrument_id, instrument);
        let quotes_subscribed = Arc::new(AtomicSet::new());
        if quotes {
            quotes_subscribed.insert(instrument_id);
        }

        let deltas_subscribed = Arc::new(AtomicSet::new());
        if deltas {
            deltas_subscribed.insert(instrument_id);
        }

        Self {
            sender: sender.into(),
            receiver,
            instruments,
            quotes_subscribed,
            deltas_subscribed,
            books: AHashMap::new(),
            quotes: DashMap::new(),
            sequence: Arc::new(AtomicU64::new(0)),
            clock: Box::leak(Box::new(AtomicTime::new(false, UnixNanos::new(1)))),
        }
    }

    fn handle(&mut self, message: KrakenFuturesWsMessage) {
        KrakenFuturesDataClient::handle_ws_message(
            message,
            &self.sender,
            &self.instruments,
            &self.quotes_subscribed,
            &self.deltas_subscribed,
            &mut self.books,
            &self.quotes,
            &self.sequence,
            self.clock,
        );
    }

    fn run(&mut self, messages: &mut Vec<KrakenFuturesWsMessage>) -> (usize, usize) {
        for (index, message) in messages.drain(..).enumerate() {
            self.clock.set_time(UnixNanos::new((index + 2) as u64));
            self.handle(message);
        }

        self.drain()
    }

    fn drain(&mut self) -> (usize, usize) {
        let mut quotes = 0;
        let mut deltas = 0;

        while let Ok(event) = self.receiver.try_recv() {
            match event {
                DataEvent::Data(Data::Quote(quote)) => {
                    black_box(quote);
                    quotes += 1;
                }
                DataEvent::Data(Data::BookDeltas(batch)) => {
                    black_box(batch);
                    deltas += 1;
                }
                other => panic!("Unexpected event: {other:?}"),
            }
        }

        (quotes, deltas)
    }
}
