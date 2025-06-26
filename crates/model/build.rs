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

//! Build script for the `nautilus-model` crate.
//!
//! In addition to the common tasks performed by the other build scripts (header generation,
//! rerun-tracking, docs.rs early-exit) this script also toggles *high-precision* mode for the
//! generated bindings based on either:
//!
//! 1. The `HIGH_PRECISION` environment variable, **or**
//! 2. The compile-time `high-precision` cargo feature.
//!
//! When enabled the flag is forwarded to the Cython bindings via a `DEF HIGH_PRECISION` macro so
//! that the Python layer compiles in a compatible configuration.

#[cfg(feature = "ffi")]
use std::env;

#[allow(clippy::expect_used)]
#[allow(unused_assignments)]
#[allow(unused_mut)]
fn main() {
    // Skip file generation if we're in the docs.rs environment
    if std::env::var("DOCS_RS").is_ok() {
        println!("cargo:warning=Running in docs.rs environment, skipping file generation");
        return;
    }

    // Ensure the build script runs on changes
    println!("cargo:rerun-if-env-changed=HIGH_PRECISION");
    println!("cargo:rerun-if-env-changed=CARGO_FEATURE_HIGH_PRECISION");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=cbindgen.toml");
    println!("cargo:rerun-if-changed=cbindgen_cython.toml");
    println!("cargo:rerun-if-changed=../Cargo.toml");

    #[cfg(feature = "ffi")]
    if env::var("CARGO_FEATURE_FFI").is_ok() {
        extern crate cbindgen;
        use std::{
            fs::File,
            io::{Read, Write},
            path::PathBuf,
        };

        let crate_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));

        // Generate C headers
        let mut config_c = cbindgen::Config::from_file("cbindgen.toml")
            .expect("unable to find cbindgen.toml configuration file");

        // Check HIGH_PRECISION environment variable for C header too
        let high_precision_c = env::var("HIGH_PRECISION")
            .map(|v| v.to_lowercase() == "true" || v == "1")
            .unwrap_or_else(|_| {
                #[cfg(feature = "high-precision")]
                {
                    true
                }
                #[cfg(not(feature = "high-precision"))]
                {
                    false
                }
            });

        if high_precision_c && let Some(mut includes) = config_c.after_includes {
            includes.insert_str(0, "\n#define HIGH_PRECISION\n");
            config_c.after_includes = Some(includes);
        }

        let c_header_path = crate_dir.join("../../nautilus_trader/core/includes/model.h");
        cbindgen::generate_with_config(&crate_dir, config_c)
            .expect("unable to generate bindings")
            .write_to_file(&c_header_path);

        // Generate Cython definitions
        let mut config_cython = cbindgen::Config::from_file("cbindgen_cython.toml")
            .expect("unable to find cbindgen_cython.toml configuration file");

        // Check HIGH_PRECISION environment variable first, then fall back to feature flag
        let high_precision = env::var("HIGH_PRECISION")
            .map(|v| v.to_lowercase() == "true" || v == "1")
            .unwrap_or_else(|_| {
                #[cfg(feature = "high-precision")]
                {
                    true
                }
                #[cfg(not(feature = "high-precision"))]
                {
                    false
                }
            });

        let flag = if high_precision {
            Some("\nDEF HIGH_PRECISION = True  # or False".to_string())
        } else {
            Some("\nDEF HIGH_PRECISION = False  # or True".to_string())
        };

        // Activate Cython high-precision flag based on feature flags passed to Rust build
        config_cython.after_includes = flag;

        let cython_path = crate_dir.join("../../nautilus_trader/core/rust/model.pxd");
        cbindgen::generate_with_config(&crate_dir, config_cython)
            .expect("unable to generate bindings")
            .write_to_file(cython_path.clone());

        // Open and read the file entirely
        let mut src = File::open(cython_path.clone()).expect("`File::open` failed");
        let mut data = String::new();
        src.read_to_string(&mut data)
            .expect("invalid UTF-8 in stream");

        // Run the replace operation in memory
        let mut data = data.replace("cdef enum", "cpdef enum");

        // Always add 128-bit typedefs for compatibility (they map to 64-bit on MSVC)
        {
            let lines: Vec<&str> = data.lines().collect();

            let mut output = String::new();
            let mut found_extern = false;

            for line in lines {
                output.push_str(line);
                output.push('\n');

                if !found_extern && line.trim().starts_with("cdef extern from") {
                    output.push_str("    ctypedef unsigned long long uint128_t\n");
                    output.push_str("    ctypedef long long int128_t\n");
                    found_extern = true;
                }
            }

            data = output;
        }

        // Recreate the file and dump the processed contents to it
        let mut dst = File::create(cython_path).expect("`File::create` failed");
        dst.write_all(data.as_bytes())
            .expect("I/O error on `dist.write`");
    }
}
