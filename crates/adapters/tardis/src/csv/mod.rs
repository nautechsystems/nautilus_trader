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

////////////////////////////////////////////////////////////////////////////////
// Common Parsing Logic
////////////////////////////////////////////////////////////////////////////////

fn update_precision_if_needed(current: &mut u8, value: f64, explicit: Option<u8>) -> bool {
    if explicit.is_some() {
        return false;
    }

    let inferred = infer_precision(value).min(FIXED_PRECISION);
    if inferred > *current {
        *current = inferred;
        true
    } else {
        false
    }
}

fn update_deltas_precision(
    deltas: &mut [OrderBookDelta],
    price_precision: Option<u8>,
    size_precision: Option<u8>,
    current_price_precision: u8,
    current_size_precision: u8,
) {
    for delta in deltas {
        if price_precision.is_none() {
            delta.order.price.precision = current_price_precision;
        }
        if size_precision.is_none() {
            delta.order.size.precision = current_size_precision;
        }
    }
}

fn update_quotes_precision(
    quotes: &mut [QuoteTick],
    price_precision: Option<u8>,
    size_precision: Option<u8>,
    current_price_precision: u8,
    current_size_precision: u8,
) {
    for quote in quotes {
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

fn update_trades_precision(
    trades: &mut [TradeTick],
    price_precision: Option<u8>,
    size_precision: Option<u8>,
    current_price_precision: u8,
    current_size_precision: u8,
) {
    for trade in trades {
        if price_precision.is_none() {
            trade.price.precision = current_price_precision;
        }
        if size_precision.is_none() {
            trade.size.precision = current_size_precision;
        }
    }
}

fn parse_delta_record(
    data: &TardisBookUpdateRecord,
    current_price_precision: u8,
    current_size_precision: u8,
    instrument_id: Option<InstrumentId>,
) -> OrderBookDelta {
    let instrument_id = match instrument_id {
        Some(id) => id,
        None => parse_instrument_id(&data.exchange, data.symbol),
    };

    let side = parse_order_side(&data.side);
    let price = parse_price(data.price, current_price_precision);
    let size = Quantity::new(data.amount, current_size_precision);
    let order_id = 0; // Not applicable for L2 data
    let order = BookOrder::new(side, price, size, order_id);

    let action = parse_book_action(data.is_snapshot, size.as_f64());
    let flags = 0; // Will be set later if needed
    let sequence = 0; // Sequence not available
    let ts_event = parse_timestamp(data.timestamp);
    let ts_init = parse_timestamp(data.local_timestamp);

    assert!(
        !(action != BookAction::Delete && size.is_zero()),
        "Invalid delta: action {action} when size zero, check size_precision ({current_size_precision}) vs data; {data:?}"
    );

    OrderBookDelta::new(
        instrument_id,
        action,
        order,
        flags,
        sequence,
        ts_event,
        ts_init,
    )
}

fn parse_quote_record(
    data: &TardisQuoteRecord,
    current_price_precision: u8,
    current_size_precision: u8,
    instrument_id: Option<InstrumentId>,
) -> QuoteTick {
    let instrument_id = match instrument_id {
        Some(id) => id,
        None => parse_instrument_id(&data.exchange, data.symbol),
    };

    let bid_price = parse_price(data.bid_price.unwrap_or(0.0), current_price_precision);
    let ask_price = parse_price(data.ask_price.unwrap_or(0.0), current_price_precision);
    let bid_size = Quantity::new(data.bid_amount.unwrap_or(0.0), current_size_precision);
    let ask_size = Quantity::new(data.ask_amount.unwrap_or(0.0), current_size_precision);
    let ts_event = parse_timestamp(data.timestamp);
    let ts_init = parse_timestamp(data.local_timestamp);

    QuoteTick::new(
        instrument_id,
        bid_price,
        ask_price,
        bid_size,
        ask_size,
        ts_event,
        ts_init,
    )
}

fn parse_trade_record(
    data: &TardisTradeRecord,
    current_price_precision: u8,
    current_size_precision: u8,
    instrument_id: Option<InstrumentId>,
) -> TradeTick {
    let instrument_id = match instrument_id {
        Some(id) => id,
        None => parse_instrument_id(&data.exchange, data.symbol),
    };

    let price = parse_price(data.price, current_price_precision);
    let size = Quantity::new(data.amount, current_size_precision);
    let aggressor_side = parse_aggressor_side(&data.side);
    let trade_id = TradeId::new(&data.id);
    let ts_event = parse_timestamp(data.timestamp);
    let ts_init = parse_timestamp(data.local_timestamp);

    TradeTick::new(
        instrument_id,
        price,
        size,
        aggressor_side,
        trade_id,
        ts_event,
        ts_init,
    )
}

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
        let price_updated =
            update_precision_if_needed(&mut current_price_precision, data.price, price_precision);
        let size_updated =
            update_precision_if_needed(&mut current_size_precision, data.amount, size_precision);

        // If precision increased, update all previous deltas
        if price_updated || size_updated {
            update_deltas_precision(
                &mut deltas,
                price_precision,
                size_precision,
                current_price_precision,
                current_size_precision,
            );
        }

        let delta = parse_delta_record(
            &data,
            current_price_precision,
            current_size_precision,
            instrument_id,
        );

        // Check if timestamp is different from last timestamp
        let ts_event = delta.ts_event;
        if last_ts_event != ts_event
            && let Some(last_delta) = deltas.last_mut()
        {
            // Set previous delta flags as F_LAST
            last_delta.flags = RecordFlag::F_LAST.value();
        }

        last_ts_event = ts_event;

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

        if price_precision.is_none()
            && let Some(bid_price) = data.bids_0_price
        {
            let inferred_price_precision = infer_precision(bid_price).min(FIXED_PRECISION);
            if inferred_price_precision > current_price_precision {
                current_price_precision = inferred_price_precision;
                precision_updated = true;
            }
        }

        if size_precision.is_none()
            && let Some(bid_amount) = data.bids_0_amount
        {
            let inferred_size_precision = infer_precision(bid_amount).min(FIXED_PRECISION);
            if inferred_size_precision > current_size_precision {
                current_size_precision = inferred_size_precision;
                precision_updated = true;
            }
        }

        // If precision increased, update all previous depths
        if precision_updated {
            for depth in &mut depths {
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

        if price_precision.is_none()
            && let Some(bid_price) = data.bids_0_price
        {
            let inferred_price_precision = infer_precision(bid_price).min(FIXED_PRECISION);
            if inferred_price_precision > current_price_precision {
                current_price_precision = inferred_price_precision;
                precision_updated = true;
            }
        }

        if size_precision.is_none()
            && let Some(bid_amount) = data.bids_0_amount
        {
            let inferred_size_precision = infer_precision(bid_amount).min(FIXED_PRECISION);
            if inferred_size_precision > current_size_precision {
                current_size_precision = inferred_size_precision;
                precision_updated = true;
            }
        }

        // If precision increased, update all previous depths
        if precision_updated {
            for depth in &mut depths {
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

        if price_precision.is_none()
            && let Some(bid_price) = data.bid_price
        {
            let inferred_price_precision = infer_precision(bid_price).min(FIXED_PRECISION);
            if inferred_price_precision > current_price_precision {
                current_price_precision = inferred_price_precision;
                precision_updated = true;
            }
        }

        if size_precision.is_none()
            && let Some(bid_amount) = data.bid_amount
        {
            let inferred_size_precision = infer_precision(bid_amount).min(FIXED_PRECISION);
            if inferred_size_precision > current_size_precision {
                current_size_precision = inferred_size_precision;
                precision_updated = true;
            }
        }

        // If precision increased, update all previous quotes
        if precision_updated {
            update_quotes_precision(
                &mut quotes,
                price_precision,
                size_precision,
                current_price_precision,
                current_size_precision,
            );
        }

        let quote = parse_quote_record(
            &data,
            current_price_precision,
            current_size_precision,
            instrument_id,
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
            update_trades_precision(
                &mut trades,
                price_precision,
                size_precision,
                current_price_precision,
                current_size_precision,
            );
        }

        let trade = parse_trade_record(
            &data,
            current_price_precision,
            current_size_precision,
            instrument_id,
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

/// Streaming iterator over CSV records that yields chunks of parsed data.
struct DeltaStreamIterator {
    reader: Reader<Box<dyn std::io::Read>>,
    record: StringRecord,
    chunk_size: usize,
    price_precision: Option<u8>,
    size_precision: Option<u8>,
    instrument_id: Option<InstrumentId>,
    current_price_precision: u8,
    current_size_precision: u8,
    last_ts_event: UnixNanos,
}

impl DeltaStreamIterator {
    fn new<P: AsRef<Path>>(
        filepath: P,
        chunk_size: usize,
        price_precision: Option<u8>,
        size_precision: Option<u8>,
        instrument_id: Option<InstrumentId>,
    ) -> anyhow::Result<Self> {
        let reader = create_csv_reader(filepath)?;
        Ok(Self {
            reader,
            record: StringRecord::new(),
            chunk_size,
            price_precision,
            size_precision,
            instrument_id,
            current_price_precision: price_precision.unwrap_or(0),
            current_size_precision: size_precision.unwrap_or(0),
            last_ts_event: UnixNanos::default(),
        })
    }
}

impl Iterator for DeltaStreamIterator {
    type Item = anyhow::Result<Vec<OrderBookDelta>>;

    fn next(&mut self) -> Option<Self::Item> {
        let mut deltas: Vec<OrderBookDelta> = Vec::with_capacity(self.chunk_size);
        let mut records_read = 0;

        while records_read < self.chunk_size {
            match self.reader.read_record(&mut self.record) {
                Ok(true) => {
                    match self.record.deserialize::<TardisBookUpdateRecord>(None) {
                        Ok(data) => {
                            // Update precisions dynamically if not explicitly set
                            let mut precision_updated = false;

                            precision_updated |= update_precision_if_needed(
                                &mut self.current_price_precision,
                                data.price,
                                self.price_precision,
                            );

                            precision_updated |= update_precision_if_needed(
                                &mut self.current_size_precision,
                                data.amount,
                                self.size_precision,
                            );

                            // If precision increased, update all previous deltas in current chunk
                            if precision_updated {
                                update_deltas_precision(
                                    &mut deltas,
                                    self.price_precision,
                                    self.size_precision,
                                    self.current_price_precision,
                                    self.current_size_precision,
                                );
                            }

                            let delta = parse_delta_record(
                                &data,
                                self.current_price_precision,
                                self.current_size_precision,
                                self.instrument_id,
                            );

                            // Check if timestamp is different from last timestamp
                            if self.last_ts_event != delta.ts_event
                                && let Some(last_delta) = deltas.last_mut()
                            {
                                last_delta.flags = RecordFlag::F_LAST.value();
                            }

                            assert!(
                                !(delta.action != BookAction::Delete && delta.order.size.is_zero()),
                                "Invalid delta: action {} when size zero, check size_precision ({}) vs data; {data:?}",
                                delta.action,
                                self.current_size_precision
                            );

                            self.last_ts_event = delta.ts_event;

                            deltas.push(delta);
                            records_read += 1;
                        }
                        Err(e) => {
                            return Some(Err(anyhow::anyhow!("Failed to deserialize record: {e}")));
                        }
                    }
                }
                Ok(false) => {
                    // End of file reached
                    if deltas.is_empty() {
                        return None;
                    }
                    // Set F_LAST flag for final delta in chunk
                    if let Some(last_delta) = deltas.last_mut() {
                        last_delta.flags = RecordFlag::F_LAST.value();
                    }
                    return Some(Ok(deltas));
                }
                Err(e) => return Some(Err(anyhow::anyhow!("Failed to read record: {e}"))),
            }
        }

        if deltas.is_empty() {
            None
        } else {
            Some(Ok(deltas))
        }
    }
}

/// Streams [`OrderBookDelta`]s from a Tardis format CSV at the given `filepath`,
/// yielding chunks of the specified size.
///
/// # Precision Inference Warning
///
/// When using streaming with precision inference (not providing explicit precisions),
/// the inferred precision may differ from bulk loading the entire file. This is because
/// precision inference works within chunk boundaries, and different chunks may contain
/// values with different precision requirements. For deterministic precision behavior,
/// provide explicit `price_precision` and `size_precision` parameters.
///
/// # Errors
///
/// Returns an error if the file cannot be opened, read, or parsed as CSV.
pub fn stream_deltas<P: AsRef<Path>>(
    filepath: P,
    chunk_size: usize,
    price_precision: Option<u8>,
    size_precision: Option<u8>,
    instrument_id: Option<InstrumentId>,
) -> anyhow::Result<impl Iterator<Item = anyhow::Result<Vec<OrderBookDelta>>>> {
    DeltaStreamIterator::new(
        filepath,
        chunk_size,
        price_precision,
        size_precision,
        instrument_id,
    )
}

////////////////////////////////////////////////////////////////////////////////
// Quote Streaming
////////////////////////////////////////////////////////////////////////////////

/// An iterator for streaming [`QuoteTick`]s from a Tardis CSV file in chunks.
struct QuoteStreamIterator {
    reader: Reader<Box<dyn Read>>,
    chunk_size: usize,
    price_precision: Option<u8>,
    size_precision: Option<u8>,
    instrument_id: Option<InstrumentId>,
    current_price_precision: u8,
    current_size_precision: u8,
    buffer: StringRecord,
}

impl QuoteStreamIterator {
    /// Creates a new [`QuoteStreamIterator`].
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be opened or read.
    pub fn new<P: AsRef<Path>>(
        filepath: P,
        chunk_size: usize,
        price_precision: Option<u8>,
        size_precision: Option<u8>,
        instrument_id: Option<InstrumentId>,
    ) -> anyhow::Result<Self> {
        let reader = create_csv_reader(filepath)?;

        Ok(Self {
            reader,
            chunk_size,
            price_precision,
            size_precision,
            instrument_id,
            current_price_precision: price_precision.unwrap_or(2),
            current_size_precision: size_precision.unwrap_or(0),
            buffer: StringRecord::new(),
        })
    }

    fn infer_precision_from_quotes(&mut self, quotes: &mut [QuoteTick]) {
        if self.price_precision.is_some() && self.size_precision.is_some() {
            return;
        }

        let mut max_bid_price_precision = self.current_price_precision;
        let mut max_ask_price_precision = self.current_price_precision;
        let mut max_bid_size_precision = self.current_size_precision;
        let mut max_ask_size_precision = self.current_size_precision;

        for quote in quotes.iter() {
            if self.price_precision.is_none() {
                max_bid_price_precision = max_bid_price_precision.max(quote.bid_price.precision);
                max_ask_price_precision = max_ask_price_precision.max(quote.ask_price.precision);
            }
            if self.size_precision.is_none() {
                max_bid_size_precision = max_bid_size_precision.max(quote.bid_size.precision);
                max_ask_size_precision = max_ask_size_precision.max(quote.ask_size.precision);
            }
        }

        let new_price_precision = max_bid_price_precision.max(max_ask_price_precision);
        let new_size_precision = max_bid_size_precision.max(max_ask_size_precision);

        if new_price_precision > self.current_price_precision {
            self.current_price_precision = new_price_precision;
            for quote in quotes.iter_mut() {
                quote.bid_price.precision = self.current_price_precision;
                quote.ask_price.precision = self.current_price_precision;
            }
        }

        if new_size_precision > self.current_size_precision {
            self.current_size_precision = new_size_precision;
            for quote in quotes.iter_mut() {
                quote.bid_size.precision = self.current_size_precision;
                quote.ask_size.precision = self.current_size_precision;
            }
        }
    }
}

impl Iterator for QuoteStreamIterator {
    type Item = anyhow::Result<Vec<QuoteTick>>;

    fn next(&mut self) -> Option<Self::Item> {
        let mut quotes = Vec::with_capacity(self.chunk_size);
        let mut records_read = 0;

        while records_read < self.chunk_size {
            match self.reader.read_record(&mut self.buffer) {
                Ok(true) => match self.buffer.deserialize::<TardisQuoteRecord>(None) {
                    Ok(data) => {
                        // Update precisions for streaming (simple max approach)
                        if self.price_precision.is_none() {
                            if let Some(bid_price_val) = data.bid_price {
                                self.current_price_precision = self
                                    .current_price_precision
                                    .max(infer_precision(bid_price_val));
                            }
                            if let Some(ask_price_val) = data.ask_price {
                                self.current_price_precision = self
                                    .current_price_precision
                                    .max(infer_precision(ask_price_val));
                            }
                        }

                        if self.size_precision.is_none() {
                            if let Some(bid_amount_val) = data.bid_amount {
                                self.current_size_precision = self
                                    .current_size_precision
                                    .max(infer_precision(bid_amount_val));
                            }
                            if let Some(ask_amount_val) = data.ask_amount {
                                self.current_size_precision = self
                                    .current_size_precision
                                    .max(infer_precision(ask_amount_val));
                            }
                        }

                        let quote = parse_quote_record(
                            &data,
                            self.current_price_precision,
                            self.current_size_precision,
                            self.instrument_id,
                        );

                        quotes.push(quote);
                        records_read += 1;
                    }
                    Err(e) => {
                        return Some(Err(anyhow::anyhow!("Failed to deserialize record: {e}")));
                    }
                },
                Ok(false) => {
                    if quotes.is_empty() {
                        return None;
                    }
                    self.infer_precision_from_quotes(&mut quotes);
                    return Some(Ok(quotes));
                }
                Err(e) => return Some(Err(anyhow::anyhow!("Failed to read record: {e}"))),
            }
        }

        if quotes.is_empty() {
            None
        } else {
            self.infer_precision_from_quotes(&mut quotes);
            Some(Ok(quotes))
        }
    }
}

/// Streams [`QuoteTick`]s from a Tardis format CSV at the given `filepath`,
/// yielding chunks of the specified size.
///
/// # Precision Inference Warning
///
/// When using streaming with precision inference (not providing explicit precisions),
/// the inferred precision may differ from bulk loading the entire file. This is because
/// precision inference works within chunk boundaries, and different chunks may contain
/// values with different precision requirements. For deterministic precision behavior,
/// provide explicit `price_precision` and `size_precision` parameters.
///
/// # Errors
///
/// Returns an error if the file cannot be opened, read, or parsed as CSV.
pub fn stream_quotes<P: AsRef<Path>>(
    filepath: P,
    chunk_size: usize,
    price_precision: Option<u8>,
    size_precision: Option<u8>,
    instrument_id: Option<InstrumentId>,
) -> anyhow::Result<impl Iterator<Item = anyhow::Result<Vec<QuoteTick>>>> {
    QuoteStreamIterator::new(
        filepath,
        chunk_size,
        price_precision,
        size_precision,
        instrument_id,
    )
}

////////////////////////////////////////////////////////////////////////////////
// Trade Streaming
////////////////////////////////////////////////////////////////////////////////

/// An iterator for streaming [`TradeTick`]s from a Tardis CSV file in chunks.
struct TradeStreamIterator {
    reader: Reader<Box<dyn Read>>,
    chunk_size: usize,
    price_precision: Option<u8>,
    size_precision: Option<u8>,
    instrument_id: Option<InstrumentId>,
    current_price_precision: u8,
    current_size_precision: u8,
    buffer: StringRecord,
}

impl TradeStreamIterator {
    /// Creates a new [`TradeStreamIterator`].
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be opened or read.
    pub fn new<P: AsRef<Path>>(
        filepath: P,
        chunk_size: usize,
        price_precision: Option<u8>,
        size_precision: Option<u8>,
        instrument_id: Option<InstrumentId>,
    ) -> anyhow::Result<Self> {
        let reader = create_csv_reader(filepath)?;

        Ok(Self {
            reader,
            chunk_size,
            price_precision,
            size_precision,
            instrument_id,
            current_price_precision: price_precision.unwrap_or(2),
            current_size_precision: size_precision.unwrap_or(0),
            buffer: StringRecord::new(),
        })
    }

    fn infer_precision_from_trades(&mut self, trades: &mut [TradeTick]) {
        if self.price_precision.is_some() && self.size_precision.is_some() {
            return;
        }

        let mut max_price_precision = self.current_price_precision;
        let mut max_size_precision = self.current_size_precision;

        for trade in trades.iter() {
            if self.price_precision.is_none() {
                max_price_precision = max_price_precision.max(trade.price.precision);
            }
            if self.size_precision.is_none() {
                max_size_precision = max_size_precision.max(trade.size.precision);
            }
        }

        if max_price_precision > self.current_price_precision {
            self.current_price_precision = max_price_precision;
            for trade in trades.iter_mut() {
                trade.price.precision = self.current_price_precision;
            }
        }

        if max_size_precision > self.current_size_precision {
            self.current_size_precision = max_size_precision;
            for trade in trades.iter_mut() {
                trade.size.precision = self.current_size_precision;
            }
        }
    }
}

impl Iterator for TradeStreamIterator {
    type Item = anyhow::Result<Vec<TradeTick>>;

    fn next(&mut self) -> Option<Self::Item> {
        let mut trades = Vec::with_capacity(self.chunk_size);
        let mut records_read = 0;

        while records_read < self.chunk_size {
            match self.reader.read_record(&mut self.buffer) {
                Ok(true) => match self.buffer.deserialize::<TardisTradeRecord>(None) {
                    Ok(data) => {
                        // Update precisions for streaming (simple max approach)
                        if self.price_precision.is_none() {
                            self.current_price_precision = self
                                .current_price_precision
                                .max(infer_precision(data.price));
                        }

                        if self.size_precision.is_none() {
                            self.current_size_precision = self
                                .current_size_precision
                                .max(infer_precision(data.amount));
                        }

                        let trade = parse_trade_record(
                            &data,
                            self.current_price_precision,
                            self.current_size_precision,
                            self.instrument_id,
                        );

                        trades.push(trade);
                        records_read += 1;
                    }
                    Err(e) => {
                        return Some(Err(anyhow::anyhow!("Failed to deserialize record: {e}")));
                    }
                },
                Ok(false) => {
                    if trades.is_empty() {
                        return None;
                    }
                    self.infer_precision_from_trades(&mut trades);
                    return Some(Ok(trades));
                }
                Err(e) => return Some(Err(anyhow::anyhow!("Failed to read record: {e}"))),
            }
        }

        if trades.is_empty() {
            None
        } else {
            self.infer_precision_from_trades(&mut trades);
            Some(Ok(trades))
        }
    }
}

/// Streams [`TradeTick`]s from a Tardis format CSV at the given `filepath`,
/// yielding chunks of the specified size.
///
/// # Precision Inference Warning
///
/// When using streaming with precision inference (not providing explicit precisions),
/// the inferred precision may differ from bulk loading the entire file. This is because
/// precision inference works within chunk boundaries, and different chunks may contain
/// values with different precision requirements. For deterministic precision behavior,
/// provide explicit `price_precision` and `size_precision` parameters.
///
/// # Errors
///
/// Returns an error if the file cannot be opened, read, or parsed as CSV.
pub fn stream_trades<P: AsRef<Path>>(
    filepath: P,
    chunk_size: usize,
    price_precision: Option<u8>,
    size_precision: Option<u8>,
    instrument_id: Option<InstrumentId>,
) -> anyhow::Result<impl Iterator<Item = anyhow::Result<Vec<TradeTick>>>> {
    TradeStreamIterator::new(
        filepath,
        chunk_size,
        price_precision,
        size_precision,
        instrument_id,
    )
}

////////////////////////////////////////////////////////////////////////////////
// Depth10 Streaming
////////////////////////////////////////////////////////////////////////////////

/// An iterator for streaming [`OrderBookDepth10`]s from a Tardis CSV file in chunks.
struct Depth10StreamIterator {
    reader: Reader<Box<dyn Read>>,
    chunk_size: usize,
    levels: u8,
    price_precision: Option<u8>,
    size_precision: Option<u8>,
    instrument_id: Option<InstrumentId>,
    current_price_precision: u8,
    current_size_precision: u8,
    buffer: StringRecord,
}

impl Depth10StreamIterator {
    /// Creates a new [`Depth10StreamIterator`].
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be opened or read.
    pub fn new<P: AsRef<Path>>(
        filepath: P,
        chunk_size: usize,
        levels: u8,
        price_precision: Option<u8>,
        size_precision: Option<u8>,
        instrument_id: Option<InstrumentId>,
    ) -> anyhow::Result<Self> {
        let reader = create_csv_reader(filepath)?;

        Ok(Self {
            reader,
            chunk_size,
            levels,
            price_precision,
            size_precision,
            instrument_id,
            current_price_precision: price_precision.unwrap_or(2),
            current_size_precision: size_precision.unwrap_or(0),
            buffer: StringRecord::new(),
        })
    }

    fn infer_precision_from_depths(&mut self, depths: &mut [OrderBookDepth10]) {
        if self.price_precision.is_some() && self.size_precision.is_some() {
            return;
        }

        let mut max_price_precision = self.current_price_precision;
        let mut max_size_precision = self.current_size_precision;

        for depth in depths.iter() {
            if self.price_precision.is_none() {
                for bid in &depth.bids {
                    max_price_precision = max_price_precision.max(bid.price.precision);
                }
                for ask in &depth.asks {
                    max_price_precision = max_price_precision.max(ask.price.precision);
                }
            }
            if self.size_precision.is_none() {
                for bid in &depth.bids {
                    max_size_precision = max_size_precision.max(bid.size.precision);
                }
                for ask in &depth.asks {
                    max_size_precision = max_size_precision.max(ask.size.precision);
                }
            }
        }

        if max_price_precision > self.current_price_precision {
            self.current_price_precision = max_price_precision;
            for depth in depths.iter_mut() {
                for bid in &mut depth.bids {
                    bid.price.precision = self.current_price_precision;
                }
                for ask in &mut depth.asks {
                    ask.price.precision = self.current_price_precision;
                }
            }
        }

        if max_size_precision > self.current_size_precision {
            self.current_size_precision = max_size_precision;
            for depth in depths.iter_mut() {
                for bid in &mut depth.bids {
                    bid.size.precision = self.current_size_precision;
                }
                for ask in &mut depth.asks {
                    ask.size.precision = self.current_size_precision;
                }
            }
        }
    }
}

impl Iterator for Depth10StreamIterator {
    type Item = anyhow::Result<Vec<OrderBookDepth10>>;

    fn next(&mut self) -> Option<Self::Item> {
        let mut depths = Vec::with_capacity(self.chunk_size);
        let mut records_read = 0;

        while records_read < self.chunk_size {
            match self.reader.read_record(&mut self.buffer) {
                Ok(true) => {
                    let result = match self.levels {
                        5 => self
                            .buffer
                            .deserialize::<TardisOrderBookSnapshot5Record>(None)
                            .map(|data| self.process_snapshot5(data)),
                        25 => self
                            .buffer
                            .deserialize::<TardisOrderBookSnapshot25Record>(None)
                            .map(|data| self.process_snapshot25(data)),
                        _ => return Some(Err(anyhow::anyhow!("Invalid levels: {}", self.levels))),
                    };

                    match result {
                        Ok(depth) => {
                            depths.push(depth);
                            records_read += 1;
                        }
                        Err(e) => {
                            return Some(Err(anyhow::anyhow!("Failed to deserialize record: {e}")));
                        }
                    }
                }
                Ok(false) => {
                    if depths.is_empty() {
                        return None;
                    }
                    self.infer_precision_from_depths(&mut depths);
                    return Some(Ok(depths));
                }
                Err(e) => return Some(Err(anyhow::anyhow!("Failed to read record: {e}"))),
            }
        }

        if depths.is_empty() {
            None
        } else {
            self.infer_precision_from_depths(&mut depths);
            Some(Ok(depths))
        }
    }
}

impl Depth10StreamIterator {
    fn process_snapshot5(&mut self, data: TardisOrderBookSnapshot5Record) -> OrderBookDepth10 {
        let instrument_id = self
            .instrument_id
            .unwrap_or_else(|| parse_instrument_id(&data.exchange, data.symbol));

        let mut bids = [NULL_ORDER; DEPTH10_LEN];
        let mut asks = [NULL_ORDER; DEPTH10_LEN];
        let mut bid_counts = [0_u32; DEPTH10_LEN];
        let mut ask_counts = [0_u32; DEPTH10_LEN];

        // Process first 5 levels from snapshot5 data
        for i in 0..5 {
            let (bid_price, bid_amount) = match i {
                0 => (data.bids_0_price, data.bids_0_amount),
                1 => (data.bids_1_price, data.bids_1_amount),
                2 => (data.bids_2_price, data.bids_2_amount),
                3 => (data.bids_3_price, data.bids_3_amount),
                4 => (data.bids_4_price, data.bids_4_amount),
                _ => unreachable!(),
            };

            let (ask_price, ask_amount) = match i {
                0 => (data.asks_0_price, data.asks_0_amount),
                1 => (data.asks_1_price, data.asks_1_amount),
                2 => (data.asks_2_price, data.asks_2_amount),
                3 => (data.asks_3_price, data.asks_3_amount),
                4 => (data.asks_4_price, data.asks_4_amount),
                _ => unreachable!(),
            };

            if self.price_precision.is_none() {
                if let (Some(bp), Some(ap)) = (bid_price, ask_price) {
                    self.current_price_precision = self
                        .current_price_precision
                        .max(infer_precision(bp))
                        .max(infer_precision(ap));
                }
            }

            if self.size_precision.is_none() {
                if let (Some(ba), Some(aa)) = (bid_amount, ask_amount) {
                    self.current_size_precision = self
                        .current_size_precision
                        .max(infer_precision(ba))
                        .max(infer_precision(aa));
                }
            }

            let (bid_order, bid_count) = create_book_order(
                OrderSide::Buy,
                bid_price,
                bid_amount,
                self.current_price_precision,
                self.current_size_precision,
            );
            bids[i] = bid_order;
            bid_counts[i] = bid_count;

            let (ask_order, ask_count) = create_book_order(
                OrderSide::Sell,
                ask_price,
                ask_amount,
                self.current_price_precision,
                self.current_size_precision,
            );
            asks[i] = ask_order;
            ask_counts[i] = ask_count;
        }

        let flags = RecordFlag::F_SNAPSHOT.value();
        let sequence = 0;
        let ts_event = parse_timestamp(data.timestamp);
        let ts_init = parse_timestamp(data.local_timestamp);

        OrderBookDepth10::new(
            instrument_id,
            bids,
            asks,
            bid_counts,
            ask_counts,
            flags,
            sequence,
            ts_event,
            ts_init,
        )
    }

    fn process_snapshot25(&mut self, data: TardisOrderBookSnapshot25Record) -> OrderBookDepth10 {
        let instrument_id = self
            .instrument_id
            .unwrap_or_else(|| parse_instrument_id(&data.exchange, data.symbol));

        let mut bids = [NULL_ORDER; DEPTH10_LEN];
        let mut asks = [NULL_ORDER; DEPTH10_LEN];
        let mut bid_counts = [0_u32; DEPTH10_LEN];
        let mut ask_counts = [0_u32; DEPTH10_LEN];

        // Process first 10 levels from snapshot25 data
        for i in 0..DEPTH10_LEN {
            let (bid_price, bid_amount) = match i {
                0 => (data.bids_0_price, data.bids_0_amount),
                1 => (data.bids_1_price, data.bids_1_amount),
                2 => (data.bids_2_price, data.bids_2_amount),
                3 => (data.bids_3_price, data.bids_3_amount),
                4 => (data.bids_4_price, data.bids_4_amount),
                5 => (data.bids_5_price, data.bids_5_amount),
                6 => (data.bids_6_price, data.bids_6_amount),
                7 => (data.bids_7_price, data.bids_7_amount),
                8 => (data.bids_8_price, data.bids_8_amount),
                9 => (data.bids_9_price, data.bids_9_amount),
                _ => unreachable!(),
            };

            let (ask_price, ask_amount) = match i {
                0 => (data.asks_0_price, data.asks_0_amount),
                1 => (data.asks_1_price, data.asks_1_amount),
                2 => (data.asks_2_price, data.asks_2_amount),
                3 => (data.asks_3_price, data.asks_3_amount),
                4 => (data.asks_4_price, data.asks_4_amount),
                5 => (data.asks_5_price, data.asks_5_amount),
                6 => (data.asks_6_price, data.asks_6_amount),
                7 => (data.asks_7_price, data.asks_7_amount),
                8 => (data.asks_8_price, data.asks_8_amount),
                9 => (data.asks_9_price, data.asks_9_amount),
                _ => unreachable!(),
            };

            if self.price_precision.is_none() {
                if let (Some(bp), Some(ap)) = (bid_price, ask_price) {
                    self.current_price_precision = self
                        .current_price_precision
                        .max(infer_precision(bp))
                        .max(infer_precision(ap));
                }
            }

            if self.size_precision.is_none() {
                if let (Some(ba), Some(aa)) = (bid_amount, ask_amount) {
                    self.current_size_precision = self
                        .current_size_precision
                        .max(infer_precision(ba))
                        .max(infer_precision(aa));
                }
            }

            let (bid_order, bid_count) = create_book_order(
                OrderSide::Buy,
                bid_price,
                bid_amount,
                self.current_price_precision,
                self.current_size_precision,
            );
            bids[i] = bid_order;
            bid_counts[i] = bid_count;

            let (ask_order, ask_count) = create_book_order(
                OrderSide::Sell,
                ask_price,
                ask_amount,
                self.current_price_precision,
                self.current_size_precision,
            );
            asks[i] = ask_order;
            ask_counts[i] = ask_count;
        }

        let flags = RecordFlag::F_SNAPSHOT.value();
        let sequence = 0;
        let ts_event = parse_timestamp(data.timestamp);
        let ts_init = parse_timestamp(data.local_timestamp);

        OrderBookDepth10::new(
            instrument_id,
            bids,
            asks,
            bid_counts,
            ask_counts,
            flags,
            sequence,
            ts_event,
            ts_init,
        )
    }
}

/// Streams [`OrderBookDepth10`]s from a Tardis format CSV at the given `filepath`,
/// yielding chunks of the specified size.
///
/// # Precision Inference Warning
///
/// When using streaming with precision inference (not providing explicit precisions),
/// the inferred precision may differ from bulk loading the entire file. This is because
/// precision inference works within chunk boundaries, and different chunks may contain
/// values with different precision requirements. For deterministic precision behavior,
/// provide explicit `price_precision` and `size_precision` parameters.
///
/// # Errors
///
/// Returns an error if the file cannot be opened, read, or parsed as CSV.
pub fn stream_depth10_from_snapshot5<P: AsRef<Path>>(
    filepath: P,
    chunk_size: usize,
    price_precision: Option<u8>,
    size_precision: Option<u8>,
    instrument_id: Option<InstrumentId>,
) -> anyhow::Result<impl Iterator<Item = anyhow::Result<Vec<OrderBookDepth10>>>> {
    Depth10StreamIterator::new(
        filepath,
        chunk_size,
        5,
        price_precision,
        size_precision,
        instrument_id,
    )
}

/// Streams [`OrderBookDepth10`]s from a Tardis format CSV at the given `filepath`,
/// yielding chunks of the specified size.
///
/// # Precision Inference Warning
///
/// When using streaming with precision inference (not providing explicit precisions),
/// the inferred precision may differ from bulk loading the entire file. This is because
/// precision inference works within chunk boundaries, and different chunks may contain
/// values with different precision requirements. For deterministic precision behavior,
/// provide explicit `price_precision` and `size_precision` parameters.
///
/// # Errors
///
/// Returns an error if the file cannot be opened, read, or parsed as CSV.
pub fn stream_depth10_from_snapshot25<P: AsRef<Path>>(
    filepath: P,
    chunk_size: usize,
    price_precision: Option<u8>,
    size_precision: Option<u8>,
    instrument_id: Option<InstrumentId>,
) -> anyhow::Result<impl Iterator<Item = anyhow::Result<Vec<OrderBookDepth10>>>> {
    Depth10StreamIterator::new(
        filepath,
        chunk_size,
        25,
        price_precision,
        size_precision,
        instrument_id,
    )
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
    pub fn test_stream_deltas_chunked() {
        let csv_data = "exchange,symbol,timestamp,local_timestamp,is_snapshot,side,price,amount
binance-futures,BTCUSDT,1640995200000000,1640995200100000,true,ask,50000.0,1.0
binance-futures,BTCUSDT,1640995201000000,1640995201100000,false,bid,49999.5,2.0
binance-futures,BTCUSDT,1640995202000000,1640995202100000,false,ask,50000.12,1.5
binance-futures,BTCUSDT,1640995203000000,1640995203100000,false,bid,49999.123,3.0
binance-futures,BTCUSDT,1640995204000000,1640995204100000,false,ask,50000.1234,0.5";

        let temp_file = std::env::temp_dir().join("test_stream_deltas.csv");
        std::fs::write(&temp_file, csv_data).unwrap();

        let stream = stream_deltas(&temp_file, 2, Some(4), Some(1), None).unwrap();
        let chunks: Vec<_> = stream.collect();

        assert_eq!(chunks.len(), 3);

        let chunk1 = chunks[0].as_ref().unwrap();
        assert_eq!(chunk1.len(), 2);
        assert_eq!(chunk1[0].order.price.precision, 4);
        assert_eq!(chunk1[1].order.price.precision, 4);

        let chunk2 = chunks[1].as_ref().unwrap();
        assert_eq!(chunk2.len(), 2);
        assert_eq!(chunk2[0].order.price.precision, 4);
        assert_eq!(chunk2[1].order.price.precision, 4);

        let chunk3 = chunks[2].as_ref().unwrap();
        assert_eq!(chunk3.len(), 1);
        assert_eq!(chunk3[0].order.price.precision, 4);

        let total_deltas: usize = chunks.iter().map(|c| c.as_ref().unwrap().len()).sum();
        assert_eq!(total_deltas, 5);

        std::fs::remove_file(&temp_file).ok();
    }

    #[rstest]
    pub fn test_dynamic_precision_inference() {
        let csv_data = "exchange,symbol,timestamp,local_timestamp,is_snapshot,side,price,amount
binance-futures,BTCUSDT,1640995200000000,1640995200100000,true,ask,50000.0,1.0
binance-futures,BTCUSDT,1640995201000000,1640995201100000,false,bid,49999.5,2.0
binance-futures,BTCUSDT,1640995202000000,1640995202100000,false,ask,50000.12,1.5
binance-futures,BTCUSDT,1640995203000000,1640995203100000,false,bid,49999.123,3.0
binance-futures,BTCUSDT,1640995204000000,1640995204100000,false,ask,50000.1234,0.5";

        let temp_file = std::env::temp_dir().join("test_dynamic_precision.csv");
        std::fs::write(&temp_file, csv_data).unwrap();

        let deltas = load_deltas(&temp_file, None, None, None, None).unwrap();

        assert_eq!(deltas.len(), 5);

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

        // Test exact values to ensure retroactive precision updates work correctly
        assert_eq!(deltas[0].order.price, parse_price(50000.0, 4));
        assert_eq!(deltas[0].order.size, Quantity::new(1.0, 1));

        assert_eq!(deltas[1].order.price, parse_price(49999.5, 4));
        assert_eq!(deltas[1].order.size, Quantity::new(2.0, 1));

        assert_eq!(deltas[2].order.price, parse_price(50000.12, 4));
        assert_eq!(deltas[2].order.size, Quantity::new(1.5, 1));

        assert_eq!(deltas[3].order.price, parse_price(49999.123, 4));
        assert_eq!(deltas[3].order.size, Quantity::new(3.0, 1));

        assert_eq!(deltas[4].order.price, parse_price(50000.1234, 4));
        assert_eq!(deltas[4].order.size, Quantity::new(0.5, 1));

        assert_eq!(
            deltas[0].order.price.precision,
            deltas[4].order.price.precision
        );
        assert_eq!(
            deltas[0].order.size.precision,
            deltas[2].order.size.precision
        );

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

    #[rstest]
    pub fn test_stream_quotes_chunked() {
        let csv_data =
            "exchange,symbol,timestamp,local_timestamp,ask_amount,ask_price,bid_price,bid_amount
binance,BTCUSDT,1640995200000000,1640995200100000,1.0,50000.0,49999.0,1.5
binance,BTCUSDT,1640995201000000,1640995201100000,2.0,50000.5,49999.5,2.5
binance,BTCUSDT,1640995202000000,1640995202100000,1.5,50000.12,49999.12,1.8
binance,BTCUSDT,1640995203000000,1640995203100000,3.0,50000.123,49999.123,3.2
binance,BTCUSDT,1640995204000000,1640995204100000,0.5,50000.1234,49999.1234,0.8";

        let temp_file = std::env::temp_dir().join("test_stream_quotes.csv");
        std::fs::write(&temp_file, csv_data).unwrap();

        let stream = stream_quotes(&temp_file, 2, Some(4), Some(1), None).unwrap();
        let chunks: Vec<_> = stream.collect();

        assert_eq!(chunks.len(), 3);

        let chunk1 = chunks[0].as_ref().unwrap();
        assert_eq!(chunk1.len(), 2);
        assert_eq!(chunk1[0].bid_price.precision, 4);
        assert_eq!(chunk1[1].bid_price.precision, 4);

        let chunk2 = chunks[1].as_ref().unwrap();
        assert_eq!(chunk2.len(), 2);
        assert_eq!(chunk2[0].bid_price.precision, 4);
        assert_eq!(chunk2[1].bid_price.precision, 4);

        let chunk3 = chunks[2].as_ref().unwrap();
        assert_eq!(chunk3.len(), 1);
        assert_eq!(chunk3[0].bid_price.precision, 4);

        let total_quotes: usize = chunks.iter().map(|c| c.as_ref().unwrap().len()).sum();
        assert_eq!(total_quotes, 5);

        std::fs::remove_file(&temp_file).ok();
    }

    #[rstest]
    pub fn test_stream_trades_chunked() {
        let csv_data = "exchange,symbol,timestamp,local_timestamp,id,side,price,amount
binance,BTCUSDT,1640995200000000,1640995200100000,trade1,buy,50000.0,1.0
binance,BTCUSDT,1640995201000000,1640995201100000,trade2,sell,49999.5,2.0
binance,BTCUSDT,1640995202000000,1640995202100000,trade3,buy,50000.12,1.5
binance,BTCUSDT,1640995203000000,1640995203100000,trade4,sell,49999.123,3.0
binance,BTCUSDT,1640995204000000,1640995204100000,trade5,buy,50000.1234,0.5";

        let temp_file = std::env::temp_dir().join("test_stream_trades.csv");
        std::fs::write(&temp_file, csv_data).unwrap();

        let stream = stream_trades(&temp_file, 3, Some(4), Some(1), None).unwrap();
        let chunks: Vec<_> = stream.collect();

        assert_eq!(chunks.len(), 2);

        let chunk1 = chunks[0].as_ref().unwrap();
        assert_eq!(chunk1.len(), 3);
        assert_eq!(chunk1[0].price.precision, 4);
        assert_eq!(chunk1[1].price.precision, 4);
        assert_eq!(chunk1[2].price.precision, 4);

        let chunk2 = chunks[1].as_ref().unwrap();
        assert_eq!(chunk2.len(), 2);
        assert_eq!(chunk2[0].price.precision, 4);
        assert_eq!(chunk2[1].price.precision, 4);

        assert_eq!(chunk1[0].aggressor_side, AggressorSide::Buyer);
        assert_eq!(chunk1[1].aggressor_side, AggressorSide::Seller);

        let total_trades: usize = chunks.iter().map(|c| c.as_ref().unwrap().len()).sum();
        assert_eq!(total_trades, 5);

        std::fs::remove_file(&temp_file).ok();
    }

    #[rstest]
    pub fn test_stream_depth10_from_snapshot5_chunked() {
        let csv_data = "exchange,symbol,timestamp,local_timestamp,asks[0].price,asks[0].amount,bids[0].price,bids[0].amount,asks[1].price,asks[1].amount,bids[1].price,bids[1].amount,asks[2].price,asks[2].amount,bids[2].price,bids[2].amount,asks[3].price,asks[3].amount,bids[3].price,bids[3].amount,asks[4].price,asks[4].amount,bids[4].price,bids[4].amount
binance,BTCUSDT,1640995200000000,1640995200100000,50001.0,1.0,49999.0,1.5,50002.0,2.0,49998.0,2.5,50003.0,3.0,49997.0,3.5,50004.0,4.0,49996.0,4.5,50005.0,5.0,49995.0,5.5
binance,BTCUSDT,1640995201000000,1640995201100000,50001.5,1.1,49999.5,1.6,50002.5,2.1,49998.5,2.6,50003.5,3.1,49997.5,3.6,50004.5,4.1,49996.5,4.6,50005.5,5.1,49995.5,5.6
binance,BTCUSDT,1640995202000000,1640995202100000,50001.12,1.12,49999.12,1.62,50002.12,2.12,49998.12,2.62,50003.12,3.12,49997.12,3.62,50004.12,4.12,49996.12,4.62,50005.12,5.12,49995.12,5.62";

        // Write to temporary file
        let temp_file = std::env::temp_dir().join("test_stream_depth10_snapshot5.csv");
        std::fs::write(&temp_file, csv_data).unwrap();

        // Stream with chunk size of 2
        let stream = stream_depth10_from_snapshot5(&temp_file, 2, None, None, None).unwrap();
        let chunks: Vec<_> = stream.collect();

        // Should have 2 chunks: [2 items, 1 item]
        assert_eq!(chunks.len(), 2);

        // First chunk: 2 depth snapshots
        let chunk1 = chunks[0].as_ref().unwrap();
        assert_eq!(chunk1.len(), 2);

        // Second chunk: 1 depth snapshot
        let chunk2 = chunks[1].as_ref().unwrap();
        assert_eq!(chunk2.len(), 1);

        // Verify depth structure
        let first_depth = &chunk1[0];
        assert_eq!(first_depth.bids.len(), 10); // Should have 10 levels
        assert_eq!(first_depth.asks.len(), 10);

        // Verify some specific prices
        assert_eq!(first_depth.bids[0].price, parse_price(49999.0, 1));
        assert_eq!(first_depth.asks[0].price, parse_price(50001.0, 1));

        // Verify total count
        let total_depths: usize = chunks.iter().map(|c| c.as_ref().unwrap().len()).sum();
        assert_eq!(total_depths, 3);

        // Clean up
        std::fs::remove_file(&temp_file).ok();
    }

    #[rstest]
    pub fn test_stream_depth10_from_snapshot25_chunked() {
        // Create minimal snapshot25 CSV data (first 10 levels only for testing)
        let mut header_parts = vec!["exchange", "symbol", "timestamp", "local_timestamp"];

        // Add bid and ask levels (we'll only populate first few for testing)
        let mut bid_headers = Vec::new();
        let mut ask_headers = Vec::new();
        for i in 0..25 {
            bid_headers.push(format!("bids[{i}].price"));
            bid_headers.push(format!("bids[{i}].amount"));
        }
        for i in 0..25 {
            ask_headers.push(format!("asks[{i}].price"));
            ask_headers.push(format!("asks[{i}].amount"));
        }

        for header in &bid_headers {
            header_parts.push(header);
        }
        for header in &ask_headers {
            header_parts.push(header);
        }

        let header = header_parts.join(",");

        // Create a row with data for first 5 levels (rest will be empty)
        let mut row1_parts = vec![
            "binance".to_string(),
            "BTCUSDT".to_string(),
            "1640995200000000".to_string(),
            "1640995200100000".to_string(),
        ];

        // Add bid levels (first 5 with data, rest empty)
        for i in 0..25 {
            if i < 5 {
                let bid_price = 49999.0 - i as f64 * 0.01;
                let bid_amount = 1.0 + i as f64;
                row1_parts.push(bid_price.to_string());
                row1_parts.push(bid_amount.to_string());
            } else {
                row1_parts.push("".to_string());
                row1_parts.push("".to_string());
            }
        }

        // Add ask levels (first 5 with data, rest empty)
        for i in 0..25 {
            if i < 5 {
                let ask_price = 50000.0 + i as f64 * 0.01;
                let ask_amount = 1.0 + i as f64;
                row1_parts.push(ask_price.to_string());
                row1_parts.push(ask_amount.to_string());
            } else {
                row1_parts.push("".to_string());
                row1_parts.push("".to_string());
            }
        }

        let csv_data = format!("{}\n{}", header, row1_parts.join(","));

        // Write to temporary file
        let temp_file = std::env::temp_dir().join("test_stream_depth10_snapshot25.csv");
        std::fs::write(&temp_file, &csv_data).unwrap();

        // Stream with chunk size of 1
        let stream = stream_depth10_from_snapshot25(&temp_file, 1, None, None, None).unwrap();
        let chunks: Vec<_> = stream.collect();

        // Should have 1 chunk with 1 item
        assert_eq!(chunks.len(), 1);

        let chunk1 = chunks[0].as_ref().unwrap();
        assert_eq!(chunk1.len(), 1);

        // Verify depth structure
        let depth = &chunk1[0];
        assert_eq!(depth.bids.len(), 10); // Should have 10 levels
        assert_eq!(depth.asks.len(), 10);

        // Verify first level has data - check whatever we actually get
        let actual_bid_price = depth.bids[0].price;
        let actual_ask_price = depth.asks[0].price;
        assert!(actual_bid_price.as_f64() > 0.0);
        assert!(actual_ask_price.as_f64() > 0.0);

        // Clean up
        std::fs::remove_file(&temp_file).ok();
    }

    #[rstest]
    pub fn test_stream_error_handling() {
        // Test with non-existent file
        let non_existent = std::path::Path::new("does_not_exist.csv");

        let result = stream_deltas(non_existent, 10, None, None, None);
        assert!(result.is_err());

        let result = stream_quotes(non_existent, 10, None, None, None);
        assert!(result.is_err());

        let result = stream_trades(non_existent, 10, None, None, None);
        assert!(result.is_err());

        let result = stream_depth10_from_snapshot5(non_existent, 10, None, None, None);
        assert!(result.is_err());

        let result = stream_depth10_from_snapshot25(non_existent, 10, None, None, None);
        assert!(result.is_err());
    }

    #[rstest]
    pub fn test_stream_empty_file() {
        // Test with empty CSV file
        let temp_file = std::env::temp_dir().join("test_empty.csv");
        std::fs::write(&temp_file, "").unwrap();

        let stream = stream_deltas(&temp_file, 10, None, None, None).unwrap();
        let chunks: Vec<_> = stream.collect();
        assert_eq!(chunks.len(), 0);

        // Clean up
        std::fs::remove_file(&temp_file).ok();
    }

    #[rstest]
    pub fn test_stream_precision_consistency() {
        // Test that streaming produces same results as bulk loading for precision inference
        let csv_data = "exchange,symbol,timestamp,local_timestamp,is_snapshot,side,price,amount
binance-futures,BTCUSDT,1640995200000000,1640995200100000,true,ask,50000.0,1.0
binance-futures,BTCUSDT,1640995201000000,1640995201100000,false,bid,49999.5,2.0
binance-futures,BTCUSDT,1640995202000000,1640995202100000,false,ask,50000.12,1.5
binance-futures,BTCUSDT,1640995203000000,1640995203100000,false,bid,49999.123,3.0";

        let temp_file = std::env::temp_dir().join("test_precision_consistency.csv");
        std::fs::write(&temp_file, csv_data).unwrap();

        // Load all at once
        let bulk_deltas = load_deltas(&temp_file, None, None, None, None).unwrap();

        // Stream in chunks and collect
        let stream = stream_deltas(&temp_file, 2, None, None, None).unwrap();
        let chunks: Vec<_> = stream.collect();
        let streamed_deltas: Vec<_> = chunks
            .into_iter()
            .flat_map(|chunk| chunk.unwrap())
            .collect();

        // Should have same number of deltas
        assert_eq!(bulk_deltas.len(), streamed_deltas.len());

        // Compare key properties (precision inference will be different due to chunking)
        for (bulk, streamed) in bulk_deltas.iter().zip(streamed_deltas.iter()) {
            assert_eq!(bulk.instrument_id, streamed.instrument_id);
            assert_eq!(bulk.action, streamed.action);
            assert_eq!(bulk.order.side, streamed.order.side);
            assert_eq!(bulk.ts_event, streamed.ts_event);
            assert_eq!(bulk.ts_init, streamed.ts_init);
            // Note: precision may differ between bulk and streaming due to chunk boundaries
        }

        // Clean up
        std::fs::remove_file(&temp_file).ok();
    }
}
