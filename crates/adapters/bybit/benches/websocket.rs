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
use nautilus_bybit::websocket::{classify_bybit_message, messages::BybitWsMessage};
use serde_json::Value;
use tokio_tungstenite::tungstenite::Message;

const TRADE: &str = include_str!("../test_data/ws_public_trade.json");
const ORDERBOOK_SNAPSHOT: &str = include_str!("../test_data/ws_orderbook_snapshot.json");
const ORDERBOOK_DELTA: &str = include_str!("../test_data/ws_orderbook_delta.json");
const TICKER_LINEAR: &str = include_str!("../test_data/ws_ticker_linear.json");
const KLINE: &str = include_str!("../test_data/ws_kline.json");
const ACCOUNT_ORDER: &str = include_str!("../test_data/ws_account_order.json");
const ACCOUNT_EXECUTION: &str = include_str!("../test_data/ws_account_execution.json");
const ACCOUNT_WALLET: &str = include_str!("../test_data/ws_account_wallet.json");
const ACCOUNT_POSITION: &str = include_str!("../test_data/ws_account_position.json");

const PONG: &str = r#"{"op":"pong"}"#;
const SUBSCRIBE_SUCCESS: &str =
    r#"{"success":true,"ret_msg":"","conn_id":"abc123","req_id":"1","op":"subscribe"}"#;

// =============================================================================
// CLASSIFICATION BENCHMARKS
// =============================================================================

fn bench_classification(c: &mut Criterion) {
    let mut group = c.benchmark_group("Classification");

    let messages = [
        ("trade", TRADE),
        ("orderbook_snapshot", ORDERBOOK_SNAPSHOT),
        ("orderbook_delta", ORDERBOOK_DELTA),
        ("ticker_linear", TICKER_LINEAR),
        ("kline", KLINE),
        ("account_order", ACCOUNT_ORDER),
        ("account_execution", ACCOUNT_EXECUTION),
        ("account_wallet", ACCOUNT_WALLET),
        ("account_position", ACCOUNT_POSITION),
        ("pong", PONG),
        ("subscribe", SUBSCRIBE_SUCCESS),
    ];

    for (name, msg) in &messages {
        let value: Value = serde_json::from_str(msg).unwrap();

        group.bench_with_input(BenchmarkId::new("classify", name), &value, |b, value| {
            b.iter(|| {
                black_box(classify_bybit_message(black_box(value.clone())));
            });
        });
    }

    group.finish();
}

// =============================================================================
// FULL FLOW BENCHMARKS (parse JSON + classify)
// =============================================================================

fn bench_full_flow(c: &mut Criterion) {
    let mut group = c.benchmark_group("Full Flow");

    let messages = [
        ("trade", TRADE),
        ("orderbook_snapshot", ORDERBOOK_SNAPSHOT),
        ("orderbook_delta", ORDERBOOK_DELTA),
        ("ticker_linear", TICKER_LINEAR),
        ("kline", KLINE),
        ("account_order", ACCOUNT_ORDER),
        ("account_execution", ACCOUNT_EXECUTION),
    ];

    for (name, msg) in &messages {
        group.bench_with_input(
            BenchmarkId::new("parse_and_classify", name),
            msg,
            |b, msg| {
                b.iter(|| {
                    let value: Value = serde_json::from_str(black_box(msg)).unwrap();
                    let msg_type = classify_bybit_message(value.clone());
                    black_box((value, msg_type));
                });
            },
        );
    }

    group.finish();
}

// =============================================================================
// JSON PARSING BENCHMARKS
// =============================================================================

fn bench_parsing(c: &mut Criterion) {
    let mut group = c.benchmark_group("Parsing");

    let messages = [
        ("trade", TRADE),
        ("orderbook_snapshot", ORDERBOOK_SNAPSHOT),
        ("orderbook_delta", ORDERBOOK_DELTA),
        ("ticker_linear", TICKER_LINEAR),
        ("kline", KLINE),
    ];

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
// BATCH BENCHMARKS
// =============================================================================

fn bench_batch(c: &mut Criterion) {
    let mut group = c.benchmark_group("Batch");

    let batch_sizes = [10, 100, 1000];

    // Typical message distribution: mostly orderbook deltas
    let messages = [
        TRADE,
        ORDERBOOK_DELTA,
        ORDERBOOK_DELTA,
        ORDERBOOK_DELTA,
        TICKER_LINEAR,
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
                        let msg_type = classify_bybit_message(value.clone());
                        black_box((value, msg_type));
                    }
                });
            },
        );
    }

    group.finish();
}

// =============================================================================
// PREFILTER BENCHMARKS
// =============================================================================

fn bench_pong_prefilter(c: &mut Criterion) {
    let mut group = c.benchmark_group("Pong Prefilter");

    group.bench_function("string_check_pong", |b| {
        let text = "pong";
        b.iter(|| {
            let is_pong = text.trim().eq_ignore_ascii_case("pong");
            black_box(is_pong);
        });
    });

    group.bench_function("string_check_data_msg", |b| {
        let text = ORDERBOOK_DELTA;
        b.iter(|| {
            let is_pong = text.trim().eq_ignore_ascii_case("pong");
            black_box(is_pong);
        });
    });

    group.bench_function("json_parse_pong", |b| {
        b.iter(|| {
            let value: Value = serde_json::from_str(black_box(PONG)).unwrap();
            black_box(value);
        });
    });

    group.finish();
}

// =============================================================================
// TUNGSTENITE MESSAGE ACCESS
// =============================================================================

fn bench_tungstenite_message_access(c: &mut Criterion) {
    let mut group = c.benchmark_group("Message Access");

    let text_msg = Message::Text(TRADE.into());
    let binary_msg = Message::Binary(TRADE.as_bytes().into());

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

// =============================================================================
// MESSAGE TYPE DISTRIBUTION
// =============================================================================

fn bench_message_type_match(c: &mut Criterion) {
    let mut group = c.benchmark_group("Message Type");

    let messages: Vec<(_, BybitWsMessage)> = vec![
        (
            "trade",
            classify_bybit_message(serde_json::from_str(TRADE).unwrap()),
        ),
        (
            "orderbook",
            classify_bybit_message(serde_json::from_str(ORDERBOOK_DELTA).unwrap()),
        ),
        (
            "ticker",
            classify_bybit_message(serde_json::from_str(TICKER_LINEAR).unwrap()),
        ),
        (
            "kline",
            classify_bybit_message(serde_json::from_str(KLINE).unwrap()),
        ),
        (
            "account_order",
            classify_bybit_message(serde_json::from_str(ACCOUNT_ORDER).unwrap()),
        ),
    ];

    for (name, msg) in messages {
        group.bench_with_input(BenchmarkId::new("is_data_message", name), &msg, |b, msg| {
            b.iter(|| {
                let is_data = matches!(
                    black_box(msg),
                    BybitWsMessage::Trade(_)
                        | BybitWsMessage::Orderbook(_)
                        | BybitWsMessage::Kline(_)
                        | BybitWsMessage::TickerLinear(_)
                        | BybitWsMessage::TickerOption(_)
                );
                black_box(is_data);
            });
        });
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_classification,
    bench_full_flow,
    bench_parsing,
    bench_batch,
    bench_pong_prefilter,
    bench_tungstenite_message_access,
    bench_message_type_match,
);
criterion_main!(benches);
