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
use nautilus_core::hex;

fn bench_encode_32(c: &mut Criterion) {
    let data = [0xdeu8; 32];
    c.bench_function("hex::encode (32 bytes)", |b| {
        b.iter(|| hex::encode(black_box(&data)));
    });
}

fn bench_encode_64(c: &mut Criterion) {
    let data = [0xabu8; 64];
    c.bench_function("hex::encode (64 bytes)", |b| {
        b.iter(|| hex::encode(black_box(&data)));
    });
}

fn bench_encode_256(c: &mut Criterion) {
    let data = [0x42u8; 256];
    c.bench_function("hex::encode (256 bytes)", |b| {
        b.iter(|| hex::encode(black_box(&data)));
    });
}

fn bench_decode_64(c: &mut Criterion) {
    let hex_str = "ab".repeat(32);
    c.bench_function("hex::decode (64 chars)", |b| {
        b.iter(|| hex::decode(black_box(hex_str.as_bytes())));
    });
}

fn bench_decode_128(c: &mut Criterion) {
    let hex_str = "cd".repeat(64);
    c.bench_function("hex::decode (128 chars)", |b| {
        b.iter(|| hex::decode(black_box(hex_str.as_bytes())));
    });
}

fn bench_decode_512(c: &mut Criterion) {
    let hex_str = "ef".repeat(256);
    c.bench_function("hex::decode (512 chars)", |b| {
        b.iter(|| hex::decode(black_box(hex_str.as_bytes())));
    });
}

fn bench_encode_prefixed_32(c: &mut Criterion) {
    let data = [0xdeu8; 32];
    c.bench_function("hex::encode_prefixed (32 bytes)", |b| {
        b.iter(|| hex::encode_prefixed(black_box(&data)));
    });
}

fn bench_decode_array_32(c: &mut Criterion) {
    let hex_str = "ab".repeat(32);
    c.bench_function("hex::decode_array::<32> (64 chars)", |b| {
        b.iter(|| hex::decode_array::<32>(black_box(hex_str.as_bytes())));
    });
}

fn bench_roundtrip_32(c: &mut Criterion) {
    let data = [0x7fu8; 32];
    c.bench_function("hex roundtrip (32 bytes)", |b| {
        b.iter(|| {
            let encoded = hex::encode(black_box(&data));
            hex::decode(encoded.as_bytes())
        });
    });
}

criterion_group!(
    benches,
    bench_encode_32,
    bench_encode_64,
    bench_encode_256,
    bench_encode_prefixed_32,
    bench_decode_64,
    bench_decode_128,
    bench_decode_512,
    bench_decode_array_32,
    bench_roundtrip_32,
);
criterion_main!(benches);
