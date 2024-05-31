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

//! Functions to evaluation total equality of two instances.

use pretty_assertions::assert_eq;
use serde::Serialize;

/// Return whether `a` and `b` are entirely equal (all fields).
///
/// Serializes the input values to JSON and compares the resulting strings.
pub fn entirely_equal<T: Serialize>(a: T, b: T) {
    let a_serialized = serde_json::to_string(&a).unwrap();
    let b_serialized = serde_json::to_string(&b).unwrap();

    assert_eq!(a_serialized, b_serialized);
}
