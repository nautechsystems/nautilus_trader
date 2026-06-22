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

//! Declarative macros for plug-in metadata export.

/// Defines a plug-in's static metadata manifest and emits the
/// `nautilus_plugin_init` entry symbol.
///
/// Use this exactly once per plug-in cdylib, at module scope.
///
/// # Required fields
///
/// - `name`: short machine-readable plug-in name.
/// - `version`: plug-in version string (usually `env!("CARGO_PKG_VERSION")`).
///
/// # Optional fields
///
/// - `vendor`: free-form vendor/author string (default `""`).
#[macro_export]
macro_rules! nautilus_plugin {
    (
        $(name: $name:expr,)?
        $(vendor: $vendor:expr,)?
        $(version: $version:expr,)?
    ) => {
        $crate::__nautilus_plugin_impl! {
            @parse
            name = ($($name)?),
            vendor = ($($vendor)?),
            version = ($($version)?),
        }
    };
}

/// Internal expansion of [`nautilus_plugin!`]. Not part of the public API.
#[doc(hidden)]
#[macro_export]
macro_rules! __nautilus_plugin_impl {
    (
        @parse
        name = (),
        $($rest:tt)*
    ) => {
        ::core::compile_error!("`nautilus_plugin!` requires a `name` field");
    };
    (
        @parse
        name = ($name:expr),
        vendor = ($($vendor:expr)?),
        version = (),
    ) => {
        ::core::compile_error!("`nautilus_plugin!` requires a `version` field");
    };
    (
        @parse
        name = ($name:expr),
        vendor = ($($vendor:expr)?),
        version = ($version:expr),
    ) => {
        const _: () = {
            static MANIFEST: ::std::sync::LazyLock<$crate::manifest::PluginManifest> =
                ::std::sync::LazyLock::new(|| $crate::manifest::PluginManifest {
                    abi_version: $crate::NAUTILUS_PLUGIN_ABI_VERSION,
                    plugin_name: $crate::boundary::BorrowedStr::from_str($name),
                    plugin_vendor: $crate::boundary::BorrowedStr::from_str(
                        $crate::__nautilus_plugin_impl!(@opt $($vendor)?),
                    ),
                    plugin_version: $crate::boundary::BorrowedStr::from_str($version),
                    build_id: $crate::manifest::PluginBuildId::current(),
                });

            #[unsafe(no_mangle)]
            pub unsafe extern "C" fn nautilus_plugin_init(
                host: *const $crate::host::HostVTable,
            ) -> *const $crate::manifest::PluginManifest {
                let result = ::std::panic::catch_unwind(|| {
                    if host.is_null() {
                        return ::core::ptr::null::<$crate::manifest::PluginManifest>();
                    }
                    &*MANIFEST as *const _
                });

                match result {
                    Ok(ptr) => ptr,
                    Err(payload) => {
                        $crate::panic::drop_payload(payload);
                        ::core::ptr::null()
                    }
                }
            }
        };
    };

    (@opt) => { "" };
    (@opt $vendor:expr) => { $vendor };
}
