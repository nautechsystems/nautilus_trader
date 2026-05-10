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

//! Utility functions for resolving project and workspace directory paths.

use std::path::PathBuf;

/// Returns the workspace root directory path.
///
/// This is the directory containing the top-level `Cargo.toml` with the
/// `[workspace]` section, typically where `pyproject.toml` and `docs/` are located.
///
/// # Panics
///
/// Panics if the `CARGO_MANIFEST_DIR` environment variable is not set or
/// the parent directories cannot be determined.
#[must_use]
pub fn get_workspace_root_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent() // crates/core -> crates/
        .and_then(|p| p.parent()) // crates/ -> nautilus_trader/
        .expect("Failed to get workspace root")
        .to_path_buf()
}

/// Returns the project root directory path.
///
/// For this monorepo, the project root is the same as the workspace root.
///
/// # Panics
///
/// Panics if the workspace root path cannot be determined.
#[must_use]
pub fn get_project_root_path() -> PathBuf {
    get_workspace_root_path()
}

/// Returns the tests root directory path.
#[must_use]
pub fn get_tests_root_path() -> PathBuf {
    get_project_root_path().join("tests")
}

/// Returns the test data directory path.
#[must_use]
pub fn get_test_data_path() -> PathBuf {
    if let Ok(test_data_root_path) = std::env::var("TEST_DATA_ROOT_PATH") {
        get_project_root_path()
            .join(test_data_root_path)
            .join("test_data")
    } else {
        get_project_root_path().join("tests").join("test_data")
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_workspace_root_contains_pyproject() {
        let root = get_workspace_root_path();
        assert!(
            root.join("pyproject.toml").exists(),
            "Workspace root should contain pyproject.toml, was: {root:?}"
        );
    }

    #[rstest]
    fn test_workspace_root_contains_crates_dir() {
        let root = get_workspace_root_path();
        assert!(
            root.join("crates").is_dir(),
            "Workspace root should contain crates/ directory, was: {root:?}"
        );
    }

    #[rstest]
    fn test_project_root_equals_workspace_root() {
        assert_eq!(get_project_root_path(), get_workspace_root_path());
    }

    #[rstest]
    fn test_tests_root_path() {
        let tests_root = get_tests_root_path();
        assert!(
            tests_root.ends_with("tests"),
            "Tests root should end with 'tests', was: {tests_root:?}"
        );
    }
}
