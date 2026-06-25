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

//! Static plug-in metadata returned from `nautilus_plugin_init`.

use std::{fmt::Display, slice};

use nautilus_model::types::fixed::FIXED_PRECISION;

use crate::{
    NAUTILUS_PLUGIN_ABI_VERSION, PLUGIN_BUILD_ID_VERSION, boundary::BorrowedStr, host::HostVTable,
};

/// Signature of the single `extern "C"` entry symbol every plug-in exports
/// under the name [`crate::NAUTILUS_PLUGIN_INIT_SYMBOL`].
pub type PluginInitFn = unsafe extern "C" fn(host: *const HostVTable) -> *const PluginManifest;

/// Versioned build identifier carried by [`PluginManifest`].
///
/// The fields identify the Nautilus plug-in crate and build environment that
/// produced the manifest. The host validates the precision mode because it
/// changes model type layout across the plug-in boundary. Other build fields
/// remain diagnostic.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
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

    /// Model fixed-point precision mode used to build the plug-in.
    pub precision_mode: BorrowedStr<'static>,

    /// Maximum fixed-point decimal precision used to build the plug-in.
    pub fixed_precision: u8,
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
            precision_mode: BorrowedStr::from_str(compiled_precision_mode()),
            fixed_precision: FIXED_PRECISION,
        }
    }
}

/// Returns the model precision mode compiled into this crate.
#[must_use]
pub const fn compiled_precision_mode() -> &'static str {
    if FIXED_PRECISION > 9 {
        "high-precision"
    } else {
        "standard"
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

/// The static, process-lifetime metadata a plug-in returns from
/// `nautilus_plugin_init`.
///
/// Public OSS metadata stops here. Strategy, actor, controller, and model
/// extension registration belongs to the host/sys layer that generates and
/// validates the private bridge contract.
#[repr(C)]
#[derive(Debug)]
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
}

impl PluginManifest {
    /// Returns whether this manifest is compatible with the compiled-in ABI.
    #[must_use]
    pub fn matches_compiled_abi(&self) -> bool {
        self.abi_version == NAUTILUS_PLUGIN_ABI_VERSION
    }

    /// Validates manifest invariants the host relies on before registration.
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
        return;
    }

    validate_optional_str(
        "build_id.nautilus_plugin_version",
        build_id.nautilus_plugin_version,
        errors,
    );
    validate_optional_str("build_id.rustc_version", build_id.rustc_version, errors);
    validate_optional_str("build_id.target_triple", build_id.target_triple, errors);
    validate_optional_str("build_id.build_profile", build_id.build_profile, errors);
    if let Some(precision_mode) =
        validate_required_str("build_id.precision_mode", build_id.precision_mode, errors)
    {
        let expected = compiled_precision_mode();
        if precision_mode != expected {
            errors.push(format!(
                "build_id.precision_mode '{precision_mode}' does not match host precision mode '{expected}'"
            ));
        }
    }

    if build_id.fixed_precision != FIXED_PRECISION {
        errors.push(format!(
            "build_id.fixed_precision {} does not match host fixed precision {}",
            build_id.fixed_precision, FIXED_PRECISION
        ));
    }
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

    // SAFETY: validation only reads the descriptor supplied by the manifest producer.
    let bytes = unsafe { slice::from_raw_parts(value.ptr, value.len) };
    match std::str::from_utf8(bytes) {
        Ok(text) => Some(text),
        Err(e) => {
            errors.push(format!("{field} is not valid UTF-8: {e}"));
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    fn valid_manifest() -> PluginManifest {
        PluginManifest {
            abi_version: NAUTILUS_PLUGIN_ABI_VERSION,
            plugin_name: BorrowedStr::from_str("test-plugin"),
            plugin_vendor: BorrowedStr::from_str("nautech"),
            plugin_version: BorrowedStr::from_str("1.0.0"),
            build_id: PluginBuildId::current(),
        }
    }

    #[rstest]
    fn matches_compiled_abi_accepts_compiled_version() {
        assert!(valid_manifest().matches_compiled_abi());
    }

    #[rstest]
    fn matches_compiled_abi_rejects_mismatch() {
        let manifest = PluginManifest {
            abi_version: NAUTILUS_PLUGIN_ABI_VERSION.wrapping_add(1),
            ..valid_manifest()
        };

        assert!(!manifest.matches_compiled_abi());
    }

    #[rstest]
    fn validate_accepts_valid_manifest() {
        valid_manifest().validate().unwrap();
    }

    #[rstest]
    fn validate_rejects_missing_name() {
        let manifest = PluginManifest {
            plugin_name: BorrowedStr::empty(),
            ..valid_manifest()
        };

        let errors = manifest.validate().unwrap_err();
        assert_eq!(errors.messages(), &["plugin_name must not be empty"]);
    }

    #[rstest]
    fn validate_rejects_mismatched_build_schema() {
        let manifest = PluginManifest {
            build_id: PluginBuildId {
                schema_version: PLUGIN_BUILD_ID_VERSION.wrapping_add(1),
                ..PluginBuildId::current()
            },
            ..valid_manifest()
        };

        let errors = manifest.validate().unwrap_err();
        assert_eq!(
            errors.messages(),
            &[format!(
                "build_id.schema_version {} does not match supported schema {}",
                PLUGIN_BUILD_ID_VERSION.wrapping_add(1),
                PLUGIN_BUILD_ID_VERSION
            )]
        );
    }
}
