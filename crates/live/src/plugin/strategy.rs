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

//! Host-side adapter that wraps a plug-in strategy cdylib as a
//! [`Strategy`].
//!
//! Mirrors [`PluginActorAdapter`](crate::plugin::actor::PluginActorAdapter)
//! shape but owns a [`StrategyCore`] and forwards the strategy-only
//! callbacks (order lifecycle and position events) as well as the actor
//! callback set. The plug-in issues `submit_order` / `cancel_order` /
//! `modify_order` back through [`HostVTable`];
//! the host vtable looks the adapter up by the per-instance
//! [`HostContextInner`] and calls
//! the matching [`Strategy`] method so the production cache/risk pipeline
//! runs.

#![allow(unsafe_code)]
#![allow(
    clippy::multiple_unsafe_ops_per_block,
    reason = "vtable deref and FFI call form a single boundary callback; \
              SAFETY comments cover both ops together"
)]

use std::{
    fmt::Debug,
    panic::{AssertUnwindSafe, catch_unwind},
};

use nautilus_common::{actor::DataActor, signal::Signal, timer::TimeEvent};
use nautilus_model::{
    data::{
        Bar, FundingRateUpdate, IndexPriceUpdate, InstrumentClose, InstrumentStatus,
        MarkPriceUpdate, QuoteTick, TradeTick,
    },
    events::{
        OrderAccepted, OrderCancelRejected, OrderCanceled, OrderDenied, OrderEmulated,
        OrderExpired, OrderFilled, OrderInitialized, OrderModifyRejected, OrderPendingCancel,
        OrderPendingUpdate, OrderRejected, OrderReleased, OrderSubmitted, OrderTriggered,
        OrderUpdated, PositionChanged, PositionClosed, PositionOpened,
    },
    identifiers::ActorId,
};
use nautilus_plugin::{
    boundary::{BorrowedStr, PluginResult},
    host::{HostContext, HostVTable},
    manifest::ValidatedStrategyVTable,
    surfaces::strategy::PluginStrategyHandle,
};
use nautilus_trading::{
    nautilus_strategy,
    strategy::{Strategy, StrategyConfig, StrategyCore},
};

use crate::plugin::registry::{HostContextInner, drop_host_context, leak_host_context};

/// Adapts a plug-in strategy (vtable + handle from a cdylib) into a host-side
/// [`Strategy`] the live node can
/// register and route through the production cache, risk, and event pipeline.
pub struct PluginStrategyAdapter {
    core: StrategyCore,
    plugin_name: String,
    type_name: String,
    vtable: ValidatedStrategyVTable,
    handle: *mut PluginStrategyHandle,
    ctx: *const HostContext,
}

// SAFETY: the adapter owns the plug-in handle exclusively and never aliases
// it across threads. The vtable pointer is process-lifetime static. The
// engine drives the adapter from a single trader thread; the bound is only
// required to satisfy the trait bounds upstream.
unsafe impl Send for PluginStrategyAdapter {}

impl Debug for PluginStrategyAdapter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(PluginStrategyAdapter))
            .field("plugin_name", &self.plugin_name)
            .field("type_name", &self.type_name)
            .field("actor_id", &self.core.actor_id())
            .finish()
    }
}

