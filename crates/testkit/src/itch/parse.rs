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

//! ITCH 5.0 message to [`OrderBookDelta`] conversion.

use std::{io::Read, path::Path};

use ahash::AHashMap;
use nautilus_core::UnixNanos;
use nautilus_model::{
    data::{delta::OrderBookDelta, order::BookOrder},
    enums::{BookAction, OrderSide, RecordFlag},
    identifiers::InstrumentId,
    types::{Price, Quantity},
};

/// Price precision for US equities (4 decimal places, $0.0001 increments).
const PRICE_PRECISION: u8 = 4;

/// Size precision for US equities (whole shares).
const SIZE_PRECISION: u8 = 0;

#[derive(Debug)]
struct OrderState {
    price: Price,
    size: u32,
    side: OrderSide,
}

/// Converts a stream of ITCH 5.0 messages into [`OrderBookDelta`] events
/// for a single instrument.
///
/// Maintains internal order state to compute remaining sizes after partial
/// executions and cancellations.
#[derive(Debug)]
pub struct ItchParser {
    instrument_id: InstrumentId,
    target_locate: Option<u16>,
    target_stock: String,
    base_ns: u64,
    orders: AHashMap<u64, OrderState>,
    sequence: u64,
}

impl ItchParser {
    /// Creates a new [`ItchParser`] for the given instrument.
    ///
    /// # Arguments
    ///
    /// - `instrument_id` - The NautilusTrader instrument ID for output deltas.
    /// - `stock` - The ITCH stock symbol to filter for (e.g., "AAPL").
    /// - `base_ns` - Base UNIX nanoseconds for midnight of the trading day
    ///   (ITCH timestamps are nanoseconds since midnight).
    pub fn new(instrument_id: InstrumentId, stock: &str, base_ns: u64) -> Self {
        Self {
            instrument_id,
            target_locate: None,
            target_stock: stock.to_string(),
            base_ns,
            orders: AHashMap::new(),
            sequence: 0,
        }
    }

    /// Parses all ITCH messages from a gzip-compressed file and returns
    /// the filtered [`OrderBookDelta`] events.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be opened or contains invalid data.
    pub fn parse_gzip_file(&mut self, path: &Path) -> anyhow::Result<Vec<OrderBookDelta>> {
        let stream = itchy::MessageStream::from_gzip(path)
            .map_err(|e| anyhow::anyhow!("Failed to open ITCH gzip: {e}"))?;
        self.parse_stream(stream)
    }

    /// Parses all ITCH messages from a reader and returns filtered deltas.
    ///
    /// # Errors
    ///
    /// Returns an error if the stream contains invalid data.
    pub fn parse_reader<R: Read>(&mut self, reader: R) -> anyhow::Result<Vec<OrderBookDelta>> {
        let stream = itchy::MessageStream::from_reader(reader);
        self.parse_stream(stream)
    }

    fn parse_stream<R: Read>(
        &mut self,
        stream: itchy::MessageStream<R>,
    ) -> anyhow::Result<Vec<OrderBookDelta>> {
        let mut deltas = Vec::new();

        for result in stream {
            let msg = result.map_err(|e| anyhow::anyhow!("ITCH parse error: {e}"))?;

            // Handle feed-level messages before stock locate filtering
            match msg.body {
                itchy::Body::StockDirectory(ref dir) => {
                    let symbol = dir.stock.trim();
                    if symbol == self.target_stock {
                        self.target_locate = Some(msg.stock_locate);
                    }
                    continue;
                }
                itchy::Body::SystemEvent {
                    event: itchy::EventCode::EndOfMessages,
                } => {
                    let ts = UnixNanos::from(self.base_ns + msg.timestamp);
                    self.handle_end_of_messages(ts, &mut deltas);
                    continue;
                }
                _ => {}
            }

            // Filter by target stock locate
            let Some(locate) = self.target_locate else {
                continue;
            };

            if msg.stock_locate != locate {
                continue;
            }

            let ts = UnixNanos::from(self.base_ns + msg.timestamp);

            match msg.body {
                itchy::Body::AddOrder(ref add) => {
                    self.handle_add_order(add, ts, &mut deltas);
                }
                itchy::Body::DeleteOrder { reference } => {
                    self.handle_delete_order(reference, ts, &mut deltas);
                }
                itchy::Body::OrderCancelled {
                    reference,
                    cancelled,
                } => {
                    self.handle_cancel(reference, cancelled, ts, &mut deltas);
                }
                itchy::Body::OrderExecuted {
                    reference,
                    executed,
                    ..
                } => {
                    self.handle_execution(reference, executed, ts, &mut deltas);
                }
                itchy::Body::OrderExecutedWithPrice {
                    reference,
                    executed,
                    ..
                } => {
                    self.handle_execution(reference, executed, ts, &mut deltas);
                }
                itchy::Body::ReplaceOrder(ref replace) => {
                    self.handle_replace(replace, ts, &mut deltas);
                }
                _ => {}
            }
        }

        // Set F_LAST on the final delta
        if let Some(last) = deltas.last_mut() {
            last.flags |= RecordFlag::F_LAST as u8;
        }

        Ok(deltas)
    }

