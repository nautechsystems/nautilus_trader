// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2022 Nautech Systems Pty Ltd. All rights reserved.
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

use std::env;
use std::path::PathBuf;

fn main() {
    let crate_dir = PathBuf::from(
        env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR env var is not defined"),
    );

    // Generate C headers
    let config_c = cbindgen::Config::from_file("cbindgen.toml")
        .expect("Unable to find cbindgen.toml configuration file");

    cbindgen::generate_with_config(&crate_dir, config_c.clone())
        .expect("Unable to generate bindings")
        .write_to_file(crate_dir.join("persistence.h"));

    cbindgen::generate_with_config(&crate_dir, config_c)
        .expect("Unable to generate bindings")
        .write_to_file(crate_dir.join("../../nautilus_trader/core/includes/persistence.h"));

    // Generate Cython definitions
    let config_cython = cbindgen::Config::from_file("cbindgen_cython.toml")
        .expect("Unable to find cbindgen.toml configuration file");

    cbindgen::generate_with_config(&crate_dir, config_cython)
        .expect("Unable to generate bindings")
        .write_to_file(crate_dir.join("../../nautilus_trader/core/rust/persistence.pxd"));
}
