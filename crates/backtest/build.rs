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
fn main() {
    // Ensure the build script runs on changes
    println!("cargo:rerun-if-env-changed=HIGH_PRECISION");
    println!("cargo:rerun-if-env-changed=CARGO_FEATURE_HIGH_PRECISION");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=cbindgen.toml");
    println!("cargo:rerun-if-changed=cbindgen_cython.toml");
    println!("cargo:rerun-if-changed=Cargo.toml");
    println!("cargo:rerun-if-changed=../Cargo.toml");

    #[cfg(feature = "ffi")]
    if env::var("CARGO_FEATURE_FFI").is_ok() {
        extern crate cbindgen;
        use std::path::PathBuf;

        let crate_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));

        // Generate C headers
        let config_c = cbindgen::Config::from_file("cbindgen.toml")
            .expect("unable to find cbindgen.toml configuration file");

        let c_header_path = crate_dir.join("../../nautilus_trader/core/includes/backtest.h");
        cbindgen::generate_with_config(&crate_dir, config_c)
            .expect("unable to generate bindings")
            .write_to_file(c_header_path);

        // Generate Cython definitions
        let config_cython = cbindgen::Config::from_file("cbindgen_cython.toml")
            .expect("unable to find cbindgen_cython.toml configuration file");

        let cython_path = crate_dir.join("../../nautilus_trader/core/rust/backtest.pxd");
        cbindgen::generate_with_config(&crate_dir, config_cython)
            .expect("unable to generate bindings")
            .write_to_file(cython_path);
    }
}
