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

//! Static manifest a plug-in returns from `nautilus_plugin_init`.
//!
//! The manifest enumerates every plug-point contribution the cdylib provides
//! and points at the per-type vtables. v1 ships with one plug-point family
//! ([`CustomDataRegistration`]); subsequent plug points add new `Slice`
//! fields to [`PluginManifest`] without removing existing ones (additive
//! ABI bumps only).

use crate::{
    NAUTILUS_PLUGIN_ABI_VERSION,
    boundary::{BorrowedStr, Slice},
    host::HostVTable,
    surfaces::{actor::ActorVTable, custom_data::CustomDataVTable, strategy::StrategyVTable},
};

/// Signature of the single `extern "C"` entry symbol every plug-in exports
/// under the name [`crate::NAUTILUS_PLUGIN_INIT_SYMBOL`].
///
/// The host calls this once at load time with a pointer to its `HostVTable`.
/// The plug-in returns a pointer to its `'static` [`PluginManifest`], or null
/// to signal load failure (in which case the host should fall back to
/// `nautilus_plugin_last_error` if defined; v1 only treats null as a load
/// error and logs the path).
pub type PluginInitFn = unsafe extern "C" fn(host: *const HostVTable) -> *const PluginManifest;

/// The static, process-lifetime manifest a plug-in returns from
/// `nautilus_plugin_init`.
///
/// Every `Slice` here borrows from `'static` storage in the plug-in's
/// cdylib. The host treats the entire manifest as immutable.
#[repr(C)]
pub struct PluginManifest {
    /// ABI version. Must equal [`NAUTILUS_PLUGIN_ABI_VERSION`] or the host
    /// refuses to load the plug-in.
    pub abi_version: u32,

    /// Short machine-readable plug-in name (e.g. `"my-momentum"`).
    pub plugin_name: BorrowedStr<'static>,

    /// Free-form vendor or author string.
    pub plugin_vendor: BorrowedStr<'static>,

    /// Plug-in version (typically the crate's `CARGO_PKG_VERSION`).
    pub plugin_version: BorrowedStr<'static>,

    /// Custom-data registrations contributed by this plug-in.
    pub custom_data: Slice<'static, CustomDataRegistration>,

    /// Actor registrations contributed by this plug-in.
    pub actors: Slice<'static, ActorRegistration>,

    /// Strategy registrations contributed by this plug-in.
    pub strategies: Slice<'static, StrategyRegistration>,
    // Future plug-point slices land here in additive ABI bumps:
    //   pub indicators: Slice<'static, IndicatorRegistration>,
    //   pub fill_models: Slice<'static, FillModelRegistration>,
    //   ...
}

impl PluginManifest {
    /// Returns whether this manifest is compatible with the compiled-in ABI.
    #[must_use]
    pub fn matches_compiled_abi(&self) -> bool {
        self.abi_version == NAUTILUS_PLUGIN_ABI_VERSION
    }
}

/// Registration entry for one custom-data type.
#[repr(C)]
pub struct CustomDataRegistration {
    /// Canonical type name; must match the `type_name` returned by the vtable.
    pub type_name: BorrowedStr<'static>,
    /// Pointer to the static vtable for this type.
    pub vtable: *const CustomDataVTable,
}

/// SAFETY: the pointer is `'static` and immutable for the process lifetime.
unsafe impl Send for CustomDataRegistration {}
/// SAFETY: see above.
unsafe impl Sync for CustomDataRegistration {}

/// Registration entry for one plug-in actor type.
#[repr(C)]
pub struct ActorRegistration {
    /// Canonical type name; must match the `type_name` returned by the vtable.
    pub type_name: BorrowedStr<'static>,
    /// Pointer to the static vtable for this actor type.
    pub vtable: *const ActorVTable,
}

/// SAFETY: the pointer is `'static` and immutable for the process lifetime.
unsafe impl Send for ActorRegistration {}
/// SAFETY: see above.
unsafe impl Sync for ActorRegistration {}

/// Registration entry for one plug-in strategy type.
#[repr(C)]
pub struct StrategyRegistration {
    /// Canonical type name; must match the `type_name` returned by the vtable.
    pub type_name: BorrowedStr<'static>,
    /// Pointer to the static vtable for this strategy type.
    pub vtable: *const StrategyVTable,
}

/// SAFETY: the pointer is `'static` and immutable for the process lifetime.
unsafe impl Send for StrategyRegistration {}
/// SAFETY: see above.
unsafe impl Sync for StrategyRegistration {}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn empty_manifest_matches_compiled_abi() {
        let m = PluginManifest {
            abi_version: NAUTILUS_PLUGIN_ABI_VERSION,
            plugin_name: BorrowedStr::from_str("test"),
            plugin_vendor: BorrowedStr::from_str("nautech"),
            plugin_version: BorrowedStr::from_str("0.0.0"),
            custom_data: Slice::empty(),
            actors: Slice::empty(),
            strategies: Slice::empty(),
        };
        assert!(m.matches_compiled_abi());
    }

    #[rstest]
    #[case::off_by_one(NAUTILUS_PLUGIN_ABI_VERSION.wrapping_add(1))]
    #[case::zero(0)]
    #[case::max(u32::MAX)]
    fn mismatched_manifest_rejects(#[case] abi: u32) {
        let m = PluginManifest {
            abi_version: abi,
            plugin_name: BorrowedStr::from_str("test"),
            plugin_vendor: BorrowedStr::from_str("nautech"),
            plugin_version: BorrowedStr::from_str("0.0.0"),
            custom_data: Slice::empty(),
            actors: Slice::empty(),
            strategies: Slice::empty(),
        };
        assert!(!m.matches_compiled_abi());
    }
}
