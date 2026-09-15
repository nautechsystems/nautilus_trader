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

use criterion::{BatchSize, BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use nautilus_common::{
    live::{dispatch::DispatchMessage, sender::EventSender},
    messages::{
        DataEvent, ExecutionEvent,
        data::{DataCommand, SubscribeCommand, SubscribeQuotes},
        execution::{QueryAccount, TradingCommand},
    },
    msgbus::{
        self, MessageBus, TypedIntoHandler, register_data_endpoint,
        switchboard::MessagingSwitchboard,
    },
    runner::{
        SyncTradingCommandSender, TimeEventMessage, TradingCommandMessage, TradingCommandSender,
        drain_trading_cmd_queue,
    },
};
use nautilus_core::{UUID4, UnixNanos};
use nautilus_live::runner::AsyncRunner;
use nautilus_model::{
    data::{Data, quote::QuoteTick, trade::TradeTick},
    enums::AggressorSide,
    events::{OrderEventAny, OrderInitialized},
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
        aggressor_side: AggressorSide::Buy,
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
            let (_tx2, _rx2) = tokio::sync::mpsc::unbounded_channel::<TimeEventMessage>();
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
            let (_time_tx, _time_rx) = tokio::sync::mpsc::unbounded_channel::<TimeEventMessage>();
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
// (`crates/live/tests/integration/stress.rs`) for end-to-end runner+engine numbers.
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

fn bench_command_channels(c: &mut Criterion) {
    let received = Rc::new(std::cell::Cell::new(0usize));
    let values = received.clone();
    msgbus::register_data_command_endpoint(
        MessagingSwitchboard::data_engine_execute(),
        TypedIntoHandler::from(move |command| {
            black_box(command);
            values.set(values.get() + 1);
        }),
    );

    let values = received.clone();
    msgbus::register_trading_command_endpoint(
        MessagingSwitchboard::exec_engine_execute(),
        TypedIntoHandler::from(move |command| {
            black_box(command);
            values.set(values.get() + 1);
        }),
    );

    let data = DataCommand::Subscribe(SubscribeCommand::Quotes(SubscribeQuotes::new(
        "EUR/USD.SIM".into(),
        Some("SIM".into()),
        None,
        UUID4::from("00000000-0000-4000-8000-000000000001"),
        1.into(),
        None,
        None,
    )));

    let trading = TradingCommandMessage::new(
        MessagingSwitchboard::exec_engine_execute(),
        TradingCommand::QueryAccount(QueryAccount::new(
            "BENCH-001".into(),
            None,
            "SIM-001".into(),
            UUID4::from("00000000-0000-4000-8000-000000000002"),
            2.into(),
            None,
            None,
        )),
    );
    bench_channel_transport(
        c,
        "live_data_commands",
        || data.clone(),
        |command| msgbus::send_data_command(MessagingSwitchboard::data_engine_execute(), command),
        AsyncRunner::handle_data_command,
        command_send,
        &received,
    );
    bench_channel_transport(
        c,
        "live_trading_commands",
        || TradingCommandMessage::new(trading.endpoint(), trading.command().clone()),
        |message| {
            let mut messages = vec![message];
            while let Some(message) = messages.pop() {
                messages.extend(message.dispatch().into_iter().rev());
            }
        },
        AsyncRunner::handle_trading_command,
        command_send,
        &received,
    );
}

fn bench_event_channels(c: &mut Criterion) {
    let received = Rc::new(std::cell::Cell::new(0usize));
    let observed = received.clone();
    msgbus::register_data_endpoint(
        MessagingSwitchboard::data_engine_process_data(),
        TypedIntoHandler::from(move |event| {
            black_box(event);
            observed.set(observed.get() + 1);
        }),
    );

    let observed = received.clone();
    msgbus::register_order_event_endpoint(
        MessagingSwitchboard::exec_engine_process(),
        TypedIntoHandler::from(move |event| {
            black_box(event);
            observed.set(observed.get() + 1);
        }),
    );

    bench_channel_transport(
        c,
        "live_data_events",
        || DataEvent::Data(Data::Quote(create_test_quote())),
        AsyncRunner::handle_data_event,
        AsyncRunner::dispatch_data_event,
        event_send,
        &received,
    );
    bench_channel_transport(
        c,
        "live_exec_events",
        || ExecutionEvent::Order(OrderEventAny::Initialized(OrderInitialized::default())),
        AsyncRunner::handle_exec_event,
        AsyncRunner::dispatch_exec_event,
        event_send,
        &received,
    );
}

fn command_send<T: std::fmt::Debug>(
    sender: tokio::sync::mpsc::UnboundedSender<DispatchMessage<T>>,
    owner: std::thread::ThreadId,
) -> impl Fn(T) {
    move |command| sender.send(DispatchMessage::new(command, owner)).unwrap()
}

fn event_send<T: std::fmt::Debug>(
    sender: tokio::sync::mpsc::UnboundedSender<DispatchMessage<T>>,
    _owner: std::thread::ThreadId,
) -> impl Fn(T) {
    let sender = EventSender::new(sender);
    move |event| sender.send(event).unwrap()
}

fn bench_channel_transport<T: std::fmt::Debug + 'static, S: Fn(T) + 'static>(
    c: &mut Criterion,
    name: &str,
    make_message: impl Fn() -> T,
    raw_dispatch: impl Fn(T),
    dispatch: impl Fn(DispatchMessage<T>),
    make_send: impl Fn(
        tokio::sync::mpsc::UnboundedSender<DispatchMessage<T>>,
        std::thread::ThreadId,
    ) -> S,
    received: &std::cell::Cell<usize>,
) {
    let mut group = c.benchmark_group(name);
    let owner = std::thread::current().id();

    for rooted in [false, true] {
        for tracked in [false, true] {
            let inputs = Rc::new(RefCell::new(Vec::new()));
            let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
            let (raw_tx, mut raw_rx) = tokio::sync::mpsc::unbounded_channel();
            let pending = inputs.clone();
            let tracked_send = make_send(tx, owner);

            let send = move || {
                for message in pending.borrow_mut().drain(..) {
                    if tracked {
                        tracked_send(message);
                    } else {
                        raw_tx.send(message).unwrap();
                    }
                }
            };

            let send = Rc::new(send);
            let publish = send.clone();
            msgbus::register_trading_command_endpoint(
                MessagingSwitchboard::risk_engine_execute(),
                TypedIntoHandler::from(move |_| publish()),
            );

            let trigger = TradingCommandMessage::new(
                MessagingSwitchboard::risk_engine_execute(),
                TradingCommand::QueryAccount(QueryAccount::new(
                    "BENCH-001".into(),
                    None,
                    "SIM-001".into(),
                    UUID4::from("00000000-0000-4000-8000-000000000003"),
                    3.into(),
                    None,
                    None,
                )),
            );

            for count in [1, 64] {
                group.throughput(Throughput::Elements(count as u64));
                group.bench_function(
                    format!(
                        "{}/{}/{count}",
                        if rooted { "shared_root" } else { "independent" },
                        if tracked { "tracked" } else { "plain" }
                    ),
                    |b| {
                        let setup = || {
                            inputs
                                .borrow_mut()
                                .extend(std::iter::repeat_with(&make_message).take(count));
                            Some(TradingCommandMessage::new(
                                trigger.endpoint(),
                                trigger.command().clone(),
                            ))
                        };

                        let mut run = |trigger: &mut Option<TradingCommandMessage>| {
                            if rooted {
                                SyncTradingCommandSender.execute(trigger.take().unwrap());
                                drain_trading_cmd_queue();
                            } else {
                                send();
                            }

                            if tracked {
                                while let Ok(message) = rx.try_recv() {
                                    dispatch(message);
                                }
                            } else {
                                while let Ok(message) = raw_rx.try_recv() {
                                    raw_dispatch(message);
                                }
                            }
                        };

                        let before = received.get();
                        run(&mut setup());
                        assert_eq!(received.get() - before, count);
                        b.iter_batched_ref(setup, run, BatchSize::PerIteration);
                    },
                );
            }
        }
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_command_channels,
    bench_event_channels,
    bench_channel_operations,
    bench_runner_components,
    bench_event_creation,
    bench_concurrent_channels,
    bench_batch_processing,
    bench_memory_usage,
    bench_runner_dispatch,
);
criterion_main!(benches);
