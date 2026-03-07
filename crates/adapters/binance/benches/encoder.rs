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
use nautilus_binance::common::{
    consts::BINANCE_NAUTILUS_SPOT_BROKER_ID,
    encoder::{decode_broker_id, encode_broker_id},
};
use nautilus_model::identifiers::ClientOrderId;

const BROKER_ID: &str = BINANCE_NAUTILUS_SPOT_BROKER_ID;

fn bench_encode_o_format(c: &mut Criterion) {
    let coid = ClientOrderId::from("O-20260305-120000-001-001-100");

    c.bench_function("encode_o_format", |b| {
        b.iter(|| encode_broker_id(black_box(&coid), BROKER_ID));
    });
}

fn bench_decode_o_format(c: &mut Criterion) {
    let coid = ClientOrderId::from("O-20260305-120000-001-001-100");
    let encoded = encode_broker_id(&coid, BROKER_ID);

    c.bench_function("decode_o_format", |b| {
        b.iter(|| decode_broker_id(black_box(&encoded), BROKER_ID));
    });
}

fn bench_encode_uuid(c: &mut Criterion) {
    let coid = ClientOrderId::from("550e8400-e29b-41d4-a716-446655440000");

    c.bench_function("encode_uuid", |b| {
        b.iter(|| encode_broker_id(black_box(&coid), BROKER_ID));
    });
}

fn bench_decode_uuid(c: &mut Criterion) {
    let coid = ClientOrderId::from("550e8400-e29b-41d4-a716-446655440000");
    let encoded = encode_broker_id(&coid, BROKER_ID);

    c.bench_function("decode_uuid", |b| {
        b.iter(|| decode_broker_id(black_box(&encoded), BROKER_ID));
    });
}

fn bench_encode_raw(c: &mut Criterion) {
    let coid = ClientOrderId::from("my-order-123");

    c.bench_function("encode_raw", |b| {
        b.iter(|| encode_broker_id(black_box(&coid), BROKER_ID));
    });
}

fn bench_decode_raw(c: &mut Criterion) {
    let coid = ClientOrderId::from("my-order-123");
    let encoded = encode_broker_id(&coid, BROKER_ID);

    c.bench_function("decode_raw", |b| {
        b.iter(|| decode_broker_id(black_box(&encoded), BROKER_ID));
    });
}

fn bench_decode_passthrough(c: &mut Criterion) {
    let non_prefixed = "O-20260305-120000-001-001-100";

    c.bench_function("decode_passthrough", |b| {
        b.iter(|| decode_broker_id(black_box(non_prefixed), BROKER_ID));
    });
}

criterion_group!(
    benches,
    bench_encode_o_format,
    bench_decode_o_format,
    bench_encode_uuid,
    bench_decode_uuid,
    bench_encode_raw,
    bench_decode_raw,
    bench_decode_passthrough,
);
criterion_main!(benches);
