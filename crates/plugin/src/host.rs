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

//! Host-side function table given to plug-ins for re-entrant callbacks.
//!
//! The surface stays explicit and versioned: every host service is a concrete
//! function pointer, and every added method requires an ABI bump. This avoids
//! exposing `Arc<MessageBus>` or any `dyn Trait` across the boundary.

#![allow(unsafe_code)]

use crate::{
    NAUTILUS_PLUGIN_ABI_VERSION,
    boundary::{BorrowedStr, OwnedBytes, PluginResult, Slice},
    surfaces::commands::{
        CancelAllOrdersHandle, CancelOrderHandle, CancelOrdersHandle, CloseAllPositionsHandle,
        ClosePositionHandle, ModifyOrderHandle, QueryAccountHandle, QueryOrderHandle,
        SubmitOrderHandle, SubmitOrderListHandle,
    },
};

/// Log levels mirrored from the host's `log` crate without dragging the
/// crate into the boundary type.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HostLogLevel {
    Error = 1,
    Warn = 2,
    Info = 3,
    Debug = 4,
    Trace = 5,
}

/// Opaque per-instance context the host supplies at plug-in creation.
///
/// The host attaches a unique context to each actor or strategy instance so
/// that host services that need attribution (logging targets, order
/// commands, cache scoping) can resolve back to the correct caller without
/// the plug-in needing to know any host identifiers. The plug-in only ever
/// passes the pointer through to the relevant [`HostVTable`] entry.
#[repr(C)]
pub struct HostContext {
    _opaque: [u8; 0],
}

/// Function table the host passes to every plug-in at load time.
///
/// All function pointers are non-null and stable for the process lifetime.
/// Plug-ins stash the pointer and call back through it whenever they need
/// host services. Adding a method is a breaking ABI change and requires a
/// [`NAUTILUS_PLUGIN_ABI_VERSION`] bump.
#[repr(C)]
pub struct HostVTable {
    /// ABI version of this vtable. Must equal [`NAUTILUS_PLUGIN_ABI_VERSION`].
    pub abi_version: u32,

    /// Returns the host's monotonic clock reading in UNIX nanoseconds.
    pub clock_now_ns: unsafe extern "C" fn() -> u64,

    /// Emits a log line to the host's logger.
    ///
    /// `target` is the log target (e.g. plug-in name), `message` is the body.
    pub log: unsafe extern "C" fn(
        level: HostLogLevel,
        target: BorrowedStr<'_>,
        message: BorrowedStr<'_>,
    ),

    /// Returns the JSON-encoded instrument snapshot for `instrument_id`.
    ///
    /// Empty bytes mean the cache had no matching instrument.
    pub cache_instrument: unsafe extern "C" fn(
        ctx: *const HostContext,
        instrument_id: BorrowedStr<'_>,
    ) -> PluginResult<OwnedBytes>,

    /// Returns the JSON-encoded account snapshot for `account_id`.
    ///
    /// Empty bytes mean the cache had no matching account.
    pub cache_account: unsafe extern "C" fn(
        ctx: *const HostContext,
        account_id: BorrowedStr<'_>,
    ) -> PluginResult<OwnedBytes>,

    /// Returns the JSON-encoded order snapshot for `client_order_id`.
    ///
    /// Empty bytes mean the cache had no matching order.
    pub cache_order: unsafe extern "C" fn(
        ctx: *const HostContext,
        client_order_id: BorrowedStr<'_>,
    ) -> PluginResult<OwnedBytes>,

    /// Returns the JSON-encoded position snapshot for `position_id`.
    ///
    /// Empty bytes mean the cache had no matching position.
    pub cache_position: unsafe extern "C" fn(
        ctx: *const HostContext,
        position_id: BorrowedStr<'_>,
    ) -> PluginResult<OwnedBytes>,

    /// Returns JSON-encoded order snapshots for the requested strategy.
    ///
    /// Passing an empty `strategy_id` uses the calling strategy's own ID.
    pub cache_orders_for_strategy: unsafe extern "C" fn(
        ctx: *const HostContext,
        strategy_id: BorrowedStr<'_>,
    ) -> PluginResult<OwnedBytes>,

