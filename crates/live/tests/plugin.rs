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
//! The cdylib tests build the example plug-ins shipped under
//! `crates/plugin/examples/`, load them through the live-node-bound
//! [`PluginLoader`], and exercise the host-side adapter pieces.
//!
//! Related cdylib boundary cases are grouped into a few test processes.
//! `cargo-nextest` runs each test in its own process, so grouping lets
//! process-local [`OnceLock`] caches share the example build and manifest load.
//! The grouped `#[rstest]` functions are the test entry points; the unannotated
//! functions they call are scenario bodies kept separate for readable failures
//! and focused assertions.
//!
//! The runtime smoke test builds
//! `crates/plugin/examples/runtime_smoke_plugin.rs` and runs by default. It
//! proves a configured real plug-in loads through [`LiveNode`], instantiates
//! from [`PluginConfig`], and receives `on_start`.
//!
//! The complementary `plugin_dispatch.rs` integration tests run on every
//! `cargo test` and exercise the adapters via in-process [`PluginActor`]
//! and [`PluginStrategy`] types without the cdylib build.
//!
//! These cdylib build/load tests run on Linux only because the plug-in system
//! is supported on Linux only.

#![cfg(target_os = "linux")]
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
use nautilus_common::{
    actor::{DataActor, registry::register_actor},
    cache::{Cache, ORDER_NOT_FOUND},
    clock::{Clock, TestClock},
    component::Component,
    messages::execution::TradingCommand,
    msgbus::{self, MessagingSwitchboard, TypedIntoHandler, switchboard::get_quotes_topic},
    timer::{TimeEvent, TimeEventCallback},
};
use nautilus_core::{Params, UUID4, UnixNanos, hex};
use nautilus_live::{
    config::{LiveExecEngineConfig, LiveNodeConfig, PluginConfig},
    node::{LiveNode, NodeState},
    plugin::{
        HostContextInner, PluginActorAdapter, PluginStrategyAdapter, host_vtable, plugin_loader,
        register_custom_data_from_manifest,
    },
};
use nautilus_model::{
    data::{
        CustomData, Data, OptionChainSlice, QuoteTick, registry::deserialize_custom_from_json,
        stubs::stub_deltas,
    },
    enums::{BookType, OmsType, OrderSide, TimeInForce},
    events::OrderEventAny,
    identifiers::{
        AccountId, ActorId, ClientId, ClientOrderId, InstrumentId, OptionSeriesId, PositionId,
        StrategyId, TraderId, VenueOrderId,
    },
    instruments::{Instrument, InstrumentAny, stubs},
    orderbook::OrderBook,
    orders::{MarketOrder, Order, OrderAny, stubs::TestOrderEventStubs},
    position::Position,
    types::{Price, Quantity, fixed::FIXED_PRECISION},
};
use nautilus_plugin::{
    PLUGIN_BUILD_ID_VERSION,
    manifest::{PluginManifest, ValidatedPluginManifest},
};
use nautilus_portfolio::portfolio::Portfolio;
use nautilus_trading::strategy::{Strategy, StrategyConfig};
use rstest::{fixture, rstest};

const EXAMPLE_NAME: &str = "custom_data_plugin";
const RUNTIME_SMOKE_EXAMPLE_NAME: &str = "runtime_smoke_plugin";
const EXEC_TEST_EXAMPLE_NAME: &str = "exec_test_plugin";
const ACTOR_EVENT_EXAMPLE_NAME: &str = "actor_event_plugin";
const PLUGIN_TEST_PROFILE: &str = "nextest";

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
        .get_or_init(|| build_plugin_example(EXAMPLE_NAME))
        .clone()
    // OnceLock cloned PathBuf so callers can use it without holding the lock.
}

fn build_runtime_smoke_example_once() -> PathBuf {
    static EXAMPLE_PATH: OnceLock<PathBuf> = OnceLock::new();
    EXAMPLE_PATH
        .get_or_init(|| build_plugin_example(RUNTIME_SMOKE_EXAMPLE_NAME))
        .clone()
}

fn build_exec_test_example_once() -> PathBuf {
    static EXAMPLE_PATH: OnceLock<PathBuf> = OnceLock::new();
    EXAMPLE_PATH
        .get_or_init(|| build_plugin_example(EXEC_TEST_EXAMPLE_NAME))
        .clone()
}

fn build_actor_event_example_once() -> PathBuf {
    static EXAMPLE_PATH: OnceLock<PathBuf> = OnceLock::new();
    EXAMPLE_PATH
        .get_or_init(|| build_plugin_example(ACTOR_EVENT_EXAMPLE_NAME))
        .clone()
}

fn build_plugin_example(name: &str) -> PathBuf {
    let root = workspace_root();
    let mut args = vec![
        "build",
        "-p",
        "nautilus-plugin",
        "--example",
        name,
        "--profile",
        PLUGIN_TEST_PROFILE,
    ];

    if host_model_uses_high_precision() {
        args.extend(["--features", "nautilus-model/high-precision"]);
    }

    let status = Command::new(env!("CARGO"))
        .current_dir(&root)
        .args(args)
        .status()
        .expect("cargo build of plug-in example cdylib");
    assert!(status.success(), "failed to build plug-in example cdylib");

    let artifact = cargo_target_dir(&root)
        .join(PLUGIN_TEST_PROFILE)
        .join("examples")
        .join(cdylib_filename(name));
    assert!(
        artifact.exists(),
        "plug-in example cdylib artifact not at {}",
        artifact.display()
    );
    artifact
}

fn host_model_uses_high_precision() -> bool {
    FIXED_PRECISION > 9
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
        load_manifest_from_path(&path)
    })
}

fn loaded_exec_manifest() -> &'static PluginManifest {
    static MANIFEST: OnceLock<&'static PluginManifest> = OnceLock::new();
    static LOAD_GUARD: Mutex<()> = Mutex::new(());
    MANIFEST.get_or_init(|| {
        let _guard = LOAD_GUARD.lock().unwrap();
        let path = build_exec_test_example_once();
        load_manifest_from_path(&path)
    })
}

fn loaded_actor_event_manifest() -> &'static PluginManifest {
    static MANIFEST: OnceLock<&'static PluginManifest> = OnceLock::new();
    static LOAD_GUARD: Mutex<()> = Mutex::new(());
    MANIFEST.get_or_init(|| {
        let _guard = LOAD_GUARD.lock().unwrap();
        let path = build_actor_event_example_once();
        load_manifest_from_path(&path)
    })
}

fn load_manifest_from_path(path: &Path) -> &'static PluginManifest {
    let loader: &'static mut _ = Box::leak(Box::new(plugin_loader()));
    let loaded = loader
        .load(path)
        .expect("loader should accept the example cdylib");
    // SAFETY: loader is leaked, so its inner manifest pointer stays live
    // for the process lifetime. Returning the static reference is sound.
    unsafe { &*std::ptr::from_ref::<PluginManifest>(loaded.manifest()) }
}

#[fixture]
fn example_manifest() -> &'static PluginManifest {
    loaded_manifest()
}

fn register_strategy_adapter(adapter: &mut PluginStrategyAdapter) {
    let trader_id = TraderId::from("TRADER-001");
    let clock = std::rc::Rc::new(std::cell::RefCell::new(TestClock::new()));
    clock
        .borrow_mut()
        .register_default_handler(TimeEventCallback::from(|_event: TimeEvent| {}));
    let cache = std::rc::Rc::new(std::cell::RefCell::new(Cache::default()));
    let portfolio = std::rc::Rc::new(std::cell::RefCell::new(Portfolio::new(
        cache.clone(),
        clock.clone(),
        None,
    )));
    adapter
        .core_mut()
        .register(trader_id, clock, cache, portfolio)
        .expect("strategy register");
    adapter.initialize().expect("strategy initialize");
}

fn register_actor_adapter(adapter: &mut PluginActorAdapter) {
    adapter
        .register(
            TraderId::from("TRADER-001"),
            std::rc::Rc::new(std::cell::RefCell::new(TestClock::new())),
            std::rc::Rc::new(std::cell::RefCell::new(Cache::default())),
        )
        .expect("actor register");
    Component::start(adapter).expect("actor starts");
}

fn plugin_test_instrument_id() -> InstrumentId {
    InstrumentId::from("ETH-USDT.BINANCE")
}

fn plugin_test_quote() -> QuoteTick {
    QuoteTick::new(
        plugin_test_instrument_id(),
        Price::from("100.00"),
        Price::from("100.50"),
        Quantity::from("1.0"),
        Quantity::from("2.0"),
        UnixNanos::from(1u64),
        UnixNanos::from(1u64),
    )
}

fn plugin_test_order_book() -> OrderBook {
    OrderBook::new(plugin_test_instrument_id(), BookType::L2_MBP)
}

