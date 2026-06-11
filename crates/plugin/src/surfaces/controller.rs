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

//! Controller plug point.
//!
//! Controllers can prepare runtime strategy definitions and issue lifecycle
//! commands through a controller-specific host service table. The surface uses
//! JSON request and response envelopes for controller-host calls because those
//! commands are orchestration actions rather than per-event market data paths.

#![allow(unsafe_code)]

use std::marker::PhantomData;

use nautilus_common::timer::TimeEvent;

use crate::{
    boundary::{BorrowedStr, OwnedBytes, PluginError, PluginErrorCode, PluginResult},
    host::{ControllerHostContext, ControllerHostVTable},
    normalize::BoundaryNormalize,
    panic::{guard, guard_drop, guard_or_null},
};

/// Opaque handle to a plug-in controller instance owned by the cdylib.
#[repr(C)]
pub struct PluginControllerHandle {
    _opaque: [u8; 0],
}

/// Function table for a single plug-in controller type.
///
/// Slots are nullable at the ABI type level so the host can reject malformed
/// manifests with null callbacks before constructing a controller. Generated
/// vtables fill every required slot.
#[repr(C)]
pub struct ControllerVTable {
    /// Prepares a controller request before the host registers runtime state.
    pub prepare:
        Option<unsafe extern "C" fn(request_json: BorrowedStr<'_>) -> PluginResult<OwnedBytes>>,

    /// Constructs a fresh controller instance bound to the supplied host
    /// vtable and instance context.
    pub create: Option<
        unsafe extern "C" fn(
            host: *const ControllerHostVTable,
            ctx: *const ControllerHostContext,
            config_json: BorrowedStr<'_>,
        ) -> *mut PluginControllerHandle,
    >,

    /// Drops the controller instance and releases all of its resources.
    pub drop_handle: Option<unsafe extern "C" fn(handle: *mut PluginControllerHandle)>,

    /// Returns the canonical type name for this controller.
    pub type_name: Option<unsafe extern "C" fn() -> BorrowedStr<'static>>,

    pub on_start:
        Option<unsafe extern "C" fn(handle: *mut PluginControllerHandle) -> PluginResult<()>>,
    pub on_stop:
        Option<unsafe extern "C" fn(handle: *mut PluginControllerHandle) -> PluginResult<()>>,
    pub on_resume:
        Option<unsafe extern "C" fn(handle: *mut PluginControllerHandle) -> PluginResult<()>>,
    pub on_reset:
        Option<unsafe extern "C" fn(handle: *mut PluginControllerHandle) -> PluginResult<()>>,
    pub on_dispose:
        Option<unsafe extern "C" fn(handle: *mut PluginControllerHandle) -> PluginResult<()>>,
    pub on_degrade:
        Option<unsafe extern "C" fn(handle: *mut PluginControllerHandle) -> PluginResult<()>>,
    pub on_fault:
        Option<unsafe extern "C" fn(handle: *mut PluginControllerHandle) -> PluginResult<()>>,

    pub on_time_event: Option<
        unsafe extern "C" fn(
            handle: *mut PluginControllerHandle,
            event: *const TimeEvent,
        ) -> PluginResult<()>,
    >,
}

/// Author-facing trait for a plug-in controller.
///
/// Controllers can define a static [`PluginController::prepare`] hook and
/// runtime lifecycle callbacks. Every callback has a no-op default. Override
/// only what you need.
pub trait PluginController: 'static + Send + Sized {
    /// Canonical type name. Must be unique across a Nautilus deployment.
    const TYPE_NAME: &'static str;

    /// Prepares a JSON controller request and returns a JSON response envelope.
    #[allow(unused_variables)]
    fn prepare(request_json: &str) -> anyhow::Result<Vec<u8>> {
        Ok(Vec::new())
    }

    /// Constructs a fresh controller instance bound to the supplied host
    /// vtable and instance context.
    fn new(
        host: *const ControllerHostVTable,
        ctx: *const ControllerHostContext,
        config_json: &str,
    ) -> Self;

    #[allow(unused_variables)]
    fn on_start(&mut self) -> anyhow::Result<()> {
        Ok(())
    }

    #[allow(unused_variables)]
    fn on_stop(&mut self) -> anyhow::Result<()> {
        Ok(())
    }

    #[allow(unused_variables)]
    fn on_resume(&mut self) -> anyhow::Result<()> {
        Ok(())
    }

    #[allow(unused_variables)]
    fn on_reset(&mut self) -> anyhow::Result<()> {
        Ok(())
    }

    #[allow(unused_variables)]
    fn on_dispose(&mut self) -> anyhow::Result<()> {
        Ok(())
    }

    #[allow(unused_variables)]
    fn on_degrade(&mut self) -> anyhow::Result<()> {
        Ok(())
    }

