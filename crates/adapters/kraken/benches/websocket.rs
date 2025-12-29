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
use nautilus_kraken::websocket::{
    futures::messages::classify_futures_message, spot_v2::messages::KrakenWsMessage,
};
use nautilus_network::websocket::SubscriptionState;
use serde_json::Value;
use tokio_tungstenite::tungstenite::Message;
use ustr::Ustr;

// Load test data from files at compile time
const FUTURES_TICKER: &str = include_str!("../test_data/ws_futures_ticker.json");
const FUTURES_TRADE: &str = include_str!("../test_data/ws_futures_trade.json");
const FUTURES_BOOK_SNAPSHOT: &str = include_str!("../test_data/ws_futures_book_snapshot.json");
const FUTURES_BOOK_DELTA: &str = include_str!("../test_data/ws_futures_book_delta.json");
const SPOT_TICKER: &str = include_str!("../test_data/ws_ticker_snapshot.json");
const SPOT_TRADE: &str = include_str!("../test_data/ws_trade_update.json");
const SPOT_BOOK_SNAPSHOT: &str = include_str!("../test_data/ws_book_snapshot.json");
const SPOT_BOOK_UPDATE: &str = include_str!("../test_data/ws_book_update.json");

const FUTURES_HEARTBEAT: &str = r#"{"feed":"heartbeat"}"#;
const FUTURES_PONG: &str = r#"{"event":"pong"}"#;
const SPOT_HEARTBEAT: &str = r#"{"channel":"heartbeat"}"#;

// =============================================================================
// FUTURES BENCHMARKS
// =============================================================================

fn bench_futures_classification(c: &mut Criterion) {
    let mut group = c.benchmark_group("Futures Classification");

    let messages = [
        ("ticker", FUTURES_TICKER),
        ("trade", FUTURES_TRADE),
        ("book_snapshot", FUTURES_BOOK_SNAPSHOT),
        ("book_delta", FUTURES_BOOK_DELTA),
        ("heartbeat", FUTURES_HEARTBEAT),
        ("pong", FUTURES_PONG),
    ];

    for (name, msg) in messages.iter() {
        let value: Value = serde_json::from_str(msg).unwrap();

        group.bench_with_input(BenchmarkId::new("classify", name), &value, |b, value| {
            b.iter(|| {
                black_box(classify_futures_message(black_box(value)));
            });
        });
    }

    group.finish();
}

fn bench_futures_full_flow(c: &mut Criterion) {
    let mut group = c.benchmark_group("Futures Full Flow");

    let messages = [
        ("ticker", FUTURES_TICKER),
        ("trade", FUTURES_TRADE),
        ("book_snapshot", FUTURES_BOOK_SNAPSHOT),
        ("book_delta", FUTURES_BOOK_DELTA),
        ("heartbeat", FUTURES_HEARTBEAT),
    ];

    for (name, msg) in messages.iter() {
        group.bench_with_input(
            BenchmarkId::new("parse_and_classify", name),
            msg,
            |b, msg| {
                b.iter(|| {
                    let value: Value = serde_json::from_str(black_box(msg)).unwrap();
                    let msg_type = classify_futures_message(&value);
                    black_box((value, msg_type));
                });
            },
        );
    }

    group.finish();
}

fn bench_futures_batch(c: &mut Criterion) {
    let mut group = c.benchmark_group("Futures Batch");

    let batch_sizes = [10, 100, 1000];
    let messages = [
        FUTURES_TICKER,
        FUTURES_TRADE,
        FUTURES_BOOK_DELTA,
        FUTURES_BOOK_DELTA,
        FUTURES_BOOK_DELTA,
    ];

    for batch_size in batch_sizes {
        group.throughput(Throughput::Elements(batch_size as u64));

        group.bench_with_input(
            BenchmarkId::new("mixed_messages", batch_size),
            &batch_size,
            |b, &size| {
                b.iter(|| {
                    for i in 0..size {
                        let msg = messages[i % messages.len()];
                        let value: Value = serde_json::from_str(msg).unwrap();
                        let msg_type = classify_futures_message(&value);
                        black_box((value, msg_type));
                    }
                });
            },
        );
    }

    group.finish();
}

// =============================================================================
// SPOT V2 BENCHMARKS
// =============================================================================

fn bench_spot_parsing(c: &mut Criterion) {
    let mut group = c.benchmark_group("Spot Parsing");

    let messages = [
        ("ticker", SPOT_TICKER),
        ("trade", SPOT_TRADE),
        ("book_snapshot", SPOT_BOOK_SNAPSHOT),
        ("book_update", SPOT_BOOK_UPDATE),
    ];

    for (name, msg) in messages.iter() {
        group.bench_with_input(BenchmarkId::new("to_value", name), msg, |b, msg| {
            b.iter(|| {
                let value: Value = serde_json::from_str(black_box(msg)).unwrap();
                black_box(value);
            });
        });
    }

    group.finish();
}

