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
//! and points at the per-type vtables. The current unreleased v1 surface ships
//! custom-data, actor, and strategy plug-point families. Future released
//! revisions should add new `Slice` fields to [`PluginManifest`] without
//! removing existing ones.

use std::{
    collections::BTreeMap,
    fmt::{Debug, Display},
    slice,
};

use crate::{
    NAUTILUS_PLUGIN_ABI_VERSION, PLUGIN_BUILD_ID_VERSION,
    boundary::{BorrowedStr, Slice},
    host::HostVTable,
    surfaces::{actor::ActorVTable, custom_data::CustomDataVTable, strategy::StrategyVTable},
};

/// Signature of the single `extern "C"` entry symbol every plug-in exports
/// under the name [`crate::NAUTILUS_PLUGIN_INIT_SYMBOL`].
///
/// The host calls this once at load time with a pointer to its `HostVTable`.
/// The plug-in returns a pointer to its `'static` [`PluginManifest`], or null
/// to signal load failure. v1 reports null as `LoadError::NullManifest` with
/// the plug-in path.
pub type PluginInitFn = unsafe extern "C" fn(host: *const HostVTable) -> *const PluginManifest;

/// Versioned build identifier carried by [`PluginManifest`].
///
/// The fields identify the Nautilus plug-in crate and build environment that
/// produced the manifest. They are diagnostic only: ABI compatibility is still
/// enforced by [`PluginManifest::abi_version`].
#[repr(C)]
#[derive(Clone, Copy)]
pub struct PluginBuildId {
    /// Build identifier schema version. Must equal
    /// [`PLUGIN_BUILD_ID_VERSION`] for the fields below.
    pub schema_version: u32,

    /// Version of the `nautilus-plugin` crate used to build the plug-in.
    pub nautilus_plugin_version: BorrowedStr<'static>,

    /// Rust compiler version reported by `rustc --version`, or empty when it
    /// was unavailable to the build script.
    pub rustc_version: BorrowedStr<'static>,

    /// Cargo target triple, or empty when Cargo did not expose one.
    pub target_triple: BorrowedStr<'static>,

    /// Cargo build profile, or empty when Cargo did not expose one.
    pub build_profile: BorrowedStr<'static>,
}

impl PluginBuildId {
    /// Returns the build identifier for the compiled `nautilus-plugin` crate.
    #[must_use]
    pub const fn current() -> Self {
        Self {
            schema_version: PLUGIN_BUILD_ID_VERSION,
            nautilus_plugin_version: BorrowedStr::from_str(env!("CARGO_PKG_VERSION")),
            rustc_version: BorrowedStr::from_str(env!("NAUTILUS_PLUGIN_BUILD_RUSTC_VERSION")),
            target_triple: BorrowedStr::from_str(env!("NAUTILUS_PLUGIN_BUILD_TARGET")),
            build_profile: BorrowedStr::from_str(env!("NAUTILUS_PLUGIN_BUILD_PROFILE")),
        }
    }
}

/// Manifest validation failures collected by the host loader.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PluginManifestValidationErrors {
    messages: Vec<String>,
}

impl PluginManifestValidationErrors {
    /// Returns whether validation found no failures.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.messages.is_empty()
    }

    /// Returns the validation failure messages in deterministic order.
    #[must_use]
    pub fn messages(&self) -> &[String] {
        &self.messages
    }

    fn push(&mut self, message: impl Into<String>) {
        self.messages.push(message.into());
    }
}

impl Display for PluginManifestValidationErrors {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for (index, message) in self.messages.iter().enumerate() {
            if index > 0 {
                write!(f, "; ")?;
            }
            write!(f, "{message}")?;
        }
        Ok(())
    }
}

impl std::error::Error for PluginManifestValidationErrors {}

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

    /// Versioned build identifier for diagnostics.
    pub build_id: PluginBuildId,

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

    /// Validates manifest invariants the host relies on before registration.
    ///
    /// This does not decide plug-in compatibility beyond the explicit ABI and
    /// build-id schema versions. Build-id content stays diagnostic; empty
    /// compiler, target, and profile strings do not make a manifest invalid.
    ///
    /// # Errors
    ///
    /// Returns every structural problem found in the manifest.
    pub fn validate(&self) -> Result<(), PluginManifestValidationErrors> {
        let mut errors = PluginManifestValidationErrors::default();

        validate_required_str("plugin_name", self.plugin_name, &mut errors);
        validate_optional_str("plugin_vendor", self.plugin_vendor, &mut errors);
        validate_required_str("plugin_version", self.plugin_version, &mut errors);
        validate_build_id(&self.build_id, &mut errors);
        validate_registrations(self, &mut errors);

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }
}

fn validate_build_id(build_id: &PluginBuildId, errors: &mut PluginManifestValidationErrors) {
    if build_id.schema_version != PLUGIN_BUILD_ID_VERSION {
        errors.push(format!(
            "build_id.schema_version {} does not match supported schema {}",
            build_id.schema_version, PLUGIN_BUILD_ID_VERSION
        ));
    }

    validate_optional_str(
        "build_id.nautilus_plugin_version",
        build_id.nautilus_plugin_version,
        errors,
    );
    validate_optional_str("build_id.rustc_version", build_id.rustc_version, errors);
    validate_optional_str("build_id.target_triple", build_id.target_triple, errors);
    validate_optional_str("build_id.build_profile", build_id.build_profile, errors);
}

macro_rules! validate_vtable_slots {
    ($location:expr, $type_name:expr, $vtable:expr, $errors:expr, [$($slot:ident),+ $(,)?]) => {
        $(
            validate_vtable_slot(
                $location,
                $type_name,
                stringify!($slot),
                $vtable.$slot.is_some(),
                $errors,
            );
        )+
    };
}

