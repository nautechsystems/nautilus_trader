//! Core constants.

use std::env;

/// The NautilusTrader string constant.
pub static NAUTILUS_TRADER: &str = "NautilusTrader";

/// The NautilusTrader version string read from the top-level `pyproject.toml` at compile time.
pub static NAUTILUS_VERSION: &str = env!("NAUTILUS_VERSION");

/// The NautilusTrader common User-Agent string including the current version at compile time.
pub static NAUTILUS_USER_AGENT: &str = env!("NAUTILUS_USER_AGENT");

/// Prefix for log messages outside the main logging subsystem.
pub static NAUTILUS_PREFIX: &str = "[NAUTILUS]";
