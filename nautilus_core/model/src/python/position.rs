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

use nautilus_core::python::serialization::from_dict_pyo3;
use nautilus_core::python::to_pyvalue_err;
use pyo3::basic::CompareOp;
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList};
use rust_decimal::prelude::ToPrimitive;

use crate::enums::OrderSide;
use crate::events::order::filled::OrderFilled;
use crate::identifiers::client_order_id::ClientOrderId;
use crate::identifiers::symbol::Symbol;
use crate::identifiers::trade_id::TradeId;
use crate::identifiers::venue::Venue;
use crate::identifiers::venue_order_id::VenueOrderId;
use crate::instruments::crypto_future::CryptoFuture;
use crate::instruments::crypto_perpetual::CryptoPerpetual;
use crate::instruments::currency_pair::CurrencyPair;
use crate::instruments::equity::Equity;
use crate::instruments::futures_contract::FuturesContract;
use crate::instruments::options_contract::OptionsContract;
use crate::position::Position;
use crate::types::money::Money;
use crate::types::price::Price;
use crate::types::quantity::Quantity;

#[pymethods]
impl Position {
    #[new]
    fn py_new(instrument: PyObject, fill: OrderFilled, py: Python) -> PyResult<Self> {
        // Extract instrument from PyObject
        let instrument_type = instrument
            .getattr(py, "instrument_type")?
            .extract::<String>(py)?;
        if instrument_type == "CryptoFuture" {
            let instrument_rust = instrument.extract::<CryptoFuture>(py)?;
            Ok(Self::new(instrument_rust, fill).unwrap())
        } else if instrument_type == "CryptoPerpetual" {
            let instrument_rust = instrument.extract::<CryptoPerpetual>(py)?;
            Ok(Self::new(instrument_rust, fill).unwrap())
        } else if instrument_type == "CurrencyPair" {
            let instrument_rust = instrument.extract::<CurrencyPair>(py)?;
            Ok(Self::new(instrument_rust, fill).unwrap())
        } else if instrument_type == "Equity" {
            let instrument_rust = instrument.extract::<Equity>(py)?;
            Ok(Self::new(instrument_rust, fill).unwrap())
        } else if instrument_type == "FuturesContract" {
            let instrument_rust = instrument.extract::<FuturesContract>(py)?;
            Ok(Self::new(instrument_rust, fill).unwrap())
        } else if instrument_type == "OptionsContract" {
            let instrument_rust = instrument.extract::<OptionsContract>(py)?;
            Ok(Self::new(instrument_rust, fill).unwrap())
        } else {
            Err(to_pyvalue_err("Unsupported instrument type"))
        }
    }

    fn __richcmp__(&self, other: &Self, op: CompareOp, py: Python<'_>) -> Py<PyAny> {
        match op {
            CompareOp::Eq => self.eq(other).into_py(py),
            CompareOp::Ne => self.ne(other).into_py(py),
            _ => py.NotImplemented(),
        }
    }

    fn __str__(&self) -> String {
        self.to_string()
    }

    fn __repr__(&self) -> String {
        self.to_string()
    }

    #[getter]
    #[pyo3(name = "symbol")]
    fn py_symbol(&self) -> Symbol {
        self.symbol()
    }

    #[getter]
    #[pyo3(name = "venue")]
    fn py_venue(&self) -> Venue {
        self.venue()
    }

    #[getter]
    #[pyo3(name = "event_count")]
    fn py_event_count(&self) -> usize {
        self.event_count()
    }

    #[getter]
    #[pyo3(name = "client_order_ids")]
    fn py_client_order_ids(&self) -> Vec<ClientOrderId> {
        self.client_order_ids()
    }

    #[getter]
    #[pyo3(name = "venue_order_ids")]
    fn py_venue_order_ids(&self) -> Vec<VenueOrderId> {
        self.venue_order_ids()
    }

    #[getter]
    #[pyo3(name = "last_trade_id")]
    fn py_last_trade_id(&self) -> Option<TradeId> {
        self.last_trade_id()
    }

    #[getter]
    #[pyo3(name = "is_opened")]
    fn py_is_opened(&self) -> bool {
        self.is_opened()
    }

    #[getter]
    #[pyo3(name = "is_closed")]
    fn py_is_closed(&self) -> bool {
        self.is_closed()
    }

    #[getter]
    #[pyo3(name = "is_long")]
    fn py_is_long(&self) -> bool {
        self.is_long()
    }

    #[getter]
    #[pyo3(name = "is_short")]
    fn py_is_short(&self) -> bool {
        self.is_short()
    }

    #[pyo3(name = "unrealized_pnl")]
    fn py_unrealized_pnl(&self, last: Price) -> Money {
        self.unrealized_pnl(last)
    }

    #[pyo3(name = "total_pnl")]
    fn py_total_pnl(&self, last: Price) -> Money {
        self.total_pnl(last)
    }

