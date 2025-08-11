// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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

use nautilus_core::{
    consts::NAUTILUS_TRADER,
    python::{IntoPyObjectNautilusExt, to_pyvalue_err},
    time::get_atomic_clock_realtime,
};
use nautilus_model::{
    data::trade::TradeTick,
    enums::{OrderSide, OrderType},
    identifiers::{ClientOrderId, Symbol, VenueOrderId},
    reports::{fill::FillReport, order::OrderStatusReport, position::PositionStatusReport},
    types::{price::Price, quantity::Quantity},
};
use pyo3::{prelude::*, types::PyList};

use crate::http::{
    client::BitmexHttpClient,
    parse::{parse_fill_report, parse_order_status_report, parse_position_report, parse_trade},
    query::{
        DeleteOrderParamsBuilder, GetExecutionParamsBuilder, GetOrderParamsBuilder,
        GetPositionParamsBuilder, GetTradeParamsBuilder, PostOrderParamsBuilder,
        PutOrderParamsBuilder,
    },
};

#[pymethods]
impl BitmexHttpClient {
    #[new]
    #[pyo3(signature = (api_key=None, api_secret=None, base_url=None))]
    fn py_new(
        api_key: Option<&str>,
        api_secret: Option<&str>,
        base_url: Option<&str>,
    ) -> PyResult<Self> {
        match (api_key, api_secret) {
            (Some(api_key), Some(api_secret)) => Ok(BitmexHttpClient::with_credentials(
                api_key, api_secret, base_url,
            )),
            (None, None) => Ok(BitmexHttpClient::new(base_url)),
            _ => Err(to_pyvalue_err("Invalid credentials")),
        }
    }