    fn handle_add_order(
        &mut self,
        add: &itchy::AddOrder,
        ts: UnixNanos,
        deltas: &mut Vec<OrderBookDelta>,
    ) {
        let side = convert_side(add.side);
        let price = convert_price(add.price);

        self.orders.insert(
            add.reference,
            OrderState {
                price,
                size: add.shares,
                side,
            },
        );

        self.sequence += 1;
        let order = BookOrder::new(
            side,
            price,
            Quantity::new(f64::from(add.shares), SIZE_PRECISION),
            add.reference,
        );
        deltas.push(OrderBookDelta::new(
            self.instrument_id,
            BookAction::Add,
            order,
            RecordFlag::F_LAST as u8,
            self.sequence,
            ts,
            ts,
        ));
    }

    fn handle_delete_order(
        &mut self,
        reference: u64,
        ts: UnixNanos,
        deltas: &mut Vec<OrderBookDelta>,
    ) {
        if let Some(state) = self.orders.remove(&reference) {
            self.sequence += 1;
            let order = BookOrder::new(
                state.side,
                state.price,
                Quantity::new(0.0, SIZE_PRECISION),
                reference,
            );
            deltas.push(OrderBookDelta::new(
                self.instrument_id,
                BookAction::Delete,
                order,
                RecordFlag::F_LAST as u8,
                self.sequence,
                ts,
                ts,
            ));
        }
    }

    fn handle_cancel(
        &mut self,
        reference: u64,
        cancelled: u32,
        ts: UnixNanos,
        deltas: &mut Vec<OrderBookDelta>,
    ) {
        if let Some(state) = self.orders.get_mut(&reference) {
            state.size = state.size.saturating_sub(cancelled);

            if state.size == 0 {
                // Full cancel
                let state = self.orders.remove(&reference).unwrap();
                self.sequence += 1;
                let order = BookOrder::new(
                    state.side,
                    state.price,
                    Quantity::new(0.0, SIZE_PRECISION),
                    reference,
                );
                deltas.push(OrderBookDelta::new(
                    self.instrument_id,
                    BookAction::Delete,
                    order,
                    RecordFlag::F_LAST as u8,
                    self.sequence,
                    ts,
                    ts,
                ));
            } else {
                // Partial cancel
                self.sequence += 1;
                let order = BookOrder::new(
                    state.side,
                    state.price,
                    Quantity::new(f64::from(state.size), SIZE_PRECISION),
                    reference,
                );
                deltas.push(OrderBookDelta::new(
                    self.instrument_id,
                    BookAction::Update,
                    order,
                    RecordFlag::F_LAST as u8,
                    self.sequence,
                    ts,
                    ts,
                ));
            }
        }
    }

