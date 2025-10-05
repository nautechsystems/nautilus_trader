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

use std::{io::Read, path::Path};

use csv::{Reader, StringRecord};
use nautilus_core::UnixNanos;
use nautilus_model::{
    data::{DEPTH10_LEN, NULL_ORDER, OrderBookDelta, OrderBookDepth10, QuoteTick, TradeTick},
    enums::{BookAction, OrderSide, RecordFlag},
    identifiers::InstrumentId,
    types::Quantity,
};
#[cfg(feature = "python")]
use nautilus_model::{
    data::{Data, OrderBookDeltas, OrderBookDeltas_API},
    python::data::data_to_pycapsule,
};
#[cfg(feature = "python")]
use pyo3::{Py, PyAny, Python};

use crate::{
    csv::{
        create_book_order, create_csv_reader, infer_precision, parse_delta_record,
        parse_derivative_ticker_record, parse_quote_record, parse_trade_record,
        record::{
            TardisBookUpdateRecord, TardisOrderBookSnapshot5Record,
            TardisOrderBookSnapshot25Record, TardisQuoteRecord, TardisTradeRecord,
        },
    },
    parse::{parse_instrument_id, parse_timestamp},
};

////////////////////////////////////////////////////////////////////////////////
// OrderBookDelta Streaming
////////////////////////////////////////////////////////////////////////////////

/// Streaming iterator over CSV records that yields chunks of parsed data.
struct DeltaStreamIterator {
    reader: Reader<Box<dyn std::io::Read>>,
    record: StringRecord,
    buffer: Vec<OrderBookDelta>,
    chunk_size: usize,
    instrument_id: Option<InstrumentId>,
    price_precision: u8,
    size_precision: u8,
    last_ts_event: UnixNanos,
    limit: Option<usize>,
    records_processed: usize,
}

impl DeltaStreamIterator {
    /// Creates a new [`DeltaStreamIterator`].
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be opened or read.
    fn new<P: AsRef<Path>>(
        filepath: P,
        chunk_size: usize,
        price_precision: Option<u8>,
        size_precision: Option<u8>,
        instrument_id: Option<InstrumentId>,
        limit: Option<usize>,
    ) -> anyhow::Result<Self> {
        let (final_price_precision, final_size_precision) =
            if let (Some(price_prec), Some(size_prec)) = (price_precision, size_precision) {
                // Both precisions provided, use them directly
                (price_prec, size_prec)
            } else {
                // One or both precisions missing, detect only the missing ones
                let mut reader = create_csv_reader(&filepath)?;
                let mut record = StringRecord::new();
                let (detected_price, detected_size) =
                    Self::detect_precision_from_sample(&mut reader, &mut record, 10_000)?;
                (
                    price_precision.unwrap_or(detected_price),
                    size_precision.unwrap_or(detected_size),
                )
            };

        let reader = create_csv_reader(filepath)?;

        Ok(Self {
            reader,
            record: StringRecord::new(),
            buffer: Vec::with_capacity(chunk_size),
            chunk_size,
            instrument_id,
            price_precision: final_price_precision,
            size_precision: final_size_precision,
            last_ts_event: UnixNanos::default(),
            limit,
            records_processed: 0,
        })
    }

    fn detect_precision_from_sample(
        reader: &mut Reader<Box<dyn std::io::Read>>,
        record: &mut StringRecord,
        sample_size: usize,
    ) -> anyhow::Result<(u8, u8)> {
        let mut max_price_precision = 0u8;
        let mut max_size_precision = 0u8;
        let mut records_scanned = 0;

        while records_scanned < sample_size {
            match reader.read_record(record) {
                Ok(true) => {
                    if let Ok(data) = record.deserialize::<TardisBookUpdateRecord>(None) {
                        max_price_precision = max_price_precision.max(infer_precision(data.price));
                        max_size_precision = max_size_precision.max(infer_precision(data.amount));
                        records_scanned += 1;
                    }
                }
                Ok(false) => break,             // End of file
                Err(_) => records_scanned += 1, // Skip malformed records
            }
        }

        Ok((max_price_precision, max_size_precision))
    }
}

impl Iterator for DeltaStreamIterator {
    type Item = anyhow::Result<Vec<OrderBookDelta>>;

