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

use std::{fs, io::Cursor, path::Path, str::FromStr};

use arrow::record_batch::RecordBatch;
use bytes::Bytes;
use nautilus_core::UnixNanos;
use nautilus_model::{
    data::{
        Bar, BarSpecification, BarType, BookOrder, FundingRateUpdate, IndexPriceUpdate,
        MarkPriceUpdate, OrderBookDelta, TradeTick,
    },
    enums::{
        AggregationSource, AggressorSide, BarAggregation, BookAction, OrderSide, PriceType,
        RecordFlag,
    },
    identifiers::{InstrumentId, TradeId},
};
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
use rust_decimal::Decimal;

use crate::{
    enums::{CryptoHFTDataExchange, GapPolicy},
    parse::{
        column, instrument_id, parse_price, parse_quantity, parse_timestamp,
        parse_ts_init_or_event, required_column, rescale_price, rescale_quantity,
        timestamp_to_unix_nanos, value_as_bool, value_as_i64, value_as_string, value_as_u64,
    },
    types::{CryptoHFTDataLiquidation, CryptoHFTDataOpenInterest},
};

const DEFAULT_BATCH_SIZE: usize = 65_536;

/// Parsed CHD mark/index/funding records.
#[derive(Clone, Debug, Default)]
pub struct CryptoHFTDataPriceUpdates {
    /// Mark price updates.
    pub mark_prices: Vec<MarkPriceUpdate>,
    /// Index price updates.
    pub index_prices: Vec<IndexPriceUpdate>,
    /// Funding rate updates.
    pub funding_rates: Vec<FundingRateUpdate>,
}

/// CHD parquet loader.
#[derive(Clone, Debug)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.cryptohftdata",
        from_py_object
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.cryptohftdata")
)]
pub struct CryptoHFTDataDataLoader {
    batch_size: usize,
    gap_policy: GapPolicy,
}

impl CryptoHFTDataDataLoader {
    /// Creates a new [`CryptoHFTDataDataLoader`].
    #[must_use]
    pub const fn new(batch_size: Option<usize>, gap_policy: Option<GapPolicy>) -> Self {
        Self {
            batch_size: match batch_size {
                Some(size) => size,
                None => DEFAULT_BATCH_SIZE,
            },
            gap_policy: match gap_policy {
                Some(policy) => policy,
                None => GapPolicy::Error,
            },
        }
    }

    /// Reads CHD parquet or parquet.zst bytes into Arrow record batches.
    ///
    /// # Errors
    ///
    /// Returns an error when zstd decompression or Parquet decoding fails.
    pub fn record_batches_from_bytes(&self, bytes: &[u8]) -> anyhow::Result<Vec<RecordBatch>> {
        let parquet_bytes = if bytes.starts_with(b"PAR1") {
            Bytes::copy_from_slice(bytes)
        } else {
            let decompressed = zstd::stream::decode_all(Cursor::new(bytes))?;
            Bytes::from(decompressed)
        };

        let builder = ParquetRecordBatchReaderBuilder::try_new(parquet_bytes)?
            .with_batch_size(self.batch_size);
        let reader = builder.build()?;
        let mut batches = Vec::new();
        for batch in reader {
            batches.push(batch?);
        }
        Ok(batches)
    }

    /// Reads CHD parquet or parquet.zst file into Arrow record batches.
    ///
    /// # Errors
    ///
    /// Returns an error when reading or decoding fails.
    pub fn record_batches_from_path(&self, path: &Path) -> anyhow::Result<Vec<RecordBatch>> {
        let bytes = fs::read(path)?;
        self.record_batches_from_bytes(&bytes)
    }

