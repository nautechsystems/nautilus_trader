//! Test utilities for the Deribit adapter.

use std::{fs, path::PathBuf};

#[cfg(test)]
#[must_use]
/// Loads a JSON fixture from the adapter test data directory.
///
/// # Panics
///
/// Panics if the test file cannot be read (e.g., file not found or permission denied).
pub fn load_test_json(file_name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("test_data")
        .join(file_name);

    fs::read_to_string(path).expect("Failed to read test JSON file")
}
