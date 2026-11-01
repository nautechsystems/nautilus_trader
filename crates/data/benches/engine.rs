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

//! Benchmarks for the `DataEngine` ingestion path.
//!
//! Targets live pipeline stages:
//!
//! - `DataEngine::process_data(Data::Trade(..))` -> `handle_trade` -> `Cache::add_trade`
//!   -> `msgbus::publish_trade` (publish has no subscribers in this bench).
//! - Direct `Cache::add_trade` to isolate cache write cost from engine plus publish.
//! - `DataEngine::process_data(Data::BookDelta(..))` -> `handle_delta` -> wrap into
//!   `OrderBookDeltas` -> `msgbus::publish_deltas`. The default path wraps every
//!   incremental delta; the buffered path flushes on `F_LAST`.
//! - External market-data decoding and publication, with fallback forwarding, application topics,
//!   and engine routing through cache writes or managed book updates.
//!
//! Run with `cargo bench -p nautilus-data --bench engine`. Add
//! `--features high-precision,nautilus-common/sbe` to include high-precision SBE ingress.

use std::{
    cell::{Cell, RefCell},
    hint::black_box,
    rc::Rc,
};

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use nautilus_common::{
    cache::Cache,
    clock::{Clock, VirtualClock},
    enums::SerializationEncoding,
    messages::data::{SubscribeBookDeltas, SubscribeBookDepth, SubscribeCommand},
    msgbus::{
        self, BusMessage, BusMessageSource, BusPayloadType, MStr, MessageBus,
        MessageBusExternalEgress, Topic, TypedHandler, get_message_bus, publish_deltas,
        publish_depth, publish_quote, publish_trade, republish_external_message,
        subscribe_book_deltas, subscribe_book_depth, subscribe_quotes, subscribe_trades,
        switchboard::{
            get_book_deltas_topic, get_book_depth_topic, get_quotes_topic, get_trades_topic,
        },
    },
};
use nautilus_core::UUID4;
use nautilus_data::engine::{DataEngine, config::DataEngineConfig};
use nautilus_model::{
    data::{
        BookOrder, Data, OrderBookDelta, OrderBookDeltas, OrderBookDepth, QuoteTick,
        stubs::{stub_deltas, stub_depth10},
        trade::TradeTick,
    },
    enums::{AggressorSide, BookAction, BookType, OrderSide, RecordFlag},
    identifiers::{InstrumentId, TradeId},
    orderbook::OrderBook,
    types::{Price, Quantity},
};

const DELTA_FRAME_SIZE: usize = 8;

fn sample_trade() -> TradeTick {
    TradeTick {
        instrument_id: InstrumentId::from("EUR/USD.SIM"),
        price: Price::from("1.10000"),
        size: Quantity::from(100_000),
        aggressor_side: AggressorSide::Buy,
        trade_id: TradeId::from("123456"),
        ts_event: 0.into(),
        ts_init: 0.into(),
    }
}

fn sample_delta(flags: u8) -> OrderBookDelta {
    OrderBookDelta::new(
        InstrumentId::from("EUR/USD.SIM"),
        BookAction::Update,
        BookOrder::new(
            OrderSide::Buy,
            Price::from("1.10000"),
            Quantity::from(100_000),
            1,
        ),
        flags,
        1,
        0.into(),
        0.into(),
    )
}

fn install_thread_local_msgbus() {
    msgbus::set_message_bus(Rc::new(RefCell::new(MessageBus::default())));
}

fn build_engine() -> Rc<RefCell<DataEngine>> {
    build_engine_with_config(None)
}

fn build_engine_with_config(config: Option<DataEngineConfig>) -> Rc<RefCell<DataEngine>> {
    install_thread_local_msgbus();
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache = Rc::new(RefCell::new(Cache::default()));
    let engine = Rc::new(RefCell::new(DataEngine::new(clock, cache, config)));
    DataEngine::register_msgbus_handlers(&engine);
    engine
}

