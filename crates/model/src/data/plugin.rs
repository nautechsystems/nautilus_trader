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

#![allow(unsafe_code)]

//! ABI-stable plugin surface for external PyO3 custom data types.

use std::{
    collections::HashMap,
    ffi::c_void,
    io::Cursor,
    ptr::NonNull,
    sync::{Arc, OnceLock},
};

use abi_stable::{
    StableAbi,
    std_types::{RResult, RString, RVec},
};
use arrow::{
    datatypes::Schema,
    ipc::{reader::StreamReader, writer::StreamWriter},
    record_batch::RecordBatch,
};
#[cfg(feature = "python")]
use dashmap::DashMap;
#[cfg(feature = "python")]
use nautilus_core::python::to_pyruntime_err;
#[cfg(feature = "python")]
use pyo3::{prelude::*, types::PyCapsule};

use crate::data::CustomDataTrait;

#[repr(C)]
#[derive(Clone, Debug, StableAbi)]
pub struct PluginMetadataEntry {
    pub key: RString,
    pub value: RString,
}

#[repr(C)]
#[derive(Debug, StableAbi)]
pub struct ExternalCustomDataHandleVTable {
    pub clone_handle: extern "C" fn(*const c_void) -> ExternalCustomDataHandle,
    pub drop_handle: extern "C" fn(*mut c_void),
    pub type_name: extern "C" fn(*const c_void) -> RString,
    pub ts_event: extern "C" fn(*const c_void) -> u64,
    pub ts_init: extern "C" fn(*const c_void) -> u64,
    pub to_json: extern "C" fn(*const c_void) -> RResult<RString, RString>,
    pub eq_handle: extern "C" fn(*const c_void, *const c_void) -> bool,
}

#[repr(C)]
#[derive(Debug, StableAbi)]
pub struct ExternalCustomDataHandle {
    ptr: *mut c_void,
    vtable: *const ExternalCustomDataHandleVTable,
}

impl Clone for ExternalCustomDataHandle {
    fn clone(&self) -> Self {
        (self.vtable().clone_handle)(self.ptr.cast_const())
    }
}

impl Drop for ExternalCustomDataHandle {
    fn drop(&mut self) {
        if !self.ptr.is_null() {
            (self.vtable().drop_handle)(self.ptr);
            self.ptr = std::ptr::null_mut();
        }
    }
}

// SAFETY: The handle only wraps types constrained to `Send + Sync`.
unsafe impl Send for ExternalCustomDataHandle {}
// SAFETY: The handle only wraps types constrained to `Send + Sync`.
unsafe impl Sync for ExternalCustomDataHandle {}

impl ExternalCustomDataHandle {
    pub fn new<T>(value: T) -> Self
    where
        T: CustomDataTrait + Clone + Send + Sync + 'static,
    {
        let boxed = Box::new(value);
        Self {
            ptr: Box::into_raw(boxed).cast(),
            vtable: std::ptr::from_ref(external_custom_data_handle_vtable::<T>()),
        }
    }

    fn vtable(&self) -> &'static ExternalCustomDataHandleVTable {
        // SAFETY: `vtable` is always initialized from a `'static` vtable.
        unsafe { &*self.vtable }
    }

    fn check_vtable<T>(&self) -> anyhow::Result<()>
    where
        T: CustomDataTrait + Clone + Send + Sync + 'static,
    {
        let expected = std::ptr::from_ref(external_custom_data_handle_vtable::<T>());
        anyhow::ensure!(
            self.vtable == expected,
            "Expected {}",
            T::type_name_static()
        );
        Ok(())
    }

    pub fn type_name(&self) -> String {
        (self.vtable().type_name)(self.ptr.cast_const()).into_string()
    }

    pub fn ts_event(&self) -> u64 {
        (self.vtable().ts_event)(self.ptr.cast_const())
    }

    pub fn ts_init(&self) -> u64 {
        (self.vtable().ts_init)(self.ptr.cast_const())
    }

    /// Serializes the handle to JSON.
    ///
    /// # Errors
    ///
    /// Returns an error if the plugin's `to_json` callback fails.
    pub fn to_json(&self) -> anyhow::Result<String> {
        (self.vtable().to_json)(self.ptr.cast_const())
            .into_result()
            .map(RString::into_string)
            .map_err(|e: RString| anyhow::anyhow!(e.into_string()))
    }

    pub fn eq_handle(&self, other: &Self) -> bool {
        self.vtable == other.vtable
            && (self.vtable().eq_handle)(self.ptr.cast_const(), other.ptr.cast_const())
    }

    /// Clones the handle as the concrete type `T` if the vtable matches.
    ///
    /// # Errors
    ///
    /// Returns an error if the handle's vtable does not match type `T`.
    ///
    /// # Panics
    ///
    /// Panics if the internal pointer is null (invalid handle).
    pub fn try_clone_as<T>(&self) -> anyhow::Result<T>
    where
        T: CustomDataTrait + Clone + Send + Sync + 'static,
    {
        self.check_vtable::<T>()?;
        // SAFETY: VTable identity proves the concrete type stored in `ptr`.
        unsafe {
            Ok((self.ptr.cast::<T>().as_ref())
                .expect("non-null handle")
                .clone())
        }
    }
}

