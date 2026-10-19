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

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use nautilus_core::UnixNanos;
use nautilus_deribit::{
    common::parse::parse_deribit_instrument_any,
    http::models::{DeribitInstrument, DeribitJsonRpcResponse},
    websocket::{
        messages::{
            DeribitBookMsg, DeribitTickerMsg, DeribitTradeMsg, DeribitWsMessage, parse_raw_message,
        },
        parse::parse_book_msg,
    },
};
use nautilus_model::instruments::InstrumentAny;
use serde_json::Value;
use tokio_tungstenite::tungstenite::Message;

// Test data loaded at compile time
const TRADES: &str = include_str!("../test_data/ws_trades.json");
const BOOK_SNAPSHOT: &str = include_str!("../test_data/ws_book_snapshot.json");
const BOOK_DELTA: &str = include_str!("../test_data/ws_book_delta.json");
const TICKER: &str = include_str!("../test_data/ws_ticker.json");
const QUOTE: &str = include_str!("../test_data/ws_quote.json");
const TEST_REQUEST: &str = include_str!("../test_data/ws_test_request.json");
const SUBSCRIBE_RESPONSE: &str = include_str!("../test_data/ws_subscribe_response.json");
const ERROR: &str = include_str!("../test_data/ws_error.json");
const INSTRUMENTS: &str = include_str!("../test_data/http_get_instruments.json");

/// Benchmarks the main `parse_raw_message` function for all message types.
fn bench_parse_raw_message(c: &mut Criterion) {
    let mut group = c.benchmark_group("Deribit parse_raw_message");

    let messages = [
        ("trades", TRADES),
        ("book_snapshot", BOOK_SNAPSHOT),
        ("book_delta", BOOK_DELTA),
        ("ticker", TICKER),
        ("quote", QUOTE),
        ("test_request", TEST_REQUEST),
        ("subscribe_response", SUBSCRIBE_RESPONSE),
        ("error", ERROR),
    ];

    for (name, msg) in &messages {
        group.bench_with_input(BenchmarkId::new("parse", name), msg, |b, msg| {
            b.iter(|| {
                let result = parse_raw_message(black_box(msg)).unwrap();
                black_box(result);
            });
        });
    }

    group.finish();
}

