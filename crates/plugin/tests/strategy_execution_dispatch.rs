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
//
//! Per-method dispatch tests for the strategy plug point's order command
//! surface.
//!
//! [`PluginStrategy`] extends the actor callback surface with a host-side
//! execution surface: `submit_order`, `cancel_order`, and `modify_order`.
//! At create time, the host hands the strategy a [`HostVTable`] pointer
//! plus an opaque [`HostContext`] the host uses to attribute each command
//! back to the calling strategy. A wiring mistake at the strategy author's
//! site (e.g. storing the wrong pointer or passing a stale context)
//! compiles but routes commands to the wrong strategy or to none at all.
//!
//! These tests build a fake host vtable whose order-command handlers each
//! bump a per-slot counter and record the [`HostContext`] pointer they
//! received. The strategy under test invokes one execution method per
//! callback through its bound `(host, ctx)`. The parametrised test drives
//! each callback in turn and asserts:
//!
//! - Only the matching counter incremented (no cross-wiring of execution
//!   methods).
//! - The host received the exact [`HostContext`] the strategy was given
//!   at `create`.
//!
//! Mirrors the strategy-side analogue of the actor dispatch coverage in
//! [`tests/hook_dispatch.rs`](./hook_dispatch.rs).

#![allow(unsafe_code)]

use std::sync::{
    Mutex, MutexGuard, OnceLock,
    atomic::{AtomicPtr, AtomicU64, Ordering},
};

use nautilus_plugin::{
    NAUTILUS_PLUGIN_ABI_VERSION,
    boundary::{BorrowedStr, OwnedBytes, PluginResult, Slice},
    host::{HostContext, HostLogLevel, HostVTable},
    surfaces::strategy::{PluginStrategy, strategy_vtable},
};
use rstest::rstest;

macro_rules! generated_slot {
    ($vtable:expr, $slot:ident) => {{
        ($vtable)
            .$slot
            .expect(concat!("generated vtable includes ", stringify!($slot)))
    }};
}

// One variant per [`HostVTable`] order-command slot that a strategy can
// invoke. Indexed into the counter and last-context arrays.
#[repr(usize)]
#[derive(Clone, Copy, Debug)]
enum ExecHook {
    Submit,
    Cancel,
    Modify,
    SubmitList,
    CancelOrders,
    CancelAll,
    ClosePosition,
    CloseAll,
    QueryAccount,
    QueryOrder,
}

const HOOK_COUNT: usize = ExecHook::QueryOrder as usize + 1;
static HOOK_CALLS: [AtomicU64; HOOK_COUNT] = [const { AtomicU64::new(0) }; HOOK_COUNT];
static LAST_CTX: [AtomicPtr<HostContext>; HOOK_COUNT] =
    [const { AtomicPtr::new(std::ptr::null_mut()) }; HOOK_COUNT];

fn dispatch_lock() -> MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|p| p.into_inner())
}

fn reset_all() {
    for c in &HOOK_CALLS {
        c.store(0, Ordering::SeqCst);
    }

    for c in &LAST_CTX {
        c.store(std::ptr::null_mut(), Ordering::SeqCst);
    }
}

fn record(ctx: *const HostContext, hook: ExecHook) {
    let idx = hook as usize;
    HOOK_CALLS[idx].fetch_add(1, Ordering::SeqCst);
    LAST_CTX[idx].store(ctx.cast_mut(), Ordering::SeqCst);
}

fn assert_only_hook(expected: ExecHook) {
    for (i, c) in HOOK_CALLS.iter().enumerate() {
        let v = c.load(Ordering::SeqCst);
        if i == expected as usize {
            assert_eq!(v, 1, "hook {expected:?} should have fired exactly once");
        } else {
            assert_eq!(
                v, 0,
                "hook at index {i} fired but {expected:?} was expected",
            );
        }
    }
}

fn assert_ctx(hook: ExecHook, expected: *const HostContext) {
    let last = LAST_CTX[hook as usize].load(Ordering::SeqCst) as *const HostContext;
    assert!(
        std::ptr::eq(last, expected),
        "host context not threaded through to {hook:?}: expected {expected:?}, was {last:?}",
    );
}