    fn handle_execution(
        &mut self,
        reference: u64,
        executed: u32,
        ts: UnixNanos,
        deltas: &mut Vec<OrderBookDelta>,
    ) {
        if let Some(state) = self.orders.get_mut(&reference) {
            state.size = state.size.saturating_sub(executed);

            if state.size == 0 {
                // Fully consumed
                let state = self.orders.remove(&reference).unwrap();
                self.sequence += 1;
                let order = BookOrder::new(
                    state.side,
                    state.price,
                    Quantity::new(0.0, SIZE_PRECISION),
                    reference,
                );
                deltas.push(OrderBookDelta::new(
                    self.instrument_id,
                    BookAction::Delete,
                    order,
                    RecordFlag::F_LAST as u8,
                    self.sequence,
                    ts,
                    ts,
                ));
            } else {
                // Partial execution
                self.sequence += 1;
                let order = BookOrder::new(
                    state.side,
                    state.price,
                    Quantity::new(f64::from(state.size), SIZE_PRECISION),
                    reference,
                );
                deltas.push(OrderBookDelta::new(
                    self.instrument_id,
                    BookAction::Update,
                    order,
                    RecordFlag::F_LAST as u8,
                    self.sequence,
                    ts,
                    ts,
                ));
            }
        }
    }

    fn handle_replace(
        &mut self,
        replace: &itchy::ReplaceOrder,
        ts: UnixNanos,
        deltas: &mut Vec<OrderBookDelta>,
    ) {
        // Delete old order
        if let Some(old_state) = self.orders.remove(&replace.old_reference) {
            self.sequence += 1;
            let old_order = BookOrder::new(
                old_state.side,
                old_state.price,
                Quantity::new(0.0, SIZE_PRECISION),
                replace.old_reference,
            );
            deltas.push(OrderBookDelta::new(
                self.instrument_id,
                BookAction::Delete,
                old_order,
                0, // Not the last in this event group
                self.sequence,
                ts,
                ts,
            ));

            // Add new order (inherits side from old order)
            let new_price = convert_price(replace.price);
            self.orders.insert(
                replace.new_reference,
                OrderState {
                    price: new_price,
                    size: replace.shares,
                    side: old_state.side,
                },
            );

            self.sequence += 1;
            let new_order = BookOrder::new(
                old_state.side,
                new_price,
                Quantity::new(f64::from(replace.shares), SIZE_PRECISION),
                replace.new_reference,
            );
            deltas.push(OrderBookDelta::new(
                self.instrument_id,
                BookAction::Add,
                new_order,
                RecordFlag::F_LAST as u8,
                self.sequence,
                ts,
                ts,
            ));
        }
    }

    fn handle_end_of_messages(&mut self, ts: UnixNanos, deltas: &mut Vec<OrderBookDelta>) {
        self.sequence += 1;
        deltas.push(OrderBookDelta::clear(
            self.instrument_id,
            self.sequence,
            ts,
            ts,
        ));
    }
}

fn convert_side(side: itchy::Side) -> OrderSide {
    match side {
        itchy::Side::Buy => OrderSide::Buy,
        itchy::Side::Sell => OrderSide::Sell,
    }
}

fn convert_price(price: itchy::Price4) -> Price {
    Price::new(f64::from(price.raw()) / 10_000.0, PRICE_PRECISION)
}

#[cfg(test)]
mod tests {
    use std::{fs, fs::File, path::PathBuf, sync::Arc};

    use nautilus_model::data::OrderBookDelta;
    use nautilus_serialization::arrow::{ArrowSchemaProvider, EncodeToRecordBatch};
    use parquet::{arrow::ArrowWriter, file::properties::WriterProperties};
    use rstest::rstest;

    use super::*;

    const AAPL_ID: &str = "AAPL.XNAS";

    fn setup_parser(base_ns: u64) -> ItchParser {
        ItchParser::new(InstrumentId::from(AAPL_ID), "AAPL", base_ns)
    }

    fn aapl_stream_with(messages: &[Vec<u8>]) -> Vec<u8> {
        let mut buf = build_stock_directory_msg(1, b"AAPL    ");
        for msg in messages {
            buf.extend_from_slice(msg);
        }
        buf
    }

    #[rstest]
    fn test_convert_side() {
        assert_eq!(convert_side(itchy::Side::Buy), OrderSide::Buy);
        assert_eq!(convert_side(itchy::Side::Sell), OrderSide::Sell);
    }

    #[rstest]
    fn test_convert_price() {
        let price = convert_price(itchy::Price4::from(1_2345));
        assert_eq!(price.as_f64(), 1.2345);
        assert_eq!(price.precision, PRICE_PRECISION);
    }

    #[rstest]
    fn test_convert_price_whole_dollar() {
        let price = convert_price(itchy::Price4::from(100_0000));
        assert_eq!(price.as_f64(), 100.0);
    }

