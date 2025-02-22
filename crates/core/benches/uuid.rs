// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

use criterion::{Criterion, criterion_group, criterion_main};
use nautilus_core::UUID4;
use uuid::Uuid;

fn bench_uuid4_new(c: &mut Criterion) {
    c.bench_function("UUID4::new", |b| b.iter(UUID4::new));
}

fn bench_uuid_crate_new_v4(c: &mut Criterion) {
    c.bench_function("Uuid::new", |b| b.iter(Uuid::new_v4));
}

fn bench_uuid4_to_string(c: &mut Criterion) {
    let uuid = UUID4::new();
    c.bench_function("UUID4::to_string", |b| b.iter(|| uuid.to_string()));
}

fn bench_uuid4_from_str(c: &mut Criterion) {
    let uuid_string = "2d89666b-1a1e-4a75-b193-4eb3b454c757";
    c.bench_function("UUID4::from_str", |b| b.iter(|| UUID4::from(uuid_string)));
}

fn bench_uuid4_serialize(c: &mut Criterion) {
    let uuid = UUID4::new();
    c.bench_function("UUID4::serialize", |b| {
        b.iter(|| serde_json::to_string(&uuid).expect("Serialization failed"));
    });
}

fn bench_uuid4_deserialize(c: &mut Criterion) {
    let uuid = UUID4::new();
    let serialized = serde_json::to_string(&uuid).expect("Serialization failed");
    c.bench_function("UUID4::deserialize", |b| {
        b.iter(|| {
            let _: UUID4 = serde_json::from_str(&serialized).expect("Deserialization failed");
        });
    });
}

fn bench_uuid4_round_trip(c: &mut Criterion) {
    let uuid = UUID4::new();
    c.bench_function("UUID4::round_trip", |b| {
        b.iter(|| {
            let serialized = serde_json::to_string(&uuid).expect("Serialization failed");
            let _: UUID4 = serde_json::from_str(&serialized).expect("Deserialization failed");
        });
    });
}

criterion_group!(
    benches,
    bench_uuid4_new,
    bench_uuid_crate_new_v4,
    bench_uuid4_to_string,
    bench_uuid4_from_str,
    bench_uuid4_serialize,
    bench_uuid4_deserialize,
    bench_uuid4_round_trip,
);
criterion_main!(benches);