// Stub handlers for every non-execution slot in [`HostVTable`]. The
// strategy under test invokes the ten execution slots (submit/cancel/
// modify order, submit_order_list, cancel_orders, cancel_all_orders,
// close_position, close_all_positions, query_account, query_order); the
// others stay as no-ops returning empty results. They still need
// non-null function pointers because [`HostVTable`] fields are
// unconditional `unsafe extern "C" fn`.
unsafe extern "C" fn stub_clock_now_ns() -> u64 {
    0
}

unsafe extern "C" fn stub_log(
    _level: HostLogLevel,
    _target: BorrowedStr<'_>,
    _message: BorrowedStr<'_>,
) {
}

macro_rules! stub_bytes_handler {
    ($name:ident) => {
        unsafe extern "C" fn $name(
            _ctx: *const HostContext,
            _arg: BorrowedStr<'_>,
        ) -> PluginResult<OwnedBytes> {
            PluginResult::Ok(OwnedBytes::empty())
        }
    };
}

stub_bytes_handler!(stub_cache_instrument);
stub_bytes_handler!(stub_cache_account);
stub_bytes_handler!(stub_cache_order);
stub_bytes_handler!(stub_cache_position);
stub_bytes_handler!(stub_cache_orders_for_strategy);
stub_bytes_handler!(stub_cache_positions_for_strategy);

macro_rules! stub_subscription_handler {
    ($name:ident) => {
        unsafe extern "C" fn $name(
            _ctx: *const HostContext,
            _id: BorrowedStr<'_>,
            _client_id: BorrowedStr<'_>,
            _params_json: BorrowedStr<'_>,
        ) -> PluginResult<()> {
            PluginResult::Ok(())
        }
    };
}

stub_subscription_handler!(stub_subscribe_quotes);
stub_subscription_handler!(stub_unsubscribe_quotes);
stub_subscription_handler!(stub_subscribe_trades);
stub_subscription_handler!(stub_unsubscribe_trades);
stub_subscription_handler!(stub_subscribe_bars);
stub_subscription_handler!(stub_unsubscribe_bars);
stub_subscription_handler!(stub_unsubscribe_book_deltas);

unsafe extern "C" fn stub_subscribe_book_deltas(
    _ctx: *const HostContext,
    _instrument_id: BorrowedStr<'_>,
    _book_type: u8,
    _depth: usize,
    _client_id: BorrowedStr<'_>,
    _managed: u8,
    _params_json: BorrowedStr<'_>,
) -> PluginResult<()> {
    PluginResult::Ok(())
}

unsafe extern "C" fn stub_subscribe_book_at_interval(
    _ctx: *const HostContext,
    _instrument_id: BorrowedStr<'_>,
    _book_type: u8,
    _depth: usize,
    _interval_ms: usize,
    _client_id: BorrowedStr<'_>,
    _params_json: BorrowedStr<'_>,
) -> PluginResult<()> {
    PluginResult::Ok(())
}

unsafe extern "C" fn stub_unsubscribe_book_at_interval(
    _ctx: *const HostContext,
    _instrument_id: BorrowedStr<'_>,
    _interval_ms: usize,
    _client_id: BorrowedStr<'_>,
    _params_json: BorrowedStr<'_>,
) -> PluginResult<()> {
    PluginResult::Ok(())
}

unsafe extern "C" fn stub_msgbus_publish(
    _ctx: *const HostContext,
    _topic: BorrowedStr<'_>,
    _payload: Slice<'_, u8>,
) -> PluginResult<()> {
    PluginResult::Ok(())
}

unsafe extern "C" fn stub_set_time_alert(
    _ctx: *const HostContext,
    _name: BorrowedStr<'_>,
    _alert_time_ns: u64,
    _allow_past: u8,
) -> PluginResult<()> {
    PluginResult::Ok(())
}

