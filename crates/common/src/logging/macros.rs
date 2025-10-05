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

//! Colored logging macros for enhanced log output with automatic color mapping.

/// Logs a trace message with automatic color mapping or custom color and component.
///
/// # Usage
/// ```rust
/// // Automatic color (normal)
/// log_trace!("Processing tick data");
///
/// // Custom color
/// log_trace!("Processing tick data", color = LogColor::Cyan);
///
/// // Custom component
/// log_trace!("Processing data", component = "DataEngine");
///
/// // Both color and component (flexible order)
/// log_trace!("Data processed", color = LogColor::Cyan, component = "DataEngine");
/// log_trace!("Data processed", component = "DataEngine", color = LogColor::Cyan);
/// ```
#[macro_export]
macro_rules! log_trace {
    // Component only
    ($msg:literal, component = $component:expr) => {
        log::trace!(component = $component; $msg);
    };
    ($fmt:literal, $($args:expr),+, component = $component:expr) => {
        log::trace!(component = $component; $fmt, $($args),+);
    };

    // Color only
    ($msg:literal, color = $color:expr) => {
        log::trace!(color = $color as u8; $msg);
    };
    ($fmt:literal, $($args:expr),+, color = $color:expr) => {
        log::trace!(color = $color as u8; $fmt, $($args),+);
    };

    // Both color and component (color first)
    ($msg:literal, color = $color:expr, component = $component:expr) => {
        log::trace!(component = $component, color = $color as u8; $msg);
    };
    ($fmt:literal, $($args:expr),+, color = $color:expr, component = $component:expr) => {
        log::trace!(component = $component, color = $color as u8; $fmt, $($args),+);
    };

    // Both color and component (component first)
    ($msg:literal, component = $component:expr, color = $color:expr) => {
        log::trace!(component = $component, color = $color as u8; $msg);
    };
    ($fmt:literal, $($args:expr),+, component = $component:expr, color = $color:expr) => {
        log::trace!(component = $component, color = $color as u8; $fmt, $($args),+);
    };

    // Default (no color or component)
    ($msg:literal) => {
        log::trace!(color = $crate::enums::LogColor::Normal as u8; $msg);
    };
    ($fmt:literal, $($args:expr),+) => {
        log::trace!(color = $crate::enums::LogColor::Normal as u8; $fmt, $($args),+);
    };
}

/// Logs a debug message with automatic color mapping or custom color and component.
///
/// # Usage
/// ```rust
/// // Automatic color (normal)
/// log_debug!("Validating order: {}", order_id);
///
/// // Custom color
/// log_debug!("Validating order: {}", order_id, color = LogColor::Blue);
///
/// // Custom component
/// log_debug!("Validating order", component = "RiskEngine");
///
/// // Both color and component (flexible order)
/// log_debug!("Order validated", color = LogColor::Blue, component = "RiskEngine");
/// log_debug!("Order validated", component = "RiskEngine", color = LogColor::Blue);
/// ```
#[macro_export]
macro_rules! log_debug {
    // Component only
    ($msg:literal, component = $component:expr) => {
        log::debug!(component = $component; $msg);
    };
    ($fmt:literal, $($args:expr),+, component = $component:expr) => {
        log::debug!(component = $component; $fmt, $($args),+);
    };

    // Color only
    ($msg:literal, color = $color:expr) => {
        log::debug!(color = $color as u8; $msg);
    };
    ($fmt:literal, $($args:expr),+, color = $color:expr) => {
        log::debug!(color = $color as u8; $fmt, $($args),+);
    };

    // Both color and component (color first)
    ($msg:literal, color = $color:expr, component = $component:expr) => {
        log::debug!(component = $component, color = $color as u8; $msg);
    };
    ($fmt:literal, $($args:expr),+, color = $color:expr, component = $component:expr) => {
        log::debug!(component = $component, color = $color as u8; $fmt, $($args),+);
    };

    // Both color and component (component first)
    ($msg:literal, component = $component:expr, color = $color:expr) => {
        log::debug!(component = $component, color = $color as u8; $msg);
    };
    ($fmt:literal, $($args:expr),+, component = $component:expr, color = $color:expr) => {
        log::debug!(component = $component, color = $color as u8; $fmt, $($args),+);
    };

    // Default (no color or component)
    ($msg:literal) => {
        log::debug!(color = $crate::enums::LogColor::Normal as u8; $msg);
    };
    ($fmt:literal, $($args:expr),+) => {
        log::debug!(color = $crate::enums::LogColor::Normal as u8; $fmt, $($args),+);
    };
}