fn plugin_test_order(strategy_id: StrategyId, client_order_id: ClientOrderId) -> OrderAny {
    OrderAny::Market(MarketOrder::new(
        TraderId::from("TRADER-001"),
        strategy_id,
        plugin_test_instrument_id(),
        client_order_id,
        OrderSide::Buy,
        Quantity::from("1.0"),
        TimeInForce::Gtc,
        UUID4::new(),
        UnixNanos::default(),
        false,
        false,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
    ))
}

fn plugin_test_accepted_order(strategy_id: StrategyId, client_order_id: ClientOrderId) -> OrderAny {
    let mut order = plugin_test_order(strategy_id, client_order_id);
    let account_id = AccountId::from("SIM-001");
    order
        .apply(TestOrderEventStubs::submitted(&order, account_id))
        .expect("submitted event applies");
    order
        .apply(TestOrderEventStubs::accepted(
            &order,
            account_id,
            VenueOrderId::from("V-CDYLIB-001"),
        ))
        .expect("accepted event applies");
    order
}

fn plugin_test_position(strategy_id: StrategyId, position_id: PositionId) -> Position {
    let instrument = InstrumentAny::CurrencyPair(stubs::currency_pair_ethusdt());
    let order = plugin_test_accepted_order(strategy_id, ClientOrderId::from("O-CDYLIB-POS-OPEN"));
    let fill = TestOrderEventStubs::filled(
        &order,
        &instrument,
        None,
        Some(position_id),
        Some(Price::from("100.00")),
        Some(Quantity::from("1.0")),
        None,
        None,
        None,
        Some(AccountId::from("SIM-001")),
    );
    let OrderEventAny::Filled(fill) = fill else {
        panic!("expected filled event");
    };
    Position::new(&instrument, fill)
}

fn plugin_test_option_series_id() -> OptionSeriesId {
    "DERIBIT:BTC:BTC:1700000000000000000"
        .parse()
        .expect("option series id parses")
}

fn plugin_test_option_chain() -> OptionChainSlice {
    OptionChainSlice::new(plugin_test_option_series_id())
}

fn plugin_test_custom_data(value: f64) -> CustomData {
    let manifest = ValidatedPluginManifest::new(loaded_manifest())
        .expect("live example manifest passes validation");
    register_custom_data_from_manifest(manifest).expect("custom data registration succeeds");
    let envelope = serde_json::json!({
        "type": "Custom",
        "data_type": {
            "type_name": "ExampleTick",
        },
        "payload": {
            "value": value,
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
    custom
}

#[rstest]
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
fn cdylib_custom_data_adapters_dispatch_lifecycle_and_custom_data(
    example_manifest: &'static PluginManifest,
) {
    actor_adapter_construct_and_dispatch_lifecycle(example_manifest);
    strategy_adapter_construct(example_manifest);
    cdylib_actor_custom_data_dispatches_to_on_data(example_manifest);
    cdylib_actor_historical_custom_data_dispatches_to_on_data(example_manifest);
    cdylib_strategy_custom_data_dispatches_to_on_data(example_manifest);
    cdylib_strategy_historical_custom_data_dispatches_to_on_data(example_manifest);
}

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

fn cdylib_actor_custom_data_dispatches_to_on_data(example_manifest: &'static PluginManifest) {
    let manifest = ValidatedPluginManifest::new(example_manifest)
        .expect("live example manifest passes validation");
    let entry = manifest.actors().next().expect("example actor entry");
    let marker = std::env::temp_dir().join(format!(
        "nautilus-plugin-actor-custom-data-{}.txt",
        UUID4::new()
    ));
    let _ = fs::remove_file(&marker);
    let config_json = serde_json::json!({
        "callback_path": marker.display().to_string(),
    })
    .to_string();

    // SAFETY: host_vtable() is process-lifetime static.
    let mut adapter = unsafe {
        PluginActorAdapter::new(
            ActorId::from("ExampleActorCustomData-001"),
            "example-custom-data-plugin",
            entry.type_name(),
            entry.vtable(),
            host_vtable(),
            &config_json,
        )
    }
    .expect("actor adapter construction succeeds");
    register_actor_adapter(&mut adapter);

    let custom = plugin_test_custom_data(7.25);
    DataActor::on_data(&mut adapter, &custom).expect("on_data dispatches");
    let contents = fs::read_to_string(&marker).expect("plug-in actor writes custom data marker");
    let _ = fs::remove_file(marker);

    assert_eq!(contents, "7.25");
}

fn cdylib_actor_historical_custom_data_dispatches_to_on_data(
    example_manifest: &'static PluginManifest,
) {
    let manifest = ValidatedPluginManifest::new(example_manifest)
        .expect("live example manifest passes validation");
    let entry = manifest.actors().next().expect("example actor entry");
    let marker = std::env::temp_dir().join(format!(
        "nautilus-plugin-actor-historical-custom-data-{}.txt",
        UUID4::new()
    ));
    let _ = fs::remove_file(&marker);
    let config_json = serde_json::json!({
        "callback_path": marker.display().to_string(),
    })
    .to_string();

    // SAFETY: host_vtable() is process-lifetime static.
    let mut adapter = unsafe {
        PluginActorAdapter::new(
            ActorId::from("ExampleActorHistoricalCustomData-001"),
            "example-custom-data-plugin",
            entry.type_name(),
            entry.vtable(),
            host_vtable(),
            &config_json,
        )
    }
    .expect("actor adapter construction succeeds");
    register_actor_adapter(&mut adapter);

    let custom = plugin_test_custom_data(7.75);
    DataActor::on_historical_data(&mut adapter, &custom).expect("on_historical_data dispatches");
    let contents =
        fs::read_to_string(&marker).expect("plug-in actor writes historical custom data marker");
    let _ = fs::remove_file(marker);

    assert_eq!(contents, "7.75");
}

fn cdylib_strategy_custom_data_dispatches_to_on_data(example_manifest: &'static PluginManifest) {
    let manifest = ValidatedPluginManifest::new(example_manifest)
        .expect("live example manifest passes validation");
    let entry = manifest
        .strategies()
        .next()
        .expect("example strategy entry");
    let strategy_id = StrategyId::from("ExampleStrategyCustomData-001");
    let marker = std::env::temp_dir().join(format!(
        "nautilus-plugin-strategy-custom-data-{}.txt",
        UUID4::new()
    ));
    let _ = fs::remove_file(&marker);
    let config_json = serde_json::json!({
        "callback_path": marker.display().to_string(),
    })
    .to_string();
    let config = StrategyConfig::builder()
        .strategy_id(strategy_id)
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
            &config_json,
        )
    }
    .expect("strategy adapter construction succeeds");
    register_strategy_adapter(&mut adapter);

    let custom = plugin_test_custom_data(8.5);
    DataActor::on_data(&mut adapter, &custom).expect("on_data dispatches");
    let contents = fs::read_to_string(&marker).expect("plug-in strategy writes custom data marker");
    let _ = fs::remove_file(marker);

    assert_eq!(contents, "8.5");
}

fn cdylib_strategy_historical_custom_data_dispatches_to_on_data(
    example_manifest: &'static PluginManifest,
) {
    let manifest = ValidatedPluginManifest::new(example_manifest)
        .expect("live example manifest passes validation");
    let entry = manifest
        .strategies()
        .next()
        .expect("example strategy entry");
    let strategy_id = StrategyId::from("ExampleStrategyHistoricalCustomData-001");
    let marker = std::env::temp_dir().join(format!(
        "nautilus-plugin-strategy-historical-custom-data-{}.txt",
        UUID4::new()
    ));
    let _ = fs::remove_file(&marker);
    let config_json = serde_json::json!({
        "callback_path": marker.display().to_string(),
    })
    .to_string();
    let config = StrategyConfig::builder()
        .strategy_id(strategy_id)
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
            &config_json,
        )
    }
    .expect("strategy adapter construction succeeds");
    register_strategy_adapter(&mut adapter);

    let custom = plugin_test_custom_data(8.75);
    DataActor::on_historical_data(&mut adapter, &custom).expect("on_historical_data dispatches");
    let contents =
        fs::read_to_string(&marker).expect("plug-in strategy writes historical custom data marker");
    let _ = fs::remove_file(marker);

    assert_eq!(contents, "8.75");
}

