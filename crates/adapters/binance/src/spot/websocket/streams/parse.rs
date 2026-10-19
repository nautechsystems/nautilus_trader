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

use crate::{
    common::parse::parse_micros_or_init,
    spot::sbe::stream::{
        BestBidAskStreamEvent, DepthDiffStreamEvent, DepthSnapshotStreamEvent, MessageHeader,
        StreamDecodeError, TradesStreamEvent, template_id,
    },
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

/// Parses a trades stream event into a vector of `TradeTick` using the supplied
/// adapter initialization timestamp.
///
/// # Errors
///
/// Returns an error if:
/// - A price is out of range or a reserved sentinel value.
/// - A quantity is negative, out of range, or zero at the instrument size precision.
pub fn parse_trades_event(
    event: &TradesStreamEvent,
    instrument: &InstrumentAny,
    ts_init: UnixNanos,
) -> anyhow::Result<Vec<Data>> {
    let instrument_id = instrument.id();
    let price_precision = instrument.price_precision();
    let size_precision = instrument.size_precision();

    let mut trades = Vec::with_capacity(event.trades.len());
    for t in &event.trades {
        let price = parse_price(t.price_mantissa, event.price_exponent, price_precision)?;
        let size = parse_quantity(t.qty_mantissa, event.qty_exponent, size_precision)?;
        let ts_event = parse_micros_or_init(
            event.transact_time_us,
            "Spot SBE stream transaction time",
            ts_init,
        );

        let trade = TradeTick::new_checked(
            instrument_id,
            price,
            size,
            if t.is_buyer_maker {
                AggressorSide::Sell
            } else {
                AggressorSide::Buy
            },
            TradeId::new(t.id.to_string()),
            ts_event,
            ts_init,
        )?;

        trades.push(Data::from(trade));
    }

    Ok(trades)
}

/// Parses a best bid/ask event into a `QuoteTick` using the supplied adapter
/// initialization timestamp.
///
/// # Errors
///
/// Returns an error if:
/// - A price is out of range or a reserved sentinel value.
/// - A quantity is negative or out of range.
pub fn parse_bbo_event(
    event: &BestBidAskStreamEvent,
    instrument: &InstrumentAny,
    ts_init: UnixNanos,
) -> anyhow::Result<QuoteTick> {
    let instrument_id = instrument.id();
    let price_precision = instrument.price_precision();
    let size_precision = instrument.size_precision();

    let bid_price = parse_price(
        event.bid_price_mantissa,
        event.price_exponent,
        price_precision,
    )?;
    let bid_size = parse_quantity(event.bid_qty_mantissa, event.qty_exponent, size_precision)?;
    let ask_price = parse_price(
        event.ask_price_mantissa,
        event.price_exponent,
        price_precision,
    )?;
    let ask_size = parse_quantity(event.ask_qty_mantissa, event.qty_exponent, size_precision)?;
    let ts_event = parse_micros_or_init(event.event_time_us, "Spot SBE BBO event time", ts_init);

    Ok(QuoteTick::new(
        instrument_id,
        bid_price,
        ask_price,
        bid_size,
        ask_size,
        ts_event,
        ts_init,
    ))
}

/// Parses a depth snapshot event into `OrderBookDeltas` using the supplied
/// adapter initialization timestamp for the aggregate and every inner delta.
///
/// A snapshot without levels clears the book.
///
/// # Errors
///
/// Returns an error if:
/// - A price is out of range or a reserved sentinel value.
/// - A quantity is negative, out of range, or zero at the instrument size precision.
pub fn parse_depth_snapshot(
    event: &DepthSnapshotStreamEvent,
    instrument: &InstrumentAny,
    ts_init: UnixNanos,
) -> anyhow::Result<OrderBookDeltas> {
    let instrument_id = instrument.id();
    let price_precision = instrument.price_precision();
    let size_precision = instrument.size_precision();
    let ts_event = parse_micros_or_init(
        event.event_time_us,
        "Spot SBE depth snapshot event time",
        ts_init,
    );
    let sequence = event.book_update_id as u64;

    let mut deltas = Vec::with_capacity(event.bids.len() + event.asks.len() + 1);

    // Add clear delta first
    deltas.push(OrderBookDelta::clear(
        instrument_id,
        sequence,
        ts_event,
        ts_init,
    ));

    for (side, levels) in [
        (OrderSide::Buy, &event.bids),
        (OrderSide::Sell, &event.asks),
    ] {
        for level in levels {
            let price = parse_price(level.price_mantissa, event.price_exponent, price_precision)?;
            let size = parse_quantity(level.qty_mantissa, event.qty_exponent, size_precision)?;
            let order = BookOrder::new(side, price, size, 0);

            deltas.push(OrderBookDelta::new_checked(
                instrument_id,
                BookAction::Add,
                order,
                RecordFlag::F_SNAPSHOT as u8,
                sequence,
                ts_event,
                ts_init,
            )?);
        }
    }

    if let Some(last) = deltas.last_mut() {
        last.flags |= RecordFlag::F_LAST as u8;
    }

    Ok(OrderBookDeltas::new(instrument_id, deltas))
}

/// Parses a depth diff event into `OrderBookDeltas` using the supplied adapter
/// initialization timestamp for the aggregate and every inner delta.
///
/// Returns `Ok(None)` if the diff contains no updates.
///
/// # Errors
///
/// Returns an error if:
/// - A price is out of range or a reserved sentinel value.
/// - A quantity is negative or out of range.
/// - A non-zero quantity rounds to zero at the instrument size precision.
pub fn parse_depth_diff(
    event: &DepthDiffStreamEvent,
    instrument: &InstrumentAny,
    ts_init: UnixNanos,
) -> anyhow::Result<Option<OrderBookDeltas>> {
    let instrument_id = instrument.id();
    let price_precision = instrument.price_precision();
    let size_precision = instrument.size_precision();
    let ts_event = parse_micros_or_init(
        event.event_time_us,
        "Spot SBE depth diff event time",
        ts_init,
    );
    let sequence = event.last_book_update_id as u64;

    let mut deltas = Vec::with_capacity(event.bids.len() + event.asks.len());

    // Add bid updates
    for (i, level) in event.bids.iter().enumerate() {
        let price = parse_price(level.price_mantissa, event.price_exponent, price_precision)?;
        let size = parse_quantity(level.qty_mantissa, event.qty_exponent, size_precision)?;

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

        deltas.push(OrderBookDelta::new_checked(
            instrument_id,
            action,
            order,
            flags,
            sequence,
            ts_event,
            ts_init,
        )?);
    }

    // Add ask updates
    for (i, level) in event.asks.iter().enumerate() {
        let price = parse_price(level.price_mantissa, event.price_exponent, price_precision)?;
        let size = parse_quantity(level.qty_mantissa, event.qty_exponent, size_precision)?;

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

        deltas.push(OrderBookDelta::new_checked(
            instrument_id,
            action,
            order,
            flags,
            sequence,
            ts_event,
            ts_init,
        )?);
    }

    if deltas.is_empty() {
        return Ok(None);
    }

    Ok(Some(OrderBookDeltas::new(instrument_id, deltas)))
}

fn parse_price(mantissa: i64, exponent: i8, precision: u8) -> anyhow::Result<Price> {
    let price = Price::from_mantissa_exponent_checked(mantissa, exponent, precision)?;
    anyhow::ensure!(
        !price.is_error() && !price.is_undefined(),
        "invalid SBE price sentinel"
    );
    Ok(price)
}

fn parse_quantity(mantissa: i64, exponent: i8, precision: u8) -> anyhow::Result<Quantity> {
    let mantissa =
        u64::try_from(mantissa).map_err(|_| anyhow::anyhow!("invalid negative SBE quantity"))?;
    Ok(Quantity::from_mantissa_exponent_checked(
        mantissa, exponent, precision,
    )?)
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
                notional_filters: Vec::new(),
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
        let ts_init = UnixNanos::from(1_800_000_000_000_000_000u64);
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

        let data = parse_trades_event(&event, &instrument, ts_init).unwrap();

        assert_eq!(data.len(), 2);
        match &data[0] {
            Data::Trade(trade) => {
                assert_eq!(trade.instrument_id, instrument.id());
                assert_eq!(trade.price, Price::from("123.45"));
                assert_eq!(trade.size, Quantity::from("2.5000"));
                assert_eq!(trade.aggressor_side, AggressorSide::Buy);
                assert_eq!(trade.trade_id, TradeId::new("1"));
                assert_eq!(
                    trade.ts_event,
                    UnixNanos::from(1_700_000_000_100_000_000u64)
                );
                assert_eq!(trade.ts_init, ts_init);
            }
            other => panic!("Expected trade data, was {other:?}"),
        }

        match &data[1] {
            Data::Trade(trade) => {
                assert_eq!(trade.instrument_id, instrument.id());
                assert_eq!(trade.price, Price::from("123.40"));
                assert_eq!(trade.size, Quantity::from("1.0000"));
                assert_eq!(trade.aggressor_side, AggressorSide::Sell);
                assert_eq!(trade.trade_id, TradeId::new("2"));
                assert_eq!(
                    trade.ts_event,
                    UnixNanos::from(1_700_000_000_100_000_000u64)
                );
                assert_eq!(trade.ts_init, ts_init);
            }
            other => panic!("Expected trade data, was {other:?}"),
        }
    }

    #[rstest]
    fn test_parse_bbo_event() {
        let instrument = sample_instrument();
        let ts_init = UnixNanos::from(1_800_000_000_000_000_000u64);
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

        let quote = parse_bbo_event(&event, &instrument, ts_init).unwrap();

        assert_eq!(quote.instrument_id, instrument.id());
        assert_eq!(quote.bid_price, Price::from("123.45"));
        assert_eq!(quote.ask_price, Price::from("123.50"));
        assert_eq!(quote.bid_size, Quantity::from("2.5000"));
        assert_eq!(quote.ask_size, Quantity::from("3.0000"));
        assert_eq!(
            quote.ts_event,
            UnixNanos::from(1_700_000_000_000_000_000u64)
        );
        assert_eq!(quote.ts_init, ts_init);
    }

    #[rstest]
    #[case::negative(-1)]
    #[case::overflow(i64::MAX)]
    fn test_parse_bbo_event_falls_back_for_invalid_timestamp(#[case] event_time_us: i64) {
        let instrument = sample_instrument();
        let event = BestBidAskStreamEvent {
            event_time_us,
            book_update_id: 123,
            price_exponent: -2,
            qty_exponent: -4,
            bid_price_mantissa: 12_345,
            bid_qty_mantissa: 25_000,
            ask_price_mantissa: 12_350,
            ask_qty_mantissa: 30_000,
            symbol: Ustr::from("ETHUSDT"),
        };

        let ts_init = UnixNanos::from(1);
        let quote = parse_bbo_event(&event, &instrument, ts_init).unwrap();

        assert_eq!(quote.ts_event, ts_init);
        assert_eq!(quote.ts_init, ts_init);
    }

    #[rstest]
    fn test_parse_depth_snapshot() {
        let instrument = sample_instrument();
        let ts_init = UnixNanos::from(1_800_000_000_000_000_000u64);
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

        let deltas = parse_depth_snapshot(&event, &instrument, ts_init).unwrap();

        assert_eq!(deltas.instrument_id, instrument.id());
        assert_eq!(deltas.deltas.len(), 3);
        assert_eq!(deltas.deltas[0].action, BookAction::Clear);
        assert_eq!(deltas.deltas[1].action, BookAction::Add);
        assert_eq!(deltas.deltas[1].order.side, OrderSide::Buy.into());
        assert_eq!(deltas.deltas[1].order.price, Price::from("123.45"));
        assert_eq!(deltas.deltas[1].order.size, Quantity::from("2.5000"));
        assert_eq!(deltas.deltas[2].action, BookAction::Add);
        assert_eq!(deltas.deltas[2].order.side, OrderSide::Sell.into());
        assert_eq!(deltas.deltas[2].order.price, Price::from("123.50"));
        assert_eq!(deltas.deltas[2].order.size, Quantity::from("3.0000"));
        assert_eq!(deltas.deltas[1].flags, RecordFlag::F_SNAPSHOT as u8);
        assert_eq!(
            deltas.deltas[2].flags,
            RecordFlag::F_SNAPSHOT as u8 | RecordFlag::F_LAST as u8
        );
        assert_eq!(deltas.deltas[0].sequence, 123);
        assert_eq!(deltas.deltas[2].sequence, 123);
        assert_eq!(
            deltas.ts_event,
            UnixNanos::from(1_700_000_000_000_000_000u64)
        );
        assert_eq!(deltas.ts_init, ts_init);
        assert!(deltas.deltas.iter().all(|delta| delta.ts_init == ts_init));
    }

    #[rstest]
    fn test_parse_depth_snapshot_empty_clears_book() {
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

        let deltas = parse_depth_snapshot(&event, &instrument, UnixNanos::from(2)).unwrap();

        assert_eq!(deltas.deltas.len(), 1);
        assert_eq!(deltas.deltas[0].action, BookAction::Clear);
        assert_eq!(deltas.deltas[0].sequence, 123);
        assert_eq!(
            deltas.deltas[0].flags,
            RecordFlag::F_SNAPSHOT as u8 | RecordFlag::F_LAST as u8
        );
    }

    #[rstest]
    fn test_parse_depth_diff() {
        let instrument = sample_instrument();
        let ts_init = UnixNanos::from(1_800_000_000_000_000_000u64);
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

        let deltas = parse_depth_diff(&event, &instrument, ts_init)
            .unwrap()
            .unwrap();

        assert_eq!(deltas.instrument_id, instrument.id());
        assert_eq!(deltas.deltas.len(), 3);
        assert_eq!(deltas.deltas[0].order.price, Price::from("123.45"));
        assert_eq!(deltas.deltas[0].order.size, Quantity::from("2.5000"));
        assert_eq!(deltas.deltas[1].order.price, Price::from("123.40"));
        assert_eq!(deltas.deltas[1].order.size, Quantity::from("0.0000"));
        assert_eq!(deltas.deltas[2].order.price, Price::from("123.50"));
        assert_eq!(deltas.deltas[2].order.size, Quantity::from("3.0000"));
        assert_eq!(deltas.deltas[0].action, BookAction::Update);
        assert_eq!(deltas.deltas[0].order.side, OrderSide::Buy.into());
        assert_eq!(deltas.deltas[1].action, BookAction::Delete);
        assert_eq!(deltas.deltas[1].order.side, OrderSide::Buy.into());
        assert_eq!(deltas.deltas[2].action, BookAction::Update);
        assert_eq!(deltas.deltas[2].order.side, OrderSide::Sell.into());
        assert_eq!(deltas.deltas[2].flags, RecordFlag::F_LAST as u8);
        assert_eq!(deltas.deltas[0].flags, 0);
        assert_eq!(deltas.deltas[1].flags, 0);

        for delta in &deltas.deltas {
            assert_eq!(delta.instrument_id, instrument.id());
            assert_eq!(delta.order.order_id, 0);
            assert_eq!(delta.sequence, 101);
            assert_eq!(
                delta.ts_event,
                UnixNanos::from(1_700_000_000_000_000_000u64)
            );
            assert_eq!(delta.ts_init, ts_init);
        }

        assert_eq!(
            deltas.ts_event,
            UnixNanos::from(1_700_000_000_000_000_000u64)
        );
        assert_eq!(deltas.ts_init, ts_init);
        assert!(deltas.deltas.iter().all(|delta| delta.ts_init == ts_init));
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

        let deltas = parse_depth_diff(&event, &instrument, UnixNanos::from(2)).unwrap();

        assert!(deltas.is_none());
    }

    #[rstest]
    #[case::price_exponent(1, 1, i8::MAX, 0)]
    #[case::qty_exponent(1, 1, 0, i8::MAX)]
    #[case::price_above_range(i64::MAX, 1, 0, 0)]
    #[case::price_below_range(i64::MIN, 1, 0, 0)]
    #[case::qty_above_range(1, i64::MAX, 0, 0)]
    #[case::negative_qty(1, -1, 0, 0)]
    #[case::minimum_qty(1, i64::MIN, 0, 0)]
    #[case::negative_qty_rounds_to_zero(1, -1, 0, i8::MIN)]
    fn test_parse_events_reject_invalid_numbers(
        #[case] price_mantissa: i64,
        #[case] qty_mantissa: i64,
        #[case] price_exponent: i8,
        #[case] qty_exponent: i8,
        #[values(false, true)] ask: bool,
    ) {
        let instrument = sample_instrument();
        let ts_init = UnixNanos::from(2);

        let level = PriceLevel {
            price_mantissa,
            qty_mantissa,
        };

        let valid = PriceLevel {
            price_mantissa: 12_345,
            qty_mantissa: 25_000,
        };

        let (bid, ask) = if ask { (valid, level) } else { (level, valid) };

        let trades = TradesStreamEvent {
            event_time_us: 1,
            transact_time_us: 1,
            price_exponent,
            qty_exponent,
            trades: vec![
                Trade {
                    id: 1,
                    price_mantissa: 0,
                    qty_mantissa: 1,
                    is_buyer_maker: false,
                },
                Trade {
                    id: 2,
                    price_mantissa,
                    qty_mantissa,
                    is_buyer_maker: true,
                },
            ],
            symbol: Ustr::from("ETHUSDT"),
        };

        let bbo = BestBidAskStreamEvent {
            event_time_us: 1,
            book_update_id: 123,
            price_exponent,
            qty_exponent,
            bid_price_mantissa: bid.price_mantissa,
            bid_qty_mantissa: bid.qty_mantissa,
            ask_price_mantissa: ask.price_mantissa,
            ask_qty_mantissa: ask.qty_mantissa,
            symbol: trades.symbol,
        };

        let snapshot = DepthSnapshotStreamEvent {
            event_time_us: 1,
            book_update_id: 123,
            price_exponent,
            qty_exponent,
            bids: vec![bid],
            asks: vec![ask],
            symbol: trades.symbol,
        };

        let diff = DepthDiffStreamEvent {
            event_time_us: 1,
            first_book_update_id: 123,
            last_book_update_id: 124,
            price_exponent,
            qty_exponent,
            bids: snapshot.bids.clone(),
            asks: snapshot.asks.clone(),
            symbol: trades.symbol,
        };

        assert!(parse_trades_event(&trades, &instrument, ts_init).is_err());
        assert!(parse_bbo_event(&bbo, &instrument, ts_init).is_err());
        assert!(parse_depth_snapshot(&snapshot, &instrument, ts_init).is_err());
        assert!(parse_depth_diff(&diff, &instrument, ts_init).is_err());
    }

    #[rstest]
    #[case::zero(0, -4)]
    #[case::rounded_to_zero(1, i8::MIN)]
    fn test_parse_events_handle_zero_size(#[case] qty_mantissa: i64, #[case] qty_exponent: i8) {
        let instrument = sample_instrument();
        let ts_init = UnixNanos::from(2);

        let trades = TradesStreamEvent {
            event_time_us: 1,
            transact_time_us: 1,
            price_exponent: -2,
            qty_exponent,
            trades: vec![Trade {
                id: 1,
                price_mantissa: 12_345,
                qty_mantissa,
                is_buyer_maker: false,
            }],
            symbol: Ustr::from("ETHUSDT"),
        };

        let snapshot = DepthSnapshotStreamEvent {
            event_time_us: 1,
            book_update_id: 123,
            price_exponent: -2,
            qty_exponent,
            bids: vec![PriceLevel {
                price_mantissa: 12_345,
                qty_mantissa,
            }],
            asks: vec![],
            symbol: trades.symbol,
        };

        let diff = DepthDiffStreamEvent {
            event_time_us: 1,
            first_book_update_id: 123,
            last_book_update_id: 124,
            price_exponent: -2,
            qty_exponent,
            bids: snapshot.bids.clone(),
            asks: vec![],
            symbol: trades.symbol,
        };

        let bbo = BestBidAskStreamEvent {
            event_time_us: 1,
            book_update_id: 123,
            price_exponent: -2,
            qty_exponent,
            bid_price_mantissa: 12_345,
            bid_qty_mantissa: qty_mantissa,
            ask_price_mantissa: 12_350,
            ask_qty_mantissa: qty_mantissa,
            symbol: trades.symbol,
        };

        assert!(parse_trades_event(&trades, &instrument, ts_init).is_err());
        assert!(parse_depth_snapshot(&snapshot, &instrument, ts_init).is_err());
        let result = parse_depth_diff(&diff, &instrument, ts_init);
        if qty_mantissa == 0 {
            let deltas = result.unwrap().unwrap();
            assert_eq!(deltas.deltas[0].action, BookAction::Delete);
            assert_eq!(
                deltas.deltas[0].order.size,
                Quantity::zero(instrument.size_precision())
            );
        } else {
            assert!(result.is_err());
        }

        let quote = parse_bbo_event(&bbo, &instrument, ts_init).unwrap();
        assert_eq!(quote.bid_size, Quantity::zero(instrument.size_precision()));
        assert_eq!(quote.ask_size, Quantity::zero(instrument.size_precision()));
    }

    #[cfg(not(feature = "high-precision"))]
    #[rstest]
    fn test_parse_price_rejects_sentinel() {
        assert!(parse_price(i64::MIN, -9, 9).is_err());
    }
}