    #[rstest]
    fn test_convert_price_sub_penny() {
        let price = convert_price(itchy::Price4::from(150_2501));
        assert_eq!(price.as_f64(), 150.2501);
    }

    #[rstest]
    fn test_add_order() {
        let buf = aapl_stream_with(&[build_add_order_msg(1, 42, b'B', 100, 1_502_500)]);
        let mut parser = setup_parser(0);
        let deltas = parser.parse_reader(&buf[..]).unwrap();

        assert_eq!(deltas.len(), 1);
        assert_eq!(deltas[0].action, BookAction::Add);
        assert_eq!(deltas[0].order.side, OrderSide::Buy);
        assert_eq!(deltas[0].order.price.as_f64(), 150.25);
        assert_eq!(deltas[0].order.size.as_f64(), 100.0);
        assert_eq!(deltas[0].order.order_id, 42);
    }

    #[rstest]
    fn test_delete_order() {
        let buf = aapl_stream_with(&[
            build_add_order_msg(1, 42, b'B', 100, 1_500_000),
            build_delete_order_msg(1, 42),
        ]);
        let mut parser = setup_parser(0);
        let deltas = parser.parse_reader(&buf[..]).unwrap();

        assert_eq!(deltas.len(), 2);
        assert_eq!(deltas[1].action, BookAction::Delete);
        assert_eq!(deltas[1].order.order_id, 42);
        assert_eq!(deltas[1].order.size.as_f64(), 0.0);
    }

    #[rstest]
    fn test_delete_unknown_order_is_ignored() {
        let buf = aapl_stream_with(&[build_delete_order_msg(1, 999)]);
        let mut parser = setup_parser(0);
        let deltas = parser.parse_reader(&buf[..]).unwrap();

        assert_eq!(deltas.len(), 0);
    }

    #[rstest]
    fn test_partial_cancel_reduces_size() {
        let buf = aapl_stream_with(&[
            build_add_order_msg(1, 42, b'B', 100, 1_500_000),
            build_order_cancelled_msg(1, 42, 30),
        ]);
        let mut parser = setup_parser(0);
        let deltas = parser.parse_reader(&buf[..]).unwrap();

        assert_eq!(deltas.len(), 2);
        assert_eq!(deltas[1].action, BookAction::Update);
        assert_eq!(deltas[1].order.size.as_f64(), 70.0);
        assert_eq!(deltas[1].order.price.as_f64(), 150.0);
        assert_eq!(deltas[1].order.order_id, 42);
    }

    #[rstest]
    fn test_full_cancel_deletes_order() {
        let buf = aapl_stream_with(&[
            build_add_order_msg(1, 42, b'S', 100, 1_500_000),
            build_order_cancelled_msg(1, 42, 100),
        ]);
        let mut parser = setup_parser(0);
        let deltas = parser.parse_reader(&buf[..]).unwrap();

        assert_eq!(deltas.len(), 2);
        assert_eq!(deltas[1].action, BookAction::Delete);
        assert_eq!(deltas[1].order.size.as_f64(), 0.0);
    }

    #[rstest]
    fn test_cancel_unknown_order_is_ignored() {
        let buf = aapl_stream_with(&[build_order_cancelled_msg(1, 999, 50)]);
        let mut parser = setup_parser(0);
        let deltas = parser.parse_reader(&buf[..]).unwrap();

        assert_eq!(deltas.len(), 0);
    }

    #[rstest]
    fn test_partial_execution_updates_size() {
        let buf = aapl_stream_with(&[
            build_add_order_msg(1, 42, b'B', 100, 1_500_000),
            build_order_executed_msg(1, 42, 40, 1001),
        ]);
        let mut parser = setup_parser(0);
        let deltas = parser.parse_reader(&buf[..]).unwrap();

        assert_eq!(deltas.len(), 2);
        assert_eq!(deltas[1].action, BookAction::Update);
        assert_eq!(deltas[1].order.size.as_f64(), 60.0);
        assert_eq!(deltas[1].order.order_id, 42);
    }

