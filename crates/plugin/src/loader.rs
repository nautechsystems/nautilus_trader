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

//! Host-side plug-in loader.
//!
//! Gated behind the `host` feature so plug-in cdylibs never pull in
//! `libloading`. Use [`PluginLoader`] from the live node startup to load every
//! configured plug-in path in order before any subscriptions are registered.

#![allow(unsafe_code)]

use std::{
    ffi::OsStr,
    fmt::Debug,
    mem::ManuallyDrop,
    path::{Path, PathBuf},
    sync::OnceLock,
};

use libloading::{Library, Symbol};

use crate::{
    NAUTILUS_PLUGIN_ABI_VERSION, NAUTILUS_PLUGIN_INIT_SYMBOL,
    boundary::{BorrowedStr, PluginError, PluginErrorCode, PluginResult},
    host::{HostContext, HostLogLevel, HostVTable},
    manifest::{PluginInitFn, PluginManifest},
};

/// Errors that can occur while loading a plug-in.
#[derive(Debug, thiserror::Error)]
pub enum LoadError {
    #[error("failed to open plug-in '{path}': {source}")]
    Open {
        path: PathBuf,
        #[source]
        source: libloading::Error,
    },

    #[error("plug-in '{path}' is missing the `nautilus_plugin_init` symbol: {source}")]
    MissingSymbol {
        path: PathBuf,
        #[source]
        source: libloading::Error,
    },

    #[error("plug-in '{path}' returned a null manifest from `nautilus_plugin_init`")]
    NullManifest { path: PathBuf },

    #[error("plug-in '{path}' ABI mismatch: host = {expected}, plug-in = {actual}")]
    AbiMismatch {
        path: PathBuf,
        expected: u32,
        actual: u32,
    },
}

/// One loaded plug-in. Holds the `Library` alive for the process lifetime so
/// the manifest pointer never dangles.
///
/// `library` is wrapped in [`ManuallyDrop`] so dropping the `LoadedPlugin`
/// (or the owning `PluginLoader`) does NOT `dlclose` the cdylib. v1 leaks
/// the handle intentionally: any manifest, vtable, or `drop_fn` pointer the
/// host has copied into its registries must outlive the loader. Unloading
/// would dangle every such pointer, and a later custom-data drop call would
/// jump into freed code.
pub struct LoadedPlugin {
    path: PathBuf,
    _library: ManuallyDrop<Library>,
    manifest: *const PluginManifest,
}

impl Debug for LoadedPlugin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(LoadedPlugin))
            .field("path", &self.path)
            .finish()
    }
}

/// SAFETY: `LoadedPlugin` only exposes the manifest through `&self`, and the
/// manifest is immutable static data inside the loaded library.
unsafe impl Send for LoadedPlugin {}
/// SAFETY: see above.
unsafe impl Sync for LoadedPlugin {}

impl LoadedPlugin {
    /// Returns the file path this plug-in was loaded from.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Returns the manifest the plug-in published at init time.
    #[must_use]
    pub fn manifest(&self) -> &PluginManifest {
        // SAFETY: pointer originates from `nautilus_plugin_init` and the
        // library is kept alive by `_library` for the lifetime of `self`.
        unsafe { &*self.manifest }
    }
}

/// Loader for plug-in cdylibs.
///
/// Owns every `Library` for the lifetime of the live node, since v1 does not
/// support `dlclose`. Caller walks the returned [`LoadedPlugin`] manifests to
/// register entries into the relevant runtime registries.
#[derive(Default)]
pub struct PluginLoader {
    loaded: Vec<LoadedPlugin>,
    host: Option<*const HostVTable>,
}

/// SAFETY: `*const HostVTable` is a process-lifetime static pointer; the host
/// commits to keeping the vtable live and the inner fn pointers are Sync by
/// construction.
unsafe impl Send for PluginLoader {}
/// SAFETY: see above.
unsafe impl Sync for PluginLoader {}

impl PluginLoader {
    /// Creates a new, empty loader that hands every plug-in the default
    /// `nautilus-plugin` host vtable. The default vtable carries
    /// `NotImplemented` stubs for the order command thunks; use
    /// [`PluginLoader::with_host`] from the live node to install a vtable
    /// whose order commands route through a strategy adapter.
    #[must_use]
    pub fn new() -> Self {
        Self {
            loaded: Vec::new(),
            host: None,
        }
    }