fn bench_process_trade(c: &mut Criterion) {
    let mut group = c.benchmark_group("DataEngine ingest");
    group.throughput(Throughput::Elements(1));

    group.bench_function("process_data_trade", |b| {
        let engine = build_engine();
        let trade = sample_trade();
        b.iter(|| {
            engine
                .borrow_mut()
                .process_data(Data::Trade(black_box(trade)));
        });
    });

    group.bench_function("cache_add_trade_only", |b| {
        install_thread_local_msgbus();
        let cache = Rc::new(RefCell::new(Cache::default()));
        let trade = sample_trade();
        b.iter(|| {
            cache
                .borrow_mut()
                .add_trade(black_box(trade))
                .expect("add_trade");
        });
    });

    group.finish();
}

fn bench_process_trade_batch(c: &mut Criterion) {
    let mut group = c.benchmark_group("DataEngine ingest batch");

    for size in [1_000_usize, 10_000, 100_000] {
        group.throughput(Throughput::Elements(size as u64));
        group.bench_with_input(
            criterion::BenchmarkId::new("process_data_trade", size),
            &size,
            |b, &n| {
                let engine = build_engine();
                let trade = sample_trade();
                b.iter(|| {
                    let mut e = engine.borrow_mut();
                    for _ in 0..n {
                        e.process_data(Data::Trade(black_box(trade)));
                    }
                });
            },
        );
    }

    group.finish();
}

fn bench_process_delta(c: &mut Criterion) {
    let mut group = c.benchmark_group("DataEngine ingest");
    group.throughput(Throughput::Elements(1));

    // Default config publishes each incremental delta as its own batch
    group.bench_function("process_data_delta", |b| {
        let engine = build_engine();
        let delta = sample_delta(RecordFlag::F_LAST as u8);
        b.iter(|| {
            engine
                .borrow_mut()
                .process_data(Data::BookDelta(black_box(delta)));
        });
    });

    group.finish();
}

fn bench_process_delta_batch(c: &mut Criterion) {
    let mut group = c.benchmark_group("DataEngine ingest batch");

    for size in [1_000_usize, 10_000, 100_000] {
        group.throughput(Throughput::Elements(size as u64));
        group.bench_with_input(
            criterion::BenchmarkId::new("process_data_delta", size),
            &size,
            |b, &n| {
                let engine = build_engine();
                let delta = sample_delta(RecordFlag::F_LAST as u8);
                b.iter(|| {
                    let mut e = engine.borrow_mut();
                    for _ in 0..n {
                        e.process_data(Data::BookDelta(black_box(delta)));
                    }
                });
            },
        );
    }

    group.finish();
}

fn bench_process_delta_buffered(c: &mut Criterion) {
    let mut group = c.benchmark_group("DataEngine ingest batch");
    let config = DataEngineConfig {
        buffer_deltas: true,
        ..DataEngineConfig::default()
    };

    // Incremental stream: engine buffers until F_LAST, then publishes
    for size in [1_000_usize, 10_000, 100_000] {
        group.throughput(Throughput::Elements(size as u64));
        group.bench_with_input(
            criterion::BenchmarkId::new("process_data_delta_buffered", size),
            &size,
            |b, &n| {
                let engine = build_engine_with_config(Some(config.clone()));
                let body = sample_delta(0);
                let last = sample_delta(RecordFlag::F_LAST as u8);
                b.iter(|| {
                    let mut e = engine.borrow_mut();

                    for i in 0..n {
                        let delta = if (i + 1).is_multiple_of(DELTA_FRAME_SIZE) || i + 1 == n {
                            last
                        } else {
                            body
                        };
                        e.process_data(Data::BookDelta(black_box(delta)));
                    }
                });
            },
        );
    }

    group.finish();
}

