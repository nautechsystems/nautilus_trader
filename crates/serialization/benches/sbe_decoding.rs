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
use nautilus_serialization::sbe::SbeCursor;

fn make_i64_buffer(count: usize) -> Vec<u8> {
    let mut buf = Vec::with_capacity(count * 8);
    for i in 0..count {
        let value = i64::try_from(i).expect("benchmark index must fit in i64");
        buf.extend_from_slice(&value.to_le_bytes());
    }
    buf
}

fn make_var_string8_buffer(count: usize, value: &str) -> Vec<u8> {
    let bytes = value.as_bytes();
    let len = u8::try_from(bytes.len()).expect("value must fit in varString8");
    let mut buf = Vec::with_capacity(count * (usize::from(len) + 1));

    for _ in 0..count {
        buf.push(len);
        buf.extend_from_slice(bytes);
    }
    buf
}

fn make_group_buffer(count: u32) -> Vec<u8> {
    let mut buf = Vec::with_capacity(6 + count as usize * 16);
    buf.extend_from_slice(&16u16.to_le_bytes()); // block_length
    buf.extend_from_slice(&count.to_le_bytes()); // num_in_group

    for i in 0..count {
        buf.extend_from_slice(&(10_000 + i64::from(i)).to_le_bytes());
        buf.extend_from_slice(&(20_000 + i64::from(i)).to_le_bytes());
    }

    buf
}

fn bench_read_i64(c: &mut Criterion) {
    let count = 1024;
    let data = make_i64_buffer(count);

    c.bench_function("SbeCursor::read_i64_le x1024", |b| {
        b.iter(|| {
            let mut cursor = SbeCursor::new(&data);
            let mut sum = 0i64;

            for _ in 0..count {
                sum += cursor.read_i64_le().unwrap();
            }

            black_box(sum)
        });
    });
}

fn bench_read_var_string8_ref(c: &mut Criterion) {
    let count = 512;
    let data = make_var_string8_buffer(count, "BTCUSDT");

    c.bench_function("SbeCursor::read_var_string8_ref x512", |b| {
        b.iter(|| {
            let mut cursor = SbeCursor::new(&data);
            let mut total_len = 0usize;

            for _ in 0..count {
                total_len += cursor.read_var_string8_ref().unwrap().len();
            }

            black_box(total_len)
        });
    });
}

fn bench_read_group(c: &mut Criterion) {
    let data = make_group_buffer(256);

    c.bench_function("SbeCursor::read_group (256 levels)", |b| {
        b.iter(|| {
            let mut cursor = SbeCursor::new(&data);
            let (block_length, count) = cursor.read_group_header().unwrap();

            let levels = cursor
                .read_group(block_length, count, |cur| {
                    let price = cur.read_i64_le()?;
                    let qty = cur.read_i64_le()?;
                    Ok((price, qty))
                })
                .unwrap();

            black_box(levels.len())
        });
    });
}

criterion_group!(
    sbe_cursor_benches,
    bench_read_i64,
    bench_read_var_string8_ref,
    bench_read_group
);
criterion_main!(sbe_cursor_benches);
