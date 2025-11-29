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
use serde_json;
use ustr::Ustr;

use crate::{
    common::enums::{
        BybitOrderSide, BybitOrderType, BybitProductType, BybitTimeInForce, BybitTriggerType,
    },
    websocket::{error::BybitWsError, messages},
};

/// Parameters for placing an order via WebSocket.
#[pyclass]
#[derive(Clone, Debug)]
pub struct BybitWsPlaceOrderParams {
    #[pyo3(get, set)]
    pub category: BybitProductType,
    #[pyo3(get, set)]
    pub symbol: String,
    #[pyo3(get, set)]
    pub side: String,
    #[pyo3(get, set)]
    pub order_type: String,
    #[pyo3(get, set)]
    pub qty: String,
    #[pyo3(get, set)]
    pub is_leverage: Option<i32>,
    #[pyo3(get, set)]
    pub market_unit: Option<String>,
    #[pyo3(get, set)]
    pub price: Option<String>,
    #[pyo3(get, set)]
    pub time_in_force: Option<String>,
    #[pyo3(get, set)]
    pub order_link_id: Option<String>,
    #[pyo3(get, set)]
    pub reduce_only: Option<bool>,
    #[pyo3(get, set)]
    pub close_on_trigger: Option<bool>,
    #[pyo3(get, set)]
    pub trigger_price: Option<String>,
    #[pyo3(get, set)]
    pub trigger_by: Option<String>,
    #[pyo3(get, set)]
    pub trigger_direction: Option<i32>,
    #[pyo3(get, set)]
    pub tpsl_mode: Option<String>,
    #[pyo3(get, set)]
    pub take_profit: Option<String>,
    #[pyo3(get, set)]
    pub stop_loss: Option<String>,
    #[pyo3(get, set)]
    pub tp_trigger_by: Option<String>,
    #[pyo3(get, set)]
    pub sl_trigger_by: Option<String>,
    #[pyo3(get, set)]
    pub sl_trigger_price: Option<String>,
    #[pyo3(get, set)]
    pub tp_trigger_price: Option<String>,
    #[pyo3(get, set)]
    pub sl_order_type: Option<String>,
    #[pyo3(get, set)]
    pub tp_order_type: Option<String>,
    #[pyo3(get, set)]
    pub sl_limit_price: Option<String>,
    #[pyo3(get, set)]
    pub tp_limit_price: Option<String>,
}

#[pymethods]
impl BybitWsPlaceOrderParams {
    #[new]
    #[allow(clippy::too_many_arguments)]
    fn py_new(
        category: BybitProductType,
        symbol: String,
        side: String,
        order_type: String,
        qty: String,
        is_leverage: Option<i32>,
        market_unit: Option<String>,
        price: Option<String>,
        time_in_force: Option<String>,
        order_link_id: Option<String>,
        reduce_only: Option<bool>,
        close_on_trigger: Option<bool>,
        trigger_price: Option<String>,
        trigger_by: Option<String>,
        trigger_direction: Option<i32>,
        tpsl_mode: Option<String>,
        take_profit: Option<String>,
        stop_loss: Option<String>,
        tp_trigger_by: Option<String>,
        sl_trigger_by: Option<String>,
        sl_trigger_price: Option<String>,
        tp_trigger_price: Option<String>,
        sl_order_type: Option<String>,
        tp_order_type: Option<String>,
        sl_limit_price: Option<String>,
        tp_limit_price: Option<String>,
    ) -> Self {
        Self {
            category,
            symbol,
            side,
            order_type,
            qty,
            is_leverage,
            market_unit,
            price,
            time_in_force,
            order_link_id,
            reduce_only,
            close_on_trigger,
            trigger_price,
            trigger_by,
            trigger_direction,
            tpsl_mode,
            take_profit,
            stop_loss,
            tp_trigger_by,
            sl_trigger_by,
            sl_trigger_price,
            tp_trigger_price,
            sl_order_type,
            tp_order_type,
            sl_limit_price,
            tp_limit_price,
        }
    }
}

impl TryFrom<BybitWsPlaceOrderParams> for messages::BybitWsPlaceOrderParams {
    type Error = BybitWsError;

