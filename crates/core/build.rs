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

const NAUTILUS_VERSION: &str = "2.0.0rc10";

fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    #[cfg(feature = "extension-module")]
    check_python_package_version();

    println!("cargo:rustc-env=NAUTILUS_VERSION={NAUTILUS_VERSION}");
    println!("cargo:rustc-env=NAUTILUS_USER_AGENT=NautilusTrader/{NAUTILUS_VERSION}");
}

#[cfg(feature = "extension-module")]
fn check_python_package_version() {
    use std::path::Path;

    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    if !crate_dir.ends_with("crates/core") {
        return;
    }

    let workspace = crate_dir.join("../..");

    // Maturin moves the Python manifest to the root of the source distribution
    let manifest = if workspace.join("PKG-INFO").is_file() {
        workspace.join("pyproject.toml")
    } else {
        workspace.join("python/pyproject.toml")
    };

    if !manifest.is_file() {
        return;
    }

    println!("cargo:rerun-if-changed={}", manifest.display());
    let contents = std::fs::read_to_string(manifest).expect("read Python package manifest");
    let manifest = contents
        .parse::<toml::Table>()
        .expect("parse Python package manifest");
    let version = manifest["project"]["version"]
        .as_str()
        .expect("read Python package version");

    let matches_base_version = version
        .strip_prefix(NAUTILUS_VERSION)
        .is_some_and(|suffix| {
            matches!(
                suffix.as_bytes(),
                [] | [b'a'..=b'z' | b'+' | b'-', ..] | [b'.', b'a'..=b'z', ..]
            )
        });

    assert!(
        matches_base_version,
        "Python package version {version} does not match Rust version {NAUTILUS_VERSION}"
    );
}
