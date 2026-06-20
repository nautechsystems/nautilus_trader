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

use pyo3::prelude::*;

use crate::database::DatabaseConfig;

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl DatabaseConfig {
    /// Configuration for database connections.
    ///
    /// # Notes
    ///
    /// If `database_type` is `"redis"`, it requires Redis version 6.2 or higher for correct operation.
    #[new]
    #[expect(clippy::too_many_arguments)]
    #[pyo3(signature = (database_type=None, host=None, port=None, username=None, password=None, ssl=None, connection_timeout=None, response_timeout=None, number_of_retries=None, exponent_base=None, max_delay=None, factor=None))]
    fn py_new(
        database_type: Option<String>,
        host: Option<String>,
        port: Option<u16>,
        username: Option<String>,
        password: Option<String>,
        ssl: Option<bool>,
        connection_timeout: Option<u16>,
        response_timeout: Option<u16>,
        number_of_retries: Option<usize>,
        exponent_base: Option<u64>,
        max_delay: Option<u64>,
        factor: Option<u64>,
    ) -> Self {
        let default = Self::default();
        Self {
            database_type: database_type.unwrap_or(default.database_type),
            host,
            port,
            username,
            password,
            ssl: ssl.unwrap_or(default.ssl),
            connection_timeout: connection_timeout.unwrap_or(default.connection_timeout),
            response_timeout: response_timeout.unwrap_or(default.response_timeout),
            number_of_retries: number_of_retries.unwrap_or(default.number_of_retries),
            exponent_base: exponent_base.unwrap_or(default.exponent_base),
            max_delay: max_delay.unwrap_or(default.max_delay),
            factor: factor.unwrap_or(default.factor),
        }
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }

    fn __str__(&self) -> String {
        format!("{self:?}")
    }

    #[getter]
    fn database_type(&self) -> &str {
        &self.database_type
    }

    #[getter]
    fn host(&self) -> Option<&str> {
        self.host.as_deref()
    }

    #[getter]
    fn port(&self) -> Option<u16> {
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
    fn ssl(&self) -> bool {
        self.ssl
    }

    #[getter]
    fn connection_timeout(&self) -> u16 {
        self.connection_timeout
    }

    #[getter]
    fn response_timeout(&self) -> u16 {
        self.response_timeout
    }

    #[getter]
    fn number_of_retries(&self) -> usize {
        self.number_of_retries
    }

    #[getter]
    fn exponent_base(&self) -> u64 {
        self.exponent_base
    }

    #[getter]
    fn max_delay(&self) -> u64 {
        self.max_delay
    }

    #[getter]
    fn factor(&self) -> u64 {
        self.factor
    }
}