    #[rstest]
    fn test_full_execution_deletes_order() {
        let buf = aapl_stream_with(&[
            build_add_order_msg(1, 42, b'S', 100, 1_500_000),
            build_order_executed_msg(1, 42, 100, 1001),
        ]);
        let mut parser = setup_parser(0);
        let deltas = parser.parse_reader(&buf[..]).unwrap();

        assert_eq!(deltas.len(), 2);
        assert_eq!(deltas[1].action, BookAction::Delete);
        assert_eq!(deltas[1].order.size.as_f64(), 0.0);
    }

    #[rstest]
    fn test_multiple_partial_executions_then_full() {
        let buf = aapl_stream_with(&[
            build_add_order_msg(1, 42, b'B', 100, 1_500_000),
            build_order_executed_msg(1, 42, 30, 1001),
            build_order_executed_msg(1, 42, 30, 1002),
            build_order_executed_msg(1, 42, 40, 1003),
        ]);
        let mut parser = setup_parser(0);
        let deltas = parser.parse_reader(&buf[..]).unwrap();

        assert_eq!(deltas.len(), 4);
        assert_eq!(deltas[1].action, BookAction::Update);
        assert_eq!(deltas[1].order.size.as_f64(), 70.0);
        assert_eq!(deltas[2].action, BookAction::Update);
        assert_eq!(deltas[2].order.size.as_f64(), 40.0);
        assert_eq!(deltas[3].action, BookAction::Delete);
        assert_eq!(deltas[3].order.size.as_f64(), 0.0);
    }

    #[rstest]
    fn test_executed_with_price_partial() {
        let buf = aapl_stream_with(&[
            build_add_order_msg(1, 42, b'B', 100, 1_500_000),
            build_order_executed_with_price_msg(1, 42, 25, 2001, 1_505_000),
        ]);
        let mut parser = setup_parser(0);
        let deltas = parser.parse_reader(&buf[..]).unwrap();

        assert_eq!(deltas.len(), 2);
        assert_eq!(deltas[1].action, BookAction::Update);
        assert_eq!(deltas[1].order.size.as_f64(), 75.0);
        // Book order retains the resting price, not the execution price
        assert_eq!(deltas[1].order.price.as_f64(), 150.0);
    }

    #[rstest]
    fn test_execution_unknown_order_is_ignored() {
        let buf = aapl_stream_with(&[build_order_executed_msg(1, 999, 50, 1001)]);
        let mut parser = setup_parser(0);
        let deltas = parser.parse_reader(&buf[..]).unwrap();

        assert_eq!(deltas.len(), 0);
    }

    #[rstest]
    fn test_replace_order() {
        let buf = aapl_stream_with(&[
            build_add_order_msg(1, 42, b'B', 100, 1_500_000),
            build_replace_order_msg(1, 42, 43, 150, 1_510_000),
        ]);
        let mut parser = setup_parser(0);
        let deltas = parser.parse_reader(&buf[..]).unwrap();

        assert_eq!(deltas.len(), 3);
        assert_eq!(deltas[1].action, BookAction::Delete);
        assert_eq!(deltas[1].order.order_id, 42);
        assert_eq!(deltas[2].action, BookAction::Add);
        assert_eq!(deltas[2].order.order_id, 43);
        assert_eq!(deltas[2].order.price.as_f64(), 151.0);
        assert_eq!(deltas[2].order.size.as_f64(), 150.0);
        assert_eq!(deltas[2].order.side, OrderSide::Buy);
    }

    #[rstest]
    fn test_replace_inherits_side_from_original() {
        let buf = aapl_stream_with(&[
            build_add_order_msg(1, 42, b'S', 100, 1_500_000),
            build_replace_order_msg(1, 42, 43, 200, 1_490_000),
        ]);
        let mut parser = setup_parser(0);
        let deltas = parser.parse_reader(&buf[..]).unwrap();

        assert_eq!(deltas[2].order.side, OrderSide::Sell);
    }

    #[rstest]
    fn test_replace_delete_has_no_f_last_flag() {
        // The delete in a replace pair is not the last event in the group
        let buf = aapl_stream_with(&[
            build_add_order_msg(1, 42, b'B', 100, 1_500_000),
            build_replace_order_msg(1, 42, 43, 100, 1_510_000),
        ]);
        let mut parser = setup_parser(0);
        let deltas = parser.parse_reader(&buf[..]).unwrap();

        assert_eq!(deltas[1].flags, 0);
        assert_ne!(deltas[2].flags & RecordFlag::F_LAST as u8, 0);
    }

