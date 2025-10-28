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

//! Mathematical functions and interpolation utilities.
//!
//! This module provides essential mathematical operations for quantitative trading,
//! including linear and quadratic interpolation functions commonly used in financial
//! data processing and analysis.

/// Macro for approximate floating-point equality comparison.
///
/// This macro compares two floating-point values with a specified epsilon tolerance,
/// providing a safe alternative to exact equality checks which can fail due to
/// floating-point precision issues.
///
/// # Usage
///
/// ```rust
/// use nautilus_core::approx_eq;
///
/// let a = 0.1 + 0.2;
/// let b = 0.3;
/// assert!(approx_eq!(f64, a, b, epsilon = 1e-10));
/// ```
#[macro_export]
macro_rules! approx_eq {
    ($type:ty, $left:expr, $right:expr, epsilon = $epsilon:expr) => {{
        let left_val: $type = $left;
        let right_val: $type = $right;
        (left_val - right_val).abs() < $epsilon
    }};
    ($type:ty, $left:expr, $right:expr, epsilon = $epsilon:expr, ulps = $ulps:expr) => {{
        let left_val: $type = $left;
        let right_val: $type = $right;
        // For compatibility, we use epsilon comparison and ignore ulps
        (left_val - right_val).abs() < $epsilon
    }};
}

/// Calculates the interpolation weight between `x1` and `x2` for a value `x`.
///
/// The returned weight `w` satisfies `y = (1 - w) * y1 + w * y2` when
/// interpolating ordinates that correspond to abscissas `x1` and `x2`.
///
/// # Panics
///
/// Panics if `x1` and `x2` are too close (within machine epsilon), which would
/// cause division by zero or numerical instability. Uses `f64::EPSILON` * 2.0 to
/// account for floating-point rounding in the difference computation.
#[inline]
#[must_use]
pub fn linear_weight(x1: f64, x2: f64, x: f64) -> f64 {
    const EPSILON: f64 = f64::EPSILON * 2.0; // ~4.44e-16
    let diff = (x2 - x1).abs();
    assert!(
        diff >= EPSILON,
        "`x1` ({x1}) and `x2` ({x2}) are too close for stable interpolation (diff: {diff}, min: {EPSILON})"
    );
    (x - x1) / (x2 - x1)
}

/// Performs linear interpolation using a weight factor.
///
/// Given ordinates `y1` and `y2` and a weight `x1_diff`, computes the
/// interpolated value using the formula: `y1 + x1_diff * (y2 - y1)`.
#[inline]
#[must_use]
pub fn linear_weighting(y1: f64, y2: f64, x1_diff: f64) -> f64 {
    x1_diff.mul_add(y2 - y1, y1)
}

/// Finds the position for interpolation in a sorted array.
///
/// Returns the index of the largest element in `xs` that is less than `x`,
/// clamped to the valid range `[0, xs.len() - 1]`.
///
/// # Edge Cases
///
/// - For empty arrays, returns 0
/// - For single-element arrays, always returns index 0, regardless of whether `x > xs[0]`
/// - For values below the minimum, returns 0
/// - For values at or above the maximum, returns `xs.len() - 1`
///
/// # Examples
///
/// ```
/// use nautilus_core::math::pos_search;
///
/// // Normal case: find position between elements
/// assert_eq!(pos_search(2.5, &[1.0, 2.0, 3.0, 4.0]), 1); // Between indices 1 and 2
///
/// // Single element: always returns 0
/// assert_eq!(pos_search(5.0, &[10.0]), 0); // Even though x < xs[0]
/// assert_eq!(pos_search(15.0, &[10.0]), 0); // Even though x > xs[0]
/// ```
#[inline]
#[must_use]
pub fn pos_search(x: f64, xs: &[f64]) -> usize {
    if xs.is_empty() {
        return 0;
    }

    let n_elem = xs.len();
    let pos = xs.partition_point(|&val| val < x);
    std::cmp::min(std::cmp::max(pos.saturating_sub(1), 0), n_elem - 1)
}

/// Evaluates the quadratic Lagrange polynomial defined by three points.
///
/// Given points `(x0, y0)`, `(x1, y1)`, `(x2, y2)` this returns *P(x)* where
/// *P* is the unique polynomial of degree ≤ 2 passing through the three
/// points.
///
/// # Panics
///
/// Panics if any two abscissas are too close (within machine epsilon), which would
/// cause division by zero or numerical instability in the interpolation.
#[inline]
#[must_use]
pub fn quad_polynomial(x: f64, x0: f64, x1: f64, x2: f64, y0: f64, y1: f64, y2: f64) -> f64 {
    const EPSILON: f64 = f64::EPSILON * 2.0; // ~4.44e-16

    // Protect against coincident x values that would lead to division by zero
    let diff_01 = (x0 - x1).abs();
    let diff_02 = (x0 - x2).abs();
    let diff_12 = (x1 - x2).abs();

    assert!(
        diff_01 >= EPSILON && diff_02 >= EPSILON && diff_12 >= EPSILON,
        "Abscissas are too close for stable interpolation: x0={x0}, x1={x1}, x2={x2} (diffs: {diff_01:.2e}, {diff_02:.2e}, {diff_12:.2e}, min: {EPSILON})"
    );

    y0 * (x - x1) * (x - x2) / ((x0 - x1) * (x0 - x2))
        + y1 * (x - x0) * (x - x2) / ((x1 - x0) * (x1 - x2))
        + y2 * (x - x0) * (x - x1) / ((x2 - x0) * (x2 - x1))
}