#[rstest]
fn cdylib_strategy_execution_normalizes_identifiers_for_plugin() {
    cdylib_strategy_submit_order_normalizes_identifiers();
    cdylib_strategy_query_order_normalizes_identifiers_for_cache_lookup();
    cdylib_strategy_cancel_order_normalizes_identifiers_for_cache_lookup();
    cdylib_strategy_modify_order_normalizes_identifiers_for_cache_lookup();
    cdylib_strategy_close_position_normalizes_identifiers_for_cache_lookup();
    cdylib_strategy_submit_order_list_normalizes_identifiers_and_routes_command_fields();
    cdylib_strategy_cancel_orders_normalizes_identifiers_for_cache_lookup();
    cdylib_strategy_cancel_orders_normalizes_identifiers_and_surfaces_missing_cache_error();
    cdylib_strategy_cancel_all_orders_normalizes_identifiers_and_routes_command_fields();
    cdylib_strategy_close_all_positions_normalizes_identifiers_for_position_lookup();
    cdylib_strategy_query_account_normalizes_identifiers_and_routes_command_fields();
    cdylib_strategy_quote_normalizes_identifiers_for_plugin();
    cdylib_strategy_book_normalizes_identifiers_for_plugin();
}

fn cdylib_strategy_submit_order_normalizes_identifiers() {
    let manifest = ValidatedPluginManifest::new(loaded_exec_manifest())
        .expect("exec test manifest passes validation");
    let entry = manifest.strategies().next().expect("exec strategy entry");

    let strategy_id = StrategyId::from("PluginExecCdylib-001");
    let client_order_id = ClientOrderId::from("O-CDYLIB-001");
    let position_id = PositionId::from("P-CDYLIB-001");
    let config_json = serde_json::json!({
        "strategy_id": strategy_id.to_string(),
        "client_order_id": client_order_id.to_string(),
        "position_id": position_id.to_string(),
    })
    .to_string();
    let config = StrategyConfig::builder()
        .strategy_id(strategy_id)
        .order_id_tag("001".to_string())
        .build();

    // SAFETY: host_vtable() is process-lifetime static.
    let mut adapter = unsafe {
        PluginStrategyAdapter::new(
            config,
            "exec-test-plugin",
            entry.type_name(),
            entry.vtable(),
            host_vtable(),
            &config_json,
        )
    }
    .expect("strategy adapter construction succeeds");
    register_strategy_adapter(&mut adapter);

    let captured = std::sync::Arc::new(std::sync::Mutex::new(None));
    let captured_clone = std::sync::Arc::clone(&captured);
    let handler_id = format!("PluginCdylibRiskProbe.{}", UUID4::new());
    let risk_handler =
        TypedIntoHandler::from_with_id(&handler_id, move |command: TradingCommand| {
            *captured_clone.lock().unwrap() = Some(command);
        });
    msgbus::register_trading_command_endpoint(
        MessagingSwitchboard::risk_engine_queue_execute(),
        risk_handler,
    );

    let actor_id = ActorId::from(adapter.actor_id().inner().as_str());
    let registered = register_actor(adapter);
    // SAFETY: `registered` owns the adapter and this test holds the only
    // mutable access while invoking on_start.
    unsafe { DataActor::on_start(&mut *registered.get()) }.expect("on_start dispatches");

    let captured = captured.lock().unwrap().take().expect("command captured");
    match captured {
        TradingCommand::SubmitOrder(command) => {
            assert_eq!(command.client_order_id, client_order_id);
            assert_eq!(command.position_id, Some(position_id));
        }
        other => panic!("expected SubmitOrder, was {other:?}"),
    }
    assert_eq!(actor_id, ActorId::from("PluginExecCdylib-001"));
}

fn cdylib_strategy_query_order_normalizes_identifiers_for_cache_lookup() {
    let manifest = ValidatedPluginManifest::new(loaded_exec_manifest())
        .expect("exec test manifest passes validation");
    let entry = manifest.strategies().next().expect("exec strategy entry");

    let strategy_id = StrategyId::from("PluginQueryCdylib-001");
    let client_order_id = ClientOrderId::from("O-CDYLIB-QUERY-001");
    let config_json = serde_json::json!({
        "action": "query_order",
        "strategy_id": strategy_id.to_string(),
        "client_order_id": client_order_id.to_string(),
    })
    .to_string();
    let config = StrategyConfig::builder()
        .strategy_id(strategy_id)
        .order_id_tag("001".to_string())
        .build();

    // SAFETY: host_vtable() is process-lifetime static.
    let mut adapter = unsafe {
        PluginStrategyAdapter::new(
            config,
            "exec-test-plugin",
            entry.type_name(),
            entry.vtable(),
            host_vtable(),
            &config_json,
        )
    }
    .expect("strategy adapter construction succeeds");
    register_strategy_adapter(&mut adapter);

    let cache_rc = adapter.core_mut().cache_rc();
    cache_rc
        .borrow_mut()
        .add_order(
            plugin_test_order(strategy_id, client_order_id),
            None,
            None,
            true,
        )
        .expect("seed query order cache");

    let captured = std::sync::Arc::new(std::sync::Mutex::new(None));
    let captured_clone = std::sync::Arc::clone(&captured);
    let handler_id = format!("PluginCdylibQueryExecProbe.{}", UUID4::new());
    let exec_handler =
        TypedIntoHandler::from_with_id(&handler_id, move |command: TradingCommand| {
            *captured_clone.lock().unwrap() = Some(command);
        });
    msgbus::register_trading_command_endpoint(
        MessagingSwitchboard::exec_engine_queue_execute(),
        exec_handler,
    );

    let registered = register_actor(adapter);
    // SAFETY: `registered` owns the adapter and this test holds the only
    // mutable access while invoking on_start.
    unsafe { DataActor::on_start(&mut *registered.get()) }.expect("on_start dispatches");

    let captured = captured.lock().unwrap().take().expect("command captured");
    match captured {
        TradingCommand::QueryOrder(command) => {
            assert_eq!(command.client_order_id, client_order_id);
            assert_eq!(command.instrument_id, plugin_test_instrument_id());
        }
        other => panic!("expected QueryOrder, was {other:?}"),
    }
}

fn cdylib_strategy_cancel_order_normalizes_identifiers_for_cache_lookup() {
    let manifest = ValidatedPluginManifest::new(loaded_exec_manifest())
        .expect("exec test manifest passes validation");
    let entry = manifest.strategies().next().expect("exec strategy entry");

    let strategy_id = StrategyId::from("PluginCancelCdylib-001");
    let client_order_id = ClientOrderId::from("O-CDYLIB-CANCEL-001");
    let config_json = serde_json::json!({
        "action": "cancel_order",
        "strategy_id": strategy_id.to_string(),
        "client_order_id": client_order_id.to_string(),
    })
    .to_string();
    let config = StrategyConfig::builder()
        .strategy_id(strategy_id)
        .order_id_tag("001".to_string())
        .build();

    // SAFETY: host_vtable() is process-lifetime static.
    let mut adapter = unsafe {
        PluginStrategyAdapter::new(
            config,
            "exec-test-plugin",
            entry.type_name(),
            entry.vtable(),
            host_vtable(),
            &config_json,
        )
    }
    .expect("strategy adapter construction succeeds");
    register_strategy_adapter(&mut adapter);

    let cache_rc = adapter.core_mut().cache_rc();
    cache_rc
        .borrow_mut()
        .add_order(
            plugin_test_accepted_order(strategy_id, client_order_id),
            None,
            None,
            true,
        )
        .expect("seed cancel order cache");

    let captured = std::sync::Arc::new(std::sync::Mutex::new(None));
    let captured_clone = std::sync::Arc::clone(&captured);
    let handler_id = format!("PluginCdylibCancelExecProbe.{}", UUID4::new());
    let exec_handler =
        TypedIntoHandler::from_with_id(&handler_id, move |command: TradingCommand| {
            *captured_clone.lock().unwrap() = Some(command);
        });
    msgbus::register_trading_command_endpoint(
        MessagingSwitchboard::exec_engine_queue_execute(),
        exec_handler,
    );

    let registered = register_actor(adapter);
    // SAFETY: `registered` owns the adapter and this test holds the only
    // mutable access while invoking on_start.
    unsafe { DataActor::on_start(&mut *registered.get()) }.expect("on_start dispatches");

    let captured = captured.lock().unwrap().take().expect("command captured");
    match captured {
        TradingCommand::CancelOrder(command) => {
            assert_eq!(command.client_order_id, client_order_id);
            assert_eq!(command.instrument_id, plugin_test_instrument_id());
        }
        other => panic!("expected CancelOrder, was {other:?}"),
    }
}

