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

use std::hash::{DefaultHasher, Hash, Hasher};

use iai::black_box;
use nautilus_core::StackStr;

fn bench_stackstr_new_short() {
    black_box(StackStr::new("BINANCE"));
}

fn bench_stackstr_new_medium() {
    black_box(StackStr::new("O-20231215-001-001"));
}

fn bench_stackstr_new_max() {
    black_box(StackStr::new("xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx")); // 36 chars
}

fn bench_stackstr_eq_same() {
    let a = StackStr::new("O-20231215-001-001");
    let b = StackStr::new("O-20231215-001-001");
    black_box(a == b);
}

fn bench_stackstr_eq_different() {
    let a = StackStr::new("O-20231215-001-001");
    let b = StackStr::new("O-20231215-001-002");
    black_box(a == b);
}

fn bench_stackstr_hash() {
    let s = StackStr::new("O-20231215-001-001");
    let mut hasher = DefaultHasher::new();
    s.hash(&mut hasher);
    black_box(hasher.finish());
}

fn bench_stackstr_as_str() {
    let s = StackStr::new("O-20231215-001-001");
    black_box(s.as_str());
}

fn bench_stackstr_clone() {
    let s = StackStr::new("O-20231215-001-001");
    black_box(s);
}

fn bench_stackstr_from_bytes() {
    black_box(StackStr::from_bytes(b"O-20231215-001-001")).unwrap();
}

fn bench_stackstr_cmp() {
    let a = StackStr::new("AAA-001");
    let b = StackStr::new("ZZZ-999");
    black_box(a.cmp(&b));
}

iai::main!(
    bench_stackstr_new_short,
    bench_stackstr_new_medium,
    bench_stackstr_new_max,
    bench_stackstr_eq_same,
    bench_stackstr_eq_different,
    bench_stackstr_hash,
    bench_stackstr_as_str,
    bench_stackstr_clone,
    bench_stackstr_from_bytes,
    bench_stackstr_cmp,
);
