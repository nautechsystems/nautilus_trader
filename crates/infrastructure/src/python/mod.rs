//! Python bindings from [PyO3](https://pyo3.rs).

#[cfg(feature = "redis")]
pub mod redis;

#[cfg(feature = "postgres")]
pub mod sql;

use pyo3::{prelude::*, pymodule};

/// Python module initializer for the `infrastructure` package.
///
/// # Errors
///
/// Returns a `PyErr` if the module initialization fails, e.g., when adding classes to the module.
#[pymodule]
#[allow(unused_variables)]
pub fn infrastructure(_: Python<'_>, m: &Bound<'_, PyModule>) -> PyResult<()> {
    #[cfg(feature = "redis")]
    m.add_class::<crate::redis::cache::RedisCacheDatabase>()?;
    #[cfg(feature = "redis")]
    m.add_class::<crate::redis::msgbus::RedisMessageBusDatabase>()?;
    #[cfg(feature = "postgres")]
    m.add_class::<crate::sql::cache::PostgresCacheDatabase>()?;
    #[cfg(feature = "postgres")]
    m.add_class::<crate::sql::pg::PostgresConnectOptions>()?;
    Ok(())
}
