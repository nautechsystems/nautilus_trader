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

use std::{env, process::Command};

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=RUSTC");

    let rustc_version = rustc_version();
    let target = env::var("TARGET").unwrap_or_default();
    let profile = env::var("PROFILE").unwrap_or_default();
    println!("cargo:rustc-env=NAUTILUS_PLUGIN_BUILD_RUSTC_VERSION={rustc_version}");
    println!("cargo:rustc-env=NAUTILUS_PLUGIN_BUILD_TARGET={target}");
    println!("cargo:rustc-env=NAUTILUS_PLUGIN_BUILD_PROFILE={profile}");
}

fn rustc_version() -> String {
    let rustc = env::var("RUSTC").unwrap_or_else(|_| "rustc".to_string());
    let Ok(output) = Command::new(rustc).arg("--version").output() else {
        return String::new();
    };

    if !output.status.success() {
        return String::new();
    }
    String::from_utf8(output.stdout)
        .map(|s| s.trim().to_string())
        .unwrap_or_default()
}
