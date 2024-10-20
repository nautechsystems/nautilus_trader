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

use csv::{Reader, ReaderBuilder, StringRecord};
use flate2::read::GzDecoder;
use nautilus_core::nanos::UnixNanos;
use nautilus_model::{
    data::{
        delta::OrderBookDelta,
        depth::{OrderBookDepth10, DEPTH10_LEN},
        order::{BookOrder, NULL_ORDER},
        quote::QuoteTick,
        trade::TradeTick,
    },
    enums::{OrderSide, RecordFlag},
    identifiers::{InstrumentId, TradeId},
    types::{price::Price, quantity::Quantity},
};

mod record;

use super::{
    csv::record::{
        TardisBookUpdateRecord, TardisOrderBookSnapshot25Record, TardisOrderBookSnapshot5Record,
        TardisQuoteRecord, TardisTradeRecord,
    },
    parse::{
        parse_aggressor_side, parse_book_action, parse_instrument_id, parse_order_side,
        parse_timestamp,
    },
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
    instrument_id: Option<InstrumentId>,
    limit: Option<usize>,
) -> Result<Vec<OrderBookDelta>, Box<dyn Error>> {
    let mut csv_reader = create_csv_reader(filepath)?;
    let mut deltas: Vec<OrderBookDelta> = Vec::new();
    let mut last_ts_event = UnixNanos::default();

    let mut raw_record = StringRecord::new();
    while csv_reader.read_record(&mut raw_record)? {
        let record: TardisBookUpdateRecord = raw_record.deserialize(None)?;

        let instrument_id = match &instrument_id {
            Some(id) => *id,
            None => parse_instrument_id(&record.exchange, &record.symbol),
        };
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
                Price::new(price, price_precision),
                Quantity::new(amount.unwrap_or(0.0), size_precision),
                0,
            ),
            1, // Count set to 1 if order exists
        ),
        None => (NULL_ORDER, 0), // NULL_ORDER if price is None
    }
}