fn validate_registrations(manifest: &PluginManifest, errors: &mut PluginManifestValidationErrors) {
    let mut seen_type_names = BTreeMap::<String, String>::new();

    if let Some(entries) = validate_slice("custom_data", &manifest.custom_data, errors) {
        for (index, entry) in entries.iter().enumerate() {
            let location = format!("custom_data[{index}]");
            let type_name = validate_type_name(&location, entry.type_name, errors);
            validate_unique_type_name(&mut seen_type_names, &location, type_name, errors);
            if entry.vtable.is_null() {
                errors.push(format!("{location}.vtable must not be null"));
            } else {
                validate_custom_data_vtable(&location, type_name, entry.vtable, errors);
            }
        }
    }

    if let Some(entries) = validate_slice("actors", &manifest.actors, errors) {
        for (index, entry) in entries.iter().enumerate() {
            let location = format!("actors[{index}]");
            let type_name = validate_type_name(&location, entry.type_name, errors);
            validate_unique_type_name(&mut seen_type_names, &location, type_name, errors);
            if entry.vtable.is_null() {
                errors.push(format!("{location}.vtable must not be null"));
            } else {
                validate_actor_vtable(&location, type_name, entry.vtable, errors);
            }
        }
    }

    if let Some(entries) = validate_slice("strategies", &manifest.strategies, errors) {
        for (index, entry) in entries.iter().enumerate() {
            let location = format!("strategies[{index}]");
            let type_name = validate_type_name(&location, entry.type_name, errors);
            validate_unique_type_name(&mut seen_type_names, &location, type_name, errors);
            if entry.vtable.is_null() {
                errors.push(format!("{location}.vtable must not be null"));
            } else {
                validate_strategy_vtable(&location, type_name, entry.vtable, errors);
            }
        }
    }
}

fn validate_custom_data_vtable(
    location: &str,
    type_name: Option<&str>,
    vtable: *const CustomDataVTable,
    errors: &mut PluginManifestValidationErrors,
) {
    // SAFETY: caller checked the vtable pointer is non-null. Validation only
    // reads nullable function-pointer slots and never invokes plug-in code.
    let vtable = unsafe { &*vtable };
    validate_vtable_slots!(
        location,
        type_name,
        vtable,
        errors,
        [
            type_name,
            schema_ipc,
            from_json,
            encode_batch,
            decode_batch,
            ts_event,
            ts_init,
            to_json,
            clone_handle,
            drop_handle,
            eq_handles,
        ]
    );
}

fn validate_actor_vtable(
    location: &str,
    type_name: Option<&str>,
    vtable: *const ActorVTable,
    errors: &mut PluginManifestValidationErrors,
) {
    // SAFETY: caller checked the vtable pointer is non-null. Validation only
    // reads nullable function-pointer slots and never invokes plug-in code.
    let vtable = unsafe { &*vtable };
    validate_vtable_slots!(
        location,
        type_name,
        vtable,
        errors,
        [
            create,
            drop_handle,
            type_name,
            on_start,
            on_stop,
            on_resume,
            on_reset,
            on_dispose,
            on_degrade,
            on_fault,
            on_time_event,
            on_quote,
            on_trade,
            on_bar,
            on_mark_price,
            on_index_price,
            on_funding_rate,
            on_instrument_status,
            on_instrument_close,
            on_order_filled,
            on_order_canceled,
            on_signal,
        ]
    );
}

fn validate_strategy_vtable(
    location: &str,
    type_name: Option<&str>,
    vtable: *const StrategyVTable,
    errors: &mut PluginManifestValidationErrors,
) {
    // SAFETY: caller checked the vtable pointer is non-null. Validation only
    // reads nullable function-pointer slots and never invokes plug-in code.
    let vtable = unsafe { &*vtable };
    validate_vtable_slots!(
        location,
        type_name,
        vtable,
        errors,
        [
            create,
            drop_handle,
            type_name,
            on_start,
            on_stop,
            on_resume,
            on_reset,
            on_dispose,
            on_degrade,
            on_fault,
            on_time_event,
            on_quote,
            on_trade,
            on_bar,
            on_mark_price,
            on_index_price,
            on_funding_rate,
            on_instrument_status,
            on_instrument_close,
            on_signal,
            on_order_initialized,
            on_order_submitted,
            on_order_accepted,
            on_order_rejected,
            on_order_filled,
            on_order_canceled,
            on_order_expired,
            on_order_triggered,
            on_order_denied,
            on_order_emulated,
            on_order_released,
            on_order_pending_update,
            on_order_pending_cancel,
            on_order_modify_rejected,
            on_order_cancel_rejected,
            on_order_updated,
            on_position_opened,
            on_position_changed,
            on_position_closed,
        ]
    );
}

fn validate_vtable_slot(
    location: &str,
    type_name: Option<&str>,
    slot: &str,
    is_present: bool,
    errors: &mut PluginManifestValidationErrors,
) {
    if is_present {
        return;
    }

    let type_name = match type_name {
        Some("") | None => "<unknown>",
        Some(value) => value,
    };
    errors.push(format!(
        "{location} type '{type_name}' vtable.{slot} must not be null"
    ));
}

fn validate_type_name<'a>(
    location: &str,
    value: BorrowedStr<'a>,
    errors: &mut PluginManifestValidationErrors,
) -> Option<&'a str> {
    validate_required_str(&format!("{location}.type_name"), value, errors)
}

fn validate_unique_type_name(
    seen_type_names: &mut BTreeMap<String, String>,
    location: &str,
    type_name: Option<&str>,
    errors: &mut PluginManifestValidationErrors,
) {
    let Some(type_name) = type_name else {
        return;
    };

    if type_name.is_empty() {
        return;
    }

    if let Some(first_location) = seen_type_names.get(type_name) {
        errors.push(format!(
            "type name '{type_name}' appears in both {first_location} and {location}"
        ));
    } else {
        seen_type_names.insert(type_name.to_string(), location.to_string());
    }
}

