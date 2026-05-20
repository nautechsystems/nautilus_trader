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

//! Host-side custom-data manifest walk.
//!
//! Walks each plug-in manifest's `custom_data` slice and registers a JSON
//! deserializer with [`nautilus_model::data::registry`] so the engine can
//! decode wire-format custom data emitted by the plug-in. The registered
//! deserializer wraps the plug-in's opaque handle in a host-side struct
//! implementing [`CustomDataTrait`] and routes all trait calls back through
//! the plug-in's vtable.

#![allow(unsafe_code)]
#![allow(
    clippy::multiple_unsafe_ops_per_block,
    reason = "vtable deref and FFI call form a single boundary callback; \
              SAFETY comments cover both ops together"
)]

use std::{any::Any, fmt::Debug, sync::Arc};

use nautilus_core::UnixNanos;
use nautilus_model::data::{
    CustomDataTrait, HasTsInit,
    registry::{JsonDeserializer, ensure_json_deserializer_registered},
};
use nautilus_plugin::{
    boundary::BorrowedStr,
    manifest::{CustomDataRegistration, PluginManifest},
    surfaces::custom_data::{CustomDataHandle, CustomDataVTable},
};

/// Walks a [`PluginManifest`] and registers a JSON deserializer for every
/// custom-data type the plug-in publishes.
///
/// Idempotent: re-registering a type the host has already seen is a no-op,
/// matching the behaviour of [`ensure_json_deserializer_registered`].
///
/// # Errors
///
/// Returns an error if any registration call into [`nautilus_model::data::registry`]
/// fails.
///
/// # Safety
///
/// `manifest` must originate from a successful [`nautilus_plugin`] load and
/// remain live for the process lifetime (the loader guarantees this).
pub unsafe fn register_custom_data_from_manifest(
    manifest: &PluginManifest,
) -> anyhow::Result<usize> {
    // SAFETY: caller upholds liveness of the manifest's static storage.
    let entries = unsafe { manifest.custom_data.as_slice() };
    let mut count = 0usize;

    for entry in entries {
        // SAFETY: each entry's storage is also process-lifetime static.
        unsafe { register_custom_data_entry(entry)? };
        count += 1;
    }
    Ok(count)
}

/// Registers a single custom-data type with the model data registry.
///
/// # Errors
///
/// Returns an error if [`ensure_json_deserializer_registered`] fails.
///
/// # Safety
///
/// `entry` must come from a live, valid [`PluginManifest`].
pub unsafe fn register_custom_data_entry(entry: &CustomDataRegistration) -> anyhow::Result<()> {
    // SAFETY: type_name string lives in the plug-in's static storage.
    let type_name: &'static str = unsafe { entry.type_name.as_str() };
    let vtable_ptr = entry.vtable;
    if vtable_ptr.is_null() {
        anyhow::bail!("custom data registration '{type_name}' has a null vtable");
    }

    // Address-only capture so the closure stays `Send + Sync`. The vtable
    // pointer is process-lifetime static and re-cast on each invocation.
    let vtable_addr = vtable_ptr as usize;

    let deserializer: JsonDeserializer = Box::new(move |value| {
        // Re-cast the address-captured pointer back to the vtable. The
        // pointer lives for the process lifetime per the plug-in contract.
        let vtable: *const CustomDataVTable = vtable_addr as *const _;
        let payload = serde_json::to_vec(&value)?;
        let payload_str = std::str::from_utf8(&payload)?;
        // SAFETY: vtable is non-null and live; payload_str outlives the call.
        let handle_result = unsafe { ((*vtable).from_json)(BorrowedStr::from_str(payload_str)) };
        let handle = handle_result.into_result().map_err(|e| {
            anyhow::anyhow!(
                "plug-in '{type_name}' from_json returned error: {}",
                e.message_string()
            )
        })?;

        if handle.is_null() {
            anyhow::bail!("plug-in '{type_name}' from_json returned a null handle");
        }

        Ok(Arc::new(PluginCustomDataValue {
            vtable,
            handle,
            type_name,
        }) as Arc<dyn CustomDataTrait>)
    });

    ensure_json_deserializer_registered(type_name, deserializer)
}

