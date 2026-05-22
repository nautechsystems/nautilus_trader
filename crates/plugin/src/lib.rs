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

//! Plug-in API and ABI surface for [NautilusTrader](https://nautilustrader.io).
//!
//! This crate defines the C-ABI boundary between a Nautilus host (the live node) and
//! independently compiled Rust plug-in cdylibs. Plug-ins ship a single
//! `nautilus_plugin_init` symbol and a `'static` [`PluginManifest`]; the host
//! `dlopen`s the library, calls the entry point, and registers every plug point
//! the manifest enumerates.
//!
//! # Layout
//!
//! Infrastructure modules describe how plug-ins work at the boundary:
//!
//! - [`boundary`]: primitive `#[repr(C)]` types used at the boundary
//!   ([`BorrowedStr`], [`Slice`], [`PluginError`], etc.).
//! - [`manifest`]: the static manifest a plug-in returns and the per-plug-point
//!   registration entries it contains.
//! - [`host`]: the `HostVTable` of function pointers the host gives to the
//!   plug-in for re-entrant callbacks (msgbus, clock, logging, etc.).
//! - [`mod@panic`]: a `catch_unwind` wrapper that every macro-generated thunk
//!   uses to stop a plug-in panic from unwinding across the FFI boundary.
//!
//! Per-plug-point trait surfaces live under [`surfaces`]:
//!
//! - [`surfaces::custom_data`]: custom data type plug-point.
//! - [`surfaces::actor`]: plug-in actor (`DataActor`-shaped) plug-point.
//! - [`surfaces::strategy`]: plug-in strategy (`Strategy`-shaped) plug-point.
//!
//! Host-side loading lives behind the `host` feature and uses `libloading`.

/// ABI version of the plug-in contract.
///
/// The host refuses to load a plug-in whose [`PluginManifest::abi_version`]
/// does not match this value. The plug-in surface is unreleased and unstable,
/// so this stays pinned at `1` during early hardening and does not promise
/// compatibility between Nautilus versions. Once the surface is released, every
/// breaking change to a `#[repr(C)]` struct or vtable must bump it.
///
/// [`PluginManifest::abi_version`]: crate::manifest::PluginManifest::abi_version
pub const NAUTILUS_PLUGIN_ABI_VERSION: u32 = 1;

/// Schema version for [`manifest::PluginBuildId`].
pub const PLUGIN_BUILD_ID_VERSION: u32 = 1;

/// Name of the single `extern "C"` entry symbol every plug-in cdylib must export.
///
/// The host looks up this symbol via `libloading::Symbol` after `dlopen`.
pub const NAUTILUS_PLUGIN_INIT_SYMBOL: &[u8] = b"nautilus_plugin_init";

pub mod boundary;
pub mod host;
pub mod manifest;
pub mod panic;
pub mod surfaces;

#[cfg(feature = "host")]
pub mod loader;

mod macros;

pub use boundary::{BorrowedStr, OwnedBytes, PluginError, PluginErrorCode, PluginResult, Slice};
pub use host::{HostContext, HostVTable};
pub use manifest::{
    ActorRegistration, CustomDataRegistration, PluginBuildId, PluginInitFn, PluginManifest,
    StrategyRegistration,
};
#[cfg(feature = "host")]
pub use manifest::{
    ValidatedActorRegistration, ValidatedActorVTable, ValidatedCustomDataRegistration,
    ValidatedCustomDataVTable, ValidatedPluginManifest, ValidatedStrategyRegistration,
    ValidatedStrategyVTable,
};
pub use surfaces::{actor::PluginActor, custom_data::PluginCustomData, strategy::PluginStrategy};

/// Re-exports that plug-in authors typically want in scope.
pub mod prelude {
    pub use crate::{
        BorrowedStr, HostContext, HostVTable, NAUTILUS_PLUGIN_ABI_VERSION, PluginActor,
        PluginBuildId, PluginCustomData, PluginError, PluginErrorCode, PluginManifest,
        PluginResult, PluginStrategy, Slice,
    };
}
