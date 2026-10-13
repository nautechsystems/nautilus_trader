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
    instruments: Arc<InstrumentCache>,
    books: Arc<DashMap<InstrumentId, OrderBook>>,
    quotes: Arc<DashMap<InstrumentId, QuoteTick>>,
    quotes_subscribed: Arc<AtomicSet<InstrumentId>>,
    deltas_subscribed: Arc<AtomicSet<InstrumentId>>,
}

pub(crate) fn bench_quotes(c: &mut Criterion) {
    let instrument = test_instrument();
    let instrument_id = instrument.id();
    let initial = book_update_deltas(
        instrument_id,
        &[(dec!(99.00), dec!(1.25)), (dec!(98.50), dec!(2.00))],
        &[(dec!(100.05), dec!(0.50)), (dec!(100.50), dec!(3.00))],
        UnixNanos::new(1),
    );
    let mut group = c.benchmark_group("dydx/book_quotes");
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
                let size = if index % 2 == 0 {
                    dec!(1.75)
                } else {
                    dec!(1.50)
                };

                let bids = if changed {
                    [(dec!(99.00), size)]
                } else {
                    [(dec!(98.50), size)]
                };

                book_update_deltas(
                    instrument_id,
                    &bids,
                    &[],
                    UnixNanos::new((index + 2) as u64),
                )
            })
            .collect();

        let mut stream = QuoteStream::new(instrument.clone(), quotes, deltas);
        stream.handle(initial.clone());
        assert_eq!(stream.drain(), (usize::from(quotes), usize::from(deltas)));
        assert_eq!(
            stream.run(&mut messages.clone()),
            (
                usize::from(changed && quotes) * MESSAGE_COUNT,
                usize::from(deltas) * MESSAGE_COUNT
            )
        );
        let bid_size = Quantity::from(if changed { "1.50000000" } else { "1.25000000" });
        let timestamp = UnixNanos::new(if changed { MESSAGE_COUNT as u64 + 1 } else { 1 });

        let expected = QuoteTick::new(
            instrument_id,
            Price::from("99.00"),
            Price::from("100.05"),
            bid_size,
            Quantity::from("0.50000000"),
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
        drop(book);
        assert_eq!(
            stream.quotes.get(&instrument_id).map(|quote| *quote),
            quotes.then_some(expected)
        );

        // Include book application, quote deduplication, dispatch, and consumer draining
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

pub(crate) fn bench_crossed(c: &mut Criterion) {
    let instrument = test_instrument();
    let instrument_id = instrument.id();
    let initial = book_update_deltas(
        instrument_id,
        &[(dec!(99.00), dec!(1.25))],
        &[(dec!(100.05), dec!(0.50)), (dec!(100.50), dec!(3.00))],
        UnixNanos::new(1),
    );

    let messages: Vec<_> = (0..MESSAGE_COUNT)
        .map(|index| {
            let crossing_size = if index % 2 == 0 {
                dec!(1.75)
            } else {
                dec!(2.00)
            };

            book_update_deltas(
                instrument_id,
                &[(dec!(100.10), crossing_size)],
                &[(dec!(100.05), dec!(0.50)), (dec!(100.50), dec!(3.00))],
                UnixNanos::new((index + 2) as u64),
            )
        })
        .collect();

    let mut stream = QuoteStream::new(instrument, true, true);
    stream.handle(initial);
    assert_eq!(stream.drain(), (1, 1));
    assert_eq!(
        stream.run(&mut messages.clone()),
        (MESSAGE_COUNT, MESSAGE_COUNT)
    );
    let timestamp = UnixNanos::new(MESSAGE_COUNT as u64 + 1);

    let expected = QuoteTick::new(
        instrument_id,
        Price::from("100.10"),
        Price::from("100.50"),
        Quantity::from("1.50000000"),
        Quantity::from("3.00000000"),
        timestamp,
        timestamp,
    );
    assert_eq!(
        stream.quotes.get(&instrument_id).map(|quote| *quote),
        Some(expected)
    );
    let mut group = c.benchmark_group("dydx/crossed_book_quotes");
    group.throughput(Throughput::Elements(MESSAGE_COUNT as u64));

    // Replenish the crossed level each message to exercise synthetic repair and delivery
    group.bench_function("repair_with_deltas", |b| {
        b.iter_batched_ref(
            || messages.clone(),
            |messages| black_box(stream.run(black_box(messages))),
            BatchSize::SmallInput,
        );
    });

    group.finish();
}

pub(crate) fn bench_snapshots(c: &mut Criterion) {
    let instrument = test_instrument();
    let instrument_id = instrument.id();
    let timestamp = UnixNanos::new(1);
    let mut group = c.benchmark_group("dydx/book_snapshots");
    group.throughput(Throughput::Elements(MESSAGE_COUNT as u64));

    for depth in [2, 100] {
        let bids: Vec<_> = (0..depth)
            .map(|index| (dec!(99.00) - Decimal::from(index) * dec!(0.5), dec!(1.25)))
            .collect();
        let asks: Vec<_> = (0..depth)
            .map(|index| (dec!(100.05) + Decimal::from(index) * dec!(0.5), dec!(0.50)))
            .collect();
        let mut snapshot = book_update_deltas(instrument_id, &bids, &asks, timestamp);
        snapshot.deltas.insert(
            0,
            OrderBookDelta::clear(instrument_id, 0, timestamp, timestamp),
        );

        for delta in &mut snapshot.deltas {
            delta.flags |= RecordFlag::F_SNAPSHOT as u8;
        }

        let snapshot = OrderBookDeltas::new(instrument_id, snapshot.deltas);
        let messages = vec![snapshot.clone(); MESSAGE_COUNT];
        let mut stream = QuoteStream::new(instrument.clone(), true, true);
        stream.handle(snapshot);
        assert_eq!(stream.drain(), (1, 1));
        assert_eq!(stream.run(&mut messages.clone()), (0, MESSAGE_COUNT));
        let book = stream.books.get(&instrument_id).unwrap();
        assert_eq!(
            (book.bids(None).count(), book.asks(None).count()),
            (depth, depth)
        );
        drop(book);

        let expected = QuoteTick::new(
            instrument_id,
            Price::from("99.00"),
            Price::from("100.05"),
            Quantity::from("1.25000000"),
            Quantity::from("0.50000000"),
            timestamp,
            timestamp,
        );
        assert_eq!(
            stream.quotes.get(&instrument_id).map(|quote| *quote),
            Some(expected)
        );

        // Full snapshot replacement includes quote suppression and independent batch delivery
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
        let instruments = Arc::new(InstrumentCache::new());
        instruments.insert_instrument_only(instrument);
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
            books: Arc::new(DashMap::new()),
            quotes: Arc::new(DashMap::new()),
            quotes_subscribed,
            deltas_subscribed,
        }
    }

    fn handle(&self, deltas: OrderBookDeltas) {
        DydxDataClient::handle_deltas_message(
            deltas,
            &self.sender,
            &self.books,
            &self.quotes,
            &self.instruments,
            &self.quotes_subscribed,
            &self.deltas_subscribed,
        );
    }

    fn run(&mut self, messages: &mut Vec<OrderBookDeltas>) -> (usize, usize) {
        for message in messages.drain(..) {
            self.handle(message);
        }

        self.drain()
    }

    fn drain(&mut self) -> (usize, usize) {
        let mut quotes = 0;
        let mut deltas = 0;

        while let Ok(event) = self.receiver.try_recv() {
            match event {
                DataEvent::Data(NautilusData::Quote(quote)) => {
                    black_box(quote);
                    quotes += 1;
                }
                DataEvent::Data(NautilusData::BookDeltas(batch)) => {
                    black_box(batch);
                    deltas += 1;
                }
                other => panic!("Unexpected event: {other:?}"),
            }
        }

        (quotes, deltas)
    }
}
