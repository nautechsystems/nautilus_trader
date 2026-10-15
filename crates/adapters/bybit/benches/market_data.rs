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

#![deny(unsafe_code)]

use criterion::{criterion_group, criterion_main};

// The included source also contains unit fixtures unused by the benchmark harness
#[allow(dead_code, unused_imports)]
#[path = "../src/lib.rs"]
mod adapter;

pub use adapter::*;
pub(crate) use adapter::{book, repay};

criterion_group!(benches, data::tests::benchmarks::bench_quotes);
criterion_main!(benches);
