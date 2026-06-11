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

//! Host-side adapter that owns a plug-in controller instance.

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

use nautilus_common::timer::TimeEvent;

use crate::{
    boundary::{BorrowedStr, OwnedBytes, PluginResult},
    bridge::registry::{
        ControllerHostContextInner, drop_controller_host_context, leak_controller_host_context,
    },
    host::{ControllerHostContext, ControllerHostVTable},
    manifest::ValidatedControllerVTable,
    surfaces::controller::PluginControllerHandle,
};

/// Adapts a plug-in controller (vtable + handle from a cdylib) into a
/// host-owned runtime component.
pub struct PluginControllerAdapter {
    plugin_name: String,
    type_name: String,
    vtable: ValidatedControllerVTable,
    handle: *mut PluginControllerHandle,
    ctx: *const ControllerHostContext,
}

// SAFETY: the adapter owns the plug-in handle exclusively and never aliases
// it across threads. The vtable pointer is process-lifetime static. Live-node
// lifecycle dispatch runs on the node thread; the bound is needed for host
// containers that require `Send`.
unsafe impl Send for PluginControllerAdapter {}

impl Debug for PluginControllerAdapter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(PluginControllerAdapter))
            .field("plugin_name", &self.plugin_name)
            .field("type_name", &self.type_name)
            .finish()
    }
}

impl PluginControllerAdapter {
    /// Constructs a new adapter by calling the plug-in's `create` thunk.
    ///
    /// `host` must point at a process-lifetime [`ControllerHostVTable`].
    /// `config_json` is forwarded verbatim to the plug-in's
    /// `PluginController::new` implementation.
    ///
    /// # Errors
    ///
    /// Returns an error if the plug-in's `create` thunk panics or returns a
    /// null handle.
    ///
    /// # Safety
    ///
    /// `host` must outlive the adapter and all controller callbacks.
    pub unsafe fn new(
        plugin_name: impl Into<String>,
        type_name: impl Into<String>,
        vtable: ValidatedControllerVTable,
        host: *const ControllerHostVTable,
        config_json: &str,
    ) -> anyhow::Result<Self> {
        let plugin_name = plugin_name.into();
        let type_name = type_name.into();
        // SAFETY: vtable comes from a validated manifest entry.
        let create = unsafe { validated_slot!(ControllerVTable, vtable.as_ptr(), create) };
        let ctx = leak_controller_host_context(ControllerHostContextInner {
            plugin_name: plugin_name.clone(),
            type_name: type_name.clone(),
        });

        let cfg = BorrowedStr::from_str(config_json);
        // SAFETY: vtable is non-null, host outlives the adapter, ctx + cfg
        // are live across the call.
        let handle = guard_call(&plugin_name, &type_name, "create", || unsafe {
            create(host, ctx, cfg)
        })
        .ok_or_else(|| {
            // SAFETY: ctx came from leak_controller_host_context above.
            unsafe { drop_controller_host_context(ctx) };
            anyhow::anyhow!("plug-in controller '{type_name}' panicked in create")
        })?;

        if handle.is_null() {
            // SAFETY: ctx came from leak_controller_host_context above.
            unsafe { drop_controller_host_context(ctx) };
            anyhow::bail!(
                "plug-in controller '{type_name}' returned a null handle from create (constructor failure or panic)"
            );
        }

        Ok(Self {
            plugin_name,
            type_name,
            vtable,
            handle,
            ctx,
        })
    }

