//! Shared helpers for adapter unit tests.

use std::{fs, path::PathBuf};

#[cfg(test)]
#[must_use]
/// Loads the named JSON fixture from the Bybit `test_data` directory.
///
/// # Panics
/// Panics if the fixture file cannot be read.
pub fn load_test_json(file_name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("test_data")
        .join(file_name);
    fs::read_to_string(path).expect("failed to load Bybit test fixture")
}