unsafe extern "C" fn stub_set_timer(
    _ctx: *const HostContext,
    _name: BorrowedStr<'_>,
    _interval_ns: u64,
    _start_time_ns: u64,
    _stop_time_ns: u64,
    _allow_past: u8,
    _fire_immediately: u8,
) -> PluginResult<()> {
    PluginResult::Ok(())
}

unsafe extern "C" fn stub_cancel_timer(
    _ctx: *const HostContext,
    _name: BorrowedStr<'_>,
) -> PluginResult<()> {
    PluginResult::Ok(())
}

// Recording handlers for the three execution slots. These are the ones a
// strategy is allowed to invoke; they bump the per-hook counter and
// capture the [`HostContext`] pointer the strategy passed.

unsafe extern "C" fn recording_submit_order(
    ctx: *const HostContext,
    _command_json: BorrowedStr<'_>,
) -> PluginResult<()> {
    record(ctx, ExecHook::Submit);
    PluginResult::Ok(())
}

unsafe extern "C" fn recording_cancel_order(
    ctx: *const HostContext,
    _command_json: BorrowedStr<'_>,
) -> PluginResult<()> {
    record(ctx, ExecHook::Cancel);
    PluginResult::Ok(())
}

unsafe extern "C" fn recording_modify_order(
    ctx: *const HostContext,
    _command_json: BorrowedStr<'_>,
) -> PluginResult<()> {
    record(ctx, ExecHook::Modify);
    PluginResult::Ok(())
}

unsafe extern "C" fn recording_submit_order_list(
    ctx: *const HostContext,
    _command_json: BorrowedStr<'_>,
) -> PluginResult<()> {
    record(ctx, ExecHook::SubmitList);
    PluginResult::Ok(())
}

unsafe extern "C" fn recording_cancel_orders(
    ctx: *const HostContext,
    _command_json: BorrowedStr<'_>,
) -> PluginResult<()> {
    record(ctx, ExecHook::CancelOrders);
    PluginResult::Ok(())
}

unsafe extern "C" fn recording_cancel_all_orders(
    ctx: *const HostContext,
    _command_json: BorrowedStr<'_>,
) -> PluginResult<()> {
    record(ctx, ExecHook::CancelAll);
    PluginResult::Ok(())
}

unsafe extern "C" fn recording_close_position(
    ctx: *const HostContext,
    _command_json: BorrowedStr<'_>,
) -> PluginResult<()> {
    record(ctx, ExecHook::ClosePosition);
    PluginResult::Ok(())
}

unsafe extern "C" fn recording_close_all_positions(
    ctx: *const HostContext,
    _command_json: BorrowedStr<'_>,
) -> PluginResult<()> {
    record(ctx, ExecHook::CloseAll);
    PluginResult::Ok(())
}

unsafe extern "C" fn recording_query_account(
    ctx: *const HostContext,
    _command_json: BorrowedStr<'_>,
) -> PluginResult<()> {
    record(ctx, ExecHook::QueryAccount);
    PluginResult::Ok(())
}

unsafe extern "C" fn recording_query_order(
    ctx: *const HostContext,
    _command_json: BorrowedStr<'_>,
) -> PluginResult<()> {
    record(ctx, ExecHook::QueryOrder);
    PluginResult::Ok(())
}