    /// Returns JSON-encoded position snapshots for the requested strategy.
    ///
    /// Passing an empty `strategy_id` uses the calling strategy's own ID.
    pub cache_positions_for_strategy: unsafe extern "C" fn(
        ctx: *const HostContext,
        strategy_id: BorrowedStr<'_>,
    ) -> PluginResult<OwnedBytes>,

    /// Subscribes the calling actor or strategy to quote ticks.
    pub subscribe_quotes: unsafe extern "C" fn(
        ctx: *const HostContext,
        instrument_id: BorrowedStr<'_>,
        client_id: BorrowedStr<'_>,
        params_json: BorrowedStr<'_>,
    ) -> PluginResult<()>,

    /// Unsubscribes the calling actor or strategy from quote ticks.
    pub unsubscribe_quotes: unsafe extern "C" fn(
        ctx: *const HostContext,
        instrument_id: BorrowedStr<'_>,
        client_id: BorrowedStr<'_>,
        params_json: BorrowedStr<'_>,
    ) -> PluginResult<()>,

    /// Subscribes the calling actor or strategy to trade ticks.
    pub subscribe_trades: unsafe extern "C" fn(
        ctx: *const HostContext,
        instrument_id: BorrowedStr<'_>,
        client_id: BorrowedStr<'_>,
        params_json: BorrowedStr<'_>,
    ) -> PluginResult<()>,

    /// Unsubscribes the calling actor or strategy from trade ticks.
    pub unsubscribe_trades: unsafe extern "C" fn(
        ctx: *const HostContext,
        instrument_id: BorrowedStr<'_>,
        client_id: BorrowedStr<'_>,
        params_json: BorrowedStr<'_>,
    ) -> PluginResult<()>,

    /// Subscribes the calling actor or strategy to bars.
    pub subscribe_bars: unsafe extern "C" fn(
        ctx: *const HostContext,
        bar_type: BorrowedStr<'_>,
        client_id: BorrowedStr<'_>,
        params_json: BorrowedStr<'_>,
    ) -> PluginResult<()>,

    /// Unsubscribes the calling actor or strategy from bars.
    pub unsubscribe_bars: unsafe extern "C" fn(
        ctx: *const HostContext,
        bar_type: BorrowedStr<'_>,
        client_id: BorrowedStr<'_>,
        params_json: BorrowedStr<'_>,
    ) -> PluginResult<()>,

    /// Subscribes the calling actor or strategy to order book deltas.
    ///
    /// `book_type` uses the `BookType` discriminant. `depth == 0` means no
    /// depth limit. `managed != 0` requests a managed book subscription.
    pub subscribe_book_deltas: unsafe extern "C" fn(
        ctx: *const HostContext,
        instrument_id: BorrowedStr<'_>,
        book_type: u8,
        depth: usize,
        client_id: BorrowedStr<'_>,
        managed: u8,
        params_json: BorrowedStr<'_>,
    ) -> PluginResult<()>,

    /// Unsubscribes the calling actor or strategy from order book deltas.
    pub unsubscribe_book_deltas: unsafe extern "C" fn(
        ctx: *const HostContext,
        instrument_id: BorrowedStr<'_>,
        client_id: BorrowedStr<'_>,
        params_json: BorrowedStr<'_>,
    ) -> PluginResult<()>,

    /// Subscribes the calling actor or strategy to periodic order book snapshots.
    ///
    /// `book_type` uses the `BookType` discriminant. `depth == 0` means no
    /// depth limit. `interval_ms` must be greater than zero.
    pub subscribe_book_at_interval: unsafe extern "C" fn(
        ctx: *const HostContext,
        instrument_id: BorrowedStr<'_>,
        book_type: u8,
        depth: usize,
        interval_ms: usize,
        client_id: BorrowedStr<'_>,
        params_json: BorrowedStr<'_>,
    ) -> PluginResult<()>,

    /// Unsubscribes the calling actor or strategy from periodic order book snapshots.
    ///
    /// `interval_ms` must be greater than zero.
    pub unsubscribe_book_at_interval: unsafe extern "C" fn(
        ctx: *const HostContext,
        instrument_id: BorrowedStr<'_>,
        interval_ms: usize,
        client_id: BorrowedStr<'_>,
        params_json: BorrowedStr<'_>,
    ) -> PluginResult<()>,