    fn try_from(params: BybitWsPlaceOrderParams) -> Result<Self, Self::Error> {
        let side: BybitOrderSide =
            serde_json::from_str(&format!("\"{}\"", params.side)).map_err(|e| {
                BybitWsError::ClientError(format!("Invalid side '{}': {}", params.side, e))
            })?;
        let order_type: BybitOrderType =
            serde_json::from_str(&format!("\"{}\"", params.order_type)).map_err(|e| {
                BybitWsError::ClientError(format!(
                    "Invalid order_type '{}': {}",
                    params.order_type, e
                ))
            })?;

        let time_in_force = params
            .time_in_force
            .map(|v| {
                serde_json::from_str::<BybitTimeInForce>(&format!("\"{v}\"")).map_err(|e| {
                    BybitWsError::ClientError(format!("Invalid time_in_force '{v}': {e}"))
                })
            })
            .transpose()?;

        let trigger_by = params
            .trigger_by
            .map(|v| {
                serde_json::from_str::<BybitTriggerType>(&format!("\"{v}\"")).map_err(|e| {
                    BybitWsError::ClientError(format!("Invalid trigger_by '{v}': {e}"))
                })
            })
            .transpose()?;

        let tp_trigger_by = params
            .tp_trigger_by
            .map(|v| {
                serde_json::from_str::<BybitTriggerType>(&format!("\"{v}\"")).map_err(|e| {
                    BybitWsError::ClientError(format!("Invalid tp_trigger_by '{v}': {e}"))
                })
            })
            .transpose()?;

        let sl_trigger_by = params
            .sl_trigger_by
            .map(|v| {
                serde_json::from_str::<BybitTriggerType>(&format!("\"{v}\"")).map_err(|e| {
                    BybitWsError::ClientError(format!("Invalid sl_trigger_by '{v}': {e}"))
                })
            })
            .transpose()?;

        let sl_order_type = params
            .sl_order_type
            .map(|v| {
                serde_json::from_str::<BybitOrderType>(&format!("\"{v}\"")).map_err(|e| {
                    BybitWsError::ClientError(format!("Invalid sl_order_type '{v}': {e}"))
                })
            })
            .transpose()?;

        let tp_order_type = params
            .tp_order_type
            .map(|v| {
                serde_json::from_str::<BybitOrderType>(&format!("\"{v}\"")).map_err(|e| {
                    BybitWsError::ClientError(format!("Invalid tp_order_type '{v}': {e}"))
                })
            })
            .transpose()?;

        Ok(Self {
            category: params.category,
            symbol: Ustr::from(&params.symbol),
            side,
            order_type,
            qty: params.qty,
            is_leverage: params.is_leverage,
            market_unit: params.market_unit,
            price: params.price,
            time_in_force,
            order_link_id: params.order_link_id,
            reduce_only: params.reduce_only,
            close_on_trigger: params.close_on_trigger,
            trigger_price: params.trigger_price,
            trigger_by,
            trigger_direction: params.trigger_direction,
            tpsl_mode: params.tpsl_mode,
            take_profit: params.take_profit,
            stop_loss: params.stop_loss,
            tp_trigger_by,
            sl_trigger_by,
            sl_trigger_price: params.sl_trigger_price,
            tp_trigger_price: params.tp_trigger_price,
            sl_order_type,
            tp_order_type,
            sl_limit_price: params.sl_limit_price,
            tp_limit_price: params.tp_limit_price,
        })
    }
}

impl From<messages::BybitWsPlaceOrderParams> for BybitWsPlaceOrderParams {
    fn from(params: messages::BybitWsPlaceOrderParams) -> Self {
        let side = serde_json::to_string(&params.side)
            .expect("Failed to serialize BybitOrderSide")
            .trim_matches('"')
            .to_string();
        let order_type = serde_json::to_string(&params.order_type)
            .expect("Failed to serialize BybitOrderType")
            .trim_matches('"')
            .to_string();
        let time_in_force = params.time_in_force.map(|v| {
            serde_json::to_string(&v)
                .expect("Failed to serialize BybitTimeInForce")
                .trim_matches('"')
                .to_string()
        });
        let trigger_by = params.trigger_by.map(|v| {
            serde_json::to_string(&v)
                .expect("Failed to serialize BybitTriggerType")
                .trim_matches('"')
                .to_string()
        });
        let tp_trigger_by = params.tp_trigger_by.map(|v| {
            serde_json::to_string(&v)
                .expect("Failed to serialize BybitTriggerType")
                .trim_matches('"')
                .to_string()
        });
        let sl_trigger_by = params.sl_trigger_by.map(|v| {
            serde_json::to_string(&v)
                .expect("Failed to serialize BybitTriggerType")
                .trim_matches('"')
                .to_string()
        });
        let sl_order_type = params.sl_order_type.map(|v| {
            serde_json::to_string(&v)
                .expect("Failed to serialize BybitOrderType")
                .trim_matches('"')
                .to_string()
        });
        let tp_order_type = params.tp_order_type.map(|v| {
            serde_json::to_string(&v)
                .expect("Failed to serialize BybitOrderType")
                .trim_matches('"')
                .to_string()
        });

        Self {
            category: params.category,
            symbol: params.symbol.to_string(),
            side,
            order_type,
            qty: params.qty,
            is_leverage: params.is_leverage,
            market_unit: params.market_unit,
            price: params.price,
            time_in_force,
            order_link_id: params.order_link_id,
            reduce_only: params.reduce_only,
            close_on_trigger: params.close_on_trigger,
            trigger_price: params.trigger_price,
            trigger_by,
            trigger_direction: params.trigger_direction,
            tpsl_mode: params.tpsl_mode,
            take_profit: params.take_profit,
            stop_loss: params.stop_loss,
            tp_trigger_by,
            sl_trigger_by,
            sl_trigger_price: params.sl_trigger_price,
            tp_trigger_price: params.tp_trigger_price,
            sl_order_type,
            tp_order_type,
            sl_limit_price: params.sl_limit_price,
            tp_limit_price: params.tp_limit_price,
        }
    }
}

