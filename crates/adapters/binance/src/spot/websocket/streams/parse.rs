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

//! Parsing utilities for Binance Spot WebSocket SBE messages.

use nautilus_core::nanos::UnixNanos;
use nautilus_model::{
    data::{BookOrder, Data, OrderBookDelta, OrderBookDeltas, QuoteTick, TradeTick},
    enums::{AggressorSide, BookAction, OrderSide, RecordFlag},
    identifiers::TradeId,
    instruments::{Instrument, InstrumentAny},
    types::{Price, Quantity},
};

use crate::spot::sbe::stream::{
    BestBidAskStreamEvent, DepthDiffStreamEvent, DepthSnapshotStreamEvent, MessageHeader,
    StreamDecodeError, TradesStreamEvent, template_id,
};

/// Decoded market data message.
#[derive(Debug)]
pub enum MarketDataMessage {
    /// Trade event.
    Trades(TradesStreamEvent),
    /// Best bid/ask update.
    BestBidAsk(BestBidAskStreamEvent),
    /// Order book snapshot.
    DepthSnapshot(DepthSnapshotStreamEvent),
    /// Order book diff update.
    DepthDiff(DepthDiffStreamEvent),
}

/// Decode an SBE binary frame into a market data message.
///
/// Validates the message header (including schema ID) and routes to the
/// appropriate decoder based on template ID.
///
/// # Errors
///
/// Returns an error if the buffer is too short, schema validation fails,
/// or the template ID is unknown.
pub fn decode_market_data(buf: &[u8]) -> Result<MarketDataMessage, StreamDecodeError> {
    let header = MessageHeader::decode(buf)?;
    header.validate_schema()?;

    match header.template_id {
        template_id::TRADES_STREAM_EVENT => Ok(MarketDataMessage::Trades(
            TradesStreamEvent::decode_validated(buf)?,
        )),
        template_id::BEST_BID_ASK_STREAM_EVENT => Ok(MarketDataMessage::BestBidAsk(
            BestBidAskStreamEvent::decode_validated(buf)?,
        )),
        template_id::DEPTH_SNAPSHOT_STREAM_EVENT => Ok(MarketDataMessage::DepthSnapshot(
            DepthSnapshotStreamEvent::decode_validated(buf)?,
        )),
        template_id::DEPTH_DIFF_STREAM_EVENT => Ok(MarketDataMessage::DepthDiff(
            DepthDiffStreamEvent::decode_validated(buf)?,
        )),
        _ => Err(StreamDecodeError::UnknownTemplateId(header.template_id)),
    }
}

/// Parses a trades stream event into a vector of `TradeTick`.
pub fn parse_trades_event(event: &TradesStreamEvent, instrument: &InstrumentAny) -> Vec<Data> {
    let instrument_id = instrument.id();
    let price_precision = instrument.price_precision();
    let size_precision = instrument.size_precision();

    event
        .trades
        .iter()
        .map(|t| {
            let price = Price::from_mantissa_exponent(
                t.price_mantissa,
                event.price_exponent,
                price_precision,
            );
            let size = Quantity::from_mantissa_exponent(
                t.qty_mantissa as u64,
                event.qty_exponent,
                size_precision,
            );
            let ts_event = UnixNanos::from_micros(event.transact_time_us as u64);

            let trade = TradeTick::new(
                instrument_id,
                price,
                size,
                if t.is_buyer_maker {
                    AggressorSide::Seller
                } else {
                    AggressorSide::Buyer
                },
                TradeId::new(t.id.to_string()),
                ts_event,
                ts_event,
            );
            Data::from(trade)
        })
        .collect()
}

/// Parses a best bid/ask event into a `QuoteTick`.
pub fn parse_bbo_event(event: &BestBidAskStreamEvent, instrument: &InstrumentAny) -> QuoteTick {
    let instrument_id = instrument.id();
    let price_precision = instrument.price_precision();
    let size_precision = instrument.size_precision();

    let bid_price = Price::from_mantissa_exponent(
        event.bid_price_mantissa,
        event.price_exponent,
        price_precision,
    );
    let bid_size = Quantity::from_mantissa_exponent(
        event.bid_qty_mantissa as u64,
        event.qty_exponent,
        size_precision,
    );
    let ask_price = Price::from_mantissa_exponent(
        event.ask_price_mantissa,
        event.price_exponent,
        price_precision,
    );
    let ask_size = Quantity::from_mantissa_exponent(
        event.ask_qty_mantissa as u64,
        event.qty_exponent,
        size_precision,
    );
    let ts_event = UnixNanos::from_micros(event.event_time_us as u64);

    QuoteTick::new(
        instrument_id,
        bid_price,
        ask_price,
        bid_size,
        ask_size,
        ts_event,
        ts_event,
    )
}

