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

use std::{cell::RefCell, hint::black_box, rc::Rc, sync::Arc, time::Duration};

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use nautilus_common::{
    messages::{DataEvent, data::DataCommand},
    msgbus::{
        self, MessageBus, TypedIntoHandler, register_data_endpoint,
        switchboard::MessagingSwitchboard,
    },
    timer::TimeEventHandler,
};
use nautilus_core::UnixNanos;
use nautilus_live::runner::AsyncRunner;
use nautilus_model::{
    data::{Data, quote::QuoteTick, trade::TradeTick},
    enums::AggressorSide,
    identifiers::{InstrumentId, TradeId},
    types::{Price, Quantity},
};

fn create_test_quote() -> QuoteTick {
    QuoteTick {
        instrument_id: InstrumentId::from("EUR/USD.SIM"),
        bid_price: Price::from("1.10000"),
        ask_price: Price::from("1.10001"),
        bid_size: Quantity::from(1_000_000),
        ask_size: Quantity::from(1_000_000),
        ts_event: UnixNanos::default(),
        ts_init: UnixNanos::default(),
    }
}

fn create_test_trade() -> TradeTick {
    TradeTick {
        instrument_id: InstrumentId::from("EUR/USD.SIM"),
        price: Price::from("1.10000"),
        size: Quantity::from(100_000),
        aggressor_side: AggressorSide::Buyer,
        trade_id: TradeId::from("123456"),
        ts_event: UnixNanos::default(),
        ts_init: UnixNanos::default(),
    }
}

fn bench_channel_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("AsyncRunner Channel Operations");

    // Benchmark raw channel send/recv operations
    group.bench_function("unbounded_channel_send_recv", |b| {
        b.iter(|| {
            let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
            let quote = create_test_quote();

            // Send events
            for _ in 0..100 {
                tx.send(DataEvent::Data(Data::Quote(black_box(quote))))
                    .unwrap();
            }

            // Receive events
            while rx.try_recv().is_ok() {
                // Process
            }
        });
    });

    // Benchmark channel creation overhead
    group.bench_function("channel_creation", |b| {
        b.iter(|| {
            let (_tx1, _rx1) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
            let (_tx2, _rx2) = tokio::sync::mpsc::unbounded_channel::<TimeEventHandler>();
            let (_tx3, _rx3) = tokio::sync::mpsc::unbounded_channel::<()>();
        });
    });

    group.finish();
}