    #[pyo3(name = "commissions")]
    fn py_commissions(&self) -> Vec<Money> {
        self.commissions()
    }

    #[pyo3(name = "apply")]
    fn py_apply(&mut self, fill: &OrderFilled) {
        self.apply(fill);
    }

    #[pyo3(name = "is_opposite_side")]
    fn py_is_opposite_side(&self, side: OrderSide) -> bool {
        self.is_opposite_side(side)
    }

    #[pyo3(name = "calculate_pnl")]
    fn py_calculate_pnl(&self, avg_px_open: f64, avg_px_close: f64, quantity: Quantity) -> Money {
        self.calculate_pnl(avg_px_open, avg_px_close, quantity)
    }

    #[pyo3(name = "notional_value")]
    fn py_notional_value(&self, price: Price) -> Money {
        self.notional_value(price)
    }

    #[staticmethod]
    #[pyo3(name = "from_dict")]
    pub fn py_from_dict(py: Python<'_>, values: Py<PyDict>) -> PyResult<Self> {
        from_dict_pyo3(py, values)
    }

    #[pyo3(name = "to_dict")]
    fn py_to_dict(&self, py: Python<'_>) -> PyResult<PyObject> {
        let dict = PyDict::new(py);
        dict.set_item("type", stringify!(Position))?;
        let events_dict: PyResult<Vec<_>> = self.events.iter().map(|e| e.py_to_dict(py)).collect();
        dict.set_item("events", events_dict?)?;
        dict.set_item("trader_id", self.trader_id.to_string())?;
        dict.set_item("strategy_id", self.strategy_id.to_string())?;
        dict.set_item("instrument_id", self.instrument_id.to_string())?;
        dict.set_item("id", self.id.to_string())?;
        dict.set_item("account_id", self.account_id.to_string())?;
        dict.set_item("opening_order_id", self.opening_order_id.to_string())?;
        match self.closing_order_id {
            Some(closing_order_id) => {
                dict.set_item("closing_order_id", closing_order_id.to_string())?;
            }
            None => dict.set_item("closing_order_id", py.None())?,
        }
        dict.set_item("entry", self.entry.to_string())?;
        dict.set_item("side", self.side.to_string())?;
        dict.set_item("signed_qty", self.signed_qty.to_f64())?;
        dict.set_item("quantity", self.quantity.to_string())?;
        dict.set_item("peak_qty", self.peak_qty.to_string())?;
        dict.set_item("price_precision", self.price_precision.to_u8())?;
        dict.set_item("size_precision", self.size_precision.to_u8())?;
        dict.set_item("multiplier", self.multiplier.to_string())?;
        dict.set_item("is_inverse", self.is_inverse)?;
        match self.base_currency {
            Some(base_currency) => {
                dict.set_item("base_currency", base_currency.code.to_string())?;
            }
            None => dict.set_item("base_currency", py.None())?,
        }
        dict.set_item("quote_currency", self.quote_currency.code.to_string())?;
        dict.set_item(
            "settlement_currency",
            self.settlement_currency.code.to_string(),
        )?;
        dict.set_item("ts_init", self.ts_init.to_u64())?;
        dict.set_item("ts_opened", self.ts_opened.to_u64())?;
        dict.set_item("ts_last", self.ts_last.to_u64())?;
        match self.ts_closed {
            Some(ts_closed) => dict.set_item("ts_closed", ts_closed.to_u64())?,
            None => dict.set_item("ts_closed", py.None())?,
        }
        dict.set_item("duration_ns", self.duration_ns.to_u64())?;
        dict.set_item("avg_px_open", self.avg_px_open.to_f64())?;
        match self.avg_px_close {
            Some(avg_px_close) => dict.set_item("avg_px_close", avg_px_close.to_u64())?,
            None => dict.set_item("avg_px_close", py.None())?,
        }
        dict.set_item("realized_return", self.realized_return.to_f64())?;
        match self.realized_pnl {
            Some(realized_pnl) => dict.set_item("realized_pnl", realized_pnl.to_string())?,
            None => dict.set_item("realized_pnl", py.None())?,
        }
        let venue_order_ids_list = PyList::new(
            py,
            self.venue_order_ids()
                .iter()
                .map(std::string::ToString::to_string),
        );
        dict.set_item("venue_order_ids", venue_order_ids_list)?;
        let trade_ids_list = PyList::new(
            py,
            self.trade_ids.iter().map(std::string::ToString::to_string),
        );
        dict.set_item("trade_ids", trade_ids_list)?;
        dict.set_item("buy_qty", self.buy_qty.to_string())?;
        dict.set_item("sell_qty", self.sell_qty.to_string())?;
        let commissions_dict = PyDict::new(py);
        for (key, value) in &self.commissions {
            commissions_dict.set_item(key.code.to_string(), value.to_string())?;
        }
        dict.set_item("commissions", commissions_dict)?;
        Ok(dict.into())
    }
}
