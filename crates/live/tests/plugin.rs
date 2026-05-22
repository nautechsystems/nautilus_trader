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
//! Most tests build the example cdylib shipped under
//! `crates/plugin/examples/custom_data_plugin.rs`, load it through the
//! live-node-bound [`PluginLoader`], and exercise the host-side adapter pieces.
//!
//! Those broader cdylib tests are marked `#[ignore]` so default `cargo test`
//! stays focused. Run them explicitly with:
//!
//! ```text
//! cargo test -p nautilus-live --features node --test plugin -- --ignored
//! ```
//!
//! The runtime smoke test builds
//! `crates/plugin/examples/runtime_smoke_plugin.rs` and runs by default. It
//! proves a configured real plug-in loads through [`LiveNode`], instantiates
//! from [`PluginConfig`], and receives `on_start`.
//!
//! The complementary `plugin_in_process.rs` integration tests run on every
//! `cargo test` and exercise the adapters via in-process [`PluginActor`]
//! and [`PluginStrategy`] types without the cdylib build.

#![allow(unsafe_code)]

use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::{Mutex, OnceLock},
    time::Duration,
};

use aws_lc_rs::digest;
use nautilus_common::actor::DataActor;
use nautilus_core::{UUID4, hex};
use nautilus_live::{
    config::{LiveExecEngineConfig, LiveNodeConfig, PluginConfig},
    node::LiveNode,
    plugin::{
        PluginActorAdapter, PluginStrategyAdapter, host_vtable, plugin_loader,
        register_custom_data_from_manifest,
    },
};
use nautilus_model::{
    data::{Data, registry::deserialize_custom_from_json},
    identifiers::{ActorId, StrategyId},
};
use nautilus_plugin::{
    PLUGIN_BUILD_ID_VERSION,
    manifest::{PluginManifest, ValidatedPluginManifest},
};
use nautilus_trading::strategy::StrategyConfig;
use rstest::{fixture, rstest};

const EXAMPLE_NAME: &str = "custom_data_plugin";
const RUNTIME_SMOKE_EXAMPLE_NAME: &str = "runtime_smoke_plugin";

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

            let artifact = cargo_target_dir(&root)
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

fn build_runtime_smoke_example_once() -> PathBuf {
    static EXAMPLE_PATH: OnceLock<PathBuf> = OnceLock::new();
    EXAMPLE_PATH
        .get_or_init(|| {
            let root = workspace_root();
            let status = Command::new(env!("CARGO"))
                .current_dir(&root)
                .args([
                    "build",
                    "-p",
                    "nautilus-plugin",
                    "--example",
                    RUNTIME_SMOKE_EXAMPLE_NAME,
                ])
                .status()
                .expect("cargo build of runtime smoke cdylib");
            assert!(status.success(), "failed to build runtime smoke cdylib");

            let artifact = cargo_target_dir(&root)
                .join("debug")
                .join("examples")
                .join(cdylib_filename(RUNTIME_SMOKE_EXAMPLE_NAME));
            assert!(
                artifact.exists(),
                "runtime smoke cdylib artifact not at {}",
                artifact.display()
            );
            artifact
        })
        .clone()
}

fn cargo_target_dir(root: &Path) -> PathBuf {
    let target_dir =
        std::env::var_os("CARGO_TARGET_DIR").map_or_else(|| PathBuf::from("target"), PathBuf::from);

    if target_dir.is_absolute() {
        target_dir
    } else {
        root.join(target_dir)
    }
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
    let manifest = ValidatedPluginManifest::new(example_manifest)
        .expect("live example manifest passes validation");
    // SAFETY: name string lives in cdylib static storage.
    assert_eq!(
        unsafe { example_manifest.plugin_name.as_str() },
        "example-custom-data-plugin"
    );
    assert_eq!(
        example_manifest.build_id.schema_version,
        PLUGIN_BUILD_ID_VERSION
    );
    // SAFETY: build id strings live in cdylib static storage.
    assert!(!unsafe { example_manifest.build_id.target_triple.as_str() }.is_empty());
    // SAFETY: build id strings live in cdylib static storage.
    assert!(!unsafe { example_manifest.build_id.build_profile.as_str() }.is_empty());
    assert_eq!(
        manifest.custom_data().len(),
        1,
        "example manifest carries one custom-data entry"
    );
    assert_eq!(
        manifest.actors().len(),
        1,
        "example manifest carries one actor entry"
    );
    assert_eq!(
        manifest.strategies().len(),
        1,
        "example manifest carries one strategy entry"
    );
}

