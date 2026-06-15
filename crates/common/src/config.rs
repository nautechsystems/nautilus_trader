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

//! Path-aware configuration validation errors and checks.

use std::{error::Error, fmt::Display};

/// Result type for configuration validation.
pub type ConfigResult<T> = Result<T, ConfigError>;

/// A typed configuration validation error with owned field paths.
///
/// Variants store owned field paths and explanatory text. Callers should avoid placing
/// secrets in reason strings, reference names, or duplicate labels.
#[allow(
    clippy::module_name_repetitions,
    reason = "public name states the error domain when imported outside the module"
)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigError {
    /// A field is not supported by the current runtime.
    UnsupportedField { field: String, reason: String },
    /// A field value is not supported by the current runtime.
    UnsupportedValue { field: String, reason: String },
    /// A required field is missing.
    MissingField { field: String },
    /// A required field is present but empty.
    EmptyField { field: String },
    /// A field value is invalid.
    InvalidValue { field: String, reason: String },
    /// A field value has an invalid format.
    InvalidFormat { field: String, expected: String },
    /// A field value is outside the accepted range.
    Range { field: String, reason: String },
    /// Fields were set together but are mutually exclusive.
    MutuallyExclusiveFields { fields: Vec<String> },
    /// At least one of the listed fields is required.
    RequiredOneOf { fields: Vec<String> },
    /// A field requires another field.
    Dependency {
        field: String,
        depends_on: String,
        reason: String,
    },
    /// A field contains a duplicate entry.
    Duplicate {
        field: String,
        value: Option<String>,
    },
    /// A field requires a disabled feature.
    FeatureDisabled { field: String, feature: String },
    /// A field references an invalid object.
    InvalidReference {
        field: String,
        reference: String,
        reason: String,
    },
    /// Multiple config validation errors were collected.
    Multiple { errors: Vec<Self> },
}

impl ConfigError {
    /// Creates an unsupported-field error.
    pub fn unsupported_field(field: impl Into<String>, reason: impl Into<String>) -> Self {
        Self::UnsupportedField {
            field: field.into(),
            reason: reason.into(),
        }
    }

    /// Creates an unsupported-value error.
    pub fn unsupported_value(field: impl Into<String>, reason: impl Into<String>) -> Self {
        Self::UnsupportedValue {
            field: field.into(),
            reason: reason.into(),
        }
    }

    /// Creates a missing-field error.
    pub fn missing_field(field: impl Into<String>) -> Self {
        Self::MissingField {
            field: field.into(),
        }
    }

    /// Creates an empty-field error.
    pub fn empty_field(field: impl Into<String>) -> Self {
        Self::EmptyField {
            field: field.into(),
        }
    }

    /// Creates an invalid-value error.
    pub fn invalid_value(field: impl Into<String>, reason: impl Into<String>) -> Self {
        Self::InvalidValue {
            field: field.into(),
            reason: reason.into(),
        }
    }

    /// Creates an invalid-format error.
    pub fn invalid_format(field: impl Into<String>, expected: impl Into<String>) -> Self {
        Self::InvalidFormat {
            field: field.into(),
            expected: expected.into(),
        }
    }

    /// Creates a range error.
    pub fn range(field: impl Into<String>, reason: impl Into<String>) -> Self {
        Self::Range {
            field: field.into(),
            reason: reason.into(),
        }
    }

    /// Creates a mutually-exclusive-fields error.
    pub fn mutually_exclusive_fields(fields: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self::MutuallyExclusiveFields {
            fields: fields.into_iter().map(Into::into).collect(),
        }
    }

    /// Creates a required-one-of error.
    pub fn required_one_of(fields: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self::RequiredOneOf {
            fields: fields.into_iter().map(Into::into).collect(),
        }
    }

    /// Creates a dependency error.
    pub fn dependency(
        field: impl Into<String>,
        depends_on: impl Into<String>,
        reason: impl Into<String>,
    ) -> Self {
        Self::Dependency {
            field: field.into(),
            depends_on: depends_on.into(),
            reason: reason.into(),
        }
    }

