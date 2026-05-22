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

//! Host-side adapter that wraps a plug-in actor cdylib as a [`DataActor`].
//!
//! Owns the plug-in's opaque handle plus a pointer to its static
//! [`nautilus_plugin::surfaces::actor::ActorVTable`] and forwards every callback
//! the surface ships in v1 through the vtable. The live engine sees a normal
//! `DataActor`; the plug-in never crosses the FFI boundary except as the typed
//! event payload pointer the engine already has from the cache.

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

use nautilus_common::{
    actor::{DataActor, DataActorConfig, DataActorCore},
    nautilus_actor,
    signal::Signal,
    timer::TimeEvent,
};
use nautilus_model::{
    data::{
        Bar, FundingRateUpdate, IndexPriceUpdate, InstrumentClose, InstrumentStatus,
        MarkPriceUpdate, QuoteTick, TradeTick,
    },
    events::{OrderCanceled, OrderFilled},
    identifiers::ActorId,
};
use nautilus_plugin::{
    boundary::{BorrowedStr, PluginResult},
    host::{HostContext, HostVTable},
    manifest::ValidatedActorVTable,
    surfaces::actor::PluginActorHandle,
};

use crate::plugin::registry::{HostContextInner, drop_host_context, leak_host_context};

/// Adapts a plug-in actor (vtable + handle from a cdylib) into a host-side
/// [`DataActor`] the live node can register and dispatch into.
pub struct PluginActorAdapter {
    core: DataActorCore,
    plugin_name: String,
    type_name: String,
    vtable: ValidatedActorVTable,
    handle: *mut PluginActorHandle,
    ctx: *const HostContext,
}

// SAFETY: the adapter owns the plug-in handle exclusively and never aliases
// it across threads. The vtable pointer is process-lifetime static. The
// engine drives the adapter from a single trader thread; the bound is only
// required to satisfy `DataActor: 'static + Send` blanket bounds.
unsafe impl Send for PluginActorAdapter {}

impl Debug for PluginActorAdapter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(PluginActorAdapter))
            .field("plugin_name", &self.plugin_name)
            .field("type_name", &self.type_name)
            .field("actor_id", &self.core.actor_id())
            .finish()
    }
}