impl PluginStrategyAdapter {
    /// Constructs a new adapter by calling the plug-in's `create` thunk.
    ///
    /// `host` must be the same vtable pointer the host handed the plug-in at
    /// load time. `strategy_config` defines the strategy ID and other core
    /// state on the host side. `config_json` is forwarded verbatim to the
    /// plug-in's `PluginStrategy::new` so the cdylib can read instance
    /// configuration.
    ///
    /// # Errors
    ///
    /// Returns an error if `strategy_config.strategy_id` is `None`, or if the
    /// plug-in's `create` thunk returns a null handle.
    ///
    /// `strategy_config.strategy_id` must be `Some(_)` so that
    /// `Trader::prepare_strategy_for_registration`'s `change_id` call is a
    /// no-op on the adapter's `actor_id`. Otherwise the trader would derive
    /// a fresh tag-suffixed id at registration time, while the host context
    /// would still carry the pre-registration address-based default, and
    /// every `submit_order` / `cancel_order` / `modify_order` callback from
    /// the plug-in would fail to resolve the registered adapter.
    ///
    /// # Safety
    ///
    /// `host` must be the same vtable pointer the host registered with the
    /// plug-in at load time.
    pub unsafe fn new(
        strategy_config: StrategyConfig,
        plugin_name: impl Into<String>,
        type_name: impl Into<String>,
        vtable: ValidatedStrategyVTable,
        host: *const HostVTable,
        config_json: &str,
    ) -> anyhow::Result<Self> {
        if strategy_config.strategy_id.is_none() {
            anyhow::bail!(
                "PluginStrategyAdapter requires StrategyConfig::strategy_id to be set so the \
                 host context's actor_id stays stable across Trader::add_strategy"
            );
        }

        let plugin_name = plugin_name.into();
        let type_name = type_name.into();
        // SAFETY: vtable comes from a validated manifest entry.
        let create = unsafe { validated_slot!(StrategyVTable, vtable.as_ptr(), create) };
        let core = StrategyCore::new(strategy_config);
        let actor_id = ActorId::from(core.actor_id().inner().as_str());

        let ctx = leak_host_context(HostContextInner {
            actor_id,
            is_strategy: true,
        });

        let cfg = BorrowedStr::from_str(config_json);
        // SAFETY: vtable is non-null, host outlives the adapter, ctx + cfg
        // are live across the call.
        let handle = guard_call(&plugin_name, &type_name, "create", || unsafe {
            create(host, ctx, cfg)
        })
        .ok_or_else(|| {
            // SAFETY: ctx came from leak_host_context above.
            unsafe { drop_host_context(ctx) };
            anyhow::anyhow!("plug-in strategy '{type_name}' panicked in create")
        })?;

        if handle.is_null() {
            // SAFETY: ctx came from leak_host_context above.
            unsafe { drop_host_context(ctx) };
            anyhow::bail!("plug-in strategy '{type_name}' returned a null handle from create");
        }

        Ok(Self {
            core,
            plugin_name,
            type_name,
            vtable,
            handle,
            ctx,
        })
    }

    /// Returns the canonical type name reported by the plug-in.
    #[must_use]
    pub fn type_name(&self) -> &str {
        &self.type_name
    }

    /// Returns the plug-in name (manifest `name`) the adapter wraps.
    #[must_use]
    pub fn plugin_name(&self) -> &str {
        &self.plugin_name
    }
}

impl Drop for PluginStrategyAdapter {
    fn drop(&mut self) {
        if !self.handle.is_null() {
            let _ = catch_unwind(AssertUnwindSafe(|| {
                // SAFETY: vtable + handle are live; drop_handle ignores null.
                unsafe {
                    validated_slot!(StrategyVTable, self.vtable.as_ptr(), drop_handle)(self.handle);
                };
            }));
            self.handle = std::ptr::null_mut();
        }
        // SAFETY: ctx originated from leak_host_context in `new`.
        unsafe { drop_host_context(self.ctx) };
        self.ctx = std::ptr::null();
    }
}

