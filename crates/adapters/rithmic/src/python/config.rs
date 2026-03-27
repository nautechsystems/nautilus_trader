//! Python bindings for configuration types.

#[cfg(feature = "python")]
use pyo3::prelude::*;

use crate::config::{RithmicDataClientConfig, RithmicEnv, RithmicExecClientConfig};

/// Python wrapper for RithmicEnv.
#[cfg(feature = "python")]
#[pyclass(name = "RithmicEnv", from_py_object)]
#[derive(Clone)]
pub struct PyRithmicEnv {
    inner: RithmicEnv,
}

#[cfg(feature = "python")]
#[pymethods]
impl PyRithmicEnv {
    /// Demo/paper trading environment.
    #[classattr]
    const DEMO: Self = Self {
        inner: RithmicEnv::Demo,
    };

    /// Live trading environment.
    #[classattr]
    const LIVE: Self = Self {
        inner: RithmicEnv::Live,
    };

    /// Test environment.
    #[classattr]
    const TEST: Self = Self {
        inner: RithmicEnv::Test,
    };

    fn __repr__(&self) -> String {
        format!("RithmicEnv.{}", self.inner.to_string().to_uppercase())
    }
}

#[cfg(feature = "python")]
impl From<PyRithmicEnv> for RithmicEnv {
    fn from(py_env: PyRithmicEnv) -> Self {
        py_env.inner
    }
}

/// Deprecated: Use [`PyRithmicEnv`] instead.
///
/// This class is provided for backwards compatibility and will be removed
/// in a future major version.
#[cfg(feature = "python")]
#[pyclass(name = "RithmicEnvironment", skip_from_py_object)]
#[derive(Clone)]
pub struct PyRithmicEnvironment {
    inner: RithmicEnv,
}

#[cfg(feature = "python")]
#[pymethods]
impl PyRithmicEnvironment {
    /// Demo/paper trading environment.
    #[classattr]
    const DEMO: Self = Self {
        inner: RithmicEnv::Demo,
    };

    /// Live trading environment.
    #[classattr]
    const LIVE: Self = Self {
        inner: RithmicEnv::Live,
    };

    /// Test environment.
    #[classattr]
    const TEST: Self = Self {
        inner: RithmicEnv::Test,
    };

    fn __repr__(&self) -> String {
        format!(
            "RithmicEnvironment.{} (deprecated, use RithmicEnv)",
            self.inner.to_string().to_uppercase()
        )
    }
}

/// Python wrapper for RithmicDataClientConfig.
#[cfg(feature = "python")]
#[pyclass(name = "RithmicDataClientConfig", skip_from_py_object)]
#[derive(Clone)]
pub struct PyRithmicDataClientConfig {
    pub(crate) inner: RithmicDataClientConfig,
}

#[cfg(feature = "python")]
#[pymethods]
impl PyRithmicDataClientConfig {
    /// Creates a new data client configuration.
    #[new]
    #[pyo3(signature = (environment, username, password, system_name, app_name="NautilusTrader", app_version="1.0", fcm_id=None, ib_id=None, server=None, alt_server=None))]
    fn new(
        environment: PyRithmicEnv,
        username: String,
        password: String,
        system_name: String,
        app_name: &str,
        app_version: &str,
        fcm_id: Option<String>,
        ib_id: Option<String>,
        server: Option<String>,
        alt_server: Option<String>,
    ) -> Self {
        Self {
            inner: RithmicDataClientConfig {
                environment: environment.inner,
                username,
                password,
                system_name,
                app_name: app_name.to_string(),
                app_version: app_version.to_string(),
                fcm_id,
                ib_id,
                server,
                alt_server,
            },
        }
    }

    /// Creates configuration from environment variables.
    #[staticmethod]
    #[pyo3(signature = (profile=None))]
    fn from_env(profile: Option<String>) -> PyResult<Self> {
        let config = RithmicDataClientConfig::from_env_with_profile(profile.as_deref())
            .map_err(|e| pyo3::exceptions::PyValueError::new_err(e.to_string()))?;
        Ok(Self { inner: config })
    }

    #[getter]
    fn username(&self) -> &str {
        &self.inner.username
    }

    #[getter]
    fn system_name(&self) -> &str {
        &self.inner.system_name
    }

    #[getter]
    fn app_name(&self) -> &str {
        &self.inner.app_name
    }

    fn __repr__(&self) -> String {
        format!(
            "RithmicDataClientConfig(environment={:?}, username='{}', system_name='{}')",
            self.inner.environment, self.inner.username, self.inner.system_name
        )
    }
}

/// Python wrapper for RithmicExecClientConfig.
#[cfg(feature = "python")]
#[pyclass(name = "RithmicExecClientConfig", skip_from_py_object)]
#[derive(Clone)]
pub struct PyRithmicExecClientConfig {
    pub(crate) inner: RithmicExecClientConfig,
}

#[cfg(feature = "python")]
#[pymethods]
impl PyRithmicExecClientConfig {
    /// Creates a new execution client configuration.
    #[new]
    #[pyo3(signature = (environment, username, password, system_name, account_id, app_name="NautilusTrader", app_version="1.0", fcm_id=None, ib_id=None, server=None, alt_server=None))]
    fn new(
        environment: PyRithmicEnv,
        username: String,
        password: String,
        system_name: String,
        account_id: String,
        app_name: &str,
        app_version: &str,
        fcm_id: Option<String>,
        ib_id: Option<String>,
        server: Option<String>,
        alt_server: Option<String>,
    ) -> Self {
        Self {
            inner: RithmicExecClientConfig {
                environment: environment.inner,
                username,
                password,
                system_name,
                app_name: app_name.to_string(),
                app_version: app_version.to_string(),
                fcm_id,
                ib_id,
                account_id,
                server,
                alt_server,
            },
        }
    }

    /// Creates configuration from environment variables.
    #[staticmethod]
    #[pyo3(signature = (profile=None))]
    fn from_env(profile: Option<String>) -> PyResult<Self> {
        let config = RithmicExecClientConfig::from_env_with_profile(profile.as_deref())
            .map_err(|e| pyo3::exceptions::PyValueError::new_err(e.to_string()))?;
        Ok(Self { inner: config })
    }

    #[getter]
    fn username(&self) -> &str {
        &self.inner.username
    }

    #[getter]
    fn account_id(&self) -> &str {
        &self.inner.account_id
    }

    #[getter]
    fn system_name(&self) -> &str {
        &self.inner.system_name
    }

    fn __repr__(&self) -> String {
        format!(
            "RithmicExecClientConfig(environment={:?}, username='{}', account_id='{}')",
            self.inner.environment, self.inner.username, self.inner.account_id
        )
    }
}

/// Registers configuration types with the Python module.
#[cfg(feature = "python")]
pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PyRithmicEnv>()?;
    m.add_class::<PyRithmicEnvironment>()?; // Deprecated alias
    m.add_class::<PyRithmicDataClientConfig>()?;
    m.add_class::<PyRithmicExecClientConfig>()?;
    Ok(())
}