    /// Creates a duplicate-entry error.
    pub fn duplicate(field: impl Into<String>, value: Option<String>) -> Self {
        Self::Duplicate {
            field: field.into(),
            value,
        }
    }

    /// Creates a feature-disabled error.
    pub fn feature_disabled(field: impl Into<String>, feature: impl Into<String>) -> Self {
        Self::FeatureDisabled {
            field: field.into(),
            feature: feature.into(),
        }
    }

    /// Creates an invalid-reference error.
    pub fn invalid_reference(
        field: impl Into<String>,
        reference: impl Into<String>,
        reason: impl Into<String>,
    ) -> Self {
        Self::InvalidReference {
            field: field.into(),
            reference: reference.into(),
            reason: reason.into(),
        }
    }

    /// Creates a multiple-errors error.
    pub fn multiple(errors: Vec<Self>) -> Self {
        Self::Multiple { errors }
    }
}

impl Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnsupportedField { field, reason } => write!(f, "{field} is {reason}"),
            Self::UnsupportedValue { field, reason } => {
                write!(f, "{field} has unsupported value: {reason}")
            }
            Self::MissingField { field } => write!(f, "{field} is required"),
            Self::EmptyField { field } => write!(f, "{field} must not be empty"),
            Self::InvalidValue { field, reason } | Self::Range { field, reason } => {
                write!(f, "invalid {field}: {reason}")
            }
            Self::InvalidFormat { field, expected } => write!(f, "invalid {field}: {expected}"),
            Self::MutuallyExclusiveFields { fields } => {
                write!(f, "mutually exclusive fields: ")?;
                write_fields(f, fields)
            }
            Self::RequiredOneOf { fields } => {
                write!(f, "one of these fields is required: ")?;
                write_fields(f, fields)
            }
            Self::Dependency {
                field,
                depends_on,
                reason,
            } => write!(f, "{field} requires {depends_on}: {reason}"),
            Self::Duplicate { field, value } => match value {
                Some(value) => write!(f, "duplicate {field}: {value}"),
                None => write!(f, "duplicate {field}"),
            },
            Self::FeatureDisabled { field, feature } => {
                write!(f, "{field} requires feature `{feature}`")
            }
            Self::InvalidReference {
                field,
                reference,
                reason,
            } => write!(f, "invalid {field} reference {reference}: {reason}"),
            Self::Multiple { errors } => {
                write!(f, "multiple config validation errors")?;
                if !errors.is_empty() {
                    write!(f, ": ")?;

                    for (index, error) in errors.iter().enumerate() {
                        if index > 0 {
                            write!(f, "; ")?;
                        }
                        write!(f, "{}. {error}", index + 1)?;
                    }
                }
                Ok(())
            }
        }
    }
}

impl Error for ConfigError {}

fn write_fields(f: &mut std::fmt::Formatter<'_>, fields: &[String]) -> std::fmt::Result {
    for (index, field) in fields.iter().enumerate() {
        if index > 0 {
            write!(f, ", ")?;
        }
        write!(f, "{field}")?;
    }
    Ok(())
}

/// Collects configuration validation errors.
#[allow(
    clippy::module_name_repetitions,
    reason = "public name states the error domain when imported outside the module"
)]
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConfigErrorCollector {
    errors: Vec<ConfigError>,
}

impl ConfigErrorCollector {
    /// Creates an empty collector.
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates an empty collector with capacity for `capacity` errors.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            errors: Vec::with_capacity(capacity),
        }
    }

    /// Returns `true` if no errors have been collected.
    pub fn is_empty(&self) -> bool {
        self.errors.is_empty()
    }

    /// Returns the number of collected errors.
    pub fn len(&self) -> usize {
        self.errors.len()
    }

    /// Returns the collected errors.
    pub fn errors(&self) -> &[ConfigError] {
        &self.errors
    }

    /// Adds an error to the collection.
    pub fn push(&mut self, error: ConfigError) {
        match error {
            ConfigError::Multiple { errors } => self.errors.extend(errors),
            error => self.errors.push(error),
        }
    }

    /// Adds `error` when `condition` is false.
    pub fn check(&mut self, condition: bool, error: ConfigError) {
        if !condition {
            self.push(error);
        }
    }

    /// Adds the error from a validation result.
    pub fn collect(&mut self, result: ConfigResult<()>) {
        if let Err(e) = result {
            self.push(e);
        }
    }

    /// Converts the collection to a validation result.
    ///
    /// # Errors
    ///
    /// Returns the collected error, or a [`ConfigError::Multiple`] when more than one error
    /// was collected.
    pub fn into_result(self) -> ConfigResult<()> {
        let mut errors = self.errors;
        if errors.is_empty() {
            Ok(())
        } else if errors.len() == 1 {
            Err(errors.remove(0))
        } else {
            Err(ConfigError::Multiple { errors })
        }
    }
}