fn bench_spot_full_flow(c: &mut Criterion) {
    let mut group = c.benchmark_group("Spot Full Flow");

    let messages = [
        ("ticker", SPOT_TICKER),
        ("trade", SPOT_TRADE),
        ("book_snapshot", SPOT_BOOK_SNAPSHOT),
        ("book_update", SPOT_BOOK_UPDATE),
    ];

    for (name, msg) in messages.iter() {
        group.bench_with_input(BenchmarkId::new("parse_to_struct", name), msg, |b, msg| {
            b.iter(|| {
                let ws_msg: KrakenWsMessage = serde_json::from_str(black_box(msg)).unwrap();
                black_box(ws_msg);
            });
        });
    }

    group.finish();
}

fn bench_spot_heartbeat_prefilter(c: &mut Criterion) {
    let mut group = c.benchmark_group("Spot Heartbeat Prefilter");

    group.bench_function("string_check_heartbeat", |b| {
        let text = SPOT_HEARTBEAT;
        b.iter(|| {
            let is_heartbeat = text.len() < 50
                && text.starts_with("{\"channel\":\"")
                && text.contains("heartbeat");
            black_box(is_heartbeat);
        });
    });

    group.bench_function("string_check_data_msg", |b| {
        let text = SPOT_BOOK_UPDATE;
        b.iter(|| {
            let is_heartbeat = text.len() < 50
                && text.starts_with("{\"channel\":\"")
                && text.contains("heartbeat");
            black_box(is_heartbeat);
        });
    });

    group.bench_function("json_parse_heartbeat", |b| {
        b.iter(|| {
            let value: Value = serde_json::from_str(black_box(SPOT_HEARTBEAT)).unwrap();
            black_box(value);
        });
    });

    group.finish();
}

fn bench_spot_batch(c: &mut Criterion) {
    let mut group = c.benchmark_group("Spot Batch");

    let batch_sizes = [10, 100, 1000];
    let messages = [
        SPOT_TICKER,
        SPOT_TRADE,
        SPOT_BOOK_UPDATE,
        SPOT_BOOK_UPDATE,
        SPOT_BOOK_UPDATE,
    ];

    for batch_size in batch_sizes {
        group.throughput(Throughput::Elements(batch_size as u64));

        group.bench_with_input(
            BenchmarkId::new("mixed_messages", batch_size),
            &batch_size,
            |b, &size| {
                b.iter(|| {
                    for i in 0..size {
                        let msg = messages[i % messages.len()];
                        let ws_msg: KrakenWsMessage = serde_json::from_str(msg).unwrap();
                        black_box(ws_msg);
                    }
                });
            },
        );
    }

    group.finish();
}

// =============================================================================
// SHARED BENCHMARKS
// =============================================================================

fn bench_subscription_check(c: &mut Criterion) {
    let mut group = c.benchmark_group("Subscription Check");

    let subscriptions = SubscriptionState::new(':');
    let channel = Ustr::from("book");
    let symbol = Ustr::from("BTC/USD");
    subscriptions.mark_subscribe("book:BTC/USD");
    subscriptions.confirm_subscribe("book:BTC/USD");

    group.bench_function("is_subscribed_hit", |b| {
        b.iter(|| {
            black_box(subscriptions.is_subscribed(black_box(&channel), black_box(&symbol)));
        });
    });

    let missing_symbol = Ustr::from("ETH/USD");
    group.bench_function("is_subscribed_miss", |b| {
        b.iter(|| {
            black_box(subscriptions.is_subscribed(black_box(&channel), black_box(&missing_symbol)));
        });
    });

    group.finish();
}

fn bench_tungstenite_message_access(c: &mut Criterion) {
    let mut group = c.benchmark_group("Message Access");

    let text_msg = Message::Text(FUTURES_TICKER.into());
    let binary_msg = Message::Binary(FUTURES_TICKER.as_bytes().into());

    group.bench_function("text_deref", |b| {
        b.iter(|| {
            if let Message::Text(text) = &text_msg {
                let s: &str = text;
                black_box(s.len());
            }
        });
    });

    group.bench_function("binary_utf8", |b| {
        b.iter(|| {
            if let Message::Binary(data) = &binary_msg {
                let s = std::str::from_utf8(data).unwrap();
                black_box(s.len());
            }
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_futures_classification,
    bench_futures_full_flow,
    bench_futures_batch,
    bench_spot_parsing,
    bench_spot_full_flow,
    bench_spot_heartbeat_prefilter,
    bench_spot_batch,
    bench_subscription_check,
    bench_tungstenite_message_access,
);
criterion_main!(benches);