fn validate_slice<'a, T>(
    field: &str,
    value: &Slice<'a, T>,
    errors: &mut PluginManifestValidationErrors,
) -> Option<&'a [T]> {
    if value.len == 0 {
        return Some(&[]);
    }

    if value.ptr.is_null() {
        errors.push(format!(
            "{field} has null pointer with non-zero length {}",
            value.len
        ));
        return None;
    }

    // SAFETY: the manifest contract requires a non-null slice pointer with
    // `len` elements to point at process-lifetime storage in the plug-in.
    Some(unsafe { slice::from_raw_parts(value.ptr, value.len) })
}

fn validate_required_str<'a>(
    field: &str,
    value: BorrowedStr<'a>,
    errors: &mut PluginManifestValidationErrors,
) -> Option<&'a str> {
    let text = validate_optional_str(field, value, errors)?;
    if text.is_empty() {
        errors.push(format!("{field} must not be empty"));
    }
    Some(text)
}

fn validate_optional_str<'a>(
    field: &str,
    value: BorrowedStr<'a>,
    errors: &mut PluginManifestValidationErrors,
) -> Option<&'a str> {
    if value.len == 0 {
        return Some("");
    }

    if value.ptr.is_null() {
        errors.push(format!(
            "{field} has null pointer with non-zero length {}",
            value.len
        ));
        return None;
    }

    // SAFETY: the manifest contract requires borrowed strings to point at
    // process-lifetime storage in the plug-in.
    let bytes = unsafe { slice::from_raw_parts(value.ptr, value.len) };
    match std::str::from_utf8(bytes) {
        Ok(text) => Some(text),
        Err(e) => {
            errors.push(format!("{field} is not valid UTF-8: {e}"));
            None
        }
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

/// Host-side view of a manifest that passed structural validation.
///
/// This wrapper is not part of the ABI. Hosts use it after loader validation
/// so registration code can carry the manifest invariants in the type system.
#[cfg(feature = "host")]
#[derive(Clone, Copy)]
pub struct ValidatedPluginManifest<'a> {
    manifest: &'a PluginManifest,
}

#[cfg(feature = "host")]
impl Debug for ValidatedPluginManifest<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(ValidatedPluginManifest))
            .field("plugin_name", &self.plugin_name())
            .finish()
    }
}

#[cfg(feature = "host")]
impl<'a> ValidatedPluginManifest<'a> {
    /// Validates `manifest` and returns a typed host-side view.
    ///
    /// # Errors
    ///
    /// Returns every structural problem found in the manifest.
    pub fn new(manifest: &'a PluginManifest) -> Result<Self, PluginManifestValidationErrors> {
        manifest.validate()?;
        Ok(Self { manifest })
    }

    /// Returns the raw ABI manifest behind this validated view.
    #[must_use]
    pub fn manifest(self) -> &'a PluginManifest {
        self.manifest
    }

    /// Returns the validated plug-in name.
    #[must_use]
    pub fn plugin_name(self) -> &'static str {
        // SAFETY: validation checked the descriptor and manifest strings live
        // in static plug-in storage.
        unsafe { self.manifest.plugin_name.as_str() }
    }

    /// Returns validated custom-data registrations in manifest order.
    #[must_use]
    pub fn custom_data(self) -> impl ExactSizeIterator<Item = ValidatedCustomDataRegistration> {
        // SAFETY: validation checked the slice descriptor.
        unsafe { self.manifest.custom_data.as_slice() }
            .iter()
            .map(ValidatedCustomDataRegistration::from_validated_registration)
    }

    /// Returns validated actor registrations in manifest order.
    #[must_use]
    pub fn actors(self) -> impl ExactSizeIterator<Item = ValidatedActorRegistration> {
        // SAFETY: validation checked the slice descriptor.
        unsafe { self.manifest.actors.as_slice() }
            .iter()
            .map(ValidatedActorRegistration::from_validated_registration)
    }

    /// Returns validated strategy registrations in manifest order.
    #[must_use]
    pub fn strategies(self) -> impl ExactSizeIterator<Item = ValidatedStrategyRegistration> {
        // SAFETY: validation checked the slice descriptor.
        unsafe { self.manifest.strategies.as_slice() }
            .iter()
            .map(ValidatedStrategyRegistration::from_validated_registration)
    }
}

/// Host-side custom-data registration with a validated type name and vtable.
#[cfg(feature = "host")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ValidatedCustomDataRegistration {
    type_name: &'static str,
    vtable: ValidatedCustomDataVTable,
}

#[cfg(feature = "host")]
impl ValidatedCustomDataRegistration {
    fn from_validated_registration(registration: &CustomDataRegistration) -> Self {
        Self {
            // SAFETY: validation checked the descriptor and manifest strings
            // live in static plug-in storage.
            type_name: unsafe { registration.type_name.as_str() },
            vtable: ValidatedCustomDataVTable::from_validated_ptr(registration.vtable),
        }
    }

    /// Returns the canonical custom-data type name.
    #[must_use]
    pub fn type_name(self) -> &'static str {
        self.type_name
    }

    /// Returns the validated vtable wrapper.
    #[must_use]
    pub fn vtable(self) -> ValidatedCustomDataVTable {
        self.vtable
    }
}

/// Host-side actor registration with a validated type name and vtable.
#[cfg(feature = "host")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ValidatedActorRegistration {
    type_name: &'static str,
    vtable: ValidatedActorVTable,
}

