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

use databento::dbn;
use nautilus_core::{python::to_pyvalue_err, time::UnixNanos};
use nautilus_model::{
    data::{depth::OrderBookDepth10, trade::TradeTick},
    identifiers::instrument_id::InstrumentId,
    instruments::{
        equity::Equity, futures_contract::FuturesContract, options_contract::OptionsContract,
    },
};
use pyo3::{exceptions::PyRuntimeError, prelude::*, types::PyTuple};

use crate::databento::decode::{
    decode_equity_v1, decode_futures_contract_v1, decode_mbo_msg, decode_mbp10_msg,
    decode_mbp1_msg, decode_options_contract_v1, decode_trade_msg,
};

#[pyfunction]
#[pyo3(name = "decode_equity")]
pub fn py_decode_equity(
    record: &dbn::compat::InstrumentDefMsgV1,
    instrument_id: InstrumentId,
    ts_init: UnixNanos,
) -> PyResult<Equity> {
    decode_equity_v1(record, instrument_id, ts_init).map_err(to_pyvalue_err)
}

#[pyfunction]
#[pyo3(name = "decode_futures_contract")]
pub fn py_decode_futures_contract(
    record: &dbn::compat::InstrumentDefMsgV1,
    instrument_id: InstrumentId,
    ts_init: UnixNanos,
) -> PyResult<FuturesContract> {
    decode_futures_contract_v1(record, instrument_id, ts_init).map_err(to_pyvalue_err)
}

#[pyfunction]
#[pyo3(name = "decode_options_contract")]
pub fn py_decode_options_contract(
    record: &dbn::compat::InstrumentDefMsgV1,
    instrument_id: InstrumentId,
    ts_init: UnixNanos,
) -> PyResult<OptionsContract> {
    decode_options_contract_v1(record, instrument_id, ts_init).map_err(to_pyvalue_err)
}

#[pyfunction]
#[pyo3(name = "decode_mbo_msg")]
pub fn py_decode_mbo_msg(
    py: Python,
    record: &dbn::MboMsg,
    instrument_id: InstrumentId,
    price_precision: u8,
    ts_init: UnixNanos,
) -> PyResult<PyObject> {
    let result = decode_mbo_msg(record, instrument_id, price_precision, ts_init, false);

    match result {
        Ok((Some(data), None)) => Ok(data.into_py(py)),
        Err(e) => Err(to_pyvalue_err(e)),
        _ => Err(PyRuntimeError::new_err("Error parsing MBO message")),
    }
}

#[pyfunction]
#[pyo3(name = "decode_trade_msg")]
pub fn py_decode_trade_msg(
    record: &dbn::TradeMsg,
    instrument_id: InstrumentId,
    price_precision: u8,
    ts_init: UnixNanos,
) -> PyResult<TradeTick> {
    decode_trade_msg(record, instrument_id, price_precision, ts_init).map_err(to_pyvalue_err)
}

#[pyfunction]
#[pyo3(name = "decode_mbp1_msg")]
pub fn py_decode_mbp1_msg(
    py: Python,
    record: &dbn::Mbp1Msg,
    instrument_id: InstrumentId,
    price_precision: u8,
    ts_init: UnixNanos,
    include_trades: bool,
) -> PyResult<PyObject> {
    let result = decode_mbp1_msg(
        record,
        instrument_id,
        price_precision,
        ts_init,
        include_trades,
    );

    match result {
        Ok((quote, Some(trade))) => {
            let quote_py = quote.into_py(py);
            let trade_py = trade.into_py(py);
            Ok(PyTuple::new(py, &[quote_py, trade_py.into_py(py)]).into_py(py))
        }
        Ok((quote, None)) => {
            let quote_py = quote.into_py(py);
            Ok(PyTuple::new(py, &[quote_py, py.None()]).into_py(py))
        }
        Err(e) => {
            // Convert the Rust error to a Python exception
            Err(PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(format!(
                "Error parsing MBP1 message: {e}"
            )))
        }
    }
}

#[pyfunction]
#[pyo3(name = "decode_mbp10_msg")]
pub fn py_decode_mbp10_msg(
    record: &dbn::Mbp10Msg,
    instrument_id: InstrumentId,
    price_precision: u8,
    ts_init: UnixNanos,
) -> PyResult<OrderBookDepth10> {
    decode_mbp10_msg(record, instrument_id, price_precision, ts_init).map_err(to_pyvalue_err)
}
