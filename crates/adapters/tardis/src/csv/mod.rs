// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

mod record;

use std::{
    error::Error,
    ffi::OsStr,
    fs::File,
    io::{BufReader, Read, Seek, SeekFrom},
    path::Path,
    time::Duration,
};

use csv::{Reader, ReaderBuilder, StringRecord};
use flate2::read::GzDecoder;
use nautilus_core::UnixNanos;
use nautilus_model::{
    data::{
        BookOrder, DEPTH10_LEN, NULL_ORDER, OrderBookDelta, OrderBookDepth10, QuoteTick, TradeTick,
    },
    enums::{BookAction, OrderSide, RecordFlag},
    identifiers::{InstrumentId, TradeId},
    types::{Quantity, fixed::FIXED_PRECISION},
};

use super::{
    csv::record::{
        TardisBookUpdateRecord, TardisOrderBookSnapshot5Record, TardisOrderBookSnapshot25Record,
        TardisQuoteRecord, TardisTradeRecord,
    },
    parse::{
        parse_aggressor_side, parse_book_action, parse_instrument_id, parse_order_side,
        parse_timestamp,
    },
};
use crate::parse::parse_price;

fn infer_precision(value: f64) -> u8 {
    let str_value = value.to_string(); // Single allocation
    match str_value.find('.') {
        Some(decimal_idx) => (str_value.len() - decimal_idx - 1) as u8,
        None => 0,
    }
}

fn create_csv_reader<P: AsRef<Path>>(
    filepath: P,
) -> anyhow::Result<Reader<Box<dyn std::io::Read>>> {
    let filepath_ref = filepath.as_ref();
    const MAX_RETRIES: u8 = 3;
    const DELAY_MS: u64 = 100;

    fn open_file_with_retry<P: AsRef<Path>>(
        path: P,
        max_retries: u8,
        delay_ms: u64,
    ) -> anyhow::Result<File> {
        let path_ref = path.as_ref();
        for attempt in 1..=max_retries {
            match File::open(path_ref) {
                Ok(file) => return Ok(file),
                Err(e) => {
                    if attempt == max_retries {
                        anyhow::bail!(
                            "Failed to open file '{path_ref:?}' after {max_retries} attempts: {e}"
                        );
                    }
                    eprintln!(
                        "Attempt {attempt}/{max_retries} failed to open file '{path_ref:?}': {e}. Retrying after {delay_ms}ms..."
                    );
                    std::thread::sleep(Duration::from_millis(delay_ms));
                }
            }
        }
        unreachable!("Loop should return either Ok or Err");
    }

    let mut file = open_file_with_retry(filepath_ref, MAX_RETRIES, DELAY_MS)?;

    let is_gzipped = filepath_ref
        .extension()
        .and_then(OsStr::to_str)
        .is_some_and(|ext| ext.eq_ignore_ascii_case("gz"));

    if !is_gzipped {
        let buf_reader = BufReader::new(file);
        return Ok(ReaderBuilder::new()
            .has_headers(true)
            .from_reader(Box::new(buf_reader)));
    }

    let file_size = file.metadata()?.len();
    if file_size < 2 {
        anyhow::bail!("File too small to be a valid gzip file");
    }

    let mut header_buf = [0u8; 2];
    for attempt in 1..=MAX_RETRIES {
        match file.read_exact(&mut header_buf) {
            Ok(()) => break,
            Err(e) => {
                if attempt == MAX_RETRIES {
                    anyhow::bail!(
                        "Failed to read gzip header from '{filepath_ref:?}' after {MAX_RETRIES} attempts: {e}"
                    );
                }
                eprintln!(
                    "Attempt {attempt}/{MAX_RETRIES} failed to read header from '{filepath_ref:?}': {e}. Retrying after {DELAY_MS}ms..."
                );
                std::thread::sleep(Duration::from_millis(DELAY_MS));
            }
        }
    }

    if header_buf[0] != 0x1f || header_buf[1] != 0x8b {
        anyhow::bail!("File '{filepath_ref:?}' has .gz extension but invalid gzip header");
    }

    for attempt in 1..=MAX_RETRIES {
        match file.seek(SeekFrom::Start(0)) {
            Ok(_) => break,
            Err(e) => {
                if attempt == MAX_RETRIES {
                    anyhow::bail!(
                        "Failed to reset file position for '{filepath_ref:?}' after {MAX_RETRIES} attempts: {e}"
                    );
                }
                eprintln!(
                    "Attempt {attempt}/{MAX_RETRIES} failed to seek in '{filepath_ref:?}': {e}. Retrying after {DELAY_MS}ms..."
                );
                std::thread::sleep(Duration::from_millis(DELAY_MS));
            }
        }
    }

    let buf_reader = BufReader::new(file);
    let decoder = GzDecoder::new(buf_reader);

    Ok(ReaderBuilder::new()
        .has_headers(true)
        .from_reader(Box::new(decoder)))
}