#[cfg(feature = "host")]
impl ValidatedActorRegistration {
    fn from_validated_registration(registration: &ActorRegistration) -> Self {
        Self {
            // SAFETY: validation checked the descriptor and manifest strings
            // live in static plug-in storage.
            type_name: unsafe { registration.type_name.as_str() },
            vtable: ValidatedActorVTable::from_validated_ptr(registration.vtable),
        }
    }

    /// Returns the canonical actor type name.
    #[must_use]
    pub fn type_name(self) -> &'static str {
        self.type_name
    }

    /// Returns the validated vtable wrapper.
    #[must_use]
    pub fn vtable(self) -> ValidatedActorVTable {
        self.vtable
    }
}

/// Host-side strategy registration with a validated type name and vtable.
#[cfg(feature = "host")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ValidatedStrategyRegistration {
    type_name: &'static str,
    vtable: ValidatedStrategyVTable,
}

#[cfg(feature = "host")]
impl ValidatedStrategyRegistration {
    fn from_validated_registration(registration: &StrategyRegistration) -> Self {
        Self {
            // SAFETY: validation checked the descriptor and manifest strings
            // live in static plug-in storage.
            type_name: unsafe { registration.type_name.as_str() },
            vtable: ValidatedStrategyVTable::from_validated_ptr(registration.vtable),
        }
    }

    /// Returns the canonical strategy type name.
    #[must_use]
    pub fn type_name(self) -> &'static str {
        self.type_name
    }

    /// Returns the validated vtable wrapper.
    #[must_use]
    pub fn vtable(self) -> ValidatedStrategyVTable {
        self.vtable
    }
}

/// Host-side pointer to a validated [`CustomDataVTable`].
#[cfg(feature = "host")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ValidatedCustomDataVTable {
    ptr: std::ptr::NonNull<CustomDataVTable>,
}

#[cfg(feature = "host")]
impl ValidatedCustomDataVTable {
    fn from_validated_ptr(ptr: *const CustomDataVTable) -> Self {
        Self {
            ptr: std::ptr::NonNull::new(ptr.cast_mut())
                .expect("validated manifest stores non-null CustomDataVTable"),
        }
    }

    /// Wraps a custom-data vtable pointer that the caller already validated.
    ///
    /// # Safety
    ///
    /// `ptr` must be non-null, point at immutable process-lifetime storage,
    /// and contain every required [`CustomDataVTable`] function slot.
    #[must_use]
    pub unsafe fn from_raw_unchecked(ptr: *const CustomDataVTable) -> Self {
        Self::from_validated_ptr(ptr)
    }

    /// Returns the raw vtable pointer for ABI calls.
    #[must_use]
    pub fn as_ptr(self) -> *const CustomDataVTable {
        self.ptr.as_ptr()
    }
}

/// SAFETY: validated vtables point at immutable process-lifetime storage.
#[cfg(feature = "host")]
unsafe impl Send for ValidatedCustomDataVTable {}
/// SAFETY: see `Send`.
#[cfg(feature = "host")]
unsafe impl Sync for ValidatedCustomDataVTable {}

/// Host-side pointer to a validated [`ActorVTable`].
#[cfg(feature = "host")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ValidatedActorVTable {
    ptr: std::ptr::NonNull<ActorVTable>,
}

#[cfg(feature = "host")]
impl ValidatedActorVTable {
    fn from_validated_ptr(ptr: *const ActorVTable) -> Self {
        Self {
            ptr: std::ptr::NonNull::new(ptr.cast_mut())
                .expect("validated manifest stores non-null ActorVTable"),
        }
    }

    /// Wraps an actor vtable pointer that the caller already validated.
    ///
    /// # Safety
    ///
    /// `ptr` must be non-null, point at immutable process-lifetime storage,
    /// and contain every required [`ActorVTable`] function slot.
    #[must_use]
    pub unsafe fn from_raw_unchecked(ptr: *const ActorVTable) -> Self {
        Self::from_validated_ptr(ptr)
    }

    /// Returns the raw vtable pointer for ABI calls.
    #[must_use]
    pub fn as_ptr(self) -> *const ActorVTable {
        self.ptr.as_ptr()
    }
}

/// SAFETY: validated vtables point at immutable process-lifetime storage.
#[cfg(feature = "host")]
unsafe impl Send for ValidatedActorVTable {}
/// SAFETY: see `Send`.
#[cfg(feature = "host")]
unsafe impl Sync for ValidatedActorVTable {}

/// Host-side pointer to a validated [`StrategyVTable`].
#[cfg(feature = "host")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ValidatedStrategyVTable {
    ptr: std::ptr::NonNull<StrategyVTable>,
}

#[cfg(feature = "host")]
impl ValidatedStrategyVTable {
    fn from_validated_ptr(ptr: *const StrategyVTable) -> Self {
        Self {
            ptr: std::ptr::NonNull::new(ptr.cast_mut())
                .expect("validated manifest stores non-null StrategyVTable"),
        }
    }

    /// Wraps a strategy vtable pointer that the caller already validated.
    ///
    /// # Safety
    ///
    /// `ptr` must be non-null, point at immutable process-lifetime storage,
    /// and contain every required [`StrategyVTable`] function slot.
    #[must_use]
    pub unsafe fn from_raw_unchecked(ptr: *const StrategyVTable) -> Self {
        Self::from_validated_ptr(ptr)
    }

    /// Returns the raw vtable pointer for ABI calls.
    #[must_use]
    pub fn as_ptr(self) -> *const StrategyVTable {
        self.ptr.as_ptr()
    }
}

/// SAFETY: validated vtables point at immutable process-lifetime storage.
#[cfg(feature = "host")]
unsafe impl Send for ValidatedStrategyVTable {}
/// SAFETY: see `Send`.
#[cfg(feature = "host")]
unsafe impl Sync for ValidatedStrategyVTable {}