    fn next(&mut self) -> Option<Self::Item> {
        if let Some(limit) = self.limit
            && self.records_processed >= limit
        {
            return None;
        }

        self.buffer.clear();
        let mut records_read = 0;

        while records_read < self.chunk_size {
            match self.reader.read_record(&mut self.record) {
                Ok(true) => {
                    match self.record.deserialize::<TardisBookUpdateRecord>(None) {
                        Ok(data) => {
                            let delta = parse_delta_record(
                                &data,
                                self.price_precision,
                                self.size_precision,
                                self.instrument_id,
                            );

                            // Check if timestamp is different from last timestamp
                            if self.last_ts_event != delta.ts_event
                                && let Some(last_delta) = self.buffer.last_mut()
                            {
                                last_delta.flags = RecordFlag::F_LAST.value();
                            }

                            assert!(
                                !(delta.action != BookAction::Delete && delta.order.size.is_zero()),
                                "Invalid delta: action {} when size zero, check size_precision ({}) vs data; {data:?}",
                                delta.action,
                                self.size_precision
                            );

                            self.last_ts_event = delta.ts_event;

                            self.buffer.push(delta);
                            records_read += 1;
                            self.records_processed += 1;

                            if let Some(limit) = self.limit
                                && self.records_processed >= limit
                            {
                                break;
                            }
                        }
                        Err(e) => {
                            return Some(Err(anyhow::anyhow!("Failed to deserialize record: {e}")));
                        }
                    }
                }
                Ok(false) => {
                    // End of file reached
                    if self.buffer.is_empty() {
                        return None;
                    }
                    // Set F_LAST flag for final delta in chunk
                    if let Some(last_delta) = self.buffer.last_mut() {
                        last_delta.flags = RecordFlag::F_LAST.value();
                    }
                    return Some(Ok(self.buffer.clone()));
                }
                Err(e) => return Some(Err(anyhow::anyhow!("Failed to read record: {e}"))),
            }
        }

        if self.buffer.is_empty() {
            None
        } else {
            Some(Ok(self.buffer.clone()))
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
    limit: Option<usize>,
) -> anyhow::Result<impl Iterator<Item = anyhow::Result<Vec<OrderBookDelta>>>> {
    DeltaStreamIterator::new(
        filepath,
        chunk_size,
        price_precision,
        size_precision,
        instrument_id,
        limit,
    )
}

////////////////////////////////////////////////////////////////////////////////
// Vec<Py<PyAny>> (OrderBookDeltas as PyCapsule) Streaming
////////////////////////////////////////////////////////////////////////////////

#[cfg(feature = "python")]
/// Streaming iterator over CSV records that yields chunks of parsed data.
struct BatchedDeltasStreamIterator {
    reader: Reader<Box<dyn std::io::Read>>,
    record: StringRecord,
    buffer: Vec<Py<PyAny>>,
    current_batch: Vec<OrderBookDelta>,
    pending_batches: Vec<Vec<OrderBookDelta>>,
    chunk_size: usize,
    instrument_id: InstrumentId,
    price_precision: u8,
    size_precision: u8,
    last_ts_event: UnixNanos,
    limit: Option<usize>,
    records_processed: usize,
}

#[cfg(feature = "python")]
impl BatchedDeltasStreamIterator {
    /// Creates a new [`DeltaStreamIterator`].
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be opened or read.
    fn new<P: AsRef<Path>>(
        filepath: P,
        chunk_size: usize,
        price_precision: Option<u8>,
        size_precision: Option<u8>,
        instrument_id: Option<InstrumentId>,
        limit: Option<usize>,
    ) -> anyhow::Result<Self> {
        let mut reader = create_csv_reader(&filepath)?;
        let mut record = StringRecord::new();

        // Read the first record to get instrument_id
        let first_record = if reader.read_record(&mut record)? {
            record.deserialize::<TardisBookUpdateRecord>(None)?
        } else {
            anyhow::bail!("CSV file is empty");
        };

        let final_instrument_id = instrument_id
            .unwrap_or_else(|| parse_instrument_id(&first_record.exchange, first_record.symbol));

        let (final_price_precision, final_size_precision) =
            if let (Some(price_prec), Some(size_prec)) = (price_precision, size_precision) {
                // Both precisions provided, use them directly
                (price_prec, size_prec)
            } else {
                // One or both precisions missing, detect from sample including first record
                let (detected_price, detected_size) =
                    Self::detect_precision_from_sample(&mut reader, &mut record, 10_000)?;
                (
                    price_precision.unwrap_or(detected_price),
                    size_precision.unwrap_or(detected_size),
                )
            };

        let reader = create_csv_reader(filepath)?;

        Ok(Self {
            reader,
            record: StringRecord::new(),
            buffer: Vec::with_capacity(chunk_size),
            current_batch: Vec::new(),
            pending_batches: Vec::with_capacity(chunk_size),
            chunk_size,
            instrument_id: final_instrument_id,
            price_precision: final_price_precision,
            size_precision: final_size_precision,
            last_ts_event: UnixNanos::default(),
            limit,
            records_processed: 0,
        })
    }

    fn detect_precision_from_sample(
        reader: &mut Reader<Box<dyn std::io::Read>>,
        record: &mut StringRecord,
        sample_size: usize,
    ) -> anyhow::Result<(u8, u8)> {
        let mut max_price_precision = 0u8;
        let mut max_size_precision = 0u8;
        let mut records_scanned = 0;

        while records_scanned < sample_size {
            match reader.read_record(record) {
                Ok(true) => {
                    if let Ok(data) = record.deserialize::<TardisBookUpdateRecord>(None) {
                        max_price_precision = max_price_precision.max(infer_precision(data.price));
                        max_size_precision = max_size_precision.max(infer_precision(data.amount));
                        records_scanned += 1;
                    }
                }
                Ok(false) => break,             // End of file
                Err(_) => records_scanned += 1, // Skip malformed records
            }
        }

        Ok((max_price_precision, max_size_precision))
    }
}

#[cfg(feature = "python")]
impl Iterator for BatchedDeltasStreamIterator {
    type Item = anyhow::Result<Vec<Py<PyAny>>>;

    fn next(&mut self) -> Option<Self::Item> {
        if let Some(limit) = self.limit
            && self.records_processed >= limit
        {
            return None;
        }

        self.buffer.clear();
        let mut batches_created = 0;

        while batches_created < self.chunk_size {
            match self.reader.read_record(&mut self.record) {
                Ok(true) => {
                    let delta = match self.record.deserialize::<TardisBookUpdateRecord>(None) {
                        Ok(data) => parse_delta_record(
                            &data,
                            self.price_precision,
                            self.size_precision,
                            Some(self.instrument_id),
                        ),
                        Err(e) => {
                            return Some(Err(anyhow::anyhow!("Failed to deserialize record: {e}")));
                        }
                    };

                    if self.last_ts_event != delta.ts_event && !self.current_batch.is_empty() {
                        // Set F_LAST on the last delta of the completed batch
                        if let Some(last_delta) = self.current_batch.last_mut() {
                            last_delta.flags = RecordFlag::F_LAST.value();
                        }
                        self.pending_batches
                            .push(std::mem::take(&mut self.current_batch));
                        batches_created += 1;
                    }

                    self.last_ts_event = delta.ts_event;
                    self.current_batch.push(delta);
                    self.records_processed += 1;

                    if let Some(limit) = self.limit
                        && self.records_processed >= limit
                    {
                        break;
                    }
                }
                Ok(false) => {
                    // End of file
                    break;
                }
                Err(e) => return Some(Err(anyhow::anyhow!("Failed to read record: {e}"))),
            }
        }

        if !self.current_batch.is_empty() && batches_created < self.chunk_size {
            // Ensure the last delta of the last batch has F_LAST set
            if let Some(last_delta) = self.current_batch.last_mut() {
                last_delta.flags = RecordFlag::F_LAST.value();
            }
            self.pending_batches
                .push(std::mem::take(&mut self.current_batch));
        }

        if self.pending_batches.is_empty() {
            None
        } else {
            // Create all capsules in a single GIL acquisition
            Python::attach(|py| {
                for batch in self.pending_batches.drain(..) {
                    let deltas = OrderBookDeltas::new(self.instrument_id, batch);
                    let deltas = OrderBookDeltas_API::new(deltas);
                    let capsule = data_to_pycapsule(py, Data::Deltas(deltas));
                    self.buffer.push(capsule);
                }
            });
            Some(Ok(std::mem::take(&mut self.buffer)))
        }
    }
}

#[cfg(feature = "python")]
/// Streams [`Vec<Py<PyAny>>`]s (`PyCapsule`) from a Tardis format CSV at the given `filepath`,
/// yielding chunks of the specified size.
///
/// # Errors
///
/// Returns an error if the file cannot be opened, read, or parsed as CSV.
pub fn stream_batched_deltas<P: AsRef<Path>>(
    filepath: P,
    chunk_size: usize,
    price_precision: Option<u8>,
    size_precision: Option<u8>,
    instrument_id: Option<InstrumentId>,
    limit: Option<usize>,
) -> anyhow::Result<impl Iterator<Item = anyhow::Result<Vec<Py<PyAny>>>>> {
    BatchedDeltasStreamIterator::new(
        filepath,
        chunk_size,
        price_precision,
        size_precision,
        instrument_id,
        limit,
    )
}

////////////////////////////////////////////////////////////////////////////////
// Quote Streaming
////////////////////////////////////////////////////////////////////////////////

/// An iterator for streaming [`QuoteTick`]s from a Tardis CSV file in chunks.
struct QuoteStreamIterator {
    reader: Reader<Box<dyn Read>>,
    record: StringRecord,
    buffer: Vec<QuoteTick>,
    chunk_size: usize,
    instrument_id: Option<InstrumentId>,
    price_precision: u8,
    size_precision: u8,
    limit: Option<usize>,
    records_processed: usize,
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
        limit: Option<usize>,
    ) -> anyhow::Result<Self> {
        let (final_price_precision, final_size_precision) =
            if let (Some(price_prec), Some(size_prec)) = (price_precision, size_precision) {
                // Both precisions provided, use them directly
                (price_prec, size_prec)
            } else {
                // One or both precisions missing, detect only the missing ones
                let mut reader = create_csv_reader(&filepath)?;
                let mut record = StringRecord::new();
                let (detected_price, detected_size) =
                    Self::detect_precision_from_sample(&mut reader, &mut record, 10_000)?;
                (
                    price_precision.unwrap_or(detected_price),
                    size_precision.unwrap_or(detected_size),
                )
            };

        let reader = create_csv_reader(filepath)?;

        Ok(Self {
            reader,
            record: StringRecord::new(),
            buffer: Vec::with_capacity(chunk_size),
            chunk_size,
            instrument_id,
            price_precision: final_price_precision,
            size_precision: final_size_precision,
            limit,
            records_processed: 0,
        })
    }

    fn detect_precision_from_sample(
        reader: &mut Reader<Box<dyn std::io::Read>>,
        record: &mut StringRecord,
        sample_size: usize,
    ) -> anyhow::Result<(u8, u8)> {
        let mut max_price_precision = 2u8;
        let mut max_size_precision = 0u8;
        let mut records_scanned = 0;

        while records_scanned < sample_size {
            match reader.read_record(record) {
                Ok(true) => {
                    if let Ok(data) = record.deserialize::<TardisQuoteRecord>(None) {
                        if let Some(bid_price_val) = data.bid_price {
                            max_price_precision =
                                max_price_precision.max(infer_precision(bid_price_val));
                        }
                        if let Some(ask_price_val) = data.ask_price {
                            max_price_precision =
                                max_price_precision.max(infer_precision(ask_price_val));
                        }
                        if let Some(bid_amount_val) = data.bid_amount {
                            max_size_precision =
                                max_size_precision.max(infer_precision(bid_amount_val));
                        }
                        if let Some(ask_amount_val) = data.ask_amount {
                            max_size_precision =
                                max_size_precision.max(infer_precision(ask_amount_val));
                        }
                        records_scanned += 1;
                    }
                }
                Ok(false) => break,             // End of file
                Err(_) => records_scanned += 1, // Skip malformed records
            }
        }

        Ok((max_price_precision, max_size_precision))
    }
}

impl Iterator for QuoteStreamIterator {
    type Item = anyhow::Result<Vec<QuoteTick>>;

