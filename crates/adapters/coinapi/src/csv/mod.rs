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

//! Loaders for CoinAPI Flat Files, the semicolon-delimited `.csv.gz` exports of the S3 API.
//!
//! - [`load_trades`], [`load_quotes`], and [`load_deltas`] read a whole per-symbol file.
//! - [`stream_trades`], [`stream_quotes`], and [`stream_deltas`] read one in chunks.
//! - [`CoinApiOhlcvLoader`] reads an exchange-wide OHLCV file.
//!
//! Values convert exactly from their text. A load infers each precision it is not given from every
//! value it reads. A stream infers it from a leading sample and rejects a later value that
//! would need rounding, so pass explicit precisions to stream values of unknown precision.

mod book;
mod ohlcv;
mod rows;
mod ticks;

use std::path::Path;

use csv::StringRecord;
use jiff::civil::Date;
use nautilus_core::correctness::check_in_range_inclusive_usize;
use nautilus_model::{
    data::{OrderBookDelta, QuoteTick, TradeTick},
    identifiers::InstrumentId,
};
pub use ohlcv::CoinApiOhlcvLoader;

use self::{
    book::BookDecoder,
    rows::{Precision, Rows, partition_date},
    ticks::{QuoteDecoder, TradeDecoder},
};

/// The largest `chunk_size` a stream accepts.
pub const STREAM_CHUNK_SIZE_MAX: usize = 1_000_000;

const LOAD_CHUNK_SIZE: usize = 1_000_000;

// Rows scanned to infer a stream's precision
const PRECISION_SAMPLE_ROWS: usize = 10_000;

/// Loads [`TradeTick`]s from a `T-TRADES` file.
///
/// Infers each precision not given from every value read.
///
/// # Errors
///
/// Returns an error if the file cannot be read, a column is missing, or a row is invalid.
pub fn load_trades(
    path: impl AsRef<Path>,
    instrument_id: InstrumentId,
    price_precision: Option<u8>,
    size_precision: Option<u8>,
    limit: Option<usize>,
) -> anyhow::Result<Vec<TradeTick>> {
    load(
        path.as_ref(),
        |rows| TradeDecoder::new(rows, instrument_id),
        price_precision,
        size_precision,
        limit,
    )
}

/// Loads [`QuoteTick`]s from a `T-QUOTES` file.
///
/// Infers each precision not given from every value read.
///
/// # Errors
///
/// Returns an error if the file cannot be read, a column is missing, or a row is invalid.
pub fn load_quotes(
    path: impl AsRef<Path>,
    instrument_id: InstrumentId,
    price_precision: Option<u8>,
    size_precision: Option<u8>,
    limit: Option<usize>,
) -> anyhow::Result<Vec<QuoteTick>> {
    load(
        path.as_ref(),
        |rows| QuoteDecoder::new(rows, instrument_id),
        price_precision,
        size_precision,
        limit,
    )
}

/// Loads [`OrderBookDelta`]s from a `T-LIMITBOOK_FULL` file.
///
/// Rows carry only a time of day, so each timestamp falls on `date`, or on the date of the file's
/// `D-YYYYMMDD` or `D-YYYYMMDDHH` partition when `date` is `None`. Infers each precision not given
/// from every value read.
///
/// # Errors
///
/// Returns an error if no date is given or found in the path, the file cannot be read, a column is
/// missing, or a row is invalid.
pub fn load_deltas(
    path: impl AsRef<Path>,
    instrument_id: InstrumentId,
    date: Option<Date>,
    price_precision: Option<u8>,
    size_precision: Option<u8>,
    limit: Option<usize>,
) -> anyhow::Result<Vec<OrderBookDelta>> {
    let path = path.as_ref();
    let date = book_date(path, date)?;
    load(
        path,
        |rows| BookDecoder::new(rows, instrument_id, date),
        price_precision,
        size_precision,
        limit,
    )
}

/// Streams [`TradeTick`]s from a `T-TRADES` file in chunks of up to `chunk_size`.
///
/// Infers each precision not given from the leading rows.
///
/// # Errors
///
/// Returns an error if `chunk_size` is outside [1, [`STREAM_CHUNK_SIZE_MAX`]] or the file cannot
/// be opened. Each chunk is an error if a row is invalid or finer than its precision.
pub fn stream_trades(
    path: impl AsRef<Path>,
    instrument_id: InstrumentId,
    chunk_size: usize,
    price_precision: Option<u8>,
    size_precision: Option<u8>,
    limit: Option<usize>,
) -> anyhow::Result<impl Iterator<Item = anyhow::Result<Vec<TradeTick>>>> {
    stream(
        path.as_ref(),
        |rows| TradeDecoder::new(rows, instrument_id),
        chunk_size,
        price_precision,
        size_precision,
        limit,
    )
}

/// Streams [`QuoteTick`]s from a `T-QUOTES` file in chunks of up to `chunk_size`.
///
/// Infers each precision not given from the leading rows.
///
/// # Errors
///
/// Returns an error if `chunk_size` is outside [1, [`STREAM_CHUNK_SIZE_MAX`]] or the file cannot
/// be opened. Each chunk is an error if a row is invalid or finer than its precision.
pub fn stream_quotes(
    path: impl AsRef<Path>,
    instrument_id: InstrumentId,
    chunk_size: usize,
    price_precision: Option<u8>,
    size_precision: Option<u8>,
    limit: Option<usize>,
) -> anyhow::Result<impl Iterator<Item = anyhow::Result<Vec<QuoteTick>>>> {
    stream(
        path.as_ref(),
        |rows| QuoteDecoder::new(rows, instrument_id),
        chunk_size,
        price_precision,
        size_precision,
        limit,
    )
}

/// Streams [`OrderBookDelta`]s from a `T-LIMITBOOK_FULL` file in chunks of about `chunk_size`.
///
/// A chunk ends only at the end of an event, so it can exceed `chunk_size`; the opening snapshot
/// is one event. A `limit` can end the last chunk partway through an event, which then ends with
/// an `F_LAST` delta. Dates timestamps as [`load_deltas`] does, and infers each precision not given
/// from the leading rows.
///
/// # Errors
///
/// Returns an error if `chunk_size` is outside [1, [`STREAM_CHUNK_SIZE_MAX`]], no date is given or
/// found in the path, or the file cannot be opened. Each chunk is an error if a row is invalid or
/// finer than its precision.
pub fn stream_deltas(
    path: impl AsRef<Path>,
    instrument_id: InstrumentId,
    date: Option<Date>,
    chunk_size: usize,
    price_precision: Option<u8>,
    size_precision: Option<u8>,
    limit: Option<usize>,
) -> anyhow::Result<impl Iterator<Item = anyhow::Result<Vec<OrderBookDelta>>>> {
    let path = path.as_ref();
    let date = book_date(path, date)?;
    stream(
        path,
        |rows| BookDecoder::new(rows, instrument_id, date),
        chunk_size,
        price_precision,
        size_precision,
        limit,
    )
}

