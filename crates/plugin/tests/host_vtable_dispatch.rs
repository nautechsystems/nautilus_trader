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

//! Per-slot dispatch tests for [`HostVTable`].
//!
//! [`HostVTable`] is the host services surface that plug-in actors and
//! strategies call back into. The vtable struct is defined in this crate
//! but the function pointers come from the host (the live node) at load
//! time. A wiring mistake at the host's vtable-init site (e.g. assigning
//! the `unsubscribe_quotes` handler to `subscribe_quotes`) compiles but
//! routes commands to the wrong service, with no compiler help.
//!
//! These tests build a fake host vtable whose every handler bumps a
//! per-slot atomic counter and records the [`HostContext`] pointer the
//! caller passed. Each test then calls one slot through the vtable and
//! asserts only the matching counter incremented and the same context
//! pointer reached the handler.
//!
//! Covers every callable field of [`HostVTable`]: `clock_now_ns`, `log`,
//! the six `cache_*` snapshots, every subscribe/unsubscribe pair, the
//! message bus publish, the three clock alert/timer entries, and the
//! ten order command entries (`submit_order`, `cancel_order`,
//! `modify_order`, `submit_order_list`, `cancel_orders`,
//! `cancel_all_orders`, `close_position`, `close_all_positions`,
//! `query_account`, `query_order`).

#![allow(unsafe_code)]

use std::sync::{
    Mutex, MutexGuard, OnceLock,
    atomic::{AtomicPtr, AtomicU8, AtomicU64, Ordering},
};

use nautilus_core::{UUID4, UnixNanos};
use nautilus_model::{
    enums::{OrderSide, TimeInForce},
    identifiers::{AccountId, ClientOrderId, InstrumentId, PositionId, StrategyId, TraderId},
    orders::{MarketOrder, OrderAny},
    types::Quantity,
};
use nautilus_plugin::{
    NAUTILUS_PLUGIN_ABI_VERSION,
    boundary::{BorrowedStr, OwnedBytes, PluginResult, Slice},
    host::{HostContext, HostLogLevel, HostVTable},
    surfaces::commands::{
        CancelAllOrdersCommand, CancelAllOrdersHandle, CancelOrderCommand, CancelOrderHandle,
        CancelOrdersCommand, CancelOrdersHandle, CloseAllPositionsCommand, CloseAllPositionsHandle,
        ClosePositionCommand, ClosePositionHandle, ModifyOrderCommand, ModifyOrderHandle,
        QueryAccountCommand, QueryAccountHandle, QueryOrderCommand, QueryOrderHandle,
        SubmitOrderCommand, SubmitOrderHandle, SubmitOrderListCommand, SubmitOrderListHandle,
    },
};
use rstest::rstest;

// One variant per callable [`HostVTable`] slot. Indexed into the per-hook
// counter and last-context arrays.
#[allow(clippy::enum_variant_names)]
#[repr(usize)]
#[derive(Clone, Copy, Debug)]
enum HostHook {
    ClockNowNs,
    Log,
    CacheInstrument,
    CacheAccount,
    CacheOrder,
    CachePosition,
    CacheOrdersForStrategy,
    CachePositionsForStrategy,
    SubscribeQuotes,
    UnsubscribeQuotes,
    SubscribeTrades,
    UnsubscribeTrades,
    SubscribeBars,
    UnsubscribeBars,
    SubscribeBookDeltas,
    UnsubscribeBookDeltas,
    SubscribeBookAtInterval,
    UnsubscribeBookAtInterval,
    MsgbusPublish,
    SetTimeAlert,
    SetTimer,
    CancelTimer,
    SubmitOrder,
    CancelOrder,
    ModifyOrder,
    SubmitOrderList,
    CancelOrders,
    CancelAllOrders,
    ClosePosition,
    CloseAllPositions,
    QueryAccount,
    QueryOrder,
}

const HOOK_COUNT: usize = HostHook::QueryOrder as usize + 1;
static HOOK_CALLS: [AtomicU64; HOOK_COUNT] = [const { AtomicU64::new(0) }; HOOK_COUNT];
static LAST_CTX: [AtomicPtr<HostContext>; HOOK_COUNT] =
    [const { AtomicPtr::new(std::ptr::null_mut()) }; HOOK_COUNT];

// Recordings used by individual slots to verify scalar arguments cross the
// boundary unchanged. Only the slots that accept scalars touch these.
static LAST_LOG_LEVEL: AtomicU8 = AtomicU8::new(0);
static LAST_TIME_ALERT_NS: AtomicU64 = AtomicU64::new(0);
static LAST_TIMER_INTERVAL_NS: AtomicU64 = AtomicU64::new(0);
static LAST_BOOK_TYPE: AtomicU8 = AtomicU8::new(0);
static LAST_BOOK_DEPTH: AtomicU64 = AtomicU64::new(0);
static LAST_BOOK_INTERVAL_MS: AtomicU64 = AtomicU64::new(0);
static LAST_MANAGED: AtomicU8 = AtomicU8::new(0);
static LAST_ALLOW_PAST: AtomicU8 = AtomicU8::new(0);
static LAST_FIRE_IMMEDIATELY: AtomicU8 = AtomicU8::new(0);
static LAST_PAYLOAD_LEN: AtomicU64 = AtomicU64::new(0);

