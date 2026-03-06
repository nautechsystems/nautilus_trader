//! Test helpers for loading BitMEX adapter fixtures.

/// Load a test JSON file from the `test_data` directory.
///
/// # Panics
///
/// Panics if the test file cannot be read (should only happen if test data is missing).
#[cfg(test)]
#[must_use]
pub fn load_test_json(file_name: &str) -> String {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("test_data")
        .join(file_name);

    std::fs::read_to_string(path).expect("Failed to read test JSON file")
}
