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
    data::quote::QuoteTick,
    identifiers::instrument_id::InstrumentId,
    types::{price::Price, quantity::Quantity},
};
use polars::{
    prelude::{DataFrame, NamedFrom, *},
    series::Series,
};
use pyo3::prelude::*;
use pyo3_polars::PyDataFrame;

#[pyclass]
pub struct QuoteTickDataWrangler {
    instrument_id: InstrumentId,
    price_precision: u8,
    size_precision: u8,
}

#[pymethods]
impl QuoteTickDataWrangler {
    #[new]
    fn py_new(instrument_id: &str, price_precision: u8, size_precision: u8) -> Self {
        QuoteTickDataWrangler {
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
        default_size: f64,
        ts_init_delta: u64,
    ) -> Vec<QuoteTick> {
        // Convert DataFrame to Series per column
        let data: DataFrame = data.into();
        let bid: &Series = data.column("bid").unwrap();
        let ask: &Series = data.column("ask").unwrap();
        let bid_size: &Series = &Series::new("bid_size", vec![default_size; data.height()]);
        let bid_size: &Series = data.column("bid_size").unwrap_or(bid_size);
        let ask_size: &Series = &Series::new("ask_size", vec![default_size; data.height()]);
        let ask_size: &Series = data.column("ask_size").unwrap_or(ask_size);
        let ts_event: Series = data
            .column("timestamp")
            .unwrap()
            .datetime()
            .unwrap()
            .cast(&DataType::Int64)
            .unwrap()
            .timestamp(TimeUnit::Nanoseconds)
            .unwrap()
            .into_series();

        // Convert Series to vectors of Rust native types
        let bid_values: Vec<f64> = bid.f64().unwrap().into_iter().map(Option::unwrap).collect();
        let ask_values: Vec<f64> = ask.f64().unwrap().into_iter().map(Option::unwrap).collect();
        let bid_size_values: Vec<f64> = bid_size
            .f64()
            .unwrap()
            .into_iter()
            .map(Option::unwrap)
            .collect();
        let ask_size_values: Vec<f64> = ask_size
            .f64()
            .unwrap()
            .into_iter()
            .map(Option::unwrap)
            .collect();
        let ts_event_values: Vec<i64> = ts_event
            .i64()
            .unwrap()
            .into_iter()
            .map(Option::unwrap)
            .collect();

        // Map Series to QuoteTick objects
        let ticks: Vec<QuoteTick> = bid_values
            .into_iter()
            .zip(ask_values.into_iter())
            .zip(bid_size_values.into_iter())
            .zip(ask_size_values.into_iter())
            .zip(ts_event_values.into_iter())
            .map(|((((bid, ask), bid_size), ask_size), ts_event)| {
                QuoteTick::new(
                    self.instrument_id.clone(),
                    Price::new(bid, self.price_precision),
                    Price::new(ask, self.price_precision),
                    Quantity::new(bid_size, self.size_precision),
                    Quantity::new(ask_size, self.size_precision),
                    ts_event as UnixNanos,
                    (ts_event as u64 + ts_init_delta) as UnixNanos,
                )
            })
            .collect();
        ticks
    }
}