fn cdylib_strategy_modify_order_normalizes_identifiers_for_cache_lookup() {
    let manifest = ValidatedPluginManifest::new(loaded_exec_manifest())
        .expect("exec test manifest passes validation");
    let entry = manifest.strategies().next().expect("exec strategy entry");

    let strategy_id = StrategyId::from("PluginModifyCdylib-001");
    let client_order_id = ClientOrderId::from("O-CDYLIB-MODIFY-001");
    let config_json = serde_json::json!({
        "action": "modify_order",
        "strategy_id": strategy_id.to_string(),
        "client_order_id": client_order_id.to_string(),
    })
    .to_string();
    let config = StrategyConfig::builder()
        .strategy_id(strategy_id)
        .order_id_tag("001".to_string())
        .build();

    // SAFETY: host_vtable() is process-lifetime static.
    let mut adapter = unsafe {
        PluginStrategyAdapter::new(
            config,
            "exec-test-plugin",
            entry.type_name(),
            entry.vtable(),
            host_vtable(),
            &config_json,
        )
    }
    .expect("strategy adapter construction succeeds");
    register_strategy_adapter(&mut adapter);

    let cache_rc = adapter.core_mut().cache_rc();
    cache_rc
        .borrow_mut()
        .add_order(
            plugin_test_accepted_order(strategy_id, client_order_id),
            None,
            None,
            true,
        )
        .expect("seed modify order cache");

    let captured = std::sync::Arc::new(std::sync::Mutex::new(None));
    let captured_clone = std::sync::Arc::clone(&captured);
    let handler_id = format!("PluginCdylibModifyRiskProbe.{}", UUID4::new());
    let risk_handler =
        TypedIntoHandler::from_with_id(&handler_id, move |command: TradingCommand| {
            *captured_clone.lock().unwrap() = Some(command);
        });
    msgbus::register_trading_command_endpoint(
        MessagingSwitchboard::risk_engine_queue_execute(),
        risk_handler,
    );

    let registered = register_actor(adapter);
    // SAFETY: `registered` owns the adapter and this test holds the only
    // mutable access while invoking on_start.
    unsafe { DataActor::on_start(&mut *registered.get()) }.expect("on_start dispatches");

    let captured = captured.lock().unwrap().take().expect("command captured");
    match captured {
        TradingCommand::ModifyOrder(command) => {
            assert_eq!(command.client_order_id, client_order_id);
            assert_eq!(command.instrument_id, plugin_test_instrument_id());
            assert_eq!(command.quantity, Some(Quantity::from("2.0")));
        }
        other => panic!("expected ModifyOrder, was {other:?}"),
    }
}

fn cdylib_strategy_close_position_normalizes_identifiers_for_cache_lookup() {
    let manifest = ValidatedPluginManifest::new(loaded_exec_manifest())
        .expect("exec test manifest passes validation");
    let entry = manifest.strategies().next().expect("exec strategy entry");

    let strategy_id = StrategyId::from("PluginCloseCdylib-001");
    let client_order_id = ClientOrderId::from("O-CDYLIB-CLOSE-001");
    let position_id = PositionId::from("P-CDYLIB-CLOSE-001");
    let config_json = serde_json::json!({
        "action": "close_position",
        "strategy_id": strategy_id.to_string(),
        "client_order_id": client_order_id.to_string(),
        "position_id": position_id.to_string(),
    })
    .to_string();
    let config = StrategyConfig::builder()
        .strategy_id(strategy_id)
        .order_id_tag("001".to_string())
        .build();

    // SAFETY: host_vtable() is process-lifetime static.
    let mut adapter = unsafe {
        PluginStrategyAdapter::new(
            config,
            "exec-test-plugin",
            entry.type_name(),
            entry.vtable(),
            host_vtable(),
            &config_json,
        )
    }
    .expect("strategy adapter construction succeeds");
    register_strategy_adapter(&mut adapter);

    let instrument = InstrumentAny::CurrencyPair(stubs::currency_pair_ethusdt());
    let position = plugin_test_position(strategy_id, position_id);
    let expected_instrument_id = position.instrument_id;
    let cache_rc = adapter.core_mut().cache_rc();
    cache_rc
        .borrow_mut()
        .add_instrument(instrument)
        .expect("seed close position instrument");
    cache_rc
        .borrow_mut()
        .add_position(&position, OmsType::Netting)
        .expect("seed close position cache");

    let captured = std::sync::Arc::new(std::sync::Mutex::new(None));
    let captured_clone = std::sync::Arc::clone(&captured);
    let handler_id = format!("PluginCdylibCloseRiskProbe.{}", UUID4::new());
    let risk_handler =
        TypedIntoHandler::from_with_id(&handler_id, move |command: TradingCommand| {
            *captured_clone.lock().unwrap() = Some(command);
        });
    msgbus::register_trading_command_endpoint(
        MessagingSwitchboard::risk_engine_queue_execute(),
        risk_handler,
    );

    let registered = register_actor(adapter);
    // SAFETY: `registered` owns the adapter and this test holds the only
    // mutable access while invoking on_start.
    unsafe { DataActor::on_start(&mut *registered.get()) }.expect("on_start dispatches");

    let captured = captured.lock().unwrap().take().expect("command captured");
    match captured {
        TradingCommand::SubmitOrder(command) => {
            assert_eq!(command.instrument_id, expected_instrument_id);
            assert_eq!(command.position_id, Some(position_id));
        }
        other => panic!("expected SubmitOrder, was {other:?}"),
    }
}

fn cdylib_strategy_submit_order_list_normalizes_identifiers_and_routes_command_fields() {
    let manifest = ValidatedPluginManifest::new(loaded_exec_manifest())
        .expect("exec test manifest passes validation");
    let entry = manifest.strategies().next().expect("exec strategy entry");

    let strategy_id = StrategyId::from("PluginSubmitListCdylib-001");
    let client_order_id = ClientOrderId::from("O-CDYLIB-LIST-001");
    let secondary_client_order_id = ClientOrderId::from("O-CDYLIB-LIST-002");
    let position_id = PositionId::from("P-CDYLIB-LIST-001");
    let client_id = ClientId::from("BINANCE");
    let expected_params = expected_params("cdylib-submit-order-list");
    let config_json = serde_json::json!({
        "action": "submit_order_list",
        "strategy_id": strategy_id.to_string(),
        "client_order_id": client_order_id.to_string(),
        "secondary_client_order_id": secondary_client_order_id.to_string(),
        "position_id": position_id.to_string(),
        "client_id": client_id.to_string(),
    })
    .to_string();
    let config = StrategyConfig::builder()
        .strategy_id(strategy_id)
        .order_id_tag("001".to_string())
        .build();

    // SAFETY: host_vtable() is process-lifetime static.
    let mut adapter = unsafe {
        PluginStrategyAdapter::new(
            config,
            "exec-test-plugin",
            entry.type_name(),
            entry.vtable(),
            host_vtable(),
            &config_json,
        )
    }
    .expect("strategy adapter construction succeeds");
    register_strategy_adapter(&mut adapter);

    let captured = std::sync::Arc::new(std::sync::Mutex::new(None));
    let captured_clone = std::sync::Arc::clone(&captured);
    let handler_id = format!("PluginCdylibSubmitListRiskProbe.{}", UUID4::new());
    let risk_handler =
        TypedIntoHandler::from_with_id(&handler_id, move |command: TradingCommand| {
            *captured_clone.lock().unwrap() = Some(command);
        });
    msgbus::register_trading_command_endpoint(
        MessagingSwitchboard::risk_engine_queue_execute(),
        risk_handler,
    );

    let registered = register_actor(adapter);
    // SAFETY: `registered` owns the adapter and this test holds the only
    // mutable access while invoking on_start.
    unsafe { DataActor::on_start(&mut *registered.get()) }.expect("on_start dispatches");

    let captured = captured.lock().unwrap().take().expect("command captured");
    match captured {
        TradingCommand::SubmitOrderList(command) => {
            assert_eq!(command.strategy_id, strategy_id);
            assert_eq!(command.client_id, Some(client_id));
            assert_eq!(command.position_id, Some(position_id));
            assert_eq!(command.params, Some(expected_params));
            assert_eq!(command.instrument_id, plugin_test_instrument_id());
            assert_eq!(
                command.order_list.client_order_ids,
                vec![client_order_id, secondary_client_order_id]
            );
            assert_eq!(command.order_inits.len(), 2);
            assert_eq!(command.order_inits[0].client_order_id, client_order_id);
            assert_eq!(
                command.order_inits[1].client_order_id,
                secondary_client_order_id
            );
            assert_eq!(command.order_inits[0].order_side, OrderSide::Buy);
            assert_eq!(command.order_inits[1].order_side, OrderSide::Sell);
            assert_eq!(command.order_inits[0].time_in_force, TimeInForce::Gtc);
            assert_eq!(command.order_inits[1].time_in_force, TimeInForce::Gtc);
        }
        other => panic!("expected SubmitOrderList, was {other:?}"),
    }
}

