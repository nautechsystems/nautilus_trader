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

use nautilus_common::{
    cache::database::CacheDatabaseFactory,
    python::cache::get_global_cache_database_factory_registry,
};
use nautilus_core::python::to_pyruntime_err;
use pyo3::prelude::*;

use crate::sql::cache::PostgresCacheConfig;

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl PostgresCacheConfig {
    /// Configuration for a Postgres-backed cache database.
    ///
    /// Missing fields are resolved from Postgres environment variables and then built-in defaults.
    #[new]
    #[pyo3(signature = (host=None, port=None, username=None, password=None, database=None))]
    fn py_new(
        host: Option<String>,
        port: Option<u16>,
        username: Option<String>,
        password: Option<String>,
        database: Option<String>,
    ) -> Self {
        Self {
            host,
            port,
            username,
            password,
            database,
        }
    }

    #[getter]
    fn host(&self) -> Option<&str> {
        self.host.as_deref()
    }

    #[getter]
    const fn port(&self) -> Option<u16> {
        self.port
    }

    #[getter]
    fn username(&self) -> Option<&str> {
        self.username.as_deref()
    }

    #[getter]
    fn password(&self) -> Option<&str> {
        self.password.as_deref()
    }

    #[getter]
    fn database(&self) -> Option<&str> {
        self.database.as_deref()
    }
}

#[expect(clippy::needless_pass_by_value)]
fn extract_postgres_cache_database_factory(
    py: Python<'_>,
    factory: Py<PyAny>,
) -> PyResult<Box<dyn CacheDatabaseFactory>> {
    Ok(Box::new(factory.extract::<PostgresCacheConfig>(py)?))
}

pub(in crate::python) fn register_postgres_cache_database_factory() -> PyResult<()> {
    get_global_cache_database_factory_registry()
        .register(
            stringify!(PostgresCacheConfig).to_string(),
            extract_postgres_cache_database_factory,
        )
        .map_err(to_pyruntime_err)
}
