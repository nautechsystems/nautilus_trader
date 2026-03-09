#![allow(clippy::doc_markdown, reason = "Python docstrings")]

use pyo3::prelude::*;
use pyo3_stub_gen::derive::gen_stub_pyfunction;

/// Masks an API key by showing only the first and last 4 characters.
///
/// For keys 8 characters or shorter, returns asterisks only.
///
/// Parameters
/// ----------
/// api_key : str
///     The API key to mask.
///
/// Returns
/// -------
/// str
///
/// Examples
/// --------
/// >>> mask_api_key("abcdefghijklmnop")
/// 'abcd...mnop'
/// >>> mask_api_key("short")
/// '*****'
///
#[gen_stub_pyfunction(module = "nautilus_trader.core")]
#[pyfunction(name = "mask_api_key")]
#[allow(
    clippy::needless_pass_by_value,
    reason = "Python FFI requires owned types"
)]
pub fn py_mask_api_key(api_key: String) -> String {
    crate::string::mask_api_key(&api_key)
}
