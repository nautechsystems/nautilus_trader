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

//! Typed declarations for the experimental executable component contract.
//!
//! Domain values are borrowed synchronously under an exact-build identity.
//! The executable contract is separate from metadata ABI 1 and carries no
//! compatibility promise across builds.

use nautilus_core::Params;
use nautilus_model::{
    identifiers::{ClientId, PositionId},
    orders::OrderAny,
};

use crate::{BorrowedStr, OwnedBytes, PluginBuildId, PluginResult};

#[doc(hidden)]
pub const EXPERIMENTAL_COMPONENT_ABI_VERSION: u32 = 0;

/// Exact build identity required by the typed executable boundary.
#[doc(hidden)]
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct ComponentBuildId {
    pub metadata: PluginBuildId,
    pub package_fingerprint: BorrowedStr<'static>,
}

impl ComponentBuildId {
    #[must_use]
    pub const fn new(metadata: PluginBuildId, package_fingerprint: BorrowedStr<'static>) -> Self {
        Self {
            metadata,
            package_fingerprint,
        }
    }
}

#[doc(hidden)]
#[repr(u32)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ComponentRole {
    DataActor = 1,
    Strategy = 2,
    ExecutionAlgorithm = 3,
}

impl ComponentRole {
    #[must_use]
    pub const fn as_u32(self) -> u32 {
        self as u32
    }

    #[must_use]
    pub const fn from_u32(value: u32) -> Option<Self> {
        match value {
            1 => Some(Self::DataActor),
            2 => Some(Self::Strategy),
            3 => Some(Self::ExecutionAlgorithm),
            _ => None,
        }
    }
}

#[doc(hidden)]
#[repr(C)]
#[derive(Debug)]
pub struct ComponentHostContext {
    _opaque: [u8; 0],
}

/// Facade arguments borrowed by the host for one synchronous call.
#[doc(hidden)]
#[repr(C)]
#[derive(Clone, Debug)]
pub struct SubmitOrderCall {
    pub order: OrderAny,
    pub position_id: Option<PositionId>,
    pub client_id: Option<ClientId>,
    pub params: Option<Params>,
}

impl SubmitOrderCall {
    #[must_use]
    pub const fn new(
        order: OrderAny,
        position_id: Option<PositionId>,
        client_id: Option<ClientId>,
        params: Option<Params>,
    ) -> Self {
        Self {
            order,
            position_id,
            client_id,
            params,
        }
    }
}

#[doc(hidden)]
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct ComponentHostVTablePrefix {
    pub abi_version: u32,
    pub struct_size: usize,
    pub role: u32,
}

/// Synchronous host operations for an exact-build component.
///
/// The caller retains each call frame and its domain values until the slot
/// returns. The host borrows those values only for that call and creates its own
/// normalized values before retaining them. Returned buffers remain producer-owned
/// and must use their producer's release function.
///
/// Callers must establish matching build identity and valid callback context before
/// invoking a slot. Tables, context pointers, and their executable code must remain
/// live throughout the call. Each slot must contain its own unwinds.
#[doc(hidden)]
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct ComponentHostVTable {
    pub abi_version: u32,
    pub struct_size: usize,
    pub role: u32,
    pub build_id: ComponentBuildId,
    pub actor_id: Option<
        unsafe extern "C" fn(
            ctx: *const ComponentHostContext,
            generation: u64,
        ) -> PluginResult<OwnedBytes>,
    >,
    pub timestamp_ns: Option<
        unsafe extern "C" fn(
            ctx: *const ComponentHostContext,
            generation: u64,
        ) -> PluginResult<u64>,
    >,
    pub submit_order: Option<
        unsafe extern "C" fn(
            ctx: *const ComponentHostContext,
            generation: u64,
            call: *const SubmitOrderCall,
        ) -> PluginResult<()>,
    >,
}
