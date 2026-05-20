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

//! Host-side `HostVTable` that routes order commands from plug-in strategies
//! through the production [`Strategy`]
//! pipeline.
//!
//! Phase 1 keeps the surface minimal: clock, log, and the three order
//! commands. The order-command thunks resolve the calling adapter from the
//! per-instance [`HostContextInner`] payload, parse the JSON envelope, and
//! call the matching `Strategy` method on the adapter. The cache, risk, and
//! event hops stay inside the engine.
//!
//! Cache reads, subscription helpers, msgbus publish, and timer helpers are
//! out of scope for phase 1.

#![allow(unsafe_code)]
#![allow(
    clippy::multiple_unsafe_ops_per_block,
    reason = "vtable deref and FFI call form a single boundary callback; \
              SAFETY comments cover both ops together"
)]

use std::sync::OnceLock;

use nautilus_common::actor::registry::try_get_actor_unchecked;
use nautilus_core::time::duration_since_unix_epoch;
use nautilus_plugin::{
    NAUTILUS_PLUGIN_ABI_VERSION,
    boundary::{BorrowedStr, PluginError, PluginErrorCode, PluginResult},
    host::{HostContext, HostLogLevel, HostVTable},
    loader::PluginLoader,
};
use nautilus_trading::strategy::Strategy;

#[cfg(doc)]
use crate::plugin::registry::HostContextInner;
use crate::plugin::{
    commands::{CancelOrderCommand, ModifyOrderCommand, SubmitOrderCommand},
    registry::host_context_inner,
    strategy::PluginStrategyAdapter,
};

/// Returns the process-wide `HostVTable` configured for the live node.
///
/// One static vtable is enough because plug-ins never compare vtables; they
/// only call through the function pointers. The vtable bundles the order
/// command thunks that route through [`Strategy`].
#[must_use]
pub fn host_vtable() -> *const HostVTable {
    static HOST: OnceLock<HostVTable> = OnceLock::new();
    std::ptr::from_ref(HOST.get_or_init(|| HostVTable {
        abi_version: NAUTILUS_PLUGIN_ABI_VERSION,
        clock_now_ns: host_clock_now_ns,
        log: host_log,
        submit_order: host_submit_order,
        cancel_order: host_cancel_order,
        modify_order: host_modify_order,
    }))
}

/// Returns a [`PluginLoader`] pre-bound to the host vtable from
/// [`host_vtable`].
///
/// The loader hands every plug-in cdylib the live-node vtable so order
/// commands route through the strategy adapter instead of returning
/// `NotImplemented`.
#[must_use]
pub fn plugin_loader() -> PluginLoader {
    PluginLoader::with_host(host_vtable())
}

unsafe extern "C" fn host_clock_now_ns() -> u64 {
    u64::try_from(duration_since_unix_epoch().as_nanos()).unwrap_or(u64::MAX)
}

unsafe extern "C" fn host_log(
    level: HostLogLevel,
    target: BorrowedStr<'_>,
    message: BorrowedStr<'_>,
) {
    // SAFETY: producer holds the storage live across the call.
    let target = unsafe { target.as_str() };
    // SAFETY: see above.
    let message = unsafe { message.as_str() };
    match level {
        HostLogLevel::Error => log::error!(target: "nautilus_plugin", "[{target}] {message}"),
        HostLogLevel::Warn => log::warn!(target: "nautilus_plugin", "[{target}] {message}"),
        HostLogLevel::Info => log::info!(target: "nautilus_plugin", "[{target}] {message}"),
        HostLogLevel::Debug => log::debug!(target: "nautilus_plugin", "[{target}] {message}"),
        HostLogLevel::Trace => log::trace!(target: "nautilus_plugin", "[{target}] {message}"),
    }
}

unsafe extern "C" fn host_submit_order(
    ctx: *const HostContext,
    command_json: BorrowedStr<'_>,
) -> PluginResult<()> {
    dispatch_command(ctx, command_json, "submit_order", |adapter, json| {
        let cmd: SubmitOrderCommand = serde_json::from_str(json)?;
        Strategy::submit_order(
            adapter,
            cmd.order,
            cmd.position_id,
            cmd.client_id,
            cmd.params,
        )
    })
}

