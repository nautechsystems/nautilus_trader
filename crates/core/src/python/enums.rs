//! Macro-generated enum utilities for PyO3.

use ::strum::{IntoEnumIterator, ParseError};
use pyo3::PyResult;

use super::to_pyvalue_err;

/// Converts a raw string to the enum `E`, returning a nicely‑formatted
/// `PyValueError` if the string does not match any variant.
///
/// The helper is aimed at Python‑exposed functions that still accept plain
/// `&str` parameters internally: call `parse_enum` instead of writing repetitive
/// `str::parse()` + error‑formatting logic yourself.
///
/// # Errors
///
/// Returns an error if `input` does not match any known variant of `E`.
pub fn parse_enum<E>(input: &str, param: &str) -> PyResult<E>
where
    E: std::str::FromStr<Err = ParseError> + IntoEnumIterator + ToString,
{
    input.parse::<E>().map_err(|_| {
        let allowed = E::iter()
            .map(|v| v.to_string())
            .collect::<Vec<_>>()
            .join(", ");
        to_pyvalue_err(format!(
            "unknown {param} `{input}`; valid values: {allowed}"
        ))
    })
}
