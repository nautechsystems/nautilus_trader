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

use std::collections::{HashMap, HashSet};

use iai::{black_box, main};
use indexmap::IndexMap;
use nautilus_core::correctness::{
    check_equal, check_in_range_inclusive_u8, check_in_range_inclusive_u64, check_key_in_map,
    check_key_not_in_map, check_map_empty, check_map_not_empty, check_member_in_set,
    check_member_not_in_set, check_predicate_false, check_predicate_true, check_string_contains,
    check_valid_string_ascii, check_valid_string_ascii_optional,
};

fn bench_check_predicate_true() {
    black_box(check_predicate_true(true, "predicate must be true")).unwrap();
}

fn bench_check_predicate_false() {
    black_box(check_predicate_false(false, "predicate must be false")).unwrap();
}

fn bench_check_valid_string_ascii() {
    black_box(check_valid_string_ascii("Hello", "param")).unwrap();
}

fn bench_check_valid_string_ascii_optional() {
    black_box(check_valid_string_ascii_optional(Some("Hello"), "param")).unwrap();
}

fn bench_check_string_contains() {
    black_box(check_string_contains("Hello, world!", "world", "param")).unwrap();
}

fn bench_check_equal() {
    black_box(check_equal(&42, &42, "lhs", "rhs")).unwrap();
}

fn bench_check_in_range_inclusive_u8() {
    black_box(check_in_range_inclusive_u8(5, 0, 10, "param")).unwrap();
}

fn bench_check_in_range_inclusive_u64() {
    black_box(check_in_range_inclusive_u64(500, 0, 1000, "param")).unwrap();
}

fn bench_check_map_empty() {
    let empty_map: HashMap<u32, u32> = HashMap::new();
    black_box(check_map_empty(&empty_map, "param")).unwrap();
}

fn bench_check_map_not_empty() {
    let map: HashMap<u32, u32> = HashMap::from([(1, 42)]);
    black_box(check_map_not_empty(&map, "param")).unwrap();
}

fn bench_check_key_in_map() {
    let map: HashMap<u32, u32> = HashMap::from([(1, 42)]);
    black_box(check_key_in_map(&1, &map, "key", "map")).unwrap();
}

fn bench_check_key_not_in_map() {
    let map: HashMap<u32, u32> = HashMap::from([(1, 42)]);
    black_box(check_key_not_in_map(&2, &map, "key", "map")).unwrap();
}

fn bench_check_index_map_in() {
    let map: IndexMap<u32, u32> = IndexMap::from([(1, 42)]);
    black_box(check_key_in_map(&1, &map, "key", "map")).unwrap();
}

fn bench_check_index_map_not_in() {
    let map: IndexMap<u32, u32> = IndexMap::from([(1, 42)]);
    black_box(check_key_not_in_map(&2, &map, "key", "map")).unwrap();
}

fn bench_check_member_in_set() {
    let set: HashSet<u32> = HashSet::from([1, 42]);
    black_box(check_member_in_set(&1, &set, "key", "set")).unwrap();
}

fn bench_check_member_not_in_set() {
    let set: HashSet<u32> = HashSet::from([1, 42]);
    black_box(check_member_not_in_set(&100, &set, "key", "set")).unwrap();
}

main!(
    bench_check_predicate_true,
    bench_check_predicate_false,
    bench_check_valid_string_ascii,
    bench_check_valid_string_ascii_optional,
    bench_check_string_contains,
    bench_check_equal,
    bench_check_in_range_inclusive_u8,
    bench_check_in_range_inclusive_u64,
    bench_check_map_empty,
    bench_check_map_not_empty,
    bench_check_key_in_map,
    bench_check_key_not_in_map,
    bench_check_index_map_in,
    bench_check_index_map_not_in,
    bench_check_member_in_set,
    bench_check_member_not_in_set,
);
