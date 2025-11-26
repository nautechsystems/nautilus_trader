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

//! iai benchmark template.
//!
//! Copy this file into `crates/<my_crate>/benches/` and adjust names and
//! imports.

use std::hint::black_box;

// -----------------------------------------------------------------------------
// Replace `fast_add` with the real function you want to measure.
// -----------------------------------------------------------------------------

fn fast_add() -> i32 {
    let a = black_box(1);
    let b = black_box(2);
    a + b
}

iai::main!(fast_add);
