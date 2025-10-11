// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

//! Cross-platform environment variable utilities.
//!
//! This module provides functions for safely accessing environment variables
//! with proper error handling.

/// Returns the value of the environment variable for the given `key`.
///
/// # Errors
///
/// Returns an error if the environment variable is not set.
pub fn get_env_var(key: &str) -> anyhow::Result<String> {
    match std::env::var(key) {
        Ok(var) => Ok(var),
        Err(_) => anyhow::bail!("environment variable '{key}' must be set"),
    }
}

/// Returns the provided `value` if `Some`, otherwise falls back to reading
/// the environment variable for the given `key`.
///
/// Only attempts to read the environment variable when `value` is `None`,
/// avoiding unnecessary environment variable lookups and errors.
///
/// # Errors
///
/// Returns an error if `value` is `None` and the environment variable is not set.
pub fn get_or_env_var(value: Option<String>, key: &str) -> anyhow::Result<String> {
    match value {
        Some(v) => Ok(v),
        None => get_env_var(key),
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use rstest::*;

    use super::*;

    #[rstest]
    fn test_get_env_var_success() {
        // Test with a commonly available environment variable
        if let Ok(path) = std::env::var("PATH") {
            let result = get_env_var("PATH");
            assert!(result.is_ok());
            assert_eq!(result.unwrap(), path);
        }
    }

    #[rstest]
    fn test_get_env_var_not_set() {
        // Use a highly unlikely environment variable name
        let result = get_env_var("NONEXISTENT_ENV_VAR_THAT_SHOULD_NOT_EXIST_12345");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains(
            "environment variable 'NONEXISTENT_ENV_VAR_THAT_SHOULD_NOT_EXIST_12345' must be set"
        ));
    }

    #[rstest]
    fn test_get_env_var_error_message_format() {
        let var_name = "DEFINITELY_NONEXISTENT_VAR_123456789";
        let result = get_env_var(var_name);
        assert!(result.is_err());
        let error_msg = result.unwrap_err().to_string();
        assert!(error_msg.contains(var_name));
        assert!(error_msg.contains("must be set"));
    }

    #[rstest]
    fn test_get_or_env_var_with_some_value() {
        let provided_value = Some("provided_value".to_string());
        let result = get_or_env_var(provided_value, "PATH");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "provided_value");
    }

    #[rstest]
    fn test_get_or_env_var_with_none_and_env_var_set() {
        // Test with a commonly available environment variable
        if let Ok(path) = std::env::var("PATH") {
            let result = get_or_env_var(None, "PATH");
            assert!(result.is_ok());
            assert_eq!(result.unwrap(), path);
        }
    }

    #[rstest]
    fn test_get_or_env_var_with_none_and_env_var_not_set() {
        let result = get_or_env_var(None, "NONEXISTENT_ENV_VAR_THAT_SHOULD_NOT_EXIST_67890");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains(
            "environment variable 'NONEXISTENT_ENV_VAR_THAT_SHOULD_NOT_EXIST_67890' must be set"
        ));
    }

    #[rstest]
    fn test_get_or_env_var_empty_string_value() {
        // Empty string is still a valid value that should be returned
        let provided_value = Some(String::new());
        let result = get_or_env_var(provided_value, "PATH");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "");
    }

    #[rstest]
    fn test_get_or_env_var_priority() {
        // When both value and env var are available, value takes precedence
        // Using PATH as it should be available in most environments
        if std::env::var("PATH").is_ok() {
            let provided = Some("custom_value_takes_priority".to_string());
            let result = get_or_env_var(provided, "PATH");
            assert!(result.is_ok());
            assert_eq!(result.unwrap(), "custom_value_takes_priority");
        }
    }
}
