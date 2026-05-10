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

//! Tracing subscriber for capturing logs from external Rust libraries.
//!
//! This module initializes a standard tracing subscriber that outputs directly
//! to stdout, allowing external Rust libraries that use the `tracing` crate
//! to have their logs displayed.
//!
//! # Usage
//!
//! 1. Set `use_tracing=True` in `LoggingConfig`.
//! 2. Set `RUST_LOG` environment variable to control filtering.
//!
//! # Example
//!
//! ```text
//! RUST_LOG=hyper=debug,tokio=warn python my_script.py
//! ```

use std::sync::atomic::{AtomicBool, Ordering};

use tracing::{Event, Level, Subscriber};
use tracing_subscriber::{
    EnvFilter,
    fmt::{self, FmtContext, FormatEvent, FormatFields, format::Writer},
    prelude::*,
    registry::LookupSpan,
};

static TRACING_INITIALIZED: AtomicBool = AtomicBool::new(false);

struct NautilusFormatter;

impl<S, N> FormatEvent<S, N> for NautilusFormatter
where
    S: Subscriber + for<'a> LookupSpan<'a>,
    N: for<'a> FormatFields<'a> + 'static,
{
    fn format_event(
        &self,
        ctx: &FmtContext<'_, S, N>,
        mut writer: Writer<'_>,
        event: &Event<'_>,
    ) -> std::fmt::Result {
        let now = chrono::Utc::now();
        let timestamp = now.format("%Y-%m-%dT%H:%M:%S%.9fZ");

        let level = match *event.metadata().level() {
            Level::TRACE => "[TRACE]",
            Level::DEBUG => "[DEBUG]",
            Level::INFO => "[INFO]",
            Level::WARN => "[WARN]",
            Level::ERROR => "[ERROR]",
        };

        let target = event.metadata().target();

        write!(writer, "{timestamp} {level} {target}: ")?;
        ctx.field_format().format_fields(writer.by_ref(), event)?;
        writeln!(writer)
    }
}

/// Returns whether the tracing subscriber has been initialized.
#[must_use]
pub fn tracing_is_initialized() -> bool {
    TRACING_INITIALIZED.load(Ordering::Relaxed)
}

/// Initializes a tracing subscriber for external Rust crate logging.
///
/// This sets up a standard tracing subscriber that outputs to stdout with
/// the format controlled by `RUST_LOG` environment variable. The output
/// format uses nanosecond timestamps to align with Nautilus logging.
///
/// # Environment Variables
///
/// - `RUST_LOG`: Controls which modules emit tracing events and at what level.
///   - Example: `RUST_LOG=hyper=debug,tokio=warn`.
///   - Default: `warn` (if not set).
///
/// # Errors
///
/// Returns an error if the tracing subscriber has already been initialized.
pub fn init_tracing() -> anyhow::Result<()> {
    if TRACING_INITIALIZED.load(Ordering::SeqCst) {
        anyhow::bail!("Tracing subscriber already initialized");
    }

    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("warn"));

    tracing_subscriber::registry()
        .with(filter)
        .with(fmt::layer().event_format(NautilusFormatter))
        .try_init()
        .map_err(|e| anyhow::anyhow!("Failed to initialize tracing subscriber: {e}"))?;

    TRACING_INITIALIZED.store(true, Ordering::SeqCst);
    Ok(())
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_tracing_is_initialized_returns_bool() {
        let _ = tracing_is_initialized();
    }
}
