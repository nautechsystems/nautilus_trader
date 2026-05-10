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

//! Python bindings from `pyo3`.

#![expect(
    clippy::missing_errors_doc,
    reason = "errors documented on underlying Rust methods"
)]

pub mod config;
pub mod http;
pub mod order_client;
pub mod websocket;

use nautilus_common::factories::{ClientConfig, DataClientFactory, ExecutionClientFactory};
use nautilus_core::python::{to_pyruntime_err, to_pyvalue_err};
use nautilus_system::get_global_pyo3_registry;
use pyo3::prelude::*;

use crate::{
    common::{consts::BULLET_POST_ONLY_WOULD_MATCH, enums::BulletEnvironment},
    config::{BulletDataClientConfig, BulletExecClientConfig},
    factories::{
        BulletDataClientFactory, BulletExecFactoryConfig, BulletExecutionClientFactory,
    },
    http::client::BulletHttpClient,
    python::order_client::BulletOrderClient,
    websocket::client::BulletWebSocketClient,
};

#[expect(clippy::needless_pass_by_value)]
fn extract_bullet_data_factory(
    py: Python<'_>,
    factory: Py<PyAny>,
) -> PyResult<Box<dyn DataClientFactory>> {
    match factory.extract::<BulletDataClientFactory>(py) {
        Ok(f) => Ok(Box::new(f)),
        Err(e) => Err(to_pyvalue_err(format!(
            "Failed to extract BulletDataClientFactory: {e}"
        ))),
    }
}

#[expect(clippy::needless_pass_by_value)]
fn extract_bullet_exec_factory(
    py: Python<'_>,
    factory: Py<PyAny>,
) -> PyResult<Box<dyn ExecutionClientFactory>> {
    match factory.extract::<BulletExecutionClientFactory>(py) {
        Ok(f) => Ok(Box::new(f)),
        Err(e) => Err(to_pyvalue_err(format!(
            "Failed to extract BulletExecutionClientFactory: {e}"
        ))),
    }
}

#[expect(clippy::needless_pass_by_value)]
fn extract_bullet_data_config(
    py: Python<'_>,
    config: Py<PyAny>,
) -> PyResult<Box<dyn ClientConfig>> {
    match config.extract::<BulletDataClientConfig>(py) {
        Ok(c) => Ok(Box::new(c)),
        Err(e) => Err(to_pyvalue_err(format!(
            "Failed to extract BulletDataClientConfig: {e}"
        ))),
    }
}

#[expect(clippy::needless_pass_by_value)]
fn extract_bullet_exec_config(
    py: Python<'_>,
    config: Py<PyAny>,
) -> PyResult<Box<dyn ClientConfig>> {
    match config.extract::<BulletExecFactoryConfig>(py) {
        Ok(c) => Ok(Box::new(c)),
        Err(e) => Err(to_pyvalue_err(format!(
            "Failed to extract BulletExecFactoryConfig: {e}"
        ))),
    }
}

/// Loaded as `nautilus_pyo3.bullet`.
#[pymodule]
pub fn bullet(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add("BULLET_POST_ONLY_WOULD_MATCH", BULLET_POST_ONLY_WOULD_MATCH)?;
    m.add_class::<BulletHttpClient>()?;
    m.add_class::<BulletOrderClient>()?;
    m.add_class::<BulletWebSocketClient>()?;
    m.add_class::<BulletEnvironment>()?;
    m.add_class::<BulletDataClientConfig>()?;
    m.add_class::<BulletExecClientConfig>()?;
    m.add_class::<BulletExecFactoryConfig>()?;
    m.add_class::<BulletDataClientFactory>()?;
    m.add_class::<BulletExecutionClientFactory>()?;

    let registry = get_global_pyo3_registry();

    if let Err(e) = registry
        .register_factory_extractor("BULLET".to_string(), extract_bullet_data_factory)
    {
        return Err(to_pyruntime_err(format!(
            "Failed to register Bullet data factory extractor: {e}"
        )));
    }

    if let Err(e) =
        registry.register_exec_factory_extractor("BULLET".to_string(), extract_bullet_exec_factory)
    {
        return Err(to_pyruntime_err(format!(
            "Failed to register Bullet exec factory extractor: {e}"
        )));
    }

    if let Err(e) = registry.register_config_extractor(
        "BulletDataClientConfig".to_string(),
        extract_bullet_data_config,
    ) {
        return Err(to_pyruntime_err(format!(
            "Failed to register Bullet data config extractor: {e}"
        )));
    }

    if let Err(e) = registry.register_config_extractor(
        "BulletExecFactoryConfig".to_string(),
        extract_bullet_exec_config,
    ) {
        return Err(to_pyruntime_err(format!(
            "Failed to register Bullet exec config extractor: {e}"
        )));
    }

    Ok(())
}