static TEST_HOST: HostVTable = HostVTable {
    abi_version: NAUTILUS_PLUGIN_ABI_VERSION,
    clock_now_ns: stub_clock_now_ns,
    log: stub_log,
    cache_instrument: stub_cache_instrument,
    cache_account: stub_cache_account,
    cache_order: stub_cache_order,
    cache_position: stub_cache_position,
    cache_orders_for_strategy: stub_cache_orders_for_strategy,
    cache_positions_for_strategy: stub_cache_positions_for_strategy,
    subscribe_quotes: stub_subscribe_quotes,
    unsubscribe_quotes: stub_unsubscribe_quotes,
    subscribe_trades: stub_subscribe_trades,
    unsubscribe_trades: stub_unsubscribe_trades,
    subscribe_bars: stub_subscribe_bars,
    unsubscribe_bars: stub_unsubscribe_bars,
    subscribe_book_deltas: stub_subscribe_book_deltas,
    unsubscribe_book_deltas: stub_unsubscribe_book_deltas,
    subscribe_book_at_interval: stub_subscribe_book_at_interval,
    unsubscribe_book_at_interval: stub_unsubscribe_book_at_interval,
    msgbus_publish: stub_msgbus_publish,
    set_time_alert: stub_set_time_alert,
    set_timer: stub_set_timer,
    cancel_timer: stub_cancel_timer,
    submit_order: recording_submit_order,
    cancel_order: recording_cancel_order,
    modify_order: recording_modify_order,
    submit_order_list: recording_submit_order_list,
    cancel_orders: recording_cancel_orders,
    cancel_all_orders: recording_cancel_all_orders,
    close_position: recording_close_position,
    close_all_positions: recording_close_all_positions,
    query_account: recording_query_account,
    query_order: recording_query_order,
};

// Strategy whose `on_start` dispatches to the [`HostVTable`] slot named
// by a thread-local target. Each parametrised test case sets the target
// before driving `on_start`, so a single callback exercises every
// execution slot the strategy can invoke through its bound
// `(host, ctx)`. The mapping is arbitrary; only the routing matters.
thread_local! {
    static TARGET: std::cell::Cell<ExecHook> = const { std::cell::Cell::new(ExecHook::Submit) };
}

fn set_target(hook: ExecHook) {
    TARGET.with(|t| t.set(hook));
}

struct ExecStrategy {
    host: *const HostVTable,
    ctx: *const HostContext,
}

// SAFETY: ExecStrategy holds opaque host pointers the host commits to
// keeping live for the strategy's lifetime; the trait requires Send.
unsafe impl Send for ExecStrategy {}

impl PluginStrategy for ExecStrategy {
    const TYPE_NAME: &'static str = "ExecStrategy";

    fn new(host: *const HostVTable, ctx: *const HostContext, _config_json: &str) -> Self {
        Self { host, ctx }
    }

