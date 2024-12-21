// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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

use criterion::{criterion_group, criterion_main, Criterion};
use nautilus_core::uuid::UUID4;

fn bench_uuid4_creation(c: &mut Criterion) {
    c.bench_function("UUID4::new", |b| b.iter(UUID4::new));
}

fn bench_uuid4_to_string(c: &mut Criterion) {
    let uuid = UUID4::new();
    c.bench_function("UUID4::to_string", |b| b.iter(|| uuid.to_string()));
}

fn bench_uuid4_from_str(c: &mut Criterion) {
    let uuid_string = "6ba7b810-9dad-11d1-80b4-00c04fd430c8";
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
    bench_uuid4_creation,
    bench_uuid4_to_string,
    bench_uuid4_from_str,
    bench_uuid4_serialize,
    bench_uuid4_deserialize,
    bench_uuid4_round_trip,
);
criterion_main!(benches);
