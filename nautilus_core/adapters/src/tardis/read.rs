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

use std::{error::Error, fs::File, io::BufReader, path::Path};

use csv::{Reader, ReaderBuilder};
use flate2::read::GzDecoder;
use nautilus_core::nanos::UnixNanos;
use nautilus_model::{
    data::{delta::OrderBookDelta, order::BookOrder, quote::QuoteTick, trade::TradeTick},
    enums::RecordFlag,
    identifiers::TradeId,
    types::{price::Price, quantity::Quantity},
};

use super::{
    parse::{
        parse_aggressor_side, parse_book_action, parse_instrument_id, parse_order_side,
        parse_timestamp,
    },
    record::{TardisBookUpdateRecord, TardisQuoteRecord, TardisTradeRecord},
};

/// Creates a new CSV reader which can handle gzip compression.
pub fn create_csv_reader<P: AsRef<Path>>(
    filepath: P,
) -> anyhow::Result<Reader<Box<dyn std::io::Read>>> {
    let file = File::open(filepath.as_ref())?;
    let buf_reader = BufReader::new(file);

    // Determine if the file is gzipped by its extension
    let reader: Box<dyn std::io::Read> =
        if filepath.as_ref().extension().unwrap_or_default() == "gz" {
            Box::new(GzDecoder::new(buf_reader)) // Decompress the gzipped file
        } else {
            Box::new(buf_reader) // Regular file reader
        };

    Ok(ReaderBuilder::new().has_headers(true).from_reader(reader))
}

/// Load [`OrderBookDelta`]s from a Tardis format CSV at the given `filepath`.
pub fn load_deltas<P: AsRef<Path>>(
    filepath: P,
    price_precision: u8,
    size_precision: u8,
    limit: Option<usize>,
) -> Result<Vec<OrderBookDelta>, Box<dyn Error>> {
    let mut csv_reader = create_csv_reader(filepath)?;
    let mut deltas: Vec<OrderBookDelta> = Vec::new();
    let mut last_ts_event = UnixNanos::default();

    for result in csv_reader.deserialize() {
        let record: TardisBookUpdateRecord = result?;

        let instrument_id = parse_instrument_id(&record.exchange, &record.symbol);
        let side = parse_order_side(&record.side);
        let price = Price::new(record.price, price_precision);
        let size = Quantity::new(record.amount, size_precision);
        let order_id = 0; // Not applicable for L2 data
        let order = BookOrder::new(side, price, size, order_id);

        let action = parse_book_action(record.is_snapshot, record.amount);
        let flags = 0; // Flags always zero until timestamp changes
        let sequence = 0; // Sequence not available
        let ts_event = parse_timestamp(record.timestamp);
        let ts_init = parse_timestamp(record.local_timestamp);

        // Check if timestamp is different from last timestamp
        if last_ts_event != ts_event {
            if let Some(last_delta) = deltas.last_mut() {
                // Set previous delta flags as F_LAST
                last_delta.flags = RecordFlag::F_LAST.value();
            }
        }

        last_ts_event = ts_event;

        let delta = OrderBookDelta::new(
            instrument_id,
            action,
            order,
            flags,
            sequence,
            ts_event,
            ts_init,
        );

        deltas.push(delta);

        if let Some(limit) = limit {
            if deltas.len() >= limit {
                break;
            }
        }
    }

    // Set F_LAST flag for final delta
    if let Some(last_delta) = deltas.last_mut() {
        last_delta.flags = RecordFlag::F_LAST.value();
    }

    Ok(deltas)
}

/// Load [`QuoteTick`]s from a Tardis format CSV at the given `filepath`.
pub fn load_quote_ticks<P: AsRef<Path>>(
    filepath: P,
    price_precision: u8,
    size_precision: u8,
    limit: Option<usize>,
) -> Result<Vec<QuoteTick>, Box<dyn Error>> {
    let mut csv_reader = create_csv_reader(filepath)?;
    let mut quotes = Vec::new();

    for result in csv_reader.deserialize() {
        let record: TardisQuoteRecord = result?;

        let instrument_id = parse_instrument_id(&record.exchange, &record.symbol);
        let bid_price = Price::new(record.bid_price.unwrap_or(0.0), price_precision);
        let bid_size = Quantity::new(record.bid_amount.unwrap_or(0.0), size_precision);
        let ask_price = Price::new(record.ask_price.unwrap_or(0.0), price_precision);
        let ask_size = Quantity::new(record.ask_amount.unwrap_or(0.0), size_precision);
        let ts_event = parse_timestamp(record.timestamp);
        let ts_init = parse_timestamp(record.local_timestamp);

        let quote = QuoteTick::new(
            instrument_id,
            bid_price,
            ask_price,
            bid_size,
            ask_size,
            ts_event,
            ts_init,
        );

        quotes.push(quote);

        if let Some(limit) = limit {
            if quotes.len() >= limit {
                break;
            }
        }
    }

    Ok(quotes)
}

