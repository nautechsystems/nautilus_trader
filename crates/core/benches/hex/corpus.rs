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

//! A varied corpus supplements the existing repeated-byte hex fixtures.

use std::hint::black_box;

use criterion::{BenchmarkId, Criterion, Throughput};
use nautilus_core::hex;

pub(super) fn bench_corpus(c: &mut Criterion) {
    let mut group = c.benchmark_group("hex_corpus");

    for len in [32, 64, 256] {
        // Deterministic strides rotate varied bytes across 64 inputs.
        let inputs: Vec<Vec<u8>> = (0..64)
            .map(|seed| {
                (0..len)
                    .map(|i| ((seed * 37 + i * 73) % 256) as u8)
                    .collect()
            })
            .collect();
        let encoded: Vec<String> = inputs
            .iter()
            .map(|bytes| bytes.iter().map(|byte| format!("{byte:02x}")).collect())
            .collect();

        for (bytes, expected) in inputs.iter().zip(&encoded) {
            assert_eq!(hex::encode(bytes), *expected);
            assert_eq!(hex::encode_prefixed(bytes), format!("0x{expected}"));
            assert_eq!(hex::decode(expected).unwrap(), *bytes);
        }
        group.throughput(Throughput::Elements(len as u64));
        group.bench_with_input(BenchmarkId::new("encode", len), &inputs, |b, inputs| {
            let mut cursor = 0;
            b.iter(|| {
                let result = hex::encode(black_box(&inputs[cursor]));
                cursor = (cursor + 1) % inputs.len();
                black_box(result)
            });
        });
        group.bench_with_input(BenchmarkId::new("prefixed", len), &inputs, |b, inputs| {
            let mut cursor = 0;
            b.iter(|| {
                let result = hex::encode_prefixed(black_box(&inputs[cursor]));
                cursor = (cursor + 1) % inputs.len();
                black_box(result)
            });
        });
        group.bench_with_input(BenchmarkId::new("decode", len), &encoded, |b, inputs| {
            let mut cursor = 0;
            b.iter(|| {
                let result = hex::decode(black_box(&inputs[cursor])).unwrap();
                cursor = (cursor + 1) % inputs.len();
                black_box(result)
            });
        });
    }
    group.finish();
}