    /// Loads CHD trades from Arrow record batches.
    ///
    /// # Errors
    ///
    /// Returns an error when required columns are missing or invalid.
    pub fn load_trades(
        &self,
        batches: &[RecordBatch],
        exchange: CryptoHFTDataExchange,
        raw_symbol: &str,
        instrument_id_override: Option<InstrumentId>,
    ) -> anyhow::Result<Vec<TradeTick>> {
        let instrument_id =
            instrument_id_override.unwrap_or_else(|| instrument_id(exchange, raw_symbol));
        let mut trades = Vec::new();
        let mut max_price_precision = 0;
        let mut max_size_precision = 0;

        for batch in batches {
            let price_col = required_column(batch, &["price"])?;
            let size_col = required_column(batch, &["quantity", "size", "qty"])?;
            let trade_id_col = column(batch, &["trade_id", "id"]);
            let side_col = column(batch, &["side", "aggressor_side"]);
            let buyer_maker_col = column(batch, &["is_buyer_maker"]);

            for row in 0..batch.num_rows() {
                let price = parse_price(price_col, row)?;
                let size = parse_quantity(size_col, row)?;
                if size.is_zero() {
                    log::debug!(
                        "Skipping CHD trade with zero quantity for {instrument_id} at row {row}"
                    );
                    continue;
                }

                max_price_precision = max_price_precision.max(price.precision);
                max_size_precision = max_size_precision.max(size.precision);

                let ts_event = parse_timestamp(
                    batch,
                    row,
                    &["trade_time", "event_time", "timestamp", "ts_event"],
                )?;
                let ts_init = parse_ts_init_or_event(batch, row, ts_event);
                let trade_id = trade_id_col
                    .and_then(|col| value_as_string(col, row))
                    .unwrap_or_else(|| format!("{}", trades.len()));
                let trade_id = TradeId::new(truncate_trade_id(&trade_id));
                let aggressor_side = parse_aggressor_side(side_col, buyer_maker_col, row);

                trades.push(TradeTick::new(
                    instrument_id,
                    price,
                    size,
                    aggressor_side,
                    trade_id,
                    ts_event,
                    ts_init,
                ));
            }
        }

        for trade in &mut trades {
            trade.price = rescale_price(trade.price, max_price_precision);
            trade.size = rescale_quantity(trade.size, max_size_precision);
        }

        Ok(trades)
    }

    /// Loads CHD order book rows as Nautilus order book deltas.
    ///
    /// # Errors
    ///
    /// Returns an error when required columns are missing, invalid, or a sequence
    /// gap is detected with `GapPolicy::Error`.
    pub fn load_order_book_deltas(
        &self,
        batches: &[RecordBatch],
        exchange: CryptoHFTDataExchange,
        raw_symbol: &str,
        instrument_id_override: Option<InstrumentId>,
    ) -> anyhow::Result<Vec<OrderBookDelta>> {
        let instrument_id =
            instrument_id_override.unwrap_or_else(|| instrument_id(exchange, raw_symbol));
        let mut deltas: Vec<OrderBookDelta> = Vec::new();
        let mut max_price_precision = 0;
        let mut max_size_precision = 0;
        let mut last_event: Option<(UnixNanos, u64)> = None;
        let mut last_sequence: Option<u64> = None;
        let mut snapshot_open = false;

        for batch in batches {
            let side_col = required_column(batch, &["side"])?;
            let price_col = required_column(batch, &["price"])?;
            let size_col = required_column(batch, &["quantity", "size", "amount"])?;
            let event_type_col = column(batch, &["event_type", "type"]);
            let first_update_col = column(batch, &["first_update_id"]);
            let final_update_col =
                column(batch, &["final_update_id", "last_update_id", "sequence"]);
            let prev_update_col = column(batch, &["prev_final_update_id"]);

            for row in 0..batch.num_rows() {
                let ts_event = parse_timestamp(
                    batch,
                    row,
                    &["event_time", "transaction_time", "timestamp", "ts_event"],
                )?;
                let sequence = final_update_col
                    .and_then(|col| value_as_u64(col, row))
                    .unwrap_or(deltas.len() as u64);

                if self.has_sequence_gap(
                    prev_update_col,
                    first_update_col,
                    row,
                    last_sequence,
                    sequence,
                ) {
                    match self.gap_policy {
                        GapPolicy::Error => anyhow::bail!(
                            "CHD order book sequence gap for {instrument_id}: previous={last_sequence:?}, row_sequence={sequence}"
                        ),
                        GapPolicy::Warn => log::warn!(
                            "CHD order book sequence gap for {instrument_id}: previous={last_sequence:?}, row_sequence={sequence}"
                        ),
                        GapPolicy::Skip => {
                            last_sequence = Some(sequence);
                            continue;
                        }
                    }
                }

                if let Some((last_ts, last_seq)) = last_event
                    && (last_ts != ts_event || last_seq != sequence)
                    && let Some(last_delta) = deltas.last_mut()
                {
                    last_delta.flags |= RecordFlag::F_LAST as u8;
                }
                last_event = Some((ts_event, sequence));
                last_sequence = Some(sequence);

                let side = parse_order_side(side_col, row)?;
                let price = parse_price(price_col, row)?;
                let size = parse_quantity(size_col, row)?;
                max_price_precision = max_price_precision.max(price.precision);
                max_size_precision = max_size_precision.max(size.precision);
                let ts_init = parse_ts_init_or_event(batch, row, ts_event);
                let is_snapshot = is_snapshot_row(event_type_col, row);

                if is_snapshot && !snapshot_open {
                    deltas.push(OrderBookDelta::clear(
                        instrument_id,
                        sequence,
                        ts_event,
                        ts_init,
                    ));
                    snapshot_open = true;
                } else if !is_snapshot {
                    snapshot_open = false;
                }

                let action = if is_snapshot {
                    BookAction::Add
                } else if size.is_zero() {
                    BookAction::Delete
                } else {
                    BookAction::Update
                };
                let flags = if is_snapshot {
                    RecordFlag::F_SNAPSHOT as u8
                } else {
                    0
                };
                let order = BookOrder::new(side, price, size, 0);
                deltas.push(OrderBookDelta::new(
                    instrument_id,
                    action,
                    order,
                    flags,
                    sequence,
                    ts_event,
                    ts_init,
                ));
            }
        }

        if let Some(last_delta) = deltas.last_mut() {
            last_delta.flags |= RecordFlag::F_LAST as u8;
        }

        for delta in &mut deltas {
            if delta.action != BookAction::Clear {
                delta.order.price = rescale_price(delta.order.price, max_price_precision);
                delta.order.size = rescale_quantity(delta.order.size, max_size_precision);
            }
        }

        Ok(deltas)
    }

