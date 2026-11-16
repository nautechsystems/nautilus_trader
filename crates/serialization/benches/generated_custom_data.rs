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
use nautilus_model::{custom_data, data::Data, identifiers::InstrumentId};
use nautilus_serialization::{
    arrow::{DecodeDataFromRecordBatch, EncodeToRecordBatch},
    arrow_custom_data,
};

#[arrow_custom_data]
#[custom_data]
struct CodecData {
    instrument_id: InstrumentId,
    label: String,
    value: f64,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
}

fn bench_custom_data(c: &mut Criterion) {
    for len in [1, 64, 4096] {
        for pattern in ["repeated", "alternating", "unique", "long_text"] {
            let rows: Vec<_> = (0..len).map(|i| row(i, pattern)).collect();
            let metadata = rows[0].metadata();
            let batch = CodecData::encode_batch(&metadata, &rows).unwrap();
            let decoded = CodecData::decode_data_batch(&metadata, batch.clone()).unwrap();
            assert_eq!(decoded.len(), rows.len());

            for (actual, expected) in decoded.iter().zip(&rows) {
                let Data::Custom(custom) = actual else {
                    panic!("expected custom data");
                };

                assert_eq!(
                    custom.data.as_any().downcast_ref::<CodecData>().unwrap(),
                    expected
                );
            }

            let mut group = c.benchmark_group(format!("generated_custom_data/{pattern}"));
            group.throughput(Throughput::Elements(len as u64));
            group.bench_with_input(BenchmarkId::new("encode", len), &rows, |b, rows| {
                b.iter(|| {
                    black_box(
                        CodecData::encode_batch(black_box(&metadata), black_box(rows)).unwrap(),
                    )
                });
            });

            group.bench_with_input(BenchmarkId::new("decode", len), &batch, |b, batch| {
                b.iter(|| {
                    black_box(
                        CodecData::decode_data_batch(
                            black_box(&metadata),
                            black_box(batch.clone()),
                        )
                        .unwrap(),
                    )
                });
            });

            group.finish();
        }
    }
}

fn row(i: usize, pattern: &str) -> CodecData {
    let key = match pattern {
        "alternating" => i % 2,
        "unique" => i,
        _ => 0,
    };

    CodecData {
        instrument_id: InstrumentId::from(format!("OPTION-{key:06}.DERIBIT").as_str()),
        label: if pattern == "long_text" {
            "yield-curve".repeat(32)
        } else {
            "USD".to_owned()
        },
        value: i as f64 + 0.125,
        ts_event: (i as u64 * 10 + 1).into(),
        ts_init: (i as u64 * 10 + 3).into(),
    }
}

criterion_group!(benches, bench_custom_data);
criterion_main!(benches);
