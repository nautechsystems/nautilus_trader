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

//! Python data model bindings for Delta Exchange.

use pyo3::prelude::*;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::http::models::{
    DeltaExchangeAsset, DeltaExchangeBalance, DeltaExchangeCandle, DeltaExchangeFill,
    DeltaExchangeOrder, DeltaExchangeOrderBook, DeltaExchangePosition, DeltaExchangeProduct,
    DeltaExchangeTicker, DeltaExchangeTrade,
};

/// Python wrapper for DeltaExchangeAsset.
#[pyclass(name = "DeltaExchangeAsset")]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PyDeltaExchangeAsset {
    #[pyo3(get)]
    pub id: u64,
    #[pyo3(get)]
    pub symbol: String,
    #[pyo3(get)]
    pub name: String,
    #[pyo3(get)]
    pub precision: u8,
    #[pyo3(get)]
    pub deposit_status: String,
    #[pyo3(get)]
    pub withdrawal_status: String,
    #[pyo3(get)]
    pub base_withdrawal_fee: String,
    #[pyo3(get)]
    pub min_withdrawal_amount: String,
}

#[pymethods]
impl PyDeltaExchangeAsset {
    #[new]
    fn py_new(
        id: u64,
        symbol: String,
        name: String,
        precision: u8,
        deposit_status: String,
        withdrawal_status: String,
        base_withdrawal_fee: String,
        min_withdrawal_amount: String,
    ) -> Self {
        Self {
            id,
            symbol,
            name,
            precision,
            deposit_status,
            withdrawal_status,
            base_withdrawal_fee,
            min_withdrawal_amount,
        }
    }

    fn __str__(&self) -> String {
        format!("DeltaExchangeAsset(id={}, symbol={})", self.id, self.symbol)
    }

    fn __repr__(&self) -> String {
        self.__str__()
    }
}

impl From<DeltaExchangeAsset> for PyDeltaExchangeAsset {
    fn from(asset: DeltaExchangeAsset) -> Self {
        Self {
            id: asset.id,
            symbol: asset.symbol,
            name: asset.name,
            precision: asset.precision,
            deposit_status: asset.deposit_status,
            withdrawal_status: asset.withdrawal_status,
            base_withdrawal_fee: asset.base_withdrawal_fee.to_string(),
            min_withdrawal_amount: asset.min_withdrawal_amount.to_string(),
        }
    }
}

/// Python wrapper for DeltaExchangeProduct.
#[pyclass(name = "DeltaExchangeProduct")]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PyDeltaExchangeProduct {
    #[pyo3(get)]
    pub id: u64,
    #[pyo3(get)]
    pub symbol: String,
    #[pyo3(get)]
    pub description: String,
    #[pyo3(get)]
    pub created_at: String,
    #[pyo3(get)]
    pub updated_at: String,
    #[pyo3(get)]
    pub settlement_time: Option<String>,
    #[pyo3(get)]
    pub notional_type: String,
    #[pyo3(get)]
    pub impact_size: String,
    #[pyo3(get)]
    pub initial_margin: String,
    #[pyo3(get)]
    pub maintenance_margin: String,
    #[pyo3(get)]
    pub contract_value: String,
    #[pyo3(get)]
    pub contract_unit_currency: String,
    #[pyo3(get)]
    pub tick_size: String,
    #[pyo3(get)]
    pub product_type: String,
    #[pyo3(get)]
    pub pricing_source: String,
    #[pyo3(get)]
    pub strike_price: Option<String>,
    #[pyo3(get)]
    pub settlement_price: Option<String>,
    #[pyo3(get)]
    pub launch_time: Option<String>,
    #[pyo3(get)]
    pub state: String,
    #[pyo3(get)]
    pub trading_status: String,
    #[pyo3(get)]
    pub max_leverage_notional: String,
    #[pyo3(get)]
    pub default_leverage: String,
    #[pyo3(get)]
    pub initial_margin_scaling_factor: String,
    #[pyo3(get)]
    pub maintenance_margin_scaling_factor: String,
    #[pyo3(get)]
    pub taker_commission_rate: String,
    #[pyo3(get)]
    pub maker_commission_rate: String,
    #[pyo3(get)]
    pub liquidation_penalty_factor: String,
    #[pyo3(get)]
    pub contract_type: String,
    #[pyo3(get)]
    pub position_size_limit: u64,
    #[pyo3(get)]
    pub basis_factor_max_limit: String,
    #[pyo3(get)]
    pub is_quanto: bool,
    #[pyo3(get)]
    pub funding_method: String,
    #[pyo3(get)]
    pub annualized_funding: String,
    #[pyo3(get)]
    pub price_band: String,
    #[pyo3(get)]
    pub underlying_asset: PyDeltaExchangeAsset,
    #[pyo3(get)]
    pub quoting_asset: PyDeltaExchangeAsset,
    #[pyo3(get)]
    pub settling_asset: PyDeltaExchangeAsset,
}