fn book_date(path: &Path, date: Option<Date>) -> anyhow::Result<Date> {
    date.or_else(|| partition_date(path)).ok_or_else(|| {
        anyhow::anyhow!(
            "no date given and '{}' is not in a D-YYYYMMDD partition",
            path.display()
        )
    })
}

/// Converts the rows of one flat file schema.
pub(crate) trait Decode {
    /// The value a row converts to.
    type Output;

    /// Raises inferred precisions to fit the row's values.
    ///
    /// # Errors
    ///
    /// Returns an error if a value is invalid.
    fn fit(
        &self,
        record: &StringRecord,
        price: &mut Precision,
        size: &mut Precision,
    ) -> anyhow::Result<()>;

    /// Converts a row, appending every value it completes to `out`.
    ///
    /// # Errors
    ///
    /// Returns an error if the row is invalid or finer than a fixed precision.
    fn decode(
        &mut self,
        record: &StringRecord,
        price: &mut Precision,
        size: &mut Precision,
        out: &mut Vec<Self::Output>,
    ) -> anyhow::Result<()>;

    /// Returns how many converted values are held for later rows.
    fn pending(&self) -> usize {
        0
    }

    /// Appends any values held for later rows once reading stops, reading ahead in `rows` when
    /// they depend on a later row.
    ///
    /// # Errors
    ///
    /// Returns an error if a row read ahead is invalid.
    fn finish(&mut self, _rows: &mut Rows, _out: &mut Vec<Self::Output>) -> anyhow::Result<()> {
        Ok(())
    }

    /// Shortens converted values to `len` once a limit cuts the input short.
    fn truncate(out: &mut Vec<Self::Output>, len: usize) {
        out.truncate(len);
    }

    /// Sets the precision of every inferred field of `value`.
    fn set_precision(value: &mut Self::Output, price: Option<u8>, size: Option<u8>);
}

/// Chunks of values converted from one flat file.
pub(crate) struct FlatFileStream<D: Decode> {
    rows: Rows,
    decoder: D,
    price: Precision,
    size: Precision,
    chunk_size: usize,
    limit: Option<usize>,
    emitted: usize,
    is_done: bool,
}

impl<D: Decode> FlatFileStream<D> {
    fn read_chunk(&mut self) -> anyhow::Result<Vec<D::Output>> {
        let mut out = Vec::new();
        let limit = self.limit.unwrap_or(usize::MAX);
        while out.len() < self.chunk_size {
            if self.emitted + out.len() + self.decoder.pending() >= limit {
                self.decoder.finish(&mut self.rows, &mut out)?;
                self.is_done = true;
                break;
            }

            let Some(record) = self.rows.next()? else {
                self.decoder.finish(&mut self.rows, &mut out)?;
                self.is_done = true;
                break;
            };

            self.decoder
                .decode(record, &mut self.price, &mut self.size, &mut out)?;
        }

        if let Some(remaining) = limit.checked_sub(self.emitted)
            && out.len() > remaining
        {
            D::truncate(&mut out, remaining);
            self.is_done = true;
        }

        self.emitted += out.len();
        Ok(out)
    }
}

impl<D: Decode> Iterator for FlatFileStream<D> {
    type Item = anyhow::Result<Vec<D::Output>>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.is_done {
            return None;
        }

        match self.read_chunk() {
            Ok(chunk) if chunk.is_empty() => None,
            Ok(chunk) => Some(Ok(chunk)),
            Err(e) => {
                self.is_done = true;
                Some(Err(e))
            }
        }
    }
}

fn load<D: Decode>(
    path: &Path,
    decoder: impl FnOnce(&mut Rows) -> anyhow::Result<D>,
    price_precision: Option<u8>,
    size_precision: Option<u8>,
    limit: Option<usize>,
) -> anyhow::Result<Vec<D::Output>> {
    let mut rows = Rows::open(path)?;
    let decoder = decoder(&mut rows)?;

    let mut stream = FlatFileStream {
        rows,
        decoder,
        price: Precision::new(price_precision),
        size: Precision::new(size_precision),
        chunk_size: LOAD_CHUNK_SIZE,
        limit,
        emitted: 0,
        is_done: false,
    };

    let mut values = Vec::new();
    for chunk in stream.by_ref() {
        values.extend(chunk?);
    }

    // Values converted before a finer one keep their exact raw value at the final precision
    let price = stream.price.is_inferred.then_some(stream.price.value);
    let size = stream.size.is_inferred.then_some(stream.size.value);

    if price.is_some() || size.is_some() {
        for value in &mut values {
            D::set_precision(value, price, size);
        }
    }

    Ok(values)
}