/// Performs quadratic interpolation for the point `x` given vectors of abscissas `xs` and ordinates `ys`.
///
/// # Panics
///
/// Panics if `xs.len() < 3` or `xs.len() != ys.len()`.
#[must_use]
pub fn quadratic_interpolation(x: f64, xs: &[f64], ys: &[f64]) -> f64 {
    let n_elem = xs.len();
    let epsilon = 1e-8;

    assert!(
        n_elem >= 3,
        "Need at least 3 points for quadratic interpolation"
    );
    assert_eq!(xs.len(), ys.len(), "xs and ys must have the same length");

    if x <= xs[0] {
        return ys[0];
    }

    if x >= xs[n_elem - 1] {
        return ys[n_elem - 1];
    }

    let pos = pos_search(x, xs);

    if (xs[pos] - x).abs() < epsilon {
        return ys[pos];
    }

    if pos == 0 {
        return quad_polynomial(x, xs[0], xs[1], xs[2], ys[0], ys[1], ys[2]);
    }

    if pos == n_elem - 2 {
        return quad_polynomial(
            x,
            xs[n_elem - 3],
            xs[n_elem - 2],
            xs[n_elem - 1],
            ys[n_elem - 3],
            ys[n_elem - 2],
            ys[n_elem - 1],
        );
    }

    let w = linear_weight(xs[pos], xs[pos + 1], x);

    linear_weighting(
        quad_polynomial(
            x,
            xs[pos - 1],
            xs[pos],
            xs[pos + 1],
            ys[pos - 1],
            ys[pos],
            ys[pos + 1],
        ),
        quad_polynomial(
            x,
            xs[pos],
            xs[pos + 1],
            xs[pos + 2],
            ys[pos],
            ys[pos + 1],
            ys[pos + 2],
        ),
        w,
    )
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use rstest::*;

    use super::*;

    #[rstest]
    #[case(0.0, 10.0, 5.0, 0.5)]
    #[case(1.0, 3.0, 2.0, 0.5)]
    #[case(0.0, 1.0, 0.25, 0.25)]
    #[case(0.0, 1.0, 0.75, 0.75)]
    fn test_linear_weight_valid_cases(
        #[case] x1: f64,
        #[case] x2: f64,
        #[case] x: f64,
        #[case] expected: f64,
    ) {
        let result = linear_weight(x1, x2, x);
        assert!(
            approx_eq!(f64, result, expected, epsilon = 1e-10),
            "Expected {expected}, was {result}"
        );
    }

    #[rstest]
    #[should_panic(expected = "too close for stable interpolation")]
    fn test_linear_weight_zero_divisor() {
        let _ = linear_weight(1.0, 1.0, 0.5);
    }

    #[rstest]
    #[should_panic(expected = "too close for stable interpolation")]
    fn test_linear_weight_near_equal_values() {
        // Values within machine epsilon should be rejected
        let _ = linear_weight(1.0, 1.0 + f64::EPSILON, 0.5);
    }

    #[rstest]
    fn test_linear_weight_with_small_differences() {
        // High-resolution data (e.g., nanosecond timestamps as seconds) should work
        let result = linear_weight(0.0, 1e-12, 5e-13);
        assert!(result.is_finite());
        assert!((result - 0.5).abs() < 1e-10); // Should be approximately 0.5
    }

    #[rstest]
    fn test_linear_weight_just_above_epsilon() {
        // Values differing by more than machine epsilon should work
        let result = linear_weight(1.0, 1.0 + 1e-9, 1.0 + 5e-10);
        // Should not panic and return a reasonable value
        assert!(result.is_finite());
    }

    #[rstest]
    #[case(1.0, 3.0, 0.5, 2.0)]
    #[case(10.0, 20.0, 0.25, 12.5)]
    #[case(0.0, 10.0, 0.0, 0.0)]
    #[case(0.0, 10.0, 1.0, 10.0)]
    fn test_linear_weighting(
        #[case] y1: f64,
        #[case] y2: f64,
        #[case] weight: f64,
        #[case] expected: f64,
    ) {
        let result = linear_weighting(y1, y2, weight);
        assert!(
            approx_eq!(f64, result, expected, epsilon = 1e-10),
            "Expected {expected}, was {result}"
        );
    }

    #[rstest]
    #[case(5.0, &[1.0, 2.0, 3.0, 4.0, 6.0, 7.0], 3)]
    #[case(1.5, &[1.0, 2.0, 3.0, 4.0], 0)]
    #[case(0.5, &[1.0, 2.0, 3.0, 4.0], 0)]
    #[case(4.5, &[1.0, 2.0, 3.0, 4.0], 3)]
    #[case(2.0, &[1.0, 2.0, 3.0, 4.0], 0)]
    fn test_pos_search(#[case] x: f64, #[case] xs: &[f64], #[case] expected: usize) {
        let result = pos_search(x, xs);
        assert_eq!(result, expected);
    }

    #[rstest]
    fn test_pos_search_edge_cases() {
        // Single element array
        let result = pos_search(5.0, &[10.0]);
        assert_eq!(result, 0);

        // Value at exact boundary
        let result = pos_search(3.0, &[1.0, 2.0, 3.0, 4.0]);
        assert_eq!(result, 1); // Index of largest element < 3.0 is index 1 (value 2.0)

        // Two element array
        let result = pos_search(1.5, &[1.0, 2.0]);
        assert_eq!(result, 0);
    }

    #[rstest]
    fn test_pos_search_empty_slice() {
        let empty: &[f64] = &[];
        assert_eq!(pos_search(42.0, empty), 0);
    }

    #[rstest]
    fn test_quad_polynomial_linear_case() {
        // Test with three collinear points - should behave like linear interpolation
        let result = quad_polynomial(1.5, 1.0, 2.0, 3.0, 1.0, 2.0, 3.0);
        assert!(approx_eq!(f64, result, 1.5, epsilon = 1e-10));
    }

    #[rstest]
    fn test_quad_polynomial_parabola() {
        // Test with a simple parabola y = x^2
        // Points: (0,0), (1,1), (2,4)
        let result = quad_polynomial(1.5, 0.0, 1.0, 2.0, 0.0, 1.0, 4.0);
        let expected = 1.5 * 1.5; // Should be 2.25
        assert!(approx_eq!(f64, result, expected, epsilon = 1e-10));
    }

    #[rstest]
    #[should_panic(expected = "too close for stable interpolation")]
    fn test_quad_polynomial_duplicate_x() {
        let _ = quad_polynomial(0.5, 1.0, 1.0, 2.0, 0.0, 1.0, 4.0);
    }

    #[rstest]
    #[should_panic(expected = "too close for stable interpolation")]
    fn test_quad_polynomial_near_equal_x_values() {
        // x0 and x1 differ by only machine epsilon
        let _ = quad_polynomial(0.5, 1.0, 1.0 + f64::EPSILON, 2.0, 0.0, 1.0, 4.0);
    }

    #[rstest]
    fn test_quad_polynomial_with_small_differences() {
        // High-resolution data should work (e.g., 1e-12 spacing)
        let result = quad_polynomial(5e-13, 0.0, 1e-12, 2e-12, 0.0, 1.0, 4.0);
        assert!(result.is_finite());
    }

    #[rstest]
    fn test_quad_polynomial_just_above_epsilon() {
        // Values differing by more than machine epsilon should work
        let result = quad_polynomial(0.5, 0.0, 1.0 + 1e-9, 2.0, 0.0, 1.0, 4.0);
        // Should not panic and return a reasonable value
        assert!(result.is_finite());
    }

    #[rstest]
    fn test_quadratic_interpolation_boundary_conditions() {
        let xs = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        let ys = vec![1.0, 4.0, 9.0, 16.0, 25.0]; // y = x^2

        // Test below minimum
        let result = quadratic_interpolation(0.5, &xs, &ys);
        assert_eq!(result, ys[0]);

        // Test above maximum
        let result = quadratic_interpolation(6.0, &xs, &ys);
        assert_eq!(result, ys[4]);
    }

    #[rstest]
    fn test_quadratic_interpolation_exact_points() {
        let xs = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        let ys = vec![1.0, 4.0, 9.0, 16.0, 25.0];

        // Test exact points
        for (i, &x) in xs.iter().enumerate() {
            let result = quadratic_interpolation(x, &xs, &ys);
            assert!(approx_eq!(f64, result, ys[i], epsilon = 1e-6));
        }
    }

    #[rstest]
    fn test_quadratic_interpolation_intermediate_values() {
        let xs = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        let ys = vec![1.0, 4.0, 9.0, 16.0, 25.0]; // y = x^2

        // Test interpolation between points
        let result = quadratic_interpolation(2.5, &xs, &ys);
        let expected = 2.5 * 2.5; // Should be close to 6.25
        assert!((result - expected).abs() < 0.1); // Allow some interpolation error
    }

    #[rstest]
    #[should_panic(expected = "Need at least 3 points")]
    fn test_quadratic_interpolation_insufficient_points() {
        let xs = vec![1.0, 2.0];
        let ys = vec![1.0, 4.0];
        let _ = quadratic_interpolation(1.5, &xs, &ys);
    }

    #[rstest]
    #[should_panic(expected = "xs and ys must have the same length")]
    fn test_quadratic_interpolation_mismatched_lengths() {
        let xs = vec![1.0, 2.0, 3.0];
        let ys = vec![1.0, 4.0];
        let _ = quadratic_interpolation(1.5, &xs, &ys);
    }
}