/// Load [`OrderBookDepth10`]s from a Tardis format CSV at the given `filepath`.
pub fn load_depth10_from_snapshot5<P: AsRef<Path>>(
    filepath: P,
    price_precision: u8,
    size_precision: u8,
    instrument_id: Option<InstrumentId>,
    limit: Option<usize>,
) -> Result<Vec<OrderBookDepth10>, Box<dyn Error>> {
    let mut csv_reader = create_csv_reader(filepath)?;
    let mut depths: Vec<OrderBookDepth10> = Vec::new();

    let mut raw_record = StringRecord::new();
    while csv_reader.read_record(&mut raw_record)? {
        let record: TardisOrderBookSnapshot5Record = raw_record.deserialize(None)?;
        let instrument_id = match &instrument_id {
            Some(id) => *id,
            None => parse_instrument_id(&record.exchange, &record.symbol),
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

pub fn load_depth10_from_snapshot25<P: AsRef<Path>>(
    filepath: P,
    price_precision: u8,
    size_precision: u8,
    instrument_id: Option<InstrumentId>,
    limit: Option<usize>,
) -> Result<Vec<OrderBookDepth10>, Box<dyn Error>> {
    let mut csv_reader = create_csv_reader(filepath)?;
    let mut depths: Vec<OrderBookDepth10> = Vec::new();

    let mut raw_record = StringRecord::new();
    while csv_reader.read_record(&mut raw_record)? {
        let record: TardisOrderBookSnapshot25Record = raw_record.deserialize(None)?;

        let instrument_id = match &instrument_id {
            Some(id) => *id,
            None => parse_instrument_id(&record.exchange, &record.symbol),
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

/// Load [`QuoteTick`]s from a Tardis format CSV at the given `filepath`.
pub fn load_quote_ticks<P: AsRef<Path>>(
    filepath: P,
    price_precision: u8,
    size_precision: u8,
    instrument_id: Option<InstrumentId>,
    limit: Option<usize>,
) -> Result<Vec<QuoteTick>, Box<dyn Error>> {
    let mut csv_reader = create_csv_reader(filepath)?;
    let mut quotes = Vec::new();

    let mut raw_record = StringRecord::new();
    while csv_reader.read_record(&mut raw_record)? {
        let record: TardisQuoteRecord = raw_record.deserialize(None)?;

        let instrument_id = match &instrument_id {
            Some(id) => *id,
            None => parse_instrument_id(&record.exchange, &record.symbol),
        };
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
    instrument_id: Option<InstrumentId>,
    limit: Option<usize>,
) -> Result<Vec<TradeTick>, Box<dyn Error>> {
    let mut csv_reader = create_csv_reader(filepath)?;
    let mut trades = Vec::new();

    let mut raw_record = StringRecord::new();
    while csv_reader.read_record(&mut raw_record)? {
        let record: TardisTradeRecord = raw_record.deserialize(None)?;

        let instrument_id = match &instrument_id {
            Some(id) => *id,
            None => parse_instrument_id(&record.exchange, &record.symbol),
        };
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
    use nautilus_model::{
        enums::{AggressorSide, BookAction},
        identifiers::InstrumentId,
    };
    use nautilus_test_kit::common::{
        ensure_data_exists_tardis_binance_snapshot25, ensure_data_exists_tardis_binance_snapshot5,
        ensure_data_exists_tardis_bitmex_trades, ensure_data_exists_tardis_deribit_book_l2,
        ensure_data_exists_tardis_huobi_quotes,
    };
    use rstest::*;

    use super::*;

    #[rstest]
    pub fn test_read_deltas() {
        let filepath = ensure_data_exists_tardis_deribit_book_l2();
        let deltas = load_deltas(filepath, 1, 0, None, Some(1_000)).unwrap();

        assert_eq!(deltas.len(), 1_000);
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
    pub fn test_read_depth10s_from_snapshot5() {
        let filepath = ensure_data_exists_tardis_binance_snapshot5();
        let depths = load_depth10_from_snapshot5(filepath, 1, 0, None, Some(100_000)).unwrap();

        assert_eq!(depths.len(), 100_000);
        assert_eq!(
            depths[0].instrument_id,
            InstrumentId::from("BTCUSDT.BINANCE")
        );
        assert_eq!(depths[0].bids.len(), 10);
        assert_eq!(depths[0].bids[0].price, Price::from("11657.1"));
        assert_eq!(depths[0].bids[0].size, Quantity::from("11"));
        assert_eq!(depths[0].bids[0].side, OrderSide::Buy);
        assert_eq!(depths[0].bids[0].order_id, 0);
        assert_eq!(depths[0].asks.len(), 10);
        assert_eq!(depths[0].asks[0].price, Price::from("11657.1"));
        assert_eq!(depths[0].asks[0].size, Quantity::from("2"));
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
    pub fn test_read_depth10s_from_snapshot25() {
        let filepath = ensure_data_exists_tardis_binance_snapshot25();
        let depths = load_depth10_from_snapshot25(filepath, 1, 0, None, Some(100_000)).unwrap();

        assert_eq!(depths.len(), 100_000);
        assert_eq!(
            depths[0].instrument_id,
            InstrumentId::from("BTCUSDT.BINANCE")
        );
        assert_eq!(depths[0].bids.len(), 10);
        assert_eq!(depths[0].bids[0].price, Price::from("11657.1"));
        assert_eq!(depths[0].bids[0].size, Quantity::from("11"));
        assert_eq!(depths[0].bids[0].side, OrderSide::Buy);
        assert_eq!(depths[0].bids[0].order_id, 0);
        assert_eq!(depths[0].asks.len(), 10);
        assert_eq!(depths[0].asks[0].price, Price::from("11657.1"));
        assert_eq!(depths[0].asks[0].size, Quantity::from("2"));
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
    pub fn test_read_quotes() {
        let filepath = ensure_data_exists_tardis_huobi_quotes();
        let quotes = load_quote_ticks(filepath, 1, 0, None, Some(100_000)).unwrap();

        assert_eq!(quotes.len(), 100_000);
        assert_eq!(quotes[0].instrument_id, InstrumentId::from("BTC-USD.HUOBI"));
        assert_eq!(quotes[0].bid_price, Price::from("8629.2"));
        assert_eq!(quotes[0].bid_size, Quantity::from("806"));
        assert_eq!(quotes[0].ask_price, Price::from("8629.3"));
        assert_eq!(quotes[0].ask_size, Quantity::from("5494"));
        assert_eq!(quotes[0].ts_event, 1588291201099000000);
        assert_eq!(quotes[0].ts_init, 1588291201234268000);
    }

    #[rstest]
    pub fn test_read_trades() {
        let filepath = ensure_data_exists_tardis_bitmex_trades();
        let trades = load_trade_ticks(filepath, 1, 0, None, Some(100_000)).unwrap();

        assert_eq!(trades.len(), 100_000);
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