#[pymethods]
impl PyDeltaExchangeProduct {
    fn __str__(&self) -> String {
        format!("DeltaExchangeProduct(id={}, symbol={})", self.id, self.symbol)
    }

    fn __repr__(&self) -> String {
        self.__str__()
    }
}

impl From<DeltaExchangeProduct> for PyDeltaExchangeProduct {
    fn from(product: DeltaExchangeProduct) -> Self {
        Self {
            id: product.id,
            symbol: product.symbol,
            description: product.description,
            created_at: product.created_at,
            updated_at: product.updated_at,
            settlement_time: product.settlement_time,
            notional_type: product.notional_type,
            impact_size: product.impact_size.to_string(),
            initial_margin: product.initial_margin.to_string(),
            maintenance_margin: product.maintenance_margin.to_string(),
            contract_value: product.contract_value.to_string(),
            contract_unit_currency: product.contract_unit_currency,
            tick_size: product.tick_size.to_string(),
            product_type: product.product_type,
            pricing_source: product.pricing_source,
            strike_price: product.strike_price.map(|p| p.to_string()),
            settlement_price: product.settlement_price.map(|p| p.to_string()),
            launch_time: product.launch_time,
            state: product.state,
            trading_status: product.trading_status,
            max_leverage_notional: product.max_leverage_notional.to_string(),
            default_leverage: product.default_leverage.to_string(),
            initial_margin_scaling_factor: product.initial_margin_scaling_factor.to_string(),
            maintenance_margin_scaling_factor: product.maintenance_margin_scaling_factor.to_string(),
            taker_commission_rate: product.taker_commission_rate.to_string(),
            maker_commission_rate: product.maker_commission_rate.to_string(),
            liquidation_penalty_factor: product.liquidation_penalty_factor.to_string(),
            contract_type: product.contract_type,
            position_size_limit: product.position_size_limit,
            basis_factor_max_limit: product.basis_factor_max_limit.to_string(),
            is_quanto: product.is_quanto,
            funding_method: product.funding_method,
            annualized_funding: product.annualized_funding.to_string(),
            price_band: product.price_band.to_string(),
            underlying_asset: product.underlying_asset.into(),
            quoting_asset: product.quoting_asset.into(),
            settling_asset: product.settling_asset.into(),
        }
    }
}

/// Python wrapper for DeltaExchangeTicker.
#[pyclass(name = "DeltaExchangeTicker")]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PyDeltaExchangeTicker {
    #[pyo3(get)]
    pub symbol: String,
    #[pyo3(get)]
    pub price: String,
    #[pyo3(get)]
    pub size: String,
    #[pyo3(get)]
    pub bid: Option<String>,
    #[pyo3(get)]
    pub ask: Option<String>,
    #[pyo3(get)]
    pub volume: String,
    #[pyo3(get)]
    pub timestamp: u64,
}

#[pymethods]
impl PyDeltaExchangeTicker {
    fn __str__(&self) -> String {
        format!("DeltaExchangeTicker(symbol={}, price={})", self.symbol, self.price)
    }

    fn __repr__(&self) -> String {
        self.__str__()
    }
}

impl From<DeltaExchangeTicker> for PyDeltaExchangeTicker {
    fn from(ticker: DeltaExchangeTicker) -> Self {
        Self {
            symbol: ticker.symbol,
            price: ticker.price.to_string(),
            size: ticker.size.to_string(),
            bid: ticker.bid.map(|b| b.to_string()),
            ask: ticker.ask.map(|a| a.to_string()),
            volume: ticker.volume.to_string(),
            timestamp: ticker.timestamp,
        }
    }
}

