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

use criterion::{Criterion, Throughput};

use super::*;

const MESSAGE_COUNT: usize = 128;

#[derive(Debug)]
struct QuoteStream {
    client: BybitDataClient,
    ws: BybitWebSocketClient,
    tasks: TaskSpawner,
    instruments: AHashMap<Ustr, InstrumentAny>,
    product_type: BybitProductType,
    funding_cache: AHashMap<Ustr, super::super::FundingCacheEntry>,
    receiver: tokio::sync::mpsc::UnboundedReceiver<DataEvent>,
    clock: AtomicTime,
}

pub(crate) fn bench_quotes(c: &mut Criterion) {
    let instrument = option_instrument();
    let instrument_id = instrument.id();
    let mut initial: BybitWsTickerOptionMsg =
        serde_json::from_str(include_str!("../../test_data/ws_ticker_option.json")).unwrap();
    initial.data.symbol = instrument.raw_symbol().inner();
    initial.data.bid_price = "9".to_string();
    initial.data.ask_price = "10".to_string();
    initial.data.bid_size = "2".to_string();
    initial.data.ask_size = "5".to_string();
    let mut group = c.benchmark_group("bybit/option_quotes");
    group.throughput(Throughput::Elements(MESSAGE_COUNT as u64));

    let mut stream = QuoteStream::new(instrument);

    for (name, changed, marks) in [
        ("unchanged", false, false),
        ("changed", true, false),
        ("unchanged_with_marks", false, true),
        ("changed_with_marks", true, true),
    ] {
        let messages: Vec<_> = (0..MESSAGE_COUNT)
            .map(|index| {
                let mut message = initial.clone();
                message.ts += (index + 1) as i64;
                if changed {
                    message.data.bid_size = if index % 2 == 0 { "3" } else { "2" }.to_string();
                }

                BybitWsMessage::TickerOption(message)
            })
            .collect();

        stream.client.last_quotes.clear();
        let mut subscriptions = AHashSet::from_iter(["quotes"]);
        if marks {
            subscriptions.insert("mark_prices");
        }

        stream
            .client
            .ticker_subs
            .insert(instrument_id, subscriptions);
        stream.handle(&BybitWsMessage::TickerOption(initial.clone()));
        assert_eq!(stream.drain(), (1, usize::from(marks)));
        let counts = stream.run(&messages);
        assert_eq!(
            counts,
            (
                if changed { MESSAGE_COUNT } else { 0 },
                if marks { MESSAGE_COUNT } else { 0 }
            )
        );
        let cached = *stream.client.last_quotes.get(&instrument_id).unwrap();
        assert_eq!(
            cached,
            QuoteTick::new(
                instrument_id,
                Price::from("9.0"),
                Price::from("10.0"),
                Quantity::from("2"),
                Quantity::from("5"),
                UnixNanos::new(
                    (initial.ts + if changed { MESSAGE_COUNT as i64 } else { 0 }) as u64
                        * 1_000_000
                ),
                UnixNanos::new(1_700_000_000_000_000_000),
            )
        );

        // Include parsing, deduplication, channel dispatch, and consumer draining
        group.bench_function(name, |b| {
            b.iter(|| black_box(stream.run(black_box(&messages))));
        });
    }

    group.finish();
    bench_books(c, &mut stream);
}

fn bench_books(c: &mut Criterion, stream: &mut QuoteStream) {
    let instrument = linear_instrument();
    let instrument_id = instrument.id();
    stream.instruments = build_instruments(&[instrument]);
    stream.product_type = BybitProductType::Linear;
    stream.client.quote_subs.insert(instrument_id);
    let mut initial = orderbook_msg("snapshot", 123_456_789, true);
    initial.topic = "orderbook.1.BTCUSDT".into();
    stream.handle(&BybitWsMessage::Orderbook(initial.clone()));

    let DataEvent::Data(Data::Quote(quote)) = stream.receiver.try_recv().unwrap() else {
        panic!("Expected quote");
    };

    assert_eq!(
        quote,
        QuoteTick::new(
            instrument_id,
            Price::from("27450.00"),
            Price::from("27451.00"),
            Quantity::from("0.500"),
            Quantity::from("0.750"),
            UnixNanos::new(1_709_891_679_000_000_000),
            UnixNanos::new(1_700_000_000_000_000_000),
        )
    );
    stream.receiver.try_recv().unwrap_err();
    let mut group = c.benchmark_group("bybit/book_quotes");
    group.throughput(Throughput::Elements(MESSAGE_COUNT as u64));

    for (name, changed) in [("unchanged", false), ("changed", true)] {
        let messages: Vec<_> = (0..MESSAGE_COUNT)
            .map(|index| {
                let mut message = initial.clone();
                message.ts += (index + 1) as i64;
                if changed {
                    message.data.b[0][1] =
                        if index % 2 == 0 { "0.600" } else { "0.500" }.to_string();
                }

                BybitWsMessage::Orderbook(message)
            })
            .collect();

        assert_eq!(stream.run(&messages), (MESSAGE_COUNT, 0));

        // Depth-one book feeds deliver every quote, including unchanged market fields
        group.bench_function(name, |b| {
            b.iter(|| black_box(stream.run(black_box(&messages))));
        });
    }

    group.finish();
}

impl QuoteStream {
    fn new(instrument: InstrumentAny) -> Self {
        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
        set_data_event_sender(sender);

        let client = BybitDataClient::new(
            *BYBIT_CLIENT_ID,
            BybitDataClientConfig {
                product_types: vec![BybitProductType::Option],
                ..Default::default()
            },
        )
        .unwrap();

        let ws = client
            .get_ws_client_for_product(BybitProductType::Option)
            .unwrap()
            .clone();
        let tasks = client.command_tasks.spawner().unwrap();

        Self {
            client,
            ws,
            tasks,
            instruments: build_instruments(&[instrument]),
            product_type: BybitProductType::Option,
            funding_cache: AHashMap::new(),
            receiver,
            clock: AtomicTime::new(false, UnixNanos::new(1_700_000_000_000_000_000)),
        }
    }

    fn run(&mut self, messages: &[BybitWsMessage]) -> (usize, usize) {
        for message in messages {
            self.handle(message);
        }

        self.drain()
    }

    fn handle(&mut self, message: &BybitWsMessage) {
        handle_ws_message(
            message,
            &self.client.data_sender,
            &self.instruments,
            Some(self.product_type),
            &self.client.trade_subs,
            &self.client.ticker_subs,
            &self.client.quote_subs,
            &self.client.book_depths,
            &self.client.book_sync,
            &self.ws,
            &self.client.option_greeks_subs,
            &self.client.liquidation_subs,
            self.ws.bar_types_cache(),
            true,
            &self.client.last_quotes,
            &mut self.funding_cache,
            Duration::ZERO,
            &self.tasks,
            &self.clock,
        );
    }

    fn drain(&mut self) -> (usize, usize) {
        let mut quotes = 0;
        let mut marks = 0;

        while let Ok(event) = self.receiver.try_recv() {
            match event {
                DataEvent::Data(Data::Quote(quote)) => {
                    black_box(quote);
                    quotes += 1;
                }
                DataEvent::Data(Data::MarkPrice(mark)) => {
                    black_box(mark);
                    marks += 1;
                }
                other => panic!("Unexpected event: {other:?}"),
            }
        }

        (quotes, marks)
    }
}