nautilus_strategy!(PluginStrategyAdapter, core, {
    fn on_order_initialized(&mut self, event: OrderInitialized) {
        log_strategy_hook_error(
            "on_order_initialized",
            self.forward_order_initialized(&event),
        );
    }

    fn on_order_submitted(&mut self, event: OrderSubmitted) {
        log_strategy_hook_error("on_order_submitted", self.forward_order_submitted(&event));
    }

    fn on_order_accepted(&mut self, event: OrderAccepted) {
        log_strategy_hook_error("on_order_accepted", self.forward_order_accepted(&event));
    }

    fn on_order_rejected(&mut self, event: OrderRejected) {
        log_strategy_hook_error("on_order_rejected", self.forward_order_rejected(&event));
    }

    fn on_order_expired(&mut self, event: OrderExpired) {
        log_strategy_hook_error("on_order_expired", self.forward_order_expired(&event));
    }

    fn on_order_triggered(&mut self, event: OrderTriggered) {
        log_strategy_hook_error("on_order_triggered", self.forward_order_triggered(&event));
    }

    fn on_order_denied(&mut self, event: OrderDenied) {
        log_strategy_hook_error("on_order_denied", self.forward_order_denied(&event));
    }

    fn on_order_emulated(&mut self, event: OrderEmulated) {
        log_strategy_hook_error("on_order_emulated", self.forward_order_emulated(&event));
    }

    fn on_order_released(&mut self, event: OrderReleased) {
        log_strategy_hook_error("on_order_released", self.forward_order_released(&event));
    }

    fn on_order_pending_update(&mut self, event: OrderPendingUpdate) {
        log_strategy_hook_error(
            "on_order_pending_update",
            self.forward_order_pending_update(&event),
        );
    }

    fn on_order_pending_cancel(&mut self, event: OrderPendingCancel) {
        log_strategy_hook_error(
            "on_order_pending_cancel",
            self.forward_order_pending_cancel(&event),
        );
    }

    fn on_order_modify_rejected(&mut self, event: OrderModifyRejected) {
        log_strategy_hook_error(
            "on_order_modify_rejected",
            self.forward_order_modify_rejected(&event),
        );
    }

    fn on_order_cancel_rejected(&mut self, event: OrderCancelRejected) {
        log_strategy_hook_error(
            "on_order_cancel_rejected",
            self.forward_order_cancel_rejected(&event),
        );
    }

    fn on_order_updated(&mut self, event: OrderUpdated) {
        log_strategy_hook_error("on_order_updated", self.forward_order_updated(&event));
    }

    fn on_position_opened(&mut self, event: PositionOpened) {
        log_strategy_hook_error("on_position_opened", self.forward_position_opened(&event));
    }

    fn on_position_changed(&mut self, event: PositionChanged) {
        log_strategy_hook_error("on_position_changed", self.forward_position_changed(&event));
    }

    fn on_position_closed(&mut self, event: PositionClosed) {
        log_strategy_hook_error("on_position_closed", self.forward_position_closed(&event));
    }
});

impl DataActor for PluginStrategyAdapter {
    fn on_start(&mut self) -> anyhow::Result<()> {
        // Run the Strategy trait default first so GTD timer reactivation
        // happens when `manage_gtd_expiry` is enabled, matching the Python
        // strategy adapter pattern in crates/trading/src/python/strategy.rs.
        Strategy::on_start(self)?;
        invoke_lifecycle(self, "on_start", |adapter| unsafe {
            validated_slot!(StrategyVTable, adapter.vtable.as_ptr(), on_start)(adapter.handle)
        })
    }

    fn on_stop(&mut self) -> anyhow::Result<()> {
        invoke_lifecycle(self, "on_stop", |adapter| unsafe {
            validated_slot!(StrategyVTable, adapter.vtable.as_ptr(), on_stop)(adapter.handle)
        })
    }

    fn on_resume(&mut self) -> anyhow::Result<()> {
        invoke_lifecycle(self, "on_resume", |adapter| unsafe {
            validated_slot!(StrategyVTable, adapter.vtable.as_ptr(), on_resume)(adapter.handle)
        })
    }

    fn on_reset(&mut self) -> anyhow::Result<()> {
        invoke_lifecycle(self, "on_reset", |adapter| unsafe {
            validated_slot!(StrategyVTable, adapter.vtable.as_ptr(), on_reset)(adapter.handle)
        })
    }

    fn on_dispose(&mut self) -> anyhow::Result<()> {
        invoke_lifecycle(self, "on_dispose", |adapter| unsafe {
            validated_slot!(StrategyVTable, adapter.vtable.as_ptr(), on_dispose)(adapter.handle)
        })
    }