    fn on_start(&mut self) -> anyhow::Result<()> {
        // SAFETY: the host commits to keeping the vtable live for the strategy.
        let host = unsafe { &*self.host };
        let payload = BorrowedStr::from_str(r#"{"k":"v"}"#);
        // Each arm is a single unsafe call into the host's function-pointer
        // slot. SAFETY: ctx is the value the host supplied at create time
        // and the host keeps every fn pointer live for the strategy
        // lifetime.
        let r = match TARGET.with(std::cell::Cell::get) {
            ExecHook::Submit => unsafe { (host.submit_order)(self.ctx, payload) },
            ExecHook::Cancel => unsafe { (host.cancel_order)(self.ctx, payload) },
            ExecHook::Modify => unsafe { (host.modify_order)(self.ctx, payload) },
            ExecHook::SubmitList => unsafe { (host.submit_order_list)(self.ctx, payload) },
            ExecHook::CancelOrders => unsafe { (host.cancel_orders)(self.ctx, payload) },
            ExecHook::CancelAll => unsafe { (host.cancel_all_orders)(self.ctx, payload) },
            ExecHook::ClosePosition => unsafe { (host.close_position)(self.ctx, payload) },
            ExecHook::CloseAll => unsafe { (host.close_all_positions)(self.ctx, payload) },
            ExecHook::QueryAccount => unsafe { (host.query_account)(self.ctx, payload) },
            ExecHook::QueryOrder => unsafe { (host.query_order)(self.ctx, payload) },
        };
        r.into_result()
            .map_err(|e| anyhow::anyhow!(e.message_string()))
    }
}

// Sentinel non-null context pointer. The recording handlers do not deref
// it; the assertion only checks pointer identity through routing.
#[repr(transparent)]
struct HostContextPad {
    _filler: u8,
}
static SENTINEL_CTX: HostContextPad = HostContextPad { _filler: 0 };
// Per-instance sentinels for the distinct-context test. Lifted to module
// scope so clippy's items_after_statements does not fire on the test's
// `let _g = dispatch_lock();` ordering.
static CTX_A: HostContextPad = HostContextPad { _filler: 1 };
static CTX_B: HostContextPad = HostContextPad { _filler: 2 };

fn sentinel_ctx() -> *const HostContext {
    std::ptr::from_ref(&SENTINEL_CTX).cast::<HostContext>()
}

#[rstest]
#[case::submit_order(ExecHook::Submit)]
#[case::cancel_order(ExecHook::Cancel)]
#[case::modify_order(ExecHook::Modify)]
#[case::submit_order_list(ExecHook::SubmitList)]
#[case::cancel_orders(ExecHook::CancelOrders)]
#[case::cancel_all_orders(ExecHook::CancelAll)]
#[case::close_position(ExecHook::ClosePosition)]
#[case::close_all_positions(ExecHook::CloseAll)]
#[case::query_account(ExecHook::QueryAccount)]
#[case::query_order(ExecHook::QueryOrder)]
fn strategy_callback_invokes_bound_execution_method_with_stored_context(#[case] hook: ExecHook) {
    let _g = dispatch_lock();
    reset_all();
    set_target(hook);

    let vt_ptr = strategy_vtable::<ExecStrategy>();
    // SAFETY: vtable lives for the process lifetime.
    let vt = unsafe { &*vt_ptr };

    let host = std::ptr::from_ref(&TEST_HOST);
    let ctx = sentinel_ctx();
    // SAFETY: create returns a fresh, exclusively-owned handle.
    let handle = unsafe { generated_slot!(vt, create)(host, ctx, BorrowedStr::empty()) };
    assert!(!handle.is_null(), "create returned null");

    // SAFETY: handle is live; on_start reads TARGET and dispatches to the
    // matching execution slot through the strategy's bound (host, ctx).
    let r = unsafe { generated_slot!(vt, on_start)(handle) };
    r.into_result().expect("strategy callback");

    assert_only_hook(hook);
    assert_ctx(hook, ctx);

    // SAFETY: handle is live.
    unsafe {
        generated_slot!(vt, drop_handle)(handle);
    };
}

#[rstest]
fn distinct_strategy_instances_carry_distinct_contexts_to_the_host() {
    // Two strategy instances created with different host contexts must
    // forward each call back to the host through its own context, never
    // each other's. Guards against accidental sharing of `ctx` between
    // instances (e.g. a static field instead of an instance field).
    let _g = dispatch_lock();
    reset_all();

    let vt_ptr = strategy_vtable::<ExecStrategy>();
    // SAFETY: vtable lives for the process lifetime.
    let vt = unsafe { &*vt_ptr };

    let ctx_a = std::ptr::from_ref(&CTX_A).cast::<HostContext>();
    let ctx_b = std::ptr::from_ref(&CTX_B).cast::<HostContext>();
    let host = std::ptr::from_ref(&TEST_HOST);

    // SAFETY: create returns a fresh handle for each instance.
    let h_a = unsafe { generated_slot!(vt, create)(host, ctx_a, BorrowedStr::empty()) };
    // SAFETY: see above.
    let h_b = unsafe { generated_slot!(vt, create)(host, ctx_b, BorrowedStr::empty()) };

    set_target(ExecHook::Submit);
    // SAFETY: h_a is live; on_start forwards to submit_order with ctx_a.
    let r = unsafe { generated_slot!(vt, on_start)(h_a) };
    r.into_result().expect("on_start on instance A");
    assert_ctx(ExecHook::Submit, ctx_a);

    set_target(ExecHook::Modify);
    // SAFETY: h_b is live; on_start forwards to modify_order with ctx_b.
    let r = unsafe { generated_slot!(vt, on_start)(h_b) };
    r.into_result().expect("on_start on instance B");
    assert_ctx(ExecHook::Modify, ctx_b);

    // SAFETY: h_a is live.
    unsafe {
        generated_slot!(vt, drop_handle)(h_a);
    };
    // SAFETY: h_b is live.
    unsafe {
        generated_slot!(vt, drop_handle)(h_b);
    };
}