/// Parameters for amending an order via WebSocket.
#[pyclass]
#[derive(Clone, Debug)]
pub struct BybitWsAmendOrderParams {
    #[pyo3(get, set)]
    pub category: BybitProductType,
    #[pyo3(get, set)]
    pub symbol: String,
    #[pyo3(get, set)]
    pub order_id: Option<String>,
    #[pyo3(get, set)]
    pub order_link_id: Option<String>,
    #[pyo3(get, set)]
    pub qty: Option<String>,
    #[pyo3(get, set)]
    pub price: Option<String>,
    #[pyo3(get, set)]
    pub trigger_price: Option<String>,
    #[pyo3(get, set)]
    pub take_profit: Option<String>,
    #[pyo3(get, set)]
    pub stop_loss: Option<String>,
    #[pyo3(get, set)]
    pub tp_trigger_by: Option<String>,
    #[pyo3(get, set)]
    pub sl_trigger_by: Option<String>,
}

#[pymethods]
impl BybitWsAmendOrderParams {
    #[new]
    #[allow(clippy::too_many_arguments)]
    fn py_new(
        category: BybitProductType,
        symbol: String,
        order_id: Option<String>,
        order_link_id: Option<String>,
        qty: Option<String>,
        price: Option<String>,
        trigger_price: Option<String>,
        take_profit: Option<String>,
        stop_loss: Option<String>,
        tp_trigger_by: Option<String>,
        sl_trigger_by: Option<String>,
    ) -> Self {
        Self {
            category,
            symbol,
            order_id,
            order_link_id,
            qty,
            price,
            trigger_price,
            take_profit,
            stop_loss,
            tp_trigger_by,
            sl_trigger_by,
        }
    }
}

impl TryFrom<BybitWsAmendOrderParams> for messages::BybitWsAmendOrderParams {
    type Error = BybitWsError;

    fn try_from(params: BybitWsAmendOrderParams) -> Result<Self, Self::Error> {
        let tp_trigger_by = params
            .tp_trigger_by
            .map(|v| {
                serde_json::from_str::<BybitTriggerType>(&format!("\"{v}\"")).map_err(|e| {
                    BybitWsError::ClientError(format!("Invalid tp_trigger_by '{v}': {e}"))
                })
            })
            .transpose()?;

        let sl_trigger_by = params
            .sl_trigger_by
            .map(|v| {
                serde_json::from_str::<BybitTriggerType>(&format!("\"{v}\"")).map_err(|e| {
                    BybitWsError::ClientError(format!("Invalid sl_trigger_by '{v}': {e}"))
                })
            })
            .transpose()?;

        Ok(Self {
            category: params.category,
            symbol: Ustr::from(&params.symbol),
            order_id: params.order_id,
            order_link_id: params.order_link_id,
            qty: params.qty,
            price: params.price,
            trigger_price: params.trigger_price,
            take_profit: params.take_profit,
            stop_loss: params.stop_loss,
            tp_trigger_by,
            sl_trigger_by,
        })
    }
}

impl From<messages::BybitWsAmendOrderParams> for BybitWsAmendOrderParams {
    fn from(params: messages::BybitWsAmendOrderParams) -> Self {
        let tp_trigger_by = params.tp_trigger_by.map(|v| {
            serde_json::to_string(&v)
                .expect("Failed to serialize BybitTriggerType")
                .trim_matches('"')
                .to_string()
        });
        let sl_trigger_by = params.sl_trigger_by.map(|v| {
            serde_json::to_string(&v)
                .expect("Failed to serialize BybitTriggerType")
                .trim_matches('"')
                .to_string()
        });

        Self {
            category: params.category,
            symbol: params.symbol.to_string(),
            order_id: params.order_id,
            order_link_id: params.order_link_id,
            qty: params.qty,
            price: params.price,
            trigger_price: params.trigger_price,
            take_profit: params.take_profit,
            stop_loss: params.stop_loss,
            tp_trigger_by,
            sl_trigger_by,
        }
    }
}