/// Parses a depth snapshot event into `OrderBookDeltas`.
///
/// Returns `None` if the snapshot contains no levels.
pub fn parse_depth_snapshot(
    event: &DepthSnapshotStreamEvent,
    instrument: &InstrumentAny,
) -> Option<OrderBookDeltas> {
    let instrument_id = instrument.id();
    let price_precision = instrument.price_precision();
    let size_precision = instrument.size_precision();
    let ts_event = UnixNanos::from_micros(event.event_time_us as u64);

    let mut deltas = Vec::with_capacity(event.bids.len() + event.asks.len() + 1);

    // Add clear delta first
    deltas.push(OrderBookDelta::clear(instrument_id, 0, ts_event, ts_event));

    // Add bid levels
    for (i, level) in event.bids.iter().enumerate() {
        let price = Price::from_mantissa_exponent(
            level.price_mantissa,
            event.price_exponent,
            price_precision,
        );
        let size = Quantity::from_mantissa_exponent(
            level.qty_mantissa as u64,
            event.qty_exponent,
            size_precision,
        );
        let flags = if i == event.bids.len() - 1 && event.asks.is_empty() {
            RecordFlag::F_LAST as u8
        } else {
            0
        };

        let order = BookOrder::new(OrderSide::Buy, price, size, 0);

        deltas.push(OrderBookDelta::new(
            instrument_id,
            BookAction::Add,
            order,
            flags,
            0,
            ts_event,
            ts_event,
        ));
    }

    // Add ask levels
    for (i, level) in event.asks.iter().enumerate() {
        let price = Price::from_mantissa_exponent(
            level.price_mantissa,
            event.price_exponent,
            price_precision,
        );
        let size = Quantity::from_mantissa_exponent(
            level.qty_mantissa as u64,
            event.qty_exponent,
            size_precision,
        );
        let flags = if i == event.asks.len() - 1 {
            RecordFlag::F_LAST as u8
        } else {
            0
        };

        let order = BookOrder::new(OrderSide::Sell, price, size, 0);

        deltas.push(OrderBookDelta::new(
            instrument_id,
            BookAction::Add,
            order,
            flags,
            0,
            ts_event,
            ts_event,
        ));
    }

    // A snapshot that only contains the synthetic clear delta has no book levels
    // to apply and is treated as "no usable update".
    if deltas.len() <= 1 {
        return None;
    }

    Some(OrderBookDeltas::new(instrument_id, deltas))
}

