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

//! End-to-end load test that builds the example cdylib and `dlopen`s it.
//!
//! Marked `#[ignore]` so `cargo test` stays fast; run explicitly with
//! `cargo test -p nautilus-plugin --features host --test load_example_cdylib -- --ignored`.

#![cfg(feature = "host")]
#![allow(unsafe_code)]

use std::{
    env,
    path::{Path, PathBuf},
    process::Command,
};

use nautilus_plugin::{NAUTILUS_PLUGIN_ABI_VERSION, PLUGIN_BUILD_ID_VERSION, loader::PluginLoader};

fn cdylib_extension() -> &'static str {
    if cfg!(target_os = "macos") {
        "dylib"
    } else if cfg!(target_os = "windows") {
        "dll"
    } else {
        "so"
    }
}

fn cdylib_prefix() -> &'static str {
    if cfg!(target_os = "windows") {
        ""
    } else {
        "lib"
    }
}

fn build_example_cdylib() -> PathBuf {
    let status = Command::new(env!("CARGO"))
        .args([
            "build",
            "-p",
            "nautilus-plugin",
            "--example",
            "custom_data_plugin",
        ])
        .status()
        .expect("invoke cargo build");
    assert!(status.success(), "cargo build --example failed");

    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.pop(); // crates/
    path.pop(); // workspace root
    path = cargo_target_dir(&path);
    path.push(if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    });
    path.push("examples");
    path.push(format!(
        "{}custom_data_plugin.{}",
        cdylib_prefix(),
        cdylib_extension()
    ));
    assert!(path.exists(), "expected cdylib at {}", path.display());
    path
}

fn cargo_target_dir(root: &Path) -> PathBuf {
    let target_dir =
        env::var_os("CARGO_TARGET_DIR").map_or_else(|| PathBuf::from("target"), PathBuf::from);

    if target_dir.is_absolute() {
        target_dir
    } else {
        root.join(target_dir)
    }
}

#[rstest::rstest]
#[ignore]
fn loads_example_cdylib_and_walks_manifest() {
    let path = build_example_cdylib();
    let mut loader = PluginLoader::new();
    loader.load(&path).expect("load failed");
    assert_eq!(loader.len(), 1);
    let plugin = &loader.loaded()[0];
    let manifest = plugin.manifest();
    assert_eq!(manifest.abi_version, NAUTILUS_PLUGIN_ABI_VERSION);
    manifest
        .validate()
        .expect("example cdylib manifest passes validation");
    // SAFETY: name string lives in the cdylib for the process lifetime.
    assert_eq!(
        unsafe { manifest.plugin_name.as_str() },
        "example-custom-data-plugin"
    );
    assert_eq!(manifest.build_id.schema_version, PLUGIN_BUILD_ID_VERSION);
    // SAFETY: build id strings live in the cdylib for the process lifetime.
    assert_eq!(
        unsafe { manifest.build_id.nautilus_plugin_version.as_str() },
        env!("CARGO_PKG_VERSION")
    );
    // SAFETY: build id strings live in the cdylib for the process lifetime.
    assert!(!unsafe { manifest.build_id.target_triple.as_str() }.is_empty());
    // SAFETY: build id strings live in the cdylib for the process lifetime.
    assert!(!unsafe { manifest.build_id.build_profile.as_str() }.is_empty());

    // SAFETY: slice points at storage inside the loaded cdylib.
    let cd = unsafe { manifest.custom_data.as_slice() };
    assert_eq!(cd.len(), 1, "one custom-data registration expected");
    // SAFETY: type_name lives in the cdylib for the process lifetime.
    assert_eq!(unsafe { cd[0].type_name.as_str() }, "ExampleTick");

    // SAFETY: slice points at storage inside the loaded cdylib.
    let actors = unsafe { manifest.actors.as_slice() };
    assert_eq!(actors.len(), 1, "one actor registration expected");
    // SAFETY: type_name lives in the cdylib for the process lifetime.
    assert_eq!(unsafe { actors[0].type_name.as_str() }, "ExampleActor");

    // SAFETY: slice points at storage inside the loaded cdylib.
    let strategies = unsafe { manifest.strategies.as_slice() };
    assert_eq!(strategies.len(), 1, "one strategy registration expected");
    // SAFETY: type_name lives in the cdylib for the process lifetime.
    assert_eq!(
        unsafe { strategies[0].type_name.as_str() },
        "ExampleStrategy",
    );
}
