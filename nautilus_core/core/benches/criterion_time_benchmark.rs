// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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

use criterion::{criterion_group, Criterion};
use nautilus_core::time::unix_timestamp_ns;

#[allow(clippy::redundant_closure)]
pub fn criterion_time_benchmark(c: &mut Criterion) {
    c.bench_function("f64_to_fixed_i64", |b| b.iter(|| unix_timestamp_ns()));
}

criterion_group!(benches, criterion_time_benchmark);
criterion::criterion_main!(benches);
