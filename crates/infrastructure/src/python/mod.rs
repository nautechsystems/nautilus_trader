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
pub fn infrastructure(_: Python<'_>, m: &Bound<'_, PyModule>) -> PyResult<()> {
    #[cfg(not(any(feature = "redis", feature = "postgres")))]
    let _ = m;

    #[cfg(feature = "redis")]
    m.add_class::<crate::redis::cache::RedisCacheConfig>()?;
    #[cfg(feature = "redis")]
    m.add_class::<crate::redis::cache::RedisCacheDatabase>()?;
    #[cfg(feature = "redis")]
    m.add_class::<redis::msgbus::PyRedisMessageBusBacking>()?;
    #[cfg(feature = "redis")]
    m.add_class::<redis::msgbus::PyRedisMessageBusFactory>()?;
    #[cfg(feature = "redis")]
    m.add_class::<crate::redis::msgbus::RedisMessageBusConfig>()?;
    #[cfg(feature = "redis")]
    redis::msgbus::register_redis_msgbus_factory()?;
    #[cfg(feature = "postgres")]
    m.add_class::<crate::sql::cache::PostgresCacheConfig>()?;
    #[cfg(feature = "postgres")]
    m.add_class::<crate::sql::cache::PostgresCacheDatabase>()?;
    #[cfg(feature = "postgres")]
    m.add_class::<crate::sql::pg::PostgresConnectOptions>()?;
    Ok(())
}

#[cfg(all(test, feature = "redis"))]
mod tests {
    use nautilus_common::python::msgbus::get_global_msgbus_factory_registry;
    use pyo3::PyRef;
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_infrastructure_module_exports_redis_message_bus_types() {
        Python::initialize();
        Python::attach(|py| {
            let module = PyModule::new(py, "infrastructure").unwrap();

            infrastructure(py, &module).unwrap();

            assert!(module.getattr("RedisMessageBusBacking").is_ok());
            assert!(module.getattr("RedisMessageBusConfig").is_ok());
            assert!(module.getattr("RedisMessageBusFactory").is_ok());

            let config = module
                .getattr("RedisMessageBusConfig")
                .unwrap()
                .call1((
                    "redis.example.com",
                    6380,
                    "user",
                    "secret",
                    true,
                    7,
                    8,
                    9,
                    3,
                    10,
                    4,
                ))
                .unwrap();
            {
                let config = config
                    .extract::<PyRef<crate::redis::msgbus::RedisMessageBusConfig>>()
                    .unwrap();

                assert_eq!(config.host.as_deref(), Some("redis.example.com"));
                assert_eq!(config.port, Some(6380));
                assert_eq!(config.username.as_deref(), Some("user"));
                assert_eq!(config.password.as_deref(), Some("secret"));
                assert!(config.ssl);
                assert_eq!(config.connection_timeout, 7);
                assert_eq!(config.response_timeout, 8);
                assert_eq!(config.number_of_retries, 9);
                assert_eq!(config.exponent_base, 3);
                assert_eq!(config.max_delay, 10);
                assert_eq!(config.factor, 4);
            }
            let factory = module
                .getattr("RedisMessageBusFactory")
                .unwrap()
                .call1((config,))
                .unwrap()
                .unbind();
            let factory = get_global_msgbus_factory_registry()
                .extract(py, factory)
                .unwrap();
            let debug = format!("{factory:?}");

            assert!(debug.contains("redis.example.com"));
            assert!(debug.contains("password: Some(\"***\")"));
            assert!(!debug.contains("secret"));

            let second_module = PyModule::new(py, "infrastructure").unwrap();
            infrastructure(py, &second_module).unwrap();
            assert!(second_module.getattr("RedisMessageBusFactory").is_ok());
        });
    }
}
