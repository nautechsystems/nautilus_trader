// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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

use std::{env, fs, path::PathBuf};

use toml::Value;

#[allow(clippy::expect_used)] // OK in build script
fn main() {
    // Ensure the build script runs again if `build.rs` or top-level `pyproject.toml` changes
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=../../pyproject.toml");

    let crate_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let project_dir = crate_dir
        .parent()
        .expect("Failed to get workspace root")
        .parent()
        .expect("Failed to get project root");

    let filepath = project_dir.join("pyproject.toml");
    let data = fs::read_to_string(filepath).expect("Unable to read pyproject.toml");
    let parsed_toml: Value = toml::from_str(&data).expect("Unable to parse pyproject.toml");

    let nautilus_version = parsed_toml["tool"]["poetry"]["version"]
        .as_str()
        .expect("Unable to find version in pyproject.toml")
        .to_string();

    // Set compile-time environment variables
    println!("cargo:rustc-env=NAUTILUS_VERSION={nautilus_version}");
    println!("cargo:rustc-env=NAUTILUS_USER_AGENT=NautilusTrader/{nautilus_version}");

    #[cfg(feature = "ffi")]
    if env::var("CARGO_FEATURE_FFI").is_ok() {
        use std::io::{Read, Write};

        use cbindgen;
        use fs::File;

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