#[repr(C)]
#[derive(Clone, Debug, StableAbi)]
pub struct ExternalCustomDataPlugin {
    pub abi_version: u32,
    pub type_name: extern "C" fn() -> RString,
    pub schema_ipc: extern "C" fn() -> RResult<RVec<u8>, RString>,
    pub from_json: extern "C" fn(RString) -> RResult<ExternalCustomDataHandle, RString>,
    pub encode_batch: extern "C" fn(RVec<ExternalCustomDataHandle>) -> RResult<RVec<u8>, RString>,
    pub decode_batch: extern "C" fn(
        RVec<PluginMetadataEntry>,
        RVec<u8>,
    ) -> RResult<RVec<ExternalCustomDataHandle>, RString>,
}

impl ExternalCustomDataPlugin {
    pub const ABI_VERSION: u32 = 1;

    pub fn type_name_string(&self) -> String {
        (self.type_name)().into_string()
    }

    /// Returns the Arrow schema for this plugin (from IPC bytes).
    ///
    /// # Errors
    ///
    /// Returns an error if schema IPC bytes are invalid.
    pub fn schema(&self) -> anyhow::Result<Arc<Schema>> {
        let bytes = (self.schema_ipc)()
            .into_result()
            .map_err(|e: RString| anyhow::anyhow!(e.into_string()))?;
        schema_from_ipc_bytes(bytes.as_slice())
    }

    /// Decodes a handle from JSON.
    ///
    /// # Errors
    ///
    /// Returns an error if the plugin's `from_json` callback fails.
    pub fn decode_from_json(&self, json: String) -> anyhow::Result<ExternalCustomDataHandle> {
        (self.from_json)(RString::from(json))
            .into_result()
            .map_err(|e: RString| anyhow::anyhow!(e.into_string()))
    }

    /// Encodes handles into an Arrow record batch.
    ///
    /// # Errors
    ///
    /// Returns an error if the plugin's `encode_batch` callback or IPC write fails.
    pub fn encode_handles(
        &self,
        handles: Vec<ExternalCustomDataHandle>,
    ) -> anyhow::Result<RecordBatch> {
        let bytes = (self.encode_batch)(RVec::from(handles))
            .into_result()
            .map_err(|e: RString| anyhow::anyhow!(e.into_string()))?;
        record_batch_from_ipc_bytes(bytes.as_slice())
    }

    /// Decodes handles from an Arrow record batch.
    ///
    /// # Errors
    ///
    /// Returns an error if the plugin's `decode_batch` callback or IPC read fails.
    pub fn decode_handles(
        &self,
        metadata: &HashMap<String, String>,
        batch: RecordBatch,
    ) -> anyhow::Result<Vec<ExternalCustomDataHandle>> {
        let metadata_entries: Vec<PluginMetadataEntry> = metadata
            .iter()
            .map(|(key, value)| PluginMetadataEntry {
                key: RString::from(key.clone()),
                value: RString::from(value.clone()),
            })
            .collect();
        let bytes = record_batch_to_ipc_bytes(&batch)?;
        (self.decode_batch)(RVec::from(metadata_entries), RVec::from(bytes))
            .into_result()
            .map(|items| items.into_iter().collect())
            .map_err(|e: RString| anyhow::anyhow!(e.into_string()))
    }
}

pub fn ok<T>(value: T) -> RResult<T, RString> {
    RResult::ROk(value)
}

pub fn err_value<T, E: std::fmt::Display>(error: E) -> RResult<T, RString> {
    RResult::RErr(RString::from(error.to_string()))
}