    /// Loads CHD klines as 1-minute externally aggregated last-price bars.
    ///
    /// # Errors
    ///
    /// Returns an error when required columns are missing or invalid.
    pub fn load_bars(
        &self,
        batches: &[RecordBatch],
        exchange: CryptoHFTDataExchange,
        raw_symbol: &str,
        instrument_id_override: Option<InstrumentId>,
    ) -> anyhow::Result<Vec<Bar>> {
        let instrument_id =
            instrument_id_override.unwrap_or_else(|| instrument_id(exchange, raw_symbol));
        let bar_type = BarType::new(
            instrument_id,
            BarSpecification::new(1, BarAggregation::Minute, PriceType::Last),
            AggregationSource::External,
        );
        let mut bars = Vec::new();
        let mut max_price_precision = 0;
        let mut max_size_precision = 0;

        for batch in batches {
            let open_col = required_column(batch, &["open"])?;
            let high_col = required_column(batch, &["high"])?;
            let low_col = required_column(batch, &["low"])?;
            let close_col = required_column(batch, &["close"])?;
            let volume_col = required_column(batch, &["volume", "quantity"])?;

            for row in 0..batch.num_rows() {
                let open = parse_price(open_col, row)?;
                let high = parse_price(high_col, row)?;
                let low = parse_price(low_col, row)?;
                let close = parse_price(close_col, row)?;
                let volume = parse_quantity(volume_col, row)?;
                max_price_precision = max_price_precision
                    .max(open.precision)
                    .max(high.precision)
                    .max(low.precision)
                    .max(close.precision);
                max_size_precision = max_size_precision.max(volume.precision);
                let ts_event = parse_timestamp(
                    batch,
                    row,
                    &["close_time", "open_time", "timestamp", "ts_event"],
                )?;
                let ts_init = parse_ts_init_or_event(batch, row, ts_event);
                bars.push(Bar::new(
                    bar_type, open, high, low, close, volume, ts_event, ts_init,
                ));
            }
        }

        for bar in &mut bars {
            bar.open = rescale_price(bar.open, max_price_precision);
            bar.high = rescale_price(bar.high, max_price_precision);
            bar.low = rescale_price(bar.low, max_price_precision);
            bar.close = rescale_price(bar.close, max_price_precision);
            bar.volume = rescale_quantity(bar.volume, max_size_precision);
        }

        Ok(bars)
    }