/// Host-side trait-object adapter for a plug-in custom-data value.
///
/// Holds an opaque handle plus a pointer to the plug-in's vtable; every
/// trait call is routed through the vtable so the host never needs to know
/// the plug-in's concrete type. Dropping the wrapper invokes the plug-in's
/// `drop_handle` thunk so the cdylib's allocator owns the value.
pub struct PluginCustomDataValue {
    vtable: *const CustomDataVTable,
    handle: *mut CustomDataHandle,
    type_name: &'static str,
}

// SAFETY: the inner handle is owned exclusively; the vtable is process-
// lifetime static. The plug-in contract requires the value type behind the
// handle to be `Send + Sync`.
unsafe impl Send for PluginCustomDataValue {}
/// SAFETY: see above.
unsafe impl Sync for PluginCustomDataValue {}

impl Debug for PluginCustomDataValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(PluginCustomDataValue))
            .field("type_name", &self.type_name)
            .finish()
    }
}

impl Drop for PluginCustomDataValue {
    fn drop(&mut self) {
        if !self.handle.is_null() {
            // SAFETY: vtable + handle are live; drop_handle ignores null.
            unsafe { ((*self.vtable).drop_handle)(self.handle) };
            self.handle = std::ptr::null_mut();
        }
    }
}

impl HasTsInit for PluginCustomDataValue {
    fn ts_init(&self) -> UnixNanos {
        // SAFETY: vtable + handle are live.
        let raw = unsafe { ((*self.vtable).ts_init)(self.handle) };
        UnixNanos::from(raw)
    }
}

impl CustomDataTrait for PluginCustomDataValue {
    fn type_name(&self) -> &'static str {
        self.type_name
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn ts_event(&self) -> UnixNanos {
        // SAFETY: vtable + handle are live.
        let raw = unsafe { ((*self.vtable).ts_event)(self.handle) };
        UnixNanos::from(raw)
    }

    fn to_json(&self) -> anyhow::Result<String> {
        // SAFETY: vtable + handle are live.
        let result = unsafe { ((*self.vtable).to_json)(self.handle) };
        let bytes = result.into_result().map_err(|e| {
            anyhow::anyhow!(
                "plug-in '{}' to_json returned error: {}",
                self.type_name,
                e.message_string()
            )
        })?;
        // SAFETY: buffer is live until `bytes` is dropped.
        let view = unsafe { bytes.as_bytes() };
        let s = std::str::from_utf8(view)?.to_owned();
        Ok(s)
    }

    fn clone_arc(&self) -> Arc<dyn CustomDataTrait> {
        // SAFETY: vtable + handle are live.
        let cloned = unsafe { ((*self.vtable).clone_handle)(self.handle) };
        Arc::new(Self {
            vtable: self.vtable,
            handle: cloned,
            type_name: self.type_name,
        })
    }

    fn eq_arc(&self, other: &dyn CustomDataTrait) -> bool {
        let Some(rhs) = other.as_any().downcast_ref::<Self>() else {
            return false;
        };

        if !std::ptr::eq(self.vtable, rhs.vtable) {
            return false;
        }
        // SAFETY: vtable + handles are live for both sides.
        unsafe { ((*self.vtable).eq_handles)(self.handle, rhs.handle) }
    }
}

#[cfg(test)]
mod tests {
    use nautilus_plugin::boundary::BorrowedStr;
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn register_custom_data_entry_rejects_null_vtable() {
        let entry = CustomDataRegistration {
            type_name: BorrowedStr::from_str("NullVTableTestType"),
            vtable: std::ptr::null(),
        };
        // SAFETY: entry has 'static storage in the local stack of this test;
        // the function only borrows it for the duration of the call.
        let r = unsafe { register_custom_data_entry(&entry) };
        let err = r.unwrap_err();
        assert!(
            err.to_string().contains("null vtable"),
            "expected null vtable error, was: {err}",
        );
    }
}
