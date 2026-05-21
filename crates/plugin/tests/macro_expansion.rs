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

//! End-to-end test of the `nautilus_plugin!` macro inside the same process.
//!
//! Exercises the manifest static, the per-type custom-data vtable, and the
//! `nautilus_plugin_init` entry symbol that the macro emits. Loading a
//! separately compiled cdylib through `PluginLoader` is covered by the
//! example crate under `crates/plugin/examples/`.

#![allow(unsafe_code)]

use std::sync::atomic::{AtomicU64, Ordering};

use nautilus_model::{data::QuoteTick, events::PositionOpened};
use nautilus_plugin::{
    NAUTILUS_PLUGIN_ABI_VERSION, PLUGIN_BUILD_ID_VERSION,
    boundary::{BorrowedStr, OwnedBytes, PluginResult, Slice},
    host::{HostContext, HostLogLevel, HostVTable},
    manifest::PluginManifest,
    surfaces::{
        actor::PluginActor,
        custom_data::{CustomDataHandle, MetadataEntry, PluginCustomData, custom_data_vtable},
        strategy::PluginStrategy,
    },
};
use rstest::rstest;

macro_rules! generated_slot {
    ($vtable:expr, $slot:ident) => {{
        ($vtable)
            .$slot
            .expect(concat!("generated vtable includes ", stringify!($slot)))
    }};
}

#[derive(Clone, Debug, PartialEq)]
struct TestTick {
    value: f64,
    ts_event: u64,
    ts_init: u64,
}

impl PluginCustomData for TestTick {
    const TYPE_NAME: &'static str = "TestTick";

    fn ts_event(&self) -> u64 {
        self.ts_event
    }

    fn ts_init(&self) -> u64 {
        self.ts_init
    }

    fn to_json(&self) -> anyhow::Result<Vec<u8>> {
        Ok(format!(
            r#"{{"value":{},"ts_event":{},"ts_init":{}}}"#,
            self.value, self.ts_event, self.ts_init
        )
        .into_bytes())
    }

    fn from_json(payload: &[u8]) -> anyhow::Result<Self> {
        let text = std::str::from_utf8(payload)?;
        let mut value = 0.0;
        let mut ts_event = 0u64;
        let mut ts_init = 0u64;

        for part in text.trim_matches(['{', '}']).split(',') {
            let mut kv = part.splitn(2, ':');
            let key = kv.next().unwrap_or("").trim_matches('"');
            let v = kv.next().unwrap_or("");
            match key {
                "value" => value = v.parse()?,
                "ts_event" => ts_event = v.parse()?,
                "ts_init" => ts_init = v.parse()?,
                _ => {}
            }
        }
        Ok(Self {
            value,
            ts_event,
            ts_init,
        })
    }

    fn schema_ipc() -> anyhow::Result<Vec<u8>> {
        // Real plug-ins would use arrow::ipc::writer::StreamWriter against
        // their schema; for this in-process test we only need a non-empty
        // sentinel byte stream.
        Ok(b"test-schema".to_vec())
    }

    fn encode_batch(items: &[&Self]) -> anyhow::Result<Vec<u8>> {
        // Sentinel encoding: number of items followed by each as JSON.
        let mut out = Vec::new();
        out.extend_from_slice(&u32::try_from(items.len()).unwrap().to_le_bytes());
        for it in items {
            let json = it.to_json()?;
            out.extend_from_slice(&u32::try_from(json.len()).unwrap().to_le_bytes());
            out.extend_from_slice(&json);
        }
        Ok(out)
    }

    fn decode_batch(ipc_bytes: &[u8], _metadata: &[(String, String)]) -> anyhow::Result<Vec<Self>> {
        let mut cursor = 0;
        let count = u32::from_le_bytes(ipc_bytes[cursor..cursor + 4].try_into()?) as usize;
        cursor += 4;
        let mut out = Vec::with_capacity(count);
        for _ in 0..count {
            let len = u32::from_le_bytes(ipc_bytes[cursor..cursor + 4].try_into()?) as usize;
            cursor += 4;
            let chunk = &ipc_bytes[cursor..cursor + len];
            cursor += len;
            out.push(Self::from_json(chunk)?);
        }
        Ok(out)
    }
}

// Test actor that exposes its internal call counts via atomics so the
// integration test can assert on dispatch without smuggling references
// across the FFI boundary.
static TEST_ACTOR_START_COUNT: AtomicU64 = AtomicU64::new(0);
static TEST_ACTOR_STOP_COUNT: AtomicU64 = AtomicU64::new(0);
static TEST_ACTOR_QUOTE_COUNT: AtomicU64 = AtomicU64::new(0);
static TEST_ACTOR_LAST_BID_RAW: AtomicU64 = AtomicU64::new(0);

#[derive(Default)]
struct TestActor;

impl PluginActor for TestActor {
    const TYPE_NAME: &'static str = "TestActor";

    fn new(_host: *const HostVTable, _ctx: *const HostContext, _config_json: &str) -> Self {
        Self
    }

