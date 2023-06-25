// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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

use std::str::FromStr;

use nautilus_model::{
    data::{delta::OrderBookDelta, order::BookOrder},
    enums::{BookAction, FromU8, OrderSide},
    identifiers::instrument_id::InstrumentId,
    types::{price::Price, quantity::Quantity},
};
use polars::{prelude::DataFrame, series::Series};
use pyo3::prelude::*;
use pyo3_polars::PyDataFrame;

#[pyclass]
pub struct OrderBookDeltaDataWrangler {
    instrument_id: InstrumentId,
    price_precision: u8,
    size_precision: u8,
}

#[pymethods]
impl OrderBookDeltaDataWrangler {
    #[new]
    fn py_new(instrument_id: &str, price_precision: u8, size_precision: u8) -> Self {
        Self {
            instrument_id: InstrumentId::from_str(instrument_id).unwrap(),
            price_precision,
            size_precision,
        }
    }

    #[getter]
    fn instrument_id(&self) -> String {
        self.instrument_id.to_string()
    }

    #[getter]
    fn price_precision(&self) -> u8 {
        self.price_precision
    }

    #[getter]
    fn size_precision(&self) -> u8 {
        self.size_precision
    }

    fn process(&self, _py: Python, data: PyDataFrame) -> PyResult<Vec<OrderBookDelta>> {
        // Convert DataFrame to Series per column
        let data: DataFrame = data.into();

        // Extract column data as Series
        let action: &Series = data.column("action").unwrap();
        let side: &Series = data.column("side").unwrap();
        let price: &Series = data.column("price").unwrap();
        let size: &Series = data.column("size").unwrap();
        let order_id: &Series = data.column("order_id").unwrap();
        let flags: &Series = data.column("flags").unwrap();
        let sequence: &Series = data.column("sequence").unwrap();
        let ts_event: &Series = data.column("ts_event").unwrap();
        let ts_init: &Series = data.column("ts_init").unwrap();

        // Extract values from Series as Rust native types
        let action_values: Vec<u8> = action
            .u8()
            .unwrap()
            .into_iter()
            .map(Option::unwrap)
            .collect();
        let side_values: Vec<u8> = side.u8().unwrap().into_iter().map(Option::unwrap).collect();
        let price_values: Vec<i64> = price
            .i64()
            .unwrap()
            .into_iter()
            .map(Option::unwrap)
            .collect();
        let size_values: Vec<u64> = size
            .u64()
            .unwrap()
            .into_iter()
            .map(Option::unwrap)
            .collect();
        let order_id_values: Vec<u64> = order_id
            .u64()
            .unwrap()
            .into_iter()
            .map(Option::unwrap)
            .collect();
        let flags_values: Vec<u8> = flags
            .u8()
            .unwrap()
            .into_iter()
            .map(Option::unwrap)
            .collect();
        let sequence_values: Vec<u64> = sequence
            .u64()
            .unwrap()
            .into_iter()
            .map(Option::unwrap)
            .collect();
        let ts_event_values: Vec<u64> = ts_event
            .u64()
            .unwrap()
            .into_iter()
            .map(Option::unwrap)
            .collect();
        let ts_init_values: Vec<u64> = ts_init
            .u64()
            .unwrap()
            .into_iter()
            .map(Option::unwrap)
            .collect();

        // Map Series to Nautilus objects
        let deltas: Vec<OrderBookDelta> = action_values
            .into_iter()
            .zip(side_values.into_iter())
            .zip(price_values.into_iter())
            .zip(size_values.into_iter())
            .zip(order_id_values.into_iter())
            .zip(flags_values.into_iter())
            .zip(sequence_values.into_iter())
            .zip(ts_event_values.into_iter())
            .zip(ts_init_values.into_iter())
            .map(
                |(
                    (((((((action, side), price), size), order_id), flags), sequence), ts_event),
                    ts_init,
                )| {
                    OrderBookDelta {
                        instrument_id: self.instrument_id.clone(),
                        action: BookAction::from_u8(action).unwrap(),
                        order: BookOrder {
                            side: OrderSide::from_u8(side).unwrap(),
                            price: Price::from_raw(price, self.price_precision),
                            size: Quantity::from_raw(size, self.size_precision),
                            order_id,
                        },
                        flags,
                        sequence,
                        ts_event,
                        ts_init,
                    }
                },
            )
            .collect();

        Ok(deltas)
    }
}