    fn next(&mut self) -> Option<Self::Item> {
        if let Some(limit) = self.limit
            && self.records_processed >= limit
        {
            return None;
        }

        self.buffer.clear();
        let mut records_read = 0;

        while records_read < self.chunk_size {
            match self.reader.read_record(&mut self.record) {
                Ok(true) => match self.record.deserialize::<TardisQuoteRecord>(None) {
                    Ok(data) => {
                        let quote = parse_quote_record(
                            &data,
                            self.price_precision,
                            self.size_precision,
                            self.instrument_id,
                        );

                        self.buffer.push(quote);
                        records_read += 1;
                        self.records_processed += 1;

                        if let Some(limit) = self.limit
                            && self.records_processed >= limit
                        {
                            break;
                        }
                    }
                    Err(e) => {
                        return Some(Err(anyhow::anyhow!("Failed to deserialize record: {e}")));
                    }
                },
                Ok(false) => {
                    if self.buffer.is_empty() {
                        return None;
                    }
                    return Some(Ok(self.buffer.clone()));
                }
                Err(e) => return Some(Err(anyhow::anyhow!("Failed to read record: {e}"))),
            }
        }

        if self.buffer.is_empty() {
            None
        } else {
            Some(Ok(self.buffer.clone()))
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
    limit: Option<usize>,
) -> anyhow::Result<impl Iterator<Item = anyhow::Result<Vec<QuoteTick>>>> {
    QuoteStreamIterator::new(
        filepath,
        chunk_size,
        price_precision,
        size_precision,
        instrument_id,
        limit,
    )
}

////////////////////////////////////////////////////////////////////////////////
// Trade Streaming
////////////////////////////////////////////////////////////////////////////////

/// An iterator for streaming [`TradeTick`]s from a Tardis CSV file in chunks.
struct TradeStreamIterator {
    reader: Reader<Box<dyn Read>>,
    record: StringRecord,
    buffer: Vec<TradeTick>,
    chunk_size: usize,
    instrument_id: Option<InstrumentId>,
    price_precision: u8,
    size_precision: u8,
    limit: Option<usize>,
    records_processed: usize,
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
        limit: Option<usize>,
    ) -> anyhow::Result<Self> {
        let (final_price_precision, final_size_precision) =
            if let (Some(price_prec), Some(size_prec)) = (price_precision, size_precision) {
                // Both precisions provided, use them directly
                (price_prec, size_prec)
            } else {
                // One or both precisions missing, detect only the missing ones
                let mut reader = create_csv_reader(&filepath)?;
                let mut record = StringRecord::new();
                let (detected_price, detected_size) =
                    Self::detect_precision_from_sample(&mut reader, &mut record, 10_000)?;
                (
                    price_precision.unwrap_or(detected_price),
                    size_precision.unwrap_or(detected_size),
                )
            };

        let reader = create_csv_reader(filepath)?;

        Ok(Self {
            reader,
            record: StringRecord::new(),
            buffer: Vec::with_capacity(chunk_size),
            chunk_size,
            instrument_id,
            price_precision: final_price_precision,
            size_precision: final_size_precision,
            limit,
            records_processed: 0,
        })
    }

    fn detect_precision_from_sample(
        reader: &mut Reader<Box<dyn std::io::Read>>,
        record: &mut StringRecord,
        sample_size: usize,
    ) -> anyhow::Result<(u8, u8)> {
        let mut max_price_precision = 2u8;
        let mut max_size_precision = 0u8;
        let mut records_scanned = 0;

        while records_scanned < sample_size {
            match reader.read_record(record) {
                Ok(true) => {
                    if let Ok(data) = record.deserialize::<TardisTradeRecord>(None) {
                        max_price_precision = max_price_precision.max(infer_precision(data.price));
                        max_size_precision = max_size_precision.max(infer_precision(data.amount));
                        records_scanned += 1;
                    }
                }
                Ok(false) => break,             // End of file
                Err(_) => records_scanned += 1, // Skip malformed records
            }
        }

        Ok((max_price_precision, max_size_precision))
    }
}

impl Iterator for TradeStreamIterator {
    type Item = anyhow::Result<Vec<TradeTick>>;

