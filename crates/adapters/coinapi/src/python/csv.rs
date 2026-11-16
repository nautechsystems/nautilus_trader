// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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

//! Python iterators for CoinAPI flat-file streams.

use std::fmt::Debug;

use nautilus_core::python::to_pyvalue_err;
use nautilus_model::data::{OrderBookDelta, QuoteTick, TradeTick};
use pyo3::prelude::*;

macro_rules! impl_coinapi_stream_iterator {
    ($struct_name:ident, $data_type:ty) => {
        #[pyclass(unsendable)]
        #[pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.adapters.coinapi")]
        pub struct $struct_name {
            pub(crate) stream: Box<dyn Iterator<Item = anyhow::Result<Vec<$data_type>>>>,
        }

        impl Debug for $struct_name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{} {{ stream: ... }}", stringify!($struct_name))
            }
        }

        #[pymethods]
        #[pyo3_stub_gen::derive::gen_stub_pymethods]
        impl $struct_name {
            const fn __iter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
                slf
            }

            fn __next__(&mut self) -> PyResult<Option<Vec<$data_type>>> {
                match self.stream.next() {
                    Some(Ok(chunk)) => Ok(Some(chunk)),
                    Some(Err(e)) => Err(to_pyvalue_err(e)),
                    None => Ok(None),
                }
            }
        }
    };
}

impl_coinapi_stream_iterator!(CoinApiTradeStreamIterator, TradeTick);
impl_coinapi_stream_iterator!(CoinApiQuoteStreamIterator, QuoteTick);
impl_coinapi_stream_iterator!(CoinApiDeltaStreamIterator, OrderBookDelta);

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, path::PathBuf};

    use nautilus_core::UnixNanos;
    use nautilus_model::{
        data::{Bar, BarType},
        identifiers::InstrumentId,
        types::{Price, Quantity},
    };
    use pyo3::exceptions::PyValueError;
    use rstest::rstest;

    use super::*;
    use crate::csv::{load_bars, stream_quotes};

    #[rstest]
    fn quote_iterator_preserves_chunks_limit_values_and_eof() {
        let directory = tempfile::TempDir::new().unwrap();
        let path = directory.path().join("quotes.csv");
        std::fs::write(
            &path,
            "time_exchange;time_coinapi;ask_px;ask_sx;bid_px;bid_sx\n\
            2025-09-01T00:00:00.1000000;2025-09-01T00:00:00.2000000;101.25;2.5;100.5;1\n\
            2025-09-01T00:00:01.3000000;2025-09-01T00:00:01.4000000;101.375;2.0625;100.125;0.125\n\
            2025-09-01T00:00:02;2025-09-01T00:00:03;102;3;101;2\n",
        )
        .unwrap();
        let instrument_id = InstrumentId::from("BTC-USD.COINBASE");
        let decoder = stream_quotes(path, instrument_id, 1, Some(3), Some(4), Some(2)).unwrap();

        let mut stream = CoinApiQuoteStreamIterator {
            stream: Box::new(decoder),
        };

        let first = stream.__next__().unwrap();
        let second = stream.__next__().unwrap();
        let end = stream.__next__().unwrap();
        let repeated_end = stream.__next__().unwrap();

        assert_eq!(
            first,
            Some(vec![QuoteTick::new(
                instrument_id,
                Price::from("100.500"),
                Price::from("101.250"),
                Quantity::from("1.0000"),
                Quantity::from("2.5000"),
                UnixNanos::from(1_756_684_800_100_000_000),
                UnixNanos::from(1_756_684_800_200_000_000)
            )])
        );
        assert_eq!(
            second,
            Some(vec![QuoteTick::new(
                instrument_id,
                Price::from("100.125"),
                Price::from("101.375"),
                Quantity::from("0.1250"),
                Quantity::from("2.0625"),
                UnixNanos::from(1_756_684_801_300_000_000),
                UnixNanos::from(1_756_684_801_400_000_000)
            )])
        );
        assert_eq!(
            first
                .into_iter()
                .chain(second)
                .flatten()
                .map(|quote| (
                    quote.bid_price.precision,
                    quote.ask_price.precision,
                    quote.bid_size.precision,
                    quote.ask_size.precision
                ))
                .collect::<Vec<_>>(),
            [(3, 3, 4, 4); 2]
        );
        assert_eq!(end, None);
        assert_eq!(repeated_end, None);
    }

    #[rstest]
    fn quote_iterator_reports_precision_errors_as_value_error() {
        Python::initialize();
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("test_data/csv/quotes_binance_spot_btc_usdt_20250901.csv");
        let decoder = stream_quotes(
            path,
            InstrumentId::from("BTCUSDT.BINANCE"),
            1,
            Some(1),
            None,
            None,
        )
        .unwrap();

        let mut stream = CoinApiQuoteStreamIterator {
            stream: Box::new(decoder),
        };

        let error = stream.__next__().unwrap_err();

        Python::attach(|py| {
            assert!(error.is_instance_of::<PyValueError>(py));
            assert_eq!(
                error.value(py).to_string(),
                "bid_px 108246.35 is finer than instrument precision 1"
            );
        });
    }

    #[rstest]
    fn ohlcv_decoding_retains_symbol_mapping() {
        let directory = tempfile::TempDir::new().unwrap();
        let path = directory.path().join("TP-1MIN.csv");
        std::fs::write(
            &path,
            "symbol_id;time_period_end;px_open;px_high;px_low;px_close;sx_sum\n\
            99;2025-09-01T00:01:00;1;2;0.5;1.5;4\n\
            7;2025-09-01T00:02:00;101.25;103.5;99.75;102.5;4.125\n",
        )
        .unwrap();
        let symbols = HashMap::from([(7, (InstrumentId::from("BTC-USD.COINBASE"), 2))]);
        let bars = load_bars(path, &symbols, None, Some(1)).unwrap();

        let timestamp = UnixNanos::from(1_756_684_920_000_000_000);
        assert_eq!(
            bars,
            [Bar::new(
                BarType::from("BTC-USD.COINBASE-1-MINUTE-LAST-EXTERNAL"),
                Price::from("101.25"),
                Price::from("103.50"),
                Price::from("99.75"),
                Price::from("102.50"),
                Quantity::from("4.125"),
                timestamp,
                timestamp
            )]
        );
        assert_eq!(
            (
                bars[0].open.precision,
                bars[0].high.precision,
                bars[0].low.precision,
                bars[0].close.precision,
                bars[0].volume.precision
            ),
            (2, 2, 2, 2, 3)
        );
    }
}