    /// Runs the controller's static `prepare` hook.
    ///
    /// # Errors
    ///
    /// Returns an error if the plug-in rejects the request.
    pub fn prepare(&self, request_json: &str) -> anyhow::Result<OwnedBytes> {
        let request = BorrowedStr::from_str(request_json);
        let result = guard_call(&self.plugin_name, &self.type_name, "prepare", || unsafe {
            validated_slot!(ControllerVTable, self.vtable.as_ptr(), prepare)(request)
        });
        finish_bytes(result, &self.plugin_name, &self.type_name, "prepare")
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

    /// Dispatches `on_start` to the plug-in controller.
    ///
    /// # Errors
    ///
    /// Returns an error if the plug-in callback fails.
    pub fn on_start(&mut self) -> anyhow::Result<()> {
        invoke_lifecycle(self, "on_start", |adapter| unsafe {
            validated_slot!(ControllerVTable, adapter.vtable.as_ptr(), on_start)(adapter.handle)
        })
    }

    /// Dispatches `on_stop` to the plug-in controller.
    ///
    /// # Errors
    ///
    /// Returns an error if the plug-in callback fails.
    pub fn on_stop(&mut self) -> anyhow::Result<()> {
        invoke_lifecycle(self, "on_stop", |adapter| unsafe {
            validated_slot!(ControllerVTable, adapter.vtable.as_ptr(), on_stop)(adapter.handle)
        })
    }

    /// Dispatches `on_resume` to the plug-in controller.
    ///
    /// # Errors
    ///
    /// Returns an error if the plug-in callback fails.
    pub fn on_resume(&mut self) -> anyhow::Result<()> {
        invoke_lifecycle(self, "on_resume", |adapter| unsafe {
            validated_slot!(ControllerVTable, adapter.vtable.as_ptr(), on_resume)(adapter.handle)
        })
    }

    /// Dispatches `on_reset` to the plug-in controller.
    ///
    /// # Errors
    ///
    /// Returns an error if the plug-in callback fails.
    pub fn on_reset(&mut self) -> anyhow::Result<()> {
        invoke_lifecycle(self, "on_reset", |adapter| unsafe {
            validated_slot!(ControllerVTable, adapter.vtable.as_ptr(), on_reset)(adapter.handle)
        })
    }

    /// Dispatches `on_dispose` to the plug-in controller.
    ///
    /// # Errors
    ///
    /// Returns an error if the plug-in callback fails.
    pub fn on_dispose(&mut self) -> anyhow::Result<()> {
        invoke_lifecycle(self, "on_dispose", |adapter| unsafe {
            validated_slot!(ControllerVTable, adapter.vtable.as_ptr(), on_dispose)(adapter.handle)
        })
    }

    /// Dispatches `on_degrade` to the plug-in controller.
    ///
    /// # Errors
    ///
    /// Returns an error if the plug-in callback fails.
    pub fn on_degrade(&mut self) -> anyhow::Result<()> {
        invoke_lifecycle(self, "on_degrade", |adapter| unsafe {
            validated_slot!(ControllerVTable, adapter.vtable.as_ptr(), on_degrade)(adapter.handle)
        })
    }

    /// Dispatches `on_fault` to the plug-in controller.
    ///
    /// # Errors
    ///
    /// Returns an error if the plug-in callback fails.
    pub fn on_fault(&mut self) -> anyhow::Result<()> {
        invoke_lifecycle(self, "on_fault", |adapter| unsafe {
            validated_slot!(ControllerVTable, adapter.vtable.as_ptr(), on_fault)(adapter.handle)
        })
    }

    /// Dispatches `on_time_event` to the plug-in controller.
    ///
    /// # Errors
    ///
    /// Returns an error if the plug-in callback fails.
    pub fn on_time_event(&mut self, event: &TimeEvent) -> anyhow::Result<()> {
        invoke_event(self, "on_time_event", event, |adapter, p| unsafe {
            validated_slot!(ControllerVTable, adapter.vtable.as_ptr(), on_time_event)(
                adapter.handle,
                p,
            )
        })
    }
}

impl Drop for PluginControllerAdapter {
    fn drop(&mut self) {
        if !self.handle.is_null() {
            let _ = catch_unwind(AssertUnwindSafe(|| {
                // SAFETY: vtable + handle are live; drop_handle ignores null.
                unsafe {
                    validated_slot!(ControllerVTable, self.vtable.as_ptr(), drop_handle)(
                        self.handle,
                    );
                };
            }));
            self.handle = std::ptr::null_mut();
        }
        // SAFETY: ctx originated from leak_controller_host_context in `new`.
        unsafe { drop_controller_host_context(self.ctx) };
        self.ctx = std::ptr::null();
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
    adapter: &PluginControllerAdapter,
    method: &str,
    f: impl FnOnce(&PluginControllerAdapter) -> PluginResult<()>,
) -> anyhow::Result<()> {
    let plugin_name = adapter.plugin_name.clone();
    let type_name = adapter.type_name.clone();
    let result = guard_call(&plugin_name, &type_name, method, || f(adapter));
    finish(result, &plugin_name, &type_name, method)
}

fn invoke_event<T>(
    adapter: &PluginControllerAdapter,
    method: &str,
    payload: &T,
    f: impl FnOnce(&PluginControllerAdapter, *const T) -> PluginResult<()>,
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

fn finish_bytes(
    result: Option<PluginResult<OwnedBytes>>,
    plugin_name: &str,
    type_name: &str,
    method: &str,
) -> anyhow::Result<OwnedBytes> {
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
    use std::sync::atomic::{AtomicU64, Ordering};

    use rstest::rstest;

    use super::*;
    use crate::{
        bridge::{
            host::controller_host_vtable,
            registry::{controller_host_context_live_count, controller_host_context_test_lock},
        },
        host::{ControllerHostContext, ControllerHostVTable},
        surfaces::controller::{ControllerVTable, PluginController, controller_vtable},
    };

    static STARTS: AtomicU64 = AtomicU64::new(0);

    struct DropTestController;

    impl PluginController for DropTestController {
        const TYPE_NAME: &'static str = "DropTestController";

        fn new(
            _host: *const ControllerHostVTable,
            _ctx: *const ControllerHostContext,
            _config_json: &str,
        ) -> Self {
            Self
        }

        fn on_start(&mut self) -> anyhow::Result<()> {
            STARTS.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    fn drop_test_controller_vtable() -> ValidatedControllerVTable {
        // SAFETY: generated vtables are process-lifetime static and fill
        // every required controller slot.
        unsafe {
            ValidatedControllerVTable::from_raw_unchecked(controller_vtable::<DropTestController>())
        }
    }

    static NULL_CREATE_VTABLE: ControllerVTable = ControllerVTable {
        prepare: Some(null_create_prepare),
        create: Some(null_create),
        drop_handle: Some(null_create_drop_handle),
        type_name: Some(null_create_type_name),
        on_start: Some(null_create_lifecycle),
        on_stop: Some(null_create_lifecycle),
        on_resume: Some(null_create_lifecycle),
        on_reset: Some(null_create_lifecycle),
        on_dispose: Some(null_create_lifecycle),
        on_degrade: Some(null_create_lifecycle),
        on_fault: Some(null_create_lifecycle),
        on_time_event: Some(null_create_time_event),
    };

    unsafe extern "C" fn null_create_prepare(
        _request_json: BorrowedStr<'_>,
    ) -> PluginResult<OwnedBytes> {
        PluginResult::Ok(OwnedBytes::empty())
    }

    unsafe extern "C" fn null_create(
        _host: *const ControllerHostVTable,
        _ctx: *const ControllerHostContext,
        _config_json: BorrowedStr<'_>,
    ) -> *mut PluginControllerHandle {
        std::ptr::null_mut()
    }

    unsafe extern "C" fn null_create_drop_handle(_handle: *mut PluginControllerHandle) {}

    unsafe extern "C" fn null_create_type_name() -> BorrowedStr<'static> {
        BorrowedStr::from_str("NullCreateController")
    }

    unsafe extern "C" fn null_create_lifecycle(
        _handle: *mut PluginControllerHandle,
    ) -> PluginResult<()> {
        PluginResult::Ok(())
    }

    unsafe extern "C" fn null_create_time_event(
        _handle: *mut PluginControllerHandle,
        _event: *const TimeEvent,
    ) -> PluginResult<()> {
        PluginResult::Ok(())
    }

    fn null_create_vtable() -> ValidatedControllerVTable {
        // SAFETY: the test vtable is process-lifetime static and fills every
        // required slot, but intentionally returns a null handle.
        unsafe { ValidatedControllerVTable::from_raw_unchecked(&raw const NULL_CREATE_VTABLE) }
    }

    #[rstest]
    fn drop_frees_controller_host_context() {
        let _guard = controller_host_context_test_lock();
        let before = controller_host_context_live_count();
        // SAFETY: controller_host_vtable is process-lifetime static.
        let adapter = unsafe {
            PluginControllerAdapter::new(
                "test-plugin",
                DropTestController::TYPE_NAME,
                drop_test_controller_vtable(),
                controller_host_vtable(),
                "{}",
            )
        }
        .expect("controller adapter construction succeeds");
        assert_eq!(controller_host_context_live_count(), before + 1);

        drop(adapter);

        assert_eq!(controller_host_context_live_count(), before);
    }

    #[rstest]
    fn null_create_frees_controller_host_context() {
        let _guard = controller_host_context_test_lock();
        let before = controller_host_context_live_count();

        // SAFETY: controller_host_vtable is process-lifetime static.
        let error = unsafe {
            PluginControllerAdapter::new(
                "test-plugin",
                "NullCreateController",
                null_create_vtable(),
                controller_host_vtable(),
                "{}",
            )
        }
        .expect_err("null controller handle is rejected");

        assert!(
            error
                .to_string()
                .contains("returned a null handle from create")
        );
        assert_eq!(controller_host_context_live_count(), before);
    }

    #[rstest]
    fn lifecycle_dispatches_to_controller() {
        let _guard = controller_host_context_test_lock();
        STARTS.store(0, Ordering::SeqCst);
        // SAFETY: controller_host_vtable is process-lifetime static.
        let mut adapter = unsafe {
            PluginControllerAdapter::new(
                "test-plugin",
                DropTestController::TYPE_NAME,
                drop_test_controller_vtable(),
                controller_host_vtable(),
                "{}",
            )
        }
        .expect("controller adapter construction succeeds");

        adapter.on_start().expect("on_start dispatches");

        assert_eq!(STARTS.load(Ordering::SeqCst), 1);
    }
}