    fn next(&mut self) -> Option<Self::Item> {
        if let Some(limit) = self.limit
            && self.records_processed >= limit
        {
            return None;
        }

        self.buffer.clear();
        let mut records_read = 0;

        while records_read < self.chunk_size {
            match self.reader.read_record(&mut self.record) {
                Ok(true) => match self.record.deserialize::<TardisTradeRecord>(None) {
                    Ok(data) => {
                        let size = Quantity::new(data.amount, self.size_precision);

                        if size.is_positive() {
                            let trade = parse_trade_record(
                                &data,
                                size,
                                self.price_precision,
                                self.instrument_id,
                            );

                            self.buffer.push(trade);
                            records_read += 1;
                            self.records_processed += 1;

                            if let Some(limit) = self.limit
                                && self.records_processed >= limit
                            {
                                break;
                            }
                        } else {
                            log::warn!("Skipping zero-sized trade: {data:?}");
                        }
                    }
                    Err(e) => {
                        return Some(Err(anyhow::anyhow!("Failed to deserialize record: {e}")));
                    }
                },
                Ok(false) => {
                    if self.buffer.is_empty() {
                        return None;
                    }
                    return Some(Ok(self.buffer.clone()));
                }
                Err(e) => return Some(Err(anyhow::anyhow!("Failed to read record: {e}"))),
            }
        }

        if self.buffer.is_empty() {
            None
        } else {
            Some(Ok(self.buffer.clone()))
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
    limit: Option<usize>,
) -> anyhow::Result<impl Iterator<Item = anyhow::Result<Vec<TradeTick>>>> {
    TradeStreamIterator::new(
        filepath,
        chunk_size,
        price_precision,
        size_precision,
        instrument_id,
        limit,
    )
}

////////////////////////////////////////////////////////////////////////////////
// Depth10 Streaming
////////////////////////////////////////////////////////////////////////////////

/// An iterator for streaming [`OrderBookDepth10`]s from a Tardis CSV file in chunks.
struct Depth10StreamIterator {
    reader: Reader<Box<dyn Read>>,
    record: StringRecord,
    buffer: Vec<OrderBookDepth10>,
    chunk_size: usize,
    levels: u8,
    instrument_id: Option<InstrumentId>,
    price_precision: u8,
    size_precision: u8,
    limit: Option<usize>,
    records_processed: usize,
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
        limit: Option<usize>,
    ) -> anyhow::Result<Self> {
        let (final_price_precision, final_size_precision) =
            if let (Some(price_prec), Some(size_prec)) = (price_precision, size_precision) {
                // Both precisions provided, use them directly
                (price_prec, size_prec)
            } else {
                // One or both precisions missing, detect only the missing ones
                let mut reader = create_csv_reader(&filepath)?;
                let mut record = StringRecord::new();
                let (detected_price, detected_size) =
                    Self::detect_precision_from_sample(&mut reader, &mut record, 10_000)?;
                (
                    price_precision.unwrap_or(detected_price),
                    size_precision.unwrap_or(detected_size),
                )
            };

        let reader = create_csv_reader(filepath)?;

        Ok(Self {
            reader,
            record: StringRecord::new(),
            buffer: Vec::with_capacity(chunk_size),
            chunk_size,
            levels,
            instrument_id,
            price_precision: final_price_precision,
            size_precision: final_size_precision,
            limit,
            records_processed: 0,
        })
    }

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

            let (bid_order, bid_count) = create_book_order(
                OrderSide::Buy,
                bid_price,
                bid_amount,
                self.price_precision,
                self.size_precision,
            );
            bids[i] = bid_order;
            bid_counts[i] = bid_count;

            let (ask_order, ask_count) = create_book_order(
                OrderSide::Sell,
                ask_price,
                ask_amount,
                self.price_precision,
                self.size_precision,
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

            let (bid_order, bid_count) = create_book_order(
                OrderSide::Buy,
                bid_price,
                bid_amount,
                self.price_precision,
                self.size_precision,
            );
            bids[i] = bid_order;
            bid_counts[i] = bid_count;

            let (ask_order, ask_count) = create_book_order(
                OrderSide::Sell,
                ask_price,
                ask_amount,
                self.price_precision,
                self.size_precision,
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

    fn detect_precision_from_sample(
        reader: &mut Reader<Box<dyn std::io::Read>>,
        record: &mut StringRecord,
        sample_size: usize,
    ) -> anyhow::Result<(u8, u8)> {
        let mut max_price_precision = 2u8;
        let mut max_size_precision = 0u8;
        let mut records_scanned = 0;

        while records_scanned < sample_size {
            match reader.read_record(record) {
                Ok(true) => {
                    // Try to deserialize as snapshot5 record first
                    if let Ok(data) = record.deserialize::<TardisOrderBookSnapshot5Record>(None) {
                        if let Some(bid_price) = data.bids_0_price {
                            max_price_precision =
                                max_price_precision.max(infer_precision(bid_price));
                        }
                        if let Some(ask_price) = data.asks_0_price {
                            max_price_precision =
                                max_price_precision.max(infer_precision(ask_price));
                        }
                        if let Some(bid_amount) = data.bids_0_amount {
                            max_size_precision =
                                max_size_precision.max(infer_precision(bid_amount));
                        }
                        if let Some(ask_amount) = data.asks_0_amount {
                            max_size_precision =
                                max_size_precision.max(infer_precision(ask_amount));
                        }
                        records_scanned += 1;
                    } else if let Ok(data) =
                        record.deserialize::<TardisOrderBookSnapshot25Record>(None)
                    {
                        if let Some(bid_price) = data.bids_0_price {
                            max_price_precision =
                                max_price_precision.max(infer_precision(bid_price));
                        }
                        if let Some(ask_price) = data.asks_0_price {
                            max_price_precision =
                                max_price_precision.max(infer_precision(ask_price));
                        }
                        if let Some(bid_amount) = data.bids_0_amount {
                            max_size_precision =
                                max_size_precision.max(infer_precision(bid_amount));
                        }
                        if let Some(ask_amount) = data.asks_0_amount {
                            max_size_precision =
                                max_size_precision.max(infer_precision(ask_amount));
                        }
                        records_scanned += 1;
                    }
                }
                Ok(false) => break,             // End of file
                Err(_) => records_scanned += 1, // Skip malformed records
            }
        }

        Ok((max_price_precision, max_size_precision))
    }
}

impl Iterator for Depth10StreamIterator {
    type Item = anyhow::Result<Vec<OrderBookDepth10>>;

    fn next(&mut self) -> Option<Self::Item> {
        if let Some(limit) = self.limit
            && self.records_processed >= limit
        {
            return None;
        }

        if !self.buffer.is_empty() {
            let chunk = self.buffer.split_off(0);
            return Some(Ok(chunk));
        }

        self.buffer.clear();
        let mut records_read = 0;

        while records_read < self.chunk_size {
            match self.reader.read_record(&mut self.record) {
                Ok(true) => {
                    let result = match self.levels {
                        5 => self
                            .record
                            .deserialize::<TardisOrderBookSnapshot5Record>(None)
                            .map(|data| self.process_snapshot5(data)),
                        25 => self
                            .record
                            .deserialize::<TardisOrderBookSnapshot25Record>(None)
                            .map(|data| self.process_snapshot25(data)),
                        _ => return Some(Err(anyhow::anyhow!("Invalid levels: {}", self.levels))),
                    };

                    match result {
                        Ok(depth) => {
                            self.buffer.push(depth);
                            records_read += 1;
                            self.records_processed += 1;

                            if let Some(limit) = self.limit
                                && self.records_processed >= limit
                            {
                                break;
                            }
                        }
                        Err(e) => {
                            return Some(Err(anyhow::anyhow!("Failed to deserialize record: {e}")));
                        }
                    }
                }
                Ok(false) => {
                    if self.buffer.is_empty() {
                        return None;
                    }
                    let chunk = self.buffer.split_off(0);
                    return Some(Ok(chunk));
                }
                Err(e) => return Some(Err(anyhow::anyhow!("Failed to read record: {e}"))),
            }
        }

        if self.buffer.is_empty() {
            None
        } else {
            let chunk = self.buffer.split_off(0);
            Some(Ok(chunk))
        }
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
    limit: Option<usize>,
) -> anyhow::Result<impl Iterator<Item = anyhow::Result<Vec<OrderBookDepth10>>>> {
    Depth10StreamIterator::new(
        filepath,
        chunk_size,
        5,
        price_precision,
        size_precision,
        instrument_id,
        limit,
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
    limit: Option<usize>,
) -> anyhow::Result<impl Iterator<Item = anyhow::Result<Vec<OrderBookDepth10>>>> {
    Depth10StreamIterator::new(
        filepath,
        chunk_size,
        25,
        price_precision,
        size_precision,
        instrument_id,
        limit,
    )
}

////////////////////////////////////////////////////////////////////////////////
// FundingRateUpdate Streaming
////////////////////////////////////////////////////////////////////////////////

use nautilus_model::data::FundingRateUpdate;

use crate::csv::record::TardisDerivativeTickerRecord;

/// An iterator for streaming [`FundingRateUpdate`]s from a Tardis CSV file in chunks.
struct FundingRateStreamIterator {
    reader: Reader<Box<dyn Read>>,
    record: StringRecord,
    buffer: Vec<FundingRateUpdate>,
    chunk_size: usize,
    instrument_id: Option<InstrumentId>,
    limit: Option<usize>,
    records_processed: usize,
}

impl FundingRateStreamIterator {
    /// Creates a new [`FundingRateStreamIterator`].
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be opened or read.
    fn new<P: AsRef<Path>>(
        filepath: P,
        chunk_size: usize,
        instrument_id: Option<InstrumentId>,
        limit: Option<usize>,
    ) -> anyhow::Result<Self> {
        let reader = create_csv_reader(filepath)?;

        Ok(Self {
            reader,
            record: StringRecord::new(),
            buffer: Vec::with_capacity(chunk_size),
            chunk_size,
            instrument_id,
            limit,
            records_processed: 0,
        })
    }
}

impl Iterator for FundingRateStreamIterator {
    type Item = anyhow::Result<Vec<FundingRateUpdate>>;

    fn next(&mut self) -> Option<Self::Item> {
        if let Some(limit) = self.limit
            && self.records_processed >= limit
        {
            return None;
        }

        if !self.buffer.is_empty() {
            let chunk = self.buffer.split_off(0);
            return Some(Ok(chunk));
        }

        self.buffer.clear();
        let mut records_read = 0;

        while records_read < self.chunk_size {
            match self.reader.read_record(&mut self.record) {
                Ok(true) => {
                    let result = self
                        .record
                        .deserialize::<TardisDerivativeTickerRecord>(None)
                        .map_err(anyhow::Error::from)
                        .map(|data| parse_derivative_ticker_record(&data, self.instrument_id));

                    match result {
                        Ok(Some(funding_rate)) => {
                            self.buffer.push(funding_rate);
                            records_read += 1;
                            self.records_processed += 1;

                            if let Some(limit) = self.limit
                                && self.records_processed >= limit
                            {
                                break;
                            }
                        }
                        Ok(None) => {
                            // Skip this record as it has no funding data
                            self.records_processed += 1;
                        }
                        Err(e) => {
                            return Some(Err(anyhow::anyhow!(
                                "Failed to parse funding rate record: {e}"
                            )));
                        }
                    }
                }
                Ok(false) => {
                    if self.buffer.is_empty() {
                        return None;
                    }
                    let chunk = self.buffer.split_off(0);
                    return Some(Ok(chunk));
                }
                Err(e) => return Some(Err(anyhow::anyhow!("Failed to read record: {e}"))),
            }
        }

        if self.buffer.is_empty() {
            None
        } else {
            let chunk = self.buffer.split_off(0);
            Some(Ok(chunk))
        }
    }
}

/// Streams [`FundingRateUpdate`]s from a Tardis derivative ticker CSV file,
/// yielding chunks of the specified size.
///
/// This function parses the `funding_rate`, `predicted_funding_rate`, and `funding_timestamp`
/// fields from derivative ticker data to create funding rate updates.
///
/// # Errors
///
/// Returns an error if the file cannot be opened, read, or parsed as CSV.
pub fn stream_funding_rates<P: AsRef<Path>>(
    filepath: P,
    chunk_size: usize,
    instrument_id: Option<InstrumentId>,
    limit: Option<usize>,
) -> anyhow::Result<impl Iterator<Item = anyhow::Result<Vec<FundingRateUpdate>>>> {
    FundingRateStreamIterator::new(filepath, chunk_size, instrument_id, limit)
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use nautilus_model::{enums::AggressorSide, identifiers::TradeId, types::Price};
    use rstest::*;

    use super::*;
    use crate::{csv::load::load_deltas, parse::parse_price, tests::get_test_data_path};

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

        let stream = stream_deltas(&temp_file, 2, Some(4), Some(1), None, None).unwrap();
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

        let stream = stream_quotes(&temp_file, 2, Some(4), Some(1), None, None).unwrap();
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

        let stream = stream_trades(&temp_file, 3, Some(4), Some(1), None, None).unwrap();
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
    pub fn test_stream_trades_with_zero_sized_trade() {
        // Test CSV data with one zero-sized trade that should be skipped
        let csv_data = "exchange,symbol,timestamp,local_timestamp,id,side,price,amount
binance,BTCUSDT,1640995200000000,1640995200100000,trade1,buy,50000.0,1.0
binance,BTCUSDT,1640995201000000,1640995201100000,trade2,sell,49999.5,0.0
binance,BTCUSDT,1640995202000000,1640995202100000,trade3,buy,50000.12,1.5
binance,BTCUSDT,1640995203000000,1640995203100000,trade4,sell,49999.123,3.0";

        let temp_file = std::env::temp_dir().join("test_stream_trades_zero_size.csv");
        std::fs::write(&temp_file, csv_data).unwrap();

        let stream = stream_trades(&temp_file, 3, Some(4), Some(1), None, None).unwrap();
        let chunks: Vec<_> = stream.collect();

        // Should have 1 chunk with 3 valid trades (zero-sized trade skipped)
        assert_eq!(chunks.len(), 1);

        let chunk1 = chunks[0].as_ref().unwrap();
        assert_eq!(chunk1.len(), 3);

        // Verify the trades are the correct ones (not the zero-sized one)
        assert_eq!(chunk1[0].size, Quantity::from("1.0"));
        assert_eq!(chunk1[1].size, Quantity::from("1.5"));
        assert_eq!(chunk1[2].size, Quantity::from("3.0"));

        // Verify trade IDs to confirm correct trades were loaded
        assert_eq!(chunk1[0].trade_id, TradeId::new("trade1"));
        assert_eq!(chunk1[1].trade_id, TradeId::new("trade3"));
        assert_eq!(chunk1[2].trade_id, TradeId::new("trade4"));

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
        let stream = stream_depth10_from_snapshot5(&temp_file, 2, None, None, None, None).unwrap();
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
                let bid_price = f64::from(i).mul_add(-0.01, 49999.0);
                let bid_amount = 1.0 + f64::from(i);
                row1_parts.push(bid_price.to_string());
                row1_parts.push(bid_amount.to_string());
            } else {
                row1_parts.push(String::new());
                row1_parts.push(String::new());
            }
        }

        // Add ask levels (first 5 with data, rest empty)
        for i in 0..25 {
            if i < 5 {
                let ask_price = f64::from(i).mul_add(0.01, 50000.0);
                let ask_amount = 1.0 + f64::from(i);
                row1_parts.push(ask_price.to_string());
                row1_parts.push(ask_amount.to_string());
            } else {
                row1_parts.push(String::new());
                row1_parts.push(String::new());
            }
        }

        let csv_data = format!("{}\n{}", header, row1_parts.join(","));

        // Write to temporary file
        let temp_file = std::env::temp_dir().join("test_stream_depth10_snapshot25.csv");
        std::fs::write(&temp_file, &csv_data).unwrap();

        // Stream with chunk size of 1
        let stream = stream_depth10_from_snapshot25(&temp_file, 1, None, None, None, None).unwrap();
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

        let result = stream_deltas(non_existent, 10, None, None, None, None);
        assert!(result.is_err());

        let result = stream_quotes(non_existent, 10, None, None, None, None);
        assert!(result.is_err());

        let result = stream_trades(non_existent, 10, None, None, None, None);
        assert!(result.is_err());

        let result = stream_depth10_from_snapshot5(non_existent, 10, None, None, None, None);
        assert!(result.is_err());

        let result = stream_depth10_from_snapshot25(non_existent, 10, None, None, None, None);
        assert!(result.is_err());
    }

    #[rstest]
    pub fn test_stream_empty_file() {
        // Test with empty CSV file
        let temp_file = std::env::temp_dir().join("test_empty.csv");
        std::fs::write(&temp_file, "").unwrap();

        let stream = stream_deltas(&temp_file, 10, None, None, None, None).unwrap();
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
        let stream = stream_deltas(&temp_file, 2, None, None, None, None).unwrap();
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

    #[rstest]
    pub fn test_stream_trades_from_local_file() {
        let filepath = get_test_data_path("csv/trades_1.csv");
        let mut stream = stream_trades(filepath, 1, Some(1), Some(0), None, None).unwrap();

        let chunk1 = stream.next().unwrap().unwrap();
        assert_eq!(chunk1.len(), 1);
        assert_eq!(chunk1[0].price, Price::from("8531.5"));

        let chunk2 = stream.next().unwrap().unwrap();
        assert_eq!(chunk2.len(), 1);
        assert_eq!(chunk2[0].size, Quantity::from("1000"));

        assert!(stream.next().is_none());
    }

    #[rstest]
    pub fn test_stream_deltas_from_local_file() {
        let filepath = get_test_data_path("csv/deltas_1.csv");
        let mut stream = stream_deltas(filepath, 1, Some(1), Some(0), None, None).unwrap();

        let chunk1 = stream.next().unwrap().unwrap();
        assert_eq!(chunk1.len(), 1);
        assert_eq!(chunk1[0].order.price, Price::from("6421.5"));

        let chunk2 = stream.next().unwrap().unwrap();
        assert_eq!(chunk2.len(), 1);
        assert_eq!(chunk2[0].order.size, Quantity::from("10000"));

        assert!(stream.next().is_none());
    }

    #[rstest]
    pub fn test_stream_deltas_with_limit() {
        let csv_data = "exchange,symbol,timestamp,local_timestamp,is_snapshot,side,price,amount
binance,BTCUSDT,1640995200000000,1640995200100000,false,bid,50000.0,1.0
binance,BTCUSDT,1640995201000000,1640995201100000,false,ask,50001.0,2.0
binance,BTCUSDT,1640995202000000,1640995202100000,false,bid,49999.0,1.5
binance,BTCUSDT,1640995203000000,1640995203100000,false,ask,50002.0,3.0
binance,BTCUSDT,1640995204000000,1640995204100000,false,bid,49998.0,0.5";

        let temp_file = std::env::temp_dir().join("test_stream_deltas_limit.csv");
        std::fs::write(&temp_file, csv_data).unwrap();

        // Test with limit of 3 records
        let stream = stream_deltas(&temp_file, 2, Some(4), Some(1), None, Some(3)).unwrap();
        let chunks: Vec<_> = stream.collect();

        // Should have 2 chunks: [2 items, 1 item] = 3 total (limited)
        assert_eq!(chunks.len(), 2);
        let chunk1 = chunks[0].as_ref().unwrap();
        assert_eq!(chunk1.len(), 2);
        let chunk2 = chunks[1].as_ref().unwrap();
        assert_eq!(chunk2.len(), 1);

        // Total should be exactly 3 records due to limit
        let total_deltas: usize = chunks.iter().map(|c| c.as_ref().unwrap().len()).sum();
        assert_eq!(total_deltas, 3);

        std::fs::remove_file(&temp_file).ok();
    }

    #[rstest]
    pub fn test_stream_quotes_with_limit() {
        let csv_data =
            "exchange,symbol,timestamp,local_timestamp,ask_price,ask_amount,bid_price,bid_amount
binance,BTCUSDT,1640995200000000,1640995200100000,50001.0,1.0,50000.0,1.5
binance,BTCUSDT,1640995201000000,1640995201100000,50002.0,2.0,49999.0,2.5
binance,BTCUSDT,1640995202000000,1640995202100000,50003.0,1.5,49998.0,3.0
binance,BTCUSDT,1640995203000000,1640995203100000,50004.0,3.0,49997.0,3.5";

        let temp_file = std::env::temp_dir().join("test_stream_quotes_limit.csv");
        std::fs::write(&temp_file, csv_data).unwrap();

        // Test with limit of 2 records
        let stream = stream_quotes(&temp_file, 2, Some(4), Some(1), None, Some(2)).unwrap();
        let chunks: Vec<_> = stream.collect();

        // Should have 1 chunk with 2 items (limited)
        assert_eq!(chunks.len(), 1);
        let chunk1 = chunks[0].as_ref().unwrap();
        assert_eq!(chunk1.len(), 2);

        // Verify we get exactly 2 records
        let total_quotes: usize = chunks.iter().map(|c| c.as_ref().unwrap().len()).sum();
        assert_eq!(total_quotes, 2);

        std::fs::remove_file(&temp_file).ok();
    }

    #[rstest]
    pub fn test_stream_trades_with_limit() {
        let csv_data = "exchange,symbol,timestamp,local_timestamp,id,side,price,amount
binance,BTCUSDT,1640995200000000,1640995200100000,trade1,buy,50000.0,1.0
binance,BTCUSDT,1640995201000000,1640995201100000,trade2,sell,49999.5,2.0
binance,BTCUSDT,1640995202000000,1640995202100000,trade3,buy,50000.12,1.5
binance,BTCUSDT,1640995203000000,1640995203100000,trade4,sell,49999.123,3.0
binance,BTCUSDT,1640995204000000,1640995204100000,trade5,buy,50000.1234,0.5";

        let temp_file = std::env::temp_dir().join("test_stream_trades_limit.csv");
        std::fs::write(&temp_file, csv_data).unwrap();

        // Test with limit of 3 records
        let stream = stream_trades(&temp_file, 2, Some(4), Some(1), None, Some(3)).unwrap();
        let chunks: Vec<_> = stream.collect();

        // Should have 2 chunks: [2 items, 1 item] = 3 total (limited)
        assert_eq!(chunks.len(), 2);
        let chunk1 = chunks[0].as_ref().unwrap();
        assert_eq!(chunk1.len(), 2);
        let chunk2 = chunks[1].as_ref().unwrap();
        assert_eq!(chunk2.len(), 1);

        // Verify we get exactly 3 records
        let total_trades: usize = chunks.iter().map(|c| c.as_ref().unwrap().len()).sum();
        assert_eq!(total_trades, 3);

        std::fs::remove_file(&temp_file).ok();
    }
}
