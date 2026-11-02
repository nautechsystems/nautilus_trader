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
    msgbus::MessageBusBackingFactory, python::msgbus::get_global_msgbus_factory_registry,
};
use nautilus_core::python::to_pyruntime_err;
use pyo3::prelude::*;

use crate::redis::msgbus::{RedisMessageBusConfig, RedisMessageBusFactory};

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl RedisMessageBusConfig {
    /// Configuration for a Redis-backed message bus backing.
    ///
    /// Redis 6.2 or higher is required for correct operation.
    #[new]
    #[expect(clippy::too_many_arguments)]
    #[pyo3(signature = (host=None, port=None, username=None, password=None, ssl=None, connection_timeout=None, response_timeout=None, number_of_retries=None, exponent_base=None, max_delay=None, factor=None))]
    fn py_new(
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
    const fn ssl(&self) -> bool {
        self.ssl
    }

    #[getter]
    const fn connection_timeout(&self) -> u16 {
        self.connection_timeout
    }

    #[getter]
    const fn response_timeout(&self) -> u16 {
        self.response_timeout
    }

    #[getter]
    const fn number_of_retries(&self) -> usize {
        self.number_of_retries
    }

    #[getter]
    const fn exponent_base(&self) -> u64 {
        self.exponent_base
    }

    #[getter]
    const fn max_delay(&self) -> u64 {
        self.max_delay
    }

    #[getter]
    const fn factor(&self) -> u64 {
        self.factor
    }
}

#[derive(Debug, Clone)]
#[pyclass(
    name = "RedisMessageBusFactory",
    module = "nautilus_trader.infrastructure",
    from_py_object
)]
#[pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.infrastructure")]
pub struct PyRedisMessageBusFactory {
    inner: RedisMessageBusFactory,
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl PyRedisMessageBusFactory {
    /// Creates a Redis message bus backing factory.
    #[new]
    #[pyo3(signature = (config=None))]
    fn py_new(config: Option<RedisMessageBusConfig>) -> Self {
        Self {
            inner: RedisMessageBusFactory::new(config.unwrap_or_default()),
        }
    }
}

#[expect(clippy::needless_pass_by_value)]
fn extract_redis_msgbus_config(
    py: Python<'_>,
    factory: Py<PyAny>,
) -> PyResult<Box<dyn MessageBusBackingFactory>> {
    Ok(Box::new(factory.extract::<RedisMessageBusConfig>(py)?))
}

#[expect(clippy::needless_pass_by_value)]
fn extract_redis_msgbus_factory(
    py: Python<'_>,
    factory: Py<PyAny>,
) -> PyResult<Box<dyn MessageBusBackingFactory>> {
    let factory = factory.extract::<PyRedisMessageBusFactory>(py)?;
    Ok(Box::new(factory.inner))
}

pub(in crate::python) fn register_redis_msgbus_factory() -> PyResult<()> {
    let registry = get_global_msgbus_factory_registry();
    registry
        .register(
            stringify!(RedisMessageBusConfig).to_string(),
            extract_redis_msgbus_config,
        )
        .map_err(to_pyruntime_err)?;
    registry
        .register(
            stringify!(RedisMessageBusFactory).to_string(),
            extract_redis_msgbus_factory,
        )
        .map_err(to_pyruntime_err)
}