/// Parses a depth diff event into `OrderBookDeltas`.
///
/// Returns `None` if the diff contains no updates.
pub fn parse_depth_diff(
    event: &DepthDiffStreamEvent,
    instrument: &InstrumentAny,
) -> Option<OrderBookDeltas> {
    let instrument_id = instrument.id();
    let price_precision = instrument.price_precision();
    let size_precision = instrument.size_precision();
    let ts_event = UnixNanos::from_micros(event.event_time_us as u64);

    let mut deltas = Vec::with_capacity(event.bids.len() + event.asks.len());

    // Add bid updates
    for (i, level) in event.bids.iter().enumerate() {
        let price = Price::from_mantissa_exponent(
            level.price_mantissa,
            event.price_exponent,
            price_precision,
        );
        let size = Quantity::from_mantissa_exponent(
            level.qty_mantissa as u64,
            event.qty_exponent,
            size_precision,
        );

        // Zero size means delete, otherwise update
        let action = if level.qty_mantissa == 0 {
            BookAction::Delete
        } else {
            BookAction::Update
        };

        let flags = if i == event.bids.len() - 1 && event.asks.is_empty() {
            RecordFlag::F_LAST as u8
        } else {
            0
        };

        let order = BookOrder::new(OrderSide::Buy, price, size, 0);

        deltas.push(OrderBookDelta::new(
            instrument_id,
            action,
            order,
            flags,
            0,
            ts_event,
            ts_event,
        ));
    }

    // Add ask updates
    for (i, level) in event.asks.iter().enumerate() {
        let price = Price::from_mantissa_exponent(
            level.price_mantissa,
            event.price_exponent,
            price_precision,
        );
        let size = Quantity::from_mantissa_exponent(
            level.qty_mantissa as u64,
            event.qty_exponent,
            size_precision,
        );

        let action = if level.qty_mantissa == 0 {
            BookAction::Delete
        } else {
            BookAction::Update
        };

        let flags = if i == event.asks.len() - 1 {
            RecordFlag::F_LAST as u8
        } else {
            0
        };

        let order = BookOrder::new(OrderSide::Sell, price, size, 0);

        deltas.push(OrderBookDelta::new(
            instrument_id,
            action,
            order,
            flags,
            0,
            ts_event,
            ts_event,
        ));
    }

    if deltas.is_empty() {
        return None;
    }

    Some(OrderBookDeltas::new(instrument_id, deltas))
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use ustr::Ustr;

    use super::*;
    use crate::{
        common::parse::parse_spot_instrument_sbe,
        spot::{
            http::models::{
                BinanceLotSizeFilterSbe, BinancePriceFilterSbe, BinanceSymbolFiltersSbe,
                BinanceSymbolSbe,
            },
            sbe::stream::{PriceLevel, STREAM_SCHEMA_ID, Trade},
        },
    };

    fn make_bbo_buffer() -> Vec<u8> {
        let mut buf = vec![0u8; 70];

        // Header
        buf[0..2].copy_from_slice(&50u16.to_le_bytes()); // block_length
        buf[2..4].copy_from_slice(&template_id::BEST_BID_ASK_STREAM_EVENT.to_le_bytes());
        buf[4..6].copy_from_slice(&STREAM_SCHEMA_ID.to_le_bytes());
        buf[6..8].copy_from_slice(&0u16.to_le_bytes()); // version

        // Body
        let body = &mut buf[8..];
        body[0..8].copy_from_slice(&1000000i64.to_le_bytes()); // event_time_us
        body[8..16].copy_from_slice(&12345i64.to_le_bytes()); // book_update_id
        body[16] = (-2i8) as u8; // price_exponent
        body[17] = (-8i8) as u8; // qty_exponent
        body[18..26].copy_from_slice(&4200000i64.to_le_bytes()); // bid_price
        body[26..34].copy_from_slice(&100000000i64.to_le_bytes()); // bid_qty
        body[34..42].copy_from_slice(&4200100i64.to_le_bytes()); // ask_price
        body[42..50].copy_from_slice(&200000000i64.to_le_bytes()); // ask_qty

        // Symbol: "BTCUSDT" (7 bytes)
        body[50] = 7;
        body[51..58].copy_from_slice(b"BTCUSDT");

        buf
    }

    fn sample_instrument() -> InstrumentAny {
        let symbol = BinanceSymbolSbe {
            symbol: "ETHUSDT".to_string(),
            base_asset: "ETH".to_string(),
            quote_asset: "USDT".to_string(),
            base_asset_precision: 8,
            quote_asset_precision: 8,
            status: 0,
            order_types: 0,
            iceberg_allowed: true,
            oco_allowed: true,
            oto_allowed: false,
            quote_order_qty_market_allowed: true,
            allow_trailing_stop: true,
            cancel_replace_allowed: true,
            amend_allowed: true,
            is_spot_trading_allowed: true,
            is_margin_trading_allowed: false,
            filters: BinanceSymbolFiltersSbe {
                price_filter: Some(BinancePriceFilterSbe {
                    price_exponent: -8,
                    min_price: 1_000_000,
                    max_price: 100_000_000_000_000,
                    tick_size: 1_000_000,
                }),
                lot_size_filter: Some(BinanceLotSizeFilterSbe {
                    qty_exponent: -8,
                    min_qty: 10_000,
                    max_qty: 900_000_000_000,
                    step_size: 10_000,
                }),
            },
            permissions: vec![vec!["SPOT".to_string()]],
        };

        let ts = UnixNanos::from(1_700_000_000_000_000_000u64);
        parse_spot_instrument_sbe(&symbol, ts, ts).unwrap()
    }

    #[rstest]
    fn test_decode_empty_buffer() {
        let err = decode_market_data(&[]).unwrap_err();
        assert!(matches!(err, StreamDecodeError::BufferTooShort { .. }));
    }

    #[rstest]
    fn test_decode_short_buffer() {
        let buf = [0u8; 5];
        let err = decode_market_data(&buf).unwrap_err();
        assert!(matches!(err, StreamDecodeError::BufferTooShort { .. }));
    }

    #[rstest]
    fn test_decode_wrong_schema() {
        let mut buf = [0u8; 100];
        buf[0..2].copy_from_slice(&50u16.to_le_bytes()); // block_length
        buf[2..4].copy_from_slice(&template_id::BEST_BID_ASK_STREAM_EVENT.to_le_bytes());
        buf[4..6].copy_from_slice(&99u16.to_le_bytes()); // Wrong schema
        buf[6..8].copy_from_slice(&0u16.to_le_bytes()); // version

        let err = decode_market_data(&buf).unwrap_err();
        assert!(matches!(err, StreamDecodeError::SchemaMismatch { .. }));
    }

    #[rstest]
    fn test_decode_unknown_template() {
        let mut buf = [0u8; 100];
        buf[0..2].copy_from_slice(&50u16.to_le_bytes()); // block_length
        buf[2..4].copy_from_slice(&9999u16.to_le_bytes()); // Unknown template
        buf[4..6].copy_from_slice(&STREAM_SCHEMA_ID.to_le_bytes());
        buf[6..8].copy_from_slice(&0u16.to_le_bytes()); // version

        let err = decode_market_data(&buf).unwrap_err();
        assert!(matches!(err, StreamDecodeError::UnknownTemplateId(9999)));
    }

    #[rstest]
    fn test_decode_valid_best_bid_ask() {
        let buf = make_bbo_buffer();
        let msg = decode_market_data(&buf).unwrap();

        match msg {
            MarketDataMessage::BestBidAsk(event) => {
                assert_eq!(event.event_time_us, 1_000_000);
                assert_eq!(event.symbol, Ustr::from("BTCUSDT"));
            }
            _ => panic!("Expected BestBidAsk"),
        }
    }

    #[rstest]
    fn test_parse_trades_event() {
        let instrument = sample_instrument();
        let event = TradesStreamEvent {
            event_time_us: 1_700_000_000_000_000,
            transact_time_us: 1_700_000_000_100_000,
            price_exponent: -2,
            qty_exponent: -4,
            trades: vec![
                Trade {
                    id: 1,
                    price_mantissa: 12_345,
                    qty_mantissa: 25_000,
                    is_buyer_maker: false,
                },
                Trade {
                    id: 2,
                    price_mantissa: 12_340,
                    qty_mantissa: 10_000,
                    is_buyer_maker: true,
                },
            ],
            symbol: Ustr::from("ETHUSDT"),
        };

        let data = parse_trades_event(&event, &instrument);

        assert_eq!(data.len(), 2);
        match &data[0] {
            Data::Trade(trade) => {
                assert_eq!(trade.instrument_id, instrument.id());
                assert_eq!(trade.price, Price::new(123.45, 2));
                assert_eq!(trade.size, Quantity::new(2.5, 4));
                assert_eq!(trade.aggressor_side, AggressorSide::Buyer);
                assert_eq!(trade.trade_id, TradeId::new("1"));
                assert_eq!(
                    trade.ts_event,
                    UnixNanos::from(1_700_000_000_100_000_000u64)
                );
                assert_eq!(trade.ts_init, UnixNanos::from(1_700_000_000_100_000_000u64));
            }
            other => panic!("Expected trade data, was {other:?}"),
        }

        match &data[1] {
            Data::Trade(trade) => assert_eq!(trade.aggressor_side, AggressorSide::Seller),
            other => panic!("Expected trade data, was {other:?}"),
        }
    }

    #[rstest]
    fn test_parse_bbo_event() {
        let instrument = sample_instrument();
        let event = BestBidAskStreamEvent {
            event_time_us: 1_700_000_000_000_000,
            book_update_id: 123,
            price_exponent: -2,
            qty_exponent: -4,
            bid_price_mantissa: 12_345,
            bid_qty_mantissa: 25_000,
            ask_price_mantissa: 12_350,
            ask_qty_mantissa: 30_000,
            symbol: Ustr::from("ETHUSDT"),
        };

        let quote = parse_bbo_event(&event, &instrument);

        assert_eq!(quote.instrument_id, instrument.id());
        assert_eq!(quote.bid_price, Price::new(123.45, 2));
        assert_eq!(quote.ask_price, Price::new(123.50, 2));
        assert_eq!(quote.bid_size, Quantity::new(2.5, 4));
        assert_eq!(quote.ask_size, Quantity::new(3.0, 4));
        assert_eq!(
            quote.ts_event,
            UnixNanos::from(1_700_000_000_000_000_000u64)
        );
        assert_eq!(quote.ts_init, UnixNanos::from(1_700_000_000_000_000_000u64));
    }

    #[rstest]
    fn test_parse_depth_snapshot() {
        let instrument = sample_instrument();
        let event = DepthSnapshotStreamEvent {
            event_time_us: 1_700_000_000_000_000,
            book_update_id: 123,
            price_exponent: -2,
            qty_exponent: -4,
            bids: vec![PriceLevel {
                price_mantissa: 12_345,
                qty_mantissa: 25_000,
            }],
            asks: vec![PriceLevel {
                price_mantissa: 12_350,
                qty_mantissa: 30_000,
            }],
            symbol: Ustr::from("ETHUSDT"),
        };

        let deltas = parse_depth_snapshot(&event, &instrument).unwrap();

        assert_eq!(deltas.instrument_id, instrument.id());
        assert_eq!(deltas.deltas.len(), 3);
        assert_eq!(deltas.deltas[0].action, BookAction::Clear);
        assert_eq!(deltas.deltas[1].action, BookAction::Add);
        assert_eq!(deltas.deltas[1].order.side, OrderSide::Buy);
        assert_eq!(deltas.deltas[1].order.price, Price::new(123.45, 2));
        assert_eq!(deltas.deltas[1].order.size, Quantity::new(2.5, 4));
        assert_eq!(deltas.deltas[2].action, BookAction::Add);
        assert_eq!(deltas.deltas[2].order.side, OrderSide::Sell);
        assert_eq!(deltas.deltas[2].order.price, Price::new(123.50, 2));
        assert_eq!(deltas.deltas[2].order.size, Quantity::new(3.0, 4));
        assert_eq!(deltas.deltas[2].flags, RecordFlag::F_LAST as u8);
        assert_eq!(
            deltas.ts_event,
            UnixNanos::from(1_700_000_000_000_000_000u64)
        );
    }

    #[rstest]
    fn test_parse_depth_snapshot_empty_returns_none() {
        let instrument = sample_instrument();
        let event = DepthSnapshotStreamEvent {
            event_time_us: 1_700_000_000_000_000,
            book_update_id: 123,
            price_exponent: -2,
            qty_exponent: -4,
            bids: vec![],
            asks: vec![],
            symbol: Ustr::from("ETHUSDT"),
        };

        let deltas = parse_depth_snapshot(&event, &instrument);

        assert!(deltas.is_none());
    }

    #[rstest]
    fn test_parse_depth_diff() {
        let instrument = sample_instrument();
        let event = DepthDiffStreamEvent {
            event_time_us: 1_700_000_000_000_000,
            first_book_update_id: 100,
            last_book_update_id: 101,
            price_exponent: -2,
            qty_exponent: -4,
            bids: vec![
                PriceLevel {
                    price_mantissa: 12_345,
                    qty_mantissa: 25_000,
                },
                PriceLevel {
                    price_mantissa: 12_340,
                    qty_mantissa: 0,
                },
            ],
            asks: vec![PriceLevel {
                price_mantissa: 12_350,
                qty_mantissa: 30_000,
            }],
            symbol: Ustr::from("ETHUSDT"),
        };

        let deltas = parse_depth_diff(&event, &instrument).unwrap();

        assert_eq!(deltas.instrument_id, instrument.id());
        assert_eq!(deltas.deltas.len(), 3);
        assert_eq!(deltas.deltas[0].action, BookAction::Update);
        assert_eq!(deltas.deltas[0].order.side, OrderSide::Buy);
        assert_eq!(deltas.deltas[1].action, BookAction::Delete);
        assert_eq!(deltas.deltas[1].order.side, OrderSide::Buy);
        assert_eq!(deltas.deltas[2].action, BookAction::Update);
        assert_eq!(deltas.deltas[2].order.side, OrderSide::Sell);
        assert_eq!(deltas.deltas[2].flags, RecordFlag::F_LAST as u8);
        assert_eq!(
            deltas.ts_event,
            UnixNanos::from(1_700_000_000_000_000_000u64)
        );
    }

    #[rstest]
    fn test_parse_depth_diff_empty_returns_none() {
        let instrument = sample_instrument();
        let event = DepthDiffStreamEvent {
            event_time_us: 1_700_000_000_000_000,
            first_book_update_id: 100,
            last_book_update_id: 101,
            price_exponent: -2,
            qty_exponent: -4,
            bids: vec![],
            asks: vec![],
            symbol: Ustr::from("ETHUSDT"),
        };

        let deltas = parse_depth_diff(&event, &instrument);

        assert!(deltas.is_none());
    }
}