// Serialises hook-dispatch test cases so the shared counters are not
// contaminated by parallel runs.
fn dispatch_lock() -> MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn reset_all() {
    for c in &HOOK_CALLS {
        c.store(0, Ordering::SeqCst);
    }

    for c in &LAST_CTX {
        c.store(std::ptr::null_mut(), Ordering::SeqCst);
    }

    LAST_LOG_LEVEL.store(0, Ordering::SeqCst);
    LAST_TIME_ALERT_NS.store(0, Ordering::SeqCst);
    LAST_TIMER_INTERVAL_NS.store(0, Ordering::SeqCst);
    LAST_BOOK_TYPE.store(0, Ordering::SeqCst);
    LAST_BOOK_DEPTH.store(0, Ordering::SeqCst);
    LAST_BOOK_INTERVAL_MS.store(0, Ordering::SeqCst);
    LAST_MANAGED.store(0, Ordering::SeqCst);
    LAST_ALLOW_PAST.store(0, Ordering::SeqCst);
    LAST_FIRE_IMMEDIATELY.store(0, Ordering::SeqCst);
    LAST_PAYLOAD_LEN.store(0, Ordering::SeqCst);
}

fn record(ctx: *const HostContext, hook: HostHook) {
    let idx = hook as usize;
    HOOK_CALLS[idx].fetch_add(1, Ordering::SeqCst);
    LAST_CTX[idx].store(ctx.cast_mut(), Ordering::SeqCst);
}

