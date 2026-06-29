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

//! Plug-in artifact identity and boundary primitives for NautilusTrader.
//!
//! This crate provides the public contract that lets an independently compiled
//! Rust cdylib identify itself to a Nautilus host. It defines versioned build
//! metadata, allocator-safe boundary values, opaque host tokens, and the
//! `nautilus_plugin!` macro for exporting the standard entry symbol and
//! manifest.

#![warn(clippy::pedantic)]

/// ABI version of the public plug-in metadata contract.
///
/// The host refuses to load a plug-in whose
/// [`PluginManifest::abi_version`](crate::manifest::PluginManifest::abi_version)
/// does not match this value.
pub const NAUTILUS_PLUGIN_ABI_VERSION: u32 = 1;

/// Schema version for [`manifest::PluginBuildId`].
pub const PLUGIN_BUILD_ID_VERSION: u32 = 1;

/// Name of the single `extern "C"` entry symbol every plug-in cdylib exports.
pub const NAUTILUS_PLUGIN_INIT_SYMBOL: &[u8] = b"nautilus_plugin_init";

pub mod boundary;
pub mod host;
pub mod manifest;
pub mod panic;

mod macros;

pub use boundary::{BorrowedStr, OwnedBytes, PluginError, PluginErrorCode, PluginResult, Slice};
pub use host::{HostContext, HostVTable};
pub use manifest::{PluginBuildId, PluginInitFn, PluginManifest};

/// Re-exports that plug-in crates typically want in scope.
pub mod prelude {
    pub use crate::{
        BorrowedStr, HostContext, HostVTable, NAUTILUS_PLUGIN_ABI_VERSION, PluginBuildId,
        PluginError, PluginErrorCode, PluginManifest, PluginResult, Slice,
    };
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn init_symbol_matches_exported_entrypoint() {
        assert_eq!(NAUTILUS_PLUGIN_INIT_SYMBOL, b"nautilus_plugin_init");
    }
}