#[rstest]
#[ignore]
fn actor_adapter_construct_and_dispatch_lifecycle(example_manifest: &'static PluginManifest) {
    let manifest = ValidatedPluginManifest::new(example_manifest)
        .expect("live example manifest passes validation");
    let entry = manifest.actors().next().expect("example actor entry");

    // SAFETY: host_vtable() is process-lifetime static.
    let mut adapter = unsafe {
        PluginActorAdapter::new(
            ActorId::from("PluginActor-001"),
            "example-custom-data-plugin",
            entry.type_name(),
            entry.vtable(),
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
    let manifest = ValidatedPluginManifest::new(example_manifest)
        .expect("live example manifest passes validation");
    let entry = manifest
        .strategies()
        .next()
        .expect("example strategy entry");

    let config = StrategyConfig::builder()
        .strategy_id(StrategyId::from("Plugin-001"))
        .order_id_tag("001".to_string())
        .build();

    // SAFETY: host_vtable() is process-lifetime static.
    let mut adapter = unsafe {
        PluginStrategyAdapter::new(
            config,
            "example-custom-data-plugin",
            entry.type_name(),
            entry.vtable(),
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
    let manifest = ValidatedPluginManifest::new(example_manifest)
        .expect("live example manifest passes validation");
    let count =
        register_custom_data_from_manifest(manifest).expect("custom data registration succeeds");
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

#[rstest]
#[ignore]
fn live_node_loads_configured_plugin_actor_strategy_and_custom_data() {
    let path = build_example_once();
    let config = LiveNodeConfig {
        exec_engine: LiveExecEngineConfig {
            reconciliation: false,
            ..Default::default()
        },
        plugins: vec![
            PluginConfig {
                path: path.display().to_string(),
                type_name: "ExampleActor".to_string(),
                config: HashMap::from([(
                    "actor_id".to_string(),
                    serde_json::json!("ExampleActor-001"),
                )]),
                sha256: None,
            },
            PluginConfig {
                path: path.display().to_string(),
                type_name: "ExampleStrategy".to_string(),
                config: HashMap::from([(
                    "strategy_id".to_string(),
                    serde_json::json!("ExampleStrategy-001"),
                )]),
                sha256: None,
            },
        ],
        ..Default::default()
    };

    let node = LiveNode::build("PluginConfigNode".to_string(), Some(config)).unwrap();
    let trader = node.kernel().trader.borrow();

    assert!(
        trader
            .actor_ids()
            .contains(&ActorId::from("ExampleActor-001"))
    );
    assert!(
        trader
            .strategy_ids()
            .contains(&StrategyId::from("ExampleStrategy-001"))
    );
    drop(trader);

    let envelope = serde_json::json!({
        "type": "Custom",
        "data_type": {
            "type_name": "ExampleTick",
        },
        "payload": {
            "value": 2.5,
            "ts_event": 20,
            "ts_init": 21,
        },
    });
    let data = deserialize_custom_from_json("ExampleTick", &envelope)
        .expect("deserializer returns Ok")
        .expect("custom data type is registered");
    assert!(matches!(data, Data::Custom(_)));
}

#[rstest]
#[ignore]
fn live_node_accepts_matching_plugin_sha256() {
    let path = build_example_once();
    let bytes = fs::read(&path).expect("example cdylib can be read");
    let sha256 = hex::encode(digest::digest(&digest::SHA256, &bytes).as_ref());
    let config = LiveNodeConfig {
        exec_engine: LiveExecEngineConfig {
            reconciliation: false,
            ..Default::default()
        },
        plugins: vec![PluginConfig {
            path: path.display().to_string(),
            type_name: "ExampleActor".to_string(),
            config: HashMap::from([(
                "actor_id".to_string(),
                serde_json::json!("ExampleActorSha-001"),
            )]),
            sha256: Some(sha256),
        }],
        ..Default::default()
    };

    let node = LiveNode::build("PluginShaOkNode".to_string(), Some(config)).unwrap();
    let trader = node.kernel().trader.borrow();

    assert!(
        trader
            .actor_ids()
            .contains(&ActorId::from("ExampleActorSha-001"))
    );
}

#[rstest]
#[ignore]
fn live_node_checks_sha256_for_each_configured_plugin_instance() {
    let path = build_example_once();
    let config = LiveNodeConfig {
        exec_engine: LiveExecEngineConfig {
            reconciliation: false,
            ..Default::default()
        },
        plugins: vec![
            PluginConfig {
                path: path.display().to_string(),
                type_name: "ExampleActor".to_string(),
                sha256: None,
                ..Default::default()
            },
            PluginConfig {
                path: path.display().to_string(),
                type_name: "ExampleStrategy".to_string(),
                sha256: Some("0".repeat(64)),
                ..Default::default()
            },
        ],
        ..Default::default()
    };

    let error = LiveNode::build("PluginShaNode".to_string(), Some(config))
        .unwrap_err()
        .to_string();

    assert!(error.contains("SHA-256 mismatch"));
}

#[tokio::test(flavor = "current_thread")]
async fn live_node_start_invokes_configured_plugin_actor() {
    let path = build_runtime_smoke_example_once();
    let marker = std::env::temp_dir().join(format!("nautilus-plugin-{}.txt", UUID4::new()));
    let _ = fs::remove_file(&marker);

    let config = LiveNodeConfig {
        delay_post_stop: Duration::ZERO,
        exec_engine: LiveExecEngineConfig {
            reconciliation: false,
            ..Default::default()
        },
        plugins: vec![PluginConfig {
            path: path.display().to_string(),
            type_name: "RuntimeSmokeActor".to_string(),
            config: HashMap::from([
                (
                    "actor_id".to_string(),
                    serde_json::json!("RuntimeSmokeActor-001"),
                ),
                (
                    "callback_path".to_string(),
                    serde_json::json!(marker.display().to_string()),
                ),
                ("label".to_string(), serde_json::json!("rust")),
            ]),
            sha256: None,
        }],
        ..Default::default()
    };

    let mut node = LiveNode::build("PluginRuntimeNode".to_string(), Some(config)).unwrap();
    let actor_registered = {
        let trader = node.kernel().trader.borrow();
        trader
            .actor_ids()
            .contains(&ActorId::from("RuntimeSmokeActor-001"))
    };
    assert!(actor_registered);

    node.start().await.unwrap();
    let contents = fs::read_to_string(&marker).expect("plug-in actor writes callback marker");
    node.stop().await.unwrap();
    let _ = fs::remove_file(marker);

    assert_eq!(contents, "rust:on_start\n");
}