    fn on_degrade(&mut self) -> anyhow::Result<()> {
        invoke_lifecycle(self, "on_degrade", |adapter| unsafe {
            validated_slot!(StrategyVTable, adapter.vtable.as_ptr(), on_degrade)(adapter.handle)
        })
    }

    fn on_fault(&mut self) -> anyhow::Result<()> {
        invoke_lifecycle(self, "on_fault", |adapter| unsafe {
            validated_slot!(StrategyVTable, adapter.vtable.as_ptr(), on_fault)(adapter.handle)
        })
    }

    fn on_time_event(&mut self, event: &TimeEvent) -> anyhow::Result<()> {
        // Run the Strategy trait default first so GTD-EXPIRY and
        // MARKET_EXIT_CHECK timers fire before user code, matching the Python
        // strategy adapter pattern in crates/trading/src/python/strategy.rs.
        Strategy::on_time_event(self, event)?;
        invoke_event(self, "on_time_event", event, |adapter, p| unsafe {
            validated_slot!(StrategyVTable, adapter.vtable.as_ptr(), on_time_event)(
                adapter.handle,
                p,
            )
        })
    }

    fn on_quote(&mut self, quote: &QuoteTick) -> anyhow::Result<()> {
        invoke_event(self, "on_quote", quote, |adapter, p| unsafe {
            validated_slot!(StrategyVTable, adapter.vtable.as_ptr(), on_quote)(adapter.handle, p)
        })
    }

    fn on_trade(&mut self, trade: &TradeTick) -> anyhow::Result<()> {
        invoke_event(self, "on_trade", trade, |adapter, p| unsafe {
            validated_slot!(StrategyVTable, adapter.vtable.as_ptr(), on_trade)(adapter.handle, p)
        })
    }

    fn on_bar(&mut self, bar: &Bar) -> anyhow::Result<()> {
        invoke_event(self, "on_bar", bar, |adapter, p| unsafe {
            validated_slot!(StrategyVTable, adapter.vtable.as_ptr(), on_bar)(adapter.handle, p)
        })
    }

    fn on_mark_price(&mut self, mark_price: &MarkPriceUpdate) -> anyhow::Result<()> {
        invoke_event(self, "on_mark_price", mark_price, |adapter, p| unsafe {
            validated_slot!(StrategyVTable, adapter.vtable.as_ptr(), on_mark_price)(
                adapter.handle,
                p,
            )
        })
    }

    fn on_index_price(&mut self, index_price: &IndexPriceUpdate) -> anyhow::Result<()> {
        invoke_event(self, "on_index_price", index_price, |adapter, p| unsafe {
            validated_slot!(StrategyVTable, adapter.vtable.as_ptr(), on_index_price)(
                adapter.handle,
                p,
            )
        })
    }

    fn on_funding_rate(&mut self, funding_rate: &FundingRateUpdate) -> anyhow::Result<()> {
        invoke_event(self, "on_funding_rate", funding_rate, |adapter, p| unsafe {
            validated_slot!(StrategyVTable, adapter.vtable.as_ptr(), on_funding_rate)(
                adapter.handle,
                p,
            )
        })
    }

    fn on_instrument_status(&mut self, data: &InstrumentStatus) -> anyhow::Result<()> {
        invoke_event(self, "on_instrument_status", data, |adapter, p| unsafe {
            validated_slot!(
                StrategyVTable,
                adapter.vtable.as_ptr(),
                on_instrument_status
            )(adapter.handle, p)
        })
    }

    fn on_instrument_close(&mut self, update: &InstrumentClose) -> anyhow::Result<()> {
        invoke_event(self, "on_instrument_close", update, |adapter, p| unsafe {
            validated_slot!(StrategyVTable, adapter.vtable.as_ptr(), on_instrument_close)(
                adapter.handle,
                p,
            )
        })
    }

    fn on_order_filled(&mut self, event: &OrderFilled) -> anyhow::Result<()> {
        invoke_event(self, "on_order_filled", event, |adapter, p| unsafe {
            validated_slot!(StrategyVTable, adapter.vtable.as_ptr(), on_order_filled)(
                adapter.handle,
                p,
            )
        })
    }