/// Logs an info message with automatic color mapping or custom color and component.
///
/// # Usage
/// ```rust
/// // Automatic color (normal)
/// log_info!("Order {} filled successfully", order_id);
///
/// // Custom color (e.g., green for success)
/// log_info!("Order {} filled successfully", order_id, color = LogColor::Green);
///
/// // Custom component
/// log_info!("Processing order", component = "OrderManager");
///
/// // Both color and component (flexible order)
/// log_info!("Order filled", color = LogColor::Green, component = "OrderManager");
/// log_info!("Order filled", component = "OrderManager", color = LogColor::Green);
/// ```
#[macro_export]
macro_rules! log_info {
    // Both color and component (color first)
    ($msg:literal, color = $color:expr, component = $component:expr) => {
        log::info!(component = $component, color = $color as u8; $msg);
    };
    ($fmt:literal, $arg1:expr, color = $color:expr, component = $component:expr) => {
        log::info!(component = $component, color = $color as u8; $fmt, $arg1);
    };
    ($fmt:literal, $arg1:expr, $arg2:expr, color = $color:expr, component = $component:expr) => {
        log::info!(component = $component, color = $color as u8; $fmt, $arg1, $arg2);
    };

    // Both color and component (component first)
    ($msg:literal, component = $component:expr, color = $color:expr) => {
        log::info!(component = $component, color = $color as u8; $msg);
    };
    ($fmt:literal, $arg1:expr, component = $component:expr, color = $color:expr) => {
        log::info!(component = $component, color = $color as u8; $fmt, $arg1);
    };
    ($fmt:literal, $arg1:expr, $arg2:expr, component = $component:expr, color = $color:expr) => {
        log::info!(component = $component, color = $color as u8; $fmt, $arg1, $arg2);
    };

    // Component only
    ($msg:literal, component = $component:expr) => {
        log::info!(component = $component; $msg);
    };
    ($fmt:literal, $arg1:expr, component = $component:expr) => {
        log::info!(component = $component; $fmt, $arg1);
    };
    ($fmt:literal, $arg1:expr, $arg2:expr, component = $component:expr) => {
        log::info!(component = $component; $fmt, $arg1, $arg2);
    };

    // Color only
    ($msg:literal, color = $color:expr) => {
        log::info!(color = $color as u8; $msg);
    };
    ($fmt:literal, $arg1:expr, color = $color:expr) => {
        log::info!(color = $color as u8; $fmt, $arg1);
    };
    ($fmt:literal, $arg1:expr, $arg2:expr, color = $color:expr) => {
        log::info!(color = $color as u8; $fmt, $arg1, $arg2);
    };
    ($fmt:literal, $arg1:expr, $arg2:expr, $arg3:expr, color = $color:expr) => {
        log::info!(color = $color as u8; $fmt, $arg1, $arg2, $arg3);
    };

    // Default (no color or component)
    ($msg:literal) => {
        log::info!(color = $crate::enums::LogColor::Normal as u8; $msg);
    };
    ($fmt:literal, $($args:expr),+) => {
        log::info!(color = $crate::enums::LogColor::Normal as u8; $fmt, $($args),+);
    };
}