    #[rstest]
    fn test_replace_unknown_order_is_ignored() {
        let buf = aapl_stream_with(&[build_replace_order_msg(1, 999, 1000, 100, 1_500_000)]);
        let mut parser = setup_parser(0);
        let deltas = parser.parse_reader(&buf[..]).unwrap();

        assert_eq!(deltas.len(), 0);
    }

    #[rstest]
    fn test_end_of_messages_emits_clear() {
        let buf = aapl_stream_with(&[
            build_add_order_msg(1, 42, b'B', 100, 1_500_000),
            // EndOfMessages uses locate=0 (feed-level, not stock-specific)
            build_system_event_msg(0, b'C'),
        ]);
        let mut parser = setup_parser(0);
        let deltas = parser.parse_reader(&buf[..]).unwrap();

        assert_eq!(deltas.len(), 2);
        assert_eq!(deltas[1].action, BookAction::Clear);
    }

    #[rstest]
    fn test_end_of_messages_with_different_locate() {
        // Regression: EndOfMessages must be processed regardless of locate code
        let buf = aapl_stream_with(&[
            build_add_order_msg(1, 42, b'B', 100, 1_500_000),
            build_system_event_msg(99, b'C'),
        ]);
        let mut parser = setup_parser(0);
        let deltas = parser.parse_reader(&buf[..]).unwrap();

        assert_eq!(deltas.len(), 2);
        assert_eq!(deltas[1].action, BookAction::Clear);
    }

    #[rstest]
    fn test_filters_by_stock_locate() {
        let mut buf = build_stock_directory_msg(1, b"AAPL    ");
        buf.extend_from_slice(&build_stock_directory_msg(2, b"MSFT    "));
        buf.extend_from_slice(&build_add_order_msg(2, 10, b'B', 50, 3_000_000));
        buf.extend_from_slice(&build_add_order_msg(1, 11, b'S', 200, 1_500_000));

        let mut parser = setup_parser(0);
        let deltas = parser.parse_reader(&buf[..]).unwrap();

        assert_eq!(deltas.len(), 1);
        assert_eq!(deltas[0].order.order_id, 11);
    }

    #[rstest]
    fn test_messages_before_directory_are_ignored() {
        let mut buf = Vec::new();
        // AddOrder arrives before any StockDirectory
        buf.extend_from_slice(&build_add_order_msg(1, 42, b'B', 100, 1_500_000));
        buf.extend_from_slice(&build_stock_directory_msg(1, b"AAPL    "));
        buf.extend_from_slice(&build_add_order_msg(1, 43, b'B', 100, 1_500_000));

        let mut parser = setup_parser(0);
        let deltas = parser.parse_reader(&buf[..]).unwrap();

        // Only the second add (after directory) should be captured
        assert_eq!(deltas.len(), 1);
        assert_eq!(deltas[0].order.order_id, 43);
    }

    #[rstest]
    fn test_timestamp_offset_from_midnight() {
        let base_ns: u64 = 1_548_806_400_000_000_000; // 2019-01-30 midnight UTC
        let itch_ts: u64 = 34_200_000_000_000; // 9:30 AM (ns since midnight)
        let buf = aapl_stream_with(&[build_add_order_msg_with_ts(
            1, 42, b'B', 100, 1_500_000, itch_ts,
        )]);
        let mut parser = setup_parser(base_ns);
        let deltas = parser.parse_reader(&buf[..]).unwrap();

        assert_eq!(deltas[0].ts_event, UnixNanos::from(base_ns + itch_ts));
        assert_eq!(deltas[0].ts_init, deltas[0].ts_event);
    }

    #[rstest]
    fn test_f_last_set_on_final_delta() {
        let buf = aapl_stream_with(&[
            build_add_order_msg(1, 42, b'B', 100, 1_500_000),
            build_add_order_msg(1, 43, b'S', 200, 1_510_000),
        ]);
        let mut parser = setup_parser(0);
        let deltas = parser.parse_reader(&buf[..]).unwrap();

        assert_eq!(deltas.len(), 2);
        assert_ne!(deltas[1].flags & RecordFlag::F_LAST as u8, 0);
    }

