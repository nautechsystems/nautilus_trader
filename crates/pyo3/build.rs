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

//! Build script for the `nautilus-pyo3` extension module.
//!
//! On macOS the extension module statically links Arrow (and other C/C++ heavy dependencies).
//! When `pyarrow` or `pandas` is imported first, dyld has already loaded another copy of those
//! symbols, and the flat namespace lets the two runtimes interpose on each other which crashes
//! the process with a SIGSEGV (see issue #4633).
//!
//! Restricting the export table of the `cdylib` to the module initializer removes every symbol
//! that could collide, so each Arrow runtime keeps using its own statically linked copy.

fn main() {
    println!("cargo::rerun-if-changed=build.rs");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("macos") {
        return;
    }

    // The `ffi` feature deliberately exposes the C API for external consumers, so the export
    // table must stay intact in that configuration.
    if std::env::var_os("CARGO_FEATURE_FFI").is_some() {
        return;
    }

    // Must match the `#[pymodule]` function name (`cython-compat` renames it to `nautilus_pyo3`),
    // with the leading underscore Mach-O prepends to C symbols.
    let init_symbol = if std::env::var_os("CARGO_FEATURE_CYTHON_COMPAT").is_some() {
        "_PyInit_nautilus_pyo3"
    } else {
        "_PyInit__libnautilus"
    };

    // Passing `-exported_symbol` makes every other symbol in the image non-external.
    println!("cargo::rustc-link-arg-cdylib=-Wl,-exported_symbol,{init_symbol}");
}
