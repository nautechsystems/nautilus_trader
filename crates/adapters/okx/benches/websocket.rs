// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use nautilus_okx::websocket::messages::OKXWsMessage;
use serde_json::Value;

const TICKERS: &str = include_str!("../test_data/ws_tickers.json");
const TRADES: &str = include_str!("../test_data/ws_trades.json");
const BOOKS_SNAPSHOT: &str = include_str!("../test_data/ws_books_snapshot.json");
const BOOKS_UPDATE: &str = include_str!("../test_data/ws_books_update.json");
const BBO_TBT: &str = include_str!("../test_data/ws_bbo_tbt.json");
const CANDLE: &str = include_str!("../test_data/ws_candle.json");
const FUNDING_RATE: &str = include_str!("../test_data/ws_funding_rate.json");
const ORDERS: &str = include_str!("../test_data/ws_orders.json");
const ACCOUNT: &str = include_str!("../test_data/ws_account.json");

// =============================================================================
// DESERIALIZATION BENCHMARKS
// =============================================================================

fn bench_message_parsing(c: &mut Criterion) {
    let mut group = c.benchmark_group("OKX Message Parsing");

    let messages = [
        ("tickers", TICKERS),
        ("trades", TRADES),
        ("books_snapshot", BOOKS_SNAPSHOT),
        ("books_update", BOOKS_UPDATE),
        ("bbo_tbt", BBO_TBT),
        ("candle", CANDLE),
        ("funding_rate", FUNDING_RATE),
        ("orders", ORDERS),
        ("account", ACCOUNT),
    ];

    // Benchmark parsing JSON string to OKXWsMessage (custom deserializer)
    for (name, msg) in &messages {
        group.bench_with_input(BenchmarkId::new("to_struct", name), msg, |b, msg| {
            b.iter(|| {
                let ws_msg: OKXWsMessage = serde_json::from_str(black_box(msg)).unwrap();
                black_box(ws_msg);
            });
        });
    }

    group.finish();
}

fn bench_json_value_parsing(c: &mut Criterion) {
    let mut group = c.benchmark_group("OKX JSON Value Parsing");

    let messages = [
        ("tickers", TICKERS),
        ("trades", TRADES),
        ("books_snapshot", BOOKS_SNAPSHOT),
        ("books_update", BOOKS_UPDATE),
        ("bbo_tbt", BBO_TBT),
        ("candle", CANDLE),
        ("funding_rate", FUNDING_RATE),
        ("orders", ORDERS),
        ("account", ACCOUNT),
    ];

    // Benchmark parsing JSON string to Value (for comparison)
    for (name, msg) in &messages {
        group.bench_with_input(BenchmarkId::new("to_value", name), msg, |b, msg| {
            b.iter(|| {
                let value: Value = serde_json::from_str(black_box(msg)).unwrap();
                black_box(value);
            });
        });
    }

    group.finish();
}

// =============================================================================
// BATCH PROCESSING BENCHMARKS
// =============================================================================

fn bench_batch_processing(c: &mut Criterion) {
    let mut group = c.benchmark_group("OKX Batch Processing");

    let batch_sizes = [10, 100, 1000];

    // Mix of common message types (weighted toward book updates which are most frequent)
    let messages = [TICKERS, TRADES, BOOKS_UPDATE, BOOKS_UPDATE, BOOKS_UPDATE];

    for batch_size in batch_sizes {
        group.throughput(Throughput::Elements(batch_size as u64));

        group.bench_with_input(
            BenchmarkId::new("mixed_messages", batch_size),
            &batch_size,
            |b, &size| {
                b.iter(|| {
                    for i in 0..size {
                        let msg = messages[i % messages.len()];
                        let ws_msg: OKXWsMessage = serde_json::from_str(msg).unwrap();
                        black_box(ws_msg);
                    }
                });
            },
        );
    }

    group.finish();
}

fn bench_book_updates_batch(c: &mut Criterion) {
    let mut group = c.benchmark_group("OKX Book Updates Batch");

    let batch_sizes = [10, 100, 1000];

    for batch_size in batch_sizes {
        group.throughput(Throughput::Elements(batch_size as u64));

        group.bench_with_input(
            BenchmarkId::new("book_updates_only", batch_size),
            &batch_size,
            |b, &size| {
                b.iter(|| {
                    for _ in 0..size {
                        let ws_msg: OKXWsMessage = serde_json::from_str(BOOKS_UPDATE).unwrap();
                        black_box(ws_msg);
                    }
                });
            },
        );
    }

    group.finish();
}

// =============================================================================
// CHANNEL ROUTING BENCHMARKS
// =============================================================================

fn bench_channel_routing(c: &mut Criterion) {
    let mut group = c.benchmark_group("OKX Channel Routing");

    let messages = [
        ("tickers", TICKERS),
        ("trades", TRADES),
        ("books_snapshot", BOOKS_SNAPSHOT),
        ("books_update", BOOKS_UPDATE),
        ("bbo_tbt", BBO_TBT),
        ("candle", CANDLE),
        ("funding_rate", FUNDING_RATE),
        ("orders", ORDERS),
        ("account", ACCOUNT),
    ];

    // Benchmark full parse + variant matching (simulates handler routing)
    for (name, msg) in &messages {
        group.bench_with_input(BenchmarkId::new("parse_and_route", name), msg, |b, msg| {
            b.iter(|| {
                let ws_msg: OKXWsMessage = serde_json::from_str(black_box(msg)).unwrap();
                // Simulate routing by matching on variant
                let channel_type = match &ws_msg {
                    OKXWsMessage::BookData { .. } => "book",
                    OKXWsMessage::Data { arg, .. } => {
                        // Access arg.channel to simulate routing
                        let _ = &arg.channel;
                        "data"
                    }
                    OKXWsMessage::OrderResponse { .. } => "order_response",
                    OKXWsMessage::Login { .. } => "login",
                    OKXWsMessage::Subscription { .. } => "subscription",
                    OKXWsMessage::ChannelConnCount { .. } => "conn_count",
                    OKXWsMessage::Error { .. } => "error",
                    OKXWsMessage::Ping => "ping",
                    OKXWsMessage::Reconnected => "reconnected",
                };
                black_box(channel_type);
            });
        });
    }

    group.finish();
}

// =============================================================================
// COMPARISON BENCHMARKS
// =============================================================================

fn bench_parsing_comparison(c: &mut Criterion) {
    let mut group = c.benchmark_group("OKX Parsing Comparison");

    // Compare custom deserializer vs Value parsing for same message
    group.bench_function("books_update_to_struct", |b| {
        b.iter(|| {
            let ws_msg: OKXWsMessage = serde_json::from_str(black_box(BOOKS_UPDATE)).unwrap();
            black_box(ws_msg);
        });
    });

    group.bench_function("books_update_to_value", |b| {
        b.iter(|| {
            let value: Value = serde_json::from_str(black_box(BOOKS_UPDATE)).unwrap();
            black_box(value);
        });
    });

    group.bench_function("trades_to_struct", |b| {
        b.iter(|| {
            let ws_msg: OKXWsMessage = serde_json::from_str(black_box(TRADES)).unwrap();
            black_box(ws_msg);
        });
    });

    group.bench_function("trades_to_value", |b| {
        b.iter(|| {
            let value: Value = serde_json::from_str(black_box(TRADES)).unwrap();
            black_box(value);
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_message_parsing,
    bench_json_value_parsing,
    bench_batch_processing,
    bench_book_updates_batch,
    bench_channel_routing,
    bench_parsing_comparison,
);
criterion_main!(benches);
