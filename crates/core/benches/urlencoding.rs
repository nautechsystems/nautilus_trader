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

use criterion::{Criterion, criterion_group, criterion_main};
use nautilus_core::string::urlencoding;

// Realistic call-site inputs. JSON payloads come from tardis options encoding,
// ISO timestamps from dydx query params, tokens from betfair form bodies.
const UNRESERVED_SHORT: &str = "some-user_name.123~example";
const UNRESERVED_LONG: &str = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789-._~";
const ISO_TIMESTAMP: &str = "2024-01-01T00:00:00.000Z";
const JSON_PAYLOAD: &str = r#"{"exchange":"binance","symbols":["BTCUSDT","ETHUSDT"],"dataTypes":["trade","book_change","derivative_ticker"],"from":"2024-01-01T00:00:00.000Z","to":"2024-01-02T00:00:00.000Z"}"#;
const UTF8_PAYLOAD: &str =
    "\u{00E9}\u{00E0}\u{00FC} \u{4E2D}\u{6587}\u{6D4B}\u{8BD5} \u{1F600}\u{1F680}";

fn bench_encode_unreserved_short(c: &mut Criterion) {
    c.bench_function("urlencoding::encode (unreserved, short)", |b| {
        b.iter(|| urlencoding::encode(black_box(UNRESERVED_SHORT)));
    });
}

fn bench_encode_unreserved_long(c: &mut Criterion) {
    c.bench_function("urlencoding::encode (unreserved, long)", |b| {
        b.iter(|| urlencoding::encode(black_box(UNRESERVED_LONG)));
    });
}

fn bench_encode_iso_timestamp(c: &mut Criterion) {
    c.bench_function("urlencoding::encode (ISO timestamp)", |b| {
        b.iter(|| urlencoding::encode(black_box(ISO_TIMESTAMP)));
    });
}

fn bench_encode_json_payload(c: &mut Criterion) {
    c.bench_function("urlencoding::encode (JSON payload, ~200 bytes)", |b| {
        b.iter(|| urlencoding::encode(black_box(JSON_PAYLOAD)));
    });
}

fn bench_encode_utf8(c: &mut Criterion) {
    c.bench_function("urlencoding::encode (UTF-8 mixed)", |b| {
        b.iter(|| urlencoding::encode(black_box(UTF8_PAYLOAD)));
    });
}

fn bench_encode_all_reserved(c: &mut Criterion) {
    let input = "!@#$%^&*(){}[]|\\:;\"'<>,?/".repeat(8);
    c.bench_function("urlencoding::encode (all reserved, 200 bytes)", |b| {
        b.iter(|| urlencoding::encode(black_box(&input)));
    });
}

fn bench_decode_no_percent(c: &mut Criterion) {
    c.bench_function("urlencoding::decode (no percent, fast path)", |b| {
        b.iter(|| urlencoding::decode(black_box(UNRESERVED_LONG)));
    });
}

fn bench_decode_json_payload(c: &mut Criterion) {
    let encoded = urlencoding::encode(JSON_PAYLOAD).into_owned();
    c.bench_function("urlencoding::decode (encoded JSON, ~300 bytes)", |b| {
        b.iter(|| urlencoding::decode(black_box(&encoded)));
    });
}

fn bench_decode_utf8(c: &mut Criterion) {
    let encoded = urlencoding::encode(UTF8_PAYLOAD).into_owned();
    c.bench_function("urlencoding::decode (encoded UTF-8)", |b| {
        b.iter(|| urlencoding::decode(black_box(&encoded)));
    });
}

fn bench_roundtrip_iso_timestamp(c: &mut Criterion) {
    c.bench_function("urlencoding roundtrip (ISO timestamp)", |b| {
        b.iter(|| {
            let encoded = urlencoding::encode(black_box(ISO_TIMESTAMP));
            urlencoding::decode(&encoded).unwrap().into_owned()
        });
    });
}

criterion_group!(
    benches,
    bench_encode_unreserved_short,
    bench_encode_unreserved_long,
    bench_encode_iso_timestamp,
    bench_encode_json_payload,
    bench_encode_utf8,
    bench_encode_all_reserved,
    bench_decode_no_percent,
    bench_decode_json_payload,
    bench_decode_utf8,
    bench_roundtrip_iso_timestamp,
);
criterion_main!(benches);
