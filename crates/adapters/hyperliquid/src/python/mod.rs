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

pub mod enums;
pub mod http;
pub mod urls;
pub mod websocket;

use nautilus_core::python::to_pyvalue_err;
use nautilus_model::identifiers::ClientOrderId;
use pyo3::{exceptions::PyRuntimeError, prelude::*};

use crate::{
    common::builder_fee::{
        BuilderFeeInfo, approve_from_env, revoke_from_env, verify_from_env_or_address,
    },
    http::models::Cloid,
};

/// Compute the cloid (hex hash) from a client_order_id.
///
/// The cloid is a keccak256 hash of the client_order_id, truncated to 16 bytes,
/// represented as a hex string with `0x` prefix.
#[pyfunction]
#[pyo3(name = "hyperliquid_cloid_from_client_order_id")]
fn py_hyperliquid_cloid_from_client_order_id(client_order_id: ClientOrderId) -> String {
    Cloid::from_client_order_id(client_order_id).to_hex()
}

/// Extract product type from a Hyperliquid symbol.
///
/// # Errors
///
/// Returns an error if the symbol does not contain a valid Hyperliquid product type suffix.
#[pyfunction]
#[pyo3(name = "hyperliquid_product_type_from_symbol")]
fn py_hyperliquid_product_type_from_symbol(
    symbol: &str,
) -> PyResult<crate::common::HyperliquidProductType> {
    crate::common::HyperliquidProductType::from_symbol(symbol).map_err(to_pyvalue_err)
}

/// Get Hyperliquid builder fee configuration information.
///
/// Returns a JSON string with the builder address and fee rates.
#[pyfunction]
#[pyo3(name = "get_hyperliquid_builder_fee_info")]
fn py_get_hyperliquid_builder_fee_info() -> PyResult<String> {
    let info = BuilderFeeInfo::new();
    serde_json::to_string(&info).map_err(to_pyvalue_err)
}

/// Print Hyperliquid builder fee configuration to stdout.
#[pyfunction]
#[pyo3(name = "print_hyperliquid_builder_fee_info")]
fn py_print_hyperliquid_builder_fee_info() {
    BuilderFeeInfo::new().print();
}

/// Approve the Nautilus builder fee for a wallet.
///
/// This signs an EIP-712 `ApproveBuilderFee` action and submits it to Hyperliquid.
/// The approval allows NautilusTrader to include builder fees on orders for this wallet.
///
/// This is a ONE-TIME setup step required before trading on Hyperliquid.
///
/// Reads private key from environment:
/// - Testnet: `HYPERLIQUID_TESTNET_PK`
/// - Mainnet: `HYPERLIQUID_PK`
///
/// Set `HYPERLIQUID_TESTNET=true` to use testnet.
///
/// # Returns
///
/// `true` if approval succeeded, `false` otherwise.
#[pyfunction]
#[pyo3(name = "approve_hyperliquid_builder_fee", signature = (non_interactive=false))]
fn py_approve_hyperliquid_builder_fee(non_interactive: bool) -> PyResult<bool> {
    std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| PyRuntimeError::new_err(format!("Failed to create runtime: {e}")))?;

        Ok(runtime.block_on(approve_from_env(non_interactive)))
    })
    .join()
    .map_err(|_| PyRuntimeError::new_err("Thread panicked"))?
}

/// Revoke the Nautilus builder fee approval for your wallet.
///
/// This signs an `ApproveBuilderFee` action with a 0% rate and submits it to Hyperliquid,
/// effectively revoking the builder's permission.
///
/// Reads private key from environment:
/// - Testnet: `HYPERLIQUID_TESTNET_PK`
/// - Mainnet: `HYPERLIQUID_PK`
///
/// Set `HYPERLIQUID_TESTNET=true` to use testnet.
///
/// WARNING: After revoking, you will not be able to trade on Hyperliquid via
/// NautilusTrader until you re-approve.
///
/// # Returns
///
/// `true` if revocation succeeded, `false` otherwise.
#[pyfunction]
#[pyo3(name = "revoke_hyperliquid_builder_fee", signature = (non_interactive=false))]
fn py_revoke_hyperliquid_builder_fee(non_interactive: bool) -> PyResult<bool> {
    std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| PyRuntimeError::new_err(format!("Failed to create runtime: {e}")))?;

        Ok(runtime.block_on(revoke_from_env(non_interactive)))
    })
    .join()
    .map_err(|_| PyRuntimeError::new_err("Thread panicked"))?
}

/// Verify the Nautilus builder fee approval status for a wallet.
///
/// Queries Hyperliquid's `maxBuilderFee` endpoint to check if the wallet
/// has approved the Nautilus builder fee at the required rate.
///
/// If `wallet_address` is provided, uses it directly. Otherwise reads private key
/// from environment to derive wallet address:
/// - Testnet: `HYPERLIQUID_TESTNET_PK`
/// - Mainnet: `HYPERLIQUID_PK`
///
/// Set `HYPERLIQUID_TESTNET=true` to use testnet.
///
/// # Returns
///
/// `true` if builder fee is approved at the required rate, `false` otherwise.
#[pyfunction]
#[pyo3(name = "verify_hyperliquid_builder_fee", signature = (wallet_address=None))]
fn py_verify_hyperliquid_builder_fee(wallet_address: Option<String>) -> PyResult<bool> {
    std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| PyRuntimeError::new_err(format!("Failed to create runtime: {e}")))?;

        Ok(runtime.block_on(verify_from_env_or_address(wallet_address)))
    })
    .join()
    .map_err(|_| PyRuntimeError::new_err("Thread panicked"))?
}

/// Loaded as `nautilus_pyo3.hyperliquid`.
#[pymodule]
pub fn hyperliquid(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add(
        "HYPERLIQUID_POST_ONLY_WOULD_MATCH",
        crate::common::consts::HYPERLIQUID_POST_ONLY_WOULD_MATCH,
    )?;
    m.add(
        "HYPERLIQUID_BUILDER_FEE_NOT_APPROVED",
        crate::common::consts::HYPERLIQUID_BUILDER_FEE_NOT_APPROVED,
    )?;
    m.add_class::<crate::http::HyperliquidHttpClient>()?;
    m.add_class::<crate::websocket::HyperliquidWebSocketClient>()?;
    m.add_class::<crate::common::enums::HyperliquidProductType>()?;
    m.add_class::<crate::common::enums::HyperliquidTpSl>()?;
    m.add_class::<crate::common::enums::HyperliquidConditionalOrderType>()?;
    m.add_class::<crate::common::enums::HyperliquidTrailingOffsetType>()?;
    m.add_function(wrap_pyfunction!(urls::py_get_hyperliquid_http_base_url, m)?)?;
    m.add_function(wrap_pyfunction!(urls::py_get_hyperliquid_ws_url, m)?)?;
    m.add_function(wrap_pyfunction!(
        py_hyperliquid_product_type_from_symbol,
        m
    )?)?;
    m.add_function(wrap_pyfunction!(
        py_hyperliquid_cloid_from_client_order_id,
        m
    )?)?;
    m.add_function(wrap_pyfunction!(py_get_hyperliquid_builder_fee_info, m)?)?;
    m.add_function(wrap_pyfunction!(py_print_hyperliquid_builder_fee_info, m)?)?;
    m.add_function(wrap_pyfunction!(py_approve_hyperliquid_builder_fee, m)?)?;
    m.add_function(wrap_pyfunction!(py_revoke_hyperliquid_builder_fee, m)?)?;
    m.add_function(wrap_pyfunction!(py_verify_hyperliquid_builder_fee, m)?)?;

    Ok(())
}
