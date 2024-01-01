use nautilus_core::{time::AtomicTime, uuid::UUID4};
use nautilus_model::identifiers::trader_id::TraderId;
use pyo3::prelude::*;

use crate::logging::{FileWriterConfig, Logger, LoggerConfig};

/// Initialize tracing
///
/// Tracing is meant to be used to trace/debug async rust code. It can be
/// configured to filter modules and write upto a specific level only using
/// by passing a configuration using the RUST_LOG environment variable.
///
/// # Safety
/// Should only be called once during an applications run, ideally at the
/// beginning of the run.
#[pyfunction]
pub fn init_tracing() {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .init();
}

/// Initialize logging
///
/// Logging should be used for Python and sync Rust logic which is most of
/// the components in the engine module. Logging can be configured
/// to filter components and write upto a specific level only
/// by passing a configuration using the NAUTILUS_LOG environment variable.
///
/// # Safety
/// Should only be called once during an applications run, ideally at the
/// beginning of the run.
#[pyfunction]
pub fn init_logging(
    clock: AtomicTime,
    trader_id: TraderId,
    instance_id: UUID4,
    file_writer_config: FileWriterConfig,
    config: LoggerConfig,
) {
    Logger::init_with_config(clock, trader_id, instance_id, file_writer_config, config);
}

#[pymethods]
impl LoggerConfig {
    #[staticmethod]
    #[pyo3(name = "from_spec")]
    pub fn py_from_spec(spec: String) -> Self {
        Self::parse(&spec)
    }

    #[staticmethod]
    #[pyo3(name = "from_env")]
    pub fn py_from_env() -> Self {
        Self::from_env()
    }
}

#[pymethods]
impl FileWriterConfig {
    #[new]
    pub fn py_new(
        directory: Option<String>,
        file_name: Option<String>,
        file_format: Option<String>,
    ) -> Self {
        Self::new(directory, file_name, file_format)
    }
}
