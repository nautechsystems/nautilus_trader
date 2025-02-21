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

use nautilus_model::types::fixed::{f64_to_fixed_i64, f64_to_fixed_u64};
use rand::Rng;

#[allow(dead_code)]
fn random_values_u64(len: u64) -> Vec<u64> {
    let mut rng = rand::rng();
    let mut vec = Vec::new();
    for _ in 0..len {
        let value = f64_to_fixed_u64(rng.random_range(1.2..1.5), 5);
        vec.push(value);
    }
    assert_eq!(vec.len() as u64, len);
    vec
}

#[allow(dead_code)]
fn random_values_i64(len: u64) -> Vec<i64> {
    let mut rng = rand::rng();
    let mut vec = Vec::new();
    for _ in 0..len {
        let value = f64_to_fixed_i64(rng.random_range(1.2..1.5), 5);
        vec.push(value);
    }
    assert_eq!(vec.len() as u64, len);
    vec
}

#[allow(dead_code)]
fn random_values_u8(len: u64) -> Vec<u8> {
    let mut rng = rand::rng();
    let mut vec = Vec::new();
    for _ in 0..len {
        let value = rng.random_range(2..5);
        vec.push(value);
    }
    assert_eq!(vec.len() as u64, len);
    vec
}

#[allow(dead_code)]
fn date_range(len: u64) -> Vec<u64> {
    let mut vec = Vec::new();
    let mut start: u64 = 1_546_304_400_000_000_000;
    let end: u64 = 1_577_840_400_000_000_000;
    let step = (end - start) / len;
    for _ in 0..len {
        start += step;
        vec.push(start);
    }
    vec
}
