//! Common test related helper functions.

/// Checks if two floating-point numbers are approximately equal within the
/// margin of floating-point precision.
///
/// - `a`: The first floating-point number.
/// - `b`: The second floating-point number.
///
/// # Returns
///
/// Returns `true` if the absolute difference between `a` and `b` is less than
/// `f64::EPSILON`, indicating that they are approximately equal.
///
/// # Example
///
/// ```
/// use nautilus_indicators::testing::approx_equal;
///
/// let a = 0.1 + 0.2;
/// let b = 0.3;
/// assert!(approx_equal(a, b));
/// ```
#[must_use]
pub fn approx_equal(a: f64, b: f64) -> bool {
    (a - b).abs() < f64::EPSILON
}
