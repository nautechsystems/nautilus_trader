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
    data::bar::{Bar, BarType},
    types::{price::Price, quantity::Quantity},
};
use polars::{
    prelude::{DataFrame, *},
    series::Series,
};
use pyo3::prelude::*;
use pyo3_polars::PyDataFrame;

#[pyclass]
pub struct BarDataWrangler {
    bar_type: BarType,
    price_precision: u8,
    size_precision: u8,
}

#[pymethods]
impl BarDataWrangler {
    #[new]
    fn py_new(bar_type: &str, price_precision: u8, size_precision: u8) -> Self {
        Self {
            bar_type: BarType::from_str(bar_type).unwrap(),
            price_precision,
            size_precision,
        }
    }

    #[getter]
    fn bar_type(&self) -> String {
        self.bar_type.to_string()
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
        default_volume: f64,
        ts_init_delta: u64,
    ) -> PyResult<Vec<Bar>> {
        // Convert DataFrame to Series per column
        let data: DataFrame = data.into();
        let open: &Series = data.column("open").unwrap();
        let high: &Series = data.column("high").unwrap(); // TODO: Change to 'size'
        let low: &Series = data.column("low").unwrap();
        let close: &Series = data.column("close").unwrap();
        let volume: &Series = &Series::new("volume", vec![default_volume; data.height()]);
        let volume: &Series = data.column("volume").unwrap_or(volume);
        let ts_event: Series = data
            .column("ts_event")
            .unwrap()
            .datetime()
            .unwrap()
            .cast(&DataType::Int64)
            .unwrap()
            .timestamp(TimeUnit::Nanoseconds)
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
        let open_values: Vec<f64> = open
            .f64()
            .unwrap()
            .into_iter()
            .map(Option::unwrap)
            .collect();
        let high_values: Vec<f64> = high
            .f64()
            .unwrap()
            .into_iter()
            .map(Option::unwrap)
            .collect();
        let low_values: Vec<f64> = low.f64().unwrap().into_iter().map(Option::unwrap).collect();
        let close_values: Vec<f64> = close
            .f64()
            .unwrap()
            .into_iter()
            .map(Option::unwrap)
            .collect();
        let volume_values: Vec<f64> = volume
            .f64()
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
        let bars: Vec<Bar> = open_values
            .into_iter()
            .zip(high_values.into_iter())
            .zip(low_values.into_iter())
            .zip(close_values.into_iter())
            .zip(volume_values.into_iter())
            .zip(ts_event_values.into_iter())
            .zip(ts_init_values.into_iter())
            .map(
                |((((((open, high), low), close), volume), ts_event), ts_init)| {
                    Bar::new(
                        self.bar_type.clone(),
                        Price::new(open, self.price_precision),
                        Price::new(high, self.price_precision),
                        Price::new(low, self.price_precision),
                        Price::new(close, self.price_precision),
                        Quantity::new(volume, self.size_precision),
                        ts_event as UnixNanos,
                        ts_init as UnixNanos,
                    )
                },
            )
            .collect();

        Ok(bars)
    }
}