    fn on_start(&mut self) -> anyhow::Result<()> {
        TEST_ACTOR_START_COUNT.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    fn on_stop(&mut self) -> anyhow::Result<()> {
        TEST_ACTOR_STOP_COUNT.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    fn on_quote(&mut self, quote: &QuoteTick) -> anyhow::Result<()> {
        TEST_ACTOR_QUOTE_COUNT.fetch_add(1, Ordering::SeqCst);
        TEST_ACTOR_LAST_BID_RAW.store(quote.bid_price.raw as u64, Ordering::SeqCst);
        Ok(())
    }
}

// Test strategy that records its host context plus lifecycle/event call
// counts via atomics. The strategy stores its `HostContext` so the
// host-binding tests can verify that the macro-generated `create` thunk
// threads the context through to the trait's `new`.
static TEST_STRATEGY_START_COUNT: AtomicU64 = AtomicU64::new(0);
static TEST_STRATEGY_POSITION_OPENED_COUNT: AtomicU64 = AtomicU64::new(0);
static TEST_STRATEGY_CONTEXT_PTR: std::sync::atomic::AtomicPtr<HostContext> =
    std::sync::atomic::AtomicPtr::new(std::ptr::null_mut());

struct TestStrategy {
    host: *const HostVTable,
    ctx: *const HostContext,
}

// SAFETY: TestStrategy holds opaque pointers the host commits to keeping
// live for the strategy's lifetime; the trait is `Send` and we only use
// the pointers to thread back through the host vtable.
unsafe impl Send for TestStrategy {}

impl PluginStrategy for TestStrategy {
    const TYPE_NAME: &'static str = "TestStrategy";

    fn new(host: *const HostVTable, ctx: *const HostContext, _config_json: &str) -> Self {
        TEST_STRATEGY_CONTEXT_PTR.store(ctx.cast_mut(), Ordering::SeqCst);
        Self { host, ctx }
    }

    fn on_start(&mut self) -> anyhow::Result<()> {
        TEST_STRATEGY_START_COUNT.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    fn on_position_opened(&mut self, _event: &PositionOpened) -> anyhow::Result<()> {
        TEST_STRATEGY_POSITION_OPENED_COUNT.fetch_add(1, Ordering::SeqCst);
        // SAFETY: the host vtable lives for the strategy's lifetime per
        // the plug-in contract.
        let host = unsafe { &*self.host };
        let cmd = BorrowedStr::from_str(r#"{"kind":"noop"}"#);
        // SAFETY: ctx is the value the host supplied at create time.
        let r = unsafe { (host.submit_order)(self.ctx, cmd) };
        r.into_result()
            .map_err(|e| anyhow::anyhow!(e.message_string()))
    }
}

// Emit the plug-in entry symbol and manifest using the macro.
nautilus_plugin::nautilus_plugin! {
    name: "macro-expansion-test",
    vendor: "Nautech",
    version: env!("CARGO_PKG_VERSION"),
    custom_data: [TestTick],
    actors: [TestActor],
    strategies: [TestStrategy],
}

unsafe extern "C" fn test_clock_now_ns() -> u64 {
    7
}

unsafe extern "C" fn test_log(
    _level: HostLogLevel,
    _target: BorrowedStr<'_>,
    _message: BorrowedStr<'_>,
) {
}

macro_rules! test_bytes_fn {
    ($name:ident, ($($arg:ident : $ty:ty),* $(,)?)) => {
        unsafe extern "C" fn $name($($arg: $ty),*) -> PluginResult<OwnedBytes> {
            $(let _ = $arg;)*
            PluginResult::Ok(OwnedBytes::empty())
        }
    };
}

macro_rules! test_unit_fn {
    ($name:ident, ($($arg:ident : $ty:ty),* $(,)?)) => {
        unsafe extern "C" fn $name($($arg: $ty),*) -> PluginResult<()> {
            $(let _ = $arg;)*
            PluginResult::Ok(())
        }
    };
}

test_bytes_fn!(test_cache_instrument, (ctx: *const HostContext, instrument_id: BorrowedStr<'_>));
test_bytes_fn!(test_cache_account, (ctx: *const HostContext, account_id: BorrowedStr<'_>));
test_bytes_fn!(test_cache_order, (ctx: *const HostContext, client_order_id: BorrowedStr<'_>));
test_bytes_fn!(test_cache_position, (ctx: *const HostContext, position_id: BorrowedStr<'_>));
test_bytes_fn!(
    test_cache_orders_for_strategy,
    (ctx: *const HostContext, strategy_id: BorrowedStr<'_>)
);
test_bytes_fn!(
    test_cache_positions_for_strategy,
    (ctx: *const HostContext, strategy_id: BorrowedStr<'_>)
);
test_unit_fn!(
    test_subscribe_quotes,
    (
        ctx: *const HostContext,
        instrument_id: BorrowedStr<'_>,
        client_id: BorrowedStr<'_>,
        params_json: BorrowedStr<'_>,
    )
);
test_unit_fn!(
    test_unsubscribe_quotes,
    (
        ctx: *const HostContext,
        instrument_id: BorrowedStr<'_>,
        client_id: BorrowedStr<'_>,
        params_json: BorrowedStr<'_>,
    )
);
test_unit_fn!(
    test_subscribe_trades,
    (
        ctx: *const HostContext,
        instrument_id: BorrowedStr<'_>,
        client_id: BorrowedStr<'_>,
        params_json: BorrowedStr<'_>,
    )
);
test_unit_fn!(
    test_unsubscribe_trades,
    (
        ctx: *const HostContext,
        instrument_id: BorrowedStr<'_>,
        client_id: BorrowedStr<'_>,
        params_json: BorrowedStr<'_>,
    )
);
test_unit_fn!(
    test_subscribe_bars,
    (
        ctx: *const HostContext,
        bar_type: BorrowedStr<'_>,
        client_id: BorrowedStr<'_>,
        params_json: BorrowedStr<'_>,
    )
);
test_unit_fn!(
    test_unsubscribe_bars,
    (
        ctx: *const HostContext,
        bar_type: BorrowedStr<'_>,
        client_id: BorrowedStr<'_>,
        params_json: BorrowedStr<'_>,
    )
);
test_unit_fn!(
    test_subscribe_book_deltas,
    (
        ctx: *const HostContext,
        instrument_id: BorrowedStr<'_>,
        book_type: u8,
        depth: usize,
        client_id: BorrowedStr<'_>,
        managed: u8,
        params_json: BorrowedStr<'_>,
    )
);
test_unit_fn!(
    test_unsubscribe_book_deltas,
    (
        ctx: *const HostContext,
        instrument_id: BorrowedStr<'_>,
        client_id: BorrowedStr<'_>,
        params_json: BorrowedStr<'_>,
    )
);
test_unit_fn!(
    test_subscribe_book_at_interval,
    (
        ctx: *const HostContext,
        instrument_id: BorrowedStr<'_>,
        book_type: u8,
        depth: usize,
        interval_ms: usize,
        client_id: BorrowedStr<'_>,
        params_json: BorrowedStr<'_>,
    )
);
test_unit_fn!(
    test_unsubscribe_book_at_interval,
    (
        ctx: *const HostContext,
        instrument_id: BorrowedStr<'_>,
        interval_ms: usize,
        client_id: BorrowedStr<'_>,
        params_json: BorrowedStr<'_>,
    )
);
test_unit_fn!(
    test_msgbus_publish,
    (
        ctx: *const HostContext,
        topic: BorrowedStr<'_>,
        payload: Slice<'_, u8>,
    )
);
test_unit_fn!(
    test_set_time_alert,
    (
        ctx: *const HostContext,
        name: BorrowedStr<'_>,
        alert_time_ns: u64,
        allow_past: u8,
    )
);
test_unit_fn!(
    test_set_timer,
    (
        ctx: *const HostContext,
        name: BorrowedStr<'_>,
        interval_ns: u64,
        start_time_ns: u64,
        stop_time_ns: u64,
        allow_past: u8,
        fire_immediately: u8,
    )
);
test_unit_fn!(test_cancel_timer, (ctx: *const HostContext, name: BorrowedStr<'_>));

static TEST_HOST_SUBMIT_COUNT: AtomicU64 = AtomicU64::new(0);
static TEST_HOST_CANCEL_COUNT: AtomicU64 = AtomicU64::new(0);
static TEST_HOST_MODIFY_COUNT: AtomicU64 = AtomicU64::new(0);
static TEST_HOST_LAST_SUBMIT_CTX: std::sync::atomic::AtomicPtr<HostContext> =
    std::sync::atomic::AtomicPtr::new(std::ptr::null_mut());

// Sentinel value the host's submit thunk reads from a static field on
// `TEST_HOST` itself and writes here. Strengthens the host-vtable
// attribution check: the assertion verifies the call reached THIS host
// vtable (via its own sentinel), not just that SOME submit_order ran.
static TEST_HOST_SUBMIT_SENTINEL: AtomicU64 = AtomicU64::new(0);
const TEST_HOST_SENTINEL_VALUE: u64 = 0xC0DE_BABE_F00D_BEEFu64;

unsafe extern "C" fn test_submit_order(
    ctx: *const HostContext,
    _command_json: BorrowedStr<'_>,
) -> PluginResult<()> {
    TEST_HOST_SUBMIT_COUNT.fetch_add(1, Ordering::SeqCst);
    TEST_HOST_LAST_SUBMIT_CTX.store(ctx.cast_mut(), Ordering::SeqCst);
    TEST_HOST_SUBMIT_SENTINEL.store(TEST_HOST_SENTINEL_VALUE, Ordering::SeqCst);
    PluginResult::Ok(())
}

unsafe extern "C" fn test_cancel_order(
    _ctx: *const HostContext,
    _command_json: BorrowedStr<'_>,
) -> PluginResult<()> {
    TEST_HOST_CANCEL_COUNT.fetch_add(1, Ordering::SeqCst);
    PluginResult::Ok(())
}

unsafe extern "C" fn test_modify_order(
    _ctx: *const HostContext,
    _command_json: BorrowedStr<'_>,
) -> PluginResult<()> {
    TEST_HOST_MODIFY_COUNT.fetch_add(1, Ordering::SeqCst);
    PluginResult::Ok(())
}

static TEST_HOST: HostVTable = HostVTable {
    abi_version: NAUTILUS_PLUGIN_ABI_VERSION,
    clock_now_ns: test_clock_now_ns,
    log: test_log,
    cache_instrument: test_cache_instrument,
    cache_account: test_cache_account,
    cache_order: test_cache_order,
    cache_position: test_cache_position,
    cache_orders_for_strategy: test_cache_orders_for_strategy,
    cache_positions_for_strategy: test_cache_positions_for_strategy,
    subscribe_quotes: test_subscribe_quotes,
    unsubscribe_quotes: test_unsubscribe_quotes,
    subscribe_trades: test_subscribe_trades,
    unsubscribe_trades: test_unsubscribe_trades,
    subscribe_bars: test_subscribe_bars,
    unsubscribe_bars: test_unsubscribe_bars,
    subscribe_book_deltas: test_subscribe_book_deltas,
    unsubscribe_book_deltas: test_unsubscribe_book_deltas,
    subscribe_book_at_interval: test_subscribe_book_at_interval,
    unsubscribe_book_at_interval: test_unsubscribe_book_at_interval,
    msgbus_publish: test_msgbus_publish,
    set_time_alert: test_set_time_alert,
    set_timer: test_set_timer,
    cancel_timer: test_cancel_timer,
    submit_order: test_submit_order,
    cancel_order: test_cancel_order,
    modify_order: test_modify_order,
};

unsafe extern "C" {
    fn nautilus_plugin_init(host: *const HostVTable) -> *const PluginManifest;
}

#[rstest]
fn macro_emits_loadable_manifest() {
    // SAFETY: the macro defined `nautilus_plugin_init` in this crate; calling
    // it with our test HostVTable mirrors what the host loader does.
    let manifest_ptr = unsafe { nautilus_plugin_init(&raw const TEST_HOST) };
    assert!(!manifest_ptr.is_null(), "init returned null");
    // SAFETY: pointer is to a static `LazyLock`-backed manifest in this crate.
    let manifest = unsafe { &*manifest_ptr };
    assert_eq!(manifest.abi_version, NAUTILUS_PLUGIN_ABI_VERSION);
    manifest
        .validate()
        .expect("macro-generated manifest passes validation");
    // SAFETY: name string lives in static storage.
    assert_eq!(
        unsafe { manifest.plugin_name.as_str() },
        "macro-expansion-test"
    );
    // SAFETY: vendor string lives in static storage.
    assert_eq!(unsafe { manifest.plugin_vendor.as_str() }, "Nautech");
    assert_eq!(manifest.build_id.schema_version, PLUGIN_BUILD_ID_VERSION);
    // SAFETY: build id strings live in static storage.
    assert_eq!(
        unsafe { manifest.build_id.nautilus_plugin_version.as_str() },
        env!("CARGO_PKG_VERSION")
    );
    // SAFETY: build id strings live in static storage.
    assert!(!unsafe { manifest.build_id.target_triple.as_str() }.is_empty());
    // SAFETY: build id strings live in static storage.
    assert!(!unsafe { manifest.build_id.build_profile.as_str() }.is_empty());

    // SAFETY: slice points at static storage owned by the manifest.
    let cd = unsafe { manifest.custom_data.as_slice() };
    assert_eq!(cd.len(), 1, "one custom-data registration expected");
    // SAFETY: type_name in the registration points at static storage.
    assert_eq!(unsafe { cd[0].type_name.as_str() }, "TestTick");

    // SAFETY: slice points at static storage owned by the manifest.
    let actors = unsafe { manifest.actors.as_slice() };
    assert_eq!(actors.len(), 1, "one actor registration expected");
    // SAFETY: type_name in the registration points at static storage.
    assert_eq!(unsafe { actors[0].type_name.as_str() }, "TestActor");

    // SAFETY: slice points at static storage owned by the manifest.
    let strategies = unsafe { manifest.strategies.as_slice() };
    assert_eq!(strategies.len(), 1, "one strategy registration expected");
    // SAFETY: type_name in the registration points at static storage.
    assert_eq!(unsafe { strategies[0].type_name.as_str() }, "TestStrategy");
}

#[rstest]
fn vtable_round_trips_a_value_through_json() {
    let manifest_ptr = unsafe { nautilus_plugin_init(&raw const TEST_HOST) };
    let manifest = unsafe { &*manifest_ptr };
    let entry = unsafe { &manifest.custom_data.as_slice()[0] };
    // SAFETY: vtable pointer is non-null and lives for the process lifetime.
    let vtable = unsafe { &*entry.vtable };

    let json_bytes = br#"{"value":1.5,"ts_event":10,"ts_init":11}"#;
    let json_str = std::str::from_utf8(json_bytes).unwrap();
    let payload = BorrowedStr::from_str(json_str);
    // SAFETY: calling through the vtable with a borrowed payload that
    // outlives the call.
    let handle_result = unsafe { generated_slot!(vtable, from_json)(payload) };
    let handle = handle_result.into_result().expect("from_json failed");
    assert!(!handle.is_null());

    // SAFETY: handle was produced by `from_json` and is still live.
    let ts_event = unsafe { generated_slot!(vtable, ts_event)(handle) };
    assert_eq!(ts_event, 10);

    // SAFETY: see above.
    let ts_init = unsafe { generated_slot!(vtable, ts_init)(handle) };
    assert_eq!(ts_init, 11);

    // SAFETY: see above.
    let to_json_result = unsafe { generated_slot!(vtable, to_json)(handle) };
    let encoded = to_json_result.into_result().expect("to_json failed");
    // SAFETY: payload bytes live until `encoded` is dropped.
    let text = std::str::from_utf8(unsafe { encoded.as_bytes() }).unwrap();
    assert!(text.contains("1.5"));

    // SAFETY: handle is still live for the clone.
    let cloned = unsafe { generated_slot!(vtable, clone_handle)(handle) };
    // SAFETY: both handles are live.
    let eq = unsafe { generated_slot!(vtable, eq_handles)(handle, cloned) };
    assert!(eq, "cloned value should compare equal");

    // SAFETY: dropping the original live handle.
    unsafe {
        generated_slot!(vtable, drop_handle)(handle);
    };
    // SAFETY: dropping the cloned live handle.
    unsafe {
        generated_slot!(vtable, drop_handle)(cloned);
    };
}

#[rstest]
#[case::null(std::ptr::null::<HostVTable>())]
fn nautilus_plugin_init_rejects_invalid_host_ptr(#[case] host: *const HostVTable) {
    let m = unsafe { nautilus_plugin_init(host) };
    assert!(m.is_null(), "init should reject the host pointer");
}

#[rstest]
#[case::off_by_one(NAUTILUS_PLUGIN_ABI_VERSION.wrapping_add(1))]
#[case::zero(0)]
#[case::max(u32::MAX)]
fn nautilus_plugin_init_rejects_abi_mismatch(#[case] abi: u32) {
    let bad_host = HostVTable {
        abi_version: abi,
        clock_now_ns: test_clock_now_ns,
        log: test_log,
        cache_instrument: test_cache_instrument,
        cache_account: test_cache_account,
        cache_order: test_cache_order,
        cache_position: test_cache_position,
        cache_orders_for_strategy: test_cache_orders_for_strategy,
        cache_positions_for_strategy: test_cache_positions_for_strategy,
        subscribe_quotes: test_subscribe_quotes,
        unsubscribe_quotes: test_unsubscribe_quotes,
        subscribe_trades: test_subscribe_trades,
        unsubscribe_trades: test_unsubscribe_trades,
        subscribe_bars: test_subscribe_bars,
        unsubscribe_bars: test_unsubscribe_bars,
        subscribe_book_deltas: test_subscribe_book_deltas,
        unsubscribe_book_deltas: test_unsubscribe_book_deltas,
        subscribe_book_at_interval: test_subscribe_book_at_interval,
        unsubscribe_book_at_interval: test_unsubscribe_book_at_interval,
        msgbus_publish: test_msgbus_publish,
        set_time_alert: test_set_time_alert,
        set_timer: test_set_timer,
        cancel_timer: test_cancel_timer,
        submit_order: test_submit_order,
        cancel_order: test_cancel_order,
        modify_order: test_modify_order,
    };
    let m = unsafe { nautilus_plugin_init(&raw const bad_host) };
    assert!(m.is_null(), "init should reject ABI {abi}");
}

#[rstest]
fn each_custom_data_type_has_its_own_vtable() {
    // Regression test: a previous design used a non-T-parameterised `static
    // OnceLock<CustomDataVTable>` inside the per-T vtable function, which
    // shared the same table across every monomorphisation. With the fix, each
    // `T` must give a distinct vtable address and distinct type-name output.
    use nautilus_plugin::surfaces::custom_data::{CustomDataVTable, custom_data_vtable};

    #[derive(Clone, Debug, PartialEq)]
    struct OtherTick(u64);
    impl PluginCustomData for OtherTick {
        const TYPE_NAME: &'static str = "OtherTick";
        fn ts_event(&self) -> u64 {
            self.0
        }
        fn ts_init(&self) -> u64 {
            self.0
        }
        fn to_json(&self) -> anyhow::Result<Vec<u8>> {
            Ok(self.0.to_string().into_bytes())
        }
        fn from_json(p: &[u8]) -> anyhow::Result<Self> {
            Ok(Self(std::str::from_utf8(p)?.parse()?))
        }
        fn schema_ipc() -> anyhow::Result<Vec<u8>> {
            Ok(b"other".to_vec())
        }
        fn encode_batch(_items: &[&Self]) -> anyhow::Result<Vec<u8>> {
            Ok(Vec::new())
        }
        fn decode_batch(_b: &[u8], _m: &[(String, String)]) -> anyhow::Result<Vec<Self>> {
            Ok(Vec::new())
        }
    }

    let v_test: *const CustomDataVTable = custom_data_vtable::<TestTick>();
    let v_other: *const CustomDataVTable = custom_data_vtable::<OtherTick>();
    assert!(!v_test.is_null());
    assert!(!v_other.is_null());
    assert!(
        !std::ptr::eq(v_test, v_other),
        "different T must produce different vtables"
    );

    // SAFETY: vtables live for the process lifetime.
    let test_vtable = unsafe { &*v_test };
    // SAFETY: see above.
    let other_vtable = unsafe { &*v_other };
    // SAFETY: type_name returns a static string.
    let test_name = unsafe { generated_slot!(test_vtable, type_name)() };
    // SAFETY: see above.
    let other_name = unsafe { generated_slot!(other_vtable, type_name)() };
    // SAFETY: name strings live in static storage in this binary.
    assert_eq!(unsafe { test_name.as_str() }, "TestTick");
    // SAFETY: see above.
    assert_eq!(unsafe { other_name.as_str() }, "OtherTick");

    // Stronger: each vtable must dispatch to its own T's logic, not just
    // expose its own type name. Round-trip a sentinel value through
    // OtherTick's from_json and back through its to_json; the output must
    // match OtherTick's encoding, not TestTick's JSON shape.
    let payload = BorrowedStr::from_str("42");
    // SAFETY: payload outlives the call.
    let handle = unsafe { generated_slot!(other_vtable, from_json)(payload) }
        .into_result()
        .expect("OtherTick from_json");
    // SAFETY: handle is live.
    let encoded = unsafe { generated_slot!(other_vtable, to_json)(handle) }
        .into_result()
        .expect("OtherTick to_json");
    // SAFETY: encoded buffer live.
    let bytes = unsafe { encoded.as_bytes() };
    assert_eq!(bytes, b"42", "OtherTick encoding is `value as decimal`");
    // SAFETY: handle is live.
    unsafe {
        generated_slot!(other_vtable, drop_handle)(handle);
    };
}

#[rstest]
fn decode_batch_round_trips_through_drop_handle_array() {
    // Exercises the largest untested path: encode_batch_thunk and
    // decode_batch_thunk, ending in drop_handle_array. Verifies that the
    // returned `OwnedBytes` carries the right `drop_fn` and that the
    // allocator pairing for the `Vec<*mut CustomDataHandle>` is intact.
    let manifest_ptr = unsafe { nautilus_plugin_init(&raw const TEST_HOST) };
    let manifest = unsafe { &*manifest_ptr };
    // SAFETY: slice points at static storage owned by the manifest.
    let entry = unsafe { &manifest.custom_data.as_slice()[0] };
    // SAFETY: vtable pointer is non-null and lives for the process lifetime.
    let vtable = unsafe { &*entry.vtable };

    let one = BorrowedStr::from_str(r#"{"value":1.0,"ts_event":1,"ts_init":2}"#);
    let two = BorrowedStr::from_str(r#"{"value":2.0,"ts_event":3,"ts_init":4}"#);
    // SAFETY: payloads outlive the calls.
    let h1 = unsafe { generated_slot!(vtable, from_json)(one) }
        .into_result()
        .expect("from_json one");
    let h2 = unsafe { generated_slot!(vtable, from_json)(two) }
        .into_result()
        .expect("from_json two");

    let handles: [*const CustomDataHandle; 2] = [h1.cast_const(), h2.cast_const()];
    let handles_slice = Slice::from_slice(&handles);
    // SAFETY: handles slice outlives the call.
    let encoded = unsafe { generated_slot!(vtable, encode_batch)(handles_slice) }
        .into_result()
        .expect("encode_batch");
    // SAFETY: encoded buffer is live.
    let ipc = unsafe { encoded.as_bytes() }.to_vec();
    // SAFETY: dropping live handle h1 before decode (decode allocates new ones).
    unsafe {
        generated_slot!(vtable, drop_handle)(h1);
    };
    // SAFETY: dropping live handle h2 before decode.
    unsafe {
        generated_slot!(vtable, drop_handle)(h2);
    };
    drop(encoded);

    // No metadata for this sentinel encoding.
    let md_entries: [MetadataEntry<'_>; 0] = [];
    let ipc_slice = Slice::from_slice(&ipc);
    let md_slice = Slice::from_slice(&md_entries);
    // SAFETY: slices outlive the call.
    let decoded = unsafe { generated_slot!(vtable, decode_batch)(ipc_slice, md_slice) }
        .into_result()
        .expect("decode_batch");
    // SAFETY: buffer is live.
    let len = unsafe { decoded.as_bytes() }.len();
    let elem_size = std::mem::size_of::<*mut CustomDataHandle>();
    let count = len / elem_size;
    assert_eq!(count, 2, "decoded handle count");

    // Walk the buffer as a contiguous run of handle pointers, verify the
    // values match what we encoded, then release each via drop_handle. The
    // final `drop(decoded)` invokes `drop_handle_array` which deallocates
    // with the correct `Vec<*mut CustomDataHandle>` layout.
    // SAFETY: buffer is live and contains `count` aligned handle pointers.
    let buf_ptr = unsafe { decoded.as_bytes() }.as_ptr();
    let handle_ptr = buf_ptr.cast::<*mut CustomDataHandle>();

    for i in 0..count {
        // SAFETY: i < count and the buffer is `count * elem_size` bytes.
        let slot = unsafe { handle_ptr.add(i) };
        // SAFETY: slot points at a freshly-decoded handle pointer.
        let h = unsafe { slot.read() };
        // SAFETY: handle is live (decode just produced it).
        let ts_init = unsafe { generated_slot!(vtable, ts_init)(h) };
        // Decoded order matches encoded order: ts_init was 2 then 4.
        assert_eq!(ts_init, ((i as u64) + 1) * 2);
        // SAFETY: handle is live.
        unsafe {
            generated_slot!(vtable, drop_handle)(h);
        };
    }
    drop(decoded);
}

#[rstest]
fn drop_handle_thunk_ignores_null() {
    // Regression: `drop_handle_thunk` short-circuits on null, so a host that
    // accidentally passes a null handle does not invoke `Box::from_raw(null)`
    // (which is undefined behaviour).
    let vtable_ptr = custom_data_vtable::<TestTick>();
    // SAFETY: vtable lives for the process lifetime.
    let vtable = unsafe { &*vtable_ptr };
    // SAFETY: the documented contract: drop_handle ignores null pointers.
    unsafe {
        generated_slot!(vtable, drop_handle)(std::ptr::null_mut());
    };
}

#[rstest]
fn actor_lifecycle_callbacks_dispatch_to_trait() {
    // Walk the manifest, find the actor registration, create an instance via
    // the vtable, then exercise the lifecycle thunks. The test actor stores
    // call counts in atomics so we can assert dispatch without crossing the
    // FFI boundary with references.
    let manifest_ptr = unsafe { nautilus_plugin_init(&raw const TEST_HOST) };
    let manifest = unsafe { &*manifest_ptr };
    // SAFETY: slice points at static storage owned by the manifest.
    let actors = unsafe { manifest.actors.as_slice() };
    let entry = &actors[0];
    // SAFETY: vtable lives for the process lifetime.
    let vtable = unsafe { &*entry.vtable };

    TEST_ACTOR_START_COUNT.store(0, Ordering::SeqCst);
    TEST_ACTOR_STOP_COUNT.store(0, Ordering::SeqCst);

    let host = (&raw const TEST_HOST).cast::<HostVTable>();
    let ctx: *const HostContext = std::ptr::null();
    // SAFETY: vtable produces a fresh, exclusively-owned handle.
    let handle = unsafe { generated_slot!(vtable, create)(host, ctx, BorrowedStr::empty()) };
    assert!(!handle.is_null(), "create returned null");

    // SAFETY: handle is live.
    let r = unsafe { generated_slot!(vtable, on_start)(handle) };
    r.into_result().expect("on_start");
    assert_eq!(TEST_ACTOR_START_COUNT.load(Ordering::SeqCst), 1);

    // SAFETY: handle is live.
    let r = unsafe { generated_slot!(vtable, on_stop)(handle) };
    r.into_result().expect("on_stop");
    assert_eq!(TEST_ACTOR_STOP_COUNT.load(Ordering::SeqCst), 1);

    // SAFETY: handle is live.
    unsafe {
        generated_slot!(vtable, drop_handle)(handle);
    };
}

#[rstest]
fn actor_on_quote_dispatches_typed_pointer() {
    use nautilus_core::UnixNanos;
    use nautilus_model::{
        identifiers::InstrumentId,
        types::{Price, Quantity},
    };

    // Verifies the on_quote thunk receives the typed `*const QuoteTick`
    // pointer and routes it into the actor's typed `on_quote` method.
    let manifest_ptr = unsafe { nautilus_plugin_init(&raw const TEST_HOST) };
    let manifest = unsafe { &*manifest_ptr };
    // SAFETY: slice points at static storage owned by the manifest.
    let actors = unsafe { manifest.actors.as_slice() };
    let entry = &actors[0];
    // SAFETY: vtable lives for the process lifetime.
    let vtable = unsafe { &*entry.vtable };

    TEST_ACTOR_QUOTE_COUNT.store(0, Ordering::SeqCst);
    TEST_ACTOR_LAST_BID_RAW.store(0, Ordering::SeqCst);

    let quote = QuoteTick::new(
        InstrumentId::from("ETH-USDT.BINANCE"),
        Price::from("1500.00"),
        Price::from("1500.05"),
        Quantity::from("1.0"),
        Quantity::from("1.0"),
        UnixNanos::from(1_700_000_000_000_000_000u64),
        UnixNanos::from(1_700_000_000_000_000_000u64),
    );

    let host = (&raw const TEST_HOST).cast::<HostVTable>();
    let ctx: *const HostContext = std::ptr::null();
    // SAFETY: vtable produces a fresh handle.
    let handle = unsafe { generated_slot!(vtable, create)(host, ctx, BorrowedStr::empty()) };
    // SAFETY: handle is live and `quote` outlives the call.
    let r = unsafe { generated_slot!(vtable, on_quote)(handle, &raw const quote) };
    r.into_result().expect("on_quote");

    assert_eq!(TEST_ACTOR_QUOTE_COUNT.load(Ordering::SeqCst), 1);
    assert_eq!(
        TEST_ACTOR_LAST_BID_RAW.load(Ordering::SeqCst),
        quote.bid_price.raw as u64,
    );

    // SAFETY: handle is live.
    unsafe {
        generated_slot!(vtable, drop_handle)(handle);
    };
}

#[rstest]
fn actor_drop_handle_thunk_ignores_null() {
    let vtable_ptr = nautilus_plugin::surfaces::actor::actor_vtable::<TestActor>();
    // SAFETY: vtable lives for the process lifetime.
    let vtable = unsafe { &*vtable_ptr };
    // SAFETY: the documented contract: drop_handle ignores null pointers.
    unsafe {
        generated_slot!(vtable, drop_handle)(std::ptr::null_mut());
    };
}

// Sentinel non-null pointer used as the strategy's host context in tests.
// The test host doesn't deref the context, so any non-null value works.
static SENTINEL_CTX: HostContextPad = HostContextPad { _filler: 0 };

#[repr(transparent)]
struct HostContextPad {
    _filler: u8,
}

fn sentinel_ctx() -> *const HostContext {
    (&raw const SENTINEL_CTX).cast::<HostContext>()
}

#[rstest]
fn strategy_lifecycle_dispatches_to_trait() {
    let vtable_ptr = nautilus_plugin::surfaces::strategy::strategy_vtable::<TestStrategy>();
    // SAFETY: vtable lives for the process lifetime.
    let vtable = unsafe { &*vtable_ptr };

    TEST_STRATEGY_START_COUNT.store(0, Ordering::SeqCst);
    TEST_STRATEGY_CONTEXT_PTR.store(std::ptr::null_mut(), Ordering::SeqCst);

    let ctx = sentinel_ctx();
    let host = (&raw const TEST_HOST).cast::<HostVTable>();
    // SAFETY: vtable produces a fresh, exclusively-owned handle.
    let handle = unsafe { generated_slot!(vtable, create)(host, ctx, BorrowedStr::empty()) };
    assert!(!handle.is_null(), "create returned null");

    // The strategy must have stored the context pointer during `new`.
    let stored = TEST_STRATEGY_CONTEXT_PTR.load(Ordering::SeqCst) as *const HostContext;
    assert!(
        std::ptr::eq(stored, ctx),
        "strategy did not store host context"
    );

    // SAFETY: handle is live.
    let r = unsafe { generated_slot!(vtable, on_start)(handle) };
    r.into_result().expect("on_start");
    assert_eq!(TEST_STRATEGY_START_COUNT.load(Ordering::SeqCst), 1);

    // SAFETY: handle is live.
    unsafe {
        generated_slot!(vtable, drop_handle)(handle);
    };
}

#[rstest]
fn strategy_on_position_opened_invokes_host_submit() {
    use nautilus_core::{UUID4, UnixNanos};
    use nautilus_model::{
        enums::{OrderSide, PositionSide},
        events::PositionOpened,
        identifiers::{AccountId, ClientOrderId, InstrumentId, PositionId, StrategyId, TraderId},
        types::{Currency, Price, Quantity},
    };

    // Drives an on_position_opened callback with a typed pointer and
    // verifies (a) the trait method ran, and (b) the strategy invoked
    // `submit_order` on the host vtable it was bound to at create time
    // with the same context it received.
    let vtable_ptr = nautilus_plugin::surfaces::strategy::strategy_vtable::<TestStrategy>();
    // SAFETY: vtable lives for the process lifetime.
    let vtable = unsafe { &*vtable_ptr };

    TEST_STRATEGY_POSITION_OPENED_COUNT.store(0, Ordering::SeqCst);
    TEST_HOST_SUBMIT_COUNT.store(0, Ordering::SeqCst);
    TEST_HOST_LAST_SUBMIT_CTX.store(std::ptr::null_mut(), Ordering::SeqCst);
    TEST_HOST_SUBMIT_SENTINEL.store(0, Ordering::SeqCst);

    let ctx = sentinel_ctx();
    let host = (&raw const TEST_HOST).cast::<HostVTable>();
    // SAFETY: vtable produces a fresh handle.
    let handle = unsafe { generated_slot!(vtable, create)(host, ctx, BorrowedStr::empty()) };

    let event = PositionOpened {
        trader_id: TraderId::from("TESTER-001"),
        strategy_id: StrategyId::from("S-001"),
        instrument_id: InstrumentId::from("ETH-USDT.BINANCE"),
        position_id: PositionId::from("P-19700101-0000-000-000-1"),
        account_id: AccountId::from("BINANCE-001"),
        opening_order_id: ClientOrderId::from("O-19700101-0000-000-000-1"),
        entry: OrderSide::Buy,
        side: PositionSide::Long,
        signed_qty: 1.0,
        quantity: Quantity::from("1.0"),
        last_qty: Quantity::from("1.0"),
        last_px: Price::from("1500.00"),
        currency: Currency::USDT(),
        avg_px_open: 1500.0,
        event_id: UUID4::new(),
        ts_event: UnixNanos::from(1u64),
        ts_init: UnixNanos::from(1u64),
    };

    // SAFETY: handle is live and `event` outlives the call.
    let r = unsafe { generated_slot!(vtable, on_position_opened)(handle, &raw const event) };
    r.into_result().expect("on_position_opened");

    assert_eq!(
        TEST_STRATEGY_POSITION_OPENED_COUNT.load(Ordering::SeqCst),
        1
    );
    assert_eq!(TEST_HOST_SUBMIT_COUNT.load(Ordering::SeqCst), 1);
    let last_ctx = TEST_HOST_LAST_SUBMIT_CTX.load(Ordering::SeqCst) as *const HostContext;
    assert!(
        std::ptr::eq(last_ctx, ctx),
        "submit_order received the wrong host context"
    );
    assert_eq!(
        TEST_HOST_SUBMIT_SENTINEL.load(Ordering::SeqCst),
        TEST_HOST_SENTINEL_VALUE,
        "submit_order ran on a different host vtable than the bound one",
    );

    // SAFETY: handle is live.
    unsafe {
        generated_slot!(vtable, drop_handle)(handle);
    };
}

#[rstest]
fn strategy_drop_handle_thunk_ignores_null() {
    let vtable_ptr = nautilus_plugin::surfaces::strategy::strategy_vtable::<TestStrategy>();
    // SAFETY: vtable lives for the process lifetime.
    let vtable = unsafe { &*vtable_ptr };
    // SAFETY: the documented contract: drop_handle ignores null pointers.
    unsafe {
        generated_slot!(vtable, drop_handle)(std::ptr::null_mut());
    };
}

#[rstest]
fn each_actor_type_has_its_own_vtable() {
    // Regression test mirroring the custom_data per-T check. Defends
    // against future refactors of the `Tag<T>`/`PhantomData<T>` pattern in
    // surfaces::actor that would silently collapse distinct actor types
    // onto a single shared vtable.
    use nautilus_plugin::surfaces::actor::{ActorVTable, actor_vtable};

    #[derive(Default)]
    struct OtherActor;
    impl PluginActor for OtherActor {
        const TYPE_NAME: &'static str = "OtherActor";

        fn new(_host: *const HostVTable, _ctx: *const HostContext, _config_json: &str) -> Self {
            Self
        }
    }

    let a: *const ActorVTable = actor_vtable::<TestActor>();
    let b: *const ActorVTable = actor_vtable::<OtherActor>();
    assert!(!a.is_null() && !b.is_null());
    assert!(
        !std::ptr::eq(a, b),
        "different actor T must produce different vtables"
    );

    // SAFETY: vtables live for the process lifetime.
    let a_vt = unsafe { &*a };
    // SAFETY: see above.
    let b_vt = unsafe { &*b };
    // SAFETY: type_name returns a static string.
    let a_name = unsafe { generated_slot!(a_vt, type_name)() };
    // SAFETY: see above.
    let b_name = unsafe { generated_slot!(b_vt, type_name)() };
    // SAFETY: name strings live in static storage.
    assert_eq!(unsafe { a_name.as_str() }, "TestActor");
    // SAFETY: see above.
    assert_eq!(unsafe { b_name.as_str() }, "OtherActor");
}

#[rstest]
fn each_strategy_type_has_its_own_vtable() {
    // Regression test for the strategy surface, matching the actor and
    // custom_data per-T identity tests.
    use nautilus_plugin::surfaces::strategy::{StrategyVTable, strategy_vtable};

    struct OtherStrategy;
    // SAFETY: OtherStrategy only stores opaque pointers the host keeps
    // alive for the strategy's lifetime; the trait requires Send.
    unsafe impl Send for OtherStrategy {}
    impl PluginStrategy for OtherStrategy {
        const TYPE_NAME: &'static str = "OtherStrategy";

        fn new(_host: *const HostVTable, _ctx: *const HostContext, _config_json: &str) -> Self {
            Self
        }
    }

    let a: *const StrategyVTable = strategy_vtable::<TestStrategy>();
    let b: *const StrategyVTable = strategy_vtable::<OtherStrategy>();
    assert!(!a.is_null() && !b.is_null());
    assert!(
        !std::ptr::eq(a, b),
        "different strategy T must produce different vtables"
    );

    // SAFETY: vtables live for the process lifetime.
    let a_vt = unsafe { &*a };
    // SAFETY: see above.
    let b_vt = unsafe { &*b };
    // SAFETY: type_name returns a static string.
    let a_name = unsafe { generated_slot!(a_vt, type_name)() };
    // SAFETY: see above.
    let b_name = unsafe { generated_slot!(b_vt, type_name)() };
    // SAFETY: name strings live in static storage.
    assert_eq!(unsafe { a_name.as_str() }, "TestStrategy");
    // SAFETY: see above.
    assert_eq!(unsafe { b_name.as_str() }, "OtherStrategy");
}

#[rstest]
fn custom_data_vtable_schema_ipc_returns_registered_schema() {
    // Direct vtable invocation of `schema_ipc` for TestTick. Locks the
    // contract that the registered closure runs and returns the same
    // bytes through the C-ABI boundary.
    let manifest_ptr = unsafe { nautilus_plugin_init(&raw const TEST_HOST) };
    let manifest = unsafe { &*manifest_ptr };
    // SAFETY: slice points at static storage owned by the manifest.
    let entry = unsafe { &manifest.custom_data.as_slice()[0] };
    // SAFETY: vtable lives for the process lifetime.
    let vtable = unsafe { &*entry.vtable };

    // SAFETY: schema_ipc takes no inputs and returns owned bytes the
    // caller is responsible for dropping.
    let r = unsafe { generated_slot!(vtable, schema_ipc)() };
    let bytes = r.into_result().expect("schema_ipc failed");
    // SAFETY: buffer live until `bytes` is dropped.
    assert_eq!(unsafe { bytes.as_bytes() }, b"test-schema");
}

#[rstest]
fn host_vtable_cancel_order_routes_to_bound_handler() {
    TEST_HOST_CANCEL_COUNT.store(0, Ordering::SeqCst);
    let host = &TEST_HOST;
    let ctx: *const HostContext = std::ptr::null();
    let cmd = BorrowedStr::from_str(r#"{"kind":"cancel"}"#);
    // SAFETY: cmd outlives the call; ctx is not dereferenced by the test stub.
    let r = unsafe { (host.cancel_order)(ctx, cmd) };
    r.into_result().expect("cancel_order");
    assert_eq!(TEST_HOST_CANCEL_COUNT.load(Ordering::SeqCst), 1);
}

#[rstest]
fn host_vtable_modify_order_routes_to_bound_handler() {
    TEST_HOST_MODIFY_COUNT.store(0, Ordering::SeqCst);
    let host = &TEST_HOST;
    let ctx: *const HostContext = std::ptr::null();
    let cmd = BorrowedStr::from_str(r#"{"kind":"modify"}"#);
    // SAFETY: cmd outlives the call; ctx is not dereferenced by the test stub.
    let r = unsafe { (host.modify_order)(ctx, cmd) };
    r.into_result().expect("modify_order");
    assert_eq!(TEST_HOST_MODIFY_COUNT.load(Ordering::SeqCst), 1);
}
