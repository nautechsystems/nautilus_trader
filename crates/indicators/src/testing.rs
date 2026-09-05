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

//! Floating-point assertions for indicator tests.

/// Relative tolerance used by [`approx_equal`].
///
/// Indicator values span many orders of magnitude, so the admissible error
/// scales with them.
pub const DEFAULT_RELATIVE_TOLERANCE: f64 = 1e-9;

/// Absolute tolerance used by [`approx_equal`].
///
/// Applies near zero, where a relative bound collapses to nothing.
pub const DEFAULT_ABSOLUTE_TOLERANCE: f64 = 1e-15;

/// Checks whether two floating-point numbers agree to the default tolerances.
///
/// Use this for calculated results. Invariants such as reset values, bounds or
/// period-one passthrough are exact and should be asserted with `==`.
///
/// # Example
///
/// ```
/// use nautilus_indicators::testing::approx_equal;
///
/// // At 1e6 the relative bound is 1e-3, which covers a gap of 1e-4.
/// assert!(approx_equal(1e6 + 1e-4, 1e6));
/// assert!(!approx_equal(1.0, 1.000_1));
/// ```
#[must_use]
pub fn approx_equal(a: f64, b: f64) -> bool {
    approx_equal_with(a, b, DEFAULT_RELATIVE_TOLERANCE, DEFAULT_ABSOLUTE_TOLERANCE)
}

/// Checks whether two floating-point numbers agree to the given tolerances.
///
/// Equal to within `absolute`, or within `relative` scaled by the larger
/// magnitude. Infinities of the same sign compare equal; any NaN does not.
///
/// # Example
///
/// ```
/// use nautilus_indicators::testing::approx_equal_with;
///
/// assert!(approx_equal_with(100.0, 100.000_001, 1e-7, 0.0));
/// assert!(!approx_equal_with(100.0, 100.000_1, 1e-7, 0.0));
/// ```
#[must_use]
pub fn approx_equal_with(a: f64, b: f64, relative: f64, absolute: f64) -> bool {
    if a.is_nan() || b.is_nan() {
        return false;
    }

    if a.is_infinite() || b.is_infinite() {
        // Scaling a tolerance by an infinite magnitude swallows any gap.
        return a.is_infinite() && b.is_infinite() && a.is_sign_positive() == b.is_sign_positive();
    }
    let difference = (a - b).abs();
    difference <= absolute || difference <= relative * a.abs().max(b.abs())
}

/// Asserts that two floating-point numbers agree to the default tolerances.
///
/// Panics with both values and the absolute and relative errors.
///
/// # Panics
///
/// If the values differ by more than the default tolerances.
#[track_caller]
pub fn assert_approx_equal(actual: f64, expected: f64) {
    assert!(
        approx_equal(actual, expected),
        "approx_equal failed\n  actual:   {actual:?}\n  expected: {expected:?}\n  \
         absolute error: {:e}\n  relative error: {:e}",
        (actual - expected).abs(),
        (actual - expected).abs() / actual.abs().max(expected.abs()),
    );
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::{approx_equal, approx_equal_with};

    #[rstest]
    fn approx_equal_scales_with_magnitude() {
        // At 1e6 the relative bound is 1e-3, and f64 spacing there is ~1.2e-10.
        assert!(approx_equal(1e6 + 1e-4, 1e6));
        assert!(approx_equal(1e-4 + 1e-14, 1e-4));
    }

    #[rstest]
    fn approx_equal_still_separates_real_differences() {
        assert!(!approx_equal(1.0, 1.000_1));
        assert!(!approx_equal(1e6, 1.001e6));
    }

    #[rstest]
    fn approx_equal_handles_zero_and_non_finite() {
        assert!(approx_equal(0.0, -0.0));
        assert!(approx_equal(f64::INFINITY, f64::INFINITY));
        assert!(!approx_equal(f64::INFINITY, f64::NEG_INFINITY));
        assert!(!approx_equal(f64::NAN, f64::NAN));
    }

    #[rstest]
    fn approx_equal_with_honours_explicit_tolerances() {
        assert!(approx_equal_with(100.0, 100.000_001, 1e-7, 0.0));
        assert!(!approx_equal_with(100.0, 100.000_1, 1e-7, 0.0));
        assert!(approx_equal_with(0.0, 1e-13, 0.0, 1e-12));
    }
}
