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

//! Order book utilities shared by REST + WebSocket code paths.

use anyhow::{Context, Result};
use nautilus_core::nanos::UnixNanos;
use nautilus_model::{
    data::{BookOrder, OrderBookDelta, OrderBookDeltas, QuoteTick},
    enums::{BookAction, OrderSide, RecordFlag},
    instruments::{Instrument, InstrumentAny},
    types::{Price, Quantity},
};
use rust_decimal::{Decimal, prelude::ToPrimitive};

use super::models::LighterOrderBookDepth;

/// Convert a depth snapshot/delta payload into Nautilus order book deltas and best bid/ask quote.
///
/// The returned [`OrderBookDeltas`] always begins with a `clear` delta, allowing callers to
/// rebuild deterministic state from either REST snapshots or WebSocket subscription payloads.
pub fn depth_to_deltas_and_quote(
    depth: &LighterOrderBookDepth,
    instrument: &InstrumentAny,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
) -> Result<(OrderBookDeltas, Option<QuoteTick>)> {
    let sequence = depth.offset.unwrap_or_default();
    let mut deltas = Vec::with_capacity(depth.bids.len() + depth.asks.len() + 1);
    deltas.push(OrderBookDelta::clear(
        instrument.id(),
        sequence,
        ts_event,
        ts_init,
    ));

    let mut best_bid: Option<(Price, Quantity)> = None;
    let mut best_ask: Option<(Price, Quantity)> = None;

    for level in &depth.bids {
        let (price, size) = parse_level(level.price, level.size, instrument, "bid")?;
        if size.is_zero() {
            deltas.push(make_delta(
                instrument,
                OrderSide::Buy,
                BookAction::Delete,
                price,
                size,
                sequence,
                ts_event,
                ts_init,
            ));
            continue;
        }

        if best_bid
            .as_ref()
            .map_or(true, |(best_price, _)| price > *best_price)
        {
            best_bid = Some((price, size));
        }

        deltas.push(make_delta(
            instrument,
            OrderSide::Buy,
            BookAction::Add,
            price,
            size,
            sequence,
            ts_event,
            ts_init,
        ));
    }

    for level in &depth.asks {
        let (price, size) = parse_level(level.price, level.size, instrument, "ask")?;
        if size.is_zero() {
            deltas.push(make_delta(
                instrument,
                OrderSide::Sell,
                BookAction::Delete,
                price,
                size,
                sequence,
                ts_event,
                ts_init,
            ));
            continue;
        }

        if best_ask
            .as_ref()
            .map_or(true, |(best_price, _)| price < *best_price)
        {
            best_ask = Some((price, size));
        }

        deltas.push(make_delta(
            instrument,
            OrderSide::Sell,
            BookAction::Add,
            price,
            size,
            sequence,
            ts_event,
            ts_init,
        ));
    }

    let quote = match (best_bid, best_ask) {
        (Some((bid_price, bid_size)), Some((ask_price, ask_size))) => Some(QuoteTick::new(
            instrument.id(),
            bid_price,
            ask_price,
            bid_size,
            ask_size,
            ts_event,
            ts_init,
        )),
        _ => None,
    };

    Ok((OrderBookDeltas::new(instrument.id(), deltas), quote))
}

fn parse_level(
    price: Decimal,
    size: Decimal,
    instrument: &InstrumentAny,
    field: &str,
) -> Result<(Price, Quantity)> {
    let price_f64 = price
        .to_f64()
        .with_context(|| format!("invalid {field} price {price}"))?;
    let size_f64 = size
        .abs()
        .to_f64()
        .with_context(|| format!("invalid {field} size {size}"))?;

    Ok((
        Price::new(price_f64, instrument.price_precision()),
        Quantity::new(size_f64, instrument.size_precision()),
    ))
}

