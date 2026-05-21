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

//! Declarative macros for the plug-in author-facing surface.
//!
//! The top-level [`nautilus_plugin!`](crate::nautilus_plugin) macro emits the
//! `extern "C" nautilus_plugin_init` symbol, the static
//! [`PluginManifest`](crate::manifest::PluginManifest), and the per-plug-point
//! registration arrays for every type listed.

/// Defines a plug-in's static manifest and emits the `nautilus_plugin_init`
/// entry symbol.
///
/// Use this exactly once per plug-in cdylib, at module scope (typically in
/// `lib.rs`).
///
/// # Required fields
///
/// - `name`: short machine-readable plug-in name.
/// - `version`: plug-in version string (usually `env!("CARGO_PKG_VERSION")`).
///
/// # Optional fields
///
/// - `vendor`: free-form vendor/author string (default `""`).
/// - `custom_data`: array of types implementing
///   [`PluginCustomData`](crate::surfaces::custom_data::PluginCustomData).
/// - `actors`: array of types implementing
///   [`PluginActor`](crate::surfaces::actor::PluginActor).
/// - `strategies`: array of types implementing
///   [`PluginStrategy`](crate::surfaces::strategy::PluginStrategy).
///
/// # Example
///
/// ```ignore
/// use nautilus_plugin::prelude::*;
///
/// pub struct MyTick { ts_event: u64, ts_init: u64, value: f64 }
///
/// impl PluginCustomData for MyTick {
///     const TYPE_NAME: &'static str = "MyTick";
///     fn ts_event(&self) -> u64 { self.ts_event }
///     fn ts_init(&self) -> u64 { self.ts_init }
///     // ... other methods
/// }
///
/// nautilus_plugin::nautilus_plugin! {
///     name: "my-plugin",
///     version: env!("CARGO_PKG_VERSION"),
///     custom_data: [MyTick],
/// }
/// ```
#[macro_export]
macro_rules! nautilus_plugin {
    (
        $(name: $name:expr,)?
        $(vendor: $vendor:expr,)?
        $(version: $version:expr,)?
        $(custom_data: [$($cd:ty),* $(,)?] ,)?
        $(actors: [$($act:ty),* $(,)?] ,)?
        $(strategies: [$($strategy:ty),* $(,)?] ,)?
    ) => {
        $crate::__nautilus_plugin_impl! {
            @parse
            name = ($($name)?),
            vendor = ($($vendor)?),
            version = ($($version)?),
            custom_data = ($($($cd),*)?),
            actors = ($($($act),*)?),
            strategies = ($($($strategy),*)?),
        }
    };
}

/// Internal expansion of [`nautilus_plugin!`]. Not part of the public API.
#[doc(hidden)]
#[macro_export]
macro_rules! __nautilus_plugin_impl {
    (
        @parse
        name = ($name:expr),
        vendor = ($($vendor:expr)?),
        version = ($version:expr),
        custom_data = ($($cd:ty),*),
        actors = ($($act:ty),*),
        strategies = ($($strategy:ty),*),
    ) => {
        const _: () = {
            // Compile-time guard: every listed type implements the trait. The
            // bound checks happen at the call sites below; this block keeps
            // the trait import scoped to the macro expansion.

            #[allow(unused_imports)]
            use $crate::surfaces::custom_data::PluginCustomData as _PluginCustomData;
            #[allow(unused_imports)]
            use $crate::surfaces::actor::PluginActor as _PluginActor;
            #[allow(unused_imports)]
            use $crate::surfaces::strategy::PluginStrategy as _PluginStrategy;

            static CUSTOM_DATA: ::std::sync::LazyLock<
                [$crate::manifest::CustomDataRegistration; $crate::__nautilus_plugin_impl!(@count $($cd),*)]
            > = ::std::sync::LazyLock::new(|| {
                [
                    $(
                        $crate::manifest::CustomDataRegistration {
                            type_name: $crate::boundary::BorrowedStr::from_str(
                                <$cd as $crate::surfaces::custom_data::PluginCustomData>::TYPE_NAME,
                            ),
                            vtable: $crate::surfaces::custom_data::custom_data_vtable::<$cd>(),
                        },
                    )*
                ]
            });

            static ACTORS: ::std::sync::LazyLock<
                [$crate::manifest::ActorRegistration; $crate::__nautilus_plugin_impl!(@count $($act),*)]
            > = ::std::sync::LazyLock::new(|| {
                [
                    $(
                        $crate::manifest::ActorRegistration {
                            type_name: $crate::boundary::BorrowedStr::from_str(
                                <$act as $crate::surfaces::actor::PluginActor>::TYPE_NAME,
                            ),
                            vtable: $crate::surfaces::actor::actor_vtable::<$act>(),
                        },
                    )*
                ]
            });

            static STRATEGIES: ::std::sync::LazyLock<
                [$crate::manifest::StrategyRegistration; $crate::__nautilus_plugin_impl!(@count $($strategy),*)]
            > = ::std::sync::LazyLock::new(|| {
                [
                    $(
                        $crate::manifest::StrategyRegistration {
                            type_name: $crate::boundary::BorrowedStr::from_str(
                                <$strategy as $crate::surfaces::strategy::PluginStrategy>::TYPE_NAME,
                            ),
                            vtable: $crate::surfaces::strategy::strategy_vtable::<$strategy>(),
                        },
                    )*
                ]
            });

            static MANIFEST: ::std::sync::LazyLock<$crate::manifest::PluginManifest> =
                ::std::sync::LazyLock::new(|| $crate::manifest::PluginManifest {
                    abi_version: $crate::NAUTILUS_PLUGIN_ABI_VERSION,
                    plugin_name: $crate::boundary::BorrowedStr::from_str($name),
                    plugin_vendor: $crate::boundary::BorrowedStr::from_str(
                        $crate::__nautilus_plugin_impl!(@opt $($vendor)?),
                    ),
                    plugin_version: $crate::boundary::BorrowedStr::from_str($version),
                    build_id: $crate::manifest::PluginBuildId::current(),
                    custom_data: $crate::boundary::Slice::from_slice(&*CUSTOM_DATA),
                    actors: $crate::boundary::Slice::from_slice(&*ACTORS),
                    strategies: $crate::boundary::Slice::from_slice(&*STRATEGIES),
                });

            #[unsafe(no_mangle)]
            pub unsafe extern "C" fn nautilus_plugin_init(
                host: *const $crate::host::HostVTable,
            ) -> *const $crate::manifest::PluginManifest {
                let result = ::std::panic::catch_unwind(|| {
                    if host.is_null() {
                        return ::core::ptr::null::<$crate::manifest::PluginManifest>();
                    }
                    // SAFETY: host pointer is non-null and the host commits
                    // to keeping the vtable live for the process lifetime.
                    let host_ref = unsafe { &*host };
                    if host_ref.abi_version != $crate::NAUTILUS_PLUGIN_ABI_VERSION {
                        return ::core::ptr::null();
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

    // Empty-string default when the optional `vendor` field is omitted
    (@opt) => { "" };
    (@opt $vendor:expr) => { $vendor };

    // Counts the listed types so the registration array has a fixed size
    (@count) => { 0usize };
    (@count $head:ty $(, $tail:ty)*) => {
        1usize + $crate::__nautilus_plugin_impl!(@count $($tail),*)
    };
}
