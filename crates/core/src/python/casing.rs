//! String-case conversion helpers (`CamelCase` ⇄ `snake_case`).

use heck::ToSnakeCase;
use pyo3::prelude::*;
use pyo3_stub_gen::derive::gen_stub_pyfunction;

/// Convert the given string from any common case (PascalCase, camelCase, kebab-case, etc.)
/// to *lower* `snake_case`.
///
/// This function uses the `heck` Rust crate under the hood.
///
/// Parameters
/// ----------
/// input : str
///     The input string to convert.
///
/// Returns
/// -------
/// str
#[must_use]
#[gen_stub_pyfunction(module = "nautilus_trader.core")]
#[pyfunction(name = "convert_to_snake_case")]
pub fn py_convert_to_snake_case(input: &str) -> String {
    input.to_snake_case()
}
