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

//! Common test related helper functions.

/// Checks if two floating-point numbers are approximately equal within the
/// margin of floating-point precision.
///
/// * `a` - The first floating-point number.
/// * `b` - The second floating-point number.
///
/// # Returns
///
/// Returns `true` if the absolute difference between `a` and `b` is less than
/// `f64::EPSILON`, indicating that they are approximately equal.
///
/// # Example
///
/// ```
/// let a = 0.1 + 0.2;
/// let b = 0.3;
/// assert!(approx_equal(a, b));
/// ```
#[must_use]
pub fn approx_equal(a: f64, b: f64) -> bool {
    (a - b).abs() < f64::EPSILON
}