fn cdylib_strategy_cancel_orders_normalizes_identifiers_for_cache_lookup() {
    let manifest = ValidatedPluginManifest::new(loaded_exec_manifest())
        .expect("exec test manifest passes validation");
    let entry = manifest.strategies().next().expect("exec strategy entry");

    let strategy_id = StrategyId::from("PluginCancelListCdylib-001");
    let client_order_id = ClientOrderId::from("O-CDYLIB-CANCEL-LIST-001");
    let secondary_client_order_id = ClientOrderId::from("O-CDYLIB-CANCEL-LIST-002");
    let client_id = ClientId::from("BINANCE");
    let expected_params = expected_params("cdylib-cancel-orders");
    let config_json = serde_json::json!({
        "action": "cancel_orders",
        "strategy_id": strategy_id.to_string(),
        "client_order_id": client_order_id.to_string(),
        "secondary_client_order_id": secondary_client_order_id.to_string(),
        "client_id": client_id.to_string(),
    })
    .to_string();
    let config = StrategyConfig::builder()
        .strategy_id(strategy_id)
        .order_id_tag("001".to_string())
        .build();

    // SAFETY: host_vtable() is process-lifetime static.
    let mut adapter = unsafe {
        PluginStrategyAdapter::new(
            config,
            "exec-test-plugin",
            entry.type_name(),
            entry.vtable(),
            host_vtable(),
            &config_json,
        )
    }
    .expect("strategy adapter construction succeeds");
    register_strategy_adapter(&mut adapter);

    let cache_rc = adapter.core_mut().cache_rc();
    for id in [client_order_id, secondary_client_order_id] {
        cache_rc
            .borrow_mut()
            .add_order(
                plugin_test_accepted_order(strategy_id, id),
                None,
                None,
                true,
            )
            .expect("seed cancel orders cache");
    }

    let captured = std::sync::Arc::new(std::sync::Mutex::new(None));
    let captured_clone = std::sync::Arc::clone(&captured);
    let handler_id = format!("PluginCdylibCancelListExecProbe.{}", UUID4::new());
    let exec_handler =
        TypedIntoHandler::from_with_id(&handler_id, move |command: TradingCommand| {
            *captured_clone.lock().unwrap() = Some(command);
        });
    msgbus::register_trading_command_endpoint(
        MessagingSwitchboard::exec_engine_queue_execute(),
        exec_handler,
    );

    let registered = register_actor(adapter);
    // SAFETY: `registered` owns the adapter and this test holds the only
    // mutable access while invoking on_start.
    unsafe { DataActor::on_start(&mut *registered.get()) }.expect("on_start dispatches");

    let captured = captured.lock().unwrap().take().expect("command captured");
    match captured {
        TradingCommand::CancelOrders(command) => {
            assert_eq!(command.strategy_id, strategy_id);
            assert_eq!(command.client_id, Some(client_id));
            assert_eq!(command.instrument_id, plugin_test_instrument_id());
            assert_eq!(command.params, Some(expected_params.clone()));
            assert_eq!(command.cancels.len(), 2);
            assert_eq!(command.cancels[0].client_order_id, client_order_id);
            assert_eq!(
                command.cancels[1].client_order_id,
                secondary_client_order_id
            );

            for cancel in command.cancels {
                assert_eq!(cancel.strategy_id, strategy_id);
                assert_eq!(cancel.client_id, Some(client_id));
                assert_eq!(cancel.instrument_id, plugin_test_instrument_id());
                assert_eq!(
                    cancel.venue_order_id,
                    Some(VenueOrderId::from("V-CDYLIB-001"))
                );
                assert_eq!(cancel.params, Some(expected_params.clone()));
            }
        }
        other => panic!("expected BatchCancelOrders, was {other:?}"),
    }
}

fn cdylib_strategy_cancel_orders_normalizes_identifiers_and_surfaces_missing_cache_error() {
    let manifest = ValidatedPluginManifest::new(loaded_exec_manifest())
        .expect("exec test manifest passes validation");
    let entry = manifest.strategies().next().expect("exec strategy entry");

    let strategy_id = StrategyId::from("PluginCancelMissingCdylib-001");
    let client_order_id = ClientOrderId::from("O-CDYLIB-CANCEL-MISSING-001");
    let secondary_client_order_id = ClientOrderId::from("O-CDYLIB-CANCEL-MISSING-002");
    let client_id = ClientId::from("BINANCE");
    let config_json = serde_json::json!({
        "action": "cancel_orders",
        "strategy_id": strategy_id.to_string(),
        "client_order_id": client_order_id.to_string(),
        "secondary_client_order_id": secondary_client_order_id.to_string(),
        "client_id": client_id.to_string(),
    })
    .to_string();
    let config = StrategyConfig::builder()
        .strategy_id(strategy_id)
        .order_id_tag("001".to_string())
        .build();

    // SAFETY: host_vtable() is process-lifetime static.
    let mut adapter = unsafe {
        PluginStrategyAdapter::new(
            config,
            "exec-test-plugin",
            entry.type_name(),
            entry.vtable(),
            host_vtable(),
            &config_json,
        )
    }
    .expect("strategy adapter construction succeeds");
    register_strategy_adapter(&mut adapter);

    let registered = register_actor(adapter);
    // SAFETY: `registered` owns the adapter and this test holds the only
    // mutable access while invoking on_start.
    let err = unsafe { DataActor::on_start(&mut *registered.get()) }
        .expect_err("missing cached order should surface as an error");
    let message = err.to_string();

    assert!(
        message.contains(&format!(
            "Cannot cancel order: {ORDER_NOT_FOUND}: {client_order_id}"
        )),
        "unexpected error: {message}"
    );
}

fn cdylib_strategy_cancel_all_orders_normalizes_identifiers_and_routes_command_fields() {
    let manifest = ValidatedPluginManifest::new(loaded_exec_manifest())
        .expect("exec test manifest passes validation");
    let entry = manifest.strategies().next().expect("exec strategy entry");

    let strategy_id = StrategyId::from("PluginCancelAllCdylib-001");
    let client_order_id = ClientOrderId::from("O-CDYLIB-CANCEL-ALL-001");
    let client_id = ClientId::from("BINANCE");
    let expected_params = expected_params("cdylib-cancel-all-orders");
    let config_json = serde_json::json!({
        "action": "cancel_all_orders",
        "strategy_id": strategy_id.to_string(),
        "client_order_id": client_order_id.to_string(),
        "client_id": client_id.to_string(),
    })
    .to_string();
    let config = StrategyConfig::builder()
        .strategy_id(strategy_id)
        .order_id_tag("001".to_string())
        .build();

    // SAFETY: host_vtable() is process-lifetime static.
    let mut adapter = unsafe {
        PluginStrategyAdapter::new(
            config,
            "exec-test-plugin",
            entry.type_name(),
            entry.vtable(),
            host_vtable(),
            &config_json,
        )
    }
    .expect("strategy adapter construction succeeds");
    register_strategy_adapter(&mut adapter);

    let cache_rc = adapter.core_mut().cache_rc();
    let order = plugin_test_order(strategy_id, client_order_id);
    let submitted = TestOrderEventStubs::submitted(&order, AccountId::from("SIM-001"));
    let accepted = TestOrderEventStubs::accepted(
        &order,
        AccountId::from("SIM-001"),
        VenueOrderId::from("V-CDYLIB-001"),
    );
    {
        let mut cache = cache_rc.borrow_mut();
        cache
            .add_order(order, None, None, true)
            .expect("seed cancel all orders cache");
        cache
            .update_order(&submitted)
            .expect("seed cancel all orders submitted state");
        cache
            .update_order(&accepted)
            .expect("seed cancel all orders accepted state");
    }

    let captured = std::sync::Arc::new(std::sync::Mutex::new(None));
    let captured_clone = std::sync::Arc::clone(&captured);
    let handler_id = format!("PluginCdylibCancelAllExecProbe.{}", UUID4::new());
    let exec_handler =
        TypedIntoHandler::from_with_id(&handler_id, move |command: TradingCommand| {
            *captured_clone.lock().unwrap() = Some(command);
        });
    msgbus::register_trading_command_endpoint(
        MessagingSwitchboard::exec_engine_queue_execute(),
        exec_handler,
    );

    let registered = register_actor(adapter);
    // SAFETY: `registered` owns the adapter and this test holds the only
    // mutable access while invoking on_start.
    unsafe { DataActor::on_start(&mut *registered.get()) }.expect("on_start dispatches");

    let captured = captured.lock().unwrap().take().expect("command captured");
    match captured {
        TradingCommand::CancelAllOrders(command) => {
            assert_eq!(command.strategy_id, strategy_id);
            assert_eq!(command.client_id, Some(client_id));
            assert_eq!(command.instrument_id, plugin_test_instrument_id());
            assert_eq!(command.order_side, OrderSide::Buy);
            assert_eq!(command.params, Some(expected_params));
        }
        other => panic!("expected CancelAllOrders, was {other:?}"),
    }
}

