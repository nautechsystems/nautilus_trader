// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

#![allow(clippy::needless_return)]

//! Build script for the `nautilus-core` crate.
//!
//! This script is executed by Cargo during compilation and is responsible for the ancillary
//! tasks that the core library requires in order to compile correctly across the various
//! combinations of features and target environments supported by NautilusTrader.
//!
//! Specifically it performs the following duties:
//!
//! 1. Propagates version information from the top-level `pyproject.toml` (when available) so it
//!    can be embedded in the compiled binary via the `NAUTILUS_VERSION` and
//!    `NAUTILUS_USER_AGENT` environment variables.
//! 2. Generates C and Cython headers when the `ffi` feature flag is enabled.  The bindings are
//!    produced with [`cbindgen`](https://github.com/mozilla/cbindgen) and written into the
//!    Python package tree so that users building Python wheels do not need to have a Rust
//!    toolchain installed.
//! 3. Emits the appropriate `cargo:rerun-if-*` directives so that Cargo reruns this build script
//!    whenever any of the relevant environment variables or configuration files change.
//!
//! The script exits early when it detects the `DOCS_RS` environment variable, as header
//! generation is unnecessary (and sometimes not permitted) in the docs.rs build sandbox.

use std::{env, path::PathBuf};

#[allow(clippy::expect_used)]
fn main() {
    // Ensure the build script runs on changes
    println!("cargo:rerun-if-env-changed=HIGH_PRECISION");
    println!("cargo:rerun-if-env-changed=CARGO_FEATURE_HIGH_PRECISION");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=cbindgen.toml");
    println!("cargo:rerun-if-changed=cbindgen_cython.toml");
    println!("cargo:rerun-if-changed=Cargo.toml");
    println!("cargo:rerun-if-changed=../Cargo.toml");

    let nautilus_version = "1.221.0"; // Hardcode to avoid including pyproject.toml in package

    // Verify the hardcoded version matches the version from the top-level pyproject.toml
    if let Some(pyproject_version) = try_read_pyproject_version() {
        assert!(
            pyproject_version.starts_with(nautilus_version),
            "Version mismatch: pyproject.toml={pyproject_version}, hardcoded={nautilus_version}",
        );
    }

    // Set compile-time environment variables
    println!("cargo:rustc-env=NAUTILUS_VERSION={nautilus_version}");
    println!("cargo:rustc-env=NAUTILUS_USER_AGENT=NautilusTrader/{nautilus_version}");

    // Skip file generation if we're in the docs.rs environment
    if std::env::var("DOCS_RS").is_ok() {
        println!("cargo:warning=Running in docs.rs environment, skipping file generation");
        return;
    }

    #[cfg(feature = "ffi")]
    if env::var("CARGO_FEATURE_FFI").is_ok() {
        use std::{
            fs::File,
            io::{Read, Write},
        };

        use cbindgen;

        let crate_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));

        // Generate C headers
        let config_c = cbindgen::Config::from_file("cbindgen.toml")
            .expect("unable to find cbindgen.toml configuration file");

        let c_header_path = crate_dir.join("../../nautilus_trader/core/includes/core.h");
        cbindgen::generate_with_config(&crate_dir, config_c)
            .expect("unable to generate bindings")
            .write_to_file(c_header_path);

        // Generate Cython definitions
        let config_cython = cbindgen::Config::from_file("cbindgen_cython.toml")
            .expect("unable to find cbindgen_cython.toml configuration file");

        let cython_path = crate_dir.join("../../nautilus_trader/core/rust/core.pxd");
        cbindgen::generate_with_config(&crate_dir, config_cython)
            .expect("unable to generate bindings")
            .write_to_file(cython_path.clone());

        // Open and read the file entirely
        let mut src = File::open(cython_path.clone()).expect("`File::open` failed");
        let mut data = String::new();
        src.read_to_string(&mut data)
            .expect("invalid UTF-8 in stream");

        // Run the replace operation in memory
        let new_data = data.replace("cdef enum", "cpdef enum");

        // Recreate the file and dump the processed contents to it
        let mut dst = File::create(cython_path).expect("`File::create` failed");
        dst.write_all(new_data.as_bytes())
            .expect("I/O error on `dist.write`");
    }
}

fn try_read_pyproject_version() -> Option<String> {
    let crate_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let path1 = crate_dir.join("pyproject.toml");
    let path2 = crate_dir
        .parent()
        .and_then(|p| p.parent())
        .map(|p| p.join("pyproject.toml"));

    let paths_to_check: Vec<PathBuf> = vec![path1].into_iter().chain(path2).collect();

    for path in paths_to_check {
        if path.exists()
            && let Ok(contents) = std::fs::read_to_string(&path)
            && let Ok(value) = toml::from_str::<toml::Value>(&contents)
            && let Some(version) = value
                .get("project")
                .and_then(|p| p.get("version"))
                .and_then(|v| v.as_str())
        {
            return Some(version.to_string());
        }
    }

    None
}