    /// Creates a new, empty loader that will hand every loaded plug-in the
    /// supplied `host` vtable instead of the default.
    ///
    /// `host` must remain live for the lifetime of every plug-in loaded
    /// through this loader (typically the process lifetime).
    #[must_use]
    pub fn with_host(host: *const HostVTable) -> Self {
        Self {
            loaded: Vec::new(),
            host: Some(host),
        }
    }

    /// Loads every plug-in path in order. Stops on the first error.
    pub fn load_all<P>(&mut self, paths: impl IntoIterator<Item = P>) -> Result<(), LoadError>
    where
        P: AsRef<OsStr>,
    {
        for p in paths {
            self.load(p.as_ref())?;
        }
        Ok(())
    }

    /// Loads a single plug-in cdylib.
    pub fn load(&mut self, path: impl AsRef<OsStr>) -> Result<&LoadedPlugin, LoadError> {
        let path_buf = PathBuf::from(path.as_ref());

        // SAFETY: `Library::new` is unsafe because loading runs arbitrary code
        // in the cdylib's static initializers. The caller of `PluginLoader`
        // commits to trusting the plug-in path before adding it to config.
        let library = unsafe { Library::new(path.as_ref()) }.map_err(|e| LoadError::Open {
            path: path_buf.clone(),
            source: e,
        })?;

        let manifest_ptr = {
            // SAFETY: looking up a known symbol name in an opened library.
            let init: Symbol<PluginInitFn> = unsafe { library.get(NAUTILUS_PLUGIN_INIT_SYMBOL) }
                .map_err(|e| LoadError::MissingSymbol {
                    path: path_buf.clone(),
                    source: e,
                })?;
            let host = self.host.unwrap_or_else(host_vtable);
            // SAFETY: calling the plug-in's published init symbol with our
            // host vtable. The plug-in promises to read the vtable and return
            // a valid `*const PluginManifest` or null.
            unsafe { init(host) }
        };

        validate_manifest_ptr(manifest_ptr, &path_buf)?;
        // SAFETY: validate_manifest_ptr returns Ok only when the pointer is
        // non-null and the ABI matches.
        let abi = unsafe { (*manifest_ptr).abi_version };

        // SAFETY: pointer is non-null and the library is kept alive below.
        let manifest_ref = unsafe { &*manifest_ptr };
        // SAFETY: slices borrow from `'static` storage in the cdylib.
        let custom_data_count = unsafe { manifest_ref.custom_data.as_slice() }.len();
        // SAFETY: see above.
        let actor_count = unsafe { manifest_ref.actors.as_slice() }.len();
        // SAFETY: see above.
        let strategy_count = unsafe { manifest_ref.strategies.as_slice() }.len();
        log::info!(
            target: "nautilus_plugin",
            "Loaded plug-in '{}' (abi={abi}, custom_data={custom_data_count}, actors={actor_count}, strategies={strategy_count}) from {}",
            // SAFETY: name string lives in the cdylib for the process lifetime.
            unsafe { manifest_ref.plugin_name.as_str() },
            path_buf.display(),
        );

        self.loaded.push(LoadedPlugin {
            path: path_buf,
            _library: ManuallyDrop::new(library),
            manifest: manifest_ptr,
        });
        Ok(self.loaded.last().expect("just pushed"))
    }

    /// Returns every loaded plug-in in load order.
    #[must_use]
    pub fn loaded(&self) -> &[LoadedPlugin] {
        &self.loaded
    }

    /// Returns the number of loaded plug-ins.
    #[must_use]
    pub fn len(&self) -> usize {
        self.loaded.len()
    }

    /// Returns whether no plug-ins have been loaded.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.loaded.is_empty()
    }
}

/// Validates a manifest pointer returned from `nautilus_plugin_init`.
///
/// Factored out so the `NullManifest` and `AbiMismatch` branches are
/// directly testable without spinning up a dedicated cdylib for each
/// failure mode.
fn validate_manifest_ptr(
    manifest_ptr: *const PluginManifest,
    path: &Path,
) -> Result<(), LoadError> {
    if manifest_ptr.is_null() {
        return Err(LoadError::NullManifest {
            path: path.to_path_buf(),
        });
    }
    // SAFETY: pointer is non-null per the check above.
    let abi = unsafe { (*manifest_ptr).abi_version };
    if abi != NAUTILUS_PLUGIN_ABI_VERSION {
        return Err(LoadError::AbiMismatch {
            path: path.to_path_buf(),
            expected: NAUTILUS_PLUGIN_ABI_VERSION,
            actual: abi,
        });
    }
    Ok(())
}