fn cdylib_strategy_close_all_positions_normalizes_identifiers_for_position_lookup() {
    let manifest = ValidatedPluginManifest::new(loaded_exec_manifest())
        .expect("exec test manifest passes validation");
    let entry = manifest.strategies().next().expect("exec strategy entry");

    let strategy_id = StrategyId::from("PluginCloseAllCdylib-001");
    let position_id = PositionId::from("P-CDYLIB-CLOSE-ALL-001");
    let client_id = ClientId::from("BINANCE");
    let instrument = InstrumentAny::CurrencyPair(stubs::currency_pair_ethusdt());
    let expected_instrument_id = instrument.id();
    let config_json = serde_json::json!({
        "action": "close_all_positions",
        "strategy_id": strategy_id.to_string(),
        "position_id": position_id.to_string(),
        "instrument_id": expected_instrument_id.to_string(),
        "client_id": client_id.to_string(),
    })
    .to_string();
    let config = StrategyConfig::builder()
        .strategy_id(strategy_id)
        .order_id_tag("001".to_string())
        .build();

    // SAFETY: host_vtable() is process-lifetime static.
    let mut adapter = unsafe {
        PluginStrategyAdapter::new(
            config,
            "exec-test-plugin",
            entry.type_name(),
            entry.vtable(),
            host_vtable(),
            &config_json,
        )
    }
    .expect("strategy adapter construction succeeds");
    register_strategy_adapter(&mut adapter);

    let position = plugin_test_position(strategy_id, position_id);
    assert_eq!(position.instrument_id, expected_instrument_id);
    let cache_rc = adapter.core_mut().cache_rc();
    cache_rc
        .borrow_mut()
        .add_instrument(instrument)
        .expect("seed close all positions instrument");
    cache_rc
        .borrow_mut()
        .add_position(&position, OmsType::Netting)
        .expect("seed close all positions cache");

    let captured = std::sync::Arc::new(std::sync::Mutex::new(None));
    let captured_clone = std::sync::Arc::clone(&captured);
    let handler_id = format!("PluginCdylibCloseAllRiskProbe.{}", UUID4::new());
    let risk_handler =
        TypedIntoHandler::from_with_id(&handler_id, move |command: TradingCommand| {
            *captured_clone.lock().unwrap() = Some(command);
        });
    msgbus::register_trading_command_endpoint(
        MessagingSwitchboard::risk_engine_queue_execute(),
        risk_handler,
    );

    let registered = register_actor(adapter);
    // SAFETY: `registered` owns the adapter and this test holds the only
    // mutable access while invoking on_start.
    unsafe { DataActor::on_start(&mut *registered.get()) }.expect("on_start dispatches");

    let captured = captured.lock().unwrap().take().expect("command captured");
    match captured {
        TradingCommand::SubmitOrder(command) => {
            assert_eq!(command.strategy_id, strategy_id);
            assert_eq!(command.client_id, Some(client_id));
            assert_eq!(command.instrument_id, expected_instrument_id);
            assert_eq!(command.position_id, Some(position_id));
            assert_eq!(command.params, None);
            assert_eq!(command.order_init.order_side, OrderSide::Sell);
            assert_eq!(command.order_init.time_in_force, TimeInForce::Ioc);
            assert!(command.order_init.reduce_only);
            assert!(!command.order_init.quote_quantity);
            assert_eq!(
                command.order_init.tags,
                Some(vec![ustr::Ustr::from("cdylib-flatten")])
            );
        }
        other => panic!("expected SubmitOrder, was {other:?}"),
    }
}

fn cdylib_strategy_query_account_normalizes_identifiers_and_routes_command_fields() {
    let manifest = ValidatedPluginManifest::new(loaded_exec_manifest())
        .expect("exec test manifest passes validation");
    let entry = manifest.strategies().next().expect("exec strategy entry");

    let strategy_id = StrategyId::from("PluginQueryAccountCdylib-001");
    let account_id = AccountId::from("BINANCE-001");
    let client_id = ClientId::from("BINANCE");
    let expected_params = expected_params("cdylib-query-account");
    let config_json = serde_json::json!({
        "action": "query_account",
        "strategy_id": strategy_id.to_string(),
        "account_id": account_id.to_string(),
        "client_id": client_id.to_string(),
    })
    .to_string();
    let config = StrategyConfig::builder()
        .strategy_id(strategy_id)
        .order_id_tag("001".to_string())
        .build();

    // SAFETY: host_vtable() is process-lifetime static.
    let mut adapter = unsafe {
        PluginStrategyAdapter::new(
            config,
            "exec-test-plugin",
            entry.type_name(),
            entry.vtable(),
            host_vtable(),
            &config_json,
        )
    }
    .expect("strategy adapter construction succeeds");
    register_strategy_adapter(&mut adapter);

    let captured = std::sync::Arc::new(std::sync::Mutex::new(None));
    let captured_clone = std::sync::Arc::clone(&captured);
    let handler_id = format!("PluginCdylibQueryAccountExecProbe.{}", UUID4::new());
    let exec_handler =
        TypedIntoHandler::from_with_id(&handler_id, move |command: TradingCommand| {
            *captured_clone.lock().unwrap() = Some(command);
        });
    msgbus::register_trading_command_endpoint(
        MessagingSwitchboard::exec_engine_queue_execute(),
        exec_handler,
    );

    let registered = register_actor(adapter);
    // SAFETY: `registered` owns the adapter and this test holds the only
    // mutable access while invoking on_start.
    unsafe { DataActor::on_start(&mut *registered.get()) }.expect("on_start dispatches");

    let captured = captured.lock().unwrap().take().expect("command captured");
    match captured {
        TradingCommand::QueryAccount(command) => {
            assert_eq!(command.account_id, account_id);
            assert_eq!(command.client_id, Some(client_id));
            assert_eq!(command.params, Some(expected_params));
        }
        other => panic!("expected QueryAccount, was {other:?}"),
    }
}

fn expected_params(marker: &str) -> Params {
    let mut params = Params::new();
    params.insert(
        "marker".to_string(),
        serde_json::Value::String(marker.to_string()),
    );
    params
}

#[rstest]
fn cdylib_actor_events_normalizes_identifiers_for_plugin() {
    cdylib_actor_quote_normalizes_identifiers_for_plugin();
    cdylib_actor_book_deltas_handle_normalizes_identifiers_for_plugin();
    cdylib_actor_book_handle_normalizes_identifiers_for_plugin();
    cdylib_actor_instrument_handle_normalizes_identifiers_for_plugin();
    cdylib_actor_option_chain_handle_normalizes_identifiers_for_plugin();
}

fn cdylib_actor_quote_normalizes_identifiers_for_plugin() {
    let manifest = ValidatedPluginManifest::new(loaded_actor_event_manifest())
        .expect("actor event manifest passes validation");
    let entry = manifest.actors().next().expect("actor event entry");
    let marker = std::env::temp_dir().join(format!("nautilus-plugin-event-{}.txt", UUID4::new()));
    let _ = fs::remove_file(&marker);
    let config_json = serde_json::json!({
        "instrument_id": plugin_test_instrument_id().to_string(),
        "callback_path": marker.display().to_string(),
    })
    .to_string();

    // SAFETY: host_vtable() is process-lifetime static.
    let mut adapter = unsafe {
        PluginActorAdapter::new(
            ActorId::from("ActorEventProbe-001"),
            "actor-event-plugin",
            entry.type_name(),
            entry.vtable(),
            host_vtable(),
            &config_json,
        )
    }
    .expect("actor adapter construction succeeds");
    register_actor_adapter(&mut adapter);
    let actor_id = ActorId::from(adapter.actor_id().inner().as_str());
    let _registered = register_actor(adapter);
    let ctx = nautilus_live::plugin::registry::leak_host_context(HostContextInner {
        actor_id,
        is_strategy: false,
    });
    let instrument = plugin_test_instrument_id().to_string();
    let p = host_vtable();
    // SAFETY: pointer is to a static OnceLock-backed HostVTable.
    let v = unsafe { &*p };

    // SAFETY: ctx and borrowed strings are live for the call.
    unsafe {
        (v.subscribe_quotes)(
            ctx,
            nautilus_plugin::BorrowedStr::from_str(&instrument),
            nautilus_plugin::BorrowedStr::empty(),
            nautilus_plugin::BorrowedStr::empty(),
        )
    }
    .into_result()
    .expect("subscribe_quotes succeeds");

    msgbus::publish_quote(
        get_quotes_topic(plugin_test_instrument_id()),
        &plugin_test_quote(),
    );
    let contents = fs::read_to_string(&marker).expect("plug-in actor writes quote marker");

    // SAFETY: ctx and borrowed strings are live for the call.
    unsafe {
        (v.unsubscribe_quotes)(
            ctx,
            nautilus_plugin::BorrowedStr::from_str(&instrument),
            nautilus_plugin::BorrowedStr::empty(),
            nautilus_plugin::BorrowedStr::empty(),
        )
    }
    .into_result()
    .expect("unsubscribe_quotes succeeds");
    // SAFETY: ctx originated from leak_host_context above.
    unsafe { nautilus_live::plugin::registry::drop_host_context(ctx) };
    let _ = fs::remove_file(marker);

    assert_eq!(contents, instrument);
}

