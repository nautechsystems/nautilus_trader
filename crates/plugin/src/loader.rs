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
    fmt::{Debug, Display},
    mem::ManuallyDrop,
    path::{Path, PathBuf},
    slice,
    sync::OnceLock,
};

use libloading::{Library, Symbol};

use crate::{
    NAUTILUS_PLUGIN_ABI_VERSION, NAUTILUS_PLUGIN_INIT_SYMBOL,
    boundary::{BorrowedStr, PluginError, PluginErrorCode, PluginResult},
    host::{HostContext, HostLogLevel, HostVTable},
    manifest::{
        PluginBuildId, PluginInitFn, PluginManifest, PluginManifestValidationErrors,
        ValidatedPluginManifest,
    },
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

    #[error("plug-in '{path}' ABI mismatch: host = {expected}, plug-in = {actual}; {diagnostics}")]
    AbiMismatch {
        path: PathBuf,
        expected: u32,
        actual: u32,
        diagnostics: Box<PluginManifestDiagnostics>,
    },

    #[error("plug-in '{path}' manifest validation failed: {diagnostics}; {errors}")]
    InvalidManifest {
        path: PathBuf,
        diagnostics: Box<PluginManifestDiagnostics>,
        #[source]
        errors: PluginManifestValidationErrors,
    },
}

/// Owned manifest diagnostics captured before a rejected plug-in is unloaded.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PluginManifestDiagnostics {
    /// Manifest plug-in name, or empty when the manifest published none.
    pub plugin_name: String,
    /// Manifest plug-in version, or empty when the manifest published none.
    pub plugin_version: String,
    /// Manifest build identifier captured as owned strings.
    pub build_id: PluginBuildIdDiagnostics,
}

impl PluginManifestDiagnostics {
    fn from_manifest(manifest: &PluginManifest) -> Self {
        Self {
            plugin_name: borrowed_str_diagnostic(manifest.plugin_name),
            plugin_version: borrowed_str_diagnostic(manifest.plugin_version),
            build_id: PluginBuildIdDiagnostics::from_build_id(&manifest.build_id),
        }
    }
}

impl Display for PluginManifestDiagnostics {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let plugin_name = unknown_if_empty(&self.plugin_name);
        let plugin_version = unknown_if_empty(&self.plugin_version);
        let build_id = &self.build_id;
        write!(
            f,
            "manifest name='{plugin_name}', version='{plugin_version}', {build_id}"
        )
    }
}

/// Owned build identifier diagnostics for loader errors and logs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PluginBuildIdDiagnostics {
    /// Build identifier schema version published by the manifest.
    pub schema_version: u32,
    /// `nautilus-plugin` crate version, or empty when unavailable.
    pub nautilus_plugin_version: String,
    /// Rust compiler version, or empty when unavailable.
    pub rustc_version: String,
    /// Cargo target triple, or empty when unavailable.
    pub target_triple: String,
    /// Cargo build profile, or empty when unavailable.
    pub build_profile: String,
}

impl PluginBuildIdDiagnostics {
    fn from_build_id(build_id: &PluginBuildId) -> Self {
        Self {
            schema_version: build_id.schema_version,
            nautilus_plugin_version: borrowed_str_diagnostic(build_id.nautilus_plugin_version),
            rustc_version: borrowed_str_diagnostic(build_id.rustc_version),
            target_triple: borrowed_str_diagnostic(build_id.target_triple),
            build_profile: borrowed_str_diagnostic(build_id.build_profile),
        }
    }
}

impl Display for PluginBuildIdDiagnostics {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let schema_version = self.schema_version;
        let nautilus_plugin_version = unknown_if_empty(&self.nautilus_plugin_version);
        let rustc_version = unknown_if_empty(&self.rustc_version);
        let target_triple = unknown_if_empty(&self.target_triple);
        let build_profile = unknown_if_empty(&self.build_profile);
        write!(
            f,
            "build_id(schema={schema_version}, nautilus_plugin_version='{nautilus_plugin_version}', rustc='{rustc_version}', target='{target_triple}', profile='{build_profile}')"
        )
    }
}

