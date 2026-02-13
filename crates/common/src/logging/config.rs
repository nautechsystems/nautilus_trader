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

//! Logging configuration types and parsing.
//!
//! This module provides configuration for the Nautilus logging subsystem via
//! the `LoggerConfig` and `FileWriterConfig` types.
//!
//! # Spec String Format
//!
//! The `NAUTILUS_LOG` environment variable uses a semicolon-separated format:
//!
//! ```text
//! stdout=Info;fileout=Debug;RiskEngine=Error;my_crate::module=Debug;is_colored
//! ```
//!
//! ## Supported Keys
//!
//! | Key                   | Type      | Description                                  |
//! |-----------------------|-----------|----------------------------------------------|
//! | `stdout`              | Log level | Maximum level for stdout output.             |
//! | `fileout`             | Log level | Maximum level for file output.               |
//! | `is_colored`          | Boolean   | Enable ANSI colors (default: true).          |
//! | `print_config`        | Boolean   | Print config to stdout at startup.           |
//! | `log_components_only` | Boolean   | Only log components with explicit filters.   |
//! | `use_tracing`         | Boolean   | Enable tracing subscriber for external libs. |
//! | `<component>`         | Log level | Component-specific log level (exact match).  |
//! | `<module::path>`      | Log level | Module-specific log level (prefix match).    |
//!
//! ## Log Levels
//!
//! All log levels are case-insensitive.
//!
//! - `Off`
//! - `Error`
//! - `Warn`
//! - `Info`
//! - `Debug`
//! - `Trace`
//!
//! ## Boolean Values
//!
//! - Bare flag: `is_colored` → true
//! - Explicit: `is_colored=true`, `is_colored=false`, `is_colored=0`, `is_colored=no`

use std::{env, str::FromStr};

use ahash::AHashMap;
use log::LevelFilter;
use ustr::Ustr;

/// Configuration for the Nautilus logger.
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.common")
)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoggerConfig {
    /// Maximum log level for stdout output.
    pub stdout_level: LevelFilter,
    /// Maximum log level for file output (`Off` disables file logging).
    pub fileout_level: LevelFilter,
    /// Per-component log level overrides (exact match).
    pub component_level: AHashMap<Ustr, LevelFilter>,
    /// Per-module path log level overrides (prefix match).
    pub module_level: AHashMap<Ustr, LevelFilter>,
    /// Log only components with explicit level filters.
    pub log_components_only: bool,
    /// Use ANSI color codes in output.
    pub is_colored: bool,
    /// Print configuration to stdout at startup.
    pub print_config: bool,
    /// Initialize the tracing subscriber for external Rust crate logs.
    pub use_tracing: bool,
}

impl Default for LoggerConfig {
    /// Creates a new default [`LoggerConfig`] instance.
    fn default() -> Self {
        Self {
            stdout_level: LevelFilter::Info,
            fileout_level: LevelFilter::Off,
            component_level: AHashMap::new(),
            module_level: AHashMap::new(),
            log_components_only: false,
            is_colored: true,
            print_config: false,
            use_tracing: false,
        }
    }
}