    /// Loads CHD mark, index and funding updates.
    ///
    /// # Errors
    ///
    /// Returns an error when required price columns are missing or invalid.
    pub fn load_price_updates(
        &self,
        batches: &[RecordBatch],
        exchange: CryptoHFTDataExchange,
        raw_symbol: &str,
        instrument_id_override: Option<InstrumentId>,
    ) -> anyhow::Result<CryptoHFTDataPriceUpdates> {
        let instrument_id =
            instrument_id_override.unwrap_or_else(|| instrument_id(exchange, raw_symbol));
        let mut result = CryptoHFTDataPriceUpdates::default();

        for batch in batches {
            let mark_col = column(batch, &["mark_price"]);
            let index_col = column(batch, &["index_price"]);
            let funding_col = column(batch, &["funding_rate"]);
            let next_funding_col = column(batch, &["next_funding_time"]);

            for row in 0..batch.num_rows() {
                let ts_event =
                    parse_timestamp(batch, row, &["event_time", "timestamp", "ts_event"])?;
                let ts_init = parse_ts_init_or_event(batch, row, ts_event);

                if let Some(col) = mark_col {
                    result.mark_prices.push(MarkPriceUpdate::new(
                        instrument_id,
                        parse_price(col, row)?,
                        ts_event,
                        ts_init,
                    ));
                }
                if let Some(col) = index_col {
                    result.index_prices.push(IndexPriceUpdate::new(
                        instrument_id,
                        parse_price(col, row)?,
                        ts_event,
                        ts_init,
                    ));
                }
                if let Some(col) = funding_col
                    && let Some(rate) = value_as_string(col, row)
                    && let Ok(rate) = Decimal::from_str(&rate)
                {
                    let next_funding_ns = next_funding_col
                        .and_then(|col| value_as_i64(col, row))
                        .map(timestamp_to_unix_nanos);
                    result.funding_rates.push(FundingRateUpdate::new(
                        instrument_id,
                        rate,
                        Some(480),
                        next_funding_ns,
                        ts_event,
                        ts_init,
                    ));
                }
            }
        }

        Ok(result)
    }

    /// Loads CHD open interest snapshots.
    ///
    /// # Errors
    ///
    /// Returns an error when required columns are missing or invalid.
    pub fn load_open_interest(
        &self,
        batches: &[RecordBatch],
        exchange: CryptoHFTDataExchange,
        raw_symbol: &str,
        instrument_id_override: Option<InstrumentId>,
    ) -> anyhow::Result<Vec<CryptoHFTDataOpenInterest>> {
        let instrument_id =
            instrument_id_override.unwrap_or_else(|| instrument_id(exchange, raw_symbol));
        let mut updates = Vec::new();

        for batch in batches {
            let open_interest_col =
                required_column(batch, &["open_interest", "sum_open_interest"])?;
            let value_col = column(batch, &["open_interest_value", "sum_open_interest_value"]);

            for row in 0..batch.num_rows() {
                let open_interest = parse_quantity(open_interest_col, row)?;
                let open_interest_value = value_col
                    .and_then(|col| value_as_string(col, row))
                    .unwrap_or_default();
                let ts_event =
                    parse_timestamp(batch, row, &["timestamp", "event_time", "ts_event"])?;
                let ts_init = parse_ts_init_or_event(batch, row, ts_event);
                updates.push(CryptoHFTDataOpenInterest {
                    instrument_id,
                    open_interest,
                    open_interest_value,
                    ts_event,
                    ts_init,
                });
            }
        }

        Ok(updates)
    }

