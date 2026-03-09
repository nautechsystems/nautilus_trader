use nautilus_core::python::to_pyvalue_err;
use pyo3::prelude::*;

use crate::signing::{ed25519_signature, hmac_signature, rsa_signature};

/// HMAC-SHA256 signature of `data` using the provided `secret`.
///
/// # Errors
///
/// Returns an error if signature generation fails due to key or cryptographic errors.
#[pyfunction(name = "hmac_signature")]
pub fn py_hmac_signature(secret: &str, data: &str) -> PyResult<String> {
    hmac_signature(secret, data).map_err(to_pyvalue_err)
}

/// RSA PKCS#1 SHA-256 signature of `data` using the provided private key in PEM format.
///
/// # Errors
///
/// Returns an error if signature generation fails, e.g., due to empty data or invalid key PEM.
#[pyfunction(name = "rsa_signature")]
pub fn py_rsa_signature(private_key_pem: &str, data: &str) -> PyResult<String> {
    rsa_signature(private_key_pem, data).map_err(to_pyvalue_err)
}

/// Ed25519 signature of `data` using the provided private key seed.
///
/// # Errors
///
/// Returns an error if the private key seed is invalid or signature creation fails.
#[pyfunction(name = "ed25519_signature")]
pub fn py_ed25519_signature(private_key: &[u8], data: &str) -> PyResult<String> {
    ed25519_signature(private_key, data).map_err(to_pyvalue_err)
}
