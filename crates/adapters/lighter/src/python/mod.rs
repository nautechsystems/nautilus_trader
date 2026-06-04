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
//!
//! Lighter's Python surface is intentionally narrow: configuration, environment
//! selection, and factories. Data and execution clients are consumed directly
//! through the Rust trait surface and are not exposed to Python.

#![expect(
    clippy::missing_errors_doc,
    reason = "errors documented on underlying Rust methods"
)]

pub mod config;
pub mod factories;

use std::time::{SystemTime, UNIX_EPOCH};

use nautilus_common::factories::{ClientConfig, DataClientFactory, ExecutionClientFactory};
use nautilus_core::python::{to_pyruntime_err, to_pyvalue_err};
use nautilus_system::get_global_pyo3_registry;
use pyo3::prelude::*;

use crate::{
    common::{
        consts::{LIGHTER, LIGHTER_NAUTILUS_INTEGRATOR_ACCOUNT_INDEX},
        credential::Credential,
        enums::LighterEnvironment,
        urls::lighter_chain_id,
    },
    config::{LighterDataClientConfig, LighterExecClientConfig},
    factories::{LighterDataClientFactory, LighterExecutionClientFactory},
    http::{
        client::{LighterHttpClient, LighterRawHttpClient},
        models::LighterSendTxRequest,
    },
    signing::{
        auth_token::fresh_k,
        tx::{ApproveIntegratorTxInfo, LighterTx, TxContext, TxInfoJson, sign_tx},
    },
};

const TX_EXPIRY_MS: i64 = 5 * 60 * 1_000;

#[expect(clippy::needless_pass_by_value)]
fn extract_lighter_data_factory(
    py: Python<'_>,
    factory: Py<PyAny>,
) -> PyResult<Box<dyn DataClientFactory>> {
    match factory.extract::<LighterDataClientFactory>(py) {
        Ok(f) => Ok(Box::new(f)),
        Err(e) => Err(to_pyvalue_err(format!(
            "Failed to extract LighterDataClientFactory: {e}"
        ))),
    }
}

#[expect(clippy::needless_pass_by_value)]
fn extract_lighter_exec_factory(
    py: Python<'_>,
    factory: Py<PyAny>,
) -> PyResult<Box<dyn ExecutionClientFactory>> {
    match factory.extract::<LighterExecutionClientFactory>(py) {
        Ok(f) => Ok(Box::new(f)),
        Err(e) => Err(to_pyvalue_err(format!(
            "Failed to extract LighterExecutionClientFactory: {e}"
        ))),
    }
}

#[expect(clippy::needless_pass_by_value)]
fn extract_lighter_data_config(
    py: Python<'_>,
    config: Py<PyAny>,
) -> PyResult<Box<dyn ClientConfig>> {
    match config.extract::<LighterDataClientConfig>(py) {
        Ok(c) => Ok(Box::new(c)),
        Err(e) => Err(to_pyvalue_err(format!(
            "Failed to extract LighterDataClientConfig: {e}"
        ))),
    }
}

#[expect(clippy::needless_pass_by_value)]
fn extract_lighter_exec_config(
    py: Python<'_>,
    config: Py<PyAny>,
) -> PyResult<Box<dyn ClientConfig>> {
    match config.extract::<LighterExecClientConfig>(py) {
        Ok(c) => Ok(Box::new(c)),
        Err(e) => Err(to_pyvalue_err(format!(
            "Failed to extract LighterExecClientConfig: {e}"
        ))),
    }
}

