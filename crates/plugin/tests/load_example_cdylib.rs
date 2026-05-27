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
//! Marked `#[ignore]` so plain `cargo test` stays fast. The required Linux cdylib smoke check
//! runs it through `make cargo-test-plugin-cdylib-smoke`.

#![cfg(feature = "host")]
#![allow(unsafe_code)]

use std::{
    env,
    path::{Path, PathBuf},
    process::Command,
};

use nautilus_model::types::fixed::FIXED_PRECISION;
use nautilus_plugin::{
    NAUTILUS_PLUGIN_ABI_VERSION, PLUGIN_BUILD_ID_VERSION,
    loader::{LoadError, PluginLoader},
    manifest::compiled_precision_mode,
};

const PLUGIN_TEST_PROFILE: &str = "nextest";
const INVALID_MANIFEST_MESSAGES: &[&str] = &[
    "plugin_name must not be empty",
    "plugin_version has null pointer with non-zero length 1",
    "custom_data[0].vtable must not be null",
    "actors has null pointer with non-zero length 1",
    "strategies[0].type_name is not valid UTF-8",
    "strategies[0].vtable must not be null",
];

#[derive(Clone, Copy)]
enum LoadErrorExpectation {
    MissingSymbol,
    NullManifest,
    AbiMismatch { actual: u32 },
    InvalidManifest { messages: &'static [&'static str] },
}

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

fn build_example_cdylib(example_name: &str) -> PathBuf {
    let mut build_command = Command::new(env!("CARGO"));
    build_command.args([
        "build",
        "-p",
        "nautilus-plugin",
        "--example",
        example_name,
        "--profile",
        PLUGIN_TEST_PROFILE,
    ]);

    if FIXED_PRECISION > 9 {
        build_command.args(["--features", "nautilus-model/high-precision"]);
    }

    let status = build_command.status().expect("invoke cargo build");
    assert!(status.success(), "cargo build --example failed");

    let path = example_cdylib_path(example_name);
    assert!(path.exists(), "expected cdylib at {}", path.display());
    path
}

fn example_cdylib_path(example_name: &str) -> PathBuf {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.pop(); // crates/
    path.pop(); // workspace root
    path = cargo_target_dir(&path);
    path.push(PLUGIN_TEST_PROFILE);
    path.push("examples");
    path.push(format!(
        "{}{}.{}",
        cdylib_prefix(),
        example_name,
        cdylib_extension()
    ));
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
    let path = build_example_cdylib("custom_data_plugin");
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
    // SAFETY: build id strings live in the cdylib for the process lifetime.
    assert_eq!(
        unsafe { manifest.build_id.precision_mode.as_str() },
        compiled_precision_mode()
    );
    assert_eq!(manifest.build_id.fixed_precision, FIXED_PRECISION);

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

#[rstest::rstest]
#[case::missing_init_symbol("bad_missing_init_plugin", LoadErrorExpectation::MissingSymbol)]
#[case::null_manifest("bad_null_manifest_plugin", LoadErrorExpectation::NullManifest)]
#[case::wrong_abi(
    "bad_abi_manifest_plugin",
    LoadErrorExpectation::AbiMismatch {
        actual: NAUTILUS_PLUGIN_ABI_VERSION + 1,
    }
)]
#[case::invalid_manifest(
    "bad_invalid_manifest_plugin",
    LoadErrorExpectation::InvalidManifest {
        messages: INVALID_MANIFEST_MESSAGES,
    }
)]
#[case::init_panic("bad_init_panic_plugin", LoadErrorExpectation::NullManifest)]
#[ignore]
fn rejects_malformed_cdylib_fixture(
    #[case] example_name: &str,
    #[case] expectation: LoadErrorExpectation,
) {
    let path = build_example_cdylib(example_name);
    let mut loader = PluginLoader::new();
    let err = loader
        .load(&path)
        .expect_err("malformed fixture should fail to load");

    assert_load_error(err, &path, expectation);
    assert_eq!(loader.len(), 0);
}

fn assert_load_error(err: LoadError, path: &Path, expectation: LoadErrorExpectation) {
    match (err, expectation) {
        (LoadError::MissingSymbol { path: actual, .. }, LoadErrorExpectation::MissingSymbol) => {
            assert_eq!(actual.as_path(), path);
        }
        (LoadError::NullManifest { path: actual }, LoadErrorExpectation::NullManifest) => {
            assert_eq!(actual.as_path(), path);
        }
        (
            LoadError::AbiMismatch {
                path: actual_path,
                expected,
                actual,
                diagnostics,
            },
            LoadErrorExpectation::AbiMismatch {
                actual: expected_actual,
            },
        ) => {
            assert_eq!(actual_path.as_path(), path);
            assert_eq!(expected, NAUTILUS_PLUGIN_ABI_VERSION);
            assert_eq!(actual, expected_actual);
            assert_eq!(diagnostics.plugin_name.as_str(), "bad-abi-plugin");
            assert_eq!(
                diagnostics.plugin_version.as_str(),
                env!("CARGO_PKG_VERSION")
            );
        }
        (
            LoadError::InvalidManifest {
                path: actual_path,
                errors,
                ..
            },
            LoadErrorExpectation::InvalidManifest { messages },
        ) => {
            assert_eq!(actual_path.as_path(), path);
            let rendered = errors.to_string();

            for message in messages {
                assert!(
                    rendered.contains(message),
                    "expected manifest error containing {message:?}, was: {rendered}",
                );
            }
        }
        (err, _) => panic!("unexpected load error: {err:?}"),
    }
}
