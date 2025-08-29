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
}
