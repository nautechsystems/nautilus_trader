// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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

extern crate cbindgen;

use std::{
    env,
    fs::File,
    io::{Read, Write},
    path::PathBuf,
};

#[allow(clippy::expect_used)] // OK in build script
fn main() {
    let crate_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));

    // Generate C headers
    let config_c = cbindgen::Config::from_file("cbindgen.toml")
        .expect("unable to find cbindgen.toml configuration file");

    let c_header_path = crate_dir.join("../../nautilus_trader/core/includes/model.h");
    cbindgen::generate_with_config(&crate_dir, config_c)
        .expect("unable to generate bindings")
        .write_to_file(c_header_path);

    // Generate Cython definitions
    let config_cython = cbindgen::Config::from_file("cbindgen_cython.toml")
        .expect("unable to find cbindgen_cython.toml configuration file");

    let cython_path = crate_dir.join("../../nautilus_trader/core/rust/model.pxd");
    cbindgen::generate_with_config(&crate_dir, config_cython)
        .expect("unable to generate bindings")
        .write_to_file(cython_path.clone());

    // Open and read the file entirely
    let mut src = File::open(cython_path.clone()).expect("`File::open` failed");
    let mut data = String::new();
    src.read_to_string(&mut data)
        .expect("invalid UTF-8 in stream");
    drop(src); // Close the file early

    // Run the replace operation in memory
    let new_data = data.replace("cdef enum", "cpdef enum");

    // Recreate the file and dump the processed contents to it
    let mut dst = File::create(cython_path).expect("`File::create` failed");
    dst.write_all(new_data.as_bytes())
        .expect("I/O error on `dist.write`");
}