    fn on_order_canceled(&mut self, event: &OrderCanceled) -> anyhow::Result<()> {
        invoke_event(self, "on_order_canceled", event, |adapter, p| unsafe {
            validated_slot!(StrategyVTable, adapter.vtable.as_ptr(), on_order_canceled)(
                adapter.handle,
                p,
            )
        })
    }

    fn on_signal(&mut self, signal: &Signal) -> anyhow::Result<()> {
        invoke_event(self, "on_signal", signal, |adapter, p| unsafe {
            validated_slot!(StrategyVTable, adapter.vtable.as_ptr(), on_signal)(adapter.handle, p)
        })
    }
}

/// Forwarders for the strategy-only callbacks the live engine dispatches
/// through `Strategy::handle_order_event` and `handle_position_event`. Each
/// forwarder runs the matching plug-in vtable callback through the same
/// `catch_unwind` + `PluginResult` plumbing as the actor surface.
impl PluginStrategyAdapter {
    fn forward_order_initialized(&self, event: &OrderInitialized) -> anyhow::Result<()> {
        invoke_event(self, "on_order_initialized", event, |adapter, p| unsafe {
            validated_slot!(
                StrategyVTable,
                adapter.vtable.as_ptr(),
                on_order_initialized
            )(adapter.handle, p)
        })
    }

    fn forward_order_submitted(&self, event: &OrderSubmitted) -> anyhow::Result<()> {
        invoke_event(self, "on_order_submitted", event, |adapter, p| unsafe {
            validated_slot!(StrategyVTable, adapter.vtable.as_ptr(), on_order_submitted)(
                adapter.handle,
                p,
            )
        })
    }

    fn forward_order_accepted(&self, event: &OrderAccepted) -> anyhow::Result<()> {
        invoke_event(self, "on_order_accepted", event, |adapter, p| unsafe {
            validated_slot!(StrategyVTable, adapter.vtable.as_ptr(), on_order_accepted)(
                adapter.handle,
                p,
            )
        })
    }

    fn forward_order_rejected(&self, event: &OrderRejected) -> anyhow::Result<()> {
        invoke_event(self, "on_order_rejected", event, |adapter, p| unsafe {
            validated_slot!(StrategyVTable, adapter.vtable.as_ptr(), on_order_rejected)(
                adapter.handle,
                p,
            )
        })
    }

    fn forward_order_expired(&self, event: &OrderExpired) -> anyhow::Result<()> {
        invoke_event(self, "on_order_expired", event, |adapter, p| unsafe {
            validated_slot!(StrategyVTable, adapter.vtable.as_ptr(), on_order_expired)(
                adapter.handle,
                p,
            )
        })
    }

    fn forward_order_triggered(&self, event: &OrderTriggered) -> anyhow::Result<()> {
        invoke_event(self, "on_order_triggered", event, |adapter, p| unsafe {
            validated_slot!(StrategyVTable, adapter.vtable.as_ptr(), on_order_triggered)(
                adapter.handle,
                p,
            )
        })
    }

    fn forward_order_denied(&self, event: &OrderDenied) -> anyhow::Result<()> {
        invoke_event(self, "on_order_denied", event, |adapter, p| unsafe {
            validated_slot!(StrategyVTable, adapter.vtable.as_ptr(), on_order_denied)(
                adapter.handle,
                p,
            )
        })
    }

    fn forward_order_emulated(&self, event: &OrderEmulated) -> anyhow::Result<()> {
        invoke_event(self, "on_order_emulated", event, |adapter, p| unsafe {
            validated_slot!(StrategyVTable, adapter.vtable.as_ptr(), on_order_emulated)(
                adapter.handle,
                p,
            )
        })
    }

    fn forward_order_released(&self, event: &OrderReleased) -> anyhow::Result<()> {
        invoke_event(self, "on_order_released", event, |adapter, p| unsafe {
            validated_slot!(StrategyVTable, adapter.vtable.as_ptr(), on_order_released)(
                adapter.handle,
                p,
            )
        })
    }