/// Loads [`OrderBookDelta`]s from a Tardis format CSV at the given `filepath`,
/// automatically applying `GZip` decompression for files ending in ".gz".
pub fn load_deltas<P: AsRef<Path>>(
    filepath: P,
    price_precision: Option<u8>,
    size_precision: Option<u8>,
    instrument_id: Option<InstrumentId>,
    limit: Option<usize>,
) -> Result<Vec<OrderBookDelta>, Box<dyn Error>> {
    // Infer precisions if not provided
    let (price_precision, size_precision) = match (price_precision, size_precision) {
        (Some(p), Some(s)) => (p, s),
        (price_precision, size_precision) => {
            let mut reader = create_csv_reader(&filepath)?;
            let mut record = StringRecord::new();

            let mut max_price_precision = 0u8;
            let mut max_size_precision = 0u8;
            let mut count = 0;

            while reader.read_record(&mut record)? {
                let parsed: TardisBookUpdateRecord = record.deserialize(None)?;

                if price_precision.is_none() {
                    max_price_precision = infer_precision(parsed.price).max(max_price_precision);
                }

                if size_precision.is_none() {
                    max_size_precision = infer_precision(parsed.amount).max(max_size_precision);
                }

                if let Some(limit) = limit {
                    if count >= limit {
                        break;
                    }
                    count += 1;
                }
            }

            drop(reader);

            max_price_precision = max_price_precision.min(FIXED_PRECISION);
            max_size_precision = max_size_precision.min(FIXED_PRECISION);

            (
                price_precision.unwrap_or(max_price_precision),
                size_precision.unwrap_or(max_size_precision),
            )
        }
    };

    let mut deltas: Vec<OrderBookDelta> = Vec::new();
    let mut last_ts_event = UnixNanos::default();

    let mut reader = create_csv_reader(filepath)?;
    let mut record = StringRecord::new();

    while reader.read_record(&mut record)? {
        let record: TardisBookUpdateRecord = record.deserialize(None)?;

        let instrument_id = match &instrument_id {
            Some(id) => *id,
            None => parse_instrument_id(&record.exchange, record.symbol),
        };
        let side = parse_order_side(&record.side);
        let price = parse_price(record.price, price_precision);
        let size = Quantity::new(record.amount, size_precision);
        let order_id = 0; // Not applicable for L2 data
        let order = BookOrder::new(side, price, size, order_id);

        let action = parse_book_action(record.is_snapshot, size.as_f64());
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

        assert!(
            !(action != BookAction::Delete && size.is_zero()),
            "Invalid delta: action {action} when size zero, check size_precision ({size_precision}) vs data; {record:?}"
        );

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

fn create_book_order(
    side: OrderSide,
    price: Option<f64>,
    amount: Option<f64>,
    price_precision: u8,
    size_precision: u8,
) -> (BookOrder, u32) {
    match price {
        Some(price) => (
            BookOrder::new(
                side,
                parse_price(price, price_precision),
                Quantity::new(amount.unwrap_or(0.0), size_precision),
                0,
            ),
            1, // Count set to 1 if order exists
        ),
        None => (NULL_ORDER, 0), // NULL_ORDER if price is None
    }
}

/// Loads [`OrderBookDepth10`]s from a Tardis format CSV at the given `filepath`,
/// automatically applying `GZip` decompression for files ending in ".gz".
pub fn load_depth10_from_snapshot5<P: AsRef<Path>>(
    filepath: P,
    price_precision: Option<u8>,
    size_precision: Option<u8>,
    instrument_id: Option<InstrumentId>,
    limit: Option<usize>,
) -> Result<Vec<OrderBookDepth10>, Box<dyn Error>> {
    // Infer precisions if not provided
    let (price_precision, size_precision) = match (price_precision, size_precision) {
        (Some(p), Some(s)) => (p, s),
        (price_precision, size_precision) => {
            let mut reader = create_csv_reader(&filepath)?;
            let mut record = StringRecord::new();

            let mut max_price_precision = 0u8;
            let mut max_size_precision = 0u8;
            let mut count = 0;

            while reader.read_record(&mut record)? {
                let parsed: TardisOrderBookSnapshot5Record = record.deserialize(None)?;

                if price_precision.is_none() {
                    if let Some(bid_price) = parsed.bids_0_price {
                        max_price_precision = infer_precision(bid_price).max(max_price_precision);
                    }
                }

                if size_precision.is_none() {
                    if let Some(bid_amount) = parsed.bids_0_amount {
                        max_size_precision = infer_precision(bid_amount).max(max_size_precision);
                    }
                }

                if let Some(limit) = limit {
                    if count >= limit {
                        break;
                    }
                    count += 1;
                }
            }

            drop(reader);

            max_price_precision = max_price_precision.min(FIXED_PRECISION);
            max_size_precision = max_size_precision.min(FIXED_PRECISION);

            (
                price_precision.unwrap_or(max_price_precision),
                size_precision.unwrap_or(max_size_precision),
            )
        }
    };

    let mut depths: Vec<OrderBookDepth10> = Vec::new();

    let mut reader = create_csv_reader(filepath)?;
    let mut record = StringRecord::new();
    while reader.read_record(&mut record)? {
        let record: TardisOrderBookSnapshot5Record = record.deserialize(None)?;
        let instrument_id = match &instrument_id {
            Some(id) => *id,
            None => parse_instrument_id(&record.exchange, record.symbol),
        };
        let flags = RecordFlag::F_LAST.value();
        let sequence = 0; // Sequence not available
        let ts_event = parse_timestamp(record.timestamp);
        let ts_init = parse_timestamp(record.local_timestamp);

        // Initialize empty arrays
        let mut bids = [NULL_ORDER; DEPTH10_LEN];
        let mut asks = [NULL_ORDER; DEPTH10_LEN];
        let mut bid_counts = [0u32; DEPTH10_LEN];
        let mut ask_counts = [0u32; DEPTH10_LEN];

        for i in 0..=4 {
            // Create bids
            let (bid_order, bid_count) = create_book_order(
                OrderSide::Buy,
                match i {
                    0 => record.bids_0_price,
                    1 => record.bids_1_price,
                    2 => record.bids_2_price,
                    3 => record.bids_3_price,
                    4 => record.bids_4_price,
                    _ => panic!("Invalid level for snapshot5 -> depth10 parsing"),
                },
                match i {
                    0 => record.bids_0_amount,
                    1 => record.bids_1_amount,
                    2 => record.bids_2_amount,
                    3 => record.bids_3_amount,
                    4 => record.bids_4_amount,
                    _ => panic!("Invalid level for snapshot5 -> depth10 parsing"),
                },
                price_precision,
                size_precision,
            );
            bids[i] = bid_order;
            bid_counts[i] = bid_count;

            // Create asks
            let (ask_order, ask_count) = create_book_order(
                OrderSide::Sell,
                match i {
                    0 => record.asks_0_price,
                    1 => record.asks_1_price,
                    2 => record.asks_2_price,
                    3 => record.asks_3_price,
                    4 => record.asks_4_price,
                    _ => None, // Unreachable, but for safety
                },
                match i {
                    0 => record.asks_0_amount,
                    1 => record.asks_1_amount,
                    2 => record.asks_2_amount,
                    3 => record.asks_3_amount,
                    4 => record.asks_4_amount,
                    _ => None, // Unreachable, but for safety
                },
                price_precision,
                size_precision,
            );
            asks[i] = ask_order;
            ask_counts[i] = ask_count;
        }

        let depth = OrderBookDepth10::new(
            instrument_id,
            bids,
            asks,
            bid_counts,
            ask_counts,
            flags,
            sequence,
            ts_event,
            ts_init,
        );

        depths.push(depth);

        if let Some(limit) = limit {
            if depths.len() >= limit {
                break;
            }
        }
    }

    Ok(depths)
}

/// Loads [`OrderBookDepth10`]s from a Tardis format CSV at the given `filepath`,
/// automatically applying `GZip` decompression for files ending in ".gz".
pub fn load_depth10_from_snapshot25<P: AsRef<Path>>(
    filepath: P,
    price_precision: Option<u8>,
    size_precision: Option<u8>,
    instrument_id: Option<InstrumentId>,
    limit: Option<usize>,
) -> Result<Vec<OrderBookDepth10>, Box<dyn Error>> {
    // Infer precisions if not provided
    let (price_precision, size_precision) = match (price_precision, size_precision) {
        (Some(p), Some(s)) => (p, s),
        (price_precision, size_precision) => {
            let mut reader = create_csv_reader(&filepath)?;
            let mut record = StringRecord::new();

            let mut max_price_precision = 0u8;
            let mut max_size_precision = 0u8;
            let mut count = 0;

            while reader.read_record(&mut record)? {
                let parsed: TardisOrderBookSnapshot25Record = record.deserialize(None)?;

                if price_precision.is_none() {
                    if let Some(bid_price) = parsed.bids_0_price {
                        max_price_precision = infer_precision(bid_price).max(max_price_precision);
                    }
                }

                if size_precision.is_none() {
                    if let Some(bid_amount) = parsed.bids_0_amount {
                        max_size_precision = infer_precision(bid_amount).max(max_size_precision);
                    }
                }

                if let Some(limit) = limit {
                    if count >= limit {
                        break;
                    }
                    count += 1;
                }
            }

            drop(reader);

            max_price_precision = max_price_precision.min(FIXED_PRECISION);
            max_size_precision = max_size_precision.min(FIXED_PRECISION);

            (
                price_precision.unwrap_or(max_price_precision),
                size_precision.unwrap_or(max_size_precision),
            )
        }
    };

    let mut depths: Vec<OrderBookDepth10> = Vec::new();
    let mut reader = create_csv_reader(filepath)?;
    let mut record = StringRecord::new();

    while reader.read_record(&mut record)? {
        let record: TardisOrderBookSnapshot25Record = record.deserialize(None)?;

        let instrument_id = match &instrument_id {
            Some(id) => *id,
            None => parse_instrument_id(&record.exchange, record.symbol),
        };
        let flags = RecordFlag::F_LAST.value();
        let sequence = 0; // Sequence not available
        let ts_event = parse_timestamp(record.timestamp);
        let ts_init = parse_timestamp(record.local_timestamp);

        // Initialize empty arrays for the first 10 levels only
        let mut bids = [NULL_ORDER; DEPTH10_LEN];
        let mut asks = [NULL_ORDER; DEPTH10_LEN];
        let mut bid_counts = [0u32; DEPTH10_LEN];
        let mut ask_counts = [0u32; DEPTH10_LEN];

        // Fill only the first 10 levels from the 25-level record
        for i in 0..DEPTH10_LEN {
            // Create bids
            let (bid_order, bid_count) = create_book_order(
                OrderSide::Buy,
                match i {
                    0 => record.bids_0_price,
                    1 => record.bids_1_price,
                    2 => record.bids_2_price,
                    3 => record.bids_3_price,
                    4 => record.bids_4_price,
                    5 => record.bids_5_price,
                    6 => record.bids_6_price,
                    7 => record.bids_7_price,
                    8 => record.bids_8_price,
                    9 => record.bids_9_price,
                    _ => panic!("Invalid level for snapshot25 -> depth10 parsing"),
                },
                match i {
                    0 => record.bids_0_amount,
                    1 => record.bids_1_amount,
                    2 => record.bids_2_amount,
                    3 => record.bids_3_amount,
                    4 => record.bids_4_amount,
                    5 => record.bids_5_amount,
                    6 => record.bids_6_amount,
                    7 => record.bids_7_amount,
                    8 => record.bids_8_amount,
                    9 => record.bids_9_amount,
                    _ => panic!("Invalid level for snapshot25 -> depth10 parsing"),
                },
                price_precision,
                size_precision,
            );
            bids[i] = bid_order;
            bid_counts[i] = bid_count;

            // Create asks
            let (ask_order, ask_count) = create_book_order(
                OrderSide::Sell,
                match i {
                    0 => record.asks_0_price,
                    1 => record.asks_1_price,
                    2 => record.asks_2_price,
                    3 => record.asks_3_price,
                    4 => record.asks_4_price,
                    5 => record.asks_5_price,
                    6 => record.asks_6_price,
                    7 => record.asks_7_price,
                    8 => record.asks_8_price,
                    9 => record.asks_9_price,
                    _ => panic!("Invalid level for snapshot25 -> depth10 parsing"),
                },
                match i {
                    0 => record.asks_0_amount,
                    1 => record.asks_1_amount,
                    2 => record.asks_2_amount,
                    3 => record.asks_3_amount,
                    4 => record.asks_4_amount,
                    5 => record.asks_5_amount,
                    6 => record.asks_6_amount,
                    7 => record.asks_7_amount,
                    8 => record.asks_8_amount,
                    9 => record.asks_9_amount,
                    _ => panic!("Invalid level for snapshot25 -> depth10 parsing"),
                },
                price_precision,
                size_precision,
            );
            asks[i] = ask_order;
            ask_counts[i] = ask_count;
        }

        let depth = OrderBookDepth10::new(
            instrument_id,
            bids,
            asks,
            bid_counts,
            ask_counts,
            flags,
            sequence,
            ts_event,
            ts_init,
        );

        depths.push(depth);

        if let Some(limit) = limit {
            if depths.len() >= limit {
                break;
            }
        }
    }

    Ok(depths)
}

/// Loads [`QuoteTick`]s from a Tardis format CSV at the given `filepath`,
/// automatically applying `GZip` decompression for files ending in ".gz".
pub fn load_quote_ticks<P: AsRef<Path>>(
    filepath: P,
    price_precision: Option<u8>,
    size_precision: Option<u8>,
    instrument_id: Option<InstrumentId>,
    limit: Option<usize>,
) -> Result<Vec<QuoteTick>, Box<dyn Error>> {
    // Infer precisions if not provided
    let (price_precision, size_precision) = match (price_precision, size_precision) {
        (Some(p), Some(s)) => (p, s),
        (price_precision, size_precision) => {
            let mut reader = create_csv_reader(&filepath)?;
            let mut record = StringRecord::new();

            let mut max_price_precision = 0u8;
            let mut max_size_precision = 0u8;
            let mut count = 0;

            while reader.read_record(&mut record)? {
                let parsed: TardisQuoteRecord = record.deserialize(None)?;

                if price_precision.is_none() {
                    if let Some(bid_price) = parsed.bid_price {
                        max_price_precision = infer_precision(bid_price).max(max_price_precision);
                    }
                }

                if size_precision.is_none() {
                    if let Some(bid_amount) = parsed.bid_amount {
                        max_size_precision = infer_precision(bid_amount).max(max_size_precision);
                    }
                }

                if let Some(limit) = limit {
                    if count >= limit {
                        break;
                    }
                    count += 1;
                }
            }

            drop(reader);

            max_price_precision = max_price_precision.min(FIXED_PRECISION);
            max_size_precision = max_size_precision.min(FIXED_PRECISION);

            (
                price_precision.unwrap_or(max_price_precision),
                size_precision.unwrap_or(max_size_precision),
            )
        }
    };

    let mut quotes = Vec::new();
    let mut reader = create_csv_reader(filepath)?;
    let mut record = StringRecord::new();

    while reader.read_record(&mut record)? {
        let record: TardisQuoteRecord = record.deserialize(None)?;

        let instrument_id = match &instrument_id {
            Some(id) => *id,
            None => parse_instrument_id(&record.exchange, record.symbol),
        };
        let bid_price = parse_price(record.bid_price.unwrap_or(0.0), price_precision);
        let bid_size = Quantity::new(record.bid_amount.unwrap_or(0.0), size_precision);
        let ask_price = parse_price(record.ask_price.unwrap_or(0.0), price_precision);
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

/// Loads [`TradeTick`]s from a Tardis format CSV at the given `filepath`,
/// automatically applying `GZip` decompression for files ending in ".gz".
pub fn load_trade_ticks<P: AsRef<Path>>(
    filepath: P,
    price_precision: Option<u8>,
    size_precision: Option<u8>,
    instrument_id: Option<InstrumentId>,
    limit: Option<usize>,
) -> Result<Vec<TradeTick>, Box<dyn Error>> {
    // Infer precisions if not provided
    let (price_precision, size_precision) = match (price_precision, size_precision) {
        (Some(p), Some(s)) => (p, s),
        (price_precision, size_precision) => {
            let mut reader = create_csv_reader(&filepath)?;
            let mut record = StringRecord::new();

            let mut max_price_precision = 0u8;
            let mut max_size_precision = 0u8;
            let mut count = 0;

            while reader.read_record(&mut record)? {
                let parsed: TardisTradeRecord = record.deserialize(None)?;

                if price_precision.is_none() {
                    max_price_precision = infer_precision(parsed.price).max(max_price_precision);
                }

                if size_precision.is_none() {
                    max_size_precision = infer_precision(parsed.amount).max(max_size_precision);
                }

                if let Some(limit) = limit {
                    if count >= limit {
                        break;
                    }
                    count += 1;
                }
            }

            drop(reader);

            max_price_precision = max_price_precision.min(FIXED_PRECISION);
            max_size_precision = max_size_precision.min(FIXED_PRECISION);

            (
                price_precision.unwrap_or(max_price_precision),
                size_precision.unwrap_or(max_size_precision),
            )
        }
    };

    let mut trades = Vec::new();
    let mut reader = create_csv_reader(filepath)?;
    let mut record = StringRecord::new();

    while reader.read_record(&mut record)? {
        let record: TardisTradeRecord = record.deserialize(None)?;

        let instrument_id = match &instrument_id {
            Some(id) => *id,
            None => parse_instrument_id(&record.exchange, record.symbol),
        };
        let price = parse_price(record.price, price_precision);
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
    use nautilus_model::{
        enums::{AggressorSide, BookAction},
        identifiers::InstrumentId,
        types::Price,
    };
    use nautilus_test_kit::common::{
        ensure_data_exists_tardis_binance_snapshot5, ensure_data_exists_tardis_binance_snapshot25,
        ensure_data_exists_tardis_bitmex_trades, ensure_data_exists_tardis_deribit_book_l2,
        ensure_data_exists_tardis_huobi_quotes,
    };
    use rstest::*;

    use super::*;

    #[rstest]
    #[case(Some(1), Some(0))] // Explicit precisions
    #[case(None, None)] // Inferred precisions
    pub fn test_read_deltas(
        #[case] price_precision: Option<u8>,
        #[case] size_precision: Option<u8>,
    ) {
        let filepath = ensure_data_exists_tardis_deribit_book_l2();
        let deltas = load_deltas(
            filepath,
            price_precision,
            size_precision,
            None,
            Some(10_000),
        )
        .unwrap();

        assert_eq!(deltas.len(), 10_000);
        assert_eq!(
            deltas[0].instrument_id,
            InstrumentId::from("BTC-PERPETUAL.DERIBIT")
        );
        assert_eq!(deltas[0].action, BookAction::Add);
        assert_eq!(deltas[0].order.side, OrderSide::Sell);
        assert_eq!(deltas[0].order.price, Price::from("6421.5"));
        assert_eq!(deltas[0].order.size, Quantity::from("18640"));
        assert_eq!(deltas[0].flags, 0);
        assert_eq!(deltas[0].sequence, 0);
        assert_eq!(deltas[0].ts_event, 1585699200245000000);
        assert_eq!(deltas[0].ts_init, 1585699200355684000);
    }

    #[rstest]
    #[case(Some(2), Some(3))] // Explicit precisions
    #[case(None, None)] // Inferred precisions
    pub fn test_read_depth10s_from_snapshot5(
        #[case] price_precision: Option<u8>,
        #[case] size_precision: Option<u8>,
    ) {
        let filepath = ensure_data_exists_tardis_binance_snapshot5();
        let depths = load_depth10_from_snapshot5(
            filepath,
            price_precision,
            size_precision,
            None,
            Some(10_000),
        )
        .unwrap();

        assert_eq!(depths.len(), 10_000);
        assert_eq!(
            depths[0].instrument_id,
            InstrumentId::from("BTCUSDT.BINANCE")
        );
        assert_eq!(depths[0].bids.len(), 10);
        assert_eq!(depths[0].bids[0].price, Price::from("11657.07"));
        assert_eq!(depths[0].bids[0].size, Quantity::from("10.896"));
        assert_eq!(depths[0].bids[0].side, OrderSide::Buy);
        assert_eq!(depths[0].bids[0].order_id, 0);
        assert_eq!(depths[0].asks.len(), 10);
        assert_eq!(depths[0].asks[0].price, Price::from("11657.08"));
        assert_eq!(depths[0].asks[0].size, Quantity::from("1.714"));
        assert_eq!(depths[0].asks[0].side, OrderSide::Sell);
        assert_eq!(depths[0].asks[0].order_id, 0);
        assert_eq!(depths[0].bid_counts[0], 1);
        assert_eq!(depths[0].ask_counts[0], 1);
        assert_eq!(depths[0].flags, 128);
        assert_eq!(depths[0].ts_event, 1598918403696000000);
        assert_eq!(depths[0].ts_init, 1598918403810979000);
        assert_eq!(depths[0].sequence, 0);
    }

    #[rstest]
    #[case(Some(2), Some(3))] // Explicit precisions
    #[case(None, None)] // Inferred precisions
    pub fn test_read_depth10s_from_snapshot25(
        #[case] price_precision: Option<u8>,
        #[case] size_precision: Option<u8>,
    ) {
        let filepath = ensure_data_exists_tardis_binance_snapshot25();
        let depths = load_depth10_from_snapshot25(
            filepath,
            price_precision,
            size_precision,
            None,
            Some(10_000),
        )
        .unwrap();

        assert_eq!(depths.len(), 10_000);
        assert_eq!(
            depths[0].instrument_id,
            InstrumentId::from("BTCUSDT.BINANCE")
        );
        assert_eq!(depths[0].bids.len(), 10);
        assert_eq!(depths[0].bids[0].price, Price::from("11657.07"));
        assert_eq!(depths[0].bids[0].size, Quantity::from("10.896"));
        assert_eq!(depths[0].bids[0].side, OrderSide::Buy);
        assert_eq!(depths[0].bids[0].order_id, 0);
        assert_eq!(depths[0].asks.len(), 10);
        assert_eq!(depths[0].asks[0].price, Price::from("11657.08"));
        assert_eq!(depths[0].asks[0].size, Quantity::from("1.714"));
        assert_eq!(depths[0].asks[0].side, OrderSide::Sell);
        assert_eq!(depths[0].asks[0].order_id, 0);
        assert_eq!(depths[0].bid_counts[0], 1);
        assert_eq!(depths[0].ask_counts[0], 1);
        assert_eq!(depths[0].flags, 128);
        assert_eq!(depths[0].ts_event, 1598918403696000000);
        assert_eq!(depths[0].ts_init, 1598918403810979000);
        assert_eq!(depths[0].sequence, 0);
    }

    #[rstest]
    #[case(Some(1), Some(0))] // Explicit precisions
    #[case(None, None)] // Inferred precisions
    pub fn test_read_quotes(
        #[case] price_precision: Option<u8>,
        #[case] size_precision: Option<u8>,
    ) {
        let filepath = ensure_data_exists_tardis_huobi_quotes();
        let quotes = load_quote_ticks(
            filepath,
            price_precision,
            size_precision,
            None,
            Some(10_000),
        )
        .unwrap();

        assert_eq!(quotes.len(), 10_000);
        assert_eq!(
            quotes[0].instrument_id,
            InstrumentId::from("BTC-USD.HUOBI_DELIVERY")
        );
        assert_eq!(quotes[0].bid_price, Price::from("8629.2"));
        assert_eq!(quotes[0].bid_size, Quantity::from("806"));
        assert_eq!(quotes[0].ask_price, Price::from("8629.3"));
        assert_eq!(quotes[0].ask_size, Quantity::from("5494"));
        assert_eq!(quotes[0].ts_event, 1588291201099000000);
        assert_eq!(quotes[0].ts_init, 1588291201234268000);
    }

    #[rstest]
    #[case(Some(1), Some(0))] // Explicit precisions
    #[case(None, None)] // Inferred precisions
    pub fn test_read_trades(
        #[case] price_precision: Option<u8>,
        #[case] size_precision: Option<u8>,
    ) {
        let filepath = ensure_data_exists_tardis_bitmex_trades();
        let trades = load_trade_ticks(
            filepath,
            price_precision,
            size_precision,
            None,
            Some(10_000),
        )
        .unwrap();

        assert_eq!(trades.len(), 10_000);
        assert_eq!(trades[0].instrument_id, InstrumentId::from("XBTUSD.BITMEX"));
        assert_eq!(trades[0].price, Price::from("8531.5"));
        assert_eq!(trades[0].size, Quantity::from("2152"));
        assert_eq!(trades[0].aggressor_side, AggressorSide::Seller);
        assert_eq!(
            trades[0].trade_id,
            TradeId::new("ccc3c1fa-212c-e8b0-1706-9b9c4f3d5ecf")
        );
        assert_eq!(trades[0].ts_event, 1583020803145000000);
        assert_eq!(trades[0].ts_init, 1583020803307160000);
    }
}
