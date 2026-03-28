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

//! Configuration types for the Rithmic adapter.
//!
//! Configuration can be loaded from environment variables or constructed programmatically.
//!
//! # Environment Variables
//!
//! - `RITHMIC_USERNAME`: Rithmic account username
//! - `RITHMIC_PASSWORD`: Rithmic account password
//! - `RITHMIC_SYSTEM_NAME`: System name for connection
//! - `RITHMIC_APP_NAME`: Application name
//! - `RITHMIC_APP_VERSION`: Application version
//! - `RITHMIC_FCM_ID`: FCM ID (optional)
//! - `RITHMIC_IB_ID`: IB ID (optional)
//! - `RITHMIC_ACCOUNT_ID`: Trading account ID (for execution)
//! - `RITHMIC_ENV`: Environment (demo, live, test)
//! - `RITHMIC_SERVER`: Named primary server (defaults to Chicago on demo/live, Test on test)
//! - `RITHMIC_ALT_SERVER`: Named alternate server (optional)

use std::env;

pub use rithmic_rs::RithmicEnv;
use serde::{Deserialize, Serialize};

use crate::error::{Result, RithmicError};

const DEFAULT_APP_NAME: &str = "fufo:fund-forge";
const DEFAULT_APP_VERSION: &str = "1.0";

/// Deprecated: Use [`RithmicEnv`] instead.
///
/// This type alias is provided for backwards compatibility and will be removed
/// in a future major version.
#[deprecated(since = "0.2.0", note = "Use RithmicEnv instead")]
pub type RithmicEnvironment = RithmicEnv;

/// Parses a `RithmicEnv` from a string with flexible aliases.
///
/// Accepts:
/// - "demo" or "paper" → `RithmicEnv::Demo`
/// - "live", "prod", or "production" → `RithmicEnv::Live`
/// - "test" → `RithmicEnv::Test`
pub fn parse_rithmic_env(s: &str) -> Result<RithmicEnv> {
    match s.to_lowercase().as_str() {
        "demo" | "paper" => Ok(RithmicEnv::Demo),
        "live" | "prod" | "production" => Ok(RithmicEnv::Live),
        "test" => Ok(RithmicEnv::Test),
        _ => Err(RithmicError::Config(format!(
            "Invalid environment: {s}. Expected: demo, live, or test"
        ))),
    }
}

fn normalize_env_profile(profile: &str) -> Result<String> {
    let normalized: String = profile
        .trim()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_uppercase()
            } else {
                '_'
            }
        })
        .collect();

    let normalized = normalized
        .split('_')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("_");

    if normalized.is_empty() {
        return Err(RithmicError::Config(
            "Rithmic env profile cannot be empty".to_string(),
        ));
    }

    Ok(normalized)
}

fn env_candidates(key: &str, profile: Option<&str>) -> Result<Vec<String>> {
    let mut candidates = Vec::new();

    if let Some(profile) = profile {
        candidates.push(format!(
            "RITHMIC_{}_{}",
            normalize_env_profile(profile)?,
            key
        ));
    }
    candidates.push(format!("RITHMIC_{key}"));
    Ok(candidates)
}

pub(crate) fn optional_env_var(key: &str, profile: Option<&str>) -> Result<Option<String>> {
    for candidate in env_candidates(key, profile)? {
        if let Ok(value) = env::var(&candidate)
            && !value.is_empty()
        {
            return Ok(Some(value));
        }
    }

    Ok(None)
}

pub(crate) fn required_env_var(key: &str, profile: Option<&str>) -> Result<String> {
    if let Some(value) = optional_env_var(key, profile)? {
        return Ok(value);
    }

    let missing = env_candidates(key, profile)?
        .into_iter()
        .next()
        .unwrap_or_else(|| format!("RITHMIC_{key}"));
    Err(RithmicError::Config(format!("{missing} not set")))
}

/// Configuration for the Rithmic data client.
#[must_use]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RithmicDataClientConfig {
    /// Rithmic environment (Demo, Live, Test).
    pub environment: RithmicEnv,
    /// Rithmic username.
    pub username: String,
    /// Rithmic password.
    pub password: String,
    /// System name for Rithmic connection.
    pub system_name: String,
    /// Application name.
    pub app_name: String,
    /// Application version.
    pub app_version: String,
    /// FCM ID (Futures Commission Merchant).
    pub fcm_id: Option<String>,
    /// IB ID (Introducing Broker).
    pub ib_id: Option<String>,
    /// Named primary server override.
    pub server: Option<String>,
    /// Named alternate server override.
    pub alt_server: Option<String>,
}

impl RithmicDataClientConfig {
    /// Creates a new data client configuration.
    pub fn new(
        environment: RithmicEnv,
        username: impl Into<String>,
        password: impl Into<String>,
        system_name: impl Into<String>,
    ) -> Self {
        Self {
            environment,
            username: username.into(),
            password: password.into(),
            system_name: system_name.into(),
            app_name: DEFAULT_APP_NAME.to_string(),
            app_version: DEFAULT_APP_VERSION.to_string(),
            fcm_id: None,
            ib_id: None,
            server: None,
            alt_server: None,
        }
    }