/// Returns the process-wide static `HostVTable` exposed to plug-ins.
///
/// One `&'static HostVTable` is enough because plug-ins never compare
/// vtables; they only call through the function pointers. Methods can be
/// added by bumping [`NAUTILUS_PLUGIN_ABI_VERSION`].
fn host_vtable() -> *const HostVTable {
    static HOST: OnceLock<HostVTable> = OnceLock::new();
    std::ptr::from_ref(HOST.get_or_init(|| HostVTable {
        abi_version: NAUTILUS_PLUGIN_ABI_VERSION,
        clock_now_ns: host_clock_now_ns,
        log: host_log,
        submit_order: host_submit_order_unbound,
        cancel_order: host_cancel_order_unbound,
        modify_order: host_modify_order_unbound,
    }))
}

unsafe extern "C" fn host_clock_now_ns() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};

    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_nanos()).unwrap_or(u64::MAX))
}

unsafe extern "C" fn host_submit_order_unbound(
    _ctx: *const HostContext,
    _command_json: BorrowedStr<'_>,
) -> PluginResult<()> {
    PluginResult::Err(PluginError::new(
        PluginErrorCode::NotImplemented,
        "submit_order is not wired into the live node yet",
    ))
}

unsafe extern "C" fn host_cancel_order_unbound(
    _ctx: *const HostContext,
    _command_json: BorrowedStr<'_>,
) -> PluginResult<()> {
    PluginResult::Err(PluginError::new(
        PluginErrorCode::NotImplemented,
        "cancel_order is not wired into the live node yet",
    ))
}

unsafe extern "C" fn host_modify_order_unbound(
    _ctx: *const HostContext,
    _command_json: BorrowedStr<'_>,
) -> PluginResult<()> {
    PluginResult::Err(PluginError::new(
        PluginErrorCode::NotImplemented,
        "modify_order is not wired into the live node yet",
    ))
}