    #[rstest]
    fn test_sequence_numbers_are_monotonic() {
        let buf = aapl_stream_with(&[
            build_add_order_msg(1, 42, b'B', 100, 1_500_000),
            build_order_executed_msg(1, 42, 50, 1001),
            build_add_order_msg(1, 43, b'S', 200, 1_510_000),
            build_delete_order_msg(1, 43),
        ]);
        let mut parser = setup_parser(0);
        let deltas = parser.parse_reader(&buf[..]).unwrap();

        for i in 1..deltas.len() {
            assert!(deltas[i].sequence > deltas[i - 1].sequence);
        }
    }

    #[rstest]
    fn test_empty_stream() {
        let buf: &[u8] = &[];
        let mut parser = setup_parser(0);
        let deltas = parser.parse_reader(buf).unwrap();

        assert_eq!(deltas.len(), 0);
    }

    fn build_msg(tag: u8, stock_locate: u16, timestamp: u64, body: &[u8]) -> Vec<u8> {
        let msg_len = (1 + 2 + 2 + 6 + body.len()) as u16;
        let mut buf = Vec::new();
        buf.extend_from_slice(&msg_len.to_be_bytes());
        buf.push(tag);
        buf.extend_from_slice(&stock_locate.to_be_bytes());
        buf.extend_from_slice(&0u16.to_be_bytes()); // tracking_number
        // 6-byte timestamp (big-endian u48)
        buf.push((timestamp >> 40) as u8);
        buf.push((timestamp >> 32) as u8);
        buf.push((timestamp >> 24) as u8);
        buf.push((timestamp >> 16) as u8);
        buf.push((timestamp >> 8) as u8);
        buf.push(timestamp as u8);
        buf.extend_from_slice(body);
        buf
    }

    fn build_stock_directory_msg(locate: u16, stock: &[u8; 8]) -> Vec<u8> {
        let mut body = Vec::new();
        body.extend_from_slice(stock);
        body.push(b'Q'); // market_category
        body.push(b'N'); // financial_status
        body.extend_from_slice(&100u32.to_be_bytes()); // round_lot_size
        body.push(b'Y'); // round_lots_only
        body.push(b'C'); // issue_classification
        body.extend_from_slice(b"C "); // issue_subtype
        body.push(b'P'); // authenticity
        body.push(b'N'); // short_sale_threshold
        body.push(b'N'); // ipo_flag
        body.push(b'1'); // luld_ref_price_tier
        body.push(b'N'); // etp_flag
        body.extend_from_slice(&0u32.to_be_bytes()); // etp_leverage_factor
        body.push(b'N'); // inverse_indicator
        build_msg(b'R', locate, 0, &body)
    }

    fn build_add_order_msg(
        locate: u16,
        reference: u64,
        side: u8,
        shares: u32,
        price: u32,
    ) -> Vec<u8> {
        build_add_order_msg_with_ts(locate, reference, side, shares, price, 0)
    }

    fn build_add_order_msg_with_ts(
        locate: u16,
        reference: u64,
        side: u8,
        shares: u32,
        price: u32,
        timestamp: u64,
    ) -> Vec<u8> {
        let mut body = Vec::new();
        body.extend_from_slice(&reference.to_be_bytes());
        body.push(side);
        body.extend_from_slice(&shares.to_be_bytes());
        body.extend_from_slice(b"AAPL    ");
        body.extend_from_slice(&price.to_be_bytes());
        build_msg(b'A', locate, timestamp, &body)
    }

    fn build_delete_order_msg(locate: u16, reference: u64) -> Vec<u8> {
        build_msg(b'D', locate, 0, &reference.to_be_bytes())
    }

    fn build_order_cancelled_msg(locate: u16, reference: u64, cancelled: u32) -> Vec<u8> {
        let mut body = Vec::new();
        body.extend_from_slice(&reference.to_be_bytes());
        body.extend_from_slice(&cancelled.to_be_bytes());
        build_msg(b'X', locate, 0, &body)
    }