/// Load [`TradeTick`]s from a Tardis format CSV at the given `filepath`.
pub fn load_trade_ticks<P: AsRef<Path>>(
    filepath: P,
    price_precision: u8,
    size_precision: u8,
    limit: Option<usize>,
) -> Result<Vec<TradeTick>, Box<dyn Error>> {
    let mut csv_reader = create_csv_reader(filepath)?;
    let mut trades = Vec::new();

    for result in csv_reader.deserialize() {
        let record: TardisTradeRecord = result?;

        let instrument_id = parse_instrument_id(&record.exchange, &record.symbol);
        let price = Price::new(record.price, price_precision);
        let size = Quantity::new(record.amount, size_precision);
        let aggressor_side = parse_aggressor_side(&record.side);
        let trade_id = TradeId::new(&record.id);
        let ts_event = parse_timestamp(record.timestamp);
        let ts_init = parse_timestamp(record.local_timestamp);

        let trade = TradeTick::new(
            instrument_id,
            price,
            size,
            aggressor_side,
            trade_id,
            ts_event,
            ts_init,
        );

        trades.push(trade);

        if let Some(limit) = limit {
            if trades.len() >= limit {
                break;
            }
        }
    }

    Ok(trades)
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use nautilus_test_kit::{
        common::{get_project_testdata_path, get_testdata_large_checksums_filepath},
        files::ensure_file_exists_or_download_http,
    };
    use rstest::*;

    use super::*;

    #[rstest]
    pub fn test_read_deltas() {
        let testdata = get_project_testdata_path();
        let checksums = get_testdata_large_checksums_filepath();
        let filename = "tardis_deribit_incremental_book_L2_2020-04-01_BTC-PERPETUAL.csv.gz";
        let filepath = testdata.join("large").join(filename);
        let url = "https://datasets.tardis.dev/v1/deribit/incremental_book_L2/2020/04/01/BTC-PERPETUAL.csv.gz";
        ensure_file_exists_or_download_http(&filepath, url, Some(&checksums)).unwrap();

        let deltas = load_deltas(filepath, 1, 0, Some(1_000)).unwrap();

        assert_eq!(deltas.len(), 1_000)
    }

    #[rstest]
    pub fn test_read_quotes() {
        let testdata = get_project_testdata_path();
        let checksums = get_testdata_large_checksums_filepath();
        let filename = "tardis_huobi-dm-swap_quotes_2020-05-01_BTC-USD.csv.gz";
        let filepath = testdata.join("large").join(filename);
        let url = "https://datasets.tardis.dev/v1/huobi-dm-swap/quotes/2020/05/01/BTC-USD.csv.gz";
        ensure_file_exists_or_download_http(&filepath, url, Some(&checksums)).unwrap();

        let quotes = load_quote_ticks(filepath, 1, 0, Some(1_000)).unwrap();

        assert_eq!(quotes.len(), 1_000)
    }

    #[rstest]
    pub fn test_read_trades() {
        let testdata = get_project_testdata_path();
        let checksums = get_testdata_large_checksums_filepath();
        let filename = "tardis_bitmex_trades_2020-03-01_XBTUSD.csv.gz";
        let filepath = testdata.join("large").join(filename);
        let url = "https://datasets.tardis.dev/v1/bitmex/trades/2020/03/01/XBTUSD.csv.gz";
        ensure_file_exists_or_download_http(&filepath, url, Some(&checksums)).unwrap();

        let trades = load_trade_ticks(filepath, 1, 0, Some(1_000)).unwrap();

        assert_eq!(trades.len(), 1_000)
    }
}
