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

use nautilus_core::time::UnixNanos;
use nautilus_model::{
    data::trade::TradeTick,
    enums::AggressorSide,
    identifiers::{instrument_id::InstrumentId, trade_id::TradeId},
    types::{price::Price, quantity::Quantity},
};
use polars::{
    prelude::{DataFrame, *},
    series::Series,
};
use pyo3::prelude::*;
use pyo3_polars::PyDataFrame;

#[pyclass]
pub struct TradeTickDataWrangler {
    instrument_id: InstrumentId,
    price_precision: u8,
    size_precision: u8,
}

#[pymethods]
impl TradeTickDataWrangler {
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

    fn process(
        &self,
        _py: Python,
        data: PyDataFrame,
        ts_init_delta: u64,
    ) -> PyResult<Vec<TradeTick>> {
        // Convert DataFrame to Series per column
        let data: DataFrame = data.into();
        let price: &Series = data.column("price").unwrap();
        let size: &Series = data.column("size").unwrap();
        let aggressor_side: &Series = data.column("aggressor_side").unwrap();
        let trade_id: Series = data
            .column("trade_id")
            .unwrap()
            .cast(&DataType::Utf8)
            .unwrap();
        let ts_event: Series = data
            .column("ts_event")
            .unwrap()
            .datetime()
            .unwrap()
            .cast(&DataType::UInt64)
            .unwrap()
            .timestamp(TimeUnit::Nanoseconds)
            .unwrap()
            .cast(&DataType::UInt64)
            .unwrap()
            .into_series();
        let ts_init: Series = match data.column("ts_init") {
            Ok(column) => column
                .datetime()
                .unwrap()
                .cast(&DataType::UInt64)
                .unwrap()
                .timestamp(TimeUnit::Nanoseconds)
                .unwrap()
                .cast(&DataType::UInt64)
                .unwrap()
                .into_series(),
            Err(_) => {
                let ts_event_plus_delta: Series = ts_event
                    .u64()
                    .unwrap()
                    .into_iter()
                    .map(|ts| ts.map(|ts| ts + ts_init_delta))
                    .collect::<ChunkedArray<UInt64Type>>()
                    .into_series();
                ts_event_plus_delta
            }
        };

        // Convert Series to Rust native types
        let price_values: Vec<f64> = price
            .f64()
            .unwrap()
            .into_iter()
            .map(Option::unwrap)
            .collect();
        let size_values: Vec<f64> = size
            .f64()
            .unwrap()
            .into_iter()
            .map(Option::unwrap)
            .collect();
        let aggressor_side_values: Vec<AggressorSide> = aggressor_side
            .utf8()
            .unwrap()
            .into_iter()
            .map(|val| AggressorSide::from_str(val.unwrap()).unwrap())
            .collect();
        let trade_id_values: Vec<TradeId> = trade_id
            .utf8()
            .unwrap()
            .into_iter()
            .map(|val| TradeId::from_str(val.unwrap()).unwrap())
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
        let ticks: Vec<TradeTick> = price_values
            .into_iter()
            .zip(size_values.into_iter())
            .zip(aggressor_side_values.into_iter())
            .zip(trade_id_values.into_iter())
            .zip(ts_event_values.into_iter())
            .zip(ts_init_values.into_iter())
            .map(
                |(((((price, size), aggressor_side), trade_id), ts_event), ts_init)| {
                    TradeTick::new(
                        self.instrument_id.clone(),
                        Price::new(price, self.price_precision),
                        Quantity::new(size, self.size_precision),
                        aggressor_side,
                        trade_id,
                        ts_event as UnixNanos,
                        ts_init as UnixNanos,
                    )
                },
            )
            .collect();

        Ok(ticks)
    }
}