async fn submit_integrator_revocation(environment: LighterEnvironment) -> anyhow::Result<String> {
    let credential = Credential::resolve(None, None, None, environment)?
        .ok_or_else(|| anyhow::anyhow!("no Lighter L2 credentials in env"))?;
    let chain_id = lighter_chain_id(environment);

    let raw = LighterRawHttpClient::new(environment, None, 30, None)?;
    let http = LighterHttpClient::from_raw(raw);
    let next_nonce = http
        .get_next_nonce(credential.account_index(), credential.api_key_index())
        .await?
        .nonce;

    let now_ms = SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis() as i64;
    let tx = ApproveIntegratorTxInfo {
        context: TxContext {
            account_index: credential.account_index(),
            api_key_index: credential.api_key_index(),
            nonce: next_nonce,
            expired_at: now_ms.saturating_add(TX_EXPIRY_MS),
        },
        integrator_account_index: LIGHTER_NAUTILUS_INTEGRATOR_ACCOUNT_INDEX as i64,
        max_perps_taker_fee: 0,
        max_perps_maker_fee: 0,
        max_spot_taker_fee: 0,
        max_spot_maker_fee: 0,
        approval_expiry: 0,
        skip_nonce: 0,
    };

    let l2_signed = sign_tx(&tx, chain_id, &credential.private_key()?, fresh_k());
    let tx_info_str = TxInfoJson::approve_integrator(&tx, &l2_signed, "");
    let request = LighterSendTxRequest::new(tx.tx_type() as u8, tx_info_str);
    let response = http.send_tx(&request).await?;

    Ok(format!(
        "integrator={LIGHTER_NAUTILUS_INTEGRATOR_ACCOUNT_INDEX} account_index={} tx_hash={}",
        credential.account_index(),
        response.tx_hash,
    ))
}

/// Revoke the Nautilus integrator approval when leaving the adapter.
///
/// This cleanup call is not a trading-mode toggle. Live trading through this
/// adapter requires the approval; the next execution-client startup records a
/// fresh zero-fee approval.
///
/// See:
/// <https://nautilustrader.io/docs/nightly/integrations/lighter.html#integrator-attribution>.
///
/// Reads L2 credentials from `LIGHTER_API_KEY_INDEX`, `LIGHTER_API_SECRET`,
/// and `LIGHTER_ACCOUNT_INDEX` (or the `LIGHTER_TESTNET_*` variants).
///
/// Returns a status string on the awaitable; raises on failure.
#[pyfunction]
#[pyo3_stub_gen::derive::gen_stub_pyfunction(module = "nautilus_trader.adapters.lighter")]
#[pyo3(name = "revoke_lighter_integrator", signature = (environment = LighterEnvironment::Mainnet))]
fn py_revoke_lighter_integrator(
    py: Python<'_>,
    environment: LighterEnvironment,
) -> PyResult<Bound<'_, PyAny>> {
    pyo3_async_runtimes::tokio::future_into_py(py, async move {
        submit_integrator_revocation(environment)
            .await
            .map(|s| format!("submitted revocation for {s}"))
            .map_err(to_pyvalue_err)
    })
}

/// Loaded as `nautilus_pyo3.lighter`.
#[pymodule]
pub fn lighter(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add(stringify!(LIGHTER), LIGHTER)?;
    m.add_class::<LighterEnvironment>()?;
    m.add_class::<LighterDataClientConfig>()?;
    m.add_class::<LighterExecClientConfig>()?;
    m.add_class::<LighterDataClientFactory>()?;
    m.add_class::<LighterExecutionClientFactory>()?;
    m.add_function(wrap_pyfunction!(py_revoke_lighter_integrator, m)?)?;

    let registry = get_global_pyo3_registry();

    if let Err(e) =
        registry.register_factory_extractor(LIGHTER.to_string(), extract_lighter_data_factory)
    {
        return Err(to_pyruntime_err(format!(
            "Failed to register Lighter data factory extractor: {e}"
        )));
    }

    if let Err(e) =
        registry.register_exec_factory_extractor(LIGHTER.to_string(), extract_lighter_exec_factory)
    {
        return Err(to_pyruntime_err(format!(
            "Failed to register Lighter exec factory extractor: {e}"
        )));
    }

    if let Err(e) = registry.register_config_extractor(
        "LighterDataClientConfig".to_string(),
        extract_lighter_data_config,
    ) {
        return Err(to_pyruntime_err(format!(
            "Failed to register Lighter data config extractor: {e}"
        )));
    }

    if let Err(e) = registry.register_config_extractor(
        "LighterExecClientConfig".to_string(),
        extract_lighter_exec_config,
    ) {
        return Err(to_pyruntime_err(format!(
            "Failed to register Lighter exec config extractor: {e}"
        )));
    }

    Ok(())
}