/// Logs a warning message with automatic yellow color or custom color and component.
///
/// # Usage
/// ```rust
/// // Automatic color (yellow)
/// log_warn!("Position size approaching limit");
///
/// // Custom color
/// log_warn!("Custom warning message", color = LogColor::Magenta);
///
/// // Custom component
/// log_warn!("Risk limit exceeded", component = "RiskEngine");
///
/// // Both color and component (flexible order)
/// log_warn!("Warning message", color = LogColor::Magenta, component = "RiskEngine");
/// log_warn!("Warning message", component = "RiskEngine", color = LogColor::Magenta);
/// ```
#[macro_export]
macro_rules! log_warn {
    // Both color and component (color first)
    ($msg:literal, color = $color:expr, component = $component:expr) => {
        log::warn!(component = $component, color = $color as u8; $msg);
    };
    ($fmt:literal, $arg1:expr, color = $color:expr, component = $component:expr) => {
        log::warn!(component = $component, color = $color as u8; $fmt, $arg1);
    };
    ($fmt:literal, $arg1:expr, $arg2:expr, color = $color:expr, component = $component:expr) => {
        log::warn!(component = $component, color = $color as u8; $fmt, $arg1, $arg2);
    };

    // Both color and component (component first)
    ($msg:literal, component = $component:expr, color = $color:expr) => {
        log::warn!(component = $component, color = $color as u8; $msg);
    };
    ($fmt:literal, $arg1:expr, component = $component:expr, color = $color:expr) => {
        log::warn!(component = $component, color = $color as u8; $fmt, $arg1);
    };
    ($fmt:literal, $arg1:expr, $arg2:expr, component = $component:expr, color = $color:expr) => {
        log::warn!(component = $component, color = $color as u8; $fmt, $arg1, $arg2);
    };

    // Component only
    ($msg:literal, component = $component:expr) => {
        log::warn!(component = $component, color = $crate::enums::LogColor::Yellow as u8; $msg);
    };
    ($fmt:literal, $arg1:expr, component = $component:expr) => {
        log::warn!(component = $component, color = $crate::enums::LogColor::Yellow as u8; $fmt, $arg1);
    };
    ($fmt:literal, $arg1:expr, $arg2:expr, component = $component:expr) => {
        log::warn!(component = $component, color = $crate::enums::LogColor::Yellow as u8; $fmt, $arg1, $arg2);
    };

    // Color only
    ($msg:literal, color = $color:expr) => {
        log::warn!(color = $color as u8; $msg);
    };
    ($fmt:literal, $arg1:expr, color = $color:expr) => {
        log::warn!(color = $color as u8; $fmt, $arg1);
    };
    ($fmt:literal, $arg1:expr, $arg2:expr, color = $color:expr) => {
        log::warn!(color = $color as u8; $fmt, $arg1, $arg2);
    };
    ($fmt:literal, $arg1:expr, $arg2:expr, $arg3:expr, color = $color:expr) => {
        log::warn!(color = $color as u8; $fmt, $arg1, $arg2, $arg3);
    };

    // Default (automatic yellow color, no component)
    ($msg:literal) => {
        log::warn!(color = $crate::enums::LogColor::Yellow as u8; $msg);
    };
    ($fmt:literal, $($args:expr),+) => {
        log::warn!(color = $crate::enums::LogColor::Yellow as u8; $fmt, $($args),+);
    };
}

/// Logs an error message with automatic red color or custom color and component.
///
/// # Usage
/// ```rust
/// // Automatic color (red)
/// log_error!("Failed to connect to exchange: {}", error);
///
/// // Custom color
/// log_error!("Custom error message", color = LogColor::Magenta);
///
/// // Custom component
/// log_error!("Connection failed", component = "DataEngine");
///
/// // Both color and component (flexible order)
/// log_error!("Critical error", color = LogColor::Magenta, component = "DataEngine");
/// log_error!("Critical error", component = "DataEngine", color = LogColor::Magenta);
/// ```
#[macro_export]
macro_rules! log_error {
    // Both color and component (color first)
    ($msg:literal, color = $color:expr, component = $component:expr) => {
        log::error!(component = $component, color = $color as u8; $msg);
    };
    ($fmt:literal, $arg1:expr, color = $color:expr, component = $component:expr) => {
        log::error!(component = $component, color = $color as u8; $fmt, $arg1);
    };
    ($fmt:literal, $arg1:expr, $arg2:expr, color = $color:expr, component = $component:expr) => {
        log::error!(component = $component, color = $color as u8; $fmt, $arg1, $arg2);
    };

    // Both color and component (component first)
    ($msg:literal, component = $component:expr, color = $color:expr) => {
        log::error!(component = $component, color = $color as u8; $msg);
    };
    ($fmt:literal, $arg1:expr, component = $component:expr, color = $color:expr) => {
        log::error!(component = $component, color = $color as u8; $fmt, $arg1);
    };
    ($fmt:literal, $arg1:expr, $arg2:expr, component = $component:expr, color = $color:expr) => {
        log::error!(component = $component, color = $color as u8; $fmt, $arg1, $arg2);
    };

    // Component only
    ($msg:literal, component = $component:expr) => {
        log::error!(component = $component, color = $crate::enums::LogColor::Red as u8; $msg);
    };
    ($fmt:literal, $arg1:expr, component = $component:expr) => {
        log::error!(component = $component, color = $crate::enums::LogColor::Red as u8; $fmt, $arg1);
    };
    ($fmt:literal, $arg1:expr, $arg2:expr, component = $component:expr) => {
        log::error!(component = $component, color = $crate::enums::LogColor::Red as u8; $fmt, $arg1, $arg2);
    };

    // Color only
    ($msg:literal, color = $color:expr) => {
        log::error!(color = $color as u8; $msg);
    };
    ($fmt:literal, $arg1:expr, color = $color:expr) => {
        log::error!(color = $color as u8; $fmt, $arg1);
    };
    ($fmt:literal, $arg1:expr, $arg2:expr, color = $color:expr) => {
        log::error!(color = $color as u8; $fmt, $arg1, $arg2);
    };
    ($fmt:literal, $arg1:expr, $arg2:expr, $arg3:expr, color = $color:expr) => {
        log::error!(color = $color as u8; $fmt, $arg1, $arg2, $arg3);
    };

    // Default (automatic red color, no component)
    ($msg:literal) => {
        log::error!(color = $crate::enums::LogColor::Red as u8; $msg);
    };
    ($fmt:literal, $($args:expr),+) => {
        log::error!(color = $crate::enums::LogColor::Red as u8; $fmt, $($args),+);
    };
}

