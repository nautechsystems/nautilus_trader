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

//! Redis message bus hot-path benchmarks.

use std::hint::black_box;

use bytes::Bytes;
use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use nautilus_common::{
    enums::SerializationEncoding,
    msgbus::{BusMessage, BusPayloadType},
};
use ustr::Ustr;

#[path = "../src/redis/stream_fields.rs"]
mod stream_fields;

const MESSAGE_BATCH_SIZE: u64 = 16;

fn bench_bus_message_fields(c: &mut Criterion) {
    let custom = message(BusPayloadType::Custom(Ustr::from("CustomData")));
    let typed = message(BusPayloadType::TradingCommand);
    let mut group = c.benchmark_group("Redis message bus/fields");
    group.throughput(Throughput::Elements(MESSAGE_BATCH_SIZE));

    group.bench_function("custom", |b| {
        b.iter(|| {
            for _ in 0..MESSAGE_BATCH_SIZE {
                let fields =
                    stream_fields::bus_message_fields(black_box(&custom), black_box("json"));
                black_box(fields.as_slice());
            }
        });
    });
    group.bench_function("typed", |b| {
        b.iter(|| {
            for _ in 0..MESSAGE_BATCH_SIZE {
                let fields =
                    stream_fields::bus_message_fields(black_box(&typed), black_box("json"));
                black_box(fields.as_slice());
            }
        });
    });

    group.finish();
}

fn message(payload_type: BusPayloadType) -> BusMessage {
    BusMessage::with_str_topic(
        "commands.execution.EXTERNAL",
        payload_type,
        Bytes::from(vec![0xAB; 256]),
        SerializationEncoding::Json,
    )
}

criterion_group!(benches, bench_bus_message_fields);
criterion_main!(benches);