fn cdylib_actor_book_deltas_handle_normalizes_identifiers_for_plugin() {
    let manifest = ValidatedPluginManifest::new(loaded_actor_event_manifest())
        .expect("actor event manifest passes validation");
    let entry = manifest.actors().next().expect("actor event entry");
    let deltas = stub_deltas();
    let marker = std::env::temp_dir().join(format!(
        "nautilus-plugin-book-deltas-event-{}.txt",
        UUID4::new()
    ));
    let _ = fs::remove_file(&marker);
    let config_json = serde_json::json!({
        "instrument_id": deltas.instrument_id.to_string(),
        "callback_path": marker.display().to_string(),
    })
    .to_string();

    // SAFETY: host_vtable() is process-lifetime static.
    let mut adapter = unsafe {
        PluginActorAdapter::new(
            ActorId::from("ActorEventProbe-BookDeltas"),
            "actor-event-plugin",
            entry.type_name(),
            entry.vtable(),
            host_vtable(),
            &config_json,
        )
    }
    .expect("actor adapter construction succeeds");
    register_actor_adapter(&mut adapter);

    DataActor::on_book_deltas(&mut adapter, &deltas).expect("on_book_deltas dispatches");
    let contents = fs::read_to_string(&marker).expect("plug-in actor writes deltas marker");
    let _ = fs::remove_file(marker);

    assert_eq!(contents, deltas.instrument_id.to_string());
}

fn cdylib_actor_book_handle_normalizes_identifiers_for_plugin() {
    let manifest = ValidatedPluginManifest::new(loaded_actor_event_manifest())
        .expect("actor event manifest passes validation");
    let entry = manifest.actors().next().expect("actor event entry");
    let book = plugin_test_order_book();
    let marker =
        std::env::temp_dir().join(format!("nautilus-plugin-book-event-{}.txt", UUID4::new()));
    let _ = fs::remove_file(&marker);
    let config_json = serde_json::json!({
        "instrument_id": book.instrument_id.to_string(),
        "callback_path": marker.display().to_string(),
    })
    .to_string();

    // SAFETY: host_vtable() is process-lifetime static.
    let mut adapter = unsafe {
        PluginActorAdapter::new(
            ActorId::from("ActorEventProbe-Book"),
            "actor-event-plugin",
            entry.type_name(),
            entry.vtable(),
            host_vtable(),
            &config_json,
        )
    }
    .expect("actor adapter construction succeeds");
    register_actor_adapter(&mut adapter);

    DataActor::on_book(&mut adapter, &book).expect("on_book dispatches");
    let contents = fs::read_to_string(&marker).expect("plug-in actor writes book marker");
    let _ = fs::remove_file(marker);

    assert_eq!(contents, book.instrument_id.to_string());
}

fn cdylib_actor_instrument_handle_normalizes_identifiers_for_plugin() {
    let manifest = ValidatedPluginManifest::new(loaded_actor_event_manifest())
        .expect("actor event manifest passes validation");
    let entry = manifest.actors().next().expect("actor event entry");
    let instrument_id = InstrumentId::from("ETHUSDT.BINANCE");
    let marker = std::env::temp_dir().join(format!(
        "nautilus-plugin-instrument-event-{}.txt",
        UUID4::new()
    ));
    let _ = fs::remove_file(&marker);
    let config_json = serde_json::json!({
        "instrument_id": instrument_id.to_string(),
        "callback_path": marker.display().to_string(),
    })
    .to_string();

    // SAFETY: host_vtable() is process-lifetime static.
    let mut adapter = unsafe {
        PluginActorAdapter::new(
            ActorId::from("ActorEventProbe-002"),
            "actor-event-plugin",
            entry.type_name(),
            entry.vtable(),
            host_vtable(),
            &config_json,
        )
    }
    .expect("actor adapter construction succeeds");
    register_actor_adapter(&mut adapter);

    let instrument = InstrumentAny::CurrencyPair(stubs::currency_pair_ethusdt());
    DataActor::on_instrument(&mut adapter, &instrument).expect("on_instrument dispatches");
    let contents = fs::read_to_string(&marker).expect("plug-in actor writes instrument marker");
    let _ = fs::remove_file(marker);

    assert_eq!(contents, instrument_id.to_string());
}

fn cdylib_actor_option_chain_handle_normalizes_identifiers_for_plugin() {
    let manifest = ValidatedPluginManifest::new(loaded_actor_event_manifest())
        .expect("actor event manifest passes validation");
    let entry = manifest.actors().next().expect("actor event entry");
    let series_id = plugin_test_option_series_id();
    let chain = plugin_test_option_chain();
    let marker = std::env::temp_dir().join(format!(
        "nautilus-plugin-option-chain-event-{}.txt",
        UUID4::new()
    ));
    let _ = fs::remove_file(&marker);
    let config_json = serde_json::json!({
        "series_id": series_id.to_wire_string(),
        "callback_path": marker.display().to_string(),
    })
    .to_string();

    // SAFETY: host_vtable() is process-lifetime static.
    let mut adapter = unsafe {
        PluginActorAdapter::new(
            ActorId::from("ActorEventProbe-OptionChain"),
            "actor-event-plugin",
            entry.type_name(),
            entry.vtable(),
            host_vtable(),
            &config_json,
        )
    }
    .expect("actor adapter construction succeeds");
    register_actor_adapter(&mut adapter);

    DataActor::on_option_chain(&mut adapter, &chain).expect("on_option_chain dispatches");
    let contents = fs::read_to_string(&marker).expect("plug-in actor writes option chain marker");
    let _ = fs::remove_file(marker);

    assert_eq!(contents, series_id.to_wire_string());
}

fn cdylib_strategy_quote_normalizes_identifiers_for_plugin() {
    let manifest = ValidatedPluginManifest::new(loaded_exec_manifest())
        .expect("exec test manifest passes validation");
    let entry = manifest.strategies().next().expect("exec strategy entry");

    let strategy_id = StrategyId::from("PluginQuoteCdylib-001");
    let marker = std::env::temp_dir().join(format!(
        "nautilus-plugin-strategy-quote-event-{}.txt",
        UUID4::new()
    ));
    let _ = fs::remove_file(&marker);
    let config_json = serde_json::json!({
        "strategy_id": strategy_id.to_string(),
        "instrument_id": plugin_test_instrument_id().to_string(),
        "callback_path": marker.display().to_string(),
    })
    .to_string();
    let config = StrategyConfig::builder()
        .strategy_id(strategy_id)
        .order_id_tag("001".to_string())
        .build();

    // SAFETY: host_vtable() is process-lifetime static.
    let mut adapter = unsafe {
        PluginStrategyAdapter::new(
            config,
            "exec-test-plugin",
            entry.type_name(),
            entry.vtable(),
            host_vtable(),
            &config_json,
        )
    }
    .expect("strategy adapter construction succeeds");
    register_strategy_adapter(&mut adapter);

    DataActor::on_quote(&mut adapter, &plugin_test_quote()).expect("on_quote dispatches");
    let contents = fs::read_to_string(&marker).expect("plug-in strategy writes quote marker");
    let _ = fs::remove_file(marker);

    assert_eq!(contents, plugin_test_instrument_id().to_string());
}