    fn forward_order_pending_update(&self, event: &OrderPendingUpdate) -> anyhow::Result<()> {
        invoke_event(
            self,
            "on_order_pending_update",
            event,
            |adapter, p| unsafe {
                validated_slot!(
                    StrategyVTable,
                    adapter.vtable.as_ptr(),
                    on_order_pending_update
                )(adapter.handle, p)
            },
        )
    }

    fn forward_order_pending_cancel(&self, event: &OrderPendingCancel) -> anyhow::Result<()> {
        invoke_event(
            self,
            "on_order_pending_cancel",
            event,
            |adapter, p| unsafe {
                validated_slot!(
                    StrategyVTable,
                    adapter.vtable.as_ptr(),
                    on_order_pending_cancel
                )(adapter.handle, p)
            },
        )
    }

    fn forward_order_modify_rejected(&self, event: &OrderModifyRejected) -> anyhow::Result<()> {
        invoke_event(
            self,
            "on_order_modify_rejected",
            event,
            |adapter, p| unsafe {
                validated_slot!(
                    StrategyVTable,
                    adapter.vtable.as_ptr(),
                    on_order_modify_rejected
                )(adapter.handle, p)
            },
        )
    }

    fn forward_order_cancel_rejected(&self, event: &OrderCancelRejected) -> anyhow::Result<()> {
        invoke_event(
            self,
            "on_order_cancel_rejected",
            event,
            |adapter, p| unsafe {
                validated_slot!(
                    StrategyVTable,
                    adapter.vtable.as_ptr(),
                    on_order_cancel_rejected
                )(adapter.handle, p)
            },
        )
    }

    fn forward_order_updated(&self, event: &OrderUpdated) -> anyhow::Result<()> {
        invoke_event(self, "on_order_updated", event, |adapter, p| unsafe {
            validated_slot!(StrategyVTable, adapter.vtable.as_ptr(), on_order_updated)(
                adapter.handle,
                p,
            )
        })
    }

    fn forward_position_opened(&self, event: &PositionOpened) -> anyhow::Result<()> {
        invoke_event(self, "on_position_opened", event, |adapter, p| unsafe {
            validated_slot!(StrategyVTable, adapter.vtable.as_ptr(), on_position_opened)(
                adapter.handle,
                p,
            )
        })
    }

    fn forward_position_changed(&self, event: &PositionChanged) -> anyhow::Result<()> {
        invoke_event(self, "on_position_changed", event, |adapter, p| unsafe {
            validated_slot!(StrategyVTable, adapter.vtable.as_ptr(), on_position_changed)(
                adapter.handle,
                p,
            )
        })
    }

    fn forward_position_closed(&self, event: &PositionClosed) -> anyhow::Result<()> {
        invoke_event(self, "on_position_closed", event, |adapter, p| unsafe {
            validated_slot!(StrategyVTable, adapter.vtable.as_ptr(), on_position_closed)(
                adapter.handle,
                p,
            )
        })
    }
}

fn log_strategy_hook_error(method: &str, r: anyhow::Result<()>) {
    if let Err(e) = r {
        log::error!(target: "nautilus_plugin", "{method}: {e}");
    }
}

fn guard_call<R>(plugin: &str, type_name: &str, method: &str, f: impl FnOnce() -> R) -> Option<R> {
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(r) => Some(r),
        Err(_payload) => {
            log::error!(
                target: "nautilus_plugin",
                "plug-in '{plugin}' ({type_name}) panicked in {method}",
            );
            None
        }
    }
}

fn invoke_lifecycle(
    adapter: &PluginStrategyAdapter,
    method: &str,
    f: impl FnOnce(&PluginStrategyAdapter) -> PluginResult<()>,
) -> anyhow::Result<()> {
    let plugin_name = adapter.plugin_name.clone();
    let type_name = adapter.type_name.clone();
    let result = guard_call(&plugin_name, &type_name, method, || f(adapter));
    finish(result, &plugin_name, &type_name, method)
}