/// Checks a boolean validation condition.
///
/// # Errors
///
/// Returns `error` when `condition` is false.
pub fn check(condition: bool, config_error: ConfigError) -> ConfigResult<()> {
    if condition { Ok(()) } else { Err(config_error) }
}

/// Checks that a field is supported.
///
/// # Errors
///
/// Returns [`ConfigError::UnsupportedField`] when `supported` is false.
pub fn check_supported_field(
    field: impl Into<String>,
    supported: bool,
    reason: impl Into<String>,
) -> ConfigResult<()> {
    check(supported, ConfigError::unsupported_field(field, reason))
}

/// Checks that a field value is supported.
///
/// # Errors
///
/// Returns [`ConfigError::UnsupportedValue`] when `supported` is false.
pub fn check_supported_value(
    field: impl Into<String>,
    supported: bool,
    reason: impl Into<String>,
) -> ConfigResult<()> {
    check(supported, ConfigError::unsupported_value(field, reason))
}

/// Checks that a string field is present and non-empty after trimming.
///
/// # Errors
///
/// Returns [`ConfigError::EmptyField`] when `value` is empty after trimming.
pub fn check_non_empty_field(field: impl Into<String>, value: &str) -> ConfigResult<()> {
    check(!value.trim().is_empty(), ConfigError::empty_field(field))
}

/// Checks that a field value is valid.
///
/// # Errors
///
/// Returns [`ConfigError::InvalidValue`] when `valid` is false.
pub fn check_valid_value(
    field: impl Into<String>,
    valid: bool,
    reason: impl Into<String>,
) -> ConfigResult<()> {
    check(valid, ConfigError::invalid_value(field, reason))
}

/// Checks that a field value has the expected format.
///
/// # Errors
///
/// Returns [`ConfigError::InvalidFormat`] when `valid` is false.
pub fn check_valid_format(
    field: impl Into<String>,
    valid: bool,
    expected: impl Into<String>,
) -> ConfigResult<()> {
    check(valid, ConfigError::invalid_format(field, expected))
}

/// Checks that a field value is in range.
///
/// # Errors
///
/// Returns [`ConfigError::Range`] when `in_range` is false.
pub fn check_range(
    field: impl Into<String>,
    in_range: bool,
    reason: impl Into<String>,
) -> ConfigResult<()> {
    check(in_range, ConfigError::range(field, reason))
}

