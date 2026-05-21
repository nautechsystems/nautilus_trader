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

use std::{collections::BTreeMap, fmt::Display, slice};

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

fn validate_registrations(manifest: &PluginManifest, errors: &mut PluginManifestValidationErrors) {
    let mut seen_type_names = BTreeMap::<String, String>::new();

    if let Some(entries) = validate_slice("custom_data", &manifest.custom_data, errors) {
        for (index, entry) in entries.iter().enumerate() {
            let location = format!("custom_data[{index}]");
            let type_name = validate_type_name(&location, entry.type_name, errors);
            validate_unique_type_name(&mut seen_type_names, &location, type_name, errors);
            if entry.vtable.is_null() {
                errors.push(format!("{location}.vtable must not be null"));
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
            }
        }
    }
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

#[cfg(test)]
mod tests {
    use std::ptr::NonNull;

    use rstest::rstest;

    use super::*;

    static VALID_CUSTOM_DATA: [CustomDataRegistration; 1] = [CustomDataRegistration {
        type_name: BorrowedStr::from_str("TestTick"),
        vtable: NonNull::<CustomDataVTable>::dangling().as_ptr(),
    }];
    static VALID_ACTORS: [ActorRegistration; 1] = [ActorRegistration {
        type_name: BorrowedStr::from_str("TestActor"),
        vtable: NonNull::<ActorVTable>::dangling().as_ptr(),
    }];
    static VALID_STRATEGIES: [StrategyRegistration; 1] = [StrategyRegistration {
        type_name: BorrowedStr::from_str("TestStrategy"),
        vtable: NonNull::<StrategyVTable>::dangling().as_ptr(),
    }];
    static DUPLICATE_CUSTOM_DATA: [CustomDataRegistration; 1] = [CustomDataRegistration {
        type_name: BorrowedStr::from_str("DuplicateType"),
        vtable: NonNull::<CustomDataVTable>::dangling().as_ptr(),
    }];
    static DUPLICATE_ACTORS: [ActorRegistration; 1] = [ActorRegistration {
        type_name: BorrowedStr::from_str("DuplicateType"),
        vtable: NonNull::<ActorVTable>::dangling().as_ptr(),
    }];
    static DUPLICATE_ACTORS_SAME_SLICE: [ActorRegistration; 2] = [
        ActorRegistration {
            type_name: BorrowedStr::from_str("DuplicateActor"),
            vtable: NonNull::<ActorVTable>::dangling().as_ptr(),
        },
        ActorRegistration {
            type_name: BorrowedStr::from_str("DuplicateActor"),
            vtable: NonNull::<ActorVTable>::dangling().as_ptr(),
        },
    ];
    static EMPTY_TYPE_NAME_ACTORS: [ActorRegistration; 1] = [ActorRegistration {
        type_name: BorrowedStr::empty(),
        vtable: NonNull::<ActorVTable>::dangling().as_ptr(),
    }];
    static NULL_VTABLE_CUSTOM_DATA: [CustomDataRegistration; 1] = [CustomDataRegistration {
        type_name: BorrowedStr::from_str("NullVTableType"),
        vtable: std::ptr::null(),
    }];
    static INVALID_UTF8: [u8; 1] = [0xff];

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
            custom_data: Slice::from_slice(&VALID_CUSTOM_DATA),
            actors: Slice::from_slice(&VALID_ACTORS),
            strategies: Slice::from_slice(&VALID_STRATEGIES),
            ..valid_manifest()
        };

        m.validate()
            .expect("well-formed plug-point registrations are valid");
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
            custom_data: Slice::from_slice(&DUPLICATE_CUSTOM_DATA),
            actors: Slice::from_slice(&DUPLICATE_ACTORS),
            strategies: Slice::from_slice(&VALID_STRATEGIES),
            ..valid_manifest()
        };

        let errors = m.validate().expect_err("duplicate type name is invalid");
        assert!(
            errors
                .to_string()
                .contains("type name 'DuplicateType' appears in both custom_data[0] and actors[0]")
        );

        let m = PluginManifest {
            actors: Slice::from_slice(&EMPTY_TYPE_NAME_ACTORS),
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
    fn validate_rejects_duplicate_type_names_within_one_plug_point_slice() {
        let m = PluginManifest {
            actors: Slice::from_slice(&DUPLICATE_ACTORS_SAME_SLICE),
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
