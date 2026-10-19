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

//! Python bindings for Deposit Wallet position operations.

use std::sync::Arc;

use nautilus_core::{env::get_or_env_var, python::to_pyvalue_err, string::secret::SecretString};
use nautilus_network::websocket::proxy::ProxyUrl;
use pyo3::prelude::*;
use rust_decimal::Decimal;

use crate::{
    common::credential::{EvmPrivateKey, RelayerApiKey, credential_env_vars},
    positions::{
        PolymarketPositionClient, PolymarketPositionOutcome, PolymarketPositionTransaction,
    },
};

/// Terminal result of a Polymarket split, merge, or redeem operation.
#[pyclass(
    module = "nautilus_trader.adapters.polymarket",
    name = "PolymarketPositionOutcome",
    frozen,
    skip_from_py_object
)]
#[pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.adapters.polymarket")]
#[derive(Clone, Debug)]
pub struct PyPolymarketPositionOutcome {
    inner: PolymarketPositionOutcome,
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl PyPolymarketPositionOutcome {
    /// Relayer terminal status: `confirmed`, `failed`, or `invalid`.
    #[getter]
    fn status(&self) -> &'static str {
        match self.inner {
            PolymarketPositionOutcome::Confirmed { .. } => "confirmed",
            PolymarketPositionOutcome::Failed { .. } => "failed",
            PolymarketPositionOutcome::Invalid { .. } => "invalid",
        }
    }

    /// Relayer transaction identifier.
    #[getter]
    fn transaction_id(&self) -> &str {
        match &self.inner {
            PolymarketPositionOutcome::Confirmed { transaction_id, .. }
            | PolymarketPositionOutcome::Failed { transaction_id, .. }
            | PolymarketPositionOutcome::Invalid { transaction_id, .. } => transaction_id,
        }
    }

    /// On-chain transaction hash when the Relayer supplied one.
    #[getter]
    fn transaction_hash(&self) -> Option<&str> {
        match &self.inner {
            PolymarketPositionOutcome::Confirmed {
                transaction_hash, ..
            }
            | PolymarketPositionOutcome::Failed {
                transaction_hash, ..
            } => transaction_hash.as_deref(),
            PolymarketPositionOutcome::Invalid { .. } => None,
        }
    }

    /// Relayer error detail when present.
    #[getter]
    fn error_msg(&self) -> Option<&str> {
        match &self.inner {
            PolymarketPositionOutcome::Failed { error_msg, .. }
            | PolymarketPositionOutcome::Invalid { error_msg, .. } => error_msg.as_deref(),
            PolymarketPositionOutcome::Confirmed { .. } => None,
        }
    }

    fn __repr__(&self) -> String {
        format!(
            "PolymarketPositionOutcome(status='{}', transaction_id='{}', transaction_hash={:?}, error_msg={:?})",
            self.status(),
            self.transaction_id(),
            self.transaction_hash(),
            self.error_msg(),
        )
    }
}

/// Submitted position operation that can be polled to a terminal Relayer state.
#[pyclass(
    module = "nautilus_trader.adapters.polymarket",
    name = "PolymarketPositionTransaction",
    skip_from_py_object
)]
#[pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.adapters.polymarket")]
#[derive(Debug)]
pub struct PyPolymarketPositionTransaction {
    transaction_id: String,
    inner: Option<PolymarketPositionTransaction>,
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl PyPolymarketPositionTransaction {
    /// Relayer transaction identifier returned at submit time.
    #[getter]
    fn transaction_id(&self) -> &str {
        &self.transaction_id
    }

    /// Polls the Relayer until the transaction is confirmed, failed, or invalid.
    fn wait<'py>(&mut self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let inner = self
            .inner
            .take()
            .ok_or_else(|| to_pyvalue_err("transaction wait() has already been consumed"))?;
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let outcome = inner.wait().await.map_err(to_pyvalue_err)?;
            Ok(PyPolymarketPositionOutcome { inner: outcome })
        })
    }

    fn __repr__(&self) -> String {
        match &self.inner {
            Some(inner) => format!(
                "PolymarketPositionTransaction(transaction_id='{}')",
                inner.transaction_id()
            ),
            None => format!(
                "PolymarketPositionTransaction(transaction_id='{}', consumed)",
                self.transaction_id
            ),
        }
    }
}

impl From<PolymarketPositionTransaction> for PyPolymarketPositionTransaction {
    fn from(inner: PolymarketPositionTransaction) -> Self {
        Self {
            transaction_id: inner.transaction_id().to_string(),
            inner: Some(inner),
        }
    }
}