unsafe extern "C" fn host_log(
    level: HostLogLevel,
    target: BorrowedStr<'_>,
    message: BorrowedStr<'_>,
) {
    // SAFETY: producer holds the storage live across the call.
    let target = unsafe { target.as_str() };
    // SAFETY: see above.
    let message = unsafe { message.as_str() };
    match level {
        HostLogLevel::Error => log::error!(target: "nautilus_plugin", "[{target}] {message}"),
        HostLogLevel::Warn => log::warn!(target: "nautilus_plugin", "[{target}] {message}"),
        HostLogLevel::Info => log::info!(target: "nautilus_plugin", "[{target}] {message}"),
        HostLogLevel::Debug => log::debug!(target: "nautilus_plugin", "[{target}] {message}"),
        HostLogLevel::Trace => log::trace!(target: "nautilus_plugin", "[{target}] {message}"),
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn empty_loader_is_empty() {
        let loader = PluginLoader::new();
        assert!(loader.is_empty());
        assert_eq!(loader.len(), 0);
        assert!(loader.loaded().is_empty());
    }

    #[rstest]
    fn missing_file_reports_open_error_with_path_and_source() {
        let mut loader = PluginLoader::new();
        let path = "/nonexistent/path/to/plugin.so";
        let err = loader.load(path).expect_err("should fail to open");
        match &err {
            LoadError::Open { path: p, source: _ } => {
                assert_eq!(p.as_os_str(), path);
            }
            other => panic!("expected Open, was {other:?}"),
        }
        let rendered = format!("{err}");
        assert!(
            rendered.contains(path),
            "rendered error should include the path, was: {rendered}",
        );
    }

    #[rstest]
    fn host_vtable_singleton_matches_abi() {
        let p = host_vtable();
        assert!(!p.is_null());
        // SAFETY: pointer is to a static `OnceLock`-backed HostVTable.
        let v = unsafe { &*p };
        assert_eq!(v.abi_version, NAUTILUS_PLUGIN_ABI_VERSION);
    }

    #[rstest]
    fn host_vtable_clock_now_ns_returns_unix_nanos() {
        let p = host_vtable();
        // SAFETY: pointer is to a static `OnceLock`-backed HostVTable.
        let v = unsafe { &*p };
        // SAFETY: the fn pointer is non-null and pointing at host_clock_now_ns
        // which uses SystemTime::now without dereferencing any input.
        let now = unsafe { (v.clock_now_ns)() };
        // Sanity bound: any time after 2020-01-01 in UNIX nanoseconds.
        assert!(now > 1_577_836_800_000_000_000u64);
    }

    #[rstest]
    fn host_vtable_log_does_not_panic() {
        let p = host_vtable();
        // SAFETY: see above.
        let v = unsafe { &*p };
        let target = BorrowedStr::from_str("nautilus_plugin_test");
        let message = BorrowedStr::from_str("test message");
        // SAFETY: target and message outlive the call; the host_log fn
        // only forwards to the `log` crate macros.
        unsafe { (v.log)(HostLogLevel::Info, target, message) };
    }

    #[rstest]
    fn validate_manifest_ptr_rejects_null() {
        let path = std::path::Path::new("/test/plugin.so");
        let err = validate_manifest_ptr(std::ptr::null(), path).unwrap_err();
        match err {
            LoadError::NullManifest { path: p } => assert_eq!(p, path),
            other => panic!("expected NullManifest, was {other:?}"),
        }
    }

    #[rstest]
    fn validate_manifest_ptr_rejects_abi_mismatch() {
        use crate::boundary::{BorrowedStr, Slice};
        let bad_manifest = PluginManifest {
            abi_version: NAUTILUS_PLUGIN_ABI_VERSION.wrapping_add(1),
            plugin_name: BorrowedStr::from_str("bad"),
            plugin_vendor: BorrowedStr::from_str(""),
            plugin_version: BorrowedStr::from_str("0.0.0"),
            custom_data: Slice::empty(),
            actors: Slice::empty(),
            strategies: Slice::empty(),
        };
        let path = std::path::Path::new("/test/plugin.so");
        let err = validate_manifest_ptr(&raw const bad_manifest, path).unwrap_err();
        match err {
            LoadError::AbiMismatch {
                path: p,
                expected,
                actual,
            } => {
                assert_eq!(p, path);
                assert_eq!(expected, NAUTILUS_PLUGIN_ABI_VERSION);
                assert_eq!(actual, NAUTILUS_PLUGIN_ABI_VERSION.wrapping_add(1));
            }
            other => panic!("expected AbiMismatch, was {other:?}"),
        }
    }

    #[rstest]
    fn validate_manifest_ptr_accepts_matching_manifest() {
        use crate::boundary::{BorrowedStr, Slice};
        let good_manifest = PluginManifest {
            abi_version: NAUTILUS_PLUGIN_ABI_VERSION,
            plugin_name: BorrowedStr::from_str("good"),
            plugin_vendor: BorrowedStr::from_str(""),
            plugin_version: BorrowedStr::from_str("0.0.0"),
            custom_data: Slice::empty(),
            actors: Slice::empty(),
            strategies: Slice::empty(),
        };
        let path = std::path::Path::new("/test/plugin.so");
        validate_manifest_ptr(&raw const good_manifest, path).expect("matching manifest accepted");
    }

    #[rstest]
    #[case::submit("submit_order is not wired into the live node yet")]
    #[case::cancel("cancel_order is not wired into the live node yet")]
    #[case::modify("modify_order is not wired into the live node yet")]
    fn host_order_command_stubs_return_not_implemented(#[case] expected: &str) {
        // The shipped loader's host vtable installs NotImplemented stubs
        // for every order command until the live-node wiring lands.
        // Regression test that locks the stub error code AND the message
        // so a future wiring commit must explicitly replace each one.
        let p = host_vtable();
        // SAFETY: pointer is to a static `OnceLock`-backed HostVTable.
        let v = unsafe { &*p };
        let ctx = std::ptr::null::<HostContext>();
        let payload = BorrowedStr::from_str("{}");

        let r = match expected {
            s if s.starts_with("submit_order") =>
            // SAFETY: stub does not deref ctx; payload outlives the call.
            unsafe { (v.submit_order)(ctx, payload) },
            s if s.starts_with("cancel_order") =>
            // SAFETY: see above.
            unsafe { (v.cancel_order)(ctx, payload) },
            s if s.starts_with("modify_order") =>
            // SAFETY: see above.
            unsafe { (v.modify_order)(ctx, payload) },
            _ => unreachable!(),
        };

        let err = r.into_result().unwrap_err();
        assert_eq!(err.code, PluginErrorCode::NotImplemented);
        assert_eq!(err.message_string(), expected);
    }
}