    /// Publishes an arbitrary byte payload on the host message bus.
    ///
    /// The payload is delivered as a `Vec<u8>` to subscribers of `topic`.
    pub msgbus_publish: unsafe extern "C" fn(
        ctx: *const HostContext,
        topic: BorrowedStr<'_>,
        payload: Slice<'_, u8>,
    ) -> PluginResult<()>,

    /// Registers a one-shot time alert on the calling actor or strategy clock.
    pub set_time_alert: unsafe extern "C" fn(
        ctx: *const HostContext,
        name: BorrowedStr<'_>,
        alert_time_ns: u64,
        allow_past: u8,
    ) -> PluginResult<()>,

    /// Registers an interval timer on the calling actor or strategy clock.
    ///
    /// `start_time_ns == 0` and `stop_time_ns == 0` mean no explicit bound.
    pub set_timer: unsafe extern "C" fn(
        ctx: *const HostContext,
        name: BorrowedStr<'_>,
        interval_ns: u64,
        start_time_ns: u64,
        stop_time_ns: u64,
        allow_past: u8,
        fire_immediately: u8,
    ) -> PluginResult<()>,

    /// Cancels a timer on the calling actor or strategy clock.
    pub cancel_timer:
        unsafe extern "C" fn(ctx: *const HostContext, name: BorrowedStr<'_>) -> PluginResult<()>,

    /// Submits an order on behalf of the calling strategy.
    ///
    /// `ctx` is the [`HostContext`] the host passed into the strategy's
    /// `create`. `command` is a boundary-owned [`SubmitOrderHandle`] the
    /// plug-in constructs around the order and its routing/position
    /// metadata. The plug-in owns the box and frees it when this call
    /// returns; the host only borrows the handle for the duration of the
    /// call.
    pub submit_order: unsafe extern "C" fn(
        ctx: *const HostContext,
        command: *const SubmitOrderHandle,
    ) -> PluginResult<()>,

    /// Cancels an in-flight order on behalf of the calling strategy.
    ///
    /// `command` is a boundary-owned [`CancelOrderHandle`] the plug-in
    /// constructs around the cancel parameters (typically `client_order_id`,
    /// optional `client_id`, optional venue params). The plug-in owns the
    /// box and frees it when this call returns; the host only borrows the
    /// handle for the duration of the call.
    pub cancel_order: unsafe extern "C" fn(
        ctx: *const HostContext,
        command: *const CancelOrderHandle,
    ) -> PluginResult<()>,

    /// Modifies an in-flight order on behalf of the calling strategy.
    ///
    /// `command` is a boundary-owned [`ModifyOrderHandle`] the plug-in
    /// constructs around the modify parameters (new quantity, price,
    /// trigger price, etc.). The plug-in owns the box and frees it when
    /// this call returns; the host only borrows the handle for the
    /// duration of the call.
    pub modify_order: unsafe extern "C" fn(
        ctx: *const HostContext,
        command: *const ModifyOrderHandle,
    ) -> PluginResult<()>,

    /// Submits a list of orders as a single batch on behalf of the calling
    /// strategy.
    ///
    /// `command` is a boundary-owned [`SubmitOrderListHandle`] the plug-in
    /// constructs around the order list and optional position id, client
    /// id, and routing params. The host dispatches the batch atomically
    /// through the execution engine.
    pub submit_order_list: unsafe extern "C" fn(
        ctx: *const HostContext,
        command: *const SubmitOrderListHandle,
    ) -> PluginResult<()>,

    /// Cancels every order named in the supplied list on behalf of the
    /// calling strategy.
    ///
    /// `command` is a boundary-owned [`CancelOrdersHandle`] carrying the
    /// `client_order_id` list plus optional client id and routing params.
    pub cancel_orders: unsafe extern "C" fn(
        ctx: *const HostContext,
        command: *const CancelOrdersHandle,
    ) -> PluginResult<()>,

    /// Cancels every open order matching the supplied filter on behalf of
    /// the calling strategy.
    ///
    /// `command` is a boundary-owned [`CancelAllOrdersHandle`] carrying the
    /// `instrument_id` and optional `order_side`, `client_id`, and routing
    /// params. The host scans its cache for matching open orders and
    /// issues the cancels.
    pub cancel_all_orders: unsafe extern "C" fn(
        ctx: *const HostContext,
        command: *const CancelAllOrdersHandle,
    ) -> PluginResult<()>,

    /// Closes the position identified by the command on behalf of the
    /// calling strategy.
    ///
    /// `command` is a boundary-owned [`ClosePositionHandle`] carrying the
    /// `position_id` plus optional `client_id`, `tags`, `time_in_force`,
    /// `reduce_only`, and `quote_quantity`. The host reads the position
    /// from its cache and submits a closing market order through the
    /// strategy's order factory.
    pub close_position: unsafe extern "C" fn(
        ctx: *const HostContext,
        command: *const ClosePositionHandle,
    ) -> PluginResult<()>,

    /// Closes every open position matching the supplied filter on behalf
    /// of the calling strategy.
    ///
    /// `command` is a boundary-owned [`CloseAllPositionsHandle`] carrying
    /// the `instrument_id` plus optional `position_side`, `client_id`,
    /// `tags`, `time_in_force`, `reduce_only`, and `quote_quantity`. The
    /// host scans its cache for matching open positions and submits
    /// closing market orders.
    pub close_all_positions: unsafe extern "C" fn(
        ctx: *const HostContext,
        command: *const CloseAllPositionsHandle,
    ) -> PluginResult<()>,

    /// Queries the venue for the latest snapshot of `account_id` on
    /// behalf of the calling strategy.
    ///
    /// `command` is a boundary-owned [`QueryAccountHandle`] carrying the
    /// `account_id` plus optional `client_id` and routing params. The
    /// result is delivered asynchronously through the host's normal
    /// account-state event flow; this call only fires the query, it does
    /// not return the snapshot inline.
    pub query_account: unsafe extern "C" fn(
        ctx: *const HostContext,
        command: *const QueryAccountHandle,
    ) -> PluginResult<()>,

    /// Queries the venue for the latest snapshot of `client_order_id` on
    /// behalf of the calling strategy.
    ///
    /// `command` is a boundary-owned [`QueryOrderHandle`] carrying the
    /// `client_order_id` plus optional `client_id` and routing params.
    /// The result is delivered asynchronously through the host's normal
    /// order-status event flow; this call only fires the query, it does
    /// not return the snapshot inline.
    pub query_order: unsafe extern "C" fn(
        ctx: *const HostContext,
        command: *const QueryOrderHandle,
    ) -> PluginResult<()>,
}

impl HostVTable {
    /// Asserts that the embedded ABI version matches the compiled-in constant.
    ///
    /// Plug-ins should call this in their `nautilus_plugin_init` body before
    /// trusting any function pointer from the table.
    #[must_use]
    pub fn matches_compiled_abi(&self) -> bool {
        self.abi_version == NAUTILUS_PLUGIN_ABI_VERSION
    }

