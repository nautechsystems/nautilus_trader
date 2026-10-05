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

//! Small inputs and error paths guard packed hex conversion costs.

use std::hint::black_box;

use criterion::{BenchmarkId, Criterion, Throughput};
use nautilus_core::hex::{self, DecodeError};

fn bench_small(c: &mut Criterion) {
    let mut group = c.benchmark_group("hex_edges");

    for len in [0, 1, 3, 4, 5, 7] {
        let bytes: Vec<u8> = (0..len).map(|i| u8::try_from(i * 37).unwrap()).collect();
        let text: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
        assert_eq!(hex::encode(&bytes), text);
        assert_eq!(hex::encode_prefixed(&bytes), format!("0x{text}"));
        assert_eq!(hex::decode(&text), Ok(bytes.clone()));
        group.throughput(Throughput::Elements(len as u64));
        group.bench_with_input(BenchmarkId::new("encode", len), &bytes, |b, input| {
            b.iter(|| black_box(hex::encode(black_box(input))));
        });
        group.bench_with_input(BenchmarkId::new("prefixed", len), &bytes, |b, input| {
            b.iter(|| black_box(hex::encode_prefixed(black_box(input))));
        });
        group.bench_with_input(BenchmarkId::new("decode", len), &text, |b, input| {
            b.iter(|| black_box(hex::decode(black_box(input))));
        });
    }
    group.finish();
}

fn bench_array<const N: usize>(c: &mut Criterion) {
    let input: String = (0..N).map(|i| format!("{:02X}", i * 37)).collect();
    let expected = std::array::from_fn::<_, N, _>(|i| u8::try_from(i * 37).unwrap());
    assert_eq!(hex::decode_array::<N>(&input), Ok(expected));
    c.bench_with_input(
        BenchmarkId::new("hex_edges/array", N),
        &input,
        |b, input| {
            b.iter(|| black_box(hex::decode_array::<N>(black_box(input))));
        },
    );
}

fn bench_errors(c: &mut Criterion) {
    let mut group = c.benchmark_group("hex_edges");

    for (name, position) in [("first", 0), ("middle", 32), ("last", 63)] {
        let mut input = vec![b'a'; 64];
        input[position] = b'z';
        assert_eq!(hex::decode(&input), Err(DecodeError::InvalidChar(b'z')));
        group.bench_with_input(BenchmarkId::new("invalid", name), &input, |b, input| {
            b.iter(|| black_box(hex::decode(black_box(input))));
        });
    }

    for (name, position) in [("high_first", 0), ("high_last", 63), ("multiple", 32)] {
        let mut input = vec![b'a'; 64];
        input[position] = 0xff;

        if name == "multiple" {
            input[position + 1] = b'z';
        }
        assert_eq!(hex::decode(&input), Err(DecodeError::InvalidChar(0xff)));
        group.bench_with_input(BenchmarkId::new("invalid", name), &input, |b, input| {
            b.iter(|| black_box(hex::decode(black_box(input))));
        });
    }
    let odd = vec![b'z'; 65];
    assert_eq!(hex::decode(&odd), Err(DecodeError::OddLength));
    group.bench_with_input("odd_length", &odd, |b, input| {
        b.iter(|| black_box(hex::decode(black_box(input))));
    });
    let mixed = "0123456789abcdefABCDEF0123456789ab";
    let expected: Vec<u8> = mixed
        .as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect();
    assert_eq!(hex::decode(mixed), Ok(expected));
    group.bench_with_input("mixed_case", &mixed, |b, input| {
        b.iter(|| black_box(hex::decode(black_box(input))));
    });
    group.finish();
}

pub(super) fn bench_edges(c: &mut Criterion) {
    bench_small(c);
    bench_array::<3>(c);
    bench_array::<4>(c);
    bench_array::<5>(c);
    bench_errors(c);
}