macro_rules! external_ingress {
    ($name:ident, $ty:ty, $value:expr, $payload:ident, $topic:ident, $publish:ident, $subscribe:ident, $prepare:expr, $verify:expr) => {
        fn $name(c: &mut Criterion) {
            let value: $ty = $value;
            let topic = $topic(value.instrument_id);
            let mut group = c.benchmark_group(concat!(
                "DataEngine external ingress/",
                stringify!($payload)
            ));
            group.throughput(Throughput::Elements(1));

            for (encoding_name, encoding) in [
                ("json", SerializationEncoding::Json),
                ("msgpack", SerializationEncoding::MsgPack),
                ("sbe", SerializationEncoding::Sbe),
            ] {
                if !BusPayloadType::$payload.supports(encoding) {
                    continue;
                }

                let message = encoded_message(&value, topic, encoding, $publish);

                for mode in [
                    "fallback",
                    "direct",
                    "application",
                    "application_external",
                    "engine",
                ] {
                    group.bench_function(BenchmarkId::new(mode, encoding_name), |b| {
                        let engine = build_external_engine(mode, &value, $prepare);

                        let mut message = message.clone();
                        let topic = if matches!(mode, "application" | "application_external") {
                            let topic: MStr<Topic> = "application.market_data".into();
                            message.topic = *topic;
                            topic
                        } else {
                            topic
                        };

                        let received = Rc::new(Cell::new(0_u64));
                        let handler = TypedHandler::from({
                            let received = Rc::clone(&received);
                            move |value: &$ty| {
                                black_box(value);
                                received.set(received.get() + 1);
                            }
                        });
                        $subscribe(topic.into(), handler, None);
                        get_message_bus()
                            .borrow_mut()
                            .add_streaming_type(BusPayloadType::$payload);
                        republish_external_message(&message).expect("ingress must succeed");

                        assert_eq!(received.get(), 1);

                        verify_external_engine(engine.as_ref(), &value, mode == "engine", $verify);

                        // Encoding, subscription setup, and semantic checks stay outside timing
                        b.iter(|| {
                            republish_external_message(black_box(&message))
                                .expect("ingress must succeed")
                        });
                    });
                }
            }

            group.finish();
            install_thread_local_msgbus();
        }
    };
}

fn build_external_engine<T>(
    mode: &str,
    value: &T,
    prepare: impl FnOnce(&mut DataEngine, &T),
) -> Option<Rc<RefCell<DataEngine>>> {
    if mode == "fallback" {
        install_thread_local_msgbus();
        return None;
    }

    let external = matches!(mode, "application_external" | "engine");

    let config = DataEngineConfig {
        external_clients: external.then(|| vec!["EXTERNAL".into()]),
        ..Default::default()
    };

    let engine = build_engine_with_config(Some(config));
    if external {
        prepare(&mut engine.borrow_mut(), value);
    }

    Some(engine)
}

fn verify_external_engine<T>(
    engine: Option<&Rc<RefCell<DataEngine>>>,
    value: &T,
    routed: bool,
    verify: impl FnOnce(&DataEngine, &T),
) {
    let Some(engine) = engine else {
        return;
    };

    let engine = engine.borrow();
    assert_eq!(engine.data_count(), u64::from(routed));

    if routed {
        verify(&engine, value);
    }
}