    /// Reads the clock through the vtable.
    ///
    /// # Safety
    ///
    /// The vtable pointer must originate from the host's `nautilus_plugin_init`
    /// call and the host's library must still be live.
    pub unsafe fn now_ns(&self) -> u64 {
        // SAFETY: caller upholds liveness of the host.
        unsafe { (self.clock_now_ns)() }
    }

    /// Logs `message` at `level` through the vtable.
    ///
    /// # Safety
    ///
    /// See [`HostVTable::now_ns`].
    pub unsafe fn log_message(&self, level: HostLogLevel, target: &str, message: &str) {
        // SAFETY: BorrowedStr is `'a` and outlives this call.
        unsafe {
            (self.log)(
                level,
                BorrowedStr::from_str(target),
                BorrowedStr::from_str(message),
            );
        }
    }
}

/// SAFETY: function pointers are thread-safe by construction; the host
/// guarantees the underlying implementations are `Sync`.
unsafe impl Send for HostVTable {}
/// SAFETY: see above.
unsafe impl Sync for HostVTable {}

#[cfg(test)]
mod tests {
    use std::sync::{
        Mutex, MutexGuard, OnceLock,
        atomic::{AtomicU8, AtomicU64, Ordering},
    };

    use rstest::rstest;

    use super::*;
    use crate::boundary::{OwnedBytes, PluginResult, Slice};

    static CLOCK_VALUE: AtomicU64 = AtomicU64::new(0);
    static LOG_LEVEL_OBSERVED: AtomicU8 = AtomicU8::new(0);