fn assert_only_hook(expected: HostHook) {
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

fn assert_ctx(hook: HostHook, expected: *const HostContext) {
    let last = LAST_CTX[hook as usize].load(Ordering::SeqCst).cast_const();
    assert!(
        std::ptr::eq(last, expected),
        "host context not threaded through to {hook:?}: expected {expected:?}, was {last:?}",
    );
}

unsafe extern "C" fn test_clock_now_ns() -> u64 {
    HOOK_CALLS[HostHook::ClockNowNs as usize].fetch_add(1, Ordering::SeqCst);
    // ClockNowNs has no ctx parameter; do not touch LAST_CTX so its
    // assertion is skipped for this slot.
    0x00C0_FFEE_u64
}

unsafe extern "C" fn test_log(
    level: HostLogLevel,
    _target: BorrowedStr<'_>,
    _message: BorrowedStr<'_>,
) {
    HOOK_CALLS[HostHook::Log as usize].fetch_add(1, Ordering::SeqCst);
    LAST_LOG_LEVEL.store(level as u8, Ordering::SeqCst);
}

macro_rules! bytes_handler {
    ($name:ident, $hook:expr) => {
        unsafe extern "C" fn $name(
            ctx: *const HostContext,
            _arg: BorrowedStr<'_>,
        ) -> PluginResult<OwnedBytes> {
            record(ctx, $hook);
            PluginResult::Ok(OwnedBytes::empty())
        }
    };
}

bytes_handler!(test_cache_instrument, HostHook::CacheInstrument);
bytes_handler!(test_cache_account, HostHook::CacheAccount);
bytes_handler!(test_cache_order, HostHook::CacheOrder);
bytes_handler!(test_cache_position, HostHook::CachePosition);
bytes_handler!(
    test_cache_orders_for_strategy,
    HostHook::CacheOrdersForStrategy
);
bytes_handler!(
    test_cache_positions_for_strategy,
    HostHook::CachePositionsForStrategy
);

macro_rules! subscription_handler {
    ($name:ident, $hook:expr) => {
        unsafe extern "C" fn $name(
            ctx: *const HostContext,
            _id: BorrowedStr<'_>,
            _client_id: BorrowedStr<'_>,
            _params_json: BorrowedStr<'_>,
        ) -> PluginResult<()> {
            record(ctx, $hook);
            PluginResult::Ok(())
        }
    };
}

subscription_handler!(test_subscribe_quotes, HostHook::SubscribeQuotes);
subscription_handler!(test_unsubscribe_quotes, HostHook::UnsubscribeQuotes);
subscription_handler!(test_subscribe_trades, HostHook::SubscribeTrades);
subscription_handler!(test_unsubscribe_trades, HostHook::UnsubscribeTrades);
subscription_handler!(test_subscribe_bars, HostHook::SubscribeBars);
subscription_handler!(test_unsubscribe_bars, HostHook::UnsubscribeBars);
subscription_handler!(
    test_unsubscribe_book_deltas,
    HostHook::UnsubscribeBookDeltas
);

unsafe extern "C" fn test_subscribe_book_deltas(
    ctx: *const HostContext,
    _instrument_id: BorrowedStr<'_>,
    book_type: u8,
    depth: usize,
    _client_id: BorrowedStr<'_>,
    managed: u8,
    _params_json: BorrowedStr<'_>,
) -> PluginResult<()> {
    record(ctx, HostHook::SubscribeBookDeltas);
    LAST_BOOK_TYPE.store(book_type, Ordering::SeqCst);
    LAST_BOOK_DEPTH.store(
        u64::try_from(depth).expect("book depth fits in u64"),
        Ordering::SeqCst,
    );
    LAST_MANAGED.store(managed, Ordering::SeqCst);
    PluginResult::Ok(())
}

unsafe extern "C" fn test_subscribe_book_at_interval(
    ctx: *const HostContext,
    _instrument_id: BorrowedStr<'_>,
    book_type: u8,
    depth: usize,
    interval_ms: usize,
    _client_id: BorrowedStr<'_>,
    _params_json: BorrowedStr<'_>,
) -> PluginResult<()> {
    record(ctx, HostHook::SubscribeBookAtInterval);
    LAST_BOOK_TYPE.store(book_type, Ordering::SeqCst);
    LAST_BOOK_DEPTH.store(
        u64::try_from(depth).expect("book depth fits in u64"),
        Ordering::SeqCst,
    );
    LAST_BOOK_INTERVAL_MS.store(
        u64::try_from(interval_ms).expect("interval fits in u64"),
        Ordering::SeqCst,
    );
    PluginResult::Ok(())
}

unsafe extern "C" fn test_unsubscribe_book_at_interval(
    ctx: *const HostContext,
    _instrument_id: BorrowedStr<'_>,
    interval_ms: usize,
    _client_id: BorrowedStr<'_>,
    _params_json: BorrowedStr<'_>,
) -> PluginResult<()> {
    record(ctx, HostHook::UnsubscribeBookAtInterval);
    LAST_BOOK_INTERVAL_MS.store(
        u64::try_from(interval_ms).expect("interval fits in u64"),
        Ordering::SeqCst,
    );
    PluginResult::Ok(())
}

unsafe extern "C" fn test_msgbus_publish(
    ctx: *const HostContext,
    _topic: BorrowedStr<'_>,
    payload: Slice<'_, u8>,
) -> PluginResult<()> {
    record(ctx, HostHook::MsgbusPublish);
    LAST_PAYLOAD_LEN.store(
        u64::try_from(payload.len).expect("payload length fits in u64"),
        Ordering::SeqCst,
    );
    PluginResult::Ok(())
}

unsafe extern "C" fn test_set_time_alert(
    ctx: *const HostContext,
    _name: BorrowedStr<'_>,
    alert_time_ns: u64,
    allow_past: u8,
) -> PluginResult<()> {
    record(ctx, HostHook::SetTimeAlert);
    LAST_TIME_ALERT_NS.store(alert_time_ns, Ordering::SeqCst);
    LAST_ALLOW_PAST.store(allow_past, Ordering::SeqCst);
    PluginResult::Ok(())
}

unsafe extern "C" fn test_set_timer(
    ctx: *const HostContext,
    _name: BorrowedStr<'_>,
    interval_ns: u64,
    _start_time_ns: u64,
    _stop_time_ns: u64,
    allow_past: u8,
    fire_immediately: u8,
) -> PluginResult<()> {
    record(ctx, HostHook::SetTimer);
    LAST_TIMER_INTERVAL_NS.store(interval_ns, Ordering::SeqCst);
    LAST_ALLOW_PAST.store(allow_past, Ordering::SeqCst);
    LAST_FIRE_IMMEDIATELY.store(fire_immediately, Ordering::SeqCst);
    PluginResult::Ok(())
}

unsafe extern "C" fn test_cancel_timer(
    ctx: *const HostContext,
    _name: BorrowedStr<'_>,
) -> PluginResult<()> {
    record(ctx, HostHook::CancelTimer);
    PluginResult::Ok(())
}

unsafe extern "C" fn test_submit_order(
    ctx: *const HostContext,
    _command: *const SubmitOrderHandle,
) -> PluginResult<()> {
    record(ctx, HostHook::SubmitOrder);
    PluginResult::Ok(())
}

unsafe extern "C" fn test_cancel_order(
    ctx: *const HostContext,
    _command: *const CancelOrderHandle,
) -> PluginResult<()> {
    record(ctx, HostHook::CancelOrder);
    PluginResult::Ok(())
}

unsafe extern "C" fn test_modify_order(
    ctx: *const HostContext,
    _command: *const ModifyOrderHandle,
) -> PluginResult<()> {
    record(ctx, HostHook::ModifyOrder);
    PluginResult::Ok(())
}

unsafe extern "C" fn test_submit_order_list(
    ctx: *const HostContext,
    _command: *const SubmitOrderListHandle,
) -> PluginResult<()> {
    record(ctx, HostHook::SubmitOrderList);
    PluginResult::Ok(())
}

unsafe extern "C" fn test_cancel_orders(
    ctx: *const HostContext,
    _command: *const CancelOrdersHandle,
) -> PluginResult<()> {
    record(ctx, HostHook::CancelOrders);
    PluginResult::Ok(())
}

unsafe extern "C" fn test_cancel_all_orders(
    ctx: *const HostContext,
    _command: *const CancelAllOrdersHandle,
) -> PluginResult<()> {
    record(ctx, HostHook::CancelAllOrders);
    PluginResult::Ok(())
}

unsafe extern "C" fn test_close_position(
    ctx: *const HostContext,
    _command: *const ClosePositionHandle,
) -> PluginResult<()> {
    record(ctx, HostHook::ClosePosition);
    PluginResult::Ok(())
}

unsafe extern "C" fn test_close_all_positions(
    ctx: *const HostContext,
    _command: *const CloseAllPositionsHandle,
) -> PluginResult<()> {
    record(ctx, HostHook::CloseAllPositions);
    PluginResult::Ok(())
}

unsafe extern "C" fn test_query_account(
    ctx: *const HostContext,
    _command: *const QueryAccountHandle,
) -> PluginResult<()> {
    record(ctx, HostHook::QueryAccount);
    PluginResult::Ok(())
}

unsafe extern "C" fn test_query_order(
    ctx: *const HostContext,
    _command: *const QueryOrderHandle,
) -> PluginResult<()> {
    record(ctx, HostHook::QueryOrder);
    PluginResult::Ok(())
}

fn make_market_order() -> OrderAny {
    OrderAny::Market(MarketOrder::new(
        TraderId::from("TRADER-001"),
        StrategyId::from("S-001"),
        InstrumentId::from("ETH-USDT.BINANCE"),
        ClientOrderId::from("O-1"),
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
    submit_order_list: test_submit_order_list,
    cancel_orders: test_cancel_orders,
    cancel_all_orders: test_cancel_all_orders,
    close_position: test_close_position,
    close_all_positions: test_close_all_positions,
    query_account: test_query_account,
    query_order: test_query_order,
};

// Sentinel non-null pointer used as the plug-in's host context in tests.
// The test handlers never dereference the context, so any non-null value
// works as a routing identity check.
#[repr(transparent)]
struct HostContextPad {
    _filler: u8,
}
static SENTINEL_CTX: HostContextPad = HostContextPad { _filler: 0 };

fn sentinel_ctx() -> *const HostContext {
    std::ptr::from_ref(&SENTINEL_CTX).cast::<HostContext>()
}

fn empty_str() -> BorrowedStr<'static> {
    BorrowedStr::empty()
}

#[rstest]
fn host_abi_version_matches_compiled_abi() {
    assert!(TEST_HOST.matches_compiled_abi());
}

#[rstest]
fn clock_now_ns_slot_invokes_bound_handler() {
    let _g = dispatch_lock();
    reset_all();
    // SAFETY: TEST_HOST is process-lifetime static.
    let ns = unsafe { (TEST_HOST.clock_now_ns)() };
    assert_eq!(ns, 0x00C0_FFEE_u64);
    assert_only_hook(HostHook::ClockNowNs);
}

#[rstest]
#[case::error(HostLogLevel::Error)]
#[case::warn(HostLogLevel::Warn)]
#[case::info(HostLogLevel::Info)]
#[case::debug(HostLogLevel::Debug)]
#[case::trace(HostLogLevel::Trace)]
fn log_slot_invokes_bound_handler_with_level(#[case] level: HostLogLevel) {
    let _g = dispatch_lock();
    reset_all();
    // SAFETY: TEST_HOST is process-lifetime static; static strings outlive the call.
    unsafe {
        (TEST_HOST.log)(
            level,
            BorrowedStr::from_str("nautilus_test"),
            BorrowedStr::from_str("hello"),
        );
    }
    assert_only_hook(HostHook::Log);
    assert_eq!(LAST_LOG_LEVEL.load(Ordering::SeqCst), level as u8);
}

#[rstest]
fn cache_instrument_slot_invokes_bound_handler() {
    let _g = dispatch_lock();
    reset_all();
    let ctx = sentinel_ctx();
    // SAFETY: TEST_HOST is process-lifetime static.
    let r = unsafe { (TEST_HOST.cache_instrument)(ctx, BorrowedStr::from_str("ETH-USDT.BINANCE")) };
    r.into_result().expect("cache_instrument");
    assert_only_hook(HostHook::CacheInstrument);
    assert_ctx(HostHook::CacheInstrument, ctx);
}

#[rstest]
fn cache_account_slot_invokes_bound_handler() {
    let _g = dispatch_lock();
    reset_all();
    let ctx = sentinel_ctx();
    // SAFETY: TEST_HOST is process-lifetime static.
    let r = unsafe { (TEST_HOST.cache_account)(ctx, BorrowedStr::from_str("BINANCE-001")) };
    r.into_result().expect("cache_account");
    assert_only_hook(HostHook::CacheAccount);
    assert_ctx(HostHook::CacheAccount, ctx);
}

#[rstest]
fn cache_order_slot_invokes_bound_handler() {
    let _g = dispatch_lock();
    reset_all();
    let ctx = sentinel_ctx();
    // SAFETY: TEST_HOST is process-lifetime static.
    let r = unsafe { (TEST_HOST.cache_order)(ctx, BorrowedStr::from_str("O-1")) };
    r.into_result().expect("cache_order");
    assert_only_hook(HostHook::CacheOrder);
    assert_ctx(HostHook::CacheOrder, ctx);
}

#[rstest]
fn cache_position_slot_invokes_bound_handler() {
    let _g = dispatch_lock();
    reset_all();
    let ctx = sentinel_ctx();
    // SAFETY: TEST_HOST is process-lifetime static.
    let r = unsafe { (TEST_HOST.cache_position)(ctx, BorrowedStr::from_str("P-1")) };
    r.into_result().expect("cache_position");
    assert_only_hook(HostHook::CachePosition);
    assert_ctx(HostHook::CachePosition, ctx);
}

#[rstest]
fn cache_orders_for_strategy_slot_invokes_bound_handler() {
    let _g = dispatch_lock();
    reset_all();
    let ctx = sentinel_ctx();
    // SAFETY: TEST_HOST is process-lifetime static.
    let r = unsafe { (TEST_HOST.cache_orders_for_strategy)(ctx, empty_str()) };
    r.into_result().expect("cache_orders_for_strategy");
    assert_only_hook(HostHook::CacheOrdersForStrategy);
    assert_ctx(HostHook::CacheOrdersForStrategy, ctx);
}

#[rstest]
fn cache_positions_for_strategy_slot_invokes_bound_handler() {
    let _g = dispatch_lock();
    reset_all();
    let ctx = sentinel_ctx();
    // SAFETY: TEST_HOST is process-lifetime static.
    let r = unsafe { (TEST_HOST.cache_positions_for_strategy)(ctx, empty_str()) };
    r.into_result().expect("cache_positions_for_strategy");
    assert_only_hook(HostHook::CachePositionsForStrategy);
    assert_ctx(HostHook::CachePositionsForStrategy, ctx);
}

#[rstest]
fn subscribe_quotes_slot_invokes_bound_handler() {
    let _g = dispatch_lock();
    reset_all();
    let ctx = sentinel_ctx();
    // SAFETY: TEST_HOST is process-lifetime static.
    let r = unsafe {
        (TEST_HOST.subscribe_quotes)(
            ctx,
            BorrowedStr::from_str("ETH-USDT.BINANCE"),
            empty_str(),
            empty_str(),
        )
    };
    r.into_result().expect("subscribe_quotes");
    assert_only_hook(HostHook::SubscribeQuotes);
    assert_ctx(HostHook::SubscribeQuotes, ctx);
}

#[rstest]
fn unsubscribe_quotes_slot_invokes_bound_handler() {
    let _g = dispatch_lock();
    reset_all();
    let ctx = sentinel_ctx();
    // SAFETY: TEST_HOST is process-lifetime static.
    let r = unsafe {
        (TEST_HOST.unsubscribe_quotes)(
            ctx,
            BorrowedStr::from_str("ETH-USDT.BINANCE"),
            empty_str(),
            empty_str(),
        )
    };
    r.into_result().expect("unsubscribe_quotes");
    assert_only_hook(HostHook::UnsubscribeQuotes);
    assert_ctx(HostHook::UnsubscribeQuotes, ctx);
}

#[rstest]
fn subscribe_trades_slot_invokes_bound_handler() {
    let _g = dispatch_lock();
    reset_all();
    let ctx = sentinel_ctx();
    // SAFETY: TEST_HOST is process-lifetime static.
    let r = unsafe {
        (TEST_HOST.subscribe_trades)(
            ctx,
            BorrowedStr::from_str("ETH-USDT.BINANCE"),
            empty_str(),
            empty_str(),
        )
    };
    r.into_result().expect("subscribe_trades");
    assert_only_hook(HostHook::SubscribeTrades);
    assert_ctx(HostHook::SubscribeTrades, ctx);
}

#[rstest]
fn unsubscribe_trades_slot_invokes_bound_handler() {
    let _g = dispatch_lock();
    reset_all();
    let ctx = sentinel_ctx();
    // SAFETY: TEST_HOST is process-lifetime static.
    let r = unsafe {
        (TEST_HOST.unsubscribe_trades)(
            ctx,
            BorrowedStr::from_str("ETH-USDT.BINANCE"),
            empty_str(),
            empty_str(),
        )
    };
    r.into_result().expect("unsubscribe_trades");
    assert_only_hook(HostHook::UnsubscribeTrades);
    assert_ctx(HostHook::UnsubscribeTrades, ctx);
}

#[rstest]
fn subscribe_bars_slot_invokes_bound_handler() {
    let _g = dispatch_lock();
    reset_all();
    let ctx = sentinel_ctx();
    // SAFETY: TEST_HOST is process-lifetime static.
    let r = unsafe {
        (TEST_HOST.subscribe_bars)(
            ctx,
            BorrowedStr::from_str("ETH-USDT.BINANCE-1-MINUTE-LAST-EXTERNAL"),
            empty_str(),
            empty_str(),
        )
    };
    r.into_result().expect("subscribe_bars");
    assert_only_hook(HostHook::SubscribeBars);
    assert_ctx(HostHook::SubscribeBars, ctx);
}

#[rstest]
fn unsubscribe_bars_slot_invokes_bound_handler() {
    let _g = dispatch_lock();
    reset_all();
    let ctx = sentinel_ctx();
    // SAFETY: TEST_HOST is process-lifetime static.
    let r = unsafe {
        (TEST_HOST.unsubscribe_bars)(
            ctx,
            BorrowedStr::from_str("ETH-USDT.BINANCE-1-MINUTE-LAST-EXTERNAL"),
            empty_str(),
            empty_str(),
        )
    };
    r.into_result().expect("unsubscribe_bars");
    assert_only_hook(HostHook::UnsubscribeBars);
    assert_ctx(HostHook::UnsubscribeBars, ctx);
}

#[rstest]
fn subscribe_book_deltas_slot_invokes_bound_handler_with_book_args() {
    let _g = dispatch_lock();
    reset_all();
    let ctx = sentinel_ctx();
    // SAFETY: TEST_HOST is process-lifetime static.
    let r = unsafe {
        (TEST_HOST.subscribe_book_deltas)(
            ctx,
            BorrowedStr::from_str("ETH-USDT.BINANCE"),
            2,  // book_type sentinel
            10, // depth
            empty_str(),
            1, // managed
            empty_str(),
        )
    };
    r.into_result().expect("subscribe_book_deltas");
    assert_only_hook(HostHook::SubscribeBookDeltas);
    assert_ctx(HostHook::SubscribeBookDeltas, ctx);
    assert_eq!(LAST_BOOK_TYPE.load(Ordering::SeqCst), 2);
    assert_eq!(LAST_BOOK_DEPTH.load(Ordering::SeqCst), 10);
    assert_eq!(LAST_MANAGED.load(Ordering::SeqCst), 1);
}

#[rstest]
fn unsubscribe_book_deltas_slot_invokes_bound_handler() {
    let _g = dispatch_lock();
    reset_all();
    let ctx = sentinel_ctx();
    // SAFETY: TEST_HOST is process-lifetime static.
    let r = unsafe {
        (TEST_HOST.unsubscribe_book_deltas)(
            ctx,
            BorrowedStr::from_str("ETH-USDT.BINANCE"),
            empty_str(),
            empty_str(),
        )
    };
    r.into_result().expect("unsubscribe_book_deltas");
    assert_only_hook(HostHook::UnsubscribeBookDeltas);
    assert_ctx(HostHook::UnsubscribeBookDeltas, ctx);
}

#[rstest]
fn subscribe_book_at_interval_slot_invokes_bound_handler_with_interval() {
    let _g = dispatch_lock();
    reset_all();
    let ctx = sentinel_ctx();
    // SAFETY: TEST_HOST is process-lifetime static.
    let r = unsafe {
        (TEST_HOST.subscribe_book_at_interval)(
            ctx,
            BorrowedStr::from_str("ETH-USDT.BINANCE"),
            3,   // book_type sentinel
            25,  // depth
            500, // interval_ms
            empty_str(),
            empty_str(),
        )
    };
    r.into_result().expect("subscribe_book_at_interval");
    assert_only_hook(HostHook::SubscribeBookAtInterval);
    assert_ctx(HostHook::SubscribeBookAtInterval, ctx);
    assert_eq!(LAST_BOOK_TYPE.load(Ordering::SeqCst), 3);
    assert_eq!(LAST_BOOK_DEPTH.load(Ordering::SeqCst), 25);
    assert_eq!(LAST_BOOK_INTERVAL_MS.load(Ordering::SeqCst), 500);
}

#[rstest]
fn unsubscribe_book_at_interval_slot_invokes_bound_handler_with_interval() {
    let _g = dispatch_lock();
    reset_all();
    let ctx = sentinel_ctx();
    // SAFETY: TEST_HOST is process-lifetime static.
    let r = unsafe {
        (TEST_HOST.unsubscribe_book_at_interval)(
            ctx,
            BorrowedStr::from_str("ETH-USDT.BINANCE"),
            500,
            empty_str(),
            empty_str(),
        )
    };
    r.into_result().expect("unsubscribe_book_at_interval");
    assert_only_hook(HostHook::UnsubscribeBookAtInterval);
    assert_ctx(HostHook::UnsubscribeBookAtInterval, ctx);
    assert_eq!(LAST_BOOK_INTERVAL_MS.load(Ordering::SeqCst), 500);
}

#[rstest]
fn msgbus_publish_slot_invokes_bound_handler_with_payload_len() {
    let _g = dispatch_lock();
    reset_all();
    let ctx = sentinel_ctx();
    let payload = [1u8, 2, 3, 4, 5];
    let payload_slice = Slice::from_slice(&payload);
    // SAFETY: TEST_HOST is process-lifetime static; payload outlives the call.
    let r =
        unsafe { (TEST_HOST.msgbus_publish)(ctx, BorrowedStr::from_str("topic"), payload_slice) };
    r.into_result().expect("msgbus_publish");
    assert_only_hook(HostHook::MsgbusPublish);
    assert_ctx(HostHook::MsgbusPublish, ctx);
    assert_eq!(
        LAST_PAYLOAD_LEN.load(Ordering::SeqCst),
        u64::try_from(payload.len()).expect("payload length fits in u64")
    );
}

#[rstest]
fn set_time_alert_slot_invokes_bound_handler() {
    let _g = dispatch_lock();
    reset_all();
    let ctx = sentinel_ctx();
    // SAFETY: TEST_HOST is process-lifetime static.
    let r = unsafe { (TEST_HOST.set_time_alert)(ctx, BorrowedStr::from_str("alert-A"), 12_345, 1) };
    r.into_result().expect("set_time_alert");
    assert_only_hook(HostHook::SetTimeAlert);
    assert_ctx(HostHook::SetTimeAlert, ctx);
    assert_eq!(LAST_TIME_ALERT_NS.load(Ordering::SeqCst), 12_345);
    assert_eq!(LAST_ALLOW_PAST.load(Ordering::SeqCst), 1);
}

#[rstest]
fn set_timer_slot_invokes_bound_handler() {
    let _g = dispatch_lock();
    reset_all();
    let ctx = sentinel_ctx();
    // SAFETY: TEST_HOST is process-lifetime static.
    let r = unsafe {
        (TEST_HOST.set_timer)(ctx, BorrowedStr::from_str("timer-A"), 1_000_000, 0, 0, 1, 1)
    };
    r.into_result().expect("set_timer");
    assert_only_hook(HostHook::SetTimer);
    assert_ctx(HostHook::SetTimer, ctx);
    assert_eq!(LAST_TIMER_INTERVAL_NS.load(Ordering::SeqCst), 1_000_000);
    assert_eq!(LAST_ALLOW_PAST.load(Ordering::SeqCst), 1);
    assert_eq!(LAST_FIRE_IMMEDIATELY.load(Ordering::SeqCst), 1);
}

#[rstest]
fn cancel_timer_slot_invokes_bound_handler() {
    let _g = dispatch_lock();
    reset_all();
    let ctx = sentinel_ctx();
    // SAFETY: TEST_HOST is process-lifetime static.
    let r = unsafe { (TEST_HOST.cancel_timer)(ctx, BorrowedStr::from_str("timer-A")) };
    r.into_result().expect("cancel_timer");
    assert_only_hook(HostHook::CancelTimer);
    assert_ctx(HostHook::CancelTimer, ctx);
}

#[rstest]
fn submit_order_slot_invokes_bound_handler() {
    let _g = dispatch_lock();
    reset_all();
    let ctx = sentinel_ctx();
    let handle = SubmitOrderHandle::new(SubmitOrderCommand::new(
        make_market_order(),
        None,
        None,
        None,
    ));
    // SAFETY: TEST_HOST is process-lifetime static; handle outlives the call.
    let r = unsafe { (TEST_HOST.submit_order)(ctx, &raw const handle) };
    r.into_result().expect("submit_order");
    assert_only_hook(HostHook::SubmitOrder);
    assert_ctx(HostHook::SubmitOrder, ctx);
}

#[rstest]
fn cancel_order_slot_invokes_bound_handler() {
    let _g = dispatch_lock();
    reset_all();
    let ctx = sentinel_ctx();
    let handle = CancelOrderHandle::new(CancelOrderCommand::new(
        ClientOrderId::from("O-1"),
        None,
        None,
    ));
    // SAFETY: TEST_HOST is process-lifetime static; handle outlives the call.
    let r = unsafe { (TEST_HOST.cancel_order)(ctx, &raw const handle) };
    r.into_result().expect("cancel_order");
    assert_only_hook(HostHook::CancelOrder);
    assert_ctx(HostHook::CancelOrder, ctx);
}

#[rstest]
fn modify_order_slot_invokes_bound_handler() {
    let _g = dispatch_lock();
    reset_all();
    let ctx = sentinel_ctx();
    let handle = ModifyOrderHandle::new(ModifyOrderCommand::new(
        ClientOrderId::from("O-1"),
        None,
        None,
        None,
        None,
        None,
    ));
    // SAFETY: TEST_HOST is process-lifetime static; handle outlives the call.
    let r = unsafe { (TEST_HOST.modify_order)(ctx, &raw const handle) };
    r.into_result().expect("modify_order");
    assert_only_hook(HostHook::ModifyOrder);
    assert_ctx(HostHook::ModifyOrder, ctx);
}

#[rstest]
fn submit_order_list_slot_invokes_bound_handler() {
    let _g = dispatch_lock();
    reset_all();
    let ctx = sentinel_ctx();
    let handle = SubmitOrderListHandle::new(SubmitOrderListCommand::new(
        vec![make_market_order()],
        None,
        None,
        None,
    ));
    // SAFETY: TEST_HOST is process-lifetime static; handle outlives the call.
    let r = unsafe { (TEST_HOST.submit_order_list)(ctx, &raw const handle) };
    r.into_result().expect("submit_order_list");
    assert_only_hook(HostHook::SubmitOrderList);
    assert_ctx(HostHook::SubmitOrderList, ctx);
}

#[rstest]
fn cancel_orders_slot_invokes_bound_handler() {
    let _g = dispatch_lock();
    reset_all();
    let ctx = sentinel_ctx();
    let handle = CancelOrdersHandle::new(CancelOrdersCommand::new(
        vec![ClientOrderId::from("O-1")],
        None,
        None,
    ));
    // SAFETY: TEST_HOST is process-lifetime static; handle outlives the call.
    let r = unsafe { (TEST_HOST.cancel_orders)(ctx, &raw const handle) };
    r.into_result().expect("cancel_orders");
    assert_only_hook(HostHook::CancelOrders);
    assert_ctx(HostHook::CancelOrders, ctx);
}

#[rstest]
fn cancel_all_orders_slot_invokes_bound_handler() {
    let _g = dispatch_lock();
    reset_all();
    let ctx = sentinel_ctx();
    let handle = CancelAllOrdersHandle::new(CancelAllOrdersCommand::new(
        InstrumentId::from("ETH-USDT.BINANCE"),
        None,
        None,
        None,
    ));
    // SAFETY: TEST_HOST is process-lifetime static; handle outlives the call.
    let r = unsafe { (TEST_HOST.cancel_all_orders)(ctx, &raw const handle) };
    r.into_result().expect("cancel_all_orders");
    assert_only_hook(HostHook::CancelAllOrders);
    assert_ctx(HostHook::CancelAllOrders, ctx);
}

#[rstest]
fn close_position_slot_invokes_bound_handler() {
    let _g = dispatch_lock();
    reset_all();
    let ctx = sentinel_ctx();
    let handle = ClosePositionHandle::new(ClosePositionCommand::new(
        PositionId::from("P-001"),
        None,
        None,
        None,
        None,
        None,
    ));
    // SAFETY: TEST_HOST is process-lifetime static; handle outlives the call.
    let r = unsafe { (TEST_HOST.close_position)(ctx, &raw const handle) };
    r.into_result().expect("close_position");
    assert_only_hook(HostHook::ClosePosition);
    assert_ctx(HostHook::ClosePosition, ctx);
}

#[rstest]
fn close_all_positions_slot_invokes_bound_handler() {
    let _g = dispatch_lock();
    reset_all();
    let ctx = sentinel_ctx();
    let handle = CloseAllPositionsHandle::new(CloseAllPositionsCommand::new(
        InstrumentId::from("ETH-USDT.BINANCE"),
        None,
        None,
        None,
        None,
        None,
        None,
    ));
    // SAFETY: TEST_HOST is process-lifetime static; handle outlives the call.
    let r = unsafe { (TEST_HOST.close_all_positions)(ctx, &raw const handle) };
    r.into_result().expect("close_all_positions");
    assert_only_hook(HostHook::CloseAllPositions);
    assert_ctx(HostHook::CloseAllPositions, ctx);
}

#[rstest]
fn query_account_slot_invokes_bound_handler() {
    let _g = dispatch_lock();
    reset_all();
    let ctx = sentinel_ctx();
    let handle = QueryAccountHandle::new(QueryAccountCommand::new(
        AccountId::from("BINANCE-001"),
        None,
        None,
    ));
    // SAFETY: TEST_HOST is process-lifetime static; handle outlives the call.
    let r = unsafe { (TEST_HOST.query_account)(ctx, &raw const handle) };
    r.into_result().expect("query_account");
    assert_only_hook(HostHook::QueryAccount);
    assert_ctx(HostHook::QueryAccount, ctx);
}

#[rstest]
fn query_order_slot_invokes_bound_handler() {
    let _g = dispatch_lock();
    reset_all();
    let ctx = sentinel_ctx();
    let handle = QueryOrderHandle::new(QueryOrderCommand::new(
        ClientOrderId::from("O-1"),
        None,
        None,
    ));
    // SAFETY: TEST_HOST is process-lifetime static; handle outlives the call.
    let r = unsafe { (TEST_HOST.query_order)(ctx, &raw const handle) };
    r.into_result().expect("query_order");
    assert_only_hook(HostHook::QueryOrder);
    assert_ctx(HostHook::QueryOrder, ctx);
}