impl LoggerConfig {
    /// Creates a new [`LoggerConfig`] instance.
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        stdout_level: LevelFilter,
        fileout_level: LevelFilter,
        component_level: AHashMap<Ustr, LevelFilter>,
        module_level: AHashMap<Ustr, LevelFilter>,
        log_components_only: bool,
        is_colored: bool,
        print_config: bool,
        use_tracing: bool,
    ) -> Self {
        Self {
            stdout_level,
            fileout_level,
            component_level,
            module_level,
            log_components_only,
            is_colored,
            print_config,
            use_tracing,
        }
    }

    /// Parses a configuration from a spec string.
    ///
    /// # Format
    ///
    /// Semicolon-separated key-value pairs or bare flags:
    /// ```text
    /// stdout=Info;fileout=Debug;RiskEngine=Error;my_crate::module=Debug;is_colored
    /// ```
    ///
    /// # Errors
    ///
    /// Returns an error if the spec string contains invalid syntax or log levels.
    pub fn from_spec(spec: &str) -> anyhow::Result<Self> {
        let mut config = Self::default();

        for kv in spec.split(';') {
            let kv = kv.trim();
            if kv.is_empty() {
                continue;
            }

            let kv_lower = kv.to_lowercase();

            // Handle bare flags (without =)
            if !kv.contains('=') {
                match kv_lower.as_str() {
                    "log_components_only" => config.log_components_only = true,
                    "is_colored" => config.is_colored = true,
                    "print_config" => config.print_config = true,
                    "use_tracing" => config.use_tracing = true,
                    _ => anyhow::bail!("Invalid spec pair: {kv}"),
                }
                continue;
            }

            let parts: Vec<&str> = kv.splitn(2, '=').collect();
            if parts.len() != 2 {
                anyhow::bail!("Invalid spec pair: {kv}");
            }

            let k = parts[0].trim();
            let v = parts[1].trim();
            let k_lower = k.to_lowercase();

            match k_lower.as_str() {
                "is_colored" => {
                    config.is_colored = parse_bool_value(v);
                }
                "log_components_only" => {
                    config.log_components_only = parse_bool_value(v);
                }
                "print_config" => {
                    config.print_config = parse_bool_value(v);
                }
                "use_tracing" => {
                    config.use_tracing = parse_bool_value(v);
                }
                "stdout" => {
                    config.stdout_level = parse_level(v)?;
                }
                "fileout" => {
                    config.fileout_level = parse_level(v)?;
                }
                _ => {
                    let lvl = parse_level(v)?;
                    if k.contains("::") {
                        config.module_level.insert(Ustr::from(k), lvl);
                    } else {
                        config.component_level.insert(Ustr::from(k), lvl);
                    }
                }
            }
        }

        Ok(config)
    }

    /// Parses configuration from the `NAUTILUS_LOG` environment variable.
    ///
    /// # Errors
    ///
    /// Returns an error if the variable is unset or contains invalid syntax.
    pub fn from_env() -> anyhow::Result<Self> {
        let spec = env::var("NAUTILUS_LOG")?;
        Self::from_spec(&spec)
    }
}

/// Parses a boolean value from a string.
///
/// Returns `true` unless the value is explicitly "false", "0", or "no" (case-insensitive).
fn parse_bool_value(v: &str) -> bool {
    !matches!(v.to_lowercase().as_str(), "false" | "0" | "no")
}