fn bench_runner_components(c: &mut Criterion) {
    let mut group = c.benchmark_group("AsyncRunner Components");

    // Benchmark just the channel setup that AsyncRunner does
    group.bench_function("runner_channel_setup", |b| {
        b.iter(|| {
            // Simulate what AsyncRunner::new() does without the global state
            let (_data_tx, _data_rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
            let (_cmd_tx, _cmd_rx) = tokio::sync::mpsc::unbounded_channel::<DataCommand>();
            let (_time_tx, _time_rx) = tokio::sync::mpsc::unbounded_channel::<TimeEventHandler>();
            let (_signal_tx, _signal_rx) = tokio::sync::mpsc::unbounded_channel::<()>();
        });
    });

    // Benchmark the stop signal mechanism
    group.bench_function("stop_signal", |b| {
        b.iter(|| {
            let (signal_tx, mut signal_rx) = tokio::sync::mpsc::unbounded_channel::<()>();

            // Send stop signal
            signal_tx.send(()).unwrap();

            // Check if signal received
            black_box(signal_rx.try_recv().is_ok());
        });
    });

    group.finish();
}

fn bench_event_creation(c: &mut Criterion) {
    let mut group = c.benchmark_group("Event Creation");

    group.bench_function("quote_tick_creation", |b| {
        b.iter(|| {
            black_box(create_test_quote());
        });
    });

    group.bench_function("trade_tick_creation", |b| {
        b.iter(|| {
            black_box(create_test_trade());
        });
    });

    group.bench_function("data_event_quote", |b| {
        let quote = create_test_quote();
        b.iter(|| {
            black_box(DataEvent::Data(Data::Quote(quote)));
        });
    });

    group.bench_function("data_event_trade", |b| {
        let trade = create_test_trade();
        b.iter(|| {
            black_box(DataEvent::Data(Data::Trade(trade)));
        });
    });

    group.finish();
}

fn bench_concurrent_channels(c: &mut Criterion) {
    let mut group = c.benchmark_group("Concurrent Channel Operations");

    for sender_count in [2, 4, 8] {
        group.bench_with_input(
            BenchmarkId::new("concurrent_sends", sender_count),
            &sender_count,
            |b, &num_senders| {
                b.iter(|| {
                    let events_per_sender = 100;
                    let total_events = num_senders * events_per_sender;

                    // Create shared channel
                    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();

                    // Simulate concurrent sends
                    let mut handles = vec![];

                    for _ in 0..num_senders {
                        let tx_clone = tx.clone();

                        handles.push(std::thread::spawn(move || {
                            let quote = create_test_quote();
                            for _ in 0..events_per_sender {
                                tx_clone.send(DataEvent::Data(Data::Quote(quote))).unwrap();
                            }
                        }));
                    }

                    // Wait for all senders
                    for handle in handles {
                        handle.join().unwrap();
                    }

                    // Drain receiver
                    let mut count = 0;
                    while rx.try_recv().is_ok() {
                        count += 1;
                    }
                    assert_eq!(count, total_events);
                });
            },
        );
    }

    group.finish();
}

fn bench_batch_processing(c: &mut Criterion) {
    let mut group = c.benchmark_group("Batch Event Processing");

    for batch_size in [10, 100, 1000] {
        group.throughput(Throughput::Elements(batch_size as u64));

        group.bench_with_input(
            BenchmarkId::new("batch_send_recv", batch_size),
            &batch_size,
            |b, &size| {
                b.iter(|| {
                    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
                    let quote = create_test_quote();

                    // Send batch
                    for _ in 0..size {
                        tx.send(DataEvent::Data(Data::Quote(black_box(quote))))
                            .unwrap();
                    }

                    // Receive batch
                    let mut received = 0;
                    while rx.try_recv().is_ok() {
                        received += 1;
                    }
                    assert_eq!(received, size);
                });
            },
        );
    }

    group.finish();
}

fn bench_memory_usage(c: &mut Criterion) {
    let mut group = c.benchmark_group("Memory Patterns");

    // Benchmark event cloning overhead
    group.bench_function("quote_clone", |b| {
        let quote = create_test_quote();
        b.iter(|| {
            black_box(quote);
        });
    });

    group.bench_function("trade_clone", |b| {
        let trade = create_test_trade();
        b.iter(|| {
            black_box(trade);
        });
    });

    // Benchmark Arc operations
    group.bench_function("arc_creation", |b| {
        let data = vec![1u8; 1024];
        b.iter(|| {
            black_box(Arc::new(data.clone()));
        });
    });

    group.bench_function("arc_clone", |b| {
        let data = Arc::new(vec![1u8; 1024]);
        b.iter(|| {
            black_box(data.clone());
        });
    });

    group.finish();
}

// Drives the actual runner dispatch path: msgbus endpoint lookup, sent_count
// increment, and a noop handler. Skips the 5-branch `select!` poll cost,
// which is bounded by `tokio::mpsc::recv` and is small relative to the
// dispatch shown by this bench. Pair with the `stress_trade_burst` test
// (`crates/live/tests/stress.rs`) for end-to-end runner+engine numbers.
fn bench_runner_dispatch(c: &mut Criterion) {
    msgbus::set_message_bus(Rc::new(RefCell::new(MessageBus::default())));

    register_data_endpoint(
        MessagingSwitchboard::data_engine_process_data(),
        TypedIntoHandler::from(|data: Data| {
            black_box(data);
        }),
    );

    let mut group = c.benchmark_group("AsyncRunner dispatch");
    let trade = create_test_trade();

    for size in [100_usize, 1_000, 10_000] {
        group.throughput(Throughput::Elements(size as u64));
        group.bench_function(BenchmarkId::new("drain_data_events", size), |b| {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_time()
                .build()
                .unwrap();

            b.iter_custom(|iters| {
                let mut total = Duration::ZERO;

                for _ in 0..iters {
                    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
                    for _ in 0..size {
                        tx.send(DataEvent::Data(Data::Trade(trade))).unwrap();
                    }
                    drop(tx);

                    let start = std::time::Instant::now();
                    rt.block_on(async {
                        while let Some(evt) = rx.recv().await {
                            AsyncRunner::handle_data_event(evt);
                        }
                    });
                    total += start.elapsed();
                }

                total
            });
        });
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_channel_operations,
    bench_runner_components,
    bench_event_creation,
    bench_concurrent_channels,
    bench_batch_processing,
    bench_memory_usage,
    bench_runner_dispatch,
);
criterion_main!(benches);