// Re-exports
pub use log_debug;
pub use log_error;
pub use log_info;
pub use log_trace;
pub use log_warn;

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use std::{thread::sleep, time::Duration};

    use nautilus_core::UUID4;
    use nautilus_model::identifiers::TraderId;
    use rstest::*;
    use tempfile::tempdir;

    use crate::{
        enums::LogColor,
        logging::{
            logger::{Logger, LoggerConfig},
            logging_clock_set_static_mode, logging_clock_set_static_time,
            writer::FileWriterConfig,
        },
        testing::wait_until,
    };

    #[rstest]
    fn test_colored_logging_macros() {
        let config = LoggerConfig::from_spec("stdout=Trace;fileout=Trace;is_colored").unwrap();

        let temp_dir = tempdir().expect("Failed to create temporary directory");
        let file_config = FileWriterConfig {
            directory: Some(temp_dir.path().to_str().unwrap().to_string()),
            ..Default::default()
        };

        let log_guard = Logger::init_with_config(
            TraderId::from("TRADER-001"),
            UUID4::new(),
            config,
            file_config,
        )
        .expect("Failed to initialize logger");

        logging_clock_set_static_mode();
        logging_clock_set_static_time(1_650_000_000_000_000);

        // Test automatic color mappings using explicit components to ensure they're written
        log_trace!("This is a trace message", component = "TestComponent");
        log_debug!("This is a debug message", component = "TestComponent");
        log_info!("This is an info message", component = "TestComponent");
        log_warn!("This is a warning message", component = "TestComponent");
        log_error!("This is an error message", component = "TestComponent");

        // Test custom colors
        log_info!(
            "Success message",
            color = LogColor::Green,
            component = "TestComponent"
        );
        log_info!(
            "Information message",
            color = LogColor::Blue,
            component = "TestComponent"
        );
        log_warn!(
            "Custom warning",
            component = "TestComponent",
            color = LogColor::Magenta
        );

        // Test component only
        log_info!("Component test", component = "TestComponent");
        log_warn!("Component warning", component = "TestComponent");

        // Test both color and component (different orders)
        log_info!(
            "Color then component",
            color = LogColor::Cyan,
            component = "TestComponent"
        );

        // Allow time for logs to be written
        sleep(Duration::from_millis(200));

        drop(log_guard);

        // Wait until log file exists and has contents
        let mut log_contents = String::new();
        wait_until(
            || {
                if let Some(log_file) = std::fs::read_dir(&temp_dir)
                    .expect("Failed to read directory")
                    .filter_map(Result::ok)
                    .find(|entry| entry.path().is_file())
                {
                    let log_file_path = log_file.path();
                    log_contents =
                        std::fs::read_to_string(log_file_path).expect("Failed to read log file");
                    !log_contents.is_empty()
                } else {
                    false
                }
            },
            Duration::from_secs(3),
        );

        // Debug: print file contents if test is failing
        if !log_contents.contains("This is a trace message") {
            println!("File contents:\n{log_contents}");
        }

        // Verify that all log levels are present
        assert!(log_contents.contains("This is a trace message"));
        assert!(log_contents.contains("This is a debug message"));
        assert!(log_contents.contains("This is an info message"));
        assert!(log_contents.contains("This is a warning message"));
        assert!(log_contents.contains("This is an error message"));
        assert!(log_contents.contains("Success message"));
        assert!(log_contents.contains("Information message"));
        assert!(log_contents.contains("Custom warning"));

        // Verify component and color combinations
        assert!(log_contents.contains("Component test"));
        assert!(log_contents.contains("Component warning"));
        assert!(log_contents.contains("Color then component"));
    }
}
