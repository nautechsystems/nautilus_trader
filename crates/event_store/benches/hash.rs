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

//! Benchmarks for [`nautilus_event_store::hash::compute_entry_hash`].
//!
//! The hash runs on every captured entry and again on every read, so its throughput sets a
//! floor for capture and replay. We sweep payload sizes that bracket the SPEC's storage
//! benchmark assumption (256 B), the typical command/event size (~1 KB), and a fat-payload
//! upper bound (~4 KB) to keep the curve visible if BLAKE3 ever regresses.

use std::hint::black_box;

use bytes::Bytes;
use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use nautilus_core::{UUID4, UnixNanos};
use nautilus_event_store::{Headers, Topic, compute_entry_hash};
use ustr::Ustr;

const PAYLOAD_SIZES: &[usize] = &[256, 1024, 4096];

fn payload_of(size: usize) -> Bytes {
    Bytes::from(vec![0xABu8; size])
}

fn populated_headers() -> Headers {
    Headers {
        correlation_id: Some(UUID4::new()),
        causation_id: Some(UUID4::new()),
    }
}

fn bench_compute_entry_hash(c: &mut Criterion) {
    let mut group = c.benchmark_group("compute_entry_hash");

    let topic: Topic = "exec.command.SubmitOrder".into();
    let payload_type = Ustr::from("SubmitOrder");
    let headers = Headers::empty();
    let ts_init = UnixNanos::from(1_700_000_000_000_000_000);
    let ts_publish = UnixNanos::from(1_700_000_000_000_000_001);

    for &size in PAYLOAD_SIZES {
        let payload = payload_of(size);
        group.throughput(Throughput::Bytes(size as u64));
        group.bench_with_input(BenchmarkId::new("empty_headers", size), &size, |b, _| {
            b.iter(|| {
                let hash = compute_entry_hash(
                    black_box(42),
                    black_box(ts_init),
                    black_box(ts_publish),
                    black_box(topic.as_ref()),
                    black_box(payload_type.as_str()),
                    black_box(&payload),
                    black_box(&headers),
                );
                black_box(hash)
            });
        });
    }

    let headers = populated_headers();

    for &size in PAYLOAD_SIZES {
        let payload = payload_of(size);
        group.throughput(Throughput::Bytes(size as u64));
        group.bench_with_input(BenchmarkId::new("full_headers", size), &size, |b, _| {
            b.iter(|| {
                let hash = compute_entry_hash(
                    black_box(42),
                    black_box(ts_init),
                    black_box(ts_publish),
                    black_box(topic.as_ref()),
                    black_box(payload_type.as_str()),
                    black_box(&payload),
                    black_box(&headers),
                );
                black_box(hash)
            });
        });
    }

    group.finish();
}

criterion_group!(benches, bench_compute_entry_hash);
criterion_main!(benches);