/// Python wrapper for DeltaExchangeOrder.
#[pyclass(name = "DeltaExchangeOrder")]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PyDeltaExchangeOrder {
    #[pyo3(get)]
    pub id: u64,
    #[pyo3(get)]
    pub user_id: u64,
    #[pyo3(get)]
    pub size: String,
    #[pyo3(get)]
    pub unfilled_size: String,
    #[pyo3(get)]
    pub side: String,
    #[pyo3(get)]
    pub order_type: String,
    #[pyo3(get)]
    pub limit_price: Option<String>,
    #[pyo3(get)]
    pub stop_price: Option<String>,
    #[pyo3(get)]
    pub paid_commission: String,
    #[pyo3(get)]
    pub commission: String,
    #[pyo3(get)]
    pub reduce_only: bool,
    #[pyo3(get)]
    pub client_order_id: Option<String>,
    #[pyo3(get)]
    pub state: String,
    #[pyo3(get)]
    pub created_at: String,
    #[pyo3(get)]
    pub updated_at: String,
    #[pyo3(get)]
    pub product_id: u64,
}

#[pymethods]
impl PyDeltaExchangeOrder {
    fn __str__(&self) -> String {
        format!("DeltaExchangeOrder(id={}, side={}, size={})", self.id, self.side, self.size)
    }

    fn __repr__(&self) -> String {
        self.__str__()
    }
}

impl From<DeltaExchangeOrder> for PyDeltaExchangeOrder {
    fn from(order: DeltaExchangeOrder) -> Self {
        Self {
            id: order.id,
            user_id: order.user_id,
            size: order.size.to_string(),
            unfilled_size: order.unfilled_size.to_string(),
            side: order.side,
            order_type: order.order_type,
            limit_price: order.limit_price.map(|p| p.to_string()),
            stop_price: order.stop_price.map(|p| p.to_string()),
            paid_commission: order.paid_commission.to_string(),
            commission: order.commission.to_string(),
            reduce_only: order.reduce_only,
            client_order_id: order.client_order_id,
            state: order.state,
            created_at: order.created_at,
            updated_at: order.updated_at,
            product_id: order.product_id,
        }
    }
}

/// Python wrapper for DeltaExchangePosition.
#[pyclass(name = "DeltaExchangePosition")]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PyDeltaExchangePosition {
    #[pyo3(get)]
    pub user_id: u64,
    #[pyo3(get)]
    pub product_id: u64,
    #[pyo3(get)]
    pub product_symbol: String,
    #[pyo3(get)]
    pub size: String,
    #[pyo3(get)]
    pub entry_price: Option<String>,
    #[pyo3(get)]
    pub margin: String,
    #[pyo3(get)]
    pub liquidation_price: Option<String>,
    #[pyo3(get)]
    pub bankruptcy_price: Option<String>,
    #[pyo3(get)]
    pub adl_level: u8,
    #[pyo3(get)]
    pub unrealized_pnl: String,
    #[pyo3(get)]
    pub realized_pnl: String,
}

#[pymethods]
impl PyDeltaExchangePosition {
    fn __str__(&self) -> String {
        format!("DeltaExchangePosition(symbol={}, size={})", self.product_symbol, self.size)
    }

    fn __repr__(&self) -> String {
        self.__str__()
    }
}

impl From<DeltaExchangePosition> for PyDeltaExchangePosition {
    fn from(position: DeltaExchangePosition) -> Self {
        Self {
            user_id: position.user_id,
            product_id: position.product_id,
            product_symbol: position.product_symbol,
            size: position.size.to_string(),
            entry_price: position.entry_price.map(|p| p.to_string()),
            margin: position.margin.to_string(),
            liquidation_price: position.liquidation_price.map(|p| p.to_string()),
            bankruptcy_price: position.bankruptcy_price.map(|p| p.to_string()),
            adl_level: position.adl_level,
            unrealized_pnl: position.unrealized_pnl.to_string(),
            realized_pnl: position.realized_pnl.to_string(),
        }
    }
}
