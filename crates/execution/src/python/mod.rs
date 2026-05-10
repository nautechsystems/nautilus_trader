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

pub mod config;
pub mod fee;
pub mod fill;
pub mod latency;
pub mod reconciliation;

use pyo3::prelude::*;

/// Loaded as `nautilus_pyo3.execution`.
///
/// # Errors
///
/// Returns a `PyErr` if registering any module components fails.
#[pymodule]
pub fn execution(_: Python<'_>, m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(
        reconciliation::py_adjust_fills_for_partial_window,
        m
    )?)?;
    m.add_function(wrap_pyfunction!(
        reconciliation::py_calculate_reconciliation_price,
        m
    )?)?;
    m.add_function(wrap_pyfunction!(
        reconciliation::py_create_inferred_reconciliation_trade_id,
        m
    )?)?;
    m.add_function(wrap_pyfunction!(
        reconciliation::py_create_position_reconciliation_venue_order_id,
        m
    )?)?;
    m.add_class::<crate::engine::config::ExecutionEngineConfig>()?;
    m.add_class::<crate::order_emulator::config::OrderEmulatorConfig>()?;
    m.add_class::<crate::models::fee::FixedFeeModel>()?;
    m.add_class::<crate::models::fee::MakerTakerFeeModel>()?;
    m.add_class::<crate::models::fee::PerContractFeeModel>()?;
    m.add_class::<crate::models::fill::DefaultFillModel>()?;
    m.add_class::<crate::models::fill::BestPriceFillModel>()?;
    m.add_class::<crate::models::fill::OneTickSlippageFillModel>()?;
    m.add_class::<crate::models::fill::ProbabilisticFillModel>()?;
    m.add_class::<crate::models::fill::TwoTierFillModel>()?;
    m.add_class::<crate::models::fill::ThreeTierFillModel>()?;
    m.add_class::<crate::models::fill::LimitOrderPartialFillModel>()?;
    m.add_class::<crate::models::fill::SizeAwareFillModel>()?;
    m.add_class::<crate::models::fill::CompetitionAwareFillModel>()?;
    m.add_class::<crate::models::fill::VolumeSensitiveFillModel>()?;
    m.add_class::<crate::models::fill::MarketHoursFillModel>()?;
    m.add_class::<crate::models::latency::StaticLatencyModel>()?;
    Ok(())
}
