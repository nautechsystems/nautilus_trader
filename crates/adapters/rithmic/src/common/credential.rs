//! Credential handling for Rithmic authentication.

use crate::error::{Result, RithmicError};

/// Rithmic API credentials.
#[derive(Debug, Clone)]
pub struct RithmicCredentials {
    /// Rithmic username.
    pub username: String,
    /// Rithmic password.
    pub password: String,
    /// System name.
    pub system_name: String,
    /// Application name.
    pub app_name: String,
    /// Application version.
    pub app_version: String,
    /// FCM ID (optional).
    pub fcm_id: Option<String>,
    /// IB ID (optional).
    pub ib_id: Option<String>,
}

impl RithmicCredentials {
    /// Creates new credentials.
    pub fn new(
        username: impl Into<String>,
        password: impl Into<String>,
        system_name: impl Into<String>,
    ) -> Self {
        Self {
            username: username.into(),
            password: password.into(),
            system_name: system_name.into(),
            app_name: "NautilusTrader".to_string(),
            app_version: "1.0".to_string(),
            fcm_id: None,
            ib_id: None,
        }
    }

    /// Loads credentials from environment variables.
    ///
    /// Required variables:
    /// - `RITHMIC_USERNAME`
    /// - `RITHMIC_PASSWORD`
    /// - `RITHMIC_SYSTEM_NAME`
    ///
    /// Optional variables:
    /// - `RITHMIC_APP_NAME`
    /// - `RITHMIC_APP_VERSION`
    /// - `RITHMIC_FCM_ID`
    /// - `RITHMIC_IB_ID`
    pub fn from_env() -> Result<Self> {
        let username = std::env::var("RITHMIC_USERNAME")
            .map_err(|_| RithmicError::Config("RITHMIC_USERNAME not set".to_string()))?;

        let password = std::env::var("RITHMIC_PASSWORD")
            .map_err(|_| RithmicError::Config("RITHMIC_PASSWORD not set".to_string()))?;

        let system_name = std::env::var("RITHMIC_SYSTEM_NAME")
            .map_err(|_| RithmicError::Config("RITHMIC_SYSTEM_NAME not set".to_string()))?;

        Ok(Self {
            username,
            password,
            system_name,
            app_name: std::env::var("RITHMIC_APP_NAME")
                .unwrap_or_else(|_| "NautilusTrader".to_string()),
            app_version: std::env::var("RITHMIC_APP_VERSION").unwrap_or_else(|_| "1.0".to_string()),
            fcm_id: std::env::var("RITHMIC_FCM_ID").ok(),
            ib_id: std::env::var("RITHMIC_IB_ID").ok(),
        })
    }

    /// Validates that all required credentials are present and non-empty.
    pub fn validate(&self) -> Result<()> {
        if self.username.is_empty() {
            return Err(RithmicError::Config("Username cannot be empty".to_string()));
        }
        if self.password.is_empty() {
            return Err(RithmicError::Config("Password cannot be empty".to_string()));
        }
        if self.system_name.is_empty() {
            return Err(RithmicError::Config(
                "System name cannot be empty".to_string(),
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_credentials_validation() {
        let creds = RithmicCredentials::new("user", "pass", "system");
        assert!(creds.validate().is_ok());

        let empty_user = RithmicCredentials::new("", "pass", "system");
        assert!(empty_user.validate().is_err());
    }
}