/// Serializes a record batch to Arrow IPC bytes.
///
/// # Errors
///
/// Returns an error if IPC writing fails.
pub fn record_batch_to_ipc_bytes(batch: &RecordBatch) -> anyhow::Result<Vec<u8>> {
    let mut cursor = Cursor::new(Vec::new());
    {
        let mut writer = StreamWriter::try_new(&mut cursor, &batch.schema())?;
        writer.write(batch)?;
        writer.finish()?;
    }
    Ok(cursor.into_inner())
}

/// Deserializes a record batch from Arrow IPC bytes.
///
/// # Errors
///
/// Returns an error if the payload is invalid or contains no record batch.
pub fn record_batch_from_ipc_bytes(bytes: &[u8]) -> anyhow::Result<RecordBatch> {
    let mut reader = StreamReader::try_new(Cursor::new(bytes.to_vec()), None)?;
    reader
        .next()
        .transpose()?
        .ok_or_else(|| anyhow::anyhow!("No record batch found in IPC payload"))
}

/// Serializes an Arrow schema to IPC bytes.
///
/// # Errors
///
/// Returns an error if IPC writing fails.
pub fn schema_to_ipc_bytes(schema: &Schema) -> anyhow::Result<Vec<u8>> {
    let mut cursor = Cursor::new(Vec::new());
    {
        let mut writer = StreamWriter::try_new(&mut cursor, schema)?;
        writer.finish()?;
    }
    Ok(cursor.into_inner())
}

/// Deserializes an Arrow schema from IPC bytes.
///
/// # Errors
///
/// Returns an error if the payload is invalid.
pub fn schema_from_ipc_bytes(bytes: &[u8]) -> anyhow::Result<Arc<Schema>> {
    let reader = StreamReader::try_new(Cursor::new(bytes.to_vec()), None)?;
    Ok(reader.schema())
}

extern "C" fn clone_handle_impl<T>(ptr: *const c_void) -> ExternalCustomDataHandle
where
    T: CustomDataTrait + Clone + Send + Sync + 'static,
{
    // SAFETY: `ptr` always comes from `ExternalCustomDataHandle::new::<T>`.
    let value = unsafe { &*(ptr.cast::<T>()) };
    ExternalCustomDataHandle::new(value.clone())
}

extern "C" fn drop_handle_impl<T>(ptr: *mut c_void)
where
    T: CustomDataTrait + Clone + Send + Sync + 'static,
{
    if let Some(ptr) = NonNull::new(ptr.cast::<T>()) {
        // SAFETY: `ptr` was allocated with `Box::into_raw` in `ExternalCustomDataHandle::new`.
        unsafe {
            drop(Box::from_raw(ptr.as_ptr()));
        }
    }
}

extern "C" fn type_name_impl<T>(_ptr: *const c_void) -> RString
where
    T: CustomDataTrait + Clone + Send + Sync + 'static,
{
    RString::from(T::type_name_static())
}

extern "C" fn ts_event_impl<T>(ptr: *const c_void) -> u64
where
    T: CustomDataTrait + Clone + Send + Sync + 'static,
{
    // SAFETY: `ptr` always comes from `ExternalCustomDataHandle::new::<T>`.
    unsafe { (&*(ptr.cast::<T>())).ts_event().as_u64() }
}

extern "C" fn ts_init_impl<T>(ptr: *const c_void) -> u64
where
    T: CustomDataTrait + Clone + Send + Sync + 'static,
{
    // SAFETY: `ptr` always comes from `ExternalCustomDataHandle::new::<T>`.
    unsafe { (&*(ptr.cast::<T>())).ts_init().as_u64() }
}

extern "C" fn to_json_impl<T>(ptr: *const c_void) -> RResult<RString, RString>
where
    T: CustomDataTrait + Clone + Send + Sync + 'static,
{
    // SAFETY: `ptr` always comes from `ExternalCustomDataHandle::new::<T>`.
    let value = unsafe { &*(ptr.cast::<T>()) };
    match value.to_json() {
        Ok(json) => ok(RString::from(json)),
        Err(e) => err_value(e),
    }
}

extern "C" fn eq_handle_impl<T>(lhs: *const c_void, rhs: *const c_void) -> bool
where
    T: CustomDataTrait + Clone + Send + Sync + 'static,
{
    // SAFETY: Both pointers always come from `ExternalCustomDataHandle::new::<T>`.
    let lhs = unsafe { &*(lhs.cast::<T>()) };
    let rhs = unsafe { &*(rhs.cast::<T>()) };
    lhs.to_json().ok() == rhs.to_json().ok()
}