fn unknown_if_empty(value: &str) -> &str {
    if value.is_empty() { "<unknown>" } else { value }
}

fn borrowed_str_diagnostic(value: BorrowedStr<'_>) -> String {
    if value.ptr.is_null() || value.len == 0 {
        return String::new();
    }

    // SAFETY: manifest strings live in static cdylib storage while the
    // library is loaded.
    let bytes = unsafe { slice::from_raw_parts(value.ptr, value.len) };
    String::from_utf8_lossy(bytes).into_owned()
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
    manifest: ValidatedPluginManifest<'static>,
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
        self.manifest.manifest()
    }

    /// Returns a host-side manifest view that carries validation invariants.
    #[must_use]
    pub fn validated_manifest(&self) -> ValidatedPluginManifest<'static> {
        self.manifest
    }
}

/// Loader for plug-in cdylibs.
///
/// Owns every `Library` for the lifetime of the live node, since v1 does not
/// support `dlclose`. Caller walks the returned [`LoadedPlugin`] manifests to
/// register entries into the relevant runtime registries.
#[derive(Debug, Default)]
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
    /// `NotImplemented` stubs for stateful host callbacks; use
    /// [`PluginLoader::with_host`] to install a live-node vtable.
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

        let manifest = validate_manifest_ptr(manifest_ptr, &path_buf)?;
        let manifest_ref = manifest.manifest();
        let abi = manifest_ref.abi_version;
        let custom_data_count = manifest.custom_data().len();
        let actor_count = manifest.actors().len();
        let strategy_count = manifest.strategies().len();
        let build_id = PluginBuildIdDiagnostics::from_build_id(&manifest_ref.build_id);
        log::info!(
            target: "nautilus_plugin",
            "Loaded plug-in '{}' (abi={abi}, {build_id}, custom_data={custom_data_count}, actors={actor_count}, strategies={strategy_count}) from {}",
            manifest.plugin_name(),
            path_buf.display(),
        );

        self.loaded.push(LoadedPlugin {
            path: path_buf,
            _library: ManuallyDrop::new(library),
            manifest,
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
) -> Result<ValidatedPluginManifest<'static>, LoadError> {
    if manifest_ptr.is_null() {
        return Err(LoadError::NullManifest {
            path: path.to_path_buf(),
        });
    }
    // SAFETY: pointer is non-null per the check above.
    let manifest = unsafe { &*manifest_ptr };
    let abi = manifest.abi_version;
    if abi != NAUTILUS_PLUGIN_ABI_VERSION {
        return Err(LoadError::AbiMismatch {
            path: path.to_path_buf(),
            expected: NAUTILUS_PLUGIN_ABI_VERSION,
            actual: abi,
            diagnostics: Box::new(PluginManifestDiagnostics::from_manifest(manifest)),
        });
    }

    match ValidatedPluginManifest::new(manifest) {
        Ok(manifest) => Ok(manifest),
        Err(errors) => Err(LoadError::InvalidManifest {
            path: path.to_path_buf(),
            diagnostics: Box::new(PluginManifestDiagnostics::from_manifest(manifest)),
            errors,
        }),
    }
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
        cache_instrument: host_cache_instrument_unbound,
        cache_account: host_cache_account_unbound,
        cache_order: host_cache_order_unbound,
        cache_position: host_cache_position_unbound,
        cache_orders_for_strategy: host_cache_orders_for_strategy_unbound,
        cache_positions_for_strategy: host_cache_positions_for_strategy_unbound,
        subscribe_quotes: host_subscribe_quotes_unbound,
        unsubscribe_quotes: host_unsubscribe_quotes_unbound,
        subscribe_trades: host_subscribe_trades_unbound,
        unsubscribe_trades: host_unsubscribe_trades_unbound,
        subscribe_bars: host_subscribe_bars_unbound,
        unsubscribe_bars: host_unsubscribe_bars_unbound,
        subscribe_book_deltas: host_subscribe_book_deltas_unbound,
        unsubscribe_book_deltas: host_unsubscribe_book_deltas_unbound,
        subscribe_book_at_interval: host_subscribe_book_at_interval_unbound,
        unsubscribe_book_at_interval: host_unsubscribe_book_at_interval_unbound,
        msgbus_publish: host_msgbus_publish_unbound,
        set_time_alert: host_set_time_alert_unbound,
        set_timer: host_set_timer_unbound,
        cancel_timer: host_cancel_timer_unbound,
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

macro_rules! unbound_bytes_fn {
    ($name:ident, $message:literal, ($($arg:ident : $ty:ty),* $(,)?)) => {
        unsafe extern "C" fn $name($($arg: $ty),*) -> PluginResult<crate::OwnedBytes> {
            $(let _ = $arg;)*
            PluginResult::Err(PluginError::new(PluginErrorCode::NotImplemented, $message))
        }
    };
}

macro_rules! unbound_unit_fn {
    ($name:ident, $message:literal, ($($arg:ident : $ty:ty),* $(,)?)) => {
        unsafe extern "C" fn $name($($arg: $ty),*) -> PluginResult<()> {
            $(let _ = $arg;)*
            PluginResult::Err(PluginError::new(PluginErrorCode::NotImplemented, $message))
        }
    };
}

unbound_bytes_fn!(
    host_cache_instrument_unbound,
    "cache_instrument is not wired into this host vtable",
    (ctx: *const HostContext, instrument_id: BorrowedStr<'_>)
);
unbound_bytes_fn!(
    host_cache_account_unbound,
    "cache_account is not wired into this host vtable",
    (ctx: *const HostContext, account_id: BorrowedStr<'_>)
);
unbound_bytes_fn!(
    host_cache_order_unbound,
    "cache_order is not wired into this host vtable",
    (ctx: *const HostContext, client_order_id: BorrowedStr<'_>)
);
unbound_bytes_fn!(
    host_cache_position_unbound,
    "cache_position is not wired into this host vtable",
    (ctx: *const HostContext, position_id: BorrowedStr<'_>)
);
unbound_bytes_fn!(
    host_cache_orders_for_strategy_unbound,
    "cache_orders_for_strategy is not wired into this host vtable",
    (ctx: *const HostContext, strategy_id: BorrowedStr<'_>)
);
unbound_bytes_fn!(
    host_cache_positions_for_strategy_unbound,
    "cache_positions_for_strategy is not wired into this host vtable",
    (ctx: *const HostContext, strategy_id: BorrowedStr<'_>)
);

unbound_unit_fn!(
    host_subscribe_quotes_unbound,
    "subscribe_quotes is not wired into this host vtable",
    (
        ctx: *const HostContext,
        instrument_id: BorrowedStr<'_>,
        client_id: BorrowedStr<'_>,
        params_json: BorrowedStr<'_>,
    )
);
unbound_unit_fn!(
    host_unsubscribe_quotes_unbound,
    "unsubscribe_quotes is not wired into this host vtable",
    (
        ctx: *const HostContext,
        instrument_id: BorrowedStr<'_>,
        client_id: BorrowedStr<'_>,
        params_json: BorrowedStr<'_>,
    )
);
unbound_unit_fn!(
    host_subscribe_trades_unbound,
    "subscribe_trades is not wired into this host vtable",
    (
        ctx: *const HostContext,
        instrument_id: BorrowedStr<'_>,
        client_id: BorrowedStr<'_>,
        params_json: BorrowedStr<'_>,
    )
);
unbound_unit_fn!(
    host_unsubscribe_trades_unbound,
    "unsubscribe_trades is not wired into this host vtable",
    (
        ctx: *const HostContext,
        instrument_id: BorrowedStr<'_>,
        client_id: BorrowedStr<'_>,
        params_json: BorrowedStr<'_>,
    )
);
unbound_unit_fn!(
    host_subscribe_bars_unbound,
    "subscribe_bars is not wired into this host vtable",
    (
        ctx: *const HostContext,
        bar_type: BorrowedStr<'_>,
        client_id: BorrowedStr<'_>,
        params_json: BorrowedStr<'_>,
    )
);
unbound_unit_fn!(
    host_unsubscribe_bars_unbound,
    "unsubscribe_bars is not wired into this host vtable",
    (
        ctx: *const HostContext,
        bar_type: BorrowedStr<'_>,
        client_id: BorrowedStr<'_>,
        params_json: BorrowedStr<'_>,
    )
);
unbound_unit_fn!(
    host_subscribe_book_deltas_unbound,
    "subscribe_book_deltas is not wired into this host vtable",
    (
        ctx: *const HostContext,
        instrument_id: BorrowedStr<'_>,
        book_type: u8,
        depth: usize,
        client_id: BorrowedStr<'_>,
        managed: u8,
        params_json: BorrowedStr<'_>,
    )
);
unbound_unit_fn!(
    host_unsubscribe_book_deltas_unbound,
    "unsubscribe_book_deltas is not wired into this host vtable",
    (
        ctx: *const HostContext,
        instrument_id: BorrowedStr<'_>,
        client_id: BorrowedStr<'_>,
        params_json: BorrowedStr<'_>,
    )
);
unbound_unit_fn!(
    host_subscribe_book_at_interval_unbound,
    "subscribe_book_at_interval is not wired into this host vtable",
    (
        ctx: *const HostContext,
        instrument_id: BorrowedStr<'_>,
        book_type: u8,
        depth: usize,
        interval_ms: usize,
        client_id: BorrowedStr<'_>,
        params_json: BorrowedStr<'_>,
    )
);
unbound_unit_fn!(
    host_unsubscribe_book_at_interval_unbound,
    "unsubscribe_book_at_interval is not wired into this host vtable",
    (
        ctx: *const HostContext,
        instrument_id: BorrowedStr<'_>,
        interval_ms: usize,
        client_id: BorrowedStr<'_>,
        params_json: BorrowedStr<'_>,
    )
);
unbound_unit_fn!(
    host_msgbus_publish_unbound,
    "msgbus_publish is not wired into this host vtable",
    (
        ctx: *const HostContext,
        topic: BorrowedStr<'_>,
        payload: crate::Slice<'_, u8>,
    )
);
unbound_unit_fn!(
    host_set_time_alert_unbound,
    "set_time_alert is not wired into this host vtable",
    (
        ctx: *const HostContext,
        name: BorrowedStr<'_>,
        alert_time_ns: u64,
        allow_past: u8,
    )
);
unbound_unit_fn!(
    host_set_timer_unbound,
    "set_timer is not wired into this host vtable",
    (
        ctx: *const HostContext,
        name: BorrowedStr<'_>,
        interval_ns: u64,
        start_time_ns: u64,
        stop_time_ns: u64,
        allow_past: u8,
        fire_immediately: u8,
    )
);
unbound_unit_fn!(
    host_cancel_timer_unbound,
    "cancel_timer is not wired into this host vtable",
    (ctx: *const HostContext, name: BorrowedStr<'_>)
);

unsafe extern "C" fn host_submit_order_unbound(
    _ctx: *const HostContext,
    _command_json: BorrowedStr<'_>,
) -> PluginResult<()> {
    PluginResult::Err(PluginError::new(
        PluginErrorCode::NotImplemented,
        "submit_order is not wired into this host vtable",
    ))
}

unsafe extern "C" fn host_cancel_order_unbound(
    _ctx: *const HostContext,
    _command_json: BorrowedStr<'_>,
) -> PluginResult<()> {
    PluginResult::Err(PluginError::new(
        PluginErrorCode::NotImplemented,
        "cancel_order is not wired into this host vtable",
    ))
}

unsafe extern "C" fn host_modify_order_unbound(
    _ctx: *const HostContext,
    _command_json: BorrowedStr<'_>,
) -> PluginResult<()> {
    PluginResult::Err(PluginError::new(
        PluginErrorCode::NotImplemented,
        "modify_order is not wired into this host vtable",
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
    use crate::{
        boundary::Slice,
        manifest::{CustomDataRegistration, PluginBuildId},
        surfaces::custom_data::{CustomDataVTable, PluginCustomData, custom_data_vtable},
    };

    #[derive(Clone, PartialEq)]
    struct LoaderTestTick;

    impl PluginCustomData for LoaderTestTick {
        const TYPE_NAME: &'static str = "LoaderTestTick";

        fn ts_event(&self) -> u64 {
            0
        }

        fn ts_init(&self) -> u64 {
            0
        }

        fn to_json(&self) -> anyhow::Result<Vec<u8>> {
            Ok(Vec::new())
        }

        fn from_json(_payload: &[u8]) -> anyhow::Result<Self> {
            Ok(Self)
        }

        fn schema_ipc() -> anyhow::Result<Vec<u8>> {
            Ok(Vec::new())
        }

        fn encode_batch(_items: &[&Self]) -> anyhow::Result<Vec<u8>> {
            Ok(Vec::new())
        }

        fn decode_batch(
            _ipc_bytes: &[u8],
            _metadata: &[(String, String)],
        ) -> anyhow::Result<Vec<Self>> {
            Ok(Vec::new())
        }
    }

    fn custom_data_vtable_missing_to_json() -> *const CustomDataVTable {
        let valid = custom_data_vtable::<LoaderTestTick>();
        // SAFETY: generated test vtable lives for the process lifetime.
        let valid = unsafe { &*valid };
        let vtable = Box::leak(Box::new(CustomDataVTable {
            type_name: valid.type_name,
            schema_ipc: valid.schema_ipc,
            from_json: valid.from_json,
            encode_batch: valid.encode_batch,
            decode_batch: valid.decode_batch,
            ts_event: valid.ts_event,
            ts_init: valid.ts_init,
            to_json: None,
            clone_handle: valid.clone_handle,
            drop_handle: valid.drop_handle,
            eq_handles: valid.eq_handles,
        }));
        std::ptr::from_ref(&*vtable)
    }

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
        let bad_manifest = PluginManifest {
            abi_version: NAUTILUS_PLUGIN_ABI_VERSION.wrapping_add(1),
            plugin_name: BorrowedStr::from_str("bad"),
            plugin_vendor: BorrowedStr::from_str(""),
            plugin_version: BorrowedStr::from_str("0.0.0"),
            build_id: PluginBuildId::current(),
            custom_data: Slice::empty(),
            actors: Slice::empty(),
            strategies: Slice::empty(),
        };
        let path = std::path::Path::new("/test/plugin.so");
        let err = validate_manifest_ptr(&raw const bad_manifest, path).unwrap_err();
        match &err {
            LoadError::AbiMismatch {
                path: p,
                expected,
                actual,
                diagnostics,
            } => {
                assert_eq!(p, path);
                assert_eq!(*expected, NAUTILUS_PLUGIN_ABI_VERSION);
                assert_eq!(*actual, NAUTILUS_PLUGIN_ABI_VERSION.wrapping_add(1));
                assert_eq!(diagnostics.plugin_name.as_str(), "bad");
                assert_eq!(diagnostics.plugin_version.as_str(), "0.0.0");
                assert_eq!(
                    diagnostics.build_id.nautilus_plugin_version.as_str(),
                    env!("CARGO_PKG_VERSION")
                );
            }
            other => panic!("expected AbiMismatch, was {other:?}"),
        }

        let rendered = format!("{err}");
        assert!(rendered.contains("manifest name='bad'"));
        assert!(rendered.contains("nautilus_plugin_version='"));
        assert!(rendered.contains("rustc='"));
        assert!(rendered.contains("target='"));
        assert!(rendered.contains("profile='"));
    }

    #[rstest]
    fn abi_mismatch_diagnostics_mark_unavailable_build_id_fields() {
        let bad_manifest = PluginManifest {
            abi_version: NAUTILUS_PLUGIN_ABI_VERSION.wrapping_add(1),
            plugin_name: BorrowedStr::empty(),
            plugin_vendor: BorrowedStr::empty(),
            plugin_version: BorrowedStr::empty(),
            build_id: PluginBuildId {
                schema_version: 7,
                nautilus_plugin_version: BorrowedStr::empty(),
                rustc_version: BorrowedStr::empty(),
                target_triple: BorrowedStr::empty(),
                build_profile: BorrowedStr::empty(),
            },
            custom_data: Slice::empty(),
            actors: Slice::empty(),
            strategies: Slice::empty(),
        };
        let path = std::path::Path::new("/test/plugin.so");
        let err = validate_manifest_ptr(&raw const bad_manifest, path).unwrap_err();
        let rendered = format!("{err}");

        assert!(rendered.contains("plug-in '/test/plugin.so' ABI mismatch"));
        assert!(rendered.contains("host = 1"));
        assert!(rendered.contains("plug-in = 2"));
        assert!(rendered.contains("manifest name='<unknown>'"));
        assert!(rendered.contains("version='<unknown>'"));
        assert!(rendered.contains("build_id(schema=7"));
        assert!(rendered.contains("nautilus_plugin_version='<unknown>'"));
        assert!(rendered.contains("rustc='<unknown>'"));
        assert!(rendered.contains("target='<unknown>'"));
        assert!(rendered.contains("profile='<unknown>'"));
    }

    #[rstest]
    fn validate_manifest_ptr_accepts_matching_manifest() {
        let registrations = Box::leak(Box::new([CustomDataRegistration {
            type_name: BorrowedStr::from_str("LoaderTestTick"),
            vtable: custom_data_vtable::<LoaderTestTick>(),
        }]));
        let good_manifest = PluginManifest {
            abi_version: NAUTILUS_PLUGIN_ABI_VERSION,
            plugin_name: BorrowedStr::from_str("good"),
            plugin_vendor: BorrowedStr::from_str(""),
            plugin_version: BorrowedStr::from_str("0.0.0"),
            build_id: PluginBuildId::current(),
            custom_data: Slice::from_slice(registrations),
            actors: Slice::empty(),
            strategies: Slice::empty(),
        };
        let path = std::path::Path::new("/test/plugin.so");
        let manifest = validate_manifest_ptr(&raw const good_manifest, path)
            .expect("matching manifest accepted");
        let custom_data = manifest.custom_data().next().expect("custom data entry");

        assert_eq!(manifest.plugin_name(), "good");
        assert_eq!(custom_data.type_name(), "LoaderTestTick");
        assert_eq!(custom_data.vtable().as_ptr(), registrations[0].vtable);
    }

    #[rstest]
    fn validate_manifest_ptr_rejects_invalid_manifest_with_diagnostics() {
        static NULL_VTABLE_CUSTOM_DATA: [CustomDataRegistration; 1] = [CustomDataRegistration {
            type_name: BorrowedStr::from_str("BadTick"),
            vtable: std::ptr::null(),
        }];

        let bad_manifest = PluginManifest {
            abi_version: NAUTILUS_PLUGIN_ABI_VERSION,
            plugin_name: BorrowedStr::empty(),
            plugin_vendor: BorrowedStr::from_str(""),
            plugin_version: BorrowedStr::from_str("0.0.0"),
            build_id: PluginBuildId {
                schema_version: crate::PLUGIN_BUILD_ID_VERSION + 1,
                ..PluginBuildId::current()
            },
            custom_data: Slice::from_slice(&NULL_VTABLE_CUSTOM_DATA),
            actors: Slice::empty(),
            strategies: Slice::empty(),
        };
        let path = std::path::Path::new("/test/plugin.so");
        let err = validate_manifest_ptr(&raw const bad_manifest, path).unwrap_err();

        match &err {
            LoadError::InvalidManifest {
                path: p,
                diagnostics,
                errors,
            } => {
                assert_eq!(p, path);
                assert_eq!(diagnostics.plugin_name.as_str(), "");
                assert_eq!(diagnostics.plugin_version.as_str(), "0.0.0");
                assert!(
                    errors
                        .messages()
                        .iter()
                        .any(|message| message == "plugin_name must not be empty")
                );
                assert!(
                    errors
                        .messages()
                        .iter()
                        .any(|message| message == "custom_data[0].vtable must not be null")
                );
            }
            other => panic!("expected InvalidManifest, was {other:?}"),
        }

        let rendered = format!("{err}");
        assert!(rendered.contains("plug-in '/test/plugin.so' manifest validation failed"));
        assert!(rendered.contains("manifest name='<unknown>'"));
        assert!(rendered.contains("plugin_name must not be empty"));
        let expected_schema_error = format!(
            "build_id.schema_version {} does not match supported schema {}",
            crate::PLUGIN_BUILD_ID_VERSION + 1,
            crate::PLUGIN_BUILD_ID_VERSION
        );
        assert!(rendered.contains(&expected_schema_error));
        assert!(rendered.contains("custom_data[0].vtable must not be null"));
    }

    #[rstest]
    fn validate_manifest_ptr_rejects_malformed_vtable_with_diagnostics() {
        let registrations = Box::leak(Box::new([CustomDataRegistration {
            type_name: BorrowedStr::from_str("BadTick"),
            vtable: custom_data_vtable_missing_to_json(),
        }]));
        let bad_manifest = PluginManifest {
            abi_version: NAUTILUS_PLUGIN_ABI_VERSION,
            plugin_name: BorrowedStr::from_str("bad-vtable"),
            plugin_vendor: BorrowedStr::from_str(""),
            plugin_version: BorrowedStr::from_str("0.0.0"),
            build_id: PluginBuildId::current(),
            custom_data: Slice::from_slice(registrations),
            actors: Slice::empty(),
            strategies: Slice::empty(),
        };
        let path = std::path::Path::new("/test/plugin.so");
        let err = validate_manifest_ptr(&raw const bad_manifest, path).unwrap_err();

        match &err {
            LoadError::InvalidManifest {
                path: p,
                diagnostics,
                errors,
            } => {
                assert_eq!(p, path);
                assert_eq!(diagnostics.plugin_name.as_str(), "bad-vtable");
                assert!(errors.messages().iter().any(|message| message
                    == "custom_data[0] type 'BadTick' vtable.to_json must not be null"));
            }
            other => panic!("expected InvalidManifest, was {other:?}"),
        }

        let rendered = format!("{err}");
        assert!(rendered.contains("manifest name='bad-vtable'"));
        assert!(rendered.contains("custom_data[0] type 'BadTick' vtable.to_json must not be null"));
    }

    #[rstest]
    #[case::submit("submit_order is not wired into this host vtable")]
    #[case::cancel("cancel_order is not wired into this host vtable")]
    #[case::modify("modify_order is not wired into this host vtable")]
    fn host_order_command_stubs_return_not_implemented(#[case] expected: &str) {
        // The default loader's host vtable installs NotImplemented stubs for
        // callbacks that need live-node state.
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

    #[rstest]
    #[case::instrument("cache_instrument")]
    #[case::account("cache_account")]
    #[case::order("cache_order")]
    #[case::position("cache_position")]
    #[case::orders_for_strategy("cache_orders_for_strategy")]
    #[case::positions_for_strategy("cache_positions_for_strategy")]
    fn host_cache_stubs_return_not_implemented(#[case] method: &str) {
        let p = host_vtable();
        // SAFETY: pointer is to a static `OnceLock`-backed HostVTable.
        let v = unsafe { &*p };
        let ctx = std::ptr::null::<HostContext>();
        let value = BorrowedStr::from_str("VALUE");

        let r = match method {
            // SAFETY: stubs do not dereference ctx or borrowed values.
            "cache_instrument" => unsafe { (v.cache_instrument)(ctx, value) },
            // SAFETY: see above.
            "cache_account" => unsafe { (v.cache_account)(ctx, value) },
            // SAFETY: see above.
            "cache_order" => unsafe { (v.cache_order)(ctx, value) },
            // SAFETY: see above.
            "cache_position" => unsafe { (v.cache_position)(ctx, value) },
            // SAFETY: see above.
            "cache_orders_for_strategy" => unsafe { (v.cache_orders_for_strategy)(ctx, value) },
            // SAFETY: see above.
            "cache_positions_for_strategy" => unsafe {
                (v.cache_positions_for_strategy)(ctx, value)
            },
            _ => unreachable!(),
        };

        let err = match r.into_result() {
            Ok(_) => panic!("{method} unexpectedly succeeded"),
            Err(e) => e,
        };
        assert_eq!(err.code, PluginErrorCode::NotImplemented);
        assert_eq!(
            err.message_string(),
            format!("{method} is not wired into this host vtable")
        );
    }

    #[rstest]
    #[case::subscribe_quotes("subscribe_quotes")]
    #[case::unsubscribe_quotes("unsubscribe_quotes")]
    #[case::subscribe_trades("subscribe_trades")]
    #[case::unsubscribe_trades("unsubscribe_trades")]
    #[case::subscribe_bars("subscribe_bars")]
    #[case::unsubscribe_bars("unsubscribe_bars")]
    #[case::subscribe_book_deltas("subscribe_book_deltas")]
    #[case::unsubscribe_book_deltas("unsubscribe_book_deltas")]
    #[case::subscribe_book_at_interval("subscribe_book_at_interval")]
    #[case::unsubscribe_book_at_interval("unsubscribe_book_at_interval")]
    #[case::msgbus_publish("msgbus_publish")]
    #[case::set_time_alert("set_time_alert")]
    #[case::set_timer("set_timer")]
    #[case::cancel_timer("cancel_timer")]
    fn host_stateful_unit_stubs_return_not_implemented(#[case] method: &str) {
        let p = host_vtable();
        // SAFETY: pointer is to a static `OnceLock`-backed HostVTable.
        let v = unsafe { &*p };
        let ctx = std::ptr::null::<HostContext>();
        let value = BorrowedStr::from_str("VALUE");
        let empty = BorrowedStr::empty();

        let r = match method {
            // SAFETY: stubs do not dereference ctx or borrowed values.
            "subscribe_quotes" => unsafe { (v.subscribe_quotes)(ctx, value, empty, empty) },
            // SAFETY: see above.
            "unsubscribe_quotes" => unsafe { (v.unsubscribe_quotes)(ctx, value, empty, empty) },
            // SAFETY: see above.
            "subscribe_trades" => unsafe { (v.subscribe_trades)(ctx, value, empty, empty) },
            // SAFETY: see above.
            "unsubscribe_trades" => unsafe { (v.unsubscribe_trades)(ctx, value, empty, empty) },
            // SAFETY: see above.
            "subscribe_bars" => unsafe { (v.subscribe_bars)(ctx, value, empty, empty) },
            // SAFETY: see above.
            "unsubscribe_bars" => unsafe { (v.unsubscribe_bars)(ctx, value, empty, empty) },
            // SAFETY: see above.
            "subscribe_book_deltas" => unsafe {
                (v.subscribe_book_deltas)(ctx, value, 0, 0, empty, 0, empty)
            },
            // SAFETY: see above.
            "unsubscribe_book_deltas" => unsafe {
                (v.unsubscribe_book_deltas)(ctx, value, empty, empty)
            },
            // SAFETY: see above.
            "subscribe_book_at_interval" => unsafe {
                (v.subscribe_book_at_interval)(ctx, value, 0, 0, 1, empty, empty)
            },
            // SAFETY: see above.
            "unsubscribe_book_at_interval" => unsafe {
                (v.unsubscribe_book_at_interval)(ctx, value, 1, empty, empty)
            },
            // SAFETY: see above.
            "msgbus_publish" => unsafe { (v.msgbus_publish)(ctx, value, crate::Slice::empty()) },
            // SAFETY: see above.
            "set_time_alert" => unsafe { (v.set_time_alert)(ctx, value, 1, 0) },
            // SAFETY: see above.
            "set_timer" => unsafe { (v.set_timer)(ctx, value, 1, 0, 0, 0, 0) },
            // SAFETY: see above.
            "cancel_timer" => unsafe { (v.cancel_timer)(ctx, value) },
            _ => unreachable!(),
        };

        let err = r.into_result().unwrap_err();
        assert_eq!(err.code, PluginErrorCode::NotImplemented);
        assert_eq!(
            err.message_string(),
            format!("{method} is not wired into this host vtable")
        );
    }
}
