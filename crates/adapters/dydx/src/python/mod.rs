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

//! Python bindings from `pyo3`.

pub mod enums;

use pyo3::prelude::*;

use crate::http::models::{
    DydxCandleResolution, DydxFillType, DydxFundingOrderSide, DydxLiquidity, DydxMarketStatus,
    DydxMarketType, DydxOrderSide, DydxOrderStatus, DydxOrderType, DydxPositionSide,
    DydxPositionStatus, DydxTimeInForce, DydxTradeType, DydxTradingRewardAggregationPeriod,
    DydxTransferType,
};

/// Loaded as `nautilus_pyo3.dydx`.
///
/// # Errors
///
/// Returns an error if any bindings fail to register with the Python module.
#[pymodule]
pub fn dydx(_: Python<'_>, m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add("__package__", "nautilus_trader.core.nautilus_pyo3.dydx")?;

    // Market data enums
    m.add_class::<DydxMarketStatus>()?;
    m.add_class::<DydxTradeType>()?;
    m.add_class::<DydxCandleResolution>()?;

    // Position enums
    m.add_class::<DydxPositionStatus>()?;
    m.add_class::<DydxPositionSide>()?;

    // Order enums
    m.add_class::<DydxOrderSide>()?;
    m.add_class::<DydxOrderType>()?;
    m.add_class::<DydxOrderStatus>()?;
    m.add_class::<DydxTimeInForce>()?;

    // Fill enums
    m.add_class::<DydxLiquidity>()?;
    m.add_class::<DydxFillType>()?;
    m.add_class::<DydxMarketType>()?;

    // Transfer enums
    m.add_class::<DydxTransferType>()?;

    // Funding enums
    m.add_class::<DydxFundingOrderSide>()?;

    // Rewards enums
    m.add_class::<DydxTradingRewardAggregationPeriod>()?;

    Ok(())
}