fn make_delta(
    instrument: &InstrumentAny,
    side: OrderSide,
    action: BookAction,
    price: Price,
    size: Quantity,
    sequence: u64,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
) -> OrderBookDelta {
    let order = BookOrder::new(side, price, size, 0);
    OrderBookDelta::new(
        instrument.id(),
        action,
        order,
        RecordFlag::F_LAST as u8,
        sequence,
        ts_event,
        ts_init,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use nautilus_model::{
        enums::CurrencyType,
        identifiers::{InstrumentId, Symbol},
        instruments::CryptoPerpetual,
        types::Currency,
    };
    use std::str::FromStr;

    fn dec(s: &str) -> Decimal {
        Decimal::from_str(s).expect("valid decimal")
    }

    fn create_test_instrument() -> InstrumentAny {
        let btc = Currency::new("BTC", 8, 0, "BTC", CurrencyType::Crypto);
        let usd = Currency::new("USD", 2, 0, "USD", CurrencyType::Fiat);
        let instrument_id =
            InstrumentId::new(Symbol::new("BTC-USD-PERP"), nautilus_model::identifiers::Venue::new("LIGHTER"));

        InstrumentAny::CryptoPerpetual(CryptoPerpetual::new(
            instrument_id,
            Symbol::new("BTC"),
            btc,
            usd,
            usd,
            false,
            1,  // price_precision
            4,  // size_precision
            Price::from("0.1"),
            Quantity::from("0.0001"),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            UnixNanos::default(),
            UnixNanos::default(),
        ))
    }

    fn create_test_depth(
        bids: Vec<(Decimal, Decimal)>,
        asks: Vec<(Decimal, Decimal)>,
        offset: Option<u64>,
    ) -> LighterOrderBookDepth {
        LighterOrderBookDepth {
            code: None,
            bids: bids
                .into_iter()
                .map(|(price, size)| super::super::models::LighterBookLevel { price, size })
                .collect(),
            asks: asks
                .into_iter()
                .map(|(price, size)| super::super::models::LighterBookLevel { price, size })
                .collect(),
            offset,
            nonce: None,
        }
    }

    #[test]
    fn test_depth_to_deltas_starts_with_clear() {
        let instrument = create_test_instrument();
        let depth = create_test_depth(
            vec![(dec("50000.0"), dec("1.0"))],
            vec![(dec("50001.0"), dec("1.0"))],
            Some(100),
        );

        let (deltas, _quote) =
            depth_to_deltas_and_quote(&depth, &instrument, UnixNanos::default(), UnixNanos::default())
                .expect("should parse");

        // First delta should be CLEAR
        assert!(!deltas.deltas.is_empty());
        assert_eq!(deltas.deltas[0].action, BookAction::Clear);
        assert_eq!(deltas.deltas[0].sequence, 100);
    }

    #[test]
    fn test_depth_to_deltas_creates_bids_and_asks() {
        let instrument = create_test_instrument();
        let depth = create_test_depth(
            vec![(dec("50000.0"), dec("1.0")), (dec("49999.0"), dec("2.0"))],
            vec![(dec("50001.0"), dec("0.5")), (dec("50002.0"), dec("1.5"))],
            Some(100),
        );

        let (deltas, _quote) =
            depth_to_deltas_and_quote(&depth, &instrument, UnixNanos::default(), UnixNanos::default())
                .expect("should parse");

        // 1 CLEAR + 2 bids + 2 asks = 5 deltas
        assert_eq!(deltas.deltas.len(), 5);

        // Check bid deltas (after CLEAR)
        assert_eq!(deltas.deltas[1].action, BookAction::Add);
        assert_eq!(deltas.deltas[1].order.side, OrderSide::Buy);

        // Check ask deltas
        assert_eq!(deltas.deltas[3].action, BookAction::Add);
        assert_eq!(deltas.deltas[3].order.side, OrderSide::Sell);
    }

    #[test]
    fn test_depth_to_deltas_zero_size_is_delete() {
        let instrument = create_test_instrument();
        let depth = create_test_depth(
            vec![(dec("50000.0"), dec("0.0"))], // Zero size = DELETE
            vec![(dec("50001.0"), dec("0.0"))], // Zero size = DELETE
            Some(100),
        );

        let (deltas, _quote) =
            depth_to_deltas_and_quote(&depth, &instrument, UnixNanos::default(), UnixNanos::default())
                .expect("should parse");

        // 1 CLEAR + 1 DELETE bid + 1 DELETE ask = 3 deltas
        assert_eq!(deltas.deltas.len(), 3);
        assert_eq!(deltas.deltas[1].action, BookAction::Delete);
        assert_eq!(deltas.deltas[2].action, BookAction::Delete);
    }

    #[test]
    fn test_depth_to_deltas_generates_quote() {
        let instrument = create_test_instrument();
        let depth = create_test_depth(
            vec![(dec("50000.0"), dec("1.0")), (dec("49999.0"), dec("2.0"))],
            vec![(dec("50001.0"), dec("0.5")), (dec("50002.0"), dec("1.5"))],
            Some(100),
        );

        let (_deltas, quote) =
            depth_to_deltas_and_quote(&depth, &instrument, UnixNanos::default(), UnixNanos::default())
                .expect("should parse");

        // Should have a quote with best bid/ask
        let quote = quote.expect("should have quote");
        assert_eq!(quote.bid_price, Price::from("50000.0"));
        assert_eq!(quote.ask_price, Price::from("50001.0"));
        assert_eq!(quote.bid_size, Quantity::from("1.0"));
        assert_eq!(quote.ask_size, Quantity::from("0.5"));
    }

    #[test]
    fn test_depth_to_deltas_no_quote_without_both_sides() {
        let instrument = create_test_instrument();

        // Only bids, no asks
        let depth = create_test_depth(
            vec![(dec("50000.0"), dec("1.0"))],
            vec![],
            Some(100),
        );

        let (_deltas, quote) =
            depth_to_deltas_and_quote(&depth, &instrument, UnixNanos::default(), UnixNanos::default())
                .expect("should parse");

        // No quote without both sides
        assert!(quote.is_none());
    }

    #[test]
    fn test_depth_to_deltas_empty_book() {
        let instrument = create_test_instrument();
        let depth = create_test_depth(vec![], vec![], Some(100));

        let (deltas, quote) =
            depth_to_deltas_and_quote(&depth, &instrument, UnixNanos::default(), UnixNanos::default())
                .expect("should parse");

        // Only CLEAR delta
        assert_eq!(deltas.deltas.len(), 1);
        assert_eq!(deltas.deltas[0].action, BookAction::Clear);

        // No quote
        assert!(quote.is_none());
    }

    #[test]
    fn test_depth_to_deltas_default_offset() {
        let instrument = create_test_instrument();
        let depth = create_test_depth(
            vec![(dec("50000.0"), dec("1.0"))],
            vec![(dec("50001.0"), dec("1.0"))],
            None, // No offset
        );

        let (deltas, _quote) =
            depth_to_deltas_and_quote(&depth, &instrument, UnixNanos::default(), UnixNanos::default())
                .expect("should parse");

        // Should use default (0) for sequence
        assert_eq!(deltas.deltas[0].sequence, 0);
    }
}
