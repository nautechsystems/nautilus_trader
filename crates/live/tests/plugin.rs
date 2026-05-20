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

//! Integration tests for the host-side plug-in adapters in `nautilus-live`.
//!
//! The tests build the example cdylib shipped under
//! `crates/plugin/examples/custom_data_plugin.rs`, load it through the
//! live-node-bound [`PluginLoader`], and exercise the adapter pieces that
//! Phase 1 ships.
//!
//! Every test is marked `#[ignore]` so default `cargo test` stays fast.
//! Run explicitly with:
//!
//! ```bash
//! cargo test -p nautilus-live --features node --test plugin -- --ignored
//! ```
//!
//! The complementary `plugin_in_process.rs` integration tests run on every
//! `cargo test` and exercise the adapters via in-process [`PluginActor`]
//! and [`PluginStrategy`] types without the cdylib build.

#![allow(unsafe_code)]

use std::{
    path::PathBuf,
    process::Command,
    sync::{Mutex, OnceLock},
};

use nautilus_common::actor::DataActor;
use nautilus_live::plugin::{
    PluginActorAdapter, PluginStrategyAdapter, host_vtable, plugin_loader,
    register_custom_data_from_manifest,
};
use nautilus_model::{
    data::{Data, registry::deserialize_custom_from_json},
    identifiers::{ActorId, StrategyId},
};
use nautilus_plugin::manifest::PluginManifest;
use nautilus_trading::strategy::StrategyConfig;
use rstest::{fixture, rstest};

const EXAMPLE_NAME: &str = "custom_data_plugin";

fn workspace_root() -> PathBuf {
    let p = std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR is set by cargo");
    // crates/live -> repo root
    PathBuf::from(p)
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root reachable from CARGO_MANIFEST_DIR")
        .to_path_buf()
}

fn cdylib_filename(name: &str) -> String {
    if cfg!(target_os = "windows") {
        format!("{name}.dll")
    } else if cfg!(target_os = "macos") {
        format!("lib{name}.dylib")
    } else {
        format!("lib{name}.so")
    }
}

fn build_example_once() -> PathBuf {
    static EXAMPLE_PATH: OnceLock<PathBuf> = OnceLock::new();
    EXAMPLE_PATH
        .get_or_init(|| {
            let root = workspace_root();
            let status = Command::new(env!("CARGO"))
                .current_dir(&root)
                .args(["build", "-p", "nautilus-plugin", "--example", EXAMPLE_NAME])
                .status()
                .expect("cargo build of example cdylib");
            assert!(status.success(), "failed to build example cdylib");

            let artifact = root
                .join("target")
                .join("debug")
                .join("examples")
                .join(cdylib_filename(EXAMPLE_NAME));
            assert!(
                artifact.exists(),
                "example cdylib artifact not at {}",
                artifact.display()
            );
            artifact
        })
        .clone()
    // OnceLock cloned PathBuf so callers can use it without holding the lock.
}

// The PluginLoader keeps libraries alive for the process lifetime, so we
// can hand out a `&PluginManifest` that lives forever. A Mutex serializes
// loading so concurrent rstest cases never race on the libloading internals.
fn loaded_manifest() -> &'static PluginManifest {
    static MANIFEST: OnceLock<&'static PluginManifest> = OnceLock::new();
    static LOAD_GUARD: Mutex<()> = Mutex::new(());
    MANIFEST.get_or_init(|| {
        let _guard = LOAD_GUARD.lock().unwrap();
        let path = build_example_once();
        let loader: &'static mut _ = Box::leak(Box::new(plugin_loader()));
        let loaded = loader
            .load(&path)
            .expect("loader should accept the example cdylib");
        // SAFETY: loader is leaked, so its inner manifest pointer stays live
        // for the process lifetime. Returning the static reference is sound.
        unsafe { &*std::ptr::from_ref::<PluginManifest>(loaded.manifest()) }
    })
}

#[fixture]
fn example_manifest() -> &'static PluginManifest {
    loaded_manifest()
}

