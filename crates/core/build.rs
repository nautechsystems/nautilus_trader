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

//! Build script for the `nautilus-core` crate.
//!
//! This script propagates version information through the `NAUTILUS_VERSION` and
//! `NAUTILUS_USER_AGENT` compile-time environment variables.

use std::path::Path;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=Cargo.toml");

    // The Python manifest is unavailable when this crate builds from a published package
    let nautilus_version = "2.0.0";

    if let Some(pyproject_version) = try_read_pyproject_version() {
        assert!(
            pyproject_version.starts_with(nautilus_version),
            "Version mismatch: pyproject.toml={pyproject_version}, hardcoded={nautilus_version}",
        );
    }

    // Set compile-time environment variables
    println!("cargo:rustc-env=NAUTILUS_VERSION={nautilus_version}");
    println!("cargo:rustc-env=NAUTILUS_USER_AGENT=NautilusTrader/{nautilus_version}");
}

fn try_read_pyproject_version() -> Option<String> {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let path = Path::new("../../python/pyproject.toml");

    // In a vendor directory, the relative path can point into the downstream project
    if !crate_dir.ends_with("crates/core") || !path.is_file() {
        return None;
    }

    println!("cargo:rerun-if-changed={}", path.display());

    let contents = std::fs::read_to_string(path).ok()?;
    let value = toml::from_str::<toml::Value>(&contents).ok()?;
    let version = value.get("project")?.get("version")?.as_str()?;
    Some(version.to_owned())
}
