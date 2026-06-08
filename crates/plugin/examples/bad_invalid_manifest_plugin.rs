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

//! Malformed plug-in fixture with invalid manifest fields.

#![allow(unsafe_code)]
#![allow(
    clippy::missing_safety_doc,
    reason = "FFI entry symbol mirrors the macro-generated nautilus_plugin_init export"
)]

use std::{marker::PhantomData, sync::LazyLock};

use nautilus_plugin::{
    BorrowedStr, CustomDataRegistration, HostVTable, NAUTILUS_PLUGIN_ABI_VERSION, PluginBuildId,
    PluginManifest, Slice, StrategyRegistration,
};

static INVALID_UTF8: [u8; 1] = [0xff];

static CUSTOM_DATA: [CustomDataRegistration; 1] = [CustomDataRegistration {
    type_name: BorrowedStr::from_str("BadTick"),
    vtable: std::ptr::null(),
}];

static STRATEGIES: LazyLock<[StrategyRegistration; 1]> = LazyLock::new(|| {
    [StrategyRegistration {
        // SAFETY: the byte points at process-lifetime storage, but the fixture
        // intentionally violates the UTF-8 contract for loader validation.
        type_name: unsafe { borrowed_str_from_raw(INVALID_UTF8.as_ptr(), INVALID_UTF8.len()) },
        vtable: std::ptr::null(),
    }]
});

static MANIFEST: LazyLock<PluginManifest> = LazyLock::new(|| PluginManifest {
    abi_version: NAUTILUS_PLUGIN_ABI_VERSION,
    plugin_name: BorrowedStr::empty(),
    plugin_vendor: BorrowedStr::from_str("Nautech"),
    // SAFETY: this fixture intentionally violates the non-null contract for
    // non-empty strings so the loader can prove it rejects the manifest.
    plugin_version: unsafe { borrowed_str_from_raw(std::ptr::null(), 1) },
    build_id: PluginBuildId::current(),
    custom_data: Slice::from_slice(&CUSTOM_DATA),
    // SAFETY: this fixture intentionally publishes a malformed slice so the
    // loader can prove it rejects the manifest before registration.
    actors: unsafe { slice_from_raw(std::ptr::null(), 1) },
    strategies: Slice::from_slice(&*STRATEGIES),
    controllers: Slice::empty(),
});

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nautilus_plugin_init(_host: *const HostVTable) -> *const PluginManifest {
    &raw const *MANIFEST
}

#[repr(C)]
struct BorrowedStrRaw {
    ptr: *const u8,
    len: usize,
    _phantom: PhantomData<&'static [u8]>,
}

#[repr(C)]
struct SliceRaw<T: 'static> {
    ptr: *const T,
    len: usize,
    _phantom: PhantomData<&'static [T]>,
}

unsafe fn borrowed_str_from_raw(ptr: *const u8, len: usize) -> BorrowedStr<'static> {
    let raw = BorrowedStrRaw {
        ptr,
        len,
        _phantom: PhantomData,
    };
    // SAFETY: this fixture mirrors `BorrowedStr`'s `#[repr(C)]` layout to
    // construct intentionally malformed boundary values for loader tests.
    unsafe { std::mem::transmute::<BorrowedStrRaw, BorrowedStr<'static>>(raw) }
}

unsafe fn slice_from_raw<T: 'static>(ptr: *const T, len: usize) -> Slice<'static, T> {
    let raw = SliceRaw {
        ptr,
        len,
        _phantom: PhantomData,
    };
    // SAFETY: this fixture mirrors `Slice`'s `#[repr(C)]` layout to construct
    // intentionally malformed boundary values for loader tests.
    unsafe { std::mem::transmute::<SliceRaw<T>, Slice<'static, T>>(raw) }
}

#[allow(dead_code)]
fn main() {}