    /// Loads CHD liquidation events.
    ///
    /// # Errors
    ///
    /// Returns an error when required columns are missing or invalid.
    pub fn load_liquidations(
        &self,
        batches: &[RecordBatch],
        exchange: CryptoHFTDataExchange,
        raw_symbol: &str,
        instrument_id_override: Option<InstrumentId>,
    ) -> anyhow::Result<Vec<CryptoHFTDataLiquidation>> {
        let instrument_id =
            instrument_id_override.unwrap_or_else(|| instrument_id(exchange, raw_symbol));
        let mut liquidations = Vec::new();

        for batch in batches {
            let price_col = required_column(batch, &["price"])?;
            let quantity_col = required_column(batch, &["quantity", "size"])?;
            let side_col = required_column(batch, &["side"])?;
            let order_id_col = column(batch, &["order_id", "id"]);

            for row in 0..batch.num_rows() {
                let ts_event =
                    parse_timestamp(batch, row, &["event_time", "timestamp", "ts_event"])?;
                let ts_init = parse_ts_init_or_event(batch, row, ts_event);
                let order_id = order_id_col
                    .and_then(|col| value_as_string(col, row))
                    .unwrap_or_else(|| liquidations.len().to_string());
                liquidations.push(CryptoHFTDataLiquidation {
                    instrument_id,
                    side: value_as_string(side_col, row).unwrap_or_default(),
                    price: parse_price(price_col, row)?,
                    quantity: parse_quantity(quantity_col, row)?,
                    order_id,
                    ts_event,
                    ts_init,
                });
            }
        }

        Ok(liquidations)
    }

    fn has_sequence_gap(
        &self,
        prev_update_col: Option<&arrow::array::ArrayRef>,
        first_update_col: Option<&arrow::array::ArrayRef>,
        row: usize,
        last_sequence: Option<u64>,
        sequence: u64,
    ) -> bool {
        let Some(last_sequence) = last_sequence else {
            return false;
        };
        if sequence == last_sequence {
            return false;
        }

        if let Some(prev) = prev_update_col.and_then(|col| value_as_u64(col, row)) {
            return prev != 0 && prev != last_sequence;
        }

        if let Some(first) = first_update_col.and_then(|col| value_as_u64(col, row)) {
            return first > last_sequence.saturating_add(1);
        }

        false
    }
}

impl Default for CryptoHFTDataDataLoader {
    fn default() -> Self {
        Self::new(None, None)
    }
}

fn parse_order_side(col: &arrow::array::ArrayRef, row: usize) -> anyhow::Result<OrderSide> {
    match value_as_string(col, row)
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "bid" | "buy" | "b" => Ok(OrderSide::Buy),
        "ask" | "sell" | "a" => Ok(OrderSide::Sell),
        value => anyhow::bail!("invalid CHD order side '{value}' at row {row}"),
    }
}

fn parse_aggressor_side(
    side_col: Option<&arrow::array::ArrayRef>,
    buyer_maker_col: Option<&arrow::array::ArrayRef>,
    row: usize,
) -> AggressorSide {
    if let Some(is_buyer_maker) = buyer_maker_col.and_then(|col| value_as_bool(col, row)) {
        // If buyer is maker then the seller removed liquidity.
        return if is_buyer_maker {
            AggressorSide::Seller
        } else {
            AggressorSide::Buyer
        };
    }

    match side_col
        .and_then(|col| value_as_string(col, row))
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "buy" | "bid" | "buyer" => AggressorSide::Buyer,
        "sell" | "ask" | "seller" => AggressorSide::Seller,
        _ => AggressorSide::NoAggressor,
    }
}

fn is_snapshot_row(event_type_col: Option<&arrow::array::ArrayRef>, row: usize) -> bool {
    event_type_col
        .and_then(|col| value_as_string(col, row))
        .is_some_and(|value| value.to_ascii_lowercase().contains("snapshot"))
}