    // Serialises tests that mutate and observe `CLOCK_VALUE` /
    // `LOG_LEVEL_OBSERVED`. cargo test runs cases in parallel by
    // default, so without this lock a parametrised case can overwrite
    // the static after another case's reset but before its assertion.
    fn shared_state_lock() -> MutexGuard<'static, ()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(|p| p.into_inner())
    }

    unsafe extern "C" fn fixed_clock_now_ns() -> u64 {
        CLOCK_VALUE.load(Ordering::SeqCst)
    }

    unsafe extern "C" fn recording_log(
        level: HostLogLevel,
        _target: BorrowedStr<'_>,
        _message: BorrowedStr<'_>,
    ) {
        LOG_LEVEL_OBSERVED.store(level as u8, Ordering::SeqCst);
    }

    macro_rules! stub_bytes {
        ($name:ident) => {
            unsafe extern "C" fn $name(
                _ctx: *const HostContext,
                _a: BorrowedStr<'_>,
            ) -> PluginResult<OwnedBytes> {
                PluginResult::Ok(OwnedBytes::empty())
            }
        };
    }

    macro_rules! stub_unit {
        ($name:ident, ($($arg:ident : $ty:ty),* $(,)?)) => {
            unsafe extern "C" fn $name($($arg: $ty),*) -> PluginResult<()> {
                $(let _ = $arg;)*
                PluginResult::Ok(())
            }
        };
    }

    stub_bytes!(stub_cache_instrument);
    stub_bytes!(stub_cache_account);
    stub_bytes!(stub_cache_order);
    stub_bytes!(stub_cache_position);
    stub_bytes!(stub_cache_orders_for_strategy);
    stub_bytes!(stub_cache_positions_for_strategy);