unsafe extern "C" fn host_cancel_order(
    ctx: *const HostContext,
    command_json: BorrowedStr<'_>,
) -> PluginResult<()> {
    dispatch_command(ctx, command_json, "cancel_order", |adapter, json| {
        let cmd: CancelOrderCommand = serde_json::from_str(json)?;
        Strategy::cancel_order(adapter, cmd.client_order_id, cmd.client_id, cmd.params)
    })
}

unsafe extern "C" fn host_modify_order(
    ctx: *const HostContext,
    command_json: BorrowedStr<'_>,
) -> PluginResult<()> {
    dispatch_command(ctx, command_json, "modify_order", |adapter, json| {
        let cmd: ModifyOrderCommand = serde_json::from_str(json)?;
        Strategy::modify_order(
            adapter,
            cmd.client_order_id,
            cmd.quantity,
            cmd.price,
            cmd.trigger_price,
            cmd.client_id,
            cmd.params,
        )
    })
}

fn dispatch_command(
    ctx: *const HostContext,
    command_json: BorrowedStr<'_>,
    method: &'static str,
    f: impl FnOnce(&mut PluginStrategyAdapter, &str) -> anyhow::Result<()>,
) -> PluginResult<()> {
    // SAFETY: command_json borrows storage that is live across the call.
    let json = unsafe { command_json.as_str() };

    // SAFETY: caller (the plug-in) round-trips the same ctx the host handed
    // back from `PluginStrategyAdapter::new`.
    let inner = match unsafe { host_context_inner(ctx) } {
        Some(inner) => inner,
        None => {
            return PluginResult::Err(PluginError::new(
                PluginErrorCode::InvalidArgument,
                format!("{method} called with null HostContext"),
            ));
        }
    };

    if !inner.is_strategy {
        return PluginResult::Err(PluginError::new(
            PluginErrorCode::InvalidArgument,
            format!(
                "{method} called from a non-strategy plug-in context (actor_id={})",
                inner.actor_id
            ),
        ));
    }

    let actor_id = inner.actor_id.inner();
    let Some(mut adapter_ref) = try_get_actor_unchecked::<PluginStrategyAdapter>(&actor_id) else {
        return PluginResult::Err(PluginError::new(
            PluginErrorCode::Generic,
            format!(
                "{method} could not resolve strategy adapter for actor_id={}",
                inner.actor_id
            ),
        ));
    };

    match f(&mut adapter_ref, json) {
        Ok(()) => PluginResult::Ok(()),
        Err(e) => PluginResult::Err(PluginError::new(PluginErrorCode::Generic, e.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn host_vtable_carries_compiled_abi() {
        let p = host_vtable();
        assert!(!p.is_null());
        // SAFETY: pointer is to a static OnceLock-backed HostVTable.
        let v = unsafe { &*p };
        assert_eq!(v.abi_version, NAUTILUS_PLUGIN_ABI_VERSION);
    }

    #[rstest]
    fn host_vtable_binds_live_node_order_command_thunks() {
        // Locks in that the live-node host vtable installs the routing
        // thunks defined in this module, not the loader.rs NotImplemented
        // stubs. A regression that fell back to the default loader vtable
        // would change these function-pointer identities.
        let p = host_vtable();
        // SAFETY: pointer is to a static OnceLock-backed HostVTable.
        let v = unsafe { &*p };
        assert_eq!(
            v.submit_order as *const () as usize,
            host_submit_order as *const () as usize,
        );
        assert_eq!(
            v.cancel_order as *const () as usize,
            host_cancel_order as *const () as usize,
        );
        assert_eq!(
            v.modify_order as *const () as usize,
            host_modify_order as *const () as usize,
        );
        assert_eq!(
            v.clock_now_ns as *const () as usize,
            host_clock_now_ns as *const () as usize,
        );
        assert_eq!(v.log as *const () as usize, host_log as *const () as usize);
    }

    #[rstest]
    fn host_clock_now_ns_returns_unix_nanos_after_2020() {
        let p = host_vtable();
        // SAFETY: pointer is to a static OnceLock-backed HostVTable.
        let v = unsafe { &*p };
        // SAFETY: fn pointer is non-null; clock_now_ns dereferences no input.
        let now = unsafe { (v.clock_now_ns)() };
        // Any time after 2020-01-01 in UNIX nanoseconds.
        assert!(now > 1_577_836_800_000_000_000u64);
    }

    #[rstest]
    fn host_submit_order_rejects_null_ctx() {
        let p = host_vtable();
        // SAFETY: pointer is to a static OnceLock-backed HostVTable.
        let v = unsafe { &*p };
        let payload = BorrowedStr::from_str("{}");
        // SAFETY: passes null ctx; the thunk handles it.
        let r = unsafe { (v.submit_order)(std::ptr::null(), payload) };
        let err = r.into_result().unwrap_err();
        assert_eq!(err.code, PluginErrorCode::InvalidArgument);
        assert!(err.message_string().contains("null HostContext"));
    }

    #[rstest]
    fn host_cancel_order_rejects_null_ctx() {
        let p = host_vtable();
        // SAFETY: see above.
        let v = unsafe { &*p };
        let payload = BorrowedStr::from_str("{}");
        // SAFETY: passes null ctx.
        let r = unsafe { (v.cancel_order)(std::ptr::null(), payload) };
        let err = r.into_result().unwrap_err();
        assert_eq!(err.code, PluginErrorCode::InvalidArgument);
    }

    #[rstest]
    fn host_modify_order_rejects_null_ctx() {
        let p = host_vtable();
        // SAFETY: see above.
        let v = unsafe { &*p };
        let payload = BorrowedStr::from_str("{}");
        // SAFETY: passes null ctx.
        let r = unsafe { (v.modify_order)(std::ptr::null(), payload) };
        let err = r.into_result().unwrap_err();
        assert_eq!(err.code, PluginErrorCode::InvalidArgument);
    }

    #[rstest]
    fn host_submit_order_rejects_non_strategy_context() {
        // Plug-in actors must not submit orders. The host vtable thunk
        // inspects HostContextInner::is_strategy and rejects calls from
        // actor contexts with InvalidArgument.
        use nautilus_model::identifiers::ActorId;

        use crate::plugin::registry::{
            HostContextInner, drop_host_context, host_context_test_lock, leak_host_context,
        };

        let _guard = host_context_test_lock();
        let ctx = leak_host_context(HostContextInner {
            actor_id: ActorId::from("ActorContextProbe"),
            is_strategy: false,
        });
        let p = host_vtable();
        // SAFETY: pointer is to a static OnceLock-backed HostVTable.
        let v = unsafe { &*p };
        let payload = BorrowedStr::from_str("{}");
        // SAFETY: ctx was leaked above and is live.
        let r = unsafe { (v.submit_order)(ctx, payload) };
        let err = r.into_result().unwrap_err();
        assert_eq!(err.code, PluginErrorCode::InvalidArgument);
        assert!(
            err.message_string().contains("non-strategy"),
            "expected non-strategy rejection, was: {}",
            err.message_string(),
        );
        // SAFETY: ctx came from leak_host_context above.
        unsafe { drop_host_context(ctx) };
    }

    #[rstest]
    fn host_submit_order_rejects_unregistered_actor_id() {
        // ctx points to a strategy actor_id that no PluginStrategyAdapter
        // has registered into the thread-local actor registry, so the
        // host vtable's try_get_actor_unchecked lookup returns None.
        use nautilus_model::identifiers::ActorId;

        use crate::plugin::registry::{
            HostContextInner, drop_host_context, host_context_test_lock, leak_host_context,
        };

        let _guard = host_context_test_lock();
        let ctx = leak_host_context(HostContextInner {
            actor_id: ActorId::from("UnregisteredStrategyAdapter"),
            is_strategy: true,
        });
        let p = host_vtable();
        // SAFETY: pointer is to a static OnceLock-backed HostVTable.
        let v = unsafe { &*p };
        let payload = BorrowedStr::from_str("{}");
        // SAFETY: ctx was leaked above and is live.
        let r = unsafe { (v.submit_order)(ctx, payload) };
        let err = r.into_result().unwrap_err();
        assert_eq!(err.code, PluginErrorCode::Generic);
        assert!(
            err.message_string().contains("could not resolve"),
            "expected unresolved-adapter rejection, was: {}",
            err.message_string(),
        );
        // SAFETY: ctx came from leak_host_context above.
        unsafe { drop_host_context(ctx) };
    }
}
