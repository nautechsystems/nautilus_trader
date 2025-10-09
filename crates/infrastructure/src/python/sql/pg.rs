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