fn cdylib_strategy_book_normalizes_identifiers_for_plugin() {
    let manifest = ValidatedPluginManifest::new(loaded_exec_manifest())
        .expect("exec test manifest passes validation");
    let entry = manifest.strategies().next().expect("exec strategy entry");

    let strategy_id = StrategyId::from("PluginBookCdylib-001");
    let marker = std::env::temp_dir().join(format!(
        "nautilus-plugin-strategy-book-event-{}.txt",
        UUID4::new()
    ));
    let _ = fs::remove_file(&marker);
    let config_json = serde_json::json!({
        "strategy_id": strategy_id.to_string(),
        "instrument_id": plugin_test_instrument_id().to_string(),
        "callback_path": marker.display().to_string(),
    })
    .to_string();
    let config = StrategyConfig::builder()
        .strategy_id(strategy_id)
        .order_id_tag("001".to_string())
        .build();

    // SAFETY: host_vtable() is process-lifetime static.
    let mut adapter = unsafe {
        PluginStrategyAdapter::new(
            config,
            "exec-test-plugin",
            entry.type_name(),
            entry.vtable(),
            host_vtable(),
            &config_json,
        )
    }
    .expect("strategy adapter construction succeeds");
    register_strategy_adapter(&mut adapter);

    let book = plugin_test_order_book();
    DataActor::on_book(&mut adapter, &book).expect("on_book dispatches");
    let contents = fs::read_to_string(&marker).expect("plug-in strategy writes book marker");
    let _ = fs::remove_file(marker);

    assert_eq!(contents, plugin_test_instrument_id().to_string());
}

#[rstest]
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
fn live_node_add_plugin_registers_actor() {
    let path = build_example_once();
    let mut node = LiveNode::build(
        "PluginAddNode".to_string(),
        Some(LiveNodeConfig {
            exec_engine: LiveExecEngineConfig {
                reconciliation: false,
                ..Default::default()
            },
            ..Default::default()
        }),
    )
    .unwrap();

    node.add_plugin(PluginConfig {
        path: path.display().to_string(),
        type_name: "ExampleActor".to_string(),
        config: HashMap::from([(
            "actor_id".to_string(),
            serde_json::json!("ExampleActorAdd-001"),
        )]),
        sha256: None,
    })
    .unwrap();

    let trader = node.kernel().trader.borrow();
    assert!(
        trader
            .actor_ids()
            .contains(&ActorId::from("ExampleActorAdd-001"))
    );
}

#[rstest]
fn live_node_add_plugin_rejects_sha256_mismatch() {
    let path = build_example_once();
    let mut node = LiveNode::build(
        "PluginAddShaNode".to_string(),
        Some(LiveNodeConfig {
            exec_engine: LiveExecEngineConfig {
                reconciliation: false,
                ..Default::default()
            },
            ..Default::default()
        }),
    )
    .unwrap();

    let error = node
        .add_plugin(PluginConfig {
            path: path.display().to_string(),
            type_name: "ExampleActor".to_string(),
            config: HashMap::from([(
                "actor_id".to_string(),
                serde_json::json!("ExampleActorShaMismatch-001"),
            )]),
            sha256: Some("0".repeat(64)),
        })
        .unwrap_err()
        .to_string();

    assert!(error.contains("SHA-256 mismatch"));
    let trader = node.kernel().trader.borrow();
    assert!(
        !trader
            .actor_ids()
            .contains(&ActorId::from("ExampleActorShaMismatch-001"))
    );
}

#[tokio::test(flavor = "current_thread")]
async fn live_node_add_plugin_rejects_after_start() {
    let mut node = LiveNode::build(
        "PluginAddRunningNode".to_string(),
        Some(LiveNodeConfig {
            delay_post_stop: Duration::ZERO,
            exec_engine: LiveExecEngineConfig {
                reconciliation: false,
                ..Default::default()
            },
            ..Default::default()
        }),
    )
    .unwrap();

    node.start().await.unwrap();
    let error = node
        .add_plugin(PluginConfig {
            path: "./libexample.so".to_string(),
            type_name: "ExampleActor".to_string(),
            ..Default::default()
        })
        .unwrap_err()
        .to_string();

    if node.is_running() {
        node.stop().await.unwrap();
    }

    assert!(error.contains("Cannot add plug-in after the node leaves Idle state"));
}

#[rstest]
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

#[tokio::test(flavor = "current_thread")]
async fn live_node_start_and_stop_invokes_configured_plugin_strategy() {
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
            type_name: "RuntimeSmokeStrategy".to_string(),
            config: HashMap::from([
                (
                    "strategy_id".to_string(),
                    serde_json::json!("RuntimeSmokeStrategy-001"),
                ),
                (
                    "callback_path".to_string(),
                    serde_json::json!(marker.display().to_string()),
                ),
                ("label".to_string(), serde_json::json!("rust-strategy")),
            ]),
            sha256: None,
        }],
        ..Default::default()
    };

    let mut node = LiveNode::build("PluginRuntimeStrategyNode".to_string(), Some(config)).unwrap();
    let strategy_registered = {
        let trader = node.kernel().trader.borrow();
        trader
            .strategy_ids()
            .contains(&StrategyId::from("RuntimeSmokeStrategy-001"))
    };
    assert!(strategy_registered);

    node.start().await.unwrap();
    node.stop().await.unwrap();
    let contents = fs::read_to_string(&marker).expect("plug-in strategy writes callback marker");
    let _ = fs::remove_file(marker);

    assert_eq!(contents, "rust-strategy:on_start\nrust-strategy:on_stop\n");
}

#[tokio::test(flavor = "current_thread")]
async fn live_node_start_and_stop_invokes_configured_plugin_controller() {
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
            type_name: "RuntimeSmokeController".to_string(),
            config: HashMap::from([
                (
                    "callback_path".to_string(),
                    serde_json::json!(marker.display().to_string()),
                ),
                ("label".to_string(), serde_json::json!("rust-controller")),
            ]),
            sha256: None,
        }],
        ..Default::default()
    };

    let mut node = LiveNode::build("PluginRuntimeControllerNode".to_string(), Some(config))
        .expect("configured controller loads");
    {
        let trader = node.kernel().trader.borrow();
        assert!(trader.actor_ids().is_empty());
        assert!(trader.strategy_ids().is_empty());
    }

    node.start().await.expect("controller starts with node");
    node.stop().await.expect("controller stops with node");
    let contents = fs::read_to_string(&marker).expect("plug-in controller writes callback marker");
    let _ = fs::remove_file(marker);

    assert_eq!(
        contents,
        "rust-controller:on_start\nrust-controller:on_stop\n"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn live_node_start_rolls_back_configured_plugin_controller_failure() {
    let path = build_runtime_smoke_example_once();
    let marker = std::env::temp_dir().join(format!("nautilus-plugin-{}.txt", UUID4::new()));
    let _ = fs::remove_file(&marker);

    let mut node = LiveNode::build(
        "PluginRuntimeControllerFailureNode".to_string(),
        Some(controller_start_failure_config(&path, &marker)),
    )
    .expect("configured controllers load");

    let error = node.start().await.expect_err("controller start fails");
    let contents = fs::read_to_string(&marker).expect("plug-in controllers write callback marker");
    let _ = fs::remove_file(marker);

    assert!(format!("{error:#}").contains("configured controller start failure"));
    assert_eq!(node.state(), NodeState::Stopped);
    assert!(!node.kernel().trader.borrow().is_running());
    assert_eq!(
        contents,
        "controller-ok:on_start\ncontroller-fail:on_start\ncontroller-ok:on_stop\n"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn live_node_run_rolls_back_configured_plugin_controller_failure() {
    let path = build_runtime_smoke_example_once();
    let marker = std::env::temp_dir().join(format!("nautilus-plugin-{}.txt", UUID4::new()));
    let _ = fs::remove_file(&marker);

    let mut node = LiveNode::build(
        "PluginRuntimeControllerRunFailureNode".to_string(),
        Some(controller_start_failure_config(&path, &marker)),
    )
    .expect("configured controllers load");

    let error = node.run().await.expect_err("controller start fails");
    let contents = fs::read_to_string(&marker).expect("plug-in controllers write callback marker");
    let _ = fs::remove_file(marker);

    assert!(format!("{error:#}").contains("configured controller start failure"));
    assert_eq!(node.state(), NodeState::Stopped);
    assert!(!node.kernel().trader.borrow().is_running());
    assert_eq!(
        contents,
        "controller-ok:on_start\ncontroller-fail:on_start\ncontroller-ok:on_stop\n"
    );
}

fn controller_start_failure_config(path: &Path, marker: &Path) -> LiveNodeConfig {
    let controller_config = |label: &str, fail_on_start: bool| PluginConfig {
        path: path.display().to_string(),
        type_name: "RuntimeSmokeController".to_string(),
        config: HashMap::from([
            (
                "callback_path".to_string(),
                serde_json::json!(marker.display().to_string()),
            ),
            ("label".to_string(), serde_json::json!(label)),
            (
                "fail_on_start".to_string(),
                serde_json::json!(fail_on_start),
            ),
        ]),
        sha256: None,
    };

    LiveNodeConfig {
        delay_post_stop: Duration::ZERO,
        exec_engine: LiveExecEngineConfig {
            reconciliation: false,
            ..Default::default()
        },
        plugins: vec![
            controller_config("controller-ok", false),
            controller_config("controller-fail", true),
        ],
        ..Default::default()
    }
}