    stub_unit!(
        stub_subscribe,
        (
            ctx: *const HostContext,
            a: BorrowedStr<'_>,
            b: BorrowedStr<'_>,
            c: BorrowedStr<'_>,
        )
    );
    stub_unit!(
        stub_subscribe_book_deltas,
        (
            ctx: *const HostContext,
            a: BorrowedStr<'_>,
            t: u8,
            d: usize,
            b: BorrowedStr<'_>,
            m: u8,
            c: BorrowedStr<'_>,
        )
    );
    stub_unit!(
        stub_subscribe_book_at_interval,
        (
            ctx: *const HostContext,
            a: BorrowedStr<'_>,
            t: u8,
            d: usize,
            i: usize,
            b: BorrowedStr<'_>,
            c: BorrowedStr<'_>,
        )
    );
    stub_unit!(
        stub_unsubscribe_book_at_interval,
        (
            ctx: *const HostContext,
            a: BorrowedStr<'_>,
            i: usize,
            b: BorrowedStr<'_>,
            c: BorrowedStr<'_>,
        )
    );
    stub_unit!(
        stub_msgbus_publish,
        (
            ctx: *const HostContext,
            t: BorrowedStr<'_>,
            p: Slice<'_, u8>,
        )
    );
    stub_unit!(
        stub_set_time_alert,
        (
            ctx: *const HostContext,
            n: BorrowedStr<'_>,
            a: u64,
            p: u8,
        )
    );
    stub_unit!(
        stub_set_timer,
        (
            ctx: *const HostContext,
            n: BorrowedStr<'_>,
            i: u64,
            s: u64,
            e: u64,
            p: u8,
            f: u8,
        )
    );
    stub_unit!(stub_cancel_timer, (ctx: *const HostContext, n: BorrowedStr<'_>));
    stub_unit!(
        stub_submit_order,
        (ctx: *const HostContext, c: *const SubmitOrderHandle)
    );
    stub_unit!(
        stub_cancel_order,
        (ctx: *const HostContext, c: *const CancelOrderHandle)
    );
    stub_unit!(
        stub_modify_order,
        (ctx: *const HostContext, c: *const ModifyOrderHandle)
    );
    stub_unit!(
        stub_submit_order_list,
        (ctx: *const HostContext, c: *const SubmitOrderListHandle)
    );
    stub_unit!(
        stub_cancel_orders,
        (ctx: *const HostContext, c: *const CancelOrdersHandle)
    );
    stub_unit!(
        stub_cancel_all_orders,
        (ctx: *const HostContext, c: *const CancelAllOrdersHandle)
    );
    stub_unit!(
        stub_close_position,
        (ctx: *const HostContext, c: *const ClosePositionHandle)
    );
    stub_unit!(
        stub_close_all_positions,
        (ctx: *const HostContext, c: *const CloseAllPositionsHandle)
    );
    stub_unit!(
        stub_query_account,
        (ctx: *const HostContext, c: *const QueryAccountHandle)
    );
    stub_unit!(
        stub_query_order,
        (ctx: *const HostContext, c: *const QueryOrderHandle)
    );

    fn build_test_host(abi: u32) -> HostVTable {
        HostVTable {
            abi_version: abi,
            clock_now_ns: fixed_clock_now_ns,
            log: recording_log,
            cache_instrument: stub_cache_instrument,
            cache_account: stub_cache_account,
            cache_order: stub_cache_order,
            cache_position: stub_cache_position,
            cache_orders_for_strategy: stub_cache_orders_for_strategy,
            cache_positions_for_strategy: stub_cache_positions_for_strategy,
            subscribe_quotes: stub_subscribe,
            unsubscribe_quotes: stub_subscribe,
            subscribe_trades: stub_subscribe,
            unsubscribe_trades: stub_subscribe,
            subscribe_bars: stub_subscribe,
            unsubscribe_bars: stub_subscribe,
            subscribe_book_deltas: stub_subscribe_book_deltas,
            unsubscribe_book_deltas: stub_subscribe,
            subscribe_book_at_interval: stub_subscribe_book_at_interval,
            unsubscribe_book_at_interval: stub_unsubscribe_book_at_interval,
            msgbus_publish: stub_msgbus_publish,
            set_time_alert: stub_set_time_alert,
            set_timer: stub_set_timer,
            cancel_timer: stub_cancel_timer,
            submit_order: stub_submit_order,
            cancel_order: stub_cancel_order,
            modify_order: stub_modify_order,
            submit_order_list: stub_submit_order_list,
            cancel_orders: stub_cancel_orders,
            cancel_all_orders: stub_cancel_all_orders,
            close_position: stub_close_position,
            close_all_positions: stub_close_all_positions,
            query_account: stub_query_account,
            query_order: stub_query_order,
        }
    }

    #[rstest]
    fn matches_compiled_abi_accepts_compiled_version() {
        let host = build_test_host(NAUTILUS_PLUGIN_ABI_VERSION);
        assert!(host.matches_compiled_abi());
    }

    #[rstest]
    #[case::off_by_one(NAUTILUS_PLUGIN_ABI_VERSION.wrapping_add(1))]
    #[case::zero(0)]
    #[case::max(u32::MAX)]
    fn matches_compiled_abi_rejects_mismatch(#[case] abi: u32) {
        let host = build_test_host(abi);
        assert!(!host.matches_compiled_abi());
    }

    #[rstest]
    fn now_ns_calls_clock_function_pointer() {
        let _g = shared_state_lock();
        CLOCK_VALUE.store(42_424_242, Ordering::SeqCst);
        let host = build_test_host(NAUTILUS_PLUGIN_ABI_VERSION);
        // SAFETY: clock_now_ns function pointer is non-null and lives for
        // the test scope.
        let n = unsafe { host.now_ns() };
        assert_eq!(n, 42_424_242);
    }

    #[rstest]
    #[case::error(HostLogLevel::Error, 1u8)]
    #[case::warn(HostLogLevel::Warn, 2)]
    #[case::info(HostLogLevel::Info, 3)]
    #[case::debug(HostLogLevel::Debug, 4)]
    #[case::trace(HostLogLevel::Trace, 5)]
    fn log_message_invokes_log_with_the_right_level(
        #[case] level: HostLogLevel,
        #[case] expected_discriminant: u8,
    ) {
        let _g = shared_state_lock();
        LOG_LEVEL_OBSERVED.store(0, Ordering::SeqCst);
        let host = build_test_host(NAUTILUS_PLUGIN_ABI_VERSION);
        // SAFETY: log fn pointer is non-null and lives for the test scope.
        unsafe { host.log_message(level, "target", "message") };
        assert_eq!(
            LOG_LEVEL_OBSERVED.load(Ordering::SeqCst),
            expected_discriminant
        );
    }
}