    fn build_order_executed_msg(
        locate: u16,
        reference: u64,
        executed: u32,
        match_number: u64,
    ) -> Vec<u8> {
        let mut body = Vec::new();
        body.extend_from_slice(&reference.to_be_bytes());
        body.extend_from_slice(&executed.to_be_bytes());
        body.extend_from_slice(&match_number.to_be_bytes());
        build_msg(b'E', locate, 0, &body)
    }

    fn build_order_executed_with_price_msg(
        locate: u16,
        reference: u64,
        executed: u32,
        match_number: u64,
        price: u32,
    ) -> Vec<u8> {
        let mut body = Vec::new();
        body.extend_from_slice(&reference.to_be_bytes());
        body.extend_from_slice(&executed.to_be_bytes());
        body.extend_from_slice(&match_number.to_be_bytes());
        body.push(b'Y'); // printable
        body.extend_from_slice(&price.to_be_bytes());
        build_msg(b'C', locate, 0, &body)
    }

    fn build_replace_order_msg(
        locate: u16,
        old_reference: u64,
        new_reference: u64,
        shares: u32,
        price: u32,
    ) -> Vec<u8> {
        let mut body = Vec::new();
        body.extend_from_slice(&old_reference.to_be_bytes());
        body.extend_from_slice(&new_reference.to_be_bytes());
        body.extend_from_slice(&shares.to_be_bytes());
        body.extend_from_slice(&price.to_be_bytes());
        build_msg(b'U', locate, 0, &body)
    }

    fn build_system_event_msg(locate: u16, event_code: u8) -> Vec<u8> {
        build_msg(b'S', locate, 0, &[event_code])
    }

    // Curates AAPL L3 deltas from NASDAQ ITCH 5.0 binary into NautilusTrader Parquet.
    // Download source: https://emi.nasdaq.com/ITCH/Nasdaq%20ITCH/01302019.NASDAQ_ITCH50.gz
    // Run: cargo test -p nautilus-testkit --lib test_curate_aapl_itch -- --ignored --nocapture
    #[rstest]
    #[ignore = "one-time dataset curation, not for routine CI"]
    fn test_curate_aapl_itch() {
        let itch_path = PathBuf::from("/tmp/01302019.NASDAQ_ITCH50.gz");
        let instrument_id = InstrumentId::from("AAPL.XNAS");

        // 2019-01-30 midnight EST (UTC-5) as Unix nanoseconds
        let base_ns: u64 = 1_548_824_400_000_000_000;
        let parquet_path = "/tmp/itch_AAPL.XNAS_2019-01-30_deltas.parquet";

        println!("Parsing ITCH from {}", itch_path.display());
        let mut parser = ItchParser::new(instrument_id, "AAPL", base_ns);
        let deltas = parser.parse_gzip_file(&itch_path).unwrap();
        let count = deltas.len();
        println!("Parsed {count} deltas for AAPL");

        let metadata =
            OrderBookDelta::get_metadata(&instrument_id, PRICE_PRECISION, SIZE_PRECISION);
        let schema = OrderBookDelta::get_schema(Some(metadata.clone()));

        println!("Writing Parquet to {parquet_path}");
        let file = File::create(parquet_path).unwrap();
        let zstd_level = parquet::basic::ZstdLevel::try_new(3).unwrap();
        let props = WriterProperties::builder()
            .set_compression(parquet::basic::Compression::ZSTD(zstd_level))
            .set_max_row_group_row_count(Some(1_000_000))
            .build();
        let mut writer = ArrowWriter::try_new(file, Arc::new(schema), Some(props)).unwrap();

        let chunk_size = 1_000_000;
        for (i, chunk) in deltas.chunks(chunk_size).enumerate() {
            println!("  Encoding chunk {} ({} records)...", i + 1, chunk.len());
            let batch = OrderBookDelta::encode_batch(&metadata, chunk).unwrap();
            writer.write(&batch).unwrap();
        }
        writer.close().unwrap();

        let file_size = fs::metadata(parquet_path).unwrap().len();
        println!("\nRecords: {count}");
        println!("Price precision: {PRICE_PRECISION}");
        println!("Size precision: {SIZE_PRECISION}");
        println!(
            "File size: {} bytes ({:.1} MB)",
            file_size,
            file_size as f64 / 1_048_576.0
        );
        println!("Output: {parquet_path}");
        println!("\nNext steps:");
        println!("  sha256sum {parquet_path}");
    }
}