#[cfg(test)]
mod tests {
    use std::sync::LazyLock;

    use rstest::rstest;

    use super::*;

    #[derive(Clone, PartialEq)]
    struct ManifestTestTick;

    impl crate::surfaces::custom_data::PluginCustomData for ManifestTestTick {
        const TYPE_NAME: &'static str = "ManifestTestTick";

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

    struct ManifestTestActor;

    impl crate::surfaces::actor::PluginActor for ManifestTestActor {
        const TYPE_NAME: &'static str = "ManifestTestActor";

        fn new(
            _host: *const HostVTable,
            _ctx: *const crate::host::HostContext,
            _config_json: &str,
        ) -> Self {
            Self
        }
    }

    struct ManifestTestStrategy;

    impl crate::surfaces::strategy::PluginStrategy for ManifestTestStrategy {
        const TYPE_NAME: &'static str = "ManifestTestStrategy";

        fn new(
            _host: *const HostVTable,
            _ctx: *const crate::host::HostContext,
            _config_json: &str,
        ) -> Self {
            Self
        }
    }

    static VALID_CUSTOM_DATA: LazyLock<[CustomDataRegistration; 1]> = LazyLock::new(|| {
        [CustomDataRegistration {
            type_name: BorrowedStr::from_str("TestTick"),
            vtable: crate::surfaces::custom_data::custom_data_vtable::<ManifestTestTick>(),
        }]
    });
    static VALID_ACTORS: LazyLock<[ActorRegistration; 1]> = LazyLock::new(|| {
        [ActorRegistration {
            type_name: BorrowedStr::from_str("TestActor"),
            vtable: crate::surfaces::actor::actor_vtable::<ManifestTestActor>(),
        }]
    });
    static VALID_STRATEGIES: LazyLock<[StrategyRegistration; 1]> = LazyLock::new(|| {
        [StrategyRegistration {
            type_name: BorrowedStr::from_str("TestStrategy"),
            vtable: crate::surfaces::strategy::strategy_vtable::<ManifestTestStrategy>(),
        }]
    });
    static DUPLICATE_CUSTOM_DATA: LazyLock<[CustomDataRegistration; 1]> = LazyLock::new(|| {
        [CustomDataRegistration {
            type_name: BorrowedStr::from_str("DuplicateType"),
            vtable: crate::surfaces::custom_data::custom_data_vtable::<ManifestTestTick>(),
        }]
    });
    static DUPLICATE_ACTORS: LazyLock<[ActorRegistration; 1]> = LazyLock::new(|| {
        [ActorRegistration {
            type_name: BorrowedStr::from_str("DuplicateType"),
            vtable: crate::surfaces::actor::actor_vtable::<ManifestTestActor>(),
        }]
    });
    static DUPLICATE_ACTORS_SAME_SLICE: LazyLock<[ActorRegistration; 2]> = LazyLock::new(|| {
        [
            ActorRegistration {
                type_name: BorrowedStr::from_str("DuplicateActor"),
                vtable: crate::surfaces::actor::actor_vtable::<ManifestTestActor>(),
            },
            ActorRegistration {
                type_name: BorrowedStr::from_str("DuplicateActor"),
                vtable: crate::surfaces::actor::actor_vtable::<ManifestTestActor>(),
            },
        ]
    });
    static EMPTY_TYPE_NAME_ACTORS: LazyLock<[ActorRegistration; 1]> = LazyLock::new(|| {
        [ActorRegistration {
            type_name: BorrowedStr::empty(),
            vtable: crate::surfaces::actor::actor_vtable::<ManifestTestActor>(),
        }]
    });
    static NULL_VTABLE_CUSTOM_DATA: [CustomDataRegistration; 1] = [CustomDataRegistration {
        type_name: BorrowedStr::from_str("NullVTableType"),
        vtable: std::ptr::null(),
    }];
    static INVALID_UTF8: [u8; 1] = [0xff];

    fn custom_data_registration(
        type_name: &'static str,
        vtable: *const CustomDataVTable,
    ) -> Slice<'static, CustomDataRegistration> {
        let entries = Box::leak(Box::new([CustomDataRegistration {
            type_name: BorrowedStr::from_str(type_name),
            vtable,
        }]));
        Slice::from_slice(entries)
    }