    #[allow(unused_variables)]
    fn on_fault(&mut self) -> anyhow::Result<()> {
        Ok(())
    }

    #[allow(unused_variables)]
    fn on_time_event(&mut self, event: &TimeEvent) -> anyhow::Result<()> {
        Ok(())
    }
}

/// Returns a `*const ControllerVTable` for the given [`PluginController`] type.
#[must_use]
pub fn controller_vtable<T>() -> *const ControllerVTable
where
    T: PluginController,
{
    &VTableTag::<T>::VTABLE
}

struct VTableTag<T>(PhantomData<T>);

impl<T> VTableTag<T>
where
    T: PluginController,
{
    const VTABLE: ControllerVTable = ControllerVTable {
        prepare: Some(prepare_thunk::<T>),
        create: Some(create_thunk::<T>),
        drop_handle: Some(drop_handle_thunk::<T>),
        type_name: Some(type_name_thunk::<T>),
        on_start: Some(on_start_thunk::<T>),
        on_stop: Some(on_stop_thunk::<T>),
        on_resume: Some(on_resume_thunk::<T>),
        on_reset: Some(on_reset_thunk::<T>),
        on_dispose: Some(on_dispose_thunk::<T>),
        on_degrade: Some(on_degrade_thunk::<T>),
        on_fault: Some(on_fault_thunk::<T>),
        on_time_event: Some(on_time_event_thunk::<T>),
    };
}

unsafe extern "C" fn prepare_thunk<T: PluginController>(
    request_json: BorrowedStr<'_>,
) -> PluginResult<OwnedBytes> {
    guard(|| {
        // SAFETY: host promises `request_json` borrows storage that is live
        // for the duration of this call.
        let request = unsafe { request_json.as_str() };
        T::prepare(request)
            .map(OwnedBytes::from_vec)
            .map_err(|e| PluginError::new(PluginErrorCode::Generic, e.to_string()))
    })
}

unsafe extern "C" fn create_thunk<T: PluginController>(
    host: *const ControllerHostVTable,
    ctx: *const ControllerHostContext,
    config_json: BorrowedStr<'_>,
) -> *mut PluginControllerHandle {
    guard_or_null("controller::create", || {
        // SAFETY: host promises `config_json` borrows storage that is live
        // for the duration of this call.
        let cfg = unsafe { config_json.as_str() };
        Box::into_raw(Box::new(T::new(host, ctx, cfg))).cast::<PluginControllerHandle>()
    })
}

unsafe extern "C" fn drop_handle_thunk<T: PluginController>(handle: *mut PluginControllerHandle) {
    if handle.is_null() {
        return;
    }
    guard_drop("controller::drop", || {
        // SAFETY: handle was allocated via `Box::into_raw(Box::new(T))`.
        unsafe {
            drop(Box::from_raw(handle.cast::<T>()));
        }
    });
}

unsafe extern "C" fn type_name_thunk<T: PluginController>() -> BorrowedStr<'static> {
    BorrowedStr::from_str(T::TYPE_NAME)
}

fn handle_as_mut<'a, T: PluginController>(handle: *mut PluginControllerHandle) -> &'a mut T {
    // SAFETY: handle is non-null and originates from a `Box::into_raw` of a
    // `T`. The host promises exclusive access while a callback is running.
    unsafe { &mut *handle.cast::<T>() }
}

fn ok_or_err<E: ::core::fmt::Display>(r: Result<(), E>) -> Result<(), PluginError> {
    r.map_err(|e| PluginError::new(PluginErrorCode::Generic, e.to_string()))
}

macro_rules! lifecycle_thunk {
    ($name:ident, $method:ident) => {
        unsafe extern "C" fn $name<T: PluginController>(
            handle: *mut PluginControllerHandle,
        ) -> PluginResult<()> {
            guard(|| {
                let controller = handle_as_mut::<T>(handle);
                ok_or_err(controller.$method())
            })
        }
    };
}

lifecycle_thunk!(on_start_thunk, on_start);
lifecycle_thunk!(on_stop_thunk, on_stop);
lifecycle_thunk!(on_resume_thunk, on_resume);
lifecycle_thunk!(on_reset_thunk, on_reset);
lifecycle_thunk!(on_dispose_thunk, on_dispose);
lifecycle_thunk!(on_degrade_thunk, on_degrade);
lifecycle_thunk!(on_fault_thunk, on_fault);

unsafe extern "C" fn on_time_event_thunk<T: PluginController>(
    handle: *mut PluginControllerHandle,
    event: *const TimeEvent,
) -> PluginResult<()> {
    guard(|| {
        // SAFETY: host keeps `event` live for the duration of the call; the
        // plug-in only borrows it for the trait-method invocation.
        let event = unsafe { &*event }.boundary_normalized();
        let controller = handle_as_mut::<T>(handle);
        ok_or_err(controller.on_time_event(&event))
    })
}
