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
    let mut buf = ryu::Buffer::new(); // Stack allocation
    let s = buf.format(value);

    match s.rsplit_once('.') {
        Some((_, frac)) if frac != "0" => frac.len() as u8,
        _ => 0,
    }
}

fn create_csv_reader<P: AsRef<Path>>(
    filepath: P,
) -> anyhow::Result<Reader<Box<dyn std::io::Read>>> {
    let filepath_ref = filepath.as_ref();
    const MAX_RETRIES: u8 = 3;
    const DELAY_MS: u64 = 100;
    const BUFFER_SIZE: usize = 8 * 1024 * 1024; // 8MB buffer for large files

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
        let buf_reader = BufReader::with_capacity(BUFFER_SIZE, file);
        return Ok(ReaderBuilder::new()
            .has_headers(true)
            .buffer_capacity(1024 * 1024) // 1MB CSV buffer
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

    let buf_reader = BufReader::with_capacity(BUFFER_SIZE, file);
    let decoder = GzDecoder::new(buf_reader);

    Ok(ReaderBuilder::new()
        .has_headers(true)
        .buffer_capacity(1024 * 1024) // 1MB CSV buffer
        .from_reader(Box::new(decoder)))
}

/// Loads [`OrderBookDelta`]s from a Tardis format CSV at the given `filepath`,
/// automatically applying `GZip` decompression for files ending in ".gz".
/// Load order book delta records from a CSV or gzipped CSV file.
///
/// # Errors
///
/// Returns an error if the file cannot be opened, read, or parsed as CSV.
/// # Panics
///
/// Panics if a CSV record has a zero size for a non-delete action or if data conversion fails.
pub fn load_deltas<P: AsRef<Path>>(
    filepath: P,
    price_precision: Option<u8>,
    size_precision: Option<u8>,
    instrument_id: Option<InstrumentId>,
    limit: Option<usize>,
) -> Result<Vec<OrderBookDelta>, Box<dyn Error>> {
    // Estimate capacity for Vec pre-allocation
    let estimated_capacity = limit.unwrap_or(1_000_000).min(10_000_000);
    let mut deltas: Vec<OrderBookDelta> = Vec::with_capacity(estimated_capacity);

    let mut current_price_precision = price_precision.unwrap_or(0);
    let mut current_size_precision = size_precision.unwrap_or(0);
    let mut last_ts_event = UnixNanos::default();

    let mut reader = create_csv_reader(filepath)?;
    let mut record = StringRecord::new();

    while reader.read_record(&mut record)? {
        let data: TardisBookUpdateRecord = record.deserialize(None)?;

        // Update precisions dynamically if not explicitly set
        let mut precision_updated = false;

        if price_precision.is_none() {
            let inferred_price_precision = infer_precision(data.price).min(FIXED_PRECISION);
            if inferred_price_precision > current_price_precision {
                current_price_precision = inferred_price_precision;
                precision_updated = true;
            }
        }

        if size_precision.is_none() {
            let inferred_size_precision = infer_precision(data.amount).min(FIXED_PRECISION);
            if inferred_size_precision > current_size_precision {
                current_size_precision = inferred_size_precision;
                precision_updated = true;
            }
        }

        // If precision increased, update all previous deltas
        if precision_updated {
            for delta in deltas.iter_mut() {
                if price_precision.is_none() {
                    delta.order.price.precision = current_price_precision;
                }
                if size_precision.is_none() {
                    delta.order.size.precision = current_size_precision;
                }
            }
        }

        let instrument_id = match &instrument_id {
            Some(id) => *id,
            None => parse_instrument_id(&data.exchange, data.symbol),
        };
        let side = parse_order_side(&data.side);
        let price = parse_price(data.price, current_price_precision);
        let size = Quantity::new(data.amount, current_size_precision);
        let order_id = 0; // Not applicable for L2 data
        let order = BookOrder::new(side, price, size, order_id);

        let action = parse_book_action(data.is_snapshot, size.as_f64());
        let flags = 0; // Flags always zero until timestamp changes
        let sequence = 0; // Sequence not available
        let ts_event = parse_timestamp(data.timestamp);
        let ts_init = parse_timestamp(data.local_timestamp);

        // Check if timestamp is different from last timestamp
        if last_ts_event != ts_event
            && let Some(last_delta) = deltas.last_mut()
        {
            // Set previous delta flags as F_LAST
            last_delta.flags = RecordFlag::F_LAST.value();
        }

        assert!(
            !(action != BookAction::Delete && size.is_zero()),
            "Invalid delta: action {action} when size zero, check size_precision ({current_size_precision}) vs data; {data:?}"
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

        if let Some(limit) = limit
            && deltas.len() >= limit
        {
            break;
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
/// Load order book depth-10 snapshots (5-level) from a CSV or gzipped CSV file.
///
/// # Errors
///
/// Returns an error if the file cannot be opened, read, or parsed as CSV.
/// # Panics
///
/// Panics if a record level cannot be parsed to depth-10.
pub fn load_depth10_from_snapshot5<P: AsRef<Path>>(
    filepath: P,
    price_precision: Option<u8>,
    size_precision: Option<u8>,
    instrument_id: Option<InstrumentId>,
    limit: Option<usize>,
) -> Result<Vec<OrderBookDepth10>, Box<dyn Error>> {
    // Estimate capacity for Vec pre-allocation
    let estimated_capacity = limit.unwrap_or(1_000_000).min(10_000_000);
    let mut depths: Vec<OrderBookDepth10> = Vec::with_capacity(estimated_capacity);

    let mut current_price_precision = price_precision.unwrap_or(0);
    let mut current_size_precision = size_precision.unwrap_or(0);

    let mut reader = create_csv_reader(filepath)?;
    let mut record = StringRecord::new();

    while reader.read_record(&mut record)? {
        let data: TardisOrderBookSnapshot5Record = record.deserialize(None)?;

        // Update precisions dynamically if not explicitly set
        let mut precision_updated = false;

        if price_precision.is_none() {
            if let Some(bid_price) = data.bids_0_price {
                let inferred_price_precision = infer_precision(bid_price).min(FIXED_PRECISION);
                if inferred_price_precision > current_price_precision {
                    current_price_precision = inferred_price_precision;
                    precision_updated = true;
                }
            }
        }

        if size_precision.is_none() {
            if let Some(bid_amount) = data.bids_0_amount {
                let inferred_size_precision = infer_precision(bid_amount).min(FIXED_PRECISION);
                if inferred_size_precision > current_size_precision {
                    current_size_precision = inferred_size_precision;
                    precision_updated = true;
                }
            }
        }

        // If precision increased, update all previous depths
        if precision_updated {
            for depth in depths.iter_mut() {
                for i in 0..DEPTH10_LEN {
                    if price_precision.is_none() {
                        depth.bids[i].price.precision = current_price_precision;
                        depth.asks[i].price.precision = current_price_precision;
                    }
                    if size_precision.is_none() {
                        depth.bids[i].size.precision = current_size_precision;
                        depth.asks[i].size.precision = current_size_precision;
                    }
                }
            }
        }

        let instrument_id = match &instrument_id {
            Some(id) => *id,
            None => parse_instrument_id(&data.exchange, data.symbol),
        };
        let flags = RecordFlag::F_LAST.value();
        let sequence = 0; // Sequence not available
        let ts_event = parse_timestamp(data.timestamp);
        let ts_init = parse_timestamp(data.local_timestamp);

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
                    0 => data.bids_0_price,
                    1 => data.bids_1_price,
                    2 => data.bids_2_price,
                    3 => data.bids_3_price,
                    4 => data.bids_4_price,
                    _ => panic!("Invalid level for snapshot5 -> depth10 parsing"),
                },
                match i {
                    0 => data.bids_0_amount,
                    1 => data.bids_1_amount,
                    2 => data.bids_2_amount,
                    3 => data.bids_3_amount,
                    4 => data.bids_4_amount,
                    _ => panic!("Invalid level for snapshot5 -> depth10 parsing"),
                },
                current_price_precision,
                current_size_precision,
            );
            bids[i] = bid_order;
            bid_counts[i] = bid_count;

            // Create asks
            let (ask_order, ask_count) = create_book_order(
                OrderSide::Sell,
                match i {
                    0 => data.asks_0_price,
                    1 => data.asks_1_price,
                    2 => data.asks_2_price,
                    3 => data.asks_3_price,
                    4 => data.asks_4_price,
                    _ => None, // Unreachable, but for safety
                },
                match i {
                    0 => data.asks_0_amount,
                    1 => data.asks_1_amount,
                    2 => data.asks_2_amount,
                    3 => data.asks_3_amount,
                    4 => data.asks_4_amount,
                    _ => None, // Unreachable, but for safety
                },
                current_price_precision,
                current_size_precision,
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

        if let Some(limit) = limit
            && depths.len() >= limit
        {
            break;
        }
    }

    Ok(depths)
}

/// Loads [`OrderBookDepth10`]s from a Tardis format CSV at the given `filepath`,
/// automatically applying `GZip` decompression for files ending in ".gz".
/// Load order book depth-10 snapshots (25-level) from a CSV or gzipped CSV file.
///
/// # Errors
///
/// Returns an error if the file cannot be opened, read, or parsed as CSV.
/// # Panics
///
/// Panics if a record level cannot be parsed to depth-10.
pub fn load_depth10_from_snapshot25<P: AsRef<Path>>(
    filepath: P,
    price_precision: Option<u8>,
    size_precision: Option<u8>,
    instrument_id: Option<InstrumentId>,
    limit: Option<usize>,
) -> Result<Vec<OrderBookDepth10>, Box<dyn Error>> {
    // Estimate capacity for Vec pre-allocation
    let estimated_capacity = limit.unwrap_or(1_000_000).min(10_000_000);
    let mut depths: Vec<OrderBookDepth10> = Vec::with_capacity(estimated_capacity);

    let mut current_price_precision = price_precision.unwrap_or(0);
    let mut current_size_precision = size_precision.unwrap_or(0);
    let mut reader = create_csv_reader(filepath)?;
    let mut record = StringRecord::new();

    while reader.read_record(&mut record)? {
        let data: TardisOrderBookSnapshot25Record = record.deserialize(None)?;

        // Update precisions dynamically if not explicitly set
        let mut precision_updated = false;

        if price_precision.is_none() {
            if let Some(bid_price) = data.bids_0_price {
                let inferred_price_precision = infer_precision(bid_price).min(FIXED_PRECISION);
                if inferred_price_precision > current_price_precision {
                    current_price_precision = inferred_price_precision;
                    precision_updated = true;
                }
            }
        }

        if size_precision.is_none() {
            if let Some(bid_amount) = data.bids_0_amount {
                let inferred_size_precision = infer_precision(bid_amount).min(FIXED_PRECISION);
                if inferred_size_precision > current_size_precision {
                    current_size_precision = inferred_size_precision;
                    precision_updated = true;
                }
            }
        }

        // If precision increased, update all previous depths
        if precision_updated {
            for depth in depths.iter_mut() {
                for i in 0..DEPTH10_LEN {
                    if price_precision.is_none() {
                        depth.bids[i].price.precision = current_price_precision;
                        depth.asks[i].price.precision = current_price_precision;
                    }
                    if size_precision.is_none() {
                        depth.bids[i].size.precision = current_size_precision;
                        depth.asks[i].size.precision = current_size_precision;
                    }
                }
            }
        }

        let instrument_id = match &instrument_id {
            Some(id) => *id,
            None => parse_instrument_id(&data.exchange, data.symbol),
        };
        let flags = RecordFlag::F_LAST.value();
        let sequence = 0; // Sequence not available
        let ts_event = parse_timestamp(data.timestamp);
        let ts_init = parse_timestamp(data.local_timestamp);

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
                    0 => data.bids_0_price,
                    1 => data.bids_1_price,
                    2 => data.bids_2_price,
                    3 => data.bids_3_price,
                    4 => data.bids_4_price,
                    5 => data.bids_5_price,
                    6 => data.bids_6_price,
                    7 => data.bids_7_price,
                    8 => data.bids_8_price,
                    9 => data.bids_9_price,
                    _ => panic!("Invalid level for snapshot25 -> depth10 parsing"),
                },
                match i {
                    0 => data.bids_0_amount,
                    1 => data.bids_1_amount,
                    2 => data.bids_2_amount,
                    3 => data.bids_3_amount,
                    4 => data.bids_4_amount,
                    5 => data.bids_5_amount,
                    6 => data.bids_6_amount,
                    7 => data.bids_7_amount,
                    8 => data.bids_8_amount,
                    9 => data.bids_9_amount,
                    _ => panic!("Invalid level for snapshot25 -> depth10 parsing"),
                },
                current_price_precision,
                current_size_precision,
            );
            bids[i] = bid_order;
            bid_counts[i] = bid_count;

            // Create asks
            let (ask_order, ask_count) = create_book_order(
                OrderSide::Sell,
                match i {
                    0 => data.asks_0_price,
                    1 => data.asks_1_price,
                    2 => data.asks_2_price,
                    3 => data.asks_3_price,
                    4 => data.asks_4_price,
                    5 => data.asks_5_price,
                    6 => data.asks_6_price,
                    7 => data.asks_7_price,
                    8 => data.asks_8_price,
                    9 => data.asks_9_price,
                    _ => panic!("Invalid level for snapshot25 -> depth10 parsing"),
                },
                match i {
                    0 => data.asks_0_amount,
                    1 => data.asks_1_amount,
                    2 => data.asks_2_amount,
                    3 => data.asks_3_amount,
                    4 => data.asks_4_amount,
                    5 => data.asks_5_amount,
                    6 => data.asks_6_amount,
                    7 => data.asks_7_amount,
                    8 => data.asks_8_amount,
                    9 => data.asks_9_amount,
                    _ => panic!("Invalid level for snapshot25 -> depth10 parsing"),
                },
                current_price_precision,
                current_size_precision,
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

        if let Some(limit) = limit
            && depths.len() >= limit
        {
            break;
        }
    }

    Ok(depths)
}

/// Loads [`QuoteTick`]s from a Tardis format CSV at the given `filepath`,
/// automatically applying `GZip` decompression for files ending in ".gz".
/// Load quote ticks from a CSV or gzipped CSV file.
///
/// # Errors
///
/// Returns an error if the file cannot be opened, read, or parsed as CSV.
/// # Panics
///
/// Panics if a record has invalid data or CSV parsing errors.
pub fn load_quote_ticks<P: AsRef<Path>>(
    filepath: P,
    price_precision: Option<u8>,
    size_precision: Option<u8>,
    instrument_id: Option<InstrumentId>,
    limit: Option<usize>,
) -> Result<Vec<QuoteTick>, Box<dyn Error>> {
    // Estimate capacity for Vec pre-allocation
    let estimated_capacity = limit.unwrap_or(1_000_000).min(10_000_000);
    let mut quotes: Vec<QuoteTick> = Vec::with_capacity(estimated_capacity);

    let mut current_price_precision = price_precision.unwrap_or(0);
    let mut current_size_precision = size_precision.unwrap_or(0);
    let mut reader = create_csv_reader(filepath)?;
    let mut record = StringRecord::new();

    while reader.read_record(&mut record)? {
        let data: TardisQuoteRecord = record.deserialize(None)?;

        // Update precisions dynamically if not explicitly set
        let mut precision_updated = false;

        if price_precision.is_none() {
            if let Some(bid_price) = data.bid_price {
                let inferred_price_precision = infer_precision(bid_price).min(FIXED_PRECISION);
                if inferred_price_precision > current_price_precision {
                    current_price_precision = inferred_price_precision;
                    precision_updated = true;
                }
            }
        }

        if size_precision.is_none() {
            if let Some(bid_amount) = data.bid_amount {
                let inferred_size_precision = infer_precision(bid_amount).min(FIXED_PRECISION);
                if inferred_size_precision > current_size_precision {
                    current_size_precision = inferred_size_precision;
                    precision_updated = true;
                }
            }
        }

        // If precision increased, update all previous quotes
        if precision_updated {
            for quote in quotes.iter_mut() {
                if price_precision.is_none() {
                    quote.bid_price.precision = current_price_precision;
                    quote.ask_price.precision = current_price_precision;
                }
                if size_precision.is_none() {
                    quote.bid_size.precision = current_size_precision;
                    quote.ask_size.precision = current_size_precision;
                }
            }
        }

        let instrument_id = match &instrument_id {
            Some(id) => *id,
            None => parse_instrument_id(&data.exchange, data.symbol),
        };
        let bid_price = parse_price(data.bid_price.unwrap_or(0.0), current_price_precision);
        let bid_size = Quantity::new(data.bid_amount.unwrap_or(0.0), current_size_precision);
        let ask_price = parse_price(data.ask_price.unwrap_or(0.0), current_price_precision);
        let ask_size = Quantity::new(data.ask_amount.unwrap_or(0.0), current_size_precision);
        let ts_event = parse_timestamp(data.timestamp);
        let ts_init = parse_timestamp(data.local_timestamp);

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

        if let Some(limit) = limit
            && quotes.len() >= limit
        {
            break;
        }
    }

    Ok(quotes)
}

/// Loads [`TradeTick`]s from a Tardis format CSV at the given `filepath`,
/// automatically applying `GZip` decompression for files ending in ".gz".
/// Load trade ticks from a CSV or gzipped CSV file.
///
/// # Errors
///
/// Returns an error if the file cannot be opened, read, or parsed as CSV.
/// # Panics
///
/// Panics if a record has invalid trade size or CSV parsing errors.
pub fn load_trade_ticks<P: AsRef<Path>>(
    filepath: P,
    price_precision: Option<u8>,
    size_precision: Option<u8>,
    instrument_id: Option<InstrumentId>,
    limit: Option<usize>,
) -> Result<Vec<TradeTick>, Box<dyn Error>> {
    // Estimate capacity for Vec pre-allocation
    let estimated_capacity = limit.unwrap_or(1_000_000).min(10_000_000);
    let mut trades: Vec<TradeTick> = Vec::with_capacity(estimated_capacity);

    let mut current_price_precision = price_precision.unwrap_or(0);
    let mut current_size_precision = size_precision.unwrap_or(0);
    let mut reader = create_csv_reader(filepath)?;
    let mut record = StringRecord::new();

    while reader.read_record(&mut record)? {
        let data: TardisTradeRecord = record.deserialize(None)?;

        // Update precisions dynamically if not explicitly set
        let mut precision_updated = false;

        if price_precision.is_none() {
            let inferred_price_precision = infer_precision(data.price).min(FIXED_PRECISION);
            if inferred_price_precision > current_price_precision {
                current_price_precision = inferred_price_precision;
                precision_updated = true;
            }
        }

        if size_precision.is_none() {
            let inferred_size_precision = infer_precision(data.amount).min(FIXED_PRECISION);
            if inferred_size_precision > current_size_precision {
                current_size_precision = inferred_size_precision;
                precision_updated = true;
            }
        }

        // If precision increased, update all previous trades
        if precision_updated {
            for trade in trades.iter_mut() {
                if price_precision.is_none() {
                    trade.price.precision = current_price_precision;
                }
                if size_precision.is_none() {
                    trade.size.precision = current_size_precision;
                }
            }
        }

        let instrument_id = match &instrument_id {
            Some(id) => *id,
            None => parse_instrument_id(&data.exchange, data.symbol),
        };
        let price = parse_price(data.price, current_price_precision);
        let size = Quantity::non_zero_checked(data.amount, current_size_precision)
            .unwrap_or_else(|e| panic!("Invalid {data:?}: size {e}"));
        let aggressor_side = parse_aggressor_side(&data.side);
        let trade_id = TradeId::new(&data.id);
        let ts_event = parse_timestamp(data.timestamp);
        let ts_init = parse_timestamp(data.local_timestamp);

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

        if let Some(limit) = limit
            && trades.len() >= limit
        {
            break;
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
    use nautilus_testkit::common::{
        ensure_data_exists_tardis_binance_snapshot5, ensure_data_exists_tardis_binance_snapshot25,
        ensure_data_exists_tardis_bitmex_trades, ensure_data_exists_tardis_deribit_book_l2,
        ensure_data_exists_tardis_huobi_quotes,
    };
    use rstest::*;

    use super::*;

    #[rstest]
    #[case(0.0, 0)]
    #[case(42.0, 0)]
    #[case(0.1, 1)]
    #[case(0.25, 2)]
    #[case(123.0001, 4)]
    #[case(-42.987654321,       9)]
    #[case(1.234_567_890_123, 12)]
    fn test_infer_precision(#[case] input: f64, #[case] expected: u8) {
        assert_eq!(infer_precision(input), expected);
    }

    #[rstest]
    pub fn test_dynamic_precision_inference() {
        // Create synthetic CSV data with increasing precision
        let csv_data = "exchange,symbol,timestamp,local_timestamp,is_snapshot,side,price,amount
binance-futures,BTCUSDT,1640995200000000,1640995200100000,true,ask,50000.0,1.0
binance-futures,BTCUSDT,1640995201000000,1640995201100000,false,bid,49999.5,2.0
binance-futures,BTCUSDT,1640995202000000,1640995202100000,false,ask,50000.12,1.5
binance-futures,BTCUSDT,1640995203000000,1640995203100000,false,bid,49999.123,3.0
binance-futures,BTCUSDT,1640995204000000,1640995204100000,false,ask,50000.1234,0.5";

        // Write to temporary file
        let temp_file = std::env::temp_dir().join("test_dynamic_precision.csv");
        std::fs::write(&temp_file, csv_data).unwrap();

        // Load with precision inference
        let deltas = load_deltas(&temp_file, None, None, None, None).unwrap();

        // Should have 5 deltas
        assert_eq!(deltas.len(), 5);

        // All deltas should have the maximum precision found (4 for price, 1 for size)
        for (i, delta) in deltas.iter().enumerate() {
            assert_eq!(
                delta.order.price.precision, 4,
                "Price precision should be 4 for delta {i}",
            );
            assert_eq!(
                delta.order.size.precision, 1,
                "Size precision should be 1 for delta {i}",
            );
        }

        // Test exact values to catch precision issues
        // First delta: 50000.0 with precision 4 should equal 50000.0000
        assert_eq!(deltas[0].order.price, parse_price(50000.0, 4));
        assert_eq!(deltas[0].order.size, Quantity::new(1.0, 1));

        // Second delta: 49999.5 with precision 4 should equal 49999.5000
        assert_eq!(deltas[1].order.price, parse_price(49999.5, 4));
        assert_eq!(deltas[1].order.size, Quantity::new(2.0, 1));

        // Third delta: 50000.12 with precision 4 should equal 50000.1200
        assert_eq!(deltas[2].order.price, parse_price(50000.12, 4));
        assert_eq!(deltas[2].order.size, Quantity::new(1.5, 1));

        // Fourth delta: 49999.123 with precision 4 should equal 49999.1230
        assert_eq!(deltas[3].order.price, parse_price(49999.123, 4));
        assert_eq!(deltas[3].order.size, Quantity::new(3.0, 1));

        // Fifth delta: 50000.1234 with precision 4 should equal 50000.1234
        assert_eq!(deltas[4].order.price, parse_price(50000.1234, 4));
        assert_eq!(deltas[4].order.size, Quantity::new(0.5, 1));

        // Test that early records (with lower input precision) match expected values
        // This verifies retroactive precision updates work correctly
        assert_eq!(
            deltas[0].order.price.precision,
            deltas[4].order.price.precision
        );
        assert_eq!(
            deltas[0].order.size.precision,
            deltas[2].order.size.precision
        );

        // Clean up
        std::fs::remove_file(&temp_file).ok();
    }

    // TODO: Flaky in CI, potentially from syncing large test data files from cache
    #[ignore = "Flaky test: called `Result::unwrap()` on an `Err` value: Error(Io(Kind(UnexpectedEof)))"]
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

    // TODO: Flaky in CI, potentially from syncing large test data files from cache
    #[ignore = "Flaky test: called `Result::unwrap()` on an `Err` value: Error(Io(Kind(UnexpectedEof)))"]
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

    // TODO: Flaky in CI, potentially from syncing large test data files from cache
    #[ignore = "Flaky test: called `Result::unwrap()` on an `Err` value: Error(Io(Kind(UnexpectedEof)))"]
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

    // TODO: Flaky in CI, potentially from syncing large test data files from cache
    #[ignore = "Flaky test: called `Result::unwrap()` on an `Err` value: Error(Io(Kind(UnexpectedEof)))"]
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

    // TODO: Flaky in CI, potentially from syncing large test data files from cache
    #[ignore = "Flaky test: called `Result::unwrap()` on an `Err` value: Error(Io(Kind(UnexpectedEof)))"]
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
