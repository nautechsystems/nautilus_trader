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

//! Host-side bridge between the plug-in C ABI and an engine.
//!
//! Provides actor and strategy adapters that wrap a cdylib's vtable + handle
//! as a `DataActor` / `Strategy` an engine can register, plus the
//! host-side [`HostVTable`](crate::HostVTable) that routes plug-in
//! callbacks through the engine's cache, risk, event, msgbus, and timer
//! paths. Used by both the live node and the backtest engine via the
//! crate's `host` feature.
//!
//! [plug-in roadmap]: https://github.com/nautechsystems/nautilus_trader/blob/develop/crates/plugin/README.md
//!
//! # Layout
//!
//! - [`actor`]: [`PluginActorAdapter`] for plug-in actors.
//! - [`strategy`]: [`PluginStrategyAdapter`] for plug-in strategies.
//! - [`host`]: host-side `HostVTable` construction with engine callback routing.
//! - [`registry`]: the per-instance opaque context the host attaches to each
//!   plug-in instance so host callbacks can be attributed to the calling
//!   adapter.
//! - [`configured`]: config-resolved adapter construction from a loaded
//!   plug-in manifest, used by engine startup code.
//!
//! Execution command structs and their boundary-owned handles live at
//! [`crate::surfaces::commands`] and are re-exported below for the
//! historical `nautilus_plugin::bridge::*` import path.

#![allow(unsafe_code)]

macro_rules! validated_slot {
    ($vtable_ty:ident, $vtable:expr, $slot:ident) => {{
        (*($vtable)).$slot.expect(concat!(
            "loader validates ",
            stringify!($vtable_ty),
            "::",
            stringify!($slot),
        ))
    }};
}

pub mod actor;
pub mod custom_data;
pub mod host;
pub mod registry;
pub mod strategy;

pub mod configured;

pub use actor::PluginActorAdapter;
pub use configured::{ConfiguredPluginEntry, configured_entry, register_manifest_custom_data};
pub use custom_data::{PluginCustomDataValue, register_custom_data_from_manifest};
pub use host::{host_vtable, plugin_loader};
pub use registry::HostContextInner;
pub use strategy::PluginStrategyAdapter;

// Re-exported for backwards compatibility with the historical
// `nautilus_plugin::bridge::*` import path. The command structs and their
// handles live under `crate::surfaces::commands`.
pub use crate::surfaces::commands::{
    CancelAllOrdersCommand, CancelAllOrdersHandle, CancelOrderCommand, CancelOrderHandle,
    CancelOrdersCommand, CancelOrdersHandle, CloseAllPositionsCommand, CloseAllPositionsHandle,
    ClosePositionCommand, ClosePositionHandle, ModifyOrderCommand, ModifyOrderHandle,
    QueryAccountCommand, QueryAccountHandle, QueryOrderCommand, QueryOrderHandle,
    SubmitOrderCommand, SubmitOrderHandle, SubmitOrderListCommand, SubmitOrderListHandle,
};
