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
    CustomData, CustomDataTrait, HasTsInit,
    registry::{JsonDeserializer, ensure_json_deserializer_registered},
};

use crate::{
    boundary::BorrowedStr,
    manifest::{
        ValidatedCustomDataRegistration, ValidatedCustomDataVTable, ValidatedPluginManifest,
    },
    surfaces::custom_data::{CustomDataHandle, PluginCustomDataRef},
};

/// Walks a [`ValidatedPluginManifest`] and registers a JSON deserializer for every
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
pub fn register_custom_data_from_manifest(
    manifest: ValidatedPluginManifest<'_>,
) -> anyhow::Result<usize> {
    let mut count = 0usize;

    for entry in manifest.custom_data() {
        register_custom_data_entry(entry)?;
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
/// The validated registration guarantees a non-null vtable with every
/// required slot present.
pub fn register_custom_data_entry(entry: ValidatedCustomDataRegistration) -> anyhow::Result<()> {
    let type_name = entry.type_name();
    let vtable = entry.vtable();
    // SAFETY: entry comes from a validated manifest registration.
    let from_json = unsafe { validated_slot!(CustomDataVTable, vtable.as_ptr(), from_json) };

    let deserializer: JsonDeserializer = Box::new(move |value| {
        let payload = serde_json::to_vec(&value)?;
        let payload_str = std::str::from_utf8(&payload)?;
        // SAFETY: vtable is non-null and live; payload_str outlives the call.
        let handle_result = unsafe { from_json(BorrowedStr::from_str(payload_str)) };
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
    vtable: ValidatedCustomDataVTable,
    handle: *mut CustomDataHandle,
    type_name: &'static str,
}

impl PluginCustomDataValue {
    /// Returns the boundary reference used for plug-in `on_data` callbacks.
    #[must_use]
    pub fn boundary_ref(&self) -> PluginCustomDataRef {
        // SAFETY: this wrapper owns a live handle allocated by `vtable`, and
        // type_name is process-lifetime static registration data.
        unsafe {
            PluginCustomDataRef::from_raw_parts(
                BorrowedStr::from_str(self.type_name),
                self.vtable.as_ptr(),
                self.handle.cast_const(),
            )
        }
    }
}

/// Returns the plug-in boundary reference for a host custom-data value when it
/// came from a plug-in custom-data registration.
#[must_use]
pub fn try_custom_data_boundary_ref(data: &CustomData) -> Option<PluginCustomDataRef> {
    data.data
        .as_any()
        .downcast_ref::<PluginCustomDataValue>()
        .map(PluginCustomDataValue::boundary_ref)
}

/// Returns the plug-in boundary reference for historical custom-data payloads
/// that carry a plug-in custom-data value.
#[must_use]
pub fn try_historical_custom_data_boundary_ref(data: &dyn Any) -> Option<PluginCustomDataRef> {
    data.downcast_ref::<CustomData>()
        .and_then(try_custom_data_boundary_ref)
}

/// Returns the plug-in boundary reference for a host custom-data value.
///
/// # Errors
///
/// Returns an error when the value was not decoded from a `PluginCustomData`
/// registration and therefore has no plug-in vtable or handle.
pub fn custom_data_boundary_ref(data: &CustomData) -> anyhow::Result<PluginCustomDataRef> {
    try_custom_data_boundary_ref(data).ok_or_else(|| {
        anyhow::anyhow!(
            "custom data type '{}' is not backed by a plug-in custom-data handle",
            data.data.type_name()
        )
    })
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
            .finish_non_exhaustive()
    }
}

impl Drop for PluginCustomDataValue {
    fn drop(&mut self) {
        if !self.handle.is_null() {
            // SAFETY: vtable + handle are live; drop_handle ignores null.
            unsafe {
                validated_slot!(CustomDataVTable, self.vtable.as_ptr(), drop_handle)(self.handle);
            };
            self.handle = std::ptr::null_mut();
        }
    }
}

impl HasTsInit for PluginCustomDataValue {
    fn ts_init(&self) -> UnixNanos {
        // SAFETY: vtable + handle are live.
        let raw = unsafe {
            validated_slot!(CustomDataVTable, self.vtable.as_ptr(), ts_init)(self.handle)
        };
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
        let raw = unsafe {
            validated_slot!(CustomDataVTable, self.vtable.as_ptr(), ts_event)(self.handle)
        };
        UnixNanos::from(raw)
    }

    fn to_json(&self) -> anyhow::Result<String> {
        // SAFETY: vtable + handle are live.
        let result = unsafe {
            validated_slot!(CustomDataVTable, self.vtable.as_ptr(), to_json)(self.handle)
        };
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
        let cloned = unsafe {
            validated_slot!(CustomDataVTable, self.vtable.as_ptr(), clone_handle)(self.handle)
        };
        // The `from_json` path rejects a null handle; clone has no error
        // channel, so a null return (a misbehaving plug-in clone_handle) would
        // otherwise be dereferenced as a live value by `ts_event` / `to_json` /
        // `eq_handles`. Fail fast here instead.
        assert!(
            !cloned.is_null(),
            "plug-in '{}' clone_handle returned a null handle",
            self.type_name
        );
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

        if self.vtable != rhs.vtable {
            return false;
        }
        // SAFETY: vtable + handles are live for both sides.
        unsafe {
            validated_slot!(CustomDataVTable, self.vtable.as_ptr(), eq_handles)(
                self.handle,
                rhs.handle,
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use nautilus_model::data::Data;
    use rstest::rstest;

    use super::*;
    use crate::{
        NAUTILUS_PLUGIN_ABI_VERSION,
        boundary::{BorrowedStr, Slice},
        manifest::{CustomDataRegistration, PluginBuildId, PluginManifest},
        surfaces::custom_data::{CustomDataVTable, PluginCustomData, custom_data_vtable},
    };

    #[derive(Clone, PartialEq)]
    struct BridgeBoundaryTick {
        value: u64,
    }

    impl PluginCustomData for BridgeBoundaryTick {
        const TYPE_NAME: &'static str = "BridgeBoundaryTick";

        fn ts_event(&self) -> u64 {
            0
        }

        fn ts_init(&self) -> u64 {
            0
        }

        fn to_json(&self) -> anyhow::Result<Vec<u8>> {
            Ok(serde_json::json!({ "value": self.value })
                .to_string()
                .into_bytes())
        }

        fn from_json(payload: &[u8]) -> anyhow::Result<Self> {
            let value: serde_json::Value = serde_json::from_slice(payload)?;
            Ok(Self {
                value: value
                    .get("value")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or_default(),
            })
        }

        fn schema_ipc() -> anyhow::Result<Vec<u8>> {
            Ok(Vec::new())
        }

        fn encode_batch(_items: &[&Self]) -> anyhow::Result<Vec<u8>> {
            Ok(Vec::new())
        }

        fn decode_batch(
            _ipc_bytes: &[u8],
            _metadata: &[(String, String)],
        ) -> anyhow::Result<Vec<Self>> {
            Ok(Vec::new())
        }
    }

    #[rstest]
    fn register_custom_data_from_manifest_rejects_null_vtable() {
        static NULL_VTABLE_CUSTOM_DATA: [CustomDataRegistration; 1] = [CustomDataRegistration {
            type_name: BorrowedStr::from_str("NullVTableTestType"),
            vtable: std::ptr::null(),
        }];
        let manifest = PluginManifest {
            abi_version: NAUTILUS_PLUGIN_ABI_VERSION,
            plugin_name: BorrowedStr::from_str("test-plugin"),
            plugin_vendor: BorrowedStr::from_str("nautech"),
            plugin_version: BorrowedStr::from_str("0.0.0"),
            build_id: PluginBuildId::current(),
            custom_data: Slice::from_slice(&NULL_VTABLE_CUSTOM_DATA),
            actors: Slice::empty(),
            strategies: Slice::empty(),
            controllers: Slice::empty(),
        };

        let r = ValidatedPluginManifest::new(&manifest);
        let err = r.unwrap_err();
        assert!(
            err.to_string()
                .contains("custom_data[0].vtable must not be null"),
            "expected null vtable error, was: {err}",
        );
    }

    #[rstest]
    fn custom_data_boundary_ref_accepts_plugin_custom_data() {
        let custom_data = Box::leak(Box::new([CustomDataRegistration {
            type_name: BorrowedStr::from_str(BridgeBoundaryTick::TYPE_NAME),
            vtable: custom_data_vtable::<BridgeBoundaryTick>(),
        }]));
        let manifest = PluginManifest {
            abi_version: NAUTILUS_PLUGIN_ABI_VERSION,
            plugin_name: BorrowedStr::from_str("test-plugin"),
            plugin_vendor: BorrowedStr::from_str("nautech"),
            plugin_version: BorrowedStr::from_str("0.0.0"),
            build_id: PluginBuildId::current(),
            custom_data: Slice::from_slice(custom_data),
            actors: Slice::empty(),
            strategies: Slice::empty(),
            controllers: Slice::empty(),
        };
        let manifest =
            ValidatedPluginManifest::new(&manifest).expect("test manifest passes validation");
        register_custom_data_from_manifest(manifest).expect("custom data registration succeeds");
        let envelope = serde_json::json!({
            "type": "Custom",
            "data_type": {
                "type_name": BridgeBoundaryTick::TYPE_NAME,
            },
            "payload": {
                "value": 42,
            },
        });
        let data = nautilus_model::data::registry::deserialize_custom_from_json(
            BridgeBoundaryTick::TYPE_NAME,
            &envelope,
        )
        .expect("deserializer succeeds")
        .expect("custom data type is registered");
        let Data::Custom(custom) = data else {
            panic!("expected Custom variant");
        };
        let data_ref =
            custom_data_boundary_ref(&custom).expect("plug-in custom data has boundary ref");
        let value = data_ref
            .downcast_ref::<BridgeBoundaryTick>()
            .expect("boundary ref downcasts to registered plug-in type");

        assert_eq!(value.value, 42);
    }

    #[rstest]
    fn custom_data_boundary_ref_rejects_non_plugin_custom_data() {
        let data = nautilus_model::data::stubs::stub_custom_data(1, 42, None, None);
        let Err(e) = custom_data_boundary_ref(&data) else {
            panic!("expected non-plugin custom data to fail");
        };

        assert!(
            e.to_string()
                .contains("not backed by a plug-in custom-data handle"),
            "expected non-plugin custom-data error, was: {e}",
        );
    }

    #[derive(Clone, PartialEq)]
    struct NonUtf8Tick;

    impl PluginCustomData for NonUtf8Tick {
        const TYPE_NAME: &'static str = "NonUtf8Tick";

        fn ts_event(&self) -> u64 {
            0
        }

        fn ts_init(&self) -> u64 {
            0
        }

        fn to_json(&self) -> anyhow::Result<Vec<u8>> {
            Ok(vec![0xff, 0xfe])
        }

        fn from_json(_payload: &[u8]) -> anyhow::Result<Self> {
            Ok(Self)
        }

        fn schema_ipc() -> anyhow::Result<Vec<u8>> {
            Ok(Vec::new())
        }

        fn encode_batch(_items: &[&Self]) -> anyhow::Result<Vec<u8>> {
            Ok(Vec::new())
        }

        fn decode_batch(
            _ipc_bytes: &[u8],
            _metadata: &[(String, String)],
        ) -> anyhow::Result<Vec<Self>> {
            Ok(Vec::new())
        }
    }

    #[rstest]
    fn to_json_surfaces_non_utf8_payload_as_error() {
        let vtable = custom_data_vtable::<NonUtf8Tick>();
        let handle = Box::into_raw(Box::new(NonUtf8Tick)).cast::<CustomDataHandle>();
        let value = PluginCustomDataValue {
            // SAFETY: generated vtable fills every slot; handle came from Box::into_raw.
            vtable: unsafe { ValidatedCustomDataVTable::from_raw_unchecked(vtable) },
            handle,
            type_name: NonUtf8Tick::TYPE_NAME,
        };

        let err = value
            .to_json()
            .expect_err("non-utf8 to_json payload should surface as an error");

        assert!(
            err.to_string().contains("utf-8"),
            "expected utf-8 decode error, was: {err}",
        );
        // `value` drops here, freeing the handle via the real drop_handle thunk.
    }

    #[rstest]
    #[should_panic(expected = "clone_handle returned a null handle")]
    fn clone_arc_panics_when_plugin_clone_returns_null() {
        unsafe extern "C" fn null_clone(_handle: *const CustomDataHandle) -> *mut CustomDataHandle {
            std::ptr::null_mut()
        }

        let valid = custom_data_vtable::<BridgeBoundaryTick>();
        // SAFETY: generated test vtable lives for the process lifetime.
        let valid = unsafe { &*valid };
        let vtable = Box::leak(Box::new(CustomDataVTable {
            type_name: valid.type_name,
            schema_ipc: valid.schema_ipc,
            from_json: valid.from_json,
            encode_batch: valid.encode_batch,
            decode_batch: valid.decode_batch,
            ts_event: valid.ts_event,
            ts_init: valid.ts_init,
            to_json: valid.to_json,
            clone_handle: Some(null_clone),
            drop_handle: valid.drop_handle,
            eq_handles: valid.eq_handles,
        }));
        let handle =
            Box::into_raw(Box::new(BridgeBoundaryTick { value: 5 })).cast::<CustomDataHandle>();
        let value = PluginCustomDataValue {
            // SAFETY: the copied vtable fills every required slot; handle came
            // from Box::into_raw.
            vtable: unsafe {
                ValidatedCustomDataVTable::from_raw_unchecked(std::ptr::from_ref(&*vtable))
            },
            handle,
            type_name: BridgeBoundaryTick::TYPE_NAME,
        };

        // clone_handle returns null, so clone_arc must panic rather than wrap a
        // null handle that later thunks would dereference.
        let _ = value.clone_arc();
    }
}