/// Parses a log level from a string.
fn parse_level(v: &str) -> anyhow::Result<LevelFilter> {
    LevelFilter::from_str(v).map_err(|_| anyhow::anyhow!("Invalid log level: {v}"))
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_default_config() {
        let config = LoggerConfig::default();
        assert_eq!(config.stdout_level, LevelFilter::Info);
        assert_eq!(config.fileout_level, LevelFilter::Off);
        assert!(config.component_level.is_empty());
        assert!(!config.log_components_only);
        assert!(config.is_colored);
        assert!(!config.print_config);
    }

    #[rstest]
    fn test_from_spec_stdout_and_fileout() {
        let config = LoggerConfig::from_spec("stdout=Debug;fileout=Error").unwrap();
        assert_eq!(config.stdout_level, LevelFilter::Debug);
        assert_eq!(config.fileout_level, LevelFilter::Error);
    }

    #[rstest]
    fn test_from_spec_case_insensitive_levels() {
        let config = LoggerConfig::from_spec("stdout=debug;fileout=ERROR").unwrap();
        assert_eq!(config.stdout_level, LevelFilter::Debug);
        assert_eq!(config.fileout_level, LevelFilter::Error);
    }

    #[rstest]
    fn test_from_spec_case_insensitive_keys() {
        let config = LoggerConfig::from_spec("STDOUT=Info;FILEOUT=Debug").unwrap();
        assert_eq!(config.stdout_level, LevelFilter::Info);
        assert_eq!(config.fileout_level, LevelFilter::Debug);
    }

    #[rstest]
    fn test_from_spec_empty_string() {
        let config = LoggerConfig::from_spec("").unwrap();
        assert_eq!(config, LoggerConfig::default());
    }

    #[rstest]
    fn test_from_spec_with_whitespace() {
        let config = LoggerConfig::from_spec("  stdout = Info ; fileout = Debug  ").unwrap();
        assert_eq!(config.stdout_level, LevelFilter::Info);
        assert_eq!(config.fileout_level, LevelFilter::Debug);
    }

    #[rstest]
    fn test_from_spec_trailing_semicolon() {
        let config = LoggerConfig::from_spec("stdout=Warn;").unwrap();
        assert_eq!(config.stdout_level, LevelFilter::Warn);
    }

    #[rstest]
    fn test_from_spec_bare_is_colored() {
        let config = LoggerConfig::from_spec("is_colored").unwrap();
        assert!(config.is_colored);
    }

    #[rstest]
    fn test_from_spec_is_colored_true() {
        let config = LoggerConfig::from_spec("is_colored=true").unwrap();
        assert!(config.is_colored);
    }

    #[rstest]
    fn test_from_spec_is_colored_false() {
        let config = LoggerConfig::from_spec("is_colored=false").unwrap();
        assert!(!config.is_colored);
    }

    #[rstest]
    fn test_from_spec_is_colored_zero() {
        let config = LoggerConfig::from_spec("is_colored=0").unwrap();
        assert!(!config.is_colored);
    }

    #[rstest]
    fn test_from_spec_is_colored_no() {
        let config = LoggerConfig::from_spec("is_colored=no").unwrap();
        assert!(!config.is_colored);
    }

    #[rstest]
    fn test_from_spec_is_colored_case_insensitive() {
        let config = LoggerConfig::from_spec("IS_COLORED=FALSE").unwrap();
        assert!(!config.is_colored);
    }

    #[rstest]
    fn test_from_spec_print_config() {
        let config = LoggerConfig::from_spec("print_config").unwrap();
        assert!(config.print_config);
    }

    #[rstest]
    fn test_from_spec_print_config_false() {
        let config = LoggerConfig::from_spec("print_config=false").unwrap();
        assert!(!config.print_config);
    }

    #[rstest]
    fn test_from_spec_log_components_only() {
        let config = LoggerConfig::from_spec("log_components_only").unwrap();
        assert!(config.log_components_only);
    }

    #[rstest]
    fn test_from_spec_log_components_only_false() {
        let config = LoggerConfig::from_spec("log_components_only=false").unwrap();
        assert!(!config.log_components_only);
    }

    #[rstest]
    fn test_from_spec_component_level() {
        let config = LoggerConfig::from_spec("RiskEngine=Error;DataEngine=Debug").unwrap();
        assert_eq!(
            config.component_level[&Ustr::from("RiskEngine")],
            LevelFilter::Error
        );
        assert_eq!(
            config.component_level[&Ustr::from("DataEngine")],
            LevelFilter::Debug
        );
    }

    #[rstest]
    fn test_from_spec_component_preserves_case() {
        // Component names should preserve their original case
        let config = LoggerConfig::from_spec("MyComponent=Info").unwrap();
        assert!(
            config
                .component_level
                .contains_key(&Ustr::from("MyComponent"))
        );
        assert!(
            !config
                .component_level
                .contains_key(&Ustr::from("mycomponent"))
        );
    }

    #[rstest]
    fn test_from_spec_full_example() {
        let config = LoggerConfig::from_spec(
            "stdout=Info;fileout=Debug;RiskEngine=Error;is_colored;print_config",
        )
        .unwrap();

        assert_eq!(config.stdout_level, LevelFilter::Info);
        assert_eq!(config.fileout_level, LevelFilter::Debug);
        assert_eq!(
            config.component_level[&Ustr::from("RiskEngine")],
            LevelFilter::Error
        );
        assert!(config.is_colored);
        assert!(config.print_config);
    }

    #[rstest]
    fn test_from_spec_disabled_colors() {
        let config = LoggerConfig::from_spec("stdout=Info;is_colored=false;fileout=Debug").unwrap();
        assert!(!config.is_colored);
        assert_eq!(config.stdout_level, LevelFilter::Info);
        assert_eq!(config.fileout_level, LevelFilter::Debug);
    }

    #[rstest]
    fn test_from_spec_invalid_level() {
        let result = LoggerConfig::from_spec("stdout=InvalidLevel");
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Invalid log level")
        );
    }

    #[rstest]
    fn test_from_spec_invalid_bare_flag() {
        let result = LoggerConfig::from_spec("unknown_flag");
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Invalid spec pair")
        );
    }

    #[rstest]
    fn test_from_spec_missing_value() {
        // "stdout=" with no value is technically valid empty string, which is invalid level
        let result = LoggerConfig::from_spec("stdout=");
        assert!(result.is_err());
    }

    #[rstest]
    #[case("Off", LevelFilter::Off)]
    #[case("Error", LevelFilter::Error)]
    #[case("Warn", LevelFilter::Warn)]
    #[case("Info", LevelFilter::Info)]
    #[case("Debug", LevelFilter::Debug)]
    #[case("Trace", LevelFilter::Trace)]
    fn test_all_log_levels(#[case] level_str: &str, #[case] expected: LevelFilter) {
        let config = LoggerConfig::from_spec(&format!("stdout={level_str}")).unwrap();
        assert_eq!(config.stdout_level, expected);
    }

    #[rstest]
    fn test_from_spec_single_module_path() {
        let config = LoggerConfig::from_spec("nautilus_okx::websocket=Debug").unwrap();
        assert_eq!(
            config.module_level[&Ustr::from("nautilus_okx::websocket")],
            LevelFilter::Debug
        );
        assert!(config.component_level.is_empty());
    }

    #[rstest]
    fn test_from_spec_multiple_module_paths() {
        let config =
            LoggerConfig::from_spec("nautilus_okx::websocket=Debug;nautilus_binance::data=Trace")
                .unwrap();
        assert_eq!(
            config.module_level[&Ustr::from("nautilus_okx::websocket")],
            LevelFilter::Debug
        );
        assert_eq!(
            config.module_level[&Ustr::from("nautilus_binance::data")],
            LevelFilter::Trace
        );
        assert!(config.component_level.is_empty());
    }

    #[rstest]
    fn test_from_spec_mixed_module_and_component() {
        let config = LoggerConfig::from_spec(
            "nautilus_okx::websocket=Debug;RiskEngine=Error;nautilus_network::data=Trace",
        )
        .unwrap();

        assert_eq!(
            config.module_level[&Ustr::from("nautilus_okx::websocket")],
            LevelFilter::Debug
        );
        assert_eq!(
            config.module_level[&Ustr::from("nautilus_network::data")],
            LevelFilter::Trace
        );
        assert_eq!(config.module_level.len(), 2);
        assert_eq!(
            config.component_level[&Ustr::from("RiskEngine")],
            LevelFilter::Error
        );
        assert_eq!(config.component_level.len(), 1);
    }

    #[rstest]
    fn test_from_spec_deeply_nested_module_path() {
        let config =
            LoggerConfig::from_spec("nautilus_okx::websocket::handler::auth=Trace").unwrap();
        assert_eq!(
            config.module_level[&Ustr::from("nautilus_okx::websocket::handler::auth")],
            LevelFilter::Trace
        );
    }

    #[rstest]
    fn test_from_spec_module_path_with_underscores() {
        let config =
            LoggerConfig::from_spec("nautilus_trader::adapters::interactive_brokers=Debug")
                .unwrap();
        assert_eq!(
            config.module_level[&Ustr::from("nautilus_trader::adapters::interactive_brokers")],
            LevelFilter::Debug
        );
    }

    #[rstest]
    fn test_from_spec_full_example_with_modules() {
        let config = LoggerConfig::from_spec(
            "stdout=Info;fileout=Debug;RiskEngine=Error;nautilus_okx::websocket=Trace;is_colored",
        )
        .unwrap();

        assert_eq!(config.stdout_level, LevelFilter::Info);
        assert_eq!(config.fileout_level, LevelFilter::Debug);
        assert_eq!(
            config.component_level[&Ustr::from("RiskEngine")],
            LevelFilter::Error
        );
        assert_eq!(
            config.module_level[&Ustr::from("nautilus_okx::websocket")],
            LevelFilter::Trace
        );
        assert!(config.is_colored);
    }

    #[rstest]
    fn test_from_spec_module_path_preserves_case() {
        let config = LoggerConfig::from_spec("MyModule::SubModule=Info").unwrap();
        assert!(
            config
                .module_level
                .contains_key(&Ustr::from("MyModule::SubModule"))
        );
    }

    #[rstest]
    fn test_from_spec_single_colon_is_component() {
        // Single colon is NOT a module path separator in Rust
        let config = LoggerConfig::from_spec("Component:Name=Info").unwrap();
        assert!(config.module_level.is_empty());
        assert!(
            config
                .component_level
                .contains_key(&Ustr::from("Component:Name"))
        );
    }

    #[rstest]
    fn test_default_module_level_is_empty() {
        let config = LoggerConfig::default();
        assert!(config.module_level.is_empty());
    }
}
