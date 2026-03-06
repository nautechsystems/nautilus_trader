use pyo3::prelude::*;

use crate::sql::pg::PostgresConnectOptions;

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods(module = "nautilus_trader.infrastructure")]
impl PostgresConnectOptions {
    /// Creates a new `PostgresConnectOptions` instance.
    #[new]
    #[pyo3(signature = (host, port, user, password, database))]
    const fn py_new(
        host: String,
        port: u16,
        user: String,
        password: String,
        database: String,
    ) -> Self {
        Self::new(host, port, user, password, database)
    }

    /// Returns a string representation of the configuration.
    fn __repr__(&self) -> String {
        format!(
            "PostgresConnectOptions(host={}, port={}, username={}, database={})",
            self.host, self.port, self.username, self.database
        )
    }

    /// Returns the host.
    #[getter]
    fn host(&self) -> String {
        self.host.clone()
    }

    /// Returns the port.
    #[getter]
    const fn port(&self) -> u16 {
        self.port
    }

    /// Returns the username.
    #[getter]
    fn username(&self) -> String {
        self.username.clone()
    }

    /// Returns the password.
    #[getter]
    fn password(&self) -> String {
        self.password.clone()
    }

    /// Returns the database.
    #[getter]
    fn database(&self) -> String {
        self.database.clone()
    }
}