fn external_custom_data_handle_vtable<T>() -> &'static ExternalCustomDataHandleVTable
where
    T: CustomDataTrait + Clone + Send + Sync + 'static,
{
    static VTABLE: OnceLock<ExternalCustomDataHandleVTable> = OnceLock::new();
    VTABLE.get_or_init(|| ExternalCustomDataHandleVTable {
        clone_handle: clone_handle_impl::<T>,
        drop_handle: drop_handle_impl::<T>,
        type_name: type_name_impl::<T>,
        ts_event: ts_event_impl::<T>,
        ts_init: ts_init_impl::<T>,
        to_json: to_json_impl::<T>,
        eq_handle: eq_handle_impl::<T>,
    })
}

#[cfg(feature = "python")]
fn python_data_classes() -> &'static DashMap<String, Py<PyAny>> {
    static PYTHON_DATA_CLASSES: OnceLock<DashMap<String, Py<PyAny>>> = OnceLock::new();
    PYTHON_DATA_CLASSES.get_or_init(DashMap::new)
}

#[cfg(feature = "python")]
pub fn register_python_data_class(type_name: &str, data_class: &Bound<'_, PyAny>) {
    python_data_classes().insert(type_name.to_string(), data_class.clone().unbind());
}

#[cfg(feature = "python")]
pub fn get_python_data_class(py: Python<'_>, type_name: &str) -> Option<Py<PyAny>> {
    python_data_classes()
        .get(type_name)
        .map(|entry| entry.value().clone_ref(py))
}

/// Reconstructs a Python custom data instance from type name and JSON.
///
/// # Errors
///
/// Returns a Python error if no class is registered for `type_name` or JSON parsing fails.
#[cfg(feature = "python")]
pub fn reconstruct_python_custom_data(
    py: Python<'_>,
    type_name: &str,
    json: &str,
) -> PyResult<Py<PyAny>> {
    let data_class = get_python_data_class(py, type_name).ok_or_else(|| {
        to_pyruntime_err(format!(
            "No registered Python class for custom data type `{type_name}`"
        ))
    })?;
    let json_module = py.import("json")?;
    let payload = json_module.call_method1("loads", (json,))?;
    data_class
        .bind(py)
        .call_method1("from_json", (payload,))
        .map(|obj| obj.unbind())
}

/// Wraps a custom data value in a PyCapsule as an external handle.
///
/// # Errors
///
/// Returns a Python error if capsule creation fails.
#[cfg(feature = "python")]
pub fn custom_data_handle_capsule<T>(py: Python<'_>, value: &T) -> PyResult<Py<PyAny>>
where
    T: CustomDataTrait + Clone + Send + Sync + 'static,
{
    let handle = ExternalCustomDataHandle::new(value.clone());
    PyCapsule::new_with_destructor(py, handle, None, |_, _| {})
        .map(|capsule| capsule.into_any().unbind())
}

/// Extracts an `ExternalCustomDataPlugin` from a PyCapsule.
///
/// # Errors
///
/// Returns a Python error if the capsule is invalid or holds the wrong type.
#[cfg(feature = "python")]
pub fn plugin_from_capsule(capsule: &Bound<'_, PyAny>) -> PyResult<ExternalCustomDataPlugin> {
    let capsule = capsule.cast::<PyCapsule>()?;
    let ptr = capsule.pointer_checked(None)?;
    // SAFETY: Capsule stores an `ExternalCustomDataPlugin` value created by
    // `custom_data_plugin_capsule`.
    let plugin = unsafe { &*(ptr.as_ptr() as *const ExternalCustomDataPlugin) };
    Ok(plugin.clone())
}

/// Extracts an `ExternalCustomDataHandle` from a PyCapsule.
///
/// # Errors
///
/// Returns a Python error if the capsule is invalid or holds the wrong type.
#[cfg(feature = "python")]
pub fn handle_from_capsule(capsule: &Bound<'_, PyAny>) -> PyResult<ExternalCustomDataHandle> {
    let capsule = capsule.cast::<PyCapsule>()?;
    let ptr = capsule.pointer_checked(None)?;
    // SAFETY: Capsule stores an `ExternalCustomDataHandle` value created by
    // `custom_data_handle_capsule`.
    let handle = unsafe { &*(ptr.as_ptr() as *const ExternalCustomDataHandle) };
    Ok(handle.clone())
}