/// Checks that a field's feature is enabled.
///
/// # Errors
///
/// Returns [`ConfigError::FeatureDisabled`] when `enabled` is false.
pub fn check_feature_enabled(
    field: impl Into<String>,
    feature: impl Into<String>,
    enabled: bool,
) -> ConfigResult<()> {
    check(enabled, ConfigError::feature_disabled(field, feature))
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_config_error_display_uses_field_path() {
        let error = ConfigError::invalid_format(
            "LiveNodeConfig.plugins[0].sha256",
            "must be a 64-character hex digest",
        );

        assert_eq!(
            error.to_string(),
            "invalid LiveNodeConfig.plugins[0].sha256: must be a 64-character hex digest",
        );
    }

    #[rstest]
    #[case(
        ConfigError::unsupported_field("field_a", "disabled"),
        "field_a is disabled"
    )]
    #[case(
        ConfigError::unsupported_value("field_a", "mode is disabled"),
        "field_a has unsupported value: mode is disabled"
    )]
    #[case(ConfigError::missing_field("field_a"), "field_a is required")]
    #[case(ConfigError::empty_field("field_a"), "field_a must not be empty")]
    #[case(
        ConfigError::invalid_value("field_a", "must be positive"),
        "invalid field_a: must be positive"
    )]
    #[case(
        ConfigError::invalid_format("field_a", "expected kind/name"),
        "invalid field_a: expected kind/name"
    )]
    #[case(
        ConfigError::range("field_a", "must be <= 10"),
        "invalid field_a: must be <= 10"
    )]
    #[case(
        ConfigError::mutually_exclusive_fields(["field_a", "field_b"]),
        "mutually exclusive fields: field_a, field_b"
    )]
    #[case(
        ConfigError::required_one_of(["field_a", "field_b"]),
        "one of these fields is required: field_a, field_b"
    )]
    #[case(
        ConfigError::dependency("field_a", "field_b", "field_b must be set first"),
        "field_a requires field_b: field_b must be set first"
    )]
    #[case(
        ConfigError::duplicate("field_a", Some("entry_a".to_string())),
        "duplicate field_a: entry_a"
    )]
    #[case(ConfigError::duplicate("field_a", None), "duplicate field_a")]
    #[case(
        ConfigError::feature_disabled("field_a", "live"),
        "field_a requires feature `live`"
    )]
    #[case(
        ConfigError::invalid_reference("field_a", "instrument ID", "not found"),
        "invalid field_a reference instrument ID: not found"
    )]
    fn test_config_error_display_covers_public_vocabulary(
        #[case] error: ConfigError,
        #[case] expected: &str,
    ) {
        assert_eq!(error.to_string(), expected);
    }

    #[rstest]
    fn test_check_non_empty_field_rejects_blank_values() {
        let error = check_non_empty_field("LiveNodeConfig.plugins[0].path", "  ").unwrap_err();

        assert_eq!(
            error,
            ConfigError::EmptyField {
                field: "LiveNodeConfig.plugins[0].path".to_string(),
            },
        );
    }

    #[rstest]
    #[case(
        check_supported_field("field_a", false, "unsupported"),
        ConfigError::unsupported_field("field_a", "unsupported")
    )]
    #[case(
        check_supported_value("field_a", false, "unsupported"),
        ConfigError::unsupported_value("field_a", "unsupported")
    )]
    #[case(
        check_valid_value("field_a", false, "must be positive"),
        ConfigError::invalid_value("field_a", "must be positive")
    )]
    #[case(
        check_valid_format("field_a", false, "expected kind/name"),
        ConfigError::invalid_format("field_a", "expected kind/name")
    )]
    #[case(
        check_range("field_a", false, "must be <= 10"),
        ConfigError::range("field_a", "must be <= 10")
    )]
    #[case(
        check_feature_enabled("field_a", "live", false),
        ConfigError::feature_disabled("field_a", "live")
    )]
    fn test_check_functions_return_expected_errors(
        #[case] result: ConfigResult<()>,
        #[case] expected: ConfigError,
    ) {
        assert_eq!(result.unwrap_err(), expected);
    }

    #[rstest]
    fn test_collector_returns_single_error_without_wrapping() {
        let mut collector = ConfigErrorCollector::new();
        collector.push(ConfigError::empty_field("LiveNodeConfig.plugins[0].path"));

        let error = collector.into_result().unwrap_err();

        assert_eq!(
            error,
            ConfigError::EmptyField {
                field: "LiveNodeConfig.plugins[0].path".to_string(),
            },
        );
    }

    #[rstest]
    fn test_collector_flattens_multiple_errors() {
        let mut collector = ConfigErrorCollector::new();
        collector.push(ConfigError::multiple(vec![
            ConfigError::empty_field("field_a"),
            ConfigError::empty_field("field_b"),
        ]));
        collector.push(ConfigError::empty_field("field_c"));

        let error = collector.into_result().unwrap_err();

        match error {
            ConfigError::Multiple { errors } => assert_eq!(errors.len(), 3),
            _ => panic!("Expected multiple config errors, received {error:?}"),
        }
    }
}