    /// Creates configuration from environment variables.
    pub fn from_env() -> Result<Self> {
        Self::from_env_with_profile(None)
    }

    /// Creates configuration from environment variables, optionally scoped by profile.
    pub fn from_env_with_profile(profile: Option<&str>) -> Result<Self> {
        let environment = optional_env_var("ENV", profile)?
            .map_or(Ok(RithmicEnv::Demo), |s| parse_rithmic_env(&s))?;

        Ok(Self {
            environment,
            username: required_env_var("USERNAME", profile)?,
            password: required_env_var("PASSWORD", profile)?,
            system_name: required_env_var("SYSTEM_NAME", profile)?,
            app_name: optional_env_var("APP_NAME", profile)?
                .unwrap_or_else(|| DEFAULT_APP_NAME.to_string()),
            app_version: optional_env_var("APP_VERSION", profile)?
                .unwrap_or_else(|| DEFAULT_APP_VERSION.to_string()),
            fcm_id: optional_env_var("FCM_ID", profile)?,
            ib_id: optional_env_var("IB_ID", profile)?,
            server: optional_env_var("SERVER", profile)?,
            alt_server: optional_env_var("ALT_SERVER", profile)?,
        })
    }

    /// Sets the application name.
    pub fn with_app_name(mut self, app_name: impl Into<String>) -> Self {
        self.app_name = app_name.into();
        self
    }

    /// Sets the application version.
    pub fn with_app_version(mut self, app_version: impl Into<String>) -> Self {
        self.app_version = app_version.into();
        self
    }

    /// Sets the FCM ID.
    pub fn with_fcm_id(mut self, fcm_id: impl Into<String>) -> Self {
        self.fcm_id = Some(fcm_id.into());
        self
    }

    /// Sets the IB ID.
    pub fn with_ib_id(mut self, ib_id: impl Into<String>) -> Self {
        self.ib_id = Some(ib_id.into());
        self
    }
}

/// Configuration for the Rithmic execution client.
#[must_use]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RithmicExecClientConfig {
    /// Rithmic environment (Demo, Live, Test).
    pub environment: RithmicEnv,
    /// Rithmic username.
    pub username: String,
    /// Rithmic password.
    pub password: String,
    /// System name for Rithmic connection.
    pub system_name: String,
    /// Application name.
    pub app_name: String,
    /// Application version.
    pub app_version: String,
    /// FCM ID (Futures Commission Merchant).
    pub fcm_id: Option<String>,
    /// IB ID (Introducing Broker).
    pub ib_id: Option<String>,
    /// Trading account ID.
    pub account_id: String,
    /// Named primary server override.
    pub server: Option<String>,
    /// Named alternate server override.
    pub alt_server: Option<String>,
}

impl RithmicExecClientConfig {
    /// Creates a new execution client configuration.
    pub fn new(
        environment: RithmicEnv,
        username: impl Into<String>,
        password: impl Into<String>,
        system_name: impl Into<String>,
        account_id: impl Into<String>,
    ) -> Self {
        Self {
            environment,
            username: username.into(),
            password: password.into(),
            system_name: system_name.into(),
            app_name: DEFAULT_APP_NAME.to_string(),
            app_version: DEFAULT_APP_VERSION.to_string(),
            fcm_id: None,
            ib_id: None,
            account_id: account_id.into(),
            server: None,
            alt_server: None,
        }
    }

    /// Creates configuration from environment variables.
    pub fn from_env() -> Result<Self> {
        Self::from_env_with_profile(None)
    }

    /// Creates configuration from environment variables, optionally scoped by profile.
    pub fn from_env_with_profile(profile: Option<&str>) -> Result<Self> {
        let environment = optional_env_var("ENV", profile)?
            .map_or(Ok(RithmicEnv::Demo), |s| parse_rithmic_env(&s))?;

        Ok(Self {
            environment,
            username: required_env_var("USERNAME", profile)?,
            password: required_env_var("PASSWORD", profile)?,
            system_name: required_env_var("SYSTEM_NAME", profile)?,
            app_name: optional_env_var("APP_NAME", profile)?
                .unwrap_or_else(|| DEFAULT_APP_NAME.to_string()),
            app_version: optional_env_var("APP_VERSION", profile)?
                .unwrap_or_else(|| DEFAULT_APP_VERSION.to_string()),
            fcm_id: optional_env_var("FCM_ID", profile)?,
            ib_id: optional_env_var("IB_ID", profile)?,
            account_id: required_env_var("ACCOUNT_ID", profile)?,
            server: optional_env_var("SERVER", profile)?,
            alt_server: optional_env_var("ALT_SERVER", profile)?,
        })
    }