/// Deposit Wallet client for split, merge, and redeem position operations.
#[pyclass(
    module = "nautilus_trader.adapters.polymarket",
    name = "PolymarketPositionClient",
    skip_from_py_object
)]
#[pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.adapters.polymarket")]
#[derive(Debug)]
pub struct PyPolymarketPositionClient {
    inner: Arc<PolymarketPositionClient>,
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl PyPolymarketPositionClient {
    #[new]
    #[pyo3(signature = (
        private_key=None,
        funder=None,
        relayer_api_key=None,
        relayer_api_key_address=None,
        base_url_relayer=None,
        base_url_clob=None,
        timeout_secs=None,
        proxy_url=None,
        base_url_rpc=None
    ))]
    #[allow(
        clippy::too_many_arguments,
        reason = "Python constructor mirrors optional credential fields"
    )]
    fn py_new(
        private_key: Option<String>,
        funder: Option<String>,
        relayer_api_key: Option<String>,
        relayer_api_key_address: Option<String>,
        base_url_relayer: Option<String>,
        base_url_clob: Option<String>,
        timeout_secs: Option<u64>,
        proxy_url: Option<String>,
        base_url_rpc: Option<String>,
    ) -> PyResult<Self> {
        let private_key = private_key.map(SecretString::from);
        let relayer_api_key = relayer_api_key.map(SecretString::from);
        let (_, _, _, private_key_var, funder_var) = credential_env_vars();

        let private_key = match private_key.filter(|value| !value.expose_secret().trim().is_empty())
        {
            Some(value) => value,
            None => get_or_env_var(None, private_key_var)
                .map(SecretString::from)
                .map_err(to_pyvalue_err)?,
        };

        let private_key =
            EvmPrivateKey::new(private_key.expose_secret()).map_err(to_pyvalue_err)?;

        let funder = match funder.filter(|value| !value.trim().is_empty()) {
            Some(value) => value,
            None => get_or_env_var(None, funder_var).map_err(to_pyvalue_err)?,
        };

        let relayer_api_key = RelayerApiKey::resolve(relayer_api_key, relayer_api_key_address)
            .map_err(to_pyvalue_err)?;
        let proxy_url = proxy_url
            .filter(|value| !value.trim().is_empty())
            .map(ProxyUrl::parse)
            .transpose()
            .map_err(to_pyvalue_err)?;
        let mut inner = PolymarketPositionClient::new(
            &private_key,
            &funder,
            relayer_api_key,
            base_url_relayer,
            base_url_clob,
            timeout_secs,
            proxy_url,
        )
        .map_err(to_pyvalue_err)?;

        if let Some(url) = base_url_rpc {
            inner = inner.with_rpc_url(url);
        }

        Ok(Self {
            inner: Arc::new(inner),
        })
    }

    /// Splits `amount` pUSD into a complete set of outcome tokens.
    fn split_position<'py>(
        &self,
        py: Python<'py>,
        condition_id: String,
        #[pyo3(from_py_with = extract_amount)] amount: Decimal,
    ) -> PyResult<Bound<'py, PyAny>> {
        let inner = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let tx = inner
                .split_position(&condition_id, amount)
                .await
                .map_err(to_pyvalue_err)?;
            Ok(PyPolymarketPositionTransaction::from(tx))
        })
    }

    /// Merges `amount` complete sets of outcome tokens back into pUSD.
    fn merge_positions<'py>(
        &self,
        py: Python<'py>,
        condition_id: String,
        #[pyo3(from_py_with = extract_amount)] amount: Decimal,
    ) -> PyResult<Bound<'py, PyAny>> {
        let inner = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let tx = inner
                .merge_positions(&condition_id, amount)
                .await
                .map_err(to_pyvalue_err)?;
            Ok(PyPolymarketPositionTransaction::from(tx))
        })
    }

    /// Redeems both binary outcome balances for a resolved market.
    fn redeem_positions<'py>(
        &self,
        py: Python<'py>,
        condition_id: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        let inner = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let tx = inner
                .redeem_positions(&condition_id)
                .await
                .map_err(to_pyvalue_err)?;
            Ok(PyPolymarketPositionTransaction::from(tx))
        })
    }
}

fn extract_amount(value: &Bound<'_, PyAny>) -> PyResult<Decimal> {
    let amount: Decimal = value.extract()?;
    if !amount.into_pyobject(value.py())?.eq(value)? {
        return Err(to_pyvalue_err("amount cannot be represented exactly"));
    }

    Ok(amount)
}

#[cfg(test)]
mod tests {
    use pyo3::{exceptions::PyValueError, types::PyDict};
    use rstest::rstest;
    use rust_decimal_macros::dec;

    use super::*;

    #[rstest]
    #[case("split_position")]
    #[case("merge_positions")]
    fn test_position_amount_rejects_rounding(#[case] method: &str) {
        Python::initialize();
        Python::attach(|py| {
            let kwargs = PyDict::new(py);
            kwargs
                .set_item(
                    "private_key",
                    "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80",
                )
                .unwrap();
            kwargs
                .set_item("funder", "0x1111111111111111111111111111111111111111")
                .unwrap();
            kwargs
                .set_item("relayer_api_key", "dummy-relayer-key")
                .unwrap();
            kwargs
                .set_item(
                    "relayer_api_key_address",
                    "0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266",
                )
                .unwrap();
            let client = py
                .get_type::<PyPolymarketPositionClient>()
                .call((), Some(&kwargs))
                .unwrap();
            let amount = py
                .import("decimal")
                .unwrap()
                .getattr("Decimal")
                .unwrap()
                .call1(("1.00000000000000000000000000001",))
                .unwrap();
            let error = client
                .call_method1(
                    method,
                    (
                        "0xcccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc",
                        amount,
                    ),
                )
                .unwrap_err();
            assert!(error.is_instance_of::<PyValueError>(py));
            assert!(
                error
                    .to_string()
                    .contains("amount cannot be represented exactly")
            );
        });
    }

    #[rstest]
    #[case("1.234567", dec!(1.234567))]
    #[case("1.00000000000000000000000000000", dec!(1))]
    #[case("1E-6", dec!(0.000001))]
    #[case("1E+20", dec!(100000000000000000000))]
    fn test_extract_amount_preserves_exact_values(#[case] input: &str, #[case] expected: Decimal) {
        Python::initialize();
        Python::attach(|py| {
            let amount = py
                .import("decimal")
                .unwrap()
                .getattr("Decimal")
                .unwrap()
                .call1((input,))
                .unwrap();
            assert_eq!(extract_amount(&amount).unwrap(), expected);
        });
    }
}