    #[pyo3(name = "get_instruments")]
    fn py_get_instruments<'py>(&self, _py: Python<'py>, _active_only: bool) -> PyResult<usize> {
        // TODO: Implement proper Python async pattern for PyO3 0.25+
        tracing::warn!("BitMEX get_instruments Python method not yet implemented");
        Ok(0)
    }

    #[pyo3(name = "get_instrument")]
    fn py_get_instrument<'py>(&self, _py: Python<'py>, _symbol: &Symbol) -> PyResult<usize> {
        // TODO: Implement proper Python async pattern for PyO3 0.25+
        tracing::warn!("BitMEX get_instrument Python method not yet implemented");
        Ok(0)
    }

    #[pyo3(name = "get_trades")]
    #[pyo3(signature = (symbol=None))]
    fn py_get_trades<'py>(
        &self,
        py: Python<'py>,
        symbol: Option<String>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();
        let mut params = GetTradeParamsBuilder::default();
        if let Some(symbol) = symbol {
            params.symbol(symbol);
        }
        let params = params.build().map_err(to_pyvalue_err)?;
        let price_precision = 1; // TBD
        let now = get_atomic_clock_realtime().get_time_ns();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let resp = client.get_trades(params).await.map_err(to_pyvalue_err)?;

            let mut trades: Vec<TradeTick> = Vec::new();
            for trade in resp {
                match parse_trade(trade, price_precision, now) {
                    Ok(trade) => trades.push(trade),
                    Err(e) => tracing::error!("Failed to parse trade: {e}"),
                }
            }

            Python::with_gil(|py| {
                let py_trades: PyResult<Vec<_>> = trades
                    .into_iter()
                    .map(|trade| Ok(trade.into_py_any_unwrap(py)))
                    .collect();
                let pylist = PyList::new(py, py_trades?).unwrap().into_any().unbind();
                Ok(pylist)
            })
        })
    }

    #[pyo3(name = "get_order_reports")]
    #[pyo3(signature = (symbol=None))]
    fn py_get_order_reports<'py>(
        &self,
        py: Python<'py>,
        symbol: Option<String>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();
        let mut params = GetOrderParamsBuilder::default();
        if let Some(symbol) = symbol {
            params.symbol(symbol);
        }
        let params = params.build().map_err(to_pyvalue_err)?;
        let price_precision = 1; // TBD

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let resp = client.get_orders(params).await.map_err(to_pyvalue_err)?;

            let mut reports: Vec<OrderStatusReport> = Vec::new();
            for order in resp {
                match parse_order_status_report(order, price_precision) {
                    Ok(report) => reports.push(report),
                    Err(e) => tracing::error!("Failed to parse order status report: {e}"),
                }
            }

            Python::with_gil(|py| {
                let py_reports: PyResult<Vec<_>> = reports
                    .into_iter()
                    .map(|report| Ok(report.into_py_any_unwrap(py)))
                    .collect();
                let pylist = PyList::new(py, py_reports?).unwrap().into_any().unbind();
                Ok(pylist)
            })
        })
    }

    #[pyo3(name = "get_fill_reports")]
    #[pyo3(signature = (symbol=None))]
    fn py_get_fill_reports<'py>(
        &self,
        py: Python<'py>,
        symbol: Option<String>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();
        let mut params = GetExecutionParamsBuilder::default();
        if let Some(symbol) = symbol {
            params.symbol(symbol);
        }
        let params = params.build().map_err(to_pyvalue_err)?;
        let price_precision = 1; // TBD

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let resp = client
                .get_executions(params)
                .await
                .map_err(to_pyvalue_err)?;

            let mut reports: Vec<FillReport> = Vec::new();
            for exec in resp {
                match parse_fill_report(exec, price_precision) {
                    Ok(report) => reports.push(report),
                    Err(e) => tracing::error!("Failed to parse fill report: {e}"),
                }
            }

            Python::with_gil(|py| {
                let py_reports: PyResult<Vec<_>> = reports
                    .into_iter()
                    .map(|report| Ok(report.into_py_any_unwrap(py)))
                    .collect();
                let pylist = PyList::new(py, py_reports?).unwrap().into_any().unbind();
                Ok(pylist)
            })
        })
    }

    #[pyo3(name = "get_position_reports")]
    fn py_get_position_reports<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();
        let params = GetPositionParamsBuilder::default();
        let params = params.build().map_err(to_pyvalue_err)?;

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let resp = client.get_positions(params).await.map_err(to_pyvalue_err)?;

            let mut reports: Vec<PositionStatusReport> = Vec::new();
            for pos in resp {
                match parse_position_report(pos) {
                    Ok(report) => reports.push(report),
                    Err(e) => tracing::error!("Failed to parse position report: {e}"),
                }
            }

            Python::with_gil(|py| {
                let py_reports: PyResult<Vec<_>> = reports
                    .into_iter()
                    .map(|report| Ok(report.into_py_any_unwrap(py)))
                    .collect();
                let pylist = PyList::new(py, py_reports?).unwrap().into_any().unbind();
                Ok(pylist)
            })
        })
    }

    #[pyo3(name = "submit_order")]
    #[pyo3(signature = (symbol, client_order_id, order_type, order_side, quantity, price = None, trigger_price = None, display_qty = None))]
    #[allow(clippy::too_many_arguments)]
    fn py_submit_order<'py>(
        &self,
        py: Python<'py>,
        symbol: &Symbol,
        client_order_id: &ClientOrderId,
        order_type: OrderType,
        order_side: OrderSide,
        quantity: Quantity,
        price: Option<Price>,
        trigger_price: Option<Price>,
        display_qty: Option<Quantity>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();
        let mut params = PostOrderParamsBuilder::default();
        params.text(NAUTILUS_TRADER);
        params.symbol(symbol.to_string());
        params.cl_ord_id(client_order_id.to_string());
        params.ord_type(crate::enums::OrderType::from_nautilus(order_type));
        params.side(crate::enums::Side::from_nautilus_order_side(order_side));
        params.order_qty(quantity.as_f64() as u32); // TODO: Improve Quantity

        if let Some(price) = price {
            params.price(price.as_f64());
        }
        if let Some(trigger_price) = trigger_price {
            params.stop_px(trigger_price.as_f64());
        }
        if let Some(display_qty) = display_qty {
            params.display_qty(display_qty.as_f64() as u32); // TODO: Improve Quantity
        }
        let params = params.build().map_err(to_pyvalue_err)?;

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.place_order(params).await.map_err(to_pyvalue_err)?;
            // TODO: Logging and error handling
            Ok(())
        })
    }

    #[pyo3(name = "cancel_order")]
    #[pyo3(signature = (client_order_id=None, venue_order_id=None))]
    fn py_cancel_order<'py>(
        &self,
        py: Python<'py>,
        client_order_id: Option<ClientOrderId>,
        venue_order_id: Option<VenueOrderId>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();
        let mut params = DeleteOrderParamsBuilder::default();
        if let Some(client_order_id) = client_order_id {
            params.cl_ord_id(vec![client_order_id.to_string()]);
        }
        if let Some(venue_order_id) = venue_order_id {
            params.order_id(vec![venue_order_id.to_string()]);
        }
        let params = params.build().map_err(to_pyvalue_err)?;

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.cancel_orders(params).await.map_err(to_pyvalue_err)?;
            // TODO: Logging and error handling
            Ok(())
        })
    }

    #[pyo3(name = "cancel_orders")]
    #[pyo3(signature = (client_order_ids=None, venue_order_ids=None))]
    fn py_cancel_orders<'py>(
        &self,
        py: Python<'py>,
        client_order_ids: Option<Vec<ClientOrderId>>,
        venue_order_ids: Option<Vec<VenueOrderId>>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();
        let mut params = DeleteOrderParamsBuilder::default();
        if let Some(client_order_ids) = client_order_ids {
            params.cl_ord_id(
                client_order_ids
                    .iter()
                    .map(|x| x.to_string())
                    .collect::<Vec<_>>(),
            );
        }
        if let Some(venue_order_ids) = venue_order_ids {
            params.cl_ord_id(
                venue_order_ids
                    .iter()
                    .map(|x| x.to_string())
                    .collect::<Vec<_>>(),
            );
        }
        let params = params.build().map_err(to_pyvalue_err)?;

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.cancel_orders(params).await.map_err(to_pyvalue_err)?;
            // TODO: Logging and error handling
            Ok(())
        })
    }

    #[pyo3(name = "modify_order")]
    #[pyo3(signature = (client_order_id=None, venue_order_id=None, quantity=None, leaves_qty=None, price=None, trigger_price=None))]
    #[allow(clippy::too_many_arguments)]
    fn py_modify_order<'py>(
        &self,
        py: Python<'py>,
        client_order_id: Option<ClientOrderId>,
        venue_order_id: Option<VenueOrderId>,
        quantity: Option<Quantity>,
        leaves_qty: Option<Quantity>,
        price: Option<Price>,
        trigger_price: Option<Price>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();
        let mut params = PutOrderParamsBuilder::default();
        if let Some(client_order_id) = client_order_id {
            params.cl_ord_id(client_order_id.to_string());
        }
        if let Some(venue_order_id) = venue_order_id {
            params.order_id(venue_order_id.to_string());
        }
        if let Some(quantity) = quantity {
            params.order_qty(quantity.as_f64() as u32); // TODO: Improve quantity
        }
        if let Some(leaves_qty) = leaves_qty {
            params.leaves_qty(leaves_qty.as_f64() as u32); // TODO: Improve quantity
        }
        if let Some(price) = price {
            params.price(price.as_f64());
        }
        if let Some(trigger_price) = trigger_price {
            params.stop_px(trigger_price.as_f64());
        }
        let params = params.build().map_err(to_pyvalue_err)?;

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.amend_order(params).await.map_err(to_pyvalue_err)?;
            // TODO: Logging and error handling
            Ok(())
        })
    }
}
