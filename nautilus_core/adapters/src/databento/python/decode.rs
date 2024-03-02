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

use anyhow::bail;
use nautilus_core::time::UnixNanos;
use nautilus_model::{
    data::{depth::OrderBookDepth10, trade::TradeTick},
    identifiers::instrument_id::InstrumentId,
    instruments::{
        equity::Equity, futures_contract::FuturesContract, options_contract::OptionsContract,
    },
};
use pyo3::{prelude::*, types::PyTuple};

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
) -> anyhow::Result<Equity> {
    decode_equity_v1(record, instrument_id, ts_init)
}

#[pyfunction]
#[pyo3(name = "decode_futures_contract")]
pub fn py_decode_futures_contract(
    record: &dbn::compat::InstrumentDefMsgV1,
    instrument_id: InstrumentId,
    ts_init: UnixNanos,
) -> anyhow::Result<FuturesContract> {
    decode_futures_contract_v1(record, instrument_id, ts_init)
}

#[pyfunction]
#[pyo3(name = "decode_options_contract")]
pub fn py_decode_options_contract(
    record: &dbn::compat::InstrumentDefMsgV1,
    instrument_id: InstrumentId,
    ts_init: UnixNanos,
) -> anyhow::Result<OptionsContract> {
    decode_options_contract_v1(record, instrument_id, ts_init)
}

#[pyfunction]
#[pyo3(name = "decode_mbo_msg")]
pub fn py_decode_mbo_msg(
    py: Python,
    record: &dbn::MboMsg,
    instrument_id: InstrumentId,
    price_precision: u8,
    ts_init: UnixNanos,
) -> anyhow::Result<PyObject> {
    let (data, _) = decode_mbo_msg(record, instrument_id, price_precision, ts_init, false)?;
    if let Some(data) = data {
        Ok(data.into_py(py))
    } else {
        bail!("Error decoding MBO message")
    }
}

#[pyfunction]
#[pyo3(name = "decode_trade_msg")]
pub fn py_decode_trade_msg(
    record: &dbn::TradeMsg,
    instrument_id: InstrumentId,
    price_precision: u8,
    ts_init: UnixNanos,
) -> anyhow::Result<TradeTick> {
    decode_trade_msg(record, instrument_id, price_precision, ts_init)
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
) -> anyhow::Result<PyObject> {
    let (quote, maybe_trade) = decode_mbp1_msg(
        record,
        instrument_id,
        price_precision,
        ts_init,
        include_trades,
    )?;

    let quote_py = quote.into_py(py);
    match maybe_trade {
        Some(trade) => {
            let trade_py = trade.into_py(py);
            Ok(PyTuple::new(py, &[quote_py, trade_py]).into_py(py))
        }
        None => Ok(PyTuple::new(py, &[quote_py, py.None()]).into_py(py)),
    }
}

#[pyfunction]
#[pyo3(name = "decode_mbp10_msg")]
pub fn py_decode_mbp10_msg(
    record: &dbn::Mbp10Msg,
    instrument_id: InstrumentId,
    price_precision: u8,
    ts_init: UnixNanos,
) -> anyhow::Result<OrderBookDepth10> {
    decode_mbp10_msg(record, instrument_id, price_precision, ts_init)
}