/// Benchmarks raw serde_json::Value parsing for comparison with struct parsing.
fn bench_json_value_parsing(c: &mut Criterion) {
    let mut group = c.benchmark_group("Deribit JSON Value Parsing");

    let messages = [
        ("trades", TRADES),
        ("book_snapshot", BOOK_SNAPSHOT),
        ("book_delta", BOOK_DELTA),
        ("ticker", TICKER),
        ("quote", QUOTE),
        ("subscribe_response", SUBSCRIBE_RESPONSE),
        ("error", ERROR),
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

/// Benchmarks the JSON-RPC method field detection (key discriminator in parse_raw_message).
fn bench_jsonrpc_method_detection(c: &mut Criterion) {
    let mut group = c.benchmark_group("Deribit JSON-RPC Method Detection");

    let messages = [
        ("subscription", TRADES),
        ("response_with_id", SUBSCRIBE_RESPONSE),
        ("error_response", ERROR),
    ];

    for (name, msg) in &messages {
        group.bench_with_input(BenchmarkId::new("detect_type", name), msg, |b, msg| {
            b.iter(|| {
                let value: Value = serde_json::from_str(black_box(msg)).unwrap();
                // Simulate the method detection logic from parse_raw_message
                let msg_type = if let Some(method) = value.get("method").and_then(|m| m.as_str()) {
                    match method {
                        "subscription" => "notification",
                        "heartbeat" => "heartbeat",
                        _ => "unknown_method",
                    }
                } else if value.get("id").is_some() {
                    if value.get("error").is_some() {
                        "error"
                    } else {
                        "response"
                    }
                } else {
                    "unknown"
                };
                black_box(msg_type);
            });
        });
    }

    group.finish();
}

/// Benchmarks batch processing of mixed message types.
fn bench_batch_processing(c: &mut Criterion) {
    let mut group = c.benchmark_group("Deribit Batch Processing");

    let batch_sizes = [10, 100, 1000];

    // Weighted toward book deltas (most frequent in real trading)
    let messages = [TRADES, BOOK_DELTA, BOOK_DELTA, BOOK_DELTA, TICKER];

    for batch_size in batch_sizes {
        group.throughput(Throughput::Elements(batch_size as u64));

        group.bench_with_input(
            BenchmarkId::new("mixed_messages", batch_size),
            &batch_size,
            |b, &size| {
                b.iter(|| {
                    for i in 0..size {
                        let msg = messages[i % messages.len()];
                        let result = parse_raw_message(msg).unwrap();
                        black_box(result);
                    }
                });
            },
        );
    }

    group.finish();
}

/// Benchmarks high-frequency book delta processing.
fn bench_book_deltas_batch(c: &mut Criterion) {
    let mut group = c.benchmark_group("Deribit Book Deltas Batch");

    let batch_sizes = [10, 100, 1000];

    for batch_size in batch_sizes {
        group.throughput(Throughput::Elements(batch_size as u64));

        group.bench_with_input(
            BenchmarkId::new("book_deltas_only", batch_size),
            &batch_size,
            |b, &size| {
                b.iter(|| {
                    for _ in 0..size {
                        let result = parse_raw_message(BOOK_DELTA).unwrap();
                        black_box(result);
                    }
                });
            },
        );
    }

    group.finish();
}

/// Benchmarks variant matching after parsing (simulates handler routing).
fn bench_message_routing(c: &mut Criterion) {
    let mut group = c.benchmark_group("Deribit Message Routing");

    let messages = [
        ("trades", TRADES),
        ("book_delta", BOOK_DELTA),
        ("ticker", TICKER),
        ("response", SUBSCRIBE_RESPONSE),
        ("error", ERROR),
    ];

    for (name, msg) in &messages {
        group.bench_with_input(BenchmarkId::new("parse_and_route", name), msg, |b, msg| {
            b.iter(|| {
                let ws_msg = parse_raw_message(black_box(msg)).unwrap();
                // Simulate routing by matching on variant
                let route = match &ws_msg {
                    DeribitWsMessage::Notification(notif) => {
                        // Access channel to simulate routing decision
                        let _ = &notif.params.channel;
                        "data_handler"
                    }
                    DeribitWsMessage::Heartbeat(_) => "heartbeat_handler",
                    DeribitWsMessage::Response(_) => "response_handler",
                    DeribitWsMessage::Error(_) => "error_handler",
                    DeribitWsMessage::Reconnected => "reconnect_handler",
                };
                black_box(route);
            });
        });
    }

    group.finish();
}

/// Benchmarks WebSocket message text extraction patterns.
fn bench_tungstenite_message_access(c: &mut Criterion) {
    let mut group = c.benchmark_group("Deribit Message Access");

    let text_msg = Message::Text(TRADES.into());
    let binary_msg = Message::Binary(TRADES.as_bytes().into());

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

/// Direct comparison of parse_raw_message vs raw Value parsing.
fn bench_parsing_comparison(c: &mut Criterion) {
    let mut group = c.benchmark_group("Deribit Parsing Comparison");

    // Book delta comparison (most common message)
    group.bench_function("book_delta_parse_raw_message", |b| {
        b.iter(|| {
            let result = parse_raw_message(black_box(BOOK_DELTA)).unwrap();
            black_box(result);
        });
    });

    group.bench_function("book_delta_to_value", |b| {
        b.iter(|| {
            let value: Value = serde_json::from_str(black_box(BOOK_DELTA)).unwrap();
            black_box(value);
        });
    });

    // Trades comparison
    group.bench_function("trades_parse_raw_message", |b| {
        b.iter(|| {
            let result = parse_raw_message(black_box(TRADES)).unwrap();
            black_box(result);
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

/// Benchmarks typed decoding of payloads as the WebSocket handler and HTTP client read them.
fn bench_payload_decode(c: &mut Criterion) {
    let mut group = c.benchmark_group("Deribit Payload Decode");

    let instrument = perpetual_instrument();
    let book_change = book_change_payload(25);
    let trades = notification_data(TRADES);
    let ticker = notification_data(TICKER);

    group.bench_function("book_change_50_levels", |b| {
        b.iter(|| {
            let msg = serde_json::from_str::<DeribitBookMsg>(black_box(&book_change)).unwrap();
            let deltas = parse_book_msg(&msg, &instrument, UnixNanos::default()).unwrap();
            black_box(deltas);
        });
    });

    group.bench_function("trades", |b| {
        b.iter(|| {
            let msgs = serde_json::from_str::<Vec<DeribitTradeMsg>>(black_box(&trades)).unwrap();
            black_box(msgs);
        });
    });

    group.bench_function("ticker", |b| {
        b.iter(|| {
            let msg = serde_json::from_str::<DeribitTickerMsg>(black_box(&ticker)).unwrap();
            black_box(msg);
        });
    });

    group.bench_function("http_instruments", |b| {
        b.iter(|| {
            let response =
                serde_json::from_slice::<DeribitJsonRpcResponse<Vec<DeribitInstrument>>>(
                    black_box(INSTRUMENTS.as_bytes()),
                )
                .unwrap();
            black_box(response);
        });
    });

    group.finish();
}

fn perpetual_instrument() -> InstrumentAny {
    let response: DeribitJsonRpcResponse<Vec<DeribitInstrument>> =
        serde_json::from_str(INSTRUMENTS).unwrap();
    let instrument = &response.result.unwrap()[0];
    parse_deribit_instrument_any(instrument, UnixNanos::default(), UnixNanos::default())
        .unwrap()
        .unwrap()
}

fn notification_data(message: &str) -> String {
    match parse_raw_message(message).unwrap() {
        DeribitWsMessage::Notification(notification) => notification.params.data.get().to_string(),
        _ => panic!("expected a notification"),
    }
}

// Mixes new, change, and delete levels with BTC-PERPETUAL tick and lot sizes
fn book_change_payload(levels_per_side: usize) -> String {
    let side = |start: f64, step: f64| {
        (0..levels_per_side)
            .map(|i| {
                let price = start + step * i as f64;
                match i % 5 {
                    0 => format!(r#"["delete",{price:.1},0.0]"#),
                    1 | 2 => format!(r#"["new",{price:.1},{}.0]"#, 100 + i * 10),
                    _ => format!(r#"["change",{price:.1},{}.0]"#, 100 + i * 10),
                }
            })
            .collect::<Vec<_>>()
            .join(",")
    };

    format!(
        r#"{{"type":"change","instrument_name":"BTC-PERPETUAL","timestamp":1699999999500,"change_id":123456790,"prev_change_id":123456789,"bids":[{}],"asks":[{}]}}"#,
        side(42500.0, -0.5),
        side(42500.5, 0.5),
    )
}

criterion_group!(
    benches,
    bench_parse_raw_message,
    bench_json_value_parsing,
    bench_jsonrpc_method_detection,
    bench_batch_processing,
    bench_book_deltas_batch,
    bench_message_routing,
    bench_tungstenite_message_access,
    bench_parsing_comparison,
    bench_payload_decode,
);
criterion_main!(benches);