    fn actor_registration(
        type_name: &'static str,
        vtable: *const ActorVTable,
    ) -> Slice<'static, ActorRegistration> {
        let entries = Box::leak(Box::new([ActorRegistration {
            type_name: BorrowedStr::from_str(type_name),
            vtable,
        }]));
        Slice::from_slice(entries)
    }

    fn strategy_registration(
        type_name: &'static str,
        vtable: *const StrategyVTable,
    ) -> Slice<'static, StrategyRegistration> {
        let entries = Box::leak(Box::new([StrategyRegistration {
            type_name: BorrowedStr::from_str(type_name),
            vtable,
        }]));
        Slice::from_slice(entries)
    }

    fn custom_data_vtable_missing_schema_ipc() -> *const CustomDataVTable {
        let valid = crate::surfaces::custom_data::custom_data_vtable::<ManifestTestTick>();
        // SAFETY: generated test vtable lives for the process lifetime.
        let valid = unsafe { &*valid };
        let vtable = Box::leak(Box::new(CustomDataVTable {
            type_name: valid.type_name,
            schema_ipc: None,
            from_json: valid.from_json,
            encode_batch: valid.encode_batch,
            decode_batch: valid.decode_batch,
            ts_event: valid.ts_event,
            ts_init: valid.ts_init,
            to_json: valid.to_json,
            clone_handle: valid.clone_handle,
            drop_handle: valid.drop_handle,
            eq_handles: valid.eq_handles,
        }));
        std::ptr::from_ref(&*vtable)
    }

    fn custom_data_vtable_missing_type_name() -> *const CustomDataVTable {
        let valid = crate::surfaces::custom_data::custom_data_vtable::<ManifestTestTick>();
        // SAFETY: generated test vtable lives for the process lifetime.
        let valid = unsafe { &*valid };
        let vtable = Box::leak(Box::new(CustomDataVTable {
            type_name: None,
            schema_ipc: valid.schema_ipc,
            from_json: valid.from_json,
            encode_batch: valid.encode_batch,
            decode_batch: valid.decode_batch,
            ts_event: valid.ts_event,
            ts_init: valid.ts_init,
            to_json: valid.to_json,
            clone_handle: valid.clone_handle,
            drop_handle: valid.drop_handle,
            eq_handles: valid.eq_handles,
        }));
        std::ptr::from_ref(&*vtable)
    }

    fn actor_vtable_missing_on_quote() -> *const ActorVTable {
        let valid = crate::surfaces::actor::actor_vtable::<ManifestTestActor>();
        // SAFETY: generated test vtable lives for the process lifetime.
        let valid = unsafe { &*valid };
        let vtable = Box::leak(Box::new(ActorVTable {
            create: valid.create,
            drop_handle: valid.drop_handle,
            type_name: valid.type_name,
            on_start: valid.on_start,
            on_stop: valid.on_stop,
            on_resume: valid.on_resume,
            on_reset: valid.on_reset,
            on_dispose: valid.on_dispose,
            on_degrade: valid.on_degrade,
            on_fault: valid.on_fault,
            on_time_event: valid.on_time_event,
            on_quote: None,
            on_trade: valid.on_trade,
            on_bar: valid.on_bar,
            on_mark_price: valid.on_mark_price,
            on_index_price: valid.on_index_price,
            on_funding_rate: valid.on_funding_rate,
            on_instrument_status: valid.on_instrument_status,
            on_instrument_close: valid.on_instrument_close,
            on_order_filled: valid.on_order_filled,
            on_order_canceled: valid.on_order_canceled,
            on_signal: valid.on_signal,
        }));
        std::ptr::from_ref(&*vtable)
    }

    fn actor_vtable_missing_create() -> *const ActorVTable {
        let valid = crate::surfaces::actor::actor_vtable::<ManifestTestActor>();
        // SAFETY: generated test vtable lives for the process lifetime.
        let valid = unsafe { &*valid };
        let vtable = Box::leak(Box::new(ActorVTable {
            create: None,
            drop_handle: valid.drop_handle,
            type_name: valid.type_name,
            on_start: valid.on_start,
            on_stop: valid.on_stop,
            on_resume: valid.on_resume,
            on_reset: valid.on_reset,
            on_dispose: valid.on_dispose,
            on_degrade: valid.on_degrade,
            on_fault: valid.on_fault,
            on_time_event: valid.on_time_event,
            on_quote: valid.on_quote,
            on_trade: valid.on_trade,
            on_bar: valid.on_bar,
            on_mark_price: valid.on_mark_price,
            on_index_price: valid.on_index_price,
            on_funding_rate: valid.on_funding_rate,
            on_instrument_status: valid.on_instrument_status,
            on_instrument_close: valid.on_instrument_close,
            on_order_filled: valid.on_order_filled,
            on_order_canceled: valid.on_order_canceled,
            on_signal: valid.on_signal,
        }));
        std::ptr::from_ref(&*vtable)
    }

    fn strategy_vtable_missing_on_position_closed() -> *const StrategyVTable {
        let valid = crate::surfaces::strategy::strategy_vtable::<ManifestTestStrategy>();
        // SAFETY: generated test vtable lives for the process lifetime.
        let valid = unsafe { &*valid };
        let vtable = Box::leak(Box::new(StrategyVTable {
            create: valid.create,
            drop_handle: valid.drop_handle,
            type_name: valid.type_name,
            on_start: valid.on_start,
            on_stop: valid.on_stop,
            on_resume: valid.on_resume,
            on_reset: valid.on_reset,
            on_dispose: valid.on_dispose,
            on_degrade: valid.on_degrade,
            on_fault: valid.on_fault,
            on_time_event: valid.on_time_event,
            on_quote: valid.on_quote,
            on_trade: valid.on_trade,
            on_bar: valid.on_bar,
            on_mark_price: valid.on_mark_price,
            on_index_price: valid.on_index_price,
            on_funding_rate: valid.on_funding_rate,
            on_instrument_status: valid.on_instrument_status,
            on_instrument_close: valid.on_instrument_close,
            on_signal: valid.on_signal,
            on_order_initialized: valid.on_order_initialized,
            on_order_submitted: valid.on_order_submitted,
            on_order_accepted: valid.on_order_accepted,
            on_order_rejected: valid.on_order_rejected,
            on_order_filled: valid.on_order_filled,
            on_order_canceled: valid.on_order_canceled,
            on_order_expired: valid.on_order_expired,
            on_order_triggered: valid.on_order_triggered,
            on_order_denied: valid.on_order_denied,
            on_order_emulated: valid.on_order_emulated,
            on_order_released: valid.on_order_released,
            on_order_pending_update: valid.on_order_pending_update,
            on_order_pending_cancel: valid.on_order_pending_cancel,
            on_order_modify_rejected: valid.on_order_modify_rejected,
            on_order_cancel_rejected: valid.on_order_cancel_rejected,
            on_order_updated: valid.on_order_updated,
            on_position_opened: valid.on_position_opened,
            on_position_changed: valid.on_position_changed,
            on_position_closed: None,
        }));
        std::ptr::from_ref(&*vtable)
    }

    fn strategy_vtable_missing_drop_handle() -> *const StrategyVTable {
        let valid = crate::surfaces::strategy::strategy_vtable::<ManifestTestStrategy>();
        // SAFETY: generated test vtable lives for the process lifetime.
        let valid = unsafe { &*valid };
        let vtable = Box::leak(Box::new(StrategyVTable {
            create: valid.create,
            drop_handle: None,
            type_name: valid.type_name,
            on_start: valid.on_start,
            on_stop: valid.on_stop,
            on_resume: valid.on_resume,
            on_reset: valid.on_reset,
            on_dispose: valid.on_dispose,
            on_degrade: valid.on_degrade,
            on_fault: valid.on_fault,
            on_time_event: valid.on_time_event,
            on_quote: valid.on_quote,
            on_trade: valid.on_trade,
            on_bar: valid.on_bar,
            on_mark_price: valid.on_mark_price,
            on_index_price: valid.on_index_price,
            on_funding_rate: valid.on_funding_rate,
            on_instrument_status: valid.on_instrument_status,
            on_instrument_close: valid.on_instrument_close,
            on_signal: valid.on_signal,
            on_order_initialized: valid.on_order_initialized,
            on_order_submitted: valid.on_order_submitted,
            on_order_accepted: valid.on_order_accepted,
            on_order_rejected: valid.on_order_rejected,
            on_order_filled: valid.on_order_filled,
            on_order_canceled: valid.on_order_canceled,
            on_order_expired: valid.on_order_expired,
            on_order_triggered: valid.on_order_triggered,
            on_order_denied: valid.on_order_denied,
            on_order_emulated: valid.on_order_emulated,
            on_order_released: valid.on_order_released,
            on_order_pending_update: valid.on_order_pending_update,
            on_order_pending_cancel: valid.on_order_pending_cancel,
            on_order_modify_rejected: valid.on_order_modify_rejected,
            on_order_cancel_rejected: valid.on_order_cancel_rejected,
            on_order_updated: valid.on_order_updated,
            on_position_opened: valid.on_position_opened,
            on_position_changed: valid.on_position_changed,
            on_position_closed: valid.on_position_closed,
        }));
        std::ptr::from_ref(&*vtable)
    }

    fn valid_manifest() -> PluginManifest {
        PluginManifest {
            abi_version: NAUTILUS_PLUGIN_ABI_VERSION,
            plugin_name: BorrowedStr::from_str("test"),
            plugin_vendor: BorrowedStr::from_str("nautech"),
            plugin_version: BorrowedStr::from_str("0.0.0"),
            build_id: PluginBuildId::current(),
            custom_data: Slice::empty(),
            actors: Slice::empty(),
            strategies: Slice::empty(),
        }
    }

    #[rstest]
    fn current_build_id_carries_compile_time_metadata() {
        let id = PluginBuildId::current();

        assert_eq!(id.schema_version, PLUGIN_BUILD_ID_VERSION);
        // SAFETY: build id strings are baked into static crate storage.
        assert_eq!(
            unsafe { id.nautilus_plugin_version.as_str() },
            env!("CARGO_PKG_VERSION")
        );
        // SAFETY: see above.
        assert!(!unsafe { id.target_triple.as_str() }.is_empty());
        // SAFETY: see above.
        assert!(!unsafe { id.build_profile.as_str() }.is_empty());
    }

    #[rstest]
    fn empty_manifest_matches_compiled_abi() {
        let m = valid_manifest();
        assert!(m.matches_compiled_abi());
        m.validate().expect("empty plug-point manifest is valid");
    }

    #[rstest]
    #[case::off_by_one(NAUTILUS_PLUGIN_ABI_VERSION.wrapping_add(1))]
    #[case::zero(0)]
    #[case::max(u32::MAX)]
    fn mismatched_manifest_rejects(#[case] abi: u32) {
        let m = PluginManifest {
            abi_version: abi,
            ..valid_manifest()
        };
        assert!(!m.matches_compiled_abi());
    }

    #[rstest]
    fn validate_accepts_manifest_with_all_plug_point_families() {
        let m = PluginManifest {
            custom_data: Slice::from_slice(&*VALID_CUSTOM_DATA),
            actors: Slice::from_slice(&*VALID_ACTORS),
            strategies: Slice::from_slice(&*VALID_STRATEGIES),
            ..valid_manifest()
        };

        m.validate()
            .expect("well-formed plug-point registrations are valid");
    }

    #[cfg(feature = "host")]
    #[rstest]
    fn validated_manifest_exposes_wrapped_registrations() {
        let m = PluginManifest {
            custom_data: Slice::from_slice(&*VALID_CUSTOM_DATA),
            actors: Slice::from_slice(&*VALID_ACTORS),
            strategies: Slice::from_slice(&*VALID_STRATEGIES),
            ..valid_manifest()
        };

        let manifest = ValidatedPluginManifest::new(&m)
            .expect("well-formed plug-point registrations are valid");
        let custom_data = manifest.custom_data().next().expect("custom data entry");
        let actor = manifest.actors().next().expect("actor entry");
        let strategy = manifest.strategies().next().expect("strategy entry");

        assert_eq!(manifest.plugin_name(), "test");
        assert_eq!(custom_data.type_name(), "TestTick");
        assert_eq!(actor.type_name(), "TestActor");
        assert_eq!(strategy.type_name(), "TestStrategy");
        assert_eq!(custom_data.vtable().as_ptr(), VALID_CUSTOM_DATA[0].vtable);
        assert_eq!(actor.vtable().as_ptr(), VALID_ACTORS[0].vtable);
        assert_eq!(strategy.vtable().as_ptr(), VALID_STRATEGIES[0].vtable);
    }

    #[rstest]
    fn validate_rejects_empty_required_manifest_identifiers() {
        let m = PluginManifest {
            plugin_name: BorrowedStr::empty(),
            plugin_version: BorrowedStr::empty(),
            ..valid_manifest()
        };

        let errors = m.validate().expect_err("empty identifiers are invalid");

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
                .any(|message| message == "plugin_version must not be empty")
        );
    }

    #[rstest]
    fn validate_rejects_mismatched_build_id_schema() {
        let m = PluginManifest {
            build_id: PluginBuildId {
                schema_version: PLUGIN_BUILD_ID_VERSION + 1,
                ..PluginBuildId::current()
            },
            ..valid_manifest()
        };

        let errors = m
            .validate()
            .expect_err("mismatched build-id schema is invalid");

        let expected = format!(
            "build_id.schema_version {} does not match supported schema {}",
            PLUGIN_BUILD_ID_VERSION + 1,
            PLUGIN_BUILD_ID_VERSION
        );
        assert!(errors.to_string().contains(&expected));
    }

    #[rstest]
    fn validate_rejects_empty_type_name_duplicate_type_name_and_null_vtable() {
        let m = PluginManifest {
            custom_data: Slice::from_slice(&*DUPLICATE_CUSTOM_DATA),
            actors: Slice::from_slice(&*DUPLICATE_ACTORS),
            strategies: Slice::from_slice(&*VALID_STRATEGIES),
            ..valid_manifest()
        };

        let errors = m.validate().expect_err("duplicate type name is invalid");
        assert!(
            errors
                .to_string()
                .contains("type name 'DuplicateType' appears in both custom_data[0] and actors[0]")
        );

        let m = PluginManifest {
            actors: Slice::from_slice(&*EMPTY_TYPE_NAME_ACTORS),
            ..valid_manifest()
        };
        let errors = m.validate().expect_err("empty type name is invalid");
        assert!(
            errors
                .messages()
                .iter()
                .any(|message| message == "actors[0].type_name must not be empty")
        );

        let m = PluginManifest {
            custom_data: Slice::from_slice(&NULL_VTABLE_CUSTOM_DATA),
            ..valid_manifest()
        };
        let errors = m.validate().expect_err("null vtable is invalid");
        assert!(
            errors
                .messages()
                .iter()
                .any(|message| message == "custom_data[0].vtable must not be null")
        );
    }

    #[rstest]
    fn validate_rejects_null_required_vtable_slots() {
        let m = PluginManifest {
            custom_data: custom_data_registration(
                "BadTick",
                custom_data_vtable_missing_schema_ipc(),
            ),
            actors: actor_registration("BadActor", actor_vtable_missing_on_quote()),
            strategies: strategy_registration(
                "BadStrategy",
                strategy_vtable_missing_on_position_closed(),
            ),
            ..valid_manifest()
        };

        let errors = m
            .validate()
            .expect_err("null required vtable slots are invalid");
        let rendered = errors.to_string();

        assert!(
            rendered.contains("custom_data[0] type 'BadTick' vtable.schema_ipc must not be null")
        );
        assert!(rendered.contains("actors[0] type 'BadActor' vtable.on_quote must not be null"));
        assert!(rendered.contains(
            "strategies[0] type 'BadStrategy' vtable.on_position_closed must not be null"
        ));
    }

    #[rstest]
    fn validate_rejects_null_identity_constructor_and_drop_vtable_slots() {
        let m = PluginManifest {
            custom_data: custom_data_registration(
                "MissingTypeNameTick",
                custom_data_vtable_missing_type_name(),
            ),
            actors: actor_registration("MissingCreateActor", actor_vtable_missing_create()),
            strategies: strategy_registration(
                "MissingDropStrategy",
                strategy_vtable_missing_drop_handle(),
            ),
            ..valid_manifest()
        };

        let errors = m
            .validate()
            .expect_err("identity, constructor, and drop slots are required");
        let rendered = errors.to_string();

        assert!(rendered.contains(
            "custom_data[0] type 'MissingTypeNameTick' vtable.type_name must not be null"
        ));
        assert!(
            rendered.contains("actors[0] type 'MissingCreateActor' vtable.create must not be null")
        );
        assert!(rendered.contains(
            "strategies[0] type 'MissingDropStrategy' vtable.drop_handle must not be null"
        ));
    }

    #[rstest]
    fn validate_rejects_duplicate_type_names_within_one_plug_point_slice() {
        let m = PluginManifest {
            actors: Slice::from_slice(&*DUPLICATE_ACTORS_SAME_SLICE),
            ..valid_manifest()
        };

        let errors = m
            .validate()
            .expect_err("duplicate type names in one slice are invalid");

        assert!(
            errors
                .to_string()
                .contains("type name 'DuplicateActor' appears in both actors[0] and actors[1]")
        );
    }

    #[rstest]
    fn validate_rejects_malformed_raw_string_and_slice_descriptors() {
        let mut plugin_name = BorrowedStr::empty();
        plugin_name.ptr = INVALID_UTF8.as_ptr();
        plugin_name.len = INVALID_UTF8.len();

        let mut plugin_vendor = BorrowedStr::empty();
        plugin_vendor.len = 1;

        let mut custom_data: Slice<'static, CustomDataRegistration> = Slice::empty();
        custom_data.len = 1;

        let m = PluginManifest {
            plugin_name,
            plugin_vendor,
            custom_data,
            ..valid_manifest()
        };

        let errors = m
            .validate()
            .expect_err("malformed raw descriptors are invalid");
        let rendered = errors.to_string();

        assert!(rendered.contains("plugin_name is not valid UTF-8"));
        assert!(rendered.contains("plugin_vendor has null pointer with non-zero length 1"));
        assert!(rendered.contains("custom_data has null pointer with non-zero length 1"));
    }
}