#[rstest]
#[ignore]
fn loader_loads_example_cdylib(example_manifest: &'static PluginManifest) {
    assert!(example_manifest.matches_compiled_abi());
    // SAFETY: name string lives in cdylib static storage.
    assert_eq!(
        unsafe { example_manifest.plugin_name.as_str() },
        "example-custom-data-plugin"
    );
    // SAFETY: slice points at static storage owned by the manifest.
    let cd = unsafe { example_manifest.custom_data.as_slice() };
    assert_eq!(
        cd.len(),
        1,
        "example manifest carries one custom-data entry"
    );
    // SAFETY: slice points at static storage owned by the manifest.
    let actors = unsafe { example_manifest.actors.as_slice() };
    assert_eq!(actors.len(), 1, "example manifest carries one actor entry");
    // SAFETY: see above.
    let strategies = unsafe { example_manifest.strategies.as_slice() };
    assert_eq!(
        strategies.len(),
        1,
        "example manifest carries one strategy entry"
    );
}

#[rstest]
#[ignore]
fn actor_adapter_construct_and_dispatch_lifecycle(example_manifest: &'static PluginManifest) {
    // SAFETY: actors slice is process-lifetime static.
    let entry = unsafe { &example_manifest.actors.as_slice()[0] };
    // SAFETY: type_name string lives in cdylib static storage.
    let type_name = unsafe { entry.type_name.as_str() };

    // SAFETY: vtable + host_vtable() are process-lifetime static.
    let mut adapter = unsafe {
        PluginActorAdapter::new(
            ActorId::from("PluginActor-001"),
            "example-custom-data-plugin",
            type_name,
            entry.vtable,
            host_vtable(),
            "{}",
        )
    }
    .expect("actor adapter construction succeeds");

    // No-op default thunks should return Ok.
    DataActor::on_start(&mut adapter).expect("on_start dispatches into plug-in");
    DataActor::on_stop(&mut adapter).expect("on_stop dispatches into plug-in");
    DataActor::on_reset(&mut adapter).expect("on_reset dispatches into plug-in");
    DataActor::on_dispose(&mut adapter).expect("on_dispose dispatches into plug-in");
}

#[rstest]
#[ignore]
fn strategy_adapter_construct(example_manifest: &'static PluginManifest) {
    // SAFETY: strategies slice is process-lifetime static.
    let entry = unsafe { &example_manifest.strategies.as_slice()[0] };
    // SAFETY: type_name string lives in cdylib static storage.
    let type_name = unsafe { entry.type_name.as_str() };

    let config = StrategyConfig::builder()
        .strategy_id(StrategyId::from("Plugin-001"))
        .order_id_tag("001".to_string())
        .build();

    // SAFETY: vtable + host_vtable() are process-lifetime static.
    let mut adapter = unsafe {
        PluginStrategyAdapter::new(
            config,
            "example-custom-data-plugin",
            type_name,
            entry.vtable,
            host_vtable(),
            "{}",
        )
    }
    .expect("strategy adapter construction succeeds");

    DataActor::on_start(&mut adapter).expect("on_start dispatches into plug-in");
    DataActor::on_stop(&mut adapter).expect("on_stop dispatches into plug-in");
}

#[rstest]
#[ignore]
fn custom_data_registration_round_trips_via_registry(example_manifest: &'static PluginManifest) {
    // SAFETY: manifest is process-lifetime static; slices are bounded by it.
    let count = unsafe { register_custom_data_from_manifest(example_manifest) }
        .expect("custom data registration succeeds");
    assert_eq!(count, 1);

    // Build a `CustomData` envelope matching what the engine's serializer
    // produces: `{ "type": "Custom", "data_type": { "type_name": ... }, "payload": ... }`.
    let envelope = serde_json::json!({
        "type": "Custom",
        "data_type": {
            "type_name": "ExampleTick",
        },
        "payload": {
            "value": 1.5,
            "ts_event": 10,
            "ts_init": 11,
        },
    });

    let data = deserialize_custom_from_json("ExampleTick", &envelope)
        .expect("deserializer returns Ok")
        .expect("custom data type is registered");

    let Data::Custom(custom) = data else {
        panic!("expected Custom variant");
    };
    assert_eq!(custom.data.type_name(), "ExampleTick");
}
