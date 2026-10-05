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

//! Opaque varied inputs validate the legacy UUID benchmarks' fixed-input results.

use std::hint::black_box;

use criterion::Criterion;
use nautilus_core::UUID4;

pub(super) fn bench_corpus(c: &mut Criterion) {
    let inputs: Vec<_> = (0..64)
        .map(|seed| {
            let mut bytes = std::array::from_fn(|i| ((seed * 37 + i * 73) % 256) as u8);
            bytes[6] = (bytes[6] & 0x0f) | 0x40;
            bytes[8] = (bytes[8] & 0x3f) | 0x80;
            let uuid = UUID4::from_bytes(bytes);
            assert_eq!(uuid.as_bytes(), bytes);
            uuid
        })
        .collect();
    let texts: Vec<_> = inputs.iter().map(UUID4::to_string).collect();

    for (text, expected) in texts.iter().zip(&inputs) {
        assert_eq!(text.parse::<UUID4>().unwrap(), *expected);
    }

    c.bench_function("uuid_corpus/as_bytes", |b| {
        let mut cursor = 0;
        b.iter(|| {
            let result = black_box(&inputs[cursor]).as_bytes();
            cursor = (cursor + 1) % inputs.len();
            black_box(result)
        });
    });
    c.bench_function("uuid_corpus/from_str", |b| {
        let mut cursor = 0;
        b.iter(|| {
            let result = black_box(&texts[cursor]).parse::<UUID4>().unwrap();
            cursor = (cursor + 1) % texts.len();
            black_box(result)
        });
    });
}
