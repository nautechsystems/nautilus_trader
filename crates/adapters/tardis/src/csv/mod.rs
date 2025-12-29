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

pub mod load;
mod record;
pub mod stream;

use std::{
    ffi::OsStr,
    fs::File,
    io::{BufReader, Read, Seek, SeekFrom},
    path::Path,
    time::Duration,
};

use csv::{Reader, ReaderBuilder};
use flate2::read::GzDecoder;
pub use load::{
    load_deltas, load_depth10_from_snapshot5, load_depth10_from_snapshot25, load_funding_rates,
    load_quotes, load_trades,
};
use nautilus_model::{
    data::{BookOrder, FundingRateUpdate, NULL_ORDER, OrderBookDelta, QuoteTick, TradeTick},
    enums::{BookAction, OrderSide},
    identifiers::{InstrumentId, TradeId},
    types::Quantity,
};
use rust_decimal::Decimal;
pub use stream::{
    stream_deltas, stream_depth10_from_snapshot5, stream_depth10_from_snapshot25,
    stream_funding_rates, stream_quotes, stream_trades,
};

use super::{
    csv::record::{
        TardisBookUpdateRecord, TardisDerivativeTickerRecord, TardisQuoteRecord, TardisTradeRecord,
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
                    tracing::warn!(
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
                tracing::warn!(
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
                tracing::warn!(
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

fn parse_delta_record(
    data: &TardisBookUpdateRecord,
    price_precision: u8,
    size_precision: u8,
    instrument_id: Option<InstrumentId>,
) -> anyhow::Result<OrderBookDelta> {
    let instrument_id = match instrument_id {
        Some(id) => id,
        None => parse_instrument_id(&data.exchange, data.symbol),
    };

    let side = parse_order_side(&data.side);
    let price = parse_price(data.price, price_precision);
    let size = Quantity::new(data.amount, size_precision);
    let order_id = 0; // Not applicable for L2 data
    let order = BookOrder::new(side, price, size, order_id);

    let action = parse_book_action(data.is_snapshot, size.as_f64());
    let flags = 0; // Will be set later if needed
    let sequence = 0; // Sequence not available
    let ts_event = parse_timestamp(data.timestamp);
    let ts_init = parse_timestamp(data.local_timestamp);

    anyhow::ensure!(
        !(action != BookAction::Delete && size.is_zero()),
        "Invalid delta: action {action} when size zero, check size_precision ({size_precision}) vs data; {data:?}"
    );

    Ok(OrderBookDelta::new(
        instrument_id,
        action,
        order,
        flags,
        sequence,
        ts_event,
        ts_init,
    ))
}

fn parse_quote_record(
    data: &TardisQuoteRecord,
    price_precision: u8,
    size_precision: u8,
    instrument_id: Option<InstrumentId>,
) -> QuoteTick {
    let instrument_id = match instrument_id {
        Some(id) => id,
        None => parse_instrument_id(&data.exchange, data.symbol),
    };

    let bid_price = parse_price(data.bid_price.unwrap_or(0.0), price_precision);
    let ask_price = parse_price(data.ask_price.unwrap_or(0.0), price_precision);
    let bid_size = Quantity::new(data.bid_amount.unwrap_or(0.0), size_precision);
    let ask_size = Quantity::new(data.ask_amount.unwrap_or(0.0), size_precision);
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
    size: Quantity,
    price_precision: u8,
    instrument_id: Option<InstrumentId>,
) -> TradeTick {
    let instrument_id = match instrument_id {
        Some(id) => id,
        None => parse_instrument_id(&data.exchange, data.symbol),
    };

    let price = parse_price(data.price, price_precision);
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

fn parse_derivative_ticker_record(
    data: &TardisDerivativeTickerRecord,
    instrument_id: Option<InstrumentId>,
) -> Option<FundingRateUpdate> {
    // Only create funding rate update if we have funding rate data
    let funding_rate = data.funding_rate?;

    let instrument_id = match instrument_id {
        Some(id) => id,
        None => parse_instrument_id(&data.exchange, data.symbol),
    };

    let rate = Decimal::try_from(funding_rate).ok()?.normalize();
    let next_funding_ns = if data.predicted_funding_rate.is_some() {
        data.funding_timestamp.map(parse_timestamp)
    } else {
        None
    };
    let ts_event = parse_timestamp(data.timestamp);
    let ts_init = parse_timestamp(data.local_timestamp);

    Some(FundingRateUpdate::new(
        instrument_id,
        rate,
        next_funding_ns,
        ts_event,
        ts_init,
    ))
}
