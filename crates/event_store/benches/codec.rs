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

//! Benchmarks for the on-disk envelope codec.
//!
//! Every captured entry is codec-serialized before the redb commit and codec-deserialized on
//! every read. The writer batches up to 100 entries per commit, so per-entry codec cost multiplies
//! into the commit-latency budget the SPEC's storage benchmark allocates (5 ms p50 at the default
//! batch size). These benches keep that cost visible.
//!
//! One-time bincode 2.0.1 baseline captured before removing the dependency with a release-mode
//! scratch harness on 2026-06-26 (50,000 iterations per operation, payload sizes below):
//! 256 bytes: bincode 349 B, codec 378 B; encode 126.3 ns vs 178.0 ns; decode 153.5 ns vs 218.3 ns.
//! 1024 bytes: bincode 1117 B, codec 1146 B; encode 196.2 ns vs 261.9 ns; decode 163.4 ns vs 216.0 ns.
//! 4096 bytes: bincode 4189 B, codec 4218 B; encode 205.3 ns vs 256.0 ns; decode 201.1 ns vs 250.9 ns.

use std::hint::black_box;

use bytes::Bytes;
use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use nautilus_core::UnixNanos;
use nautilus_event_store::{EventStoreEntry, Headers, Topic, codec, compute_entry_hash};
use ustr::Ustr;

const PAYLOAD_SIZES: &[usize] = &[256, 1024, 4096];

fn entry_with_payload(size: usize) -> EventStoreEntry {
    let topic: Topic = "exec.command.SubmitOrder".into();
    let payload_type = Ustr::from("SubmitOrder");
    let payload = Bytes::from(vec![0xABu8; size]);
    let headers = Headers::empty();
    let ts_init = UnixNanos::from(1_700_000_000_000_000_000);
    let ts_publish = UnixNanos::from(1_700_000_000_000_000_001);
    let hash = compute_entry_hash(
        42,
        ts_init,
        ts_publish,
        topic.as_ref(),
        payload_type.as_str(),
        &payload,
        &headers,
    );
    EventStoreEntry::new(
        hash,
        42,
        headers,
        topic,
        payload_type,
        payload,
        ts_init,
        ts_publish,
    )
}

fn bench_serialize(c: &mut Criterion) {
    let mut group = c.benchmark_group("codec_serialize_entry");

    for &size in PAYLOAD_SIZES {
        let entry = entry_with_payload(size);
        group.throughput(Throughput::Bytes(size as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), &entry, |b, entry| {
            b.iter(|| {
                let bytes = codec::encode_to_vec(black_box(entry)).expect("serialize");
                black_box(bytes)
            });
        });
    }

    group.finish();
}

fn bench_deserialize(c: &mut Criterion) {
    let mut group = c.benchmark_group("codec_deserialize_entry");

    for &size in PAYLOAD_SIZES {
        let entry = entry_with_payload(size);
        let encoded = codec::encode_to_vec(&entry).expect("serialize");
        group.throughput(Throughput::Bytes(size as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), &encoded, |b, encoded| {
            b.iter(|| {
                let decoded = codec::decode_from_slice::<EventStoreEntry>(black_box(encoded))
                    .expect("deserialize");
                black_box(decoded)
            });
        });
    }

    group.finish();
}

fn bench_recompute_hash(c: &mut Criterion) {
    let mut group = c.benchmark_group("recompute_hash");

    for &size in PAYLOAD_SIZES {
        let entry = entry_with_payload(size);
        group.throughput(Throughput::Bytes(size as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), &entry, |b, entry| {
            b.iter(|| {
                let hash = entry.recompute_hash();
                black_box(hash)
            });
        });
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_serialize,
    bench_deserialize,
    bench_recompute_hash
);
criterion_main!(benches);