impl PluginActorAdapter {
    /// Constructs a new adapter by calling the plug-in's `create` thunk.
    ///
    /// `host` must be the same vtable pointer the host handed the plug-in at
    /// load time. `actor_id` becomes the adapter's identity in the actor
    /// registry. `config_json` is forwarded verbatim to the plug-in's
    /// `PluginActor::new` so the cdylib can read instance-specific config.
    ///
    /// # Errors
    ///
    /// Returns an error if the plug-in's `create` thunk returns a null handle.
    ///
    /// # Safety
    ///
    /// `host` must be the same vtable pointer the host registered with the
    /// plug-in at load time.
    pub unsafe fn new(
        actor_id: ActorId,
        plugin_name: impl Into<String>,
        type_name: impl Into<String>,
        vtable: ValidatedActorVTable,
        host: *const HostVTable,
        config_json: &str,
    ) -> anyhow::Result<Self> {
        let plugin_name = plugin_name.into();
        let type_name = type_name.into();
        // SAFETY: vtable comes from a validated manifest entry.
        let create = unsafe { validated_slot!(ActorVTable, vtable.as_ptr(), create) };

        let ctx = leak_host_context(HostContextInner {
            actor_id,
            is_strategy: false,
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
            anyhow::anyhow!("plug-in actor '{type_name}' panicked in create")
        })?;

        if handle.is_null() {
            // SAFETY: ctx came from leak_host_context above.
            unsafe { drop_host_context(ctx) };
            anyhow::bail!("plug-in actor '{type_name}' returned a null handle from create");
        }

        let core = DataActorCore::new(DataActorConfig {
            actor_id: Some(actor_id),
            log_events: true,
            log_commands: true,
        });

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

impl Drop for PluginActorAdapter {
    fn drop(&mut self) {
        if !self.handle.is_null() {
            let _ = catch_unwind(AssertUnwindSafe(|| {
                // SAFETY: vtable + handle are live; drop_handle ignores null.
                unsafe {
                    validated_slot!(ActorVTable, self.vtable.as_ptr(), drop_handle)(self.handle);
                };
            }));
            self.handle = std::ptr::null_mut();
        }
        // SAFETY: ctx originated from leak_host_context in `new`.
        unsafe { drop_host_context(self.ctx) };
        self.ctx = std::ptr::null();
    }
}

nautilus_actor!(PluginActorAdapter);

impl DataActor for PluginActorAdapter {
    fn on_start(&mut self) -> anyhow::Result<()> {
        invoke_lifecycle(self, "on_start", |adapter| unsafe {
            validated_slot!(ActorVTable, adapter.vtable.as_ptr(), on_start)(adapter.handle)
        })
    }

    fn on_stop(&mut self) -> anyhow::Result<()> {
        invoke_lifecycle(self, "on_stop", |adapter| unsafe {
            validated_slot!(ActorVTable, adapter.vtable.as_ptr(), on_stop)(adapter.handle)
        })
    }

    fn on_resume(&mut self) -> anyhow::Result<()> {
        invoke_lifecycle(self, "on_resume", |adapter| unsafe {
            validated_slot!(ActorVTable, adapter.vtable.as_ptr(), on_resume)(adapter.handle)
        })
    }

    fn on_reset(&mut self) -> anyhow::Result<()> {
        invoke_lifecycle(self, "on_reset", |adapter| unsafe {
            validated_slot!(ActorVTable, adapter.vtable.as_ptr(), on_reset)(adapter.handle)
        })
    }

    fn on_dispose(&mut self) -> anyhow::Result<()> {
        invoke_lifecycle(self, "on_dispose", |adapter| unsafe {
            validated_slot!(ActorVTable, adapter.vtable.as_ptr(), on_dispose)(adapter.handle)
        })
    }

    fn on_degrade(&mut self) -> anyhow::Result<()> {
        invoke_lifecycle(self, "on_degrade", |adapter| unsafe {
            validated_slot!(ActorVTable, adapter.vtable.as_ptr(), on_degrade)(adapter.handle)
        })
    }

    fn on_fault(&mut self) -> anyhow::Result<()> {
        invoke_lifecycle(self, "on_fault", |adapter| unsafe {
            validated_slot!(ActorVTable, adapter.vtable.as_ptr(), on_fault)(adapter.handle)
        })
    }

    fn on_time_event(&mut self, event: &TimeEvent) -> anyhow::Result<()> {
        invoke_event(self, "on_time_event", event, |adapter, p| unsafe {
            validated_slot!(ActorVTable, adapter.vtable.as_ptr(), on_time_event)(adapter.handle, p)
        })
    }

    fn on_quote(&mut self, quote: &QuoteTick) -> anyhow::Result<()> {
        invoke_event(self, "on_quote", quote, |adapter, p| unsafe {
            validated_slot!(ActorVTable, adapter.vtable.as_ptr(), on_quote)(adapter.handle, p)
        })
    }

    fn on_trade(&mut self, trade: &TradeTick) -> anyhow::Result<()> {
        invoke_event(self, "on_trade", trade, |adapter, p| unsafe {
            validated_slot!(ActorVTable, adapter.vtable.as_ptr(), on_trade)(adapter.handle, p)
        })
    }

    fn on_bar(&mut self, bar: &Bar) -> anyhow::Result<()> {
        invoke_event(self, "on_bar", bar, |adapter, p| unsafe {
            validated_slot!(ActorVTable, adapter.vtable.as_ptr(), on_bar)(adapter.handle, p)
        })
    }

    fn on_mark_price(&mut self, mark_price: &MarkPriceUpdate) -> anyhow::Result<()> {
        invoke_event(self, "on_mark_price", mark_price, |adapter, p| unsafe {
            validated_slot!(ActorVTable, adapter.vtable.as_ptr(), on_mark_price)(adapter.handle, p)
        })
    }

    fn on_index_price(&mut self, index_price: &IndexPriceUpdate) -> anyhow::Result<()> {
        invoke_event(self, "on_index_price", index_price, |adapter, p| unsafe {
            validated_slot!(ActorVTable, adapter.vtable.as_ptr(), on_index_price)(adapter.handle, p)
        })
    }

    fn on_funding_rate(&mut self, funding_rate: &FundingRateUpdate) -> anyhow::Result<()> {
        invoke_event(self, "on_funding_rate", funding_rate, |adapter, p| unsafe {
            validated_slot!(ActorVTable, adapter.vtable.as_ptr(), on_funding_rate)(
                adapter.handle,
                p,
            )
        })
    }

    fn on_instrument_status(&mut self, data: &InstrumentStatus) -> anyhow::Result<()> {
        invoke_event(self, "on_instrument_status", data, |adapter, p| unsafe {
            validated_slot!(ActorVTable, adapter.vtable.as_ptr(), on_instrument_status)(
                adapter.handle,
                p,
            )
        })
    }

    fn on_instrument_close(&mut self, update: &InstrumentClose) -> anyhow::Result<()> {
        invoke_event(self, "on_instrument_close", update, |adapter, p| unsafe {
            validated_slot!(ActorVTable, adapter.vtable.as_ptr(), on_instrument_close)(
                adapter.handle,
                p,
            )
        })
    }

    fn on_order_filled(&mut self, event: &OrderFilled) -> anyhow::Result<()> {
        invoke_event(self, "on_order_filled", event, |adapter, p| unsafe {
            validated_slot!(ActorVTable, adapter.vtable.as_ptr(), on_order_filled)(
                adapter.handle,
                p,
            )
        })
    }

    fn on_order_canceled(&mut self, event: &OrderCanceled) -> anyhow::Result<()> {
        invoke_event(self, "on_order_canceled", event, |adapter, p| unsafe {
            validated_slot!(ActorVTable, adapter.vtable.as_ptr(), on_order_canceled)(
                adapter.handle,
                p,
            )
        })
    }

    fn on_signal(&mut self, signal: &Signal) -> anyhow::Result<()> {
        invoke_event(self, "on_signal", signal, |adapter, p| unsafe {
            validated_slot!(ActorVTable, adapter.vtable.as_ptr(), on_signal)(adapter.handle, p)
        })
    }
}

/// Wraps a call into the plug-in cdylib in `catch_unwind` so a plug-in panic
/// surfaces as `None` here instead of unwinding across the FFI boundary
/// (undefined behaviour). The plug-in's own macro-generated thunks `catch_unwind`
/// internally, so this guard is defence in depth.
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
    adapter: &PluginActorAdapter,
    method: &str,
    f: impl FnOnce(&PluginActorAdapter) -> PluginResult<()>,
) -> anyhow::Result<()> {
    let plugin_name = adapter.plugin_name.clone();
    let type_name = adapter.type_name.clone();
    let result = guard_call(&plugin_name, &type_name, method, || f(adapter));
    finish(result, &plugin_name, &type_name, method)
}

fn invoke_event<T>(
    adapter: &PluginActorAdapter,
    method: &str,
    payload: &T,
    f: impl FnOnce(&PluginActorAdapter, *const T) -> PluginResult<()>,
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
    use nautilus_plugin::surfaces::actor::{PluginActor, actor_vtable};
    use rstest::rstest;

    use super::*;
    use crate::plugin::{
        host::host_vtable,
        registry::{host_context_live_count, host_context_test_lock},
    };

    struct DropTestActor;

    impl PluginActor for DropTestActor {
        const TYPE_NAME: &'static str = "DropTestActor";

        fn new(_host: *const HostVTable, _ctx: *const HostContext, _config_json: &str) -> Self {
            Self
        }
    }

    fn drop_test_actor_vtable() -> ValidatedActorVTable {
        // SAFETY: generated vtables are process-lifetime static and fill
        // every required actor slot.
        unsafe { ValidatedActorVTable::from_raw_unchecked(actor_vtable::<DropTestActor>()) }
    }

    #[rstest]
    fn drop_frees_host_context() {
        let _guard = host_context_test_lock();
        let before = host_context_live_count();
        // SAFETY: host_vtable is process-lifetime static.
        let adapter = unsafe {
            PluginActorAdapter::new(
                ActorId::from("DropTestActor-001"),
                "plug-in",
                DropTestActor::TYPE_NAME,
                drop_test_actor_vtable(),
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