fn truncate_trade_id(value: &str) -> &str {
    if value.len() <= 36 {
        value
    } else {
        &value[..36]
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use arrow::{
        array::{BooleanArray, Int64Array, StringArray},
        datatypes::{Field, Schema},
    };

    use super::*;

    fn batch(columns: Vec<(&str, Arc<dyn arrow::array::Array>)>) -> RecordBatch {
        let fields = columns
            .iter()
            .map(|(name, array)| Field::new(*name, array.data_type().clone(), true))
            .collect::<Vec<_>>();
        let arrays = columns.into_iter().map(|(_, array)| array).collect();
        RecordBatch::try_new(Arc::new(Schema::new(fields)), arrays).unwrap()
    }

    #[test]
    fn trades_map_binance_buyer_maker_to_seller_aggressor() {
        let batch = batch(vec![
            ("price", Arc::new(StringArray::from(vec!["100.10"]))),
            ("quantity", Arc::new(StringArray::from(vec!["0.25"]))),
            ("trade_id", Arc::new(StringArray::from(vec!["123"]))),
            (
                "trade_time",
                Arc::new(Int64Array::from(vec![1_700_000_000_000i64])),
            ),
            (
                "received_time",
                Arc::new(Int64Array::from(vec![1_700_000_000_001i64])),
            ),
            ("is_buyer_maker", Arc::new(BooleanArray::from(vec![true]))),
        ]);
        let loader = CryptoHFTDataDataLoader::default();
        let trades = loader
            .load_trades(
                &[batch],
                CryptoHFTDataExchange::BinanceFutures,
                "BTCUSDT",
                None,
            )
            .unwrap();

        assert_eq!(trades.len(), 1);
        assert_eq!(trades[0].aggressor_side, AggressorSide::Seller);
    }

    #[test]
    fn trades_skip_zero_quantity_rows() {
        let batch = batch(vec![
            (
                "price",
                Arc::new(StringArray::from(vec!["100.10", "100.20"])),
            ),
            ("quantity", Arc::new(StringArray::from(vec!["0", "0.25"]))),
            (
                "trade_id",
                Arc::new(StringArray::from(vec!["zero", "valid"])),
            ),
            (
                "trade_time",
                Arc::new(Int64Array::from(vec![
                    1_700_000_000_000i64,
                    1_700_000_000_001i64,
                ])),
            ),
            (
                "received_time",
                Arc::new(Int64Array::from(vec![
                    1_700_000_000_000i64,
                    1_700_000_000_001i64,
                ])),
            ),
        ]);
        let loader = CryptoHFTDataDataLoader::default();
        let trades = loader
            .load_trades(
                &[batch],
                CryptoHFTDataExchange::BinanceSpot,
                "1INCHUSDT",
                None,
            )
            .unwrap();

        assert_eq!(trades.len(), 1);
        assert_eq!(trades[0].trade_id.to_string(), "valid");
    }

    #[test]
    fn orderbook_quantity_zero_maps_to_delete() {
        let batch = batch(vec![
            ("side", Arc::new(StringArray::from(vec!["ask"]))),
            ("price", Arc::new(StringArray::from(vec!["100.10"]))),
            ("quantity", Arc::new(StringArray::from(vec!["0"]))),
            (
                "event_time",
                Arc::new(Int64Array::from(vec![1_700_000_000_000i64])),
            ),
            ("final_update_id", Arc::new(Int64Array::from(vec![7i64]))),
        ]);
        let loader = CryptoHFTDataDataLoader::default();
        let deltas = loader
            .load_order_book_deltas(
                &[batch],
                CryptoHFTDataExchange::BinanceFutures,
                "BTCUSDT",
                None,
            )
            .unwrap();

        assert_eq!(deltas.len(), 1);
        assert_eq!(deltas[0].action, BookAction::Delete);
        assert_eq!(deltas[0].flags, RecordFlag::F_LAST as u8);
    }

    #[test]
    fn orderbook_repeated_sequence_rows_do_not_gap() {
        let batch = batch(vec![
            ("side", Arc::new(StringArray::from(vec!["bid", "ask"]))),
            (
                "price",
                Arc::new(StringArray::from(vec!["100.10", "100.20"])),
            ),
            ("quantity", Arc::new(StringArray::from(vec!["1", "2"]))),
            (
                "event_time",
                Arc::new(Int64Array::from(vec![
                    1_700_000_000_000i64,
                    1_700_000_000_000i64,
                ])),
            ),
            (
                "prev_final_update_id",
                Arc::new(Int64Array::from(vec![6i64, 6i64])),
            ),
            (
                "final_update_id",
                Arc::new(Int64Array::from(vec![7i64, 7i64])),
            ),
        ]);
        let loader = CryptoHFTDataDataLoader::default();
        let deltas = loader
            .load_order_book_deltas(
                &[batch],
                CryptoHFTDataExchange::BinanceFutures,
                "BTCUSDT",
                None,
            )
            .unwrap();

        assert_eq!(deltas.len(), 2);
        assert!(
            deltas
                .last()
                .is_some_and(|delta| delta.flags & RecordFlag::F_LAST as u8 != 0)
        );
    }
}
