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

#[cfg(feature = "ffi")]
use std::env;

#[allow(clippy::expect_used)]
#[allow(unused_assignments)]
#[allow(unused_mut)]
fn main() {
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

        #[cfg(feature = "high-precision")]
        {
            if let Some(mut includes) = config_c.after_includes {
                includes.insert_str(0, "\n#define HIGH_PRECISION\n");
                config_c.after_includes = Some(includes);
            }
        }

        let c_header_path = crate_dir.join("../../nautilus_trader/core/includes/model.h");
        cbindgen::generate_with_config(&crate_dir, config_c)
            .expect("unable to generate bindings")
            .write_to_file(&c_header_path);

        // Generate Cython definitions
        let mut config_cython = cbindgen::Config::from_file("cbindgen_cython.toml")
            .expect("unable to find cbindgen_cython.toml configuration file");

        #[cfg(feature = "high-precision")]
        let flag = Some("\nDEF HIGH_PRECISION = True  # or False".to_string());
        #[cfg(not(feature = "high-precision"))]
        let flag = Some("\nDEF HIGH_PRECISION = False  # or True".to_string());

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

        #[cfg(feature = "high-precision")]
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