fn invoke_event<T>(
    adapter: &PluginStrategyAdapter,
    method: &str,
    payload: &T,
    f: impl FnOnce(&PluginStrategyAdapter, *const T) -> PluginResult<()>,
) -> anyhow::Result<()> {
    let plugin_name = adapter.plugin_name.clone();
    let type_name = adapter.type_name.clone();
    let ptr: *const T = payload;
    let result = guard_call(&plugin_name, &type_name, method, || f(adapter, ptr));
    finish(result, &plugin_name, &type_name, method)
}

fn finish(
    result: Option<PluginResult<()>>,
    plugin_name: &str,
    type_name: &str,
    method: &str,
) -> anyhow::Result<()> {
    match result {
        Some(r) => r.into_result().map_err(|e| {
            anyhow::anyhow!(
                "plug-in '{plugin_name}' ({type_name}) {method} returned error: {}",
                e.message_string()
            )
        }),
        None => anyhow::bail!("plug-in '{plugin_name}' ({type_name}) panicked in {method}"),
    }
}

#[cfg(test)]
mod tests {
    use nautilus_model::identifiers::StrategyId;
    use nautilus_plugin::surfaces::strategy::{PluginStrategy, strategy_vtable};
    use rstest::rstest;

    use super::*;
    use crate::plugin::{
        host::host_vtable,
        registry::{host_context_live_count, host_context_test_lock},
    };

    struct DropTestStrategy;
    // SAFETY: empty unit struct holds no non-Send state.
    unsafe impl Send for DropTestStrategy {}

    impl PluginStrategy for DropTestStrategy {
        const TYPE_NAME: &'static str = "DropTestStrategy";

        fn new(_host: *const HostVTable, _ctx: *const HostContext, _config_json: &str) -> Self {
            Self
        }
    }

    fn test_strategy_config(strategy_id: &str) -> StrategyConfig {
        StrategyConfig::builder()
            .strategy_id(StrategyId::from(strategy_id))
            .order_id_tag("001".to_string())
            .build()
    }

    fn drop_test_strategy_vtable() -> ValidatedStrategyVTable {
        // SAFETY: generated vtables are process-lifetime static and fill
        // every required strategy slot.
        unsafe {
            ValidatedStrategyVTable::from_raw_unchecked(strategy_vtable::<DropTestStrategy>())
        }
    }

    #[rstest]
    fn new_rejects_config_without_strategy_id() {
        // Regression: StrategyConfig::default() leaves strategy_id == None.
        // Trader::prepare_strategy_for_registration would later call
        // change_id which mutates actor_id, leaving the host context's
        // actor_id pointing at the stale pre-registration default.
        let config = StrategyConfig::default();
        assert!(
            config.strategy_id.is_none(),
            "fixture assumes default has no strategy_id"
        );

        // SAFETY: documented error path.
        let r = unsafe {
            PluginStrategyAdapter::new(
                config,
                "plug-in",
                DropTestStrategy::TYPE_NAME,
                drop_test_strategy_vtable(),
                host_vtable(),
                "{}",
            )
        };
        let err = r.unwrap_err();
        assert!(
            err.to_string().contains("strategy_id"),
            "expected strategy_id error, was: {err}",
        );
    }

    #[rstest]
    fn drop_frees_host_context() {
        let _guard = host_context_test_lock();
        let before = host_context_live_count();
        let config = test_strategy_config("PluginStrategyAdapter-Drop");
        // SAFETY: host_vtable is process-lifetime static.
        let adapter = unsafe {
            PluginStrategyAdapter::new(
                config,
                "plug-in",
                DropTestStrategy::TYPE_NAME,
                drop_test_strategy_vtable(),
                host_vtable(),
                "{}",
            )
        }
        .expect("adapter construction");
        assert_eq!(host_context_live_count(), before + 1);

        drop(adapter);
        assert_eq!(host_context_live_count(), before);
    }
}
