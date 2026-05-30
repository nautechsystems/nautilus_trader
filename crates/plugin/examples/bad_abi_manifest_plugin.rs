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

//! Malformed plug-in fixture that advertises an unsupported ABI.

#![allow(unsafe_code)]
#![allow(
    clippy::missing_safety_doc,
    reason = "FFI entry symbol mirrors the macro-generated nautilus_plugin_init export"
)]

use nautilus_plugin::{
    BorrowedStr, HostVTable, NAUTILUS_PLUGIN_ABI_VERSION, PluginBuildId, PluginManifest, Slice,
};

static MANIFEST: PluginManifest = PluginManifest {
    abi_version: NAUTILUS_PLUGIN_ABI_VERSION + 1,
    plugin_name: BorrowedStr::from_str("bad-abi-plugin"),
    plugin_vendor: BorrowedStr::from_str("Nautech"),
    plugin_version: BorrowedStr::from_str(env!("CARGO_PKG_VERSION")),
    build_id: PluginBuildId::current(),
    custom_data: Slice::empty(),
    actors: Slice::empty(),
    strategies: Slice::empty(),
};

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nautilus_plugin_init(_host: *const HostVTable) -> *const PluginManifest {
    &raw const MANIFEST
}

#[allow(dead_code)]
fn main() {}