    /// Sets the application name.
    pub fn with_app_name(mut self, app_name: impl Into<String>) -> Self {
        self.app_name = app_name.into();
        self
    }

    /// Sets the application version.
    pub fn with_app_version(mut self, app_version: impl Into<String>) -> Self {
        self.app_version = app_version.into();
        self
    }

    /// Sets the FCM ID.
    pub fn with_fcm_id(mut self, fcm_id: impl Into<String>) -> Self {
        self.fcm_id = Some(fcm_id.into());
        self
    }

    /// Sets the IB ID.
    pub fn with_ib_id(mut self, ib_id: impl Into<String>) -> Self {
        self.ib_id = Some(ib_id.into());
        self
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{LazyLock, Mutex};

    use super::*;

    static ENV_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

    fn set_env(key: &str, value: Option<&str>) -> Option<String> {
        let previous = std::env::var(key).ok();
        match value {
            Some(value) => unsafe { std::env::set_var(key, value) },
            None => unsafe { std::env::remove_var(key) },
        }
        previous
    }

    fn restore_env(entries: &[(&str, Option<String>)]) {
        for (key, value) in entries {
            match value {
                Some(value) => unsafe { std::env::set_var(key, value) },
                None => unsafe { std::env::remove_var(key) },
            }
        }
    }

    #[rstest::rstest]
    fn test_parse_rithmic_env() {
        assert_eq!(parse_rithmic_env("demo").unwrap(), RithmicEnv::Demo);
        assert_eq!(parse_rithmic_env("live").unwrap(), RithmicEnv::Live);
        assert_eq!(parse_rithmic_env("test").unwrap(), RithmicEnv::Test);
        assert!(parse_rithmic_env("invalid").is_err());
    }

    #[rstest::rstest]
    fn test_data_client_config_builder() {
        let config = RithmicDataClientConfig::new(RithmicEnv::Demo, "user", "pass", "system")
            .with_app_name("TestApp")
            .with_fcm_id("FCM001");

        assert_eq!(config.app_name, "TestApp");
        assert_eq!(config.fcm_id, Some("FCM001".to_string()));
    }

    #[rstest::rstest]
    fn test_data_client_config_from_profile_env() {
        let _guard = ENV_LOCK.lock().unwrap();
        let previous = [
            (
                "RITHMIC_APEX_ENV",
                set_env("RITHMIC_APEX_ENV", Some("live")),
            ),
            (
                "RITHMIC_APEX_USERNAME",
                set_env("RITHMIC_APEX_USERNAME", Some("user")),
            ),
            (
                "RITHMIC_APEX_PASSWORD",
                set_env("RITHMIC_APEX_PASSWORD", Some("pass")),
            ),
            (
                "RITHMIC_APEX_SYSTEM_NAME",
                set_env("RITHMIC_APEX_SYSTEM_NAME", Some("Apex")),
            ),
            (
                "RITHMIC_APEX_APP_NAME",
                set_env("RITHMIC_APEX_APP_NAME", Some("MyApp")),
            ),
            (
                "RITHMIC_APEX_FCM_ID",
                set_env("RITHMIC_APEX_FCM_ID", Some("fcm")),
            ),
            (
                "RITHMIC_USERNAME",
                set_env("RITHMIC_USERNAME", Some("legacy-user")),
            ),
        ];

        let config = RithmicDataClientConfig::from_env_with_profile(Some("Apex")).unwrap();

        assert_eq!(config.environment, RithmicEnv::Live);
        assert_eq!(config.username, "user");
        assert_eq!(config.password, "pass");
        assert_eq!(config.system_name, "Apex");
        assert_eq!(config.app_name, "MyApp");
        assert_eq!(config.fcm_id.as_deref(), Some("fcm"));

        restore_env(&previous);
    }

    #[rstest::rstest]
    fn test_exec_client_config_profile_falls_back_to_canonical_env() {
        let _guard = ENV_LOCK.lock().unwrap();
        let previous = [
            ("RITHMIC_ENV", set_env("RITHMIC_ENV", Some("demo"))),
            (
                "RITHMIC_USERNAME",
                set_env("RITHMIC_USERNAME", Some("user")),
            ),
            (
                "RITHMIC_PASSWORD",
                set_env("RITHMIC_PASSWORD", Some("pass")),
            ),
            (
                "RITHMIC_SYSTEM_NAME",
                set_env("RITHMIC_SYSTEM_NAME", Some("system")),
            ),
            (
                "RITHMIC_ACCOUNT_ID",
                set_env("RITHMIC_ACCOUNT_ID", Some("account")),
            ),
            ("RITHMIC_EMPTY_ENV", set_env("RITHMIC_EMPTY_ENV", None)),
        ];

        let config = RithmicExecClientConfig::from_env_with_profile(Some("empty")).unwrap();

        assert_eq!(config.environment, RithmicEnv::Demo);
        assert_eq!(config.username, "user");
        assert_eq!(config.account_id, "account");

        restore_env(&previous);
    }
}
