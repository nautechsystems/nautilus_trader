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
    /// `ctx` is the [`HostContext`] the host passed into the
    /// strategy's `create`. `command_json` is a serialised order-submit
    /// command; the host parses it into the in-engine `SubmitOrder` shape
    /// and routes it through the execution engine.
    pub submit_order: unsafe extern "C" fn(
        ctx: *const HostContext,
        command_json: BorrowedStr<'_>,
    ) -> PluginResult<()>,

    /// Cancels an in-flight order on behalf of the calling strategy.
    ///
    /// `command_json` carries the cancel command identifying the order to
    /// cancel (typically by `client_order_id` and `instrument_id`).
    pub cancel_order: unsafe extern "C" fn(
        ctx: *const HostContext,
        command_json: BorrowedStr<'_>,
    ) -> PluginResult<()>,

    /// Modifies an in-flight order on behalf of the calling strategy.
    ///
    /// `command_json` carries the modify command (new quantity, price, etc.).
    pub modify_order: unsafe extern "C" fn(
        ctx: *const HostContext,
        command_json: BorrowedStr<'_>,
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
