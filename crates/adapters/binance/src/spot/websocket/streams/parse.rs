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
};

use crate::common::{
    fixed::{mantissa_to_price, mantissa_to_quantity},
    sbe::stream::{
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
        template_id::TRADES_STREAM_EVENT => {
            Ok(MarketDataMessage::Trades(TradesStreamEvent::decode(buf)?))
        }
        template_id::BEST_BID_ASK_STREAM_EVENT => Ok(MarketDataMessage::BestBidAsk(
            BestBidAskStreamEvent::decode(buf)?,
        )),
        template_id::DEPTH_SNAPSHOT_STREAM_EVENT => Ok(MarketDataMessage::DepthSnapshot(
            DepthSnapshotStreamEvent::decode(buf)?,
        )),
        template_id::DEPTH_DIFF_STREAM_EVENT => Ok(MarketDataMessage::DepthDiff(
            DepthDiffStreamEvent::decode(buf)?,
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
            let price = mantissa_to_price(t.price_mantissa, event.price_exponent, price_precision);
            let size = mantissa_to_quantity(t.qty_mantissa, event.qty_exponent, size_precision);
            let ts_event = UnixNanos::from(event.transact_time_us as u64 * 1000); // us to ns

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

    let bid_price = mantissa_to_price(
        event.bid_price_mantissa,
        event.price_exponent,
        price_precision,
    );
    let bid_size = mantissa_to_quantity(event.bid_qty_mantissa, event.qty_exponent, size_precision);
    let ask_price = mantissa_to_price(
        event.ask_price_mantissa,
        event.price_exponent,
        price_precision,
    );
    let ask_size = mantissa_to_quantity(event.ask_qty_mantissa, event.qty_exponent, size_precision);
    let ts_event = UnixNanos::from(event.event_time_us as u64 * 1000); // us to ns

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
    let ts_event = UnixNanos::from(event.event_time_us as u64 * 1000);

    let mut deltas = Vec::with_capacity(event.bids.len() + event.asks.len() + 1);

    // Add clear delta first
    deltas.push(OrderBookDelta::clear(instrument_id, 0, ts_event, ts_event));

    // Add bid levels
    for (i, level) in event.bids.iter().enumerate() {
        let price = mantissa_to_price(level.price_mantissa, event.price_exponent, price_precision);
        let size = mantissa_to_quantity(level.qty_mantissa, event.qty_exponent, size_precision);
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
        let price = mantissa_to_price(level.price_mantissa, event.price_exponent, price_precision);
        let size = mantissa_to_quantity(level.qty_mantissa, event.qty_exponent, size_precision);
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
    let ts_event = UnixNanos::from(event.event_time_us as u64 * 1000);

    let mut deltas = Vec::with_capacity(event.bids.len() + event.asks.len());

    // Add bid updates
    for (i, level) in event.bids.iter().enumerate() {
        let price = mantissa_to_price(level.price_mantissa, event.price_exponent, price_precision);
        let size = mantissa_to_quantity(level.qty_mantissa, event.qty_exponent, size_precision);

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
        let price = mantissa_to_price(level.price_mantissa, event.price_exponent, price_precision);
        let size = mantissa_to_quantity(level.qty_mantissa, event.qty_exponent, size_precision);

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

    use super::*;
    use crate::common::sbe::stream::STREAM_SCHEMA_ID;

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
}