fn stream<D: Decode>(
    path: &Path,
    mut decoder: impl FnMut(&mut Rows) -> anyhow::Result<D>,
    chunk_size: usize,
    price_precision: Option<u8>,
    size_precision: Option<u8>,
    limit: Option<usize>,
) -> anyhow::Result<FlatFileStream<D>> {
    check_in_range_inclusive_usize(chunk_size, 1, STREAM_CHUNK_SIZE_MAX, "chunk_size")?;

    let mut price = Precision::new(price_precision);
    let mut size = Precision::new(size_precision);

    if price.is_inferred || size.is_inferred {
        let mut rows = Rows::open(path)?;
        let sampler = decoder(&mut rows)?;
        for _ in 0..PRECISION_SAMPLE_ROWS {
            let Some(record) = rows.next()? else {
                break;
            };

            sampler.fit(record, &mut price, &mut size)?;
        }
    }

    let mut rows = Rows::open(path)?;
    let decoder = decoder(&mut rows)?;

    Ok(FlatFileStream {
        rows,
        decoder,
        price: price.fixed(),
        size: size.fixed(),
        chunk_size,
        limit,
        emitted: 0,
        is_done: false,
    })
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        io::Write,
        path::{Path, PathBuf},
        str::FromStr,
    };

    use ahash::AHashMap;
    use flate2::{Compression, write::GzEncoder};
    use nautilus_core::UnixNanos;
    use nautilus_model::{
        data::{Bar, BarType, BookOrder},
        enums::{AggressorSide, BookAction, OrderSide, RecordFlag},
        identifiers::TradeId,
        types::{Price, Quantity, fixed::FIXED_PRECISION},
    };
    use rstest::rstest;
    use rust_decimal::{Decimal, RoundingStrategy};
    use tempfile::TempDir;

    use super::*;
    use crate::http::models::Symbol;

    const SNAPSHOT: u8 = RecordFlag::F_SNAPSHOT as u8;
    const LAST: u8 = RecordFlag::F_LAST as u8;
    // 2025-09-01T00:00:00Z
    const DAY_START: u64 = 1_756_684_800_000_000_000;

    const BOOK_HEADER: &str =
        "time_exchange;time_coinapi;update_type;is_buy;entry_px;entry_sx;order_id";
    const TRADES_HEADER: &str = "time_exchange;time_coinapi;guid;price;base_amount;taker_side";
    const OHLCV_HEADER: &str = "symbol_id;time_period_start;time_period_end;time_open;time_close;\
                                px_open;px_high;px_low;px_close;sx_cnt;sx_sum";

    fn fixture(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("test_data/csv")
            .join(name)
    }

    fn date() -> Date {
        Date::new(2025, 9, 1).unwrap()
    }

    fn at(nanos_after_day_start: u64) -> UnixNanos {
        UnixNanos::from(DAY_START + nanos_after_day_start)
    }

    fn binance() -> InstrumentId {
        InstrumentId::from("BTCUSDT.BINANCE")
    }

    fn coinbase() -> InstrumentId {
        InstrumentId::from("BTC-USD.COINBASE")
    }

    fn write_file(dir: &Path, name: &str, lines: &[&str]) -> PathBuf {
        let path = dir.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, lines.join("\n") + "\n").unwrap();
        path
    }

    fn trade_row(guid: &str, price: &str, size: &str) -> String {
        format!("2025-09-01T00:00:00.0000000;2025-09-01T00:00:00.0000000;{guid};{price};{size};BUY")
    }

    fn actions(deltas: &[OrderBookDelta]) -> AHashMap<BookAction, usize> {
        let mut counts = AHashMap::new();
        for delta in deltas {
            *counts.entry(delta.action).or_default() += 1;
        }

        counts
    }

    #[rstest]
    fn load_trades_converts_every_field_at_the_inferred_precision() {
        let trades = load_trades(
            fixture("trades_binance_spot_btc_usdt_20250901.csv"),
            binance(),
            None,
            None,
            None,
        )
        .unwrap();

        assert_eq!(trades.len(), 50);
        assert_eq!(
            trades[0],
            TradeTick::new(
                binance(),
                Price::from("108246.36"),
                Quantity::from("0.00023"),
                AggressorSide::Buy,
                TradeId::from("8a61934b-2ca0-4e94-b4ec-cb5409a28c53"),
                at(178_000_000),
                at(299_824_200),
            )
        );
        assert!(trades.iter().all(|t| t.price.precision == 2));
        assert!(trades.iter().all(|t| t.size.precision == 5));
        assert!(
            trades
                .iter()
                .any(|t| t.aggressor_side == AggressorSide::Sell)
        );
    }

    #[rstest]
    fn load_trades_uses_explicit_precisions_and_rejects_finer_values() {
        let path = fixture("trades_coinbase_spot_btc_usd_20250901.csv");

        let trades = load_trades(&path, coinbase(), Some(4), Some(9), Some(3)).unwrap();
        let error = load_trades(&path, coinbase(), Some(1), None, None).unwrap_err();

        assert_eq!(trades.len(), 3);
        assert!(trades.iter().all(|t| t.price.precision == 4));
        assert!(trades.iter().all(|t| t.size.precision == 9));
        assert_eq!(trades[0].size, Quantity::from("0.04120085"));
        assert_eq!(
            error.to_string(),
            "price 108247.95 is finer than instrument precision 1"
        );
    }

    #[rstest]
    fn load_quotes_converts_every_field_at_the_inferred_precision() {
        let quotes = load_quotes(
            fixture("quotes_coinbase_spot_btc_usd_20250901.csv"),
            coinbase(),
            None,
            None,
            None,
        )
        .unwrap();

        assert_eq!(quotes.len(), 50);
        assert_eq!(
            quotes[0],
            QuoteTick::new(
                coinbase(),
                Price::from("108247.90"),
                Price::from("108247.94"),
                Quantity::from("0.18478034"),
                Quantity::from("0.19673805"),
                at(47_937_000),
                at(453_522_400),
            )
        );
        assert!(
            quotes
                .iter()
                .all(|q| q.bid_price.precision == 2 && q.ask_price.precision == 2)
        );
        assert!(
            quotes
                .iter()
                .all(|q| q.bid_size.precision == 8 && q.ask_size.precision == 8)
        );
    }

    #[rstest]
    fn load_deltas_rebuilds_price_levels() {
        let deltas = load_deltas(
            fixture("limitbook_full_binance_spot_btc_usdt_20250901.csv"),
            binance(),
            Some(date()),
            None,
            None,
            None,
        )
        .unwrap();

        let last_indices: Vec<usize> = deltas
            .iter()
            .enumerate()
            .filter(|(_, d)| d.flags & LAST != 0)
            .map(|(i, _)| i)
            .collect();

        assert_eq!(deltas.len(), 81);
        assert_eq!(deltas[0].action, BookAction::Clear);
        assert_eq!(
            deltas[1],
            OrderBookDelta::new(
                binance(),
                BookAction::Add,
                BookOrder::new(
                    OrderSide::Sell,
                    Price::from("108246.36"),
                    Quantity::from("6.61982"),
                    0
                ),
                SNAPSHOT,
                0,
                at(14_000_000),
                at(135_378_300),
            )
        );
        assert!(deltas[..=20].iter().all(|d| d.flags & SNAPSHOT != 0));
        assert!(deltas[21..].iter().all(|d| d.flags & SNAPSHOT == 0));
        assert_eq!(last_indices, [20, 32, 46, 62, 80]);
        assert_eq!(actions(&deltas[21..])[&BookAction::Delete], 23);
        assert_eq!(actions(&deltas[21..])[&BookAction::Update], 37);
        assert!(
            deltas[21..]
                .iter()
                .filter(|d| d.action == BookAction::Delete)
                .all(|d| d.order.size.is_zero())
        );
        assert!(deltas.iter().all(|d| d.order.price.precision == 2));
        assert!(deltas.iter().all(|d| d.order.size.precision == 5));
    }

    #[rstest]
    fn load_deltas_tracks_resting_orders() {
        let deltas = load_deltas(
            fixture("limitbook_full_coinbase_spot_btc_usd_20250901.csv"),
            coinbase(),
            Some(date()),
            None,
            None,
            None,
        )
        .unwrap();

        let mut book: AHashMap<u64, BookOrder> = AHashMap::new();
        let mut history: AHashMap<u64, Vec<(BookAction, Quantity)>> = AHashMap::new();

        for delta in &deltas {
            match delta.action {
                BookAction::Clear => book.clear(),
                BookAction::Add | BookAction::Update => {
                    book.insert(delta.order.order_id, delta.order);
                }
                BookAction::Delete => {
                    assert_eq!(book.remove(&delta.order.order_id), Some(delta.order));
                }
            }

            if delta.action != BookAction::Clear {
                history
                    .entry(delta.order.order_id)
                    .or_default()
                    .push((delta.action, delta.order.size));
            }
        }

        let mut resting: Vec<(OrderSide, Price, Quantity)> = book
            .values()
            .map(|o| (o.side.unwrap(), o.price, o.size))
            .collect();
        resting.sort_by_key(|(side, price, _)| (*side as u8, *price));

        let counts = actions(&deltas);
        let q = Quantity::from;

        assert_eq!(deltas.len(), 52);
        assert_eq!(counts[&BookAction::Add], 26);
        assert_eq!(counts[&BookAction::Update], 6);
        assert_eq!(counts[&BookAction::Delete], 19);
        assert_eq!(deltas.iter().filter(|d| d.flags & LAST != 0).count(), 29);
        assert_eq!(deltas[13].flags, SNAPSHOT | LAST);
        assert_eq!(
            resting,
            [
                (OrderSide::Buy, Price::from("108163.01"), q("2.7735913")),
                (OrderSide::Buy, Price::from("108200"), q("0.00091951")),
                (OrderSide::Buy, Price::from("108205.44"), q("0.000011")),
                (OrderSide::Buy, Price::from("108211"), q("0.00047886")),
                (OrderSide::Sell, Price::from("108275.16"), q("0.000011")),
                (OrderSide::Sell, Price::from("108275.86"), q("0.00034227")),
                (OrderSide::Sell, Price::from("108281.57"), q("0.00499456")),
            ]
        );
        // A full MATCH removes the order without a DELETE row
        assert_eq!(
            history[&18],
            [
                (BookAction::Add, q("0.01439792")),
                (BookAction::Delete, q("0.01439792"))
            ]
        );
        // MATCH subtracts the filled size
        assert_eq!(
            history[&20],
            [
                (BookAction::Add, q("0.03137")),
                (BookAction::Update, q("0.03134167")),
                (BookAction::Update, q("0.00116539")),
                (BookAction::Delete, q("0.00116539")),
            ]
        );

        // A venue identifier added again after its DELETE is a new order
        for order_id in [12, 17, 23, 25] {
            assert_eq!(history[&order_id].len(), 2);
            assert_eq!(history[&order_id][1].0, BookAction::Delete);
        }
    }

    #[rstest]
    fn load_deltas_applies_sub_and_skips_changes_to_unknown_orders() {
        let dir = TempDir::new().unwrap();
        let path = write_file(
            dir.path(),
            "book.csv",
            &[
                BOOK_HEADER,
                "00:00:00.0000000;00:00:00.0000000;SNAPSHOT;1;100;2;a",
                "00:00:01.0000000;00:00:01.0000000;SUB;1;100;0.5;a",
                "00:00:02.0000000;00:00:02.0000000;ADD;1;100;3;a",
                "00:00:03.0000000;00:00:03.0000000;SET;1;99;1;z",
                "00:00:03.0000000;00:00:03.0000000;DELETE;1;99;1;y",
                "00:00:04.0000000;00:00:04.0000000;SNAPSHOT;0;101;1;b",
                "00:00:05.0000000;00:00:05.0000000;DELETE;1;100;3;a",
            ],
        );

        let deltas = load_deltas(&path, binance(), Some(date()), None, None, None).unwrap();

        let summary: Vec<(BookAction, u64, Quantity, u8)> = deltas
            .iter()
            .map(|d| (d.action, d.order.order_id, d.order.size, d.flags))
            .collect();
        let q = Quantity::from;
        let zero = Quantity::from("0");
        assert_eq!(
            summary,
            [
                (BookAction::Clear, 0, zero, SNAPSHOT),
                (BookAction::Add, 1, q("2.0"), SNAPSHOT | LAST),
                (BookAction::Update, 1, q("1.5"), LAST),
                // An ADD for a resting order sets its size
                (BookAction::Update, 1, q("3.0"), LAST),
                // A later snapshot replaces the book, so order a is no longer resting
                (BookAction::Clear, 0, zero, SNAPSHOT),
                (BookAction::Add, 2, q("1.0"), SNAPSHOT | LAST),
            ]
        );
    }

    #[rstest]
    #[case::negative_fill(
        "MATCH;1;100;-0.5",
        None,
        "Decimal value '-0.5' is negative, Quantity must be non-negative"
    )]
    #[case::finer_than_precision(
        "MATCH;1;100;2.001",
        Some(2),
        "entry_sx 2.001 is finer than instrument precision 2"
    )]
    fn load_deltas_rejects_an_invalid_fill_before_applying_it(
        #[case] fill: &str,
        #[case] size_precision: Option<u8>,
        #[case] expected: &str,
    ) {
        let dir = TempDir::new().unwrap();
        let update = format!("00:00:01.0000000;00:00:01.0000000;{fill};a");
        let path = write_file(
            dir.path(),
            "book.csv",
            &[
                BOOK_HEADER,
                "00:00:00.0000000;00:00:00.0000000;SNAPSHOT;1;100;2;a",
                &update,
            ],
        );

        let error =
            load_deltas(&path, binance(), Some(date()), None, size_precision, None).unwrap_err();

        assert_eq!(error.to_string(), expected);
    }

    #[rstest]
    fn load_deltas_dates_rows_from_the_partition_when_not_given() {
        let dir = TempDir::new().unwrap();
        let source = fixture("limitbook_full_binance_spot_btc_usdt_20250901.csv");
        let path = dir
            .path()
            .join("T-LIMITBOOK_FULL/D-20250901/E-BINANCE/book.csv");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::copy(&source, &path).unwrap();

        let from_partition = load_deltas(&path, binance(), None, None, None, None).unwrap();
        let explicit = load_deltas(&source, binance(), Some(date()), None, None, None).unwrap();
        let error = load_deltas(&source, binance(), None, None, None, None).unwrap_err();

        assert_eq!(from_partition, explicit);
        assert!(
            error.to_string().starts_with("no date given and '"),
            "{error}"
        );
    }

    #[rstest]
    fn load_deltas_moves_a_prior_day_close_snapshot_back_one_day() {
        let dir = TempDir::new().unwrap();
        let path = write_file(
            dir.path(),
            "book.csv",
            &[
                BOOK_HEADER,
                "23:59:59.9990000;23:59:59.9990000;SNAPSHOT;1;100.5;2;a",
                "00:00:00.5000000;00:00:00.6000000;ADD;0;101;1;b",
            ],
        );

        let deltas = load_deltas(&path, binance(), Some(date()), None, None, None).unwrap();

        let previous_close = UnixNanos::from(DAY_START - 1_000_000);
        assert_eq!(deltas.len(), 3);
        assert_eq!(
            deltas
                .iter()
                .map(|d| (d.ts_event, d.ts_init, d.flags))
                .collect::<Vec<_>>(),
            [
                (previous_close, previous_close, SNAPSHOT),
                (previous_close, previous_close, SNAPSHOT | LAST),
                (at(500_000_000), at(600_000_000), LAST),
            ]
        );
    }

    #[rstest]
    fn load_deltas_moves_a_receive_time_after_midnight_to_the_next_day() {
        let dir = TempDir::new().unwrap();
        let path = write_file(
            dir.path(),
            "book.csv",
            &[
                BOOK_HEADER,
                "23:59:59.9000000;00:00:00.1000000;SET;1;100;1;",
            ],
        );

        let deltas = load_deltas(&path, binance(), Some(date()), None, None, None).unwrap();

        assert_eq!(deltas.len(), 1);
        assert_eq!(deltas[0].ts_event, at(86_399_900_000_000));
        assert_eq!(deltas[0].ts_init, at(86_400_100_000_000));
    }

    #[rstest]
    fn load_deltas_stops_reading_once_held_deltas_reach_the_limit() {
        let dir = TempDir::new().unwrap();
        let path = write_file(
            dir.path(),
            "book.csv",
            &[
                BOOK_HEADER,
                "00:00:00.0000000;00:00:00.0000000;SNAPSHOT;1;100;2;a",
                "00:00:01.0000000;00:00:01.0000000;SET;1;100.001;1;a",
            ],
        );

        let deltas = load_deltas(&path, binance(), Some(date()), Some(2), None, Some(2)).unwrap();

        assert_eq!(
            deltas
                .iter()
                .map(|d| (d.action, d.flags))
                .collect::<Vec<_>>(),
            [
                (BookAction::Clear, SNAPSHOT),
                (BookAction::Add, SNAPSHOT | LAST)
            ]
        );
    }

    #[rstest]
    #[case::after_snapshot(2)]
    #[case::inside_snapshot(2 + 1)]
    fn load_deltas_dates_a_limited_prior_day_snapshot(#[case] limit: usize) {
        let dir = TempDir::new().unwrap();
        let path = write_file(
            dir.path(),
            "book.csv",
            &[
                BOOK_HEADER,
                "23:59:59.9990000;23:59:59.9990000;SNAPSHOT;1;100.5;2;a",
                "23:59:59.9990000;23:59:59.9990000;SNAPSHOT;1;100.4;1;b",
                "23:59:59.9990000;23:59:59.9990000;SNAPSHOT;0;100.6;1;c",
                "00:00:00.5000000;00:00:00.6000000;ADD;0;101;1;d",
            ],
        );

        let deltas = load_deltas(&path, binance(), Some(date()), None, None, Some(limit)).unwrap();

        let previous_close = UnixNanos::from(DAY_START - 1_000_000);
        assert_eq!(deltas.len(), limit);
        assert!(
            deltas
                .iter()
                .all(|d| d.ts_event == previous_close && d.ts_init == previous_close)
        );
        assert_eq!(deltas.last().unwrap().flags, SNAPSHOT | LAST);
    }

    #[rstest]
    fn load_deltas_ends_a_limited_load_with_a_last_delta() {
        let deltas = load_deltas(
            fixture("limitbook_full_binance_spot_btc_usdt_20250901.csv"),
            binance(),
            Some(date()),
            None,
            None,
            Some(25),
        )
        .unwrap();

        assert_eq!(deltas.len(), 25);
        assert_eq!(deltas[20].flags, SNAPSHOT | LAST);
        assert_eq!(deltas[24].flags, LAST);
        assert!(deltas[21..24].iter().all(|d| d.flags == 0));
    }

    #[rstest]
    fn load_reads_gzipped_files() {
        let dir = TempDir::new().unwrap();
        let source = fixture("trades_binance_spot_btc_usdt_20250901.csv");
        let path = dir.path().join("trades.csv.gz");
        let mut encoder = GzEncoder::new(fs::File::create(&path).unwrap(), Compression::default());
        encoder.write_all(&fs::read(&source).unwrap()).unwrap();
        encoder.finish().unwrap();

        let gzipped = load_trades(&path, binance(), None, None, None).unwrap();
        let plain = load_trades(&source, binance(), None, None, None).unwrap();

        assert_eq!(gzipped, plain);
    }

    #[rstest]
    #[case(1)]
    #[case(7)]
    #[case(50)]
    fn stream_trades_yields_the_loaded_trades_in_chunks(#[case] chunk_size: usize) {
        let path = fixture("trades_binance_spot_btc_usdt_20250901.csv");

        let chunks: Vec<Vec<TradeTick>> =
            stream_trades(&path, binance(), chunk_size, None, None, None)
                .unwrap()
                .collect::<anyhow::Result<_>>()
                .unwrap();
        let loaded = load_trades(&path, binance(), None, None, None).unwrap();

        assert!(chunks.iter().all(|chunk| chunk.len() <= chunk_size));
        assert_eq!(chunks.len(), 50_usize.div_ceil(chunk_size));
        assert_eq!(chunks.concat(), loaded);
        assert!(chunks.concat().iter().all(|t| t.size.precision == 5));
    }

    #[rstest]
    fn stream_quotes_applies_limit() {
        let path = fixture("quotes_binance_spot_btc_usdt_20250901.csv");

        let chunks: Vec<Vec<QuoteTick>> = stream_quotes(&path, binance(), 4, None, None, Some(10))
            .unwrap()
            .collect::<anyhow::Result<_>>()
            .unwrap();

        assert_eq!(chunks.iter().map(Vec::len).collect::<Vec<_>>(), [4, 4, 2]);
        assert_eq!(
            chunks.concat(),
            load_quotes(&path, binance(), None, None, Some(10)).unwrap()
        );
    }

    #[rstest]
    fn stream_deltas_ends_every_chunk_at_an_event_boundary() {
        let path = fixture("limitbook_full_coinbase_spot_btc_usd_20250901.csv");

        let chunks: Vec<Vec<OrderBookDelta>> =
            stream_deltas(&path, coinbase(), Some(date()), 5, None, None, None)
                .unwrap()
                .collect::<anyhow::Result<_>>()
                .unwrap();
        let loaded = load_deltas(&path, coinbase(), Some(date()), None, None, None).unwrap();

        assert!(
            chunks
                .iter()
                .all(|chunk| chunk.last().unwrap().flags & LAST != 0)
        );
        assert_eq!(chunks[0].len(), 14);
        assert_eq!(chunks.concat(), loaded);
    }

    #[rstest]
    fn load_backfills_the_final_precision_onto_earlier_values() {
        let dir = TempDir::new().unwrap();
        let first = trade_row("t1", "100.5", "1");
        let second = trade_row("t2", "100.25", "0.125");
        let path = write_file(dir.path(), "trades.csv", &[TRADES_HEADER, &first, &second]);

        let trades = load_trades(&path, binance(), None, None, None).unwrap();

        assert_eq!(
            trades
                .iter()
                .map(|t| (t.price, t.price.precision, t.size, t.size.precision))
                .collect::<Vec<_>>(),
            [
                (Price::from("100.50"), 2, Quantity::from("1.000"), 3),
                (Price::from("100.25"), 2, Quantity::from("0.125"), 3),
            ]
        );
    }

    #[rstest]
    fn stream_rejects_a_finer_value_after_its_precision_sample() {
        let dir = TempDir::new().unwrap();
        let mut lines = vec![TRADES_HEADER.to_string()];
        lines.extend((0..PRECISION_SAMPLE_ROWS).map(|i| trade_row(&format!("t{i}"), "100.5", "1")));
        lines.push(trade_row("late", "100.25", "1"));
        let lines: Vec<&str> = lines.iter().map(String::as_str).collect();
        let path = write_file(dir.path(), "trades.csv", &lines);

        let results: Vec<anyhow::Result<Vec<TradeTick>>> =
            stream_trades(&path, binance(), PRECISION_SAMPLE_ROWS, None, None, None)
                .unwrap()
                .collect();

        assert_eq!(results.len(), 2);
        assert_eq!(results[0].as_ref().unwrap().len(), PRECISION_SAMPLE_ROWS);
        assert_eq!(
            results[1].as_ref().unwrap_err().to_string(),
            "price 100.25 is finer than instrument precision 1"
        );
        assert_eq!(
            load_trades(&path, binance(), None, None, None)
                .unwrap()
                .last()
                .unwrap()
                .price,
            Price::from("100.25")
        );
    }

    #[rstest]
    fn stream_yields_an_error_for_a_value_finer_than_its_precision() {
        let path = fixture("trades_binance_spot_btc_usdt_20250901.csv");

        let mut chunks = stream_trades(&path, binance(), 10, Some(1), None, None).unwrap();

        assert_eq!(
            chunks.next().unwrap().unwrap_err().to_string(),
            "price 108246.36 is finer than instrument precision 1"
        );
        assert!(chunks.next().is_none());
    }

    #[rstest]
    #[case(0)]
    #[case(STREAM_CHUNK_SIZE_MAX + 1)]
    fn stream_rejects_chunk_size_out_of_range(#[case] chunk_size: usize) {
        let path = fixture("trades_binance_spot_btc_usdt_20250901.csv");

        assert!(stream_trades(&path, binance(), chunk_size, None, None, None).is_err());
    }

    #[rstest]
    fn ohlcv_loader_loads_mapped_symbols_at_their_inferred_precisions() {
        let first = InstrumentId::from("SOLUSD.BITSTAMP");
        let second = InstrumentId::from("DOGEUSD.BITSTAMP");
        let loader = CoinApiOhlcvLoader::new([(138_492, first), (5_968_363, second)]);

        let bars = loader
            .load_bars(
                fixture("ohlcv_bitstamp_1day_20250911.csv"),
                Some("1DAY"),
                None,
            )
            .unwrap();

        let period_end = UnixNanos::from(1_757_635_200_000_000_000);
        let volume = Decimal::from_str("689.5507347100003")
            .unwrap()
            .round_dp_with_strategy(
                u32::from(FIXED_PRECISION),
                RoundingStrategy::MidpointNearestEven,
            )
            .normalize();
        assert_eq!(bars.len(), 2);
        assert_eq!(
            bars[0],
            Bar::new(
                BarType::from("SOLUSD.BITSTAMP-1-DAY-LAST-EXTERNAL"),
                Price::from("496.11"),
                Price::from("515.47"),
                Price::from("493.23"),
                Price::from("508.00"),
                Quantity::from_decimal(volume).unwrap(),
                period_end,
                period_end,
            )
        );
        assert_eq!(bars[0].close.precision, 2);
        assert_eq!(u32::from(bars[0].volume.precision), volume.scale());
        assert_eq!(
            bars[1].bar_type,
            BarType::from("DOGEUSD.BITSTAMP-1-DAY-LAST-EXTERNAL")
        );
        assert_eq!(bars[1].open.precision, 5);
        assert_eq!(bars[1].volume, Quantity::from("1474.43175271"));
    }

    #[rstest]
    #[case::negative(-100, true)]
    #[case::negative_subunit(-25, true)]
    #[case::positive_subunit(25, false)]
    #[case::positive_midpoint(50, false)]
    fn ohlcv_loader_rejects_negative_and_zero_rounded_volumes(
        #[case] hundredths: i64,
        #[case] negative: bool,
    ) {
        let dir = TempDir::new().unwrap();
        let volume = Decimal::new(1, u32::from(FIXED_PRECISION)) * Decimal::new(hundredths, 2);
        let row = format!(
            "7;2025-09-11T00:00:00;2025-09-11T00:01:00;\
             2025-09-11T00:00:01;2025-09-11T00:00:59;1;2;0.5;1.5;3;{volume}"
        );
        let path = write_file(dir.path(), "TP-1MIN.csv", &[OHLCV_HEADER, &row]);
        let loader = CoinApiOhlcvLoader::new([(7, InstrumentId::from("SOLUSD.BITSTAMP"))]);

        let error = loader.load_bars(&path, None, None).unwrap_err();

        let expected = if negative {
            format!("sx_sum must be non-negative: {volume}")
        } else {
            format!("sx_sum {volume} rounds to zero at precision {FIXED_PRECISION}")
        };

        assert_eq!(error.to_string(), expected);
    }

    #[rstest]
    #[case::zero(0, 0)]
    #[case::rounded_unit(75, 1)]
    #[case::exact_unit(100, 1)]
    #[case::midpoint_even_down(250, 2)]
    #[case::midpoint_even_up(350, 4)]
    fn ohlcv_loader_preserves_representable_volumes(#[case] hundredths: i64, #[case] units: i64) {
        let dir = TempDir::new().unwrap();
        let unit = Decimal::new(1, u32::from(FIXED_PRECISION));
        let volume = unit * Decimal::new(hundredths, 2);
        let row = format!(
            "7;2025-09-11T00:00:00;2025-09-11T00:01:00;\
             2025-09-11T00:00:01;2025-09-11T00:00:59;1;2;0.5;1.5;3;{volume}"
        );
        let path = write_file(dir.path(), "TP-1MIN.csv", &[OHLCV_HEADER, &row]);
        let loader = CoinApiOhlcvLoader::new([(7, InstrumentId::from("SOLUSD.BITSTAMP"))]);

        let bars = loader.load_bars(&path, None, None).unwrap();

        let period_end = UnixNanos::from(1_757_548_860_000_000_000);
        assert_eq!(
            bars,
            [Bar::new(
                BarType::from("SOLUSD.BITSTAMP-1-MINUTE-LAST-EXTERNAL"),
                Price::from("1.0"),
                Price::from("2.0"),
                Price::from("0.5"),
                Price::from("1.5"),
                Quantity::from_decimal(unit * Decimal::from(units)).unwrap(),
                period_end,
                period_end,
            )]
        );
    }

    #[rstest]
    fn ohlcv_loader_from_symbols_maps_symbol_id_int_to_native_instruments() {
        let mut symbols: Vec<Symbol> =
            serde_json::from_str(include_str!("../../test_data/symbols_spot.json")).unwrap();
        let kraken = symbols
            .iter_mut()
            .find(|symbol| symbol.symbol_id == "KRAKEN_SPOT_BTC_USD")
            .unwrap();
        kraken.symbol_id_int = None;
        let loader = CoinApiOhlcvLoader::from_symbols(&symbols);

        let dir = TempDir::new().unwrap();

        let row = |symbol_id: &str| {
            format!(
                "{symbol_id};2025-09-11T00:00:00.0000000;2025-09-11T00:01:00.0000000;\
                 2025-09-11T00:00:01.0000000;2025-09-11T00:00:59.0000000;1;2;0.5;1.5;3;4"
            )
        };

        let rows = [row("138123"), row("27576"), row("599"), row("1")];
        let mut lines = vec![
            "symbol_id;time_period_start;time_period_end;time_open;time_close;px_open;px_high;\
             px_low;px_close;sx_cnt;sx_sum",
        ];
        lines.extend(rows.iter().map(String::as_str));
        let path = write_file(dir.path(), "TP-1MIN.csv", &lines);

        let bars = loader.load_bars(&path, None, None).unwrap();

        assert_eq!(
            bars.iter().map(|bar| bar.bar_type).collect::<Vec<_>>(),
            [
                BarType::from("BTCUSDT.BINANCE-1-MINUTE-LAST-EXTERNAL"),
                BarType::from("BTC-USD.COINBASE-1-MINUTE-LAST-EXTERNAL"),
            ]
        );
    }

    #[rstest]
    fn ohlcv_loader_backfills_each_instruments_final_precision() {
        let dir = TempDir::new().unwrap();

        let row = |open: &str| {
            format!(
                "7;2025-09-11T00:00:00.0000000;2025-09-11T00:01:00.0000000;\
                 2025-09-11T00:00:01.0000000;2025-09-11T00:00:59.0000000;{open};2;0.5;1.5;3;4"
            )
        };

        let (first, second) = (row("1"), row("1.125"));
        let path = write_file(dir.path(), "TP-1MIN.csv", &[OHLCV_HEADER, &first, &second]);
        let loader = CoinApiOhlcvLoader::new([(7, InstrumentId::from("SOLUSD.BITSTAMP"))]);

        let bars = loader.load_bars(&path, None, None).unwrap();

        assert_eq!(bars.len(), 2);
        assert_eq!(bars[0].open, Price::from("1.000"));
        assert_eq!(bars[1].open, Price::from("1.125"));
        assert!(bars.iter().all(|bar| {
            [bar.open, bar.high, bar.low, bar.close]
                .iter()
                .all(|price| price.precision == 3)
        }));
    }

    #[rstest]
    fn ohlcv_loader_reads_the_period_from_the_file_name() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("TP-1DAY.csv");
        fs::copy(fixture("ohlcv_bitstamp_1day_20250911.csv"), &path).unwrap();
        let loader = CoinApiOhlcvLoader::new([(138_492, InstrumentId::from("SOLUSD.BITSTAMP"))]);

        let bars = loader.load_bars(&path, None, None).unwrap();
        let error = loader
            .load_bars(fixture("ohlcv_bitstamp_1day_20250911.csv"), None, None)
            .unwrap_err();

        assert_eq!(bars.len(), 1);
        assert_eq!(bars[0].bar_type.spec().to_string(), "1-DAY-LAST");
        assert!(
            error.to_string().starts_with("no period given and '"),
            "{error}"
        );
    }

    #[rstest]
    #[case(0)]
    #[case(1)]
    #[case(2)]
    #[case(3)]
    fn ohlcv_limit_counts_mapped_bars(#[case] limit: usize) {
        let dir = TempDir::new().unwrap();
        let path = write_file(
            dir.path(),
            "TP-1MIN.csv",
            &[
                OHLCV_HEADER,
                "99;2025-09-01T00:00:00;2025-09-01T00:01:00;;;;;;;0;0",
                "7;2025-09-01T00:00:00;2025-09-01T00:01:00;;;101.25;103.5;99.75;102.5;3;4.125",
                "99;2025-09-01T00:01:00;2025-09-01T00:02:00;;;;;;;0;0",
                "7;2025-09-01T00:01:00;2025-09-01T00:02:00;;;102.5;104.75;100.25;103.25;5;6.375",
            ],
        );
        let loader = CoinApiOhlcvLoader::new([(7, coinbase())]);
        let expected = [
            Bar::new(
                BarType::from("BTC-USD.COINBASE-1-MINUTE-LAST-EXTERNAL"),
                Price::from("101.25"),
                Price::from("103.50"),
                Price::from("99.75"),
                Price::from("102.50"),
                Quantity::from("4.125"),
                at(60_000_000_000),
                at(60_000_000_000),
            ),
            Bar::new(
                BarType::from("BTC-USD.COINBASE-1-MINUTE-LAST-EXTERNAL"),
                Price::from("102.50"),
                Price::from("104.75"),
                Price::from("100.25"),
                Price::from("103.25"),
                Quantity::from("6.375"),
                at(120_000_000_000),
                at(120_000_000_000),
            ),
        ];

        let bars = loader.load_bars(path, None, Some(limit)).unwrap();

        assert_eq!(bars, expected[..limit.min(2)]);
        assert!(bars.iter().all(|bar| {
            [bar.open, bar.high, bar.low, bar.close]
                .iter()
                .all(|p| p.precision == 2)
                && bar.volume.precision == 3
        }));
    }

    #[rstest]
    fn load_quotes_backfills_all_four_precisions() {
        let dir = TempDir::new().unwrap();
        let path = write_file(
            dir.path(),
            "quotes.csv",
            &[
                "time_exchange;time_coinapi;ask_px;ask_sx;bid_px;bid_sx",
                "2025-09-01T00:00:00.1000000;2025-09-01T00:00:00.2000000;101.25;2.5;100.5;1",
                "2025-09-01T00:00:01.3000000;2025-09-01T00:00:01.4000000;101.375;2.0625;100.125;0.125",
            ],
        );

        let quotes = load_quotes(path, coinbase(), None, None, None).unwrap();

        assert_eq!(
            quotes,
            [
                QuoteTick::new(
                    coinbase(),
                    Price::from("100.500"),
                    Price::from("101.250"),
                    Quantity::from("1.0000"),
                    Quantity::from("2.5000"),
                    at(100_000_000),
                    at(200_000_000)
                ),
                QuoteTick::new(
                    coinbase(),
                    Price::from("100.125"),
                    Price::from("101.375"),
                    Quantity::from("0.1250"),
                    Quantity::from("2.0625"),
                    at(1_300_000_000),
                    at(1_400_000_000)
                ),
            ]
        );
        assert_eq!(
            quotes
                .iter()
                .map(|q| (
                    q.bid_price.precision,
                    q.ask_price.precision,
                    q.bid_size.precision,
                    q.ask_size.precision
                ))
                .collect::<Vec<_>>(),
            [(3, 3, 4, 4); 2]
        );
    }

    #[rstest]
    #[case(0, false)]
    #[case(1, false)]
    #[case(0, true)]
    #[case(1, true)]
    fn snapshot_limit_retains_a_complete_clear_event(
        #[case] limit: usize,
        #[case] streaming: bool,
    ) {
        let dir = TempDir::new().unwrap();
        let path = write_file(
            dir.path(),
            "book.csv",
            &[
                BOOK_HEADER,
                "00:00:00.0000000;00:00:00.2500000;SNAPSHOT;1;100.50;2.0;a",
                "00:00:01.0000000;00:00:01.5000000;SET;1;101.25;3.0;a",
            ],
        );

        let deltas = if streaming {
            stream_deltas(
                &path,
                coinbase(),
                Some(date()),
                1,
                Some(2),
                Some(1),
                Some(limit),
            )
            .unwrap()
            .collect::<anyhow::Result<Vec<_>>>()
            .unwrap()
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
        } else {
            load_deltas(
                &path,
                coinbase(),
                Some(date()),
                Some(2),
                Some(1),
                Some(limit),
            )
            .unwrap()
        };

        let mut clear = OrderBookDelta::clear(coinbase(), 0, at(0), at(250_000_000));
        clear.flags = SNAPSHOT | LAST;

        assert_eq!(deltas, [clear][..limit]);
        assert!(
            deltas
                .iter()
                .all(|d| d.order.price.precision == 0 && d.order.size.precision == 0)
        );
    }

    #[rstest]
    #[case(false)]
    #[case(true)]
    fn zero_sized_trades_do_not_consume_the_limit(#[case] streaming: bool) {
        let dir = TempDir::new().unwrap();
        let zero = trade_row("zero", "99.75", "0");
        let first = trade_row("first", "101.25", "2.125");
        let second = trade_row("second", "102.50", "3.375");
        let path = write_file(
            dir.path(),
            "trades.csv",
            &[TRADES_HEADER, &zero, &first, &zero, &second],
        );

        let trades = if streaming {
            stream_trades(&path, coinbase(), 1, Some(2), Some(3), Some(1))
                .unwrap()
                .collect::<anyhow::Result<Vec<_>>>()
                .unwrap()
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
        } else {
            load_trades(&path, coinbase(), Some(2), Some(3), Some(1)).unwrap()
        };

        assert_eq!(
            trades,
            [TradeTick::new(
                coinbase(),
                Price::from("101.25"),
                Quantity::from("2.125"),
                AggressorSide::Buy,
                TradeId::from("first"),
                at(0),
                at(0)
            )]
        );
        assert_eq!(
            (trades[0].price.precision, trades[0].size.precision),
            (2, 3)
        );
    }

    #[rstest]
    #[case::missing_column(
        "time_exchange;time_coinapi;guid;base_amount;taker_side",
        "2025-09-01T00:00:00;2025-09-01T00:00:01;first;2;BUY",
        "flat file has no 'price' column"
    )]
    #[case::invalid_price(
        TRADES_HEADER,
        "2025-09-01T00:00:00;2025-09-01T00:00:01;first;bad;2;BUY",
        "invalid price 'bad':"
    )]
    #[case::invalid_exchange_time(
        TRADES_HEADER,
        "bad;2025-09-01T00:00:01;first;101.25;2;BUY",
        "invalid time_exchange 'bad':"
    )]
    #[case::invalid_receive_time(
        TRADES_HEADER,
        "2025-09-01T00:00:00;bad;first;101.25;2;BUY",
        "invalid time_coinapi 'bad':"
    )]
    #[case::invalid_size(
        TRADES_HEADER,
        "2025-09-01T00:00:00;2025-09-01T00:00:01;first;101.25;bad;BUY",
        "invalid base_amount 'bad':"
    )]
    fn malformed_trade_rows_report_the_field_and_source_value(
        #[case] header: &str,
        #[case] row: &str,
        #[case] context: &str,
    ) {
        let dir = TempDir::new().unwrap();
        let path = write_file(dir.path(), "trades.csv", &[header, row]);

        let error = load_trades(path, coinbase(), None, None, None)
            .unwrap_err()
            .to_string();

        if context.ends_with(':') {
            assert!(error.starts_with(&format!("{context} ")), "{error}");
        } else {
            assert_eq!(error, context);
        }
    }
}