external_ingress!(
    bench_external_quote,
    QuoteTick,
    QuoteTick::default(),
    QuoteTick,
    get_quotes_topic,
    publish_quote,
    subscribe_quotes,
    |_: &mut DataEngine, _: &QuoteTick| {},
    |engine: &DataEngine, value: &QuoteTick| {
        assert_eq!(
            engine.cache().borrow().quote(&value.instrument_id),
            Some(value)
        );
    }
);
external_ingress!(
    bench_external_trade,
    TradeTick,
    sample_trade(),
    TradeTick,
    get_trades_topic,
    publish_trade,
    subscribe_trades,
    |_: &mut DataEngine, _: &TradeTick| {},
    |engine: &DataEngine, value: &TradeTick| {
        assert_eq!(
            engine.cache().borrow().trade(&value.instrument_id),
            Some(value)
        );
    }
);
external_ingress!(
    bench_external_deltas,
    OrderBookDeltas,
    stub_deltas(),
    OrderBookDeltas,
    get_book_deltas_topic,
    publish_deltas,
    subscribe_book_deltas,
    |engine: &mut DataEngine, value: &OrderBookDeltas| {
        engine
            .execute_subscribe(SubscribeCommand::BookDeltas(SubscribeBookDeltas::new(
                value.instrument_id,
                BookType::L2_MBP,
                Some("EXTERNAL".into()),
                None,
                UUID4::new(),
                0.into(),
                None,
                true,
                None,
                None,
            )))
            .expect("managed delta subscription must succeed");
    },
    |engine: &DataEngine, value: &OrderBookDeltas| {
        let mut expected = OrderBook::new(value.instrument_id, BookType::L2_MBP);
        expected.apply_deltas(value).expect("fixture must apply");
        let cache = engine.cache().borrow();
        let actual = cache
            .order_book(&value.instrument_id)
            .expect("managed book must exist");
        assert_eq!(
            actual.to_deltas(0.into(), 0.into()),
            expected.to_deltas(0.into(), 0.into())
        );
    }
);
external_ingress!(
    bench_external_depth,
    OrderBookDepth,
    stub_depth10(),
    OrderBookDepth,
    get_book_depth_topic,
    publish_depth,
    subscribe_book_depth,
    |engine: &mut DataEngine, value: &OrderBookDepth| {
        engine
            .execute_subscribe(SubscribeCommand::BookDepth(SubscribeBookDepth::new(
                value.instrument_id,
                BookType::L2_MBP,
                Some("EXTERNAL".into()),
                None,
                UUID4::new(),
                0.into(),
                None,
                true,
                None,
                None,
            )))
            .expect("managed depth subscription must succeed");
    },
    |engine: &DataEngine, value: &OrderBookDepth| {
        let mut expected = OrderBook::new(value.instrument_id, BookType::L2_MBP);
        expected.apply_depth(value).expect("fixture must apply");
        let cache = engine.cache().borrow();
        let actual = cache
            .order_book(&value.instrument_id)
            .expect("managed book must exist");
        assert_eq!(
            actual.to_deltas(0.into(), 0.into()),
            expected.to_deltas(0.into(), 0.into())
        );
    }
);

fn encoded_message<T: 'static>(
    value: &T,
    topic: MStr<Topic>,
    encoding: SerializationEncoding,
    publish: fn(MStr<Topic>, &T),
) -> BusMessage {
    install_thread_local_msgbus();
    let captured = Rc::new(RefCell::new(None));
    get_message_bus()
        .borrow_mut()
        .set_external_egress(Box::new(CapturingEgress(Rc::clone(&captured))), encoding);
    publish(topic, value);
    let mut message = captured
        .borrow_mut()
        .take()
        .expect("egress must serialize the fixture");
    message.source = Some(BusMessageSource {
        publisher_id: "PUBLISHER-001".into(),
        instance_id: UUID4::new(),
        stream: "market-data".into(),
        sequence: 17,
    });

    message
}

struct CapturingEgress(Rc<RefCell<Option<BusMessage>>>);

impl MessageBusExternalEgress for CapturingEgress {
    fn is_closed(&self) -> bool {
        false
    }

    fn publish(&self, message: BusMessage) {
        assert!(self.0.borrow_mut().replace(message).is_none());
    }

    fn close(&mut self) {}
}

criterion_group!(
    benches,
    bench_process_trade,
    bench_process_trade_batch,
    bench_process_delta,
    bench_process_delta_batch,
    bench_process_delta_buffered,
    bench_external_quote,
    bench_external_trade,
    bench_external_deltas,
    bench_external_depth,
);
criterion_main!(benches);
