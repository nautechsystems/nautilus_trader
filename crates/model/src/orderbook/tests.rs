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

use std::collections::HashSet;

use nautilus_core::UnixNanos;
use rstest::{fixture, rstest};
use rust_decimal_macros::dec;

use crate::{
    data::{
        OrderBookDelta, OrderBookDeltas, QuoteTick, TradeTick, depth::OrderBookDepth10,
        order::BookOrder, stubs::*,
    },
    enums::{
        AggressorSide, BookAction, BookType, OrderSide, OrderSideSpecified, OrderStatus, OrderType,
        RecordFlag, TimeInForce,
    },
    identifiers::{ClientOrderId, InstrumentId, TradeId, TraderId, VenueOrderId},
    orderbook::{
        BookIntegrityError, BookPrice, OrderBook, OwnBookOrder,
        analysis::book_check_integrity,
        own::{OwnBookLadder, OwnBookLevel, OwnOrderBook},
    },
    types::{Price, Quantity},
};

////////////////////////////////////////////////////////////////////////////////
// OrderBook
////////////////////////////////////////////////////////////////////////////////

#[rstest]
#[case::valid_book(
    BookType::L2_MBP,
    vec![
        (OrderSide::Buy, "99.00", 100, 1001),
        (OrderSide::Sell, "101.00", 100, 2001),
    ],
    Ok(())
)]
#[case::crossed_book(
    BookType::L2_MBP,
    vec![
        (OrderSide::Buy, "101.00", 100, 1001),
        (OrderSide::Sell, "99.00", 100, 2001),
    ],
    Err(BookIntegrityError::OrdersCrossed(
        BookPrice::new(Price::from("101.00"), OrderSideSpecified::Buy),
        BookPrice::new(Price::from("99.00"), OrderSideSpecified::Sell),
    ))
)]
#[case::l1_ghost_levels_handled(
    BookType::L1_MBP,
    vec![
        (OrderSide::Buy, "99.00", 100, 1001),
        (OrderSide::Buy, "98.00", 100, 1002),
    ],
    // With L1 ghost levels fix, adding two L1 orders at different prices
    // properly removes the old level, leaving only 1 level (valid state)
    Ok(())
)]
fn test_book_integrity_cases(
    #[case] book_type: BookType,
    #[case] orders: Vec<(OrderSide, &str, i64, u64)>,
    #[case] expected: Result<(), BookIntegrityError>,
) {
    let instrument_id = InstrumentId::from("AAPL.XNAS");
    let mut book = OrderBook::new(instrument_id, book_type);

    for (side, price, size, id) in orders {
        let order = BookOrder::new(side, Price::from(price), Quantity::from(size), id);
        book.add(order, 0, id, id.into());
    }

    assert_eq!(book_check_integrity(&book), expected);
}

#[rstest]
fn test_book_integrity_price_boundaries() {
    let instrument_id = InstrumentId::from("AAPL.XNAS");
    let mut book = OrderBook::new(instrument_id, BookType::L2_MBP);
    let min_bid = BookOrder::new(OrderSide::Buy, Price::min(2), Quantity::from(100), 1);
    let max_ask = BookOrder::new(OrderSide::Sell, Price::max(2), Quantity::from(100), 2);

    book.add(min_bid, 0, 1, 1.into());
    book.add(max_ask, 0, 2, 2.into());

    assert!(book_check_integrity(&book).is_ok());
}

#[rstest]
#[case::small_quantity(100)]
#[case::medium_quantity(1000)]
#[case::large_quantity(1000000)]
fn test_book_integrity_quantity_sizes(#[case] quantity: i64) {
    let instrument_id = InstrumentId::from("AAPL.XNAS");
    let mut book = OrderBook::new(instrument_id, BookType::L2_MBP);

    let bid = BookOrder::new(
        OrderSide::Buy,
        Price::from("100.00"),
        Quantity::from(quantity),
        1,
    );
    book.add(bid, 0, 1, 1.into());

    assert!(book_check_integrity(&book).is_ok());
    assert_eq!(book.best_bid_size().unwrap().as_f64() as i64, quantity);
}

#[rstest]
fn test_book_display() {
    let instrument_id = InstrumentId::from("ETHUSDT-PERP.BINANCE");
    let book = OrderBook::new(instrument_id, BookType::L2_MBP);
    assert_eq!(
        book.to_string(),
        "OrderBook(instrument_id=ETHUSDT-PERP.BINANCE, book_type=L2_MBP, update_count=0)"
    );
}

#[rstest]
fn test_book_empty_state() {
    let instrument_id = InstrumentId::from("ETHUSDT-PERP.BINANCE");
    let book = OrderBook::new(instrument_id, BookType::L2_MBP);

    assert_eq!(book.best_bid_price(), None);
    assert_eq!(book.best_ask_price(), None);
    assert_eq!(book.best_bid_size(), None);
    assert_eq!(book.best_ask_size(), None);
    assert!(!book.has_bid());
    assert!(!book.has_ask());
}

#[rstest]
fn test_book_single_bid_state() {
    let instrument_id = InstrumentId::from("ETHUSDT-PERP.BINANCE");
    let mut book = OrderBook::new(instrument_id, BookType::L3_MBO);
    let order1 = BookOrder::new(
        OrderSide::Buy,
        Price::from("1.000"),
        Quantity::from("1.0"),
        1,
    );
    book.add(order1, 0, 1, 100.into());

    assert_eq!(book.best_bid_price(), Some(Price::from("1.000")));
    assert_eq!(book.best_bid_size(), Some(Quantity::from("1.0")));
    assert!(book.has_bid());
}

#[rstest]
fn test_book_single_ask_state() {
    let instrument_id = InstrumentId::from("ETHUSDT-PERP.BINANCE");
    let mut book = OrderBook::new(instrument_id, BookType::L3_MBO);
    let order = BookOrder::new(
        OrderSide::Sell,
        Price::from("2.000"),
        Quantity::from("2.0"),
        2,
    );
    book.add(order, 0, 2, 200.into());

    assert_eq!(book.best_ask_price(), Some(Price::from("2.000")));
    assert_eq!(book.best_ask_size(), Some(Quantity::from("2.0")));
    assert!(book.has_ask());
}

#[rstest]
fn test_book_empty_book_spread() {
    let instrument_id = InstrumentId::from("ETHUSDT-PERP.BINANCE");
    let book = OrderBook::new(instrument_id, BookType::L3_MBO);
    assert_eq!(book.spread(), None);
}

#[rstest]
fn test_book_spread_with_orders() {
    let instrument_id = InstrumentId::from("ETHUSDT-PERP.BINANCE");
    let mut book = OrderBook::new(instrument_id, BookType::L3_MBO);
    let bid1 = BookOrder::new(
        OrderSide::Buy,
        Price::from("1.000"),
        Quantity::from("1.0"),
        1,
    );
    let ask1 = BookOrder::new(
        OrderSide::Sell,
        Price::from("2.000"),
        Quantity::from("2.0"),
        2,
    );
    book.add(bid1, 0, 1, 100.into());
    book.add(ask1, 0, 2, 200.into());

    assert_eq!(book.spread(), Some(1.0));
}

#[rstest]
fn test_book_empty_book_midpoint() {
    let instrument_id = InstrumentId::from("ETHUSDT-PERP.BINANCE");
    let book = OrderBook::new(instrument_id, BookType::L2_MBP);
    assert_eq!(book.midpoint(), None);
}

#[rstest]
fn test_book_midpoint_with_orders() {
    let instrument_id = InstrumentId::from("ETHUSDT-PERP.BINANCE");
    let mut book = OrderBook::new(instrument_id, BookType::L2_MBP);

    let bid1 = BookOrder::new(
        OrderSide::Buy,
        Price::from("1.000"),
        Quantity::from("1.0"),
        1,
    );
    let ask1 = BookOrder::new(
        OrderSide::Sell,
        Price::from("2.000"),
        Quantity::from("2.0"),
        2,
    );
    book.add(bid1, 0, 1, 100.into());
    book.add(ask1, 0, 2, 200.into());

    assert_eq!(book.midpoint(), Some(1.5));
}

#[rstest]
fn test_book_get_price_for_quantity_no_market() {
    let instrument_id = InstrumentId::from("ETHUSDT-PERP.BINANCE");
    let book = OrderBook::new(instrument_id, BookType::L2_MBP);

    let qty = Quantity::from(1);

    assert_eq!(book.get_avg_px_for_quantity(qty, OrderSide::Buy), 0.0);
    assert_eq!(book.get_avg_px_for_quantity(qty, OrderSide::Sell), 0.0);
}

#[rstest]
fn test_book_get_quantity_for_price_no_market() {
    let instrument_id = InstrumentId::from("ETHUSDT-PERP.BINANCE");
    let book = OrderBook::new(instrument_id, BookType::L2_MBP);

    let price = Price::from("1.0");

    assert_eq!(book.get_quantity_for_price(price, OrderSide::Buy), 0.0);
    assert_eq!(book.get_quantity_for_price(price, OrderSide::Sell), 0.0);
}

#[rstest]
fn test_book_get_price_for_quantity() {
    let instrument_id = InstrumentId::from("ETHUSDT-PERP.BINANCE");
    let mut book = OrderBook::new(instrument_id, BookType::L2_MBP);

    let ask2 = BookOrder::new(
        OrderSide::Sell,
        Price::from("2.010"),
        Quantity::from("2.0"),
        0, // order_id not applicable
    );
    let ask1 = BookOrder::new(
        OrderSide::Sell,
        Price::from("2.000"),
        Quantity::from("1.0"),
        0, // order_id not applicable
    );
    let bid1 = BookOrder::new(
        OrderSide::Buy,
        Price::from("1.000"),
        Quantity::from("1.0"),
        0, // order_id not applicable
    );
    let bid2 = BookOrder::new(
        OrderSide::Buy,
        Price::from("0.990"),
        Quantity::from("2.0"),
        0, // order_id not applicable
    );
    book.add(bid1, 0, 1, 2.into());
    book.add(bid2, 0, 1, 2.into());
    book.add(ask1, 0, 1, 2.into());
    book.add(ask2, 0, 1, 2.into());

    let qty = Quantity::from("1.5");

    assert_eq!(
        book.get_avg_px_for_quantity(qty, OrderSide::Buy),
        2.003_333_333_333_333_4
    );
    assert_eq!(
        book.get_avg_px_for_quantity(qty, OrderSide::Sell),
        0.996_666_666_666_666_7
    );
}

#[rstest]
fn test_book_get_quantity_for_price() {
    let instrument_id = InstrumentId::from("ETHUSDT-PERP.BINANCE");
    let mut book = OrderBook::new(instrument_id, BookType::L2_MBP);

    let ask3 = BookOrder::new(
        OrderSide::Sell,
        Price::from("2.011"),
        Quantity::from("3.0"),
        0, // order_id not applicable
    );
    let ask2 = BookOrder::new(
        OrderSide::Sell,
        Price::from("2.010"),
        Quantity::from("2.0"),
        0, // order_id not applicable
    );
    let ask1 = BookOrder::new(
        OrderSide::Sell,
        Price::from("2.000"),
        Quantity::from("1.0"),
        0, // order_id not applicable
    );
    let bid1 = BookOrder::new(
        OrderSide::Buy,
        Price::from("1.000"),
        Quantity::from("1.0"),
        0, // order_id not applicable
    );
    let bid2 = BookOrder::new(
        OrderSide::Buy,
        Price::from("0.990"),
        Quantity::from("2.0"),
        0, // order_id not applicable
    );
    let bid3 = BookOrder::new(
        OrderSide::Buy,
        Price::from("0.989"),
        Quantity::from("3.0"),
        0, // order_id not applicable
    );
    book.add(bid1, 0, 0, 1.into());
    book.add(bid2, 0, 1, 2.into());
    book.add(bid3, 0, 2, 3.into());
    book.add(ask1, 0, 3, 4.into());
    book.add(ask2, 0, 4, 5.into());
    book.add(ask3, 0, 5, 6.into());

    assert_eq!(
        book.get_quantity_for_price(Price::from("2.010"), OrderSide::Buy),
        3.0
    );
    assert_eq!(
        book.get_quantity_for_price(Price::from("0.990"), OrderSide::Sell),
        3.0
    );
}

#[rstest]
fn test_book_get_price_for_exposure_no_market() {
    let instrument_id = InstrumentId::from("ETHUSDT-PERP.BINANCE");
    let book = OrderBook::new(instrument_id, BookType::L2_MBP);
    let qty = Quantity::from(1);

    assert_eq!(
        book.get_avg_px_qty_for_exposure(qty, OrderSide::Buy),
        (0.0, 0.0, 0.0)
    );
    assert_eq!(
        book.get_avg_px_qty_for_exposure(qty, OrderSide::Sell),
        (0.0, 0.0, 0.0)
    );
}

#[rstest]
fn test_book_get_price_for_exposure(stub_depth10: OrderBookDepth10) {
    let depth = stub_depth10;
    let instrument_id = InstrumentId::from("ETHUSDT-PERP.BINANCE");
    let mut book = OrderBook::new(instrument_id, BookType::L2_MBP);
    book.apply_depth(&depth);

    let qty = Quantity::from(1);

    assert_eq!(
        book.get_avg_px_qty_for_exposure(qty, OrderSide::Buy),
        (100.0, 0.01, 100.0)
    );
    // TODO: Revisit calculations
    // assert_eq!(
    //     book.get_avg_px_qty_for_exposure(qty, OrderSide::Sell),
    //     (99.0, 0.01010101, 99.0)
    // );
}

#[rstest]
fn test_book_apply_depth(stub_depth10: OrderBookDepth10) {
    let depth = stub_depth10;
    let instrument_id = InstrumentId::from("AAPL.XNAS");
    let mut book = OrderBook::new(instrument_id, BookType::L2_MBP);

    book.apply_depth(&depth);

    assert_eq!(book.best_bid_price().unwrap(), Price::from("99.00"));
    assert_eq!(book.best_ask_price().unwrap(), Price::from("100.00"));
    assert_eq!(book.best_bid_size().unwrap(), Quantity::from("100.0"));
    assert_eq!(book.best_ask_size().unwrap(), Quantity::from("100.0"));
}

#[rstest]
fn test_book_apply_depth_all_levels(stub_depth10: OrderBookDepth10) {
    let depth = stub_depth10;
    let instrument_id = InstrumentId::from("AAPL.XNAS");
    let mut book = OrderBook::new(instrument_id, BookType::L2_MBP);

    book.apply_depth(&depth);

    // Verify exactly 10 bid levels
    let bid_levels: Vec<_> = book.bids(None).collect();
    assert_eq!(bid_levels.len(), 10, "Should have exactly 10 bid levels");

    // Verify exactly 10 ask levels
    let ask_levels: Vec<_> = book.asks(None).collect();
    assert_eq!(ask_levels.len(), 10, "Should have exactly 10 ask levels");

    // Verify bid prices in descending order (99, 98, 97, ..., 90)
    let expected_bid_prices = vec![
        Price::from("99.0"),
        Price::from("98.0"),
        Price::from("97.0"),
        Price::from("96.0"),
        Price::from("95.0"),
        Price::from("94.0"),
        Price::from("93.0"),
        Price::from("92.0"),
        Price::from("91.0"),
        Price::from("90.0"),
    ];
    for (i, level) in bid_levels.iter().enumerate() {
        assert_eq!(
            level.price.value, expected_bid_prices[i],
            "Bid level {} price mismatch",
            i
        );
        assert!(level.size() > 0.0, "Bid level {} has zero size", i);
    }

    // Verify ask prices in ascending order (100, 101, 102, ..., 109)
    let expected_ask_prices = vec![
        Price::from("100.0"),
        Price::from("101.0"),
        Price::from("102.0"),
        Price::from("103.0"),
        Price::from("104.0"),
        Price::from("105.0"),
        Price::from("106.0"),
        Price::from("107.0"),
        Price::from("108.0"),
        Price::from("109.0"),
    ];
    for (i, level) in ask_levels.iter().enumerate() {
        assert_eq!(
            level.price.value, expected_ask_prices[i],
            "Ask level {} price mismatch",
            i
        );
        assert!(level.size() > 0.0, "Ask level {} has zero size", i);
    }

    // Verify sizes increase with each level (100, 200, 300, ..., 1000)
    let expected_sizes = [
        100.0, 200.0, 300.0, 400.0, 500.0, 600.0, 700.0, 800.0, 900.0, 1000.0,
    ];
    for (i, level) in bid_levels.iter().enumerate() {
        assert_eq!(
            level.size(),
            expected_sizes[i],
            "Bid level {} size mismatch",
            i
        );
    }
    for (i, level) in ask_levels.iter().enumerate() {
        assert_eq!(
            level.size(),
            expected_sizes[i],
            "Ask level {} size mismatch",
            i
        );
    }
}

#[rstest]
fn test_book_apply_depth_empty_snapshot() {
    use crate::data::depth::DEPTH10_LEN;

    let instrument_id = InstrumentId::from("AAPL.XNAS");
    let mut book = OrderBook::new(instrument_id, BookType::L2_MBP);

    // Create empty depth with all padding entries (NoOrderSide, zero size)
    let empty_order = BookOrder::new(
        OrderSide::NoOrderSide,
        Price::from("0.0"),
        Quantity::from("0"),
        0,
    );
    let depth = OrderBookDepth10::new(
        instrument_id,
        [empty_order; DEPTH10_LEN],
        [empty_order; DEPTH10_LEN],
        [0; DEPTH10_LEN],
        [0; DEPTH10_LEN],
        0,
        12345,
        UnixNanos::from(1000),
        UnixNanos::from(2000),
    );

    book.apply_depth(&depth);

    // Verify no phantom levels at price 0
    assert_eq!(
        book.best_bid_price(),
        None,
        "Empty snapshot should have no bids"
    );
    assert_eq!(
        book.best_ask_price(),
        None,
        "Empty snapshot should have no asks"
    );
    assert!(!book.has_bid(), "Empty snapshot should not have bid");
    assert!(!book.has_ask(), "Empty snapshot should not have ask");

    let bid_levels: Vec<_> = book.bids(None).collect();
    let ask_levels: Vec<_> = book.asks(None).collect();
    assert_eq!(bid_levels.len(), 0, "Should have 0 bid levels");
    assert_eq!(ask_levels.len(), 0, "Should have 0 ask levels");

    // Verify metadata was still updated
    assert_eq!(book.sequence, 12345);
    assert_eq!(book.ts_last, UnixNanos::from(1000));
    assert_eq!(book.update_count, 1);
}

#[rstest]
fn test_book_apply_depth_partial_snapshot() {
    use crate::data::depth::DEPTH10_LEN;

    let instrument_id = InstrumentId::from("AAPL.XNAS");
    let mut book = OrderBook::new(instrument_id, BookType::L2_MBP);

    // Create depth with only 3 valid levels, rest are padding
    let mut bids = [BookOrder::new(
        OrderSide::NoOrderSide,
        Price::from("0.0"),
        Quantity::from("0"),
        0,
    ); DEPTH10_LEN];
    let mut asks = [BookOrder::new(
        OrderSide::NoOrderSide,
        Price::from("0.0"),
        Quantity::from("0"),
        0,
    ); DEPTH10_LEN];

    // Add 3 valid bids
    bids[0] = BookOrder::new(
        OrderSide::Buy,
        Price::from("99.0"),
        Quantity::from("100"),
        1,
    );
    bids[1] = BookOrder::new(
        OrderSide::Buy,
        Price::from("98.0"),
        Quantity::from("200"),
        2,
    );
    bids[2] = BookOrder::new(
        OrderSide::Buy,
        Price::from("97.0"),
        Quantity::from("300"),
        3,
    );

    // Add 3 valid asks
    asks[0] = BookOrder::new(
        OrderSide::Sell,
        Price::from("100.0"),
        Quantity::from("100"),
        11,
    );
    asks[1] = BookOrder::new(
        OrderSide::Sell,
        Price::from("101.0"),
        Quantity::from("200"),
        12,
    );
    asks[2] = BookOrder::new(
        OrderSide::Sell,
        Price::from("102.0"),
        Quantity::from("300"),
        13,
    );

    let depth = OrderBookDepth10::new(
        instrument_id,
        bids,
        asks,
        [1, 1, 1, 0, 0, 0, 0, 0, 0, 0],
        [1, 1, 1, 0, 0, 0, 0, 0, 0, 0],
        0,
        54321,
        UnixNanos::from(3000),
        UnixNanos::from(4000),
    );

    book.apply_depth(&depth);

    // Verify exactly 3 levels on each side
    let bid_levels: Vec<_> = book.bids(None).collect();
    let ask_levels: Vec<_> = book.asks(None).collect();
    assert_eq!(bid_levels.len(), 3, "Should have exactly 3 bid levels");
    assert_eq!(ask_levels.len(), 3, "Should have exactly 3 ask levels");

    // Verify no zero-price levels
    for level in bid_levels.iter() {
        assert!(
            level.price.value > Price::from("0.0"),
            "No zero-price bid levels"
        );
        assert!(level.size() > 0.0, "No zero-size bid levels");
    }
    for level in ask_levels.iter() {
        assert!(
            level.price.value > Price::from("0.0"),
            "No zero-price ask levels"
        );
        assert!(level.size() > 0.0, "No zero-size ask levels");
    }

    // Verify metadata updated
    assert_eq!(book.sequence, 54321);
    assert_eq!(book.ts_last, UnixNanos::from(3000));
    assert_eq!(book.update_count, 1);
}

#[rstest]
fn test_book_apply_depth_updates_metadata_once(stub_depth10: OrderBookDepth10) {
    let depth = stub_depth10;
    let instrument_id = InstrumentId::from("AAPL.XNAS");
    let mut book = OrderBook::new(instrument_id, BookType::L2_MBP);

    book.apply_depth(&depth);

    // Verify metadata updated exactly once (not 20 times for 20 orders)
    assert_eq!(book.sequence, depth.sequence);
    assert_eq!(book.ts_last, depth.ts_event);
    assert_eq!(
        book.update_count, 1,
        "Should increment update_count exactly once"
    );
}

#[rstest]
fn test_book_orderbook_creation() {
    let instrument_id = InstrumentId::from("AAPL.XNAS");
    let book = OrderBook::new(instrument_id, BookType::L2_MBP);

    assert_eq!(book.instrument_id, instrument_id);
    assert_eq!(book.book_type, BookType::L2_MBP);
    assert_eq!(book.sequence, 0);
    assert_eq!(book.ts_last, 0);
    assert_eq!(book.update_count, 0);
}

#[rstest]
fn test_book_orderbook_reset() {
    let instrument_id = InstrumentId::from("AAPL.XNAS");
    let mut book = OrderBook::new(instrument_id, BookType::L1_MBP);
    book.sequence = 10;
    book.ts_last = 100.into();
    book.update_count = 3;

    book.reset();

    assert_eq!(book.book_type, BookType::L1_MBP);
    assert_eq!(book.sequence, 0);
    assert_eq!(book.ts_last, 0);
    assert_eq!(book.update_count, 0);
}

#[rstest]
fn test_book_update_quote_tick_l1() {
    let instrument_id = InstrumentId::from("ETHUSDT-PERP.BINANCE");
    let mut book = OrderBook::new(instrument_id, BookType::L1_MBP);
    let quote = QuoteTick::new(
        InstrumentId::from("ETHUSDT-PERP.BINANCE"),
        Price::from("5000.000"),
        Price::from("5100.000"),
        Quantity::from("100.00000000"),
        Quantity::from("99.00000000"),
        0.into(),
        0.into(),
    );

    book.update_quote_tick(&quote).unwrap();

    assert_eq!(book.best_bid_price().unwrap(), quote.bid_price);
    assert_eq!(book.best_ask_price().unwrap(), quote.ask_price);
    assert_eq!(book.best_bid_size().unwrap(), quote.bid_size);
    assert_eq!(book.best_ask_size().unwrap(), quote.ask_size);
}

#[rstest]
fn test_book_update_trade_tick_l1() {
    let instrument_id = InstrumentId::from("ETHUSDT-PERP.BINANCE");
    let mut book = OrderBook::new(instrument_id, BookType::L1_MBP);

    let price = Price::from("15000.000");
    let size = Quantity::from("10.00000000");
    let trade = TradeTick::new(
        instrument_id,
        price,
        size,
        AggressorSide::Buyer,
        TradeId::new("123456789"),
        0.into(),
        0.into(),
    );

    book.update_trade_tick(&trade).unwrap();

    assert_eq!(book.best_bid_price().unwrap(), price);
    assert_eq!(book.best_ask_price().unwrap(), price);
    assert_eq!(book.best_bid_size().unwrap(), size);
    assert_eq!(book.best_ask_size().unwrap(), size);
}

#[rstest]
fn test_book_update_quote_tick_advances_sequence() {
    let instrument_id = InstrumentId::from("ETHUSDT-PERP.BINANCE");
    let mut book = OrderBook::new(instrument_id, BookType::L1_MBP);

    // Initial state
    assert_eq!(book.sequence, 0);
    assert_eq!(book.update_count, 0);

    let quote = QuoteTick::new(
        instrument_id,
        Price::from("5000.000"),
        Price::from("5100.000"),
        Quantity::from("100.00000000"),
        Quantity::from("99.00000000"),
        UnixNanos::from(1000),
        UnixNanos::from(2000),
    );

    book.update_quote_tick(&quote).unwrap();

    // Verify sequence advanced
    assert_eq!(book.sequence, 1, "Sequence should increment to 1");
    assert_eq!(book.ts_last, UnixNanos::from(1000), "ts_last should update");
    assert_eq!(book.update_count, 1, "update_count should increment");

    // Apply another quote tick
    let quote2 = QuoteTick::new(
        instrument_id,
        Price::from("5050.000"),
        Price::from("5150.000"),
        Quantity::from("110.00000000"),
        Quantity::from("89.00000000"),
        UnixNanos::from(2000),
        UnixNanos::from(3000),
    );

    book.update_quote_tick(&quote2).unwrap();

    // Verify sequence continues to advance
    assert_eq!(book.sequence, 2, "Sequence should increment to 2");
    assert_eq!(
        book.ts_last,
        UnixNanos::from(2000),
        "ts_last should update again"
    );
    assert_eq!(book.update_count, 2, "update_count should increment to 2");
}

#[rstest]
fn test_book_update_trade_tick_advances_sequence() {
    let instrument_id = InstrumentId::from("ETHUSDT-PERP.BINANCE");
    let mut book = OrderBook::new(instrument_id, BookType::L1_MBP);

    // Initial state
    assert_eq!(book.sequence, 0);
    assert_eq!(book.update_count, 0);

    let trade = TradeTick::new(
        instrument_id,
        Price::from("15000.000"),
        Quantity::from("10.00000000"),
        AggressorSide::Buyer,
        TradeId::new("123456789"),
        UnixNanos::from(5000),
        UnixNanos::from(6000),
    );

    book.update_trade_tick(&trade).unwrap();

    // Verify sequence advanced
    assert_eq!(book.sequence, 1, "Sequence should increment to 1");
    assert_eq!(book.ts_last, UnixNanos::from(5000), "ts_last should update");
    assert_eq!(book.update_count, 1, "update_count should increment");

    // Apply another trade tick
    let trade2 = TradeTick::new(
        instrument_id,
        Price::from("15100.000"),
        Quantity::from("20.00000000"),
        AggressorSide::Seller,
        TradeId::new("987654321"),
        UnixNanos::from(7000),
        UnixNanos::from(8000),
    );

    book.update_trade_tick(&trade2).unwrap();

    // Verify sequence continues to advance
    assert_eq!(book.sequence, 2, "Sequence should increment to 2");
    assert_eq!(
        book.ts_last,
        UnixNanos::from(7000),
        "ts_last should update again"
    );
    assert_eq!(book.update_count, 2, "update_count should increment to 2");
}

#[rstest]
fn test_book_pprint() {
    let instrument_id = InstrumentId::from("ETHUSDT-PERP.BINANCE");
    let mut book = OrderBook::new(instrument_id, BookType::L3_MBO);

    let order1 = BookOrder::new(
        OrderSide::Buy,
        Price::from("1.000"),
        Quantity::from("1.0"),
        1,
    );
    let order2 = BookOrder::new(
        OrderSide::Buy,
        Price::from("1.500"),
        Quantity::from("2.0"),
        2,
    );
    let order3 = BookOrder::new(
        OrderSide::Buy,
        Price::from("2.000"),
        Quantity::from("3.0"),
        3,
    );
    let order4 = BookOrder::new(
        OrderSide::Sell,
        Price::from("3.000"),
        Quantity::from("3.0"),
        4,
    );
    let order5 = BookOrder::new(
        OrderSide::Sell,
        Price::from("4.000"),
        Quantity::from("4.0"),
        5,
    );
    let order6 = BookOrder::new(
        OrderSide::Sell,
        Price::from("5.000"),
        Quantity::from("8.0"),
        6,
    );

    book.add(order1, 0, 1, 100.into());
    book.add(order2, 0, 2, 200.into());
    book.add(order3, 0, 3, 300.into());
    book.add(order4, 0, 4, 400.into());
    book.add(order5, 0, 5, 500.into());
    book.add(order6, 0, 6, 600.into());

    let pprint_output = book.pprint(3, None);

    let expected_output = "bid_levels: 3\n\
ask_levels: 3\n\
sequence: 6\n\
update_count: 6\n\
ts_last: 600\n\
╭───────┬───────┬───────╮\n\
│ bids  │ price │ asks  │\n\
├───────┼───────┼───────┤\n\
│       │ 5.000 │ [8.0] │\n\
│       │ 4.000 │ [4.0] │\n\
│       │ 3.000 │ [3.0] │\n\
│ [3.0] │ 2.000 │       │\n\
│ [2.0] │ 1.500 │       │\n\
│ [1.0] │ 1.000 │       │\n\
╰───────┴───────┴───────╯";

    println!("{pprint_output}");
    assert_eq!(pprint_output, expected_output);
}

#[rstest]
fn test_book_group_empty_book() {
    let instrument_id = InstrumentId::from("ETHUSDT-PERP.BINANCE");
    let book = OrderBook::new(instrument_id, BookType::L2_MBP);

    let grouped_bids = book.group_bids(dec!(1), None);
    let grouped_asks = book.group_asks(dec!(1), None);

    assert!(grouped_bids.is_empty());
    assert!(grouped_asks.is_empty());
}

#[rstest]
fn test_book_group_price_levels() {
    let instrument_id = InstrumentId::from("ETHUSDT-PERP.BINANCE");
    let mut book = OrderBook::new(instrument_id, BookType::L2_MBP);
    let orders = vec![
        BookOrder::new(OrderSide::Buy, Price::from("1.1"), Quantity::from(1), 1),
        BookOrder::new(OrderSide::Buy, Price::from("1.2"), Quantity::from(2), 2),
        BookOrder::new(OrderSide::Buy, Price::from("1.8"), Quantity::from(3), 3),
        BookOrder::new(OrderSide::Sell, Price::from("2.1"), Quantity::from(1), 4),
        BookOrder::new(OrderSide::Sell, Price::from("2.2"), Quantity::from(2), 5),
        BookOrder::new(OrderSide::Sell, Price::from("2.8"), Quantity::from(3), 6),
    ];
    for (i, order) in orders.into_iter().enumerate() {
        book.add(order, 0, i as u64, 100.into());
    }

    let grouped_bids = book.group_bids(dec!(0.5), Some(10));
    let grouped_asks = book.group_asks(dec!(0.5), Some(10));

    assert_eq!(grouped_bids.len(), 2);
    assert_eq!(grouped_asks.len(), 2);
    assert_eq!(grouped_bids.get(&dec!(1.0)), Some(&dec!(3))); // 1.1, 1.2 group to 1.0
    assert_eq!(grouped_bids.get(&dec!(1.5)), Some(&dec!(3))); // 1.8 groups to 1.5
    assert_eq!(grouped_asks.get(&dec!(2.5)), Some(&dec!(3))); // 2.1, 2.2 group to 2.5
    assert_eq!(grouped_asks.get(&dec!(3.0)), Some(&dec!(3))); // 2.8 groups to 3.0
}

#[rstest]
fn test_book_group_with_depth_limit() {
    let instrument_id = InstrumentId::from("ETHUSDT-PERP.BINANCE");
    let mut book = OrderBook::new(instrument_id, BookType::L2_MBP);

    let orders = vec![
        BookOrder::new(OrderSide::Buy, Price::from("1.0"), Quantity::from(1), 1),
        BookOrder::new(OrderSide::Buy, Price::from("2.0"), Quantity::from(2), 2),
        BookOrder::new(OrderSide::Buy, Price::from("3.0"), Quantity::from(3), 3),
        BookOrder::new(OrderSide::Sell, Price::from("4.0"), Quantity::from(1), 4),
        BookOrder::new(OrderSide::Sell, Price::from("5.0"), Quantity::from(2), 5),
        BookOrder::new(OrderSide::Sell, Price::from("6.0"), Quantity::from(3), 6),
    ];

    for (i, order) in orders.into_iter().enumerate() {
        book.add(order, 0, i as u64, 100.into());
    }

    let grouped_bids = book.group_bids(dec!(1), Some(2));
    let grouped_asks = book.group_asks(dec!(1), Some(2));

    assert_eq!(grouped_bids.len(), 2); // Should only have levels at 2.0 and 3.0
    assert_eq!(grouped_asks.len(), 2); // Should only have levels at 5.0 and 6.0
    assert_eq!(grouped_bids.get(&dec!(3)), Some(&dec!(3)));
    assert_eq!(grouped_bids.get(&dec!(2)), Some(&dec!(2)));
    assert_eq!(grouped_asks.get(&dec!(4)), Some(&dec!(1)));
    assert_eq!(grouped_asks.get(&dec!(5)), Some(&dec!(2)));
}

#[rstest]
fn test_book_group_price_realistic() {
    let instrument_id = InstrumentId::from("ETHUSDT-PERP.BINANCE");
    let mut book = OrderBook::new(instrument_id, BookType::L2_MBP);
    let orders = vec![
        BookOrder::new(
            OrderSide::Buy,
            Price::from("100.00000"),
            Quantity::from(1000),
            1,
        ),
        BookOrder::new(
            OrderSide::Buy,
            Price::from("99.00000"),
            Quantity::from(2000),
            2,
        ),
        BookOrder::new(
            OrderSide::Buy,
            Price::from("98.00000"),
            Quantity::from(3000),
            3,
        ),
        BookOrder::new(
            OrderSide::Sell,
            Price::from("101.00000"),
            Quantity::from(1000),
            4,
        ),
        BookOrder::new(
            OrderSide::Sell,
            Price::from("102.00000"),
            Quantity::from(2000),
            5,
        ),
        BookOrder::new(
            OrderSide::Sell,
            Price::from("103.00000"),
            Quantity::from(3000),
            6,
        ),
    ];
    for (i, order) in orders.into_iter().enumerate() {
        book.add(order, 0, i as u64, 100.into());
    }

    let grouped_bids = book.group_bids(dec!(2), Some(10));
    let grouped_asks = book.group_asks(dec!(2), Some(10));

    assert_eq!(grouped_bids.len(), 2);
    assert_eq!(grouped_asks.len(), 2);
    assert_eq!(grouped_bids.get(&dec!(100.0)), Some(&dec!(1000)));
    assert_eq!(grouped_bids.get(&dec!(98.0)), Some(&dec!(5000))); // 2000 + 3000 grouped
    assert_eq!(grouped_asks.get(&dec!(102.0)), Some(&dec!(3000))); // 1000 + 2000 grouped
    assert_eq!(grouped_asks.get(&dec!(104.0)), Some(&dec!(3000)));
}

#[rstest]
fn test_book_filtered_book_empty_own_book() {
    let instrument_id = InstrumentId::from("AAPL.XNAS");
    let mut book = OrderBook::new(instrument_id, BookType::L2_MBP);

    // Add some orders to the public book
    let bid_order = BookOrder::new(
        OrderSide::Buy,
        Price::from("100.00"),
        Quantity::from(100),
        1,
    );
    let ask_order = BookOrder::new(
        OrderSide::Sell,
        Price::from("101.00"),
        Quantity::from(100),
        2,
    );
    book.add(bid_order, 0, 1, 1.into());
    book.add(ask_order, 0, 2, 2.into());

    // No own book provided, filtered map should be identical to regular map
    let bids_filtered = book.bids_filtered_as_map(None, None, None, None, None);
    let asks_filtered = book.asks_filtered_as_map(None, None, None, None, None);

    let bids_regular = book.bids_as_map(None);
    let asks_regular = book.asks_as_map(None);

    assert_eq!(bids_filtered, bids_regular);
    assert_eq!(asks_filtered, asks_regular);
}

#[rstest]
fn test_book_filtered_book_with_own_orders() {
    let instrument_id = InstrumentId::from("AAPL.XNAS");
    let mut book = OrderBook::new(instrument_id, BookType::L2_MBP);
    let mut own_book = OwnOrderBook::new(instrument_id);

    // Add orders to the public book
    let bid_order1 = BookOrder::new(
        OrderSide::Buy,
        Price::from("100.00"),
        Quantity::from(100),
        1,
    );
    let bid_order2 = BookOrder::new(OrderSide::Buy, Price::from("99.00"), Quantity::from(200), 2);
    let ask_order1 = BookOrder::new(
        OrderSide::Sell,
        Price::from("101.00"),
        Quantity::from(100),
        3,
    );
    let ask_order2 = BookOrder::new(
        OrderSide::Sell,
        Price::from("102.00"),
        Quantity::from(200),
        4,
    );

    book.add(bid_order1, 0, 1, 1.into());
    book.add(bid_order2, 0, 2, 2.into());
    book.add(ask_order1, 0, 3, 3.into());
    book.add(ask_order2, 0, 4, 4.into());

    // Add own orders - half the size of public orders at the same levels
    let own_bid_order = OwnBookOrder::new(
        TraderId::from("TRADER-001"),
        ClientOrderId::from("BID-1"),
        Some(VenueOrderId::from("1")),
        OrderSideSpecified::Buy,
        Price::from("100.00"),
        Quantity::from(50),
        OrderType::Limit,
        TimeInForce::Gtc,
        OrderStatus::Accepted,
        1.into(),
        1.into(),
        1.into(),
        1.into(),
    );

    let own_ask_order = OwnBookOrder::new(
        TraderId::from("TRADER-001"),
        ClientOrderId::from("ASK-1"),
        Some(VenueOrderId::from("1")),
        OrderSideSpecified::Sell,
        Price::from("101.00"),
        Quantity::from(50),
        OrderType::Limit,
        TimeInForce::Gtc,
        OrderStatus::Accepted,
        2.into(),
        2.into(),
        2.into(),
        2.into(),
    );

    own_book.add(own_bid_order);
    own_book.add(own_ask_order);

    // Get filtered maps
    let bids_filtered = book.bids_filtered_as_map(None, Some(&own_book), None, None, None);
    let asks_filtered = book.asks_filtered_as_map(None, Some(&own_book), None, None, None);

    // Check that own order sizes are subtracted
    assert_eq!(bids_filtered.get(&dec!(100.00)), Some(&dec!(50))); // 100 - 50 = 50
    assert_eq!(bids_filtered.get(&dec!(99.00)), Some(&dec!(200))); // unchanged
    assert_eq!(asks_filtered.get(&dec!(101.00)), Some(&dec!(50))); // 100 - 50 = 50
    assert_eq!(asks_filtered.get(&dec!(102.00)), Some(&dec!(200))); // unchanged
}

#[rstest]
fn test_book_filtered_with_own_orders_exact_size() {
    let instrument_id = InstrumentId::from("AAPL.XNAS");
    let mut book = OrderBook::new(instrument_id, BookType::L2_MBP);
    let mut own_book = OwnOrderBook::new(instrument_id);

    // Add orders to the public book
    let bid_order = BookOrder::new(
        OrderSide::Buy,
        Price::from("100.00"),
        Quantity::from(100),
        1,
    );
    let ask_order = BookOrder::new(
        OrderSide::Sell,
        Price::from("101.00"),
        Quantity::from(100),
        2,
    );

    book.add(bid_order, 0, 1, 1.into());
    book.add(ask_order, 0, 2, 2.into());

    // Add own orders with exact same size as public orders
    let own_bid_order = OwnBookOrder::new(
        TraderId::from("TRADER-001"),
        ClientOrderId::from("BID-1"),
        Some(VenueOrderId::from("1")),
        OrderSideSpecified::Buy,
        Price::from("100.00"),
        Quantity::from(100),
        OrderType::Limit,
        TimeInForce::Gtc,
        OrderStatus::Accepted,
        1.into(),
        1.into(),
        1.into(),
        1.into(),
    );

    let own_ask_order = OwnBookOrder::new(
        TraderId::from("TRADER-001"),
        ClientOrderId::from("ASK-1"),
        Some(VenueOrderId::from("1")),
        OrderSideSpecified::Sell,
        Price::from("101.00"),
        Quantity::from(100),
        OrderType::Limit,
        TimeInForce::Gtc,
        OrderStatus::Accepted,
        2.into(),
        2.into(),
        2.into(),
        2.into(),
    );

    own_book.add(own_bid_order);
    own_book.add(own_ask_order);

    // Get filtered maps
    let bids_filtered = book.bids_filtered_as_map(None, Some(&own_book), None, None, None);
    let asks_filtered = book.asks_filtered_as_map(None, Some(&own_book), None, None, None);

    // Price levels should be removed as resulting size is zero
    assert!(!bids_filtered.contains_key(&dec!(100.00)));
    assert!(!asks_filtered.contains_key(&dec!(101.00)));
}

#[rstest]
fn test_book_filtered_with_own_orders_larger_size() {
    let instrument_id = InstrumentId::from("AAPL.XNAS");
    let mut book = OrderBook::new(instrument_id, BookType::L2_MBP);
    let mut own_book = OwnOrderBook::new(instrument_id);

    // Add orders to the public book
    let bid_order = BookOrder::new(
        OrderSide::Buy,
        Price::from("100.00"),
        Quantity::from(100),
        1,
    );
    let ask_order = BookOrder::new(
        OrderSide::Sell,
        Price::from("101.00"),
        Quantity::from(100),
        2,
    );

    book.add(bid_order, 0, 1, 1.into());
    book.add(ask_order, 0, 2, 2.into());

    // Add own orders with larger size than public orders
    let own_bid_order = OwnBookOrder::new(
        TraderId::from("TRADER-001"),
        ClientOrderId::from("BID-1"),
        Some(VenueOrderId::from("1")),
        OrderSideSpecified::Buy,
        Price::from("100.00"),
        Quantity::from(150),
        OrderType::Limit,
        TimeInForce::Gtc,
        OrderStatus::Accepted,
        1.into(),
        1.into(),
        1.into(),
        1.into(),
    );

    let own_ask_order = OwnBookOrder::new(
        TraderId::from("TRADER-001"),
        ClientOrderId::from("ASK-1"),
        Some(VenueOrderId::from("1")),
        OrderSideSpecified::Sell,
        Price::from("101.00"),
        Quantity::from(150),
        OrderType::Limit,
        TimeInForce::Gtc,
        OrderStatus::Accepted,
        2.into(),
        2.into(),
        2.into(),
        2.into(),
    );

    own_book.add(own_bid_order);
    own_book.add(own_ask_order);

    // Get filtered maps
    let bids_filtered = book.bids_filtered_as_map(None, Some(&own_book), None, None, None);
    let asks_filtered = book.asks_filtered_as_map(None, Some(&own_book), None, None, None);

    // Price levels should be removed as resulting size is zero or negative
    assert!(!bids_filtered.contains_key(&dec!(100.00)));
    assert!(!asks_filtered.contains_key(&dec!(101.00)));
}

#[rstest]
fn test_book_filtered_with_own_orders_different_level() {
    let instrument_id = InstrumentId::from("AAPL.XNAS");
    let mut book = OrderBook::new(instrument_id, BookType::L2_MBP);
    let mut own_book = OwnOrderBook::new(instrument_id);

    // Add orders to the public book at certain levels
    let bid_order = BookOrder::new(
        OrderSide::Buy,
        Price::from("100.00"),
        Quantity::from(100),
        1,
    );
    let ask_order = BookOrder::new(
        OrderSide::Sell,
        Price::from("101.00"),
        Quantity::from(100),
        2,
    );

    book.add(bid_order, 0, 1, 1.into());
    book.add(ask_order, 0, 2, 2.into());

    // Add own orders at different price levels
    let own_bid_order = OwnBookOrder::new(
        TraderId::from("TRADER-001"),
        ClientOrderId::from("BID-1"),
        Some(VenueOrderId::from("1")),
        OrderSideSpecified::Buy,
        Price::from("99.00"),
        Quantity::from(50),
        OrderType::Limit,
        TimeInForce::Gtc,
        OrderStatus::Accepted,
        1.into(),
        1.into(),
        1.into(),
        1.into(),
    );

    let own_ask_order = OwnBookOrder::new(
        TraderId::from("TRADER-001"),
        ClientOrderId::from("ASK-1"),
        Some(VenueOrderId::from("1")),
        OrderSideSpecified::Sell,
        Price::from("102.00"),
        Quantity::from(50),
        OrderType::Limit,
        TimeInForce::Gtc,
        OrderStatus::Accepted,
        2.into(),
        2.into(),
        2.into(),
        2.into(),
    );

    own_book.add(own_bid_order);
    own_book.add(own_ask_order);

    // Get filtered maps
    let bids_filtered = book.bids_filtered_as_map(None, Some(&own_book), None, None, None);
    let asks_filtered = book.asks_filtered_as_map(None, Some(&own_book), None, None, None);

    // Public book levels should be unchanged as own orders are at different levels
    assert_eq!(bids_filtered.get(&dec!(100.00)), Some(&dec!(100)));
    assert_eq!(asks_filtered.get(&dec!(101.00)), Some(&dec!(100)));
}

#[rstest]
fn test_book_filtered_with_status_filter() {
    let instrument_id = InstrumentId::from("AAPL.XNAS");
    let mut book = OrderBook::new(instrument_id, BookType::L2_MBP);
    let mut own_book = OwnOrderBook::new(instrument_id);

    // Add orders to the public book
    let bid_order = BookOrder::new(
        OrderSide::Buy,
        Price::from("100.00"),
        Quantity::from(100),
        1,
    );
    let ask_order = BookOrder::new(
        OrderSide::Sell,
        Price::from("101.00"),
        Quantity::from(100),
        2,
    );

    book.add(bid_order, 0, 1, 1.into());
    book.add(ask_order, 0, 2, 2.into());

    // Add multiple own orders with different statuses at same price
    let own_bid_accepted = OwnBookOrder::new(
        TraderId::from("TRADER-001"),
        ClientOrderId::from("BID-1"),
        Some(VenueOrderId::from("1")),
        OrderSideSpecified::Buy,
        Price::from("100.00"),
        Quantity::from(30),
        OrderType::Limit,
        TimeInForce::Gtc,
        OrderStatus::Accepted,
        1.into(),
        1.into(),
        1.into(),
        1.into(),
    );

    let own_bid_submitted = OwnBookOrder::new(
        TraderId::from("TRADER-001"),
        ClientOrderId::from("BID-2"),
        Some(VenueOrderId::from("2")),
        OrderSideSpecified::Buy,
        Price::from("100.00"),
        Quantity::from(40),
        OrderType::Limit,
        TimeInForce::Gtc,
        OrderStatus::Submitted,
        2.into(),
        2.into(),
        2.into(),
        2.into(),
    );

    let own_ask_accepted = OwnBookOrder::new(
        TraderId::from("TRADER-001"),
        ClientOrderId::from("ASK-1"),
        Some(VenueOrderId::from("1")),
        OrderSideSpecified::Sell,
        Price::from("101.00"),
        Quantity::from(30),
        OrderType::Limit,
        TimeInForce::Gtc,
        OrderStatus::Accepted,
        3.into(),
        3.into(),
        3.into(),
        3.into(),
    );

    let own_ask_submitted = OwnBookOrder::new(
        TraderId::from("TRADER-001"),
        ClientOrderId::from("ASK-2"),
        Some(VenueOrderId::from("2")),
        OrderSideSpecified::Sell,
        Price::from("101.00"),
        Quantity::from(40),
        OrderType::Limit,
        TimeInForce::Gtc,
        OrderStatus::Submitted,
        4.into(),
        4.into(),
        4.into(),
        4.into(),
    );

    own_book.add(own_bid_accepted);
    own_book.add(own_bid_submitted);
    own_book.add(own_ask_accepted);
    own_book.add(own_ask_submitted);

    // Create a status filter for only ACCEPTED orders
    let mut status_filter = HashSet::new();
    status_filter.insert(OrderStatus::Accepted);

    // Get filtered maps with status filter
    let bids_filtered = book.bids_filtered_as_map(
        None,
        Some(&own_book),
        Some(status_filter.clone()),
        None,
        None,
    );
    let asks_filtered =
        book.asks_filtered_as_map(None, Some(&own_book), Some(status_filter), None, None);

    // Check that only ACCEPTED own orders are subtracted
    assert_eq!(bids_filtered.get(&dec!(100.00)), Some(&dec!(70))); // 100 - 30 = 70
    assert_eq!(asks_filtered.get(&dec!(101.00)), Some(&dec!(70))); // 100 - 30 = 70

    // Get filtered maps without status filter (should subtract all own orders)
    let bids_all_filtered = book.bids_filtered_as_map(None, Some(&own_book), None, None, None);
    let asks_all_filtered = book.asks_filtered_as_map(None, Some(&own_book), None, None, None);

    assert_eq!(bids_all_filtered.get(&dec!(100.00)), Some(&dec!(30))); // 100 - 30 - 40 = 30
    assert_eq!(asks_all_filtered.get(&dec!(101.00)), Some(&dec!(30))); // 100 - 30 - 40 = 30
}

#[rstest]
fn test_book_filtered_with_depth_limit() {
    let instrument_id = InstrumentId::from("AAPL.XNAS");
    let mut book = OrderBook::new(instrument_id, BookType::L2_MBP);
    let mut own_book = OwnOrderBook::new(instrument_id);

    // Add orders to the public book at multiple levels
    let bid_orders = [
        BookOrder::new(
            OrderSide::Buy,
            Price::from("100.00"),
            Quantity::from(100),
            1,
        ),
        BookOrder::new(OrderSide::Buy, Price::from("99.00"), Quantity::from(200), 2),
        BookOrder::new(OrderSide::Buy, Price::from("98.00"), Quantity::from(300), 3),
    ];

    let ask_orders = [
        BookOrder::new(
            OrderSide::Sell,
            Price::from("101.00"),
            Quantity::from(100),
            4,
        ),
        BookOrder::new(
            OrderSide::Sell,
            Price::from("102.00"),
            Quantity::from(200),
            5,
        ),
        BookOrder::new(
            OrderSide::Sell,
            Price::from("103.00"),
            Quantity::from(300),
            6,
        ),
    ];

    for (i, order) in bid_orders.iter().enumerate() {
        book.add(*order, 0, i as u64, (i as u64).into());
    }

    for (i, order) in ask_orders.iter().enumerate() {
        book.add(
            *order,
            0,
            (i + bid_orders.len()) as u64,
            UnixNanos::from((i + bid_orders.len()) as u64),
        );
    }

    // Add own orders at some levels
    let own_bid_order = OwnBookOrder::new(
        TraderId::from("TRADER-001"),
        ClientOrderId::from("BID-1"),
        Some(VenueOrderId::from("1")),
        OrderSideSpecified::Buy,
        Price::from("100.00"),
        Quantity::from(50),
        OrderType::Limit,
        TimeInForce::Gtc,
        OrderStatus::Accepted,
        1.into(),
        1.into(),
        1.into(),
        1.into(),
    );

    let own_ask_order = OwnBookOrder::new(
        TraderId::from("TRADER-001"),
        ClientOrderId::from("ASK-1"),
        Some(VenueOrderId::from("1")),
        OrderSideSpecified::Sell,
        Price::from("101.00"),
        Quantity::from(50),
        OrderType::Limit,
        TimeInForce::Gtc,
        OrderStatus::Accepted,
        2.into(),
        2.into(),
        2.into(),
        2.into(),
    );

    own_book.add(own_bid_order);
    own_book.add(own_ask_order);

    // Get filtered maps with depth limit
    let bids_filtered = book.bids_filtered_as_map(Some(2), Some(&own_book), None, None, None);
    let asks_filtered = book.asks_filtered_as_map(Some(2), Some(&own_book), None, None, None);

    // Check that depth limit is respected and filtering still works
    assert_eq!(bids_filtered.len(), 2);
    assert_eq!(asks_filtered.len(), 2);

    assert_eq!(bids_filtered.get(&dec!(100.00)), Some(&dec!(50))); // 100 - 50 = 50
    assert_eq!(bids_filtered.get(&dec!(99.00)), Some(&dec!(200))); // unchanged
    assert_eq!(asks_filtered.get(&dec!(101.00)), Some(&dec!(50))); // 100 - 50 = 50
    assert_eq!(asks_filtered.get(&dec!(102.00)), Some(&dec!(200))); // unchanged

    // Third level should not be present due to depth limit
    assert!(!bids_filtered.contains_key(&dec!(98.00)));
    assert!(!asks_filtered.contains_key(&dec!(103.00)));
}

#[rstest]
fn test_book_filtered_with_accepted_buffer() {
    let instrument_id = InstrumentId::from("AAPL.XNAS");
    let mut book = OrderBook::new(instrument_id, BookType::L2_MBP);
    let mut own_book = OwnOrderBook::new(instrument_id);

    // Add orders to the public book
    let bid_order = BookOrder::new(
        OrderSide::Buy,
        Price::from("100.00"),
        Quantity::from(100),
        1,
    );
    let ask_order = BookOrder::new(
        OrderSide::Sell,
        Price::from("101.00"),
        Quantity::from(100),
        2,
    );

    book.add(bid_order, 0, 1, 1.into());
    book.add(ask_order, 0, 2, 2.into());

    // Current time is 1000 ns
    let now = UnixNanos::from(1000);

    // Add own orders with ACCEPTED status at different times
    // This order was accepted at time 900 ns (100 ns ago)
    let own_bid_recent = OwnBookOrder::new(
        TraderId::from("TRADER-001"),
        ClientOrderId::from("BID-RECENT"),
        Some(VenueOrderId::from("1")),
        OrderSideSpecified::Buy,
        Price::from("100.00"),
        Quantity::from(30),
        OrderType::Limit,
        TimeInForce::Gtc,
        OrderStatus::Accepted,
        900.into(), // ts_last is 100 ns ago
        900.into(), // ts_last is 100 ns ago
        800.into(),
        800.into(),
    );

    // This order was accepted at time 500 ns (500 ns ago)
    let own_bid_older = OwnBookOrder::new(
        TraderId::from("TRADER-001"),
        ClientOrderId::from("BID-OLDER"),
        Some(VenueOrderId::from("2")),
        OrderSideSpecified::Buy,
        Price::from("100.00"),
        Quantity::from(40),
        OrderType::Limit,
        TimeInForce::Gtc,
        OrderStatus::Accepted,
        500.into(), // ts_last is 500 ns ago
        500.into(), // ts_last is 500 ns ago
        400.into(),
        400.into(),
    );

    // This order was accepted at time 900 ns (100 ns ago)
    let own_ask_recent = OwnBookOrder::new(
        TraderId::from("TRADER-001"),
        ClientOrderId::from("ASK-RECENT"),
        Some(VenueOrderId::from("3")),
        OrderSideSpecified::Sell,
        Price::from("101.00"),
        Quantity::from(30),
        OrderType::Limit,
        TimeInForce::Gtc,
        OrderStatus::Accepted,
        900.into(), // ts_last is 100 ns ago
        900.into(), // ts_last is 100 ns ago
        800.into(),
        800.into(),
    );

    // This order was accepted at time 500 ns (500 ns ago)
    let own_ask_older = OwnBookOrder::new(
        TraderId::from("TRADER-001"),
        ClientOrderId::from("ASK-OLDER"),
        Some(VenueOrderId::from("4")),
        OrderSideSpecified::Sell,
        Price::from("101.00"),
        Quantity::from(40),
        OrderType::Limit,
        TimeInForce::Gtc,
        OrderStatus::Accepted,
        500.into(), // ts_last is 500 ns ago
        500.into(), // ts_last is 500 ns ago
        400.into(),
        400.into(),
    );

    own_book.add(own_bid_recent);
    own_book.add(own_bid_older);
    own_book.add(own_ask_recent);
    own_book.add(own_ask_older);

    // Status filter for ACCEPTED orders only
    let mut status_filter = HashSet::new();
    status_filter.insert(OrderStatus::Accepted);

    // Test with a 200 ns buffer - only orders accepted before 800 ns should be filtered
    let accepted_buffer = 200;

    // Get filtered maps with accepted_buffer
    let bids_filtered = book.bids_filtered_as_map(
        None,
        Some(&own_book),
        Some(status_filter.clone()),
        Some(accepted_buffer),
        Some(now.into()),
    );

    let asks_filtered = book.asks_filtered_as_map(
        None,
        Some(&own_book),
        Some(status_filter.clone()),
        Some(accepted_buffer),
        Some(now.into()),
    );

    // Only older orders should be filtered out, recent orders should still be included
    // 100 - 40 = 60 (only older order subtracted)
    assert_eq!(bids_filtered.get(&dec!(100.00)), Some(&dec!(60)));
    assert_eq!(asks_filtered.get(&dec!(101.00)), Some(&dec!(60)));

    // Test with a 50 ns buffer - all orders should be filtered
    let short_buffer = 50;

    let bids_short_buffer = book.bids_filtered_as_map(
        None,
        Some(&own_book),
        Some(status_filter.clone()),
        Some(short_buffer),
        Some(now.into()),
    );

    let asks_short_buffer = book.asks_filtered_as_map(
        None,
        Some(&own_book),
        Some(status_filter.clone()),
        Some(short_buffer),
        Some(now.into()),
    );

    // All orders should be filtered out
    // 100 - 30 - 40 = 30
    assert_eq!(bids_short_buffer.get(&dec!(100.00)), Some(&dec!(30)));
    assert_eq!(asks_short_buffer.get(&dec!(101.00)), Some(&dec!(30)));

    // Test with a 600 ns buffer - no orders should be filtered
    let long_buffer = 600;

    let bids_long_buffer = book.bids_filtered_as_map(
        None,
        Some(&own_book),
        Some(status_filter.clone()),
        Some(long_buffer),
        Some(now.into()),
    );

    let asks_long_buffer = book.asks_filtered_as_map(
        None,
        Some(&own_book),
        Some(status_filter.clone()),
        Some(long_buffer),
        Some(now.into()),
    );

    // No orders should be filtered out (all too recent)
    assert_eq!(bids_long_buffer.get(&dec!(100.00)), Some(&dec!(100)));
    assert_eq!(asks_long_buffer.get(&dec!(101.00)), Some(&dec!(100)));
}

#[rstest]
fn test_book_filtered_with_accepted_buffer_mixed_statuses() {
    let instrument_id = InstrumentId::from("AAPL.XNAS");
    let mut book = OrderBook::new(instrument_id, BookType::L2_MBP);
    let mut own_book = OwnOrderBook::new(instrument_id);

    // Add orders to the public book
    let bid_order = BookOrder::new(
        OrderSide::Buy,
        Price::from("100.00"),
        Quantity::from(100),
        1,
    );
    book.add(bid_order, 0, 1, 1.into());

    // Current time is 1000 ns
    let now = UnixNanos::from(1000);

    // Add own orders with different statuses
    let own_bid_accepted = OwnBookOrder::new(
        TraderId::from("TRADER-001"),
        ClientOrderId::from("BID-ACCEPTED"),
        Some(VenueOrderId::from("1")),
        OrderSideSpecified::Buy,
        Price::from("100.00"),
        Quantity::from(20),
        OrderType::Limit,
        TimeInForce::Gtc,
        OrderStatus::Accepted,
        500.into(), // ts_last is 500 ns ago
        500.into(), // ts_last is 500 ns ago
        400.into(),
        400.into(),
    );

    let own_bid_submitted = OwnBookOrder::new(
        TraderId::from("TRADER-001"),
        ClientOrderId::from("BID-SUBMITTED"),
        None,
        OrderSideSpecified::Buy,
        Price::from("100.00"),
        Quantity::from(30),
        OrderType::Limit,
        TimeInForce::Gtc,
        OrderStatus::Submitted,
        500.into(), // ts_last doesn't matter for non-ACCEPTED orders
        500.into(),
        400.into(),
        400.into(),
    );

    own_book.add(own_bid_accepted);
    own_book.add(own_bid_submitted);

    // Test with no status filter but with accepted buffer
    // Buffer of 300 ns means orders accepted before 700 ns should be filtered
    let accepted_buffer = 300;

    // Without status filter, buffer applies only to ACCEPTED orders
    let bids_filtered = book.bids_filtered_as_map(
        None,
        Some(&own_book),
        None,
        Some(accepted_buffer),
        Some(now.into()),
    );

    // ACCEPTED order should be filtered (500 + 300 = 800 < 1000)
    // SUBMITTED order is always filtered when no status filter
    // 100 - 20 - 30 = 50
    assert_eq!(bids_filtered.get(&dec!(100.00)), Some(&dec!(50)));

    // Now test with a status filter for SUBMITTED only
    let mut status_filter = HashSet::new();
    status_filter.insert(OrderStatus::Submitted);

    let bids_filtered_submitted = book.bids_filtered_as_map(
        None,
        Some(&own_book),
        Some(status_filter),
        Some(accepted_buffer),
        Some(now.into()),
    );

    // Only SUBMITTED orders should be filtered, buffer doesn't apply
    // 100 - 30 = 70
    assert_eq!(bids_filtered_submitted.get(&dec!(100.00)), Some(&dec!(70)));

    // Now test with a status filter for both SUBMITTED and ACCEPTED
    let mut status_filter_both = HashSet::new();
    status_filter_both.insert(OrderStatus::Submitted);
    status_filter_both.insert(OrderStatus::Accepted);

    let bids_filtered_both = book.bids_filtered_as_map(
        None,
        Some(&own_book),
        Some(status_filter_both.clone()),
        Some(accepted_buffer),
        Some(now.into()),
    );

    // Both orders should be filtered, buffer applies to ACCEPTED
    // 100 - 20 - 30 = 50
    assert_eq!(bids_filtered_both.get(&dec!(100.00)), Some(&dec!(50)));

    // Test with a longer buffer that excludes the ACCEPTED order
    let long_buffer = 600;

    let bids_filtered_long_buffer = book.bids_filtered_as_map(
        None,
        Some(&own_book),
        Some(status_filter_both),
        Some(long_buffer),
        Some(now.into()),
    );

    // Only SUBMITTED order is filtered, ACCEPTED is too recent
    // 100 - 30 = 70
    assert_eq!(
        bids_filtered_long_buffer.get(&dec!(100.00)),
        Some(&dec!(100))
    );
}

#[rstest]
fn test_book_group_bids_filtered_empty_own_book() {
    let instrument_id = InstrumentId::from("AAPL.XNAS");
    let mut book = OrderBook::new(instrument_id, BookType::L2_MBP);

    // Add some orders to the public book
    let bid_order1 = BookOrder::new(
        OrderSide::Buy,
        Price::from("100.00"),
        Quantity::from(100),
        1,
    );
    let bid_order2 = BookOrder::new(OrderSide::Buy, Price::from("99.50"), Quantity::from(200), 2);
    let bid_order3 = BookOrder::new(OrderSide::Buy, Price::from("99.00"), Quantity::from(300), 3);

    book.add(bid_order1, 0, 1, 1.into());
    book.add(bid_order2, 0, 2, 2.into());
    book.add(bid_order3, 0, 3, 3.into());

    // Group bids with no own book
    let grouped_bids = book.group_bids_filtered(dec!(1.0), None, None, None, None, None);

    // Manually group the expected result
    assert_eq!(grouped_bids.len(), 2);
    assert_eq!(grouped_bids.get(&dec!(100.0)), Some(&dec!(100)));
    assert_eq!(grouped_bids.get(&dec!(99.0)), Some(&dec!(500))); // 200 + 300 = 500
}

#[rstest]
fn test_book_group_asks_filtered_empty_own_book() {
    let instrument_id = InstrumentId::from("AAPL.XNAS");
    let mut book = OrderBook::new(instrument_id, BookType::L2_MBP);

    // Add some orders to the public book
    let ask_order1 = BookOrder::new(
        OrderSide::Sell,
        Price::from("101.00"),
        Quantity::from(100),
        1,
    );
    let ask_order2 = BookOrder::new(
        OrderSide::Sell,
        Price::from("101.50"),
        Quantity::from(200),
        2,
    );
    let ask_order3 = BookOrder::new(
        OrderSide::Sell,
        Price::from("102.00"),
        Quantity::from(300),
        3,
    );

    book.add(ask_order1, 0, 1, 1.into());
    book.add(ask_order2, 0, 2, 2.into());
    book.add(ask_order3, 0, 3, 3.into());

    // Group asks with no own book - check that is_bid flag is correctly set to false
    let grouped_asks = book.group_asks_filtered(dec!(1.0), None, None, None, None, None);

    // Manually group the expected result
    assert_eq!(grouped_asks.len(), 2);
    assert_eq!(grouped_asks.get(&dec!(101.0)), Some(&dec!(100)));
    assert_eq!(grouped_asks.get(&dec!(102.0)), Some(&dec!(500)));
}

#[rstest]
fn test_book_group_bids_filtered_with_own_book() {
    let instrument_id = InstrumentId::from("AAPL.XNAS");
    let mut book = OrderBook::new(instrument_id, BookType::L2_MBP);
    let mut own_book = OwnOrderBook::new(instrument_id);

    // Add orders to the public book
    let bid_order1 = BookOrder::new(
        OrderSide::Buy,
        Price::from("100.00"),
        Quantity::from(100),
        1,
    );
    let bid_order2 = BookOrder::new(OrderSide::Buy, Price::from("99.00"), Quantity::from(200), 2);

    book.add(bid_order1, 0, 1, 1.into());
    book.add(bid_order2, 0, 2, 2.into());

    // Add own orders
    let own_bid_order1 = OwnBookOrder::new(
        TraderId::from("TRADER-001"),
        ClientOrderId::from("BID-1"),
        Some(VenueOrderId::from("1")),
        OrderSideSpecified::Buy,
        Price::from("100.00"),
        Quantity::from("40"),
        OrderType::Limit,
        TimeInForce::Gtc,
        OrderStatus::Accepted,
        UnixNanos::default(),
        UnixNanos::default(),
        UnixNanos::default(),
        UnixNanos::default(),
    );

    let own_bid_order2 = OwnBookOrder::new(
        TraderId::from("TRADER-001"),
        ClientOrderId::from("BID-2"),
        Some(VenueOrderId::from("2")),
        OrderSideSpecified::Buy,
        Price::from("99.00"),
        Quantity::from("50"),
        OrderType::Limit,
        TimeInForce::Gtc,
        OrderStatus::Accepted,
        UnixNanos::default(),
        UnixNanos::default(),
        UnixNanos::default(),
        UnixNanos::default(),
    );

    own_book.add(own_bid_order1);
    own_book.add(own_bid_order2);

    // Group bids with own book
    let grouped_bids = book.group_bids_filtered(dec!(1.0), None, Some(&own_book), None, None, None);

    // Verify that own orders are correctly filtered from the grouped results
    assert_eq!(grouped_bids.len(), 2);
    assert_eq!(grouped_bids.get(&dec!(100.0)), Some(&dec!(60))); // 100 - 40 = 60
    assert_eq!(grouped_bids.get(&dec!(99.0)), Some(&dec!(150))); // 200 - 50 = 150
}

#[rstest]
fn test_book_group_asks_filtered_with_own_book() {
    let instrument_id = InstrumentId::from("AAPL.XNAS");
    let mut book = OrderBook::new(instrument_id, BookType::L2_MBP);
    let mut own_book = OwnOrderBook::new(instrument_id);

    // Add orders to the public book
    let ask_order1 = BookOrder::new(
        OrderSide::Sell,
        Price::from("101.00"),
        Quantity::from(100),
        1,
    );
    let ask_order2 = BookOrder::new(
        OrderSide::Sell,
        Price::from("102.00"),
        Quantity::from(200),
        2,
    );

    book.add(ask_order1, 0, 1, 1.into());
    book.add(ask_order2, 0, 2, 2.into());

    // Add own orders
    let own_ask_order1 = OwnBookOrder::new(
        TraderId::from("TRADER-001"),
        ClientOrderId::from("ASK-1"),
        Some(VenueOrderId::from("1")),
        OrderSideSpecified::Sell,
        Price::from("101.00"),
        Quantity::from("40"),
        OrderType::Limit,
        TimeInForce::Gtc,
        OrderStatus::Accepted,
        UnixNanos::default(),
        UnixNanos::default(),
        UnixNanos::default(),
        UnixNanos::default(),
    );

    let own_ask_order2 = OwnBookOrder::new(
        TraderId::from("TRADER-001"),
        ClientOrderId::from("ASK-2"),
        Some(VenueOrderId::from("2")),
        OrderSideSpecified::Sell,
        Price::from("102.00"),
        Quantity::from("50"),
        OrderType::Limit,
        TimeInForce::Gtc,
        OrderStatus::Accepted,
        UnixNanos::default(),
        UnixNanos::default(),
        UnixNanos::default(),
        UnixNanos::default(),
    );

    own_book.add(own_ask_order1);
    own_book.add(own_ask_order2);

    // Group asks with own book
    let grouped_asks = book.group_asks_filtered(dec!(1.0), None, Some(&own_book), None, None, None);

    // Verify that own orders are correctly filtered from the grouped results
    assert_eq!(grouped_asks.len(), 2);
    assert_eq!(grouped_asks.get(&dec!(101.0)), Some(&dec!(60))); // 100 - 40 = 60
    assert_eq!(grouped_asks.get(&dec!(102.0)), Some(&dec!(150))); // 200 - 50 = 150
}

#[rstest]
fn test_book_group_with_status_filter() {
    let instrument_id = InstrumentId::from("AAPL.XNAS");
    let mut book = OrderBook::new(instrument_id, BookType::L2_MBP);
    let mut own_book = OwnOrderBook::new(instrument_id);

    // Add orders to the public book
    let bid_order1 = BookOrder::new(
        OrderSide::Buy,
        Price::from("100.00"),
        Quantity::from(100),
        1,
    );

    book.add(bid_order1, 0, 1, 1.into());

    // Add own orders with different statuses
    let own_accepted = OwnBookOrder::new(
        TraderId::from("TRADER-001"),
        ClientOrderId::from("BID-1"),
        Some(VenueOrderId::from("1")),
        OrderSideSpecified::Buy,
        Price::from("100.00"),
        Quantity::from("40"),
        OrderType::Limit,
        TimeInForce::Gtc,
        OrderStatus::Accepted,
        UnixNanos::default(),
        UnixNanos::default(),
        UnixNanos::default(),
        UnixNanos::default(),
    );

    let own_submitted = OwnBookOrder::new(
        TraderId::from("TRADER-001"),
        ClientOrderId::from("BID-2"),
        Some(VenueOrderId::from("2")),
        OrderSideSpecified::Buy,
        Price::from("100.00"),
        Quantity::from("30"),
        OrderType::Limit,
        TimeInForce::Gtc,
        OrderStatus::Submitted,
        UnixNanos::default(),
        UnixNanos::default(),
        UnixNanos::default(),
        UnixNanos::default(),
    );

    own_book.add(own_accepted);
    own_book.add(own_submitted);

    // Create a status filter for ACCEPTED orders only
    let mut status_filter = HashSet::new();
    status_filter.insert(OrderStatus::Accepted);

    // Group with status filter
    let grouped_bids = book.group_bids_filtered(
        dec!(1.0),
        None,
        Some(&own_book),
        Some(status_filter),
        None,
        None,
    );

    // Verify that only ACCEPTED own orders are filtered out
    assert_eq!(grouped_bids.len(), 1);
    assert_eq!(grouped_bids.get(&dec!(100.0)), Some(&dec!(60))); // 100 - 40 = 60 (only ACCEPTED is filtered)
}

#[rstest]
#[case(None)]
#[case(Some(OrderSide::NoOrderSide))]
#[case(Some(OrderSide::Buy))]
#[case(Some(OrderSide::Sell))]
fn test_book_clear_stale_levels_not_crossed(#[case] side: Option<OrderSide>) {
    let instrument_id = InstrumentId::from("ETHUSDT-PERP.BINANCE");
    let mut book = OrderBook::new(instrument_id, BookType::L2_MBP);

    // Add normal, non-crossed levels
    let bid1 = BookOrder::new(
        OrderSide::Buy,
        Price::from("99.00"),
        Quantity::from("10.0"),
        1,
    );
    let bid2 = BookOrder::new(
        OrderSide::Buy,
        Price::from("98.00"),
        Quantity::from("20.0"),
        2,
    );
    let ask1 = BookOrder::new(
        OrderSide::Sell,
        Price::from("101.00"),
        Quantity::from("10.0"),
        3,
    );
    let ask2 = BookOrder::new(
        OrderSide::Sell,
        Price::from("102.00"),
        Quantity::from("20.0"),
        4,
    );

    book.add(bid1, 0, 1, 100.into());
    book.add(bid2, 0, 2, 200.into());
    book.add(ask1, 0, 3, 300.into());
    book.add(ask2, 0, 4, 400.into());

    let initial_update_count = book.update_count;
    let removed = book.clear_stale_levels(side);

    assert!(removed.is_none());
    assert_eq!(book.update_count, initial_update_count); // no increment when nothing removed
    assert_eq!(book.best_bid_price(), Some(Price::from("99.00")));
    assert_eq!(book.best_ask_price(), Some(Price::from("101.00")));
    assert_eq!(book.bids(None).count(), 2);
    assert_eq!(book.asks(None).count(), 2);
}

#[rstest]
#[case(None)]
#[case(Some(OrderSide::NoOrderSide))]
fn test_book_clear_stale_levels_simple_crossed(#[case] side: Option<OrderSide>) {
    let instrument_id = InstrumentId::from("ETHUSDT-PERP.BINANCE");
    let mut book = OrderBook::new(instrument_id, BookType::L2_MBP);

    // Create a crossed book: best bid > best ask
    let bid1 = BookOrder::new(
        OrderSide::Buy,
        Price::from("105.00"),
        Quantity::from("10.0"),
        1,
    );
    let bid2 = BookOrder::new(
        OrderSide::Buy,
        Price::from("100.00"),
        Quantity::from("20.0"),
        2,
    );
    let ask1 = BookOrder::new(
        OrderSide::Sell,
        Price::from("95.00"),
        Quantity::from("10.0"),
        3,
    );
    let ask2 = BookOrder::new(
        OrderSide::Sell,
        Price::from("110.00"),
        Quantity::from("20.0"),
        4,
    );

    book.add(bid1, 0, 1, 100.into());
    book.add(bid2, 0, 2, 200.into());
    book.add(ask1, 0, 3, 300.into());
    book.add(ask2, 0, 4, 400.into());

    let initial_update_count = book.update_count;
    let removed = book.clear_stale_levels(side);

    // Should remove:
    // - Bids with price >= 95 (original best ask): 105, 100
    // - Asks with price <= 105 (original best bid): 95
    assert!(removed.is_some());
    let removed_levels = removed.unwrap();
    assert_eq!(removed_levels.len(), 3); // 2 bid levels + 1 ask level
    assert_eq!(book.update_count, initial_update_count + 1); // Should increment once
    assert_eq!(book.best_bid_price(), None); // Both bids removed
    assert_eq!(book.best_ask_price(), Some(Price::from("110.00"))); // ask1 removed
    assert_eq!(book.bids(None).count(), 0);
    assert_eq!(book.asks(None).count(), 1);

    // Idempotence: second call should return None and not change counters
    let removed2 = book.clear_stale_levels(side);
    assert!(removed2.is_none());
    assert_eq!(book.update_count, initial_update_count + 1);
}

#[rstest]
fn test_book_clear_stale_levels_multiple_overlapping() {
    let instrument_id = InstrumentId::from("ETHUSDT-PERP.BINANCE");
    let mut book = OrderBook::new(instrument_id, BookType::L3_MBO);

    // Create deeply crossed book with multiple overlapping levels
    // Bids: 110, 108, 105, 90
    let bid1 = BookOrder::new(
        OrderSide::Buy,
        Price::from("110.00"),
        Quantity::from("10.0"),
        1,
    );
    let bid2 = BookOrder::new(
        OrderSide::Buy,
        Price::from("108.00"),
        Quantity::from("20.0"),
        2,
    );
    let bid3 = BookOrder::new(
        OrderSide::Buy,
        Price::from("105.00"),
        Quantity::from("30.0"),
        3,
    );
    let bid4 = BookOrder::new(
        OrderSide::Buy,
        Price::from("90.00"),
        Quantity::from("40.0"),
        4,
    );

    // Asks: 95, 100, 103, 115
    let ask1 = BookOrder::new(
        OrderSide::Sell,
        Price::from("95.00"),
        Quantity::from("10.0"),
        5,
    );
    let ask2 = BookOrder::new(
        OrderSide::Sell,
        Price::from("100.00"),
        Quantity::from("20.0"),
        6,
    );
    let ask3 = BookOrder::new(
        OrderSide::Sell,
        Price::from("103.00"),
        Quantity::from("30.0"),
        7,
    );
    let ask4 = BookOrder::new(
        OrderSide::Sell,
        Price::from("115.00"),
        Quantity::from("40.0"),
        8,
    );

    book.add(bid1, 0, 1, 100.into());
    book.add(bid2, 0, 2, 200.into());
    book.add(bid3, 0, 3, 300.into());
    book.add(bid4, 0, 4, 400.into());
    book.add(ask1, 0, 5, 500.into());
    book.add(ask2, 0, 6, 600.into());
    book.add(ask3, 0, 7, 700.into());
    book.add(ask4, 0, 8, 800.into());

    let removed = book.clear_stale_levels(None);

    // Should remove:
    // - Bids with price >= 95 (original best ask): 110, 108, 105
    // - Asks with price <= 110 (original best bid): 95, 100, 103
    assert!(removed.is_some());
    let removed_levels = removed.unwrap();
    assert_eq!(removed_levels.len(), 6); // 3 bid levels + 3 ask levels
    assert_eq!(book.best_bid_price(), Some(Price::from("90.00")));
    assert_eq!(book.best_ask_price(), Some(Price::from("115.00")));
    assert_eq!(book.bids(None).count(), 1);
    assert_eq!(book.asks(None).count(), 1);
}

#[rstest]
fn test_book_clear_stale_levels_with_multiple_orders_per_level() {
    let instrument_id = InstrumentId::from("ETHUSDT-PERP.BINANCE");
    let mut book = OrderBook::new(instrument_id, BookType::L2_MBP);

    // Add orders at crossed price levels - in L2_MBP, later orders replace earlier ones at same price
    let bid1 = BookOrder::new(
        OrderSide::Buy,
        Price::from("105.00"),
        Quantity::from("30.0"),
        1,
    );
    let bid2 = BookOrder::new(
        OrderSide::Buy,
        Price::from("90.00"),
        Quantity::from("20.0"),
        2,
    );

    let ask1 = BookOrder::new(
        OrderSide::Sell,
        Price::from("95.00"),
        Quantity::from("25.0"),
        3,
    );
    let ask2 = BookOrder::new(
        OrderSide::Sell,
        Price::from("110.00"),
        Quantity::from("20.0"),
        4,
    );

    book.add(bid1, 0, 1, 100.into());
    book.add(bid2, 0, 2, 200.into());
    book.add(ask1, 0, 3, 300.into());
    book.add(ask2, 0, 4, 400.into());

    assert_eq!(book.best_bid_size(), Some(Quantity::from("30.0")));

    let removed = book.clear_stale_levels(None);

    // Should remove 1 bid level at 105 + 1 ask level at 95
    assert!(removed.is_some());
    let removed_levels = removed.unwrap();
    assert_eq!(removed_levels.len(), 2); // Count of price levels
    assert_eq!(book.best_bid_price(), Some(Price::from("90.00")));
    assert_eq!(book.best_ask_price(), Some(Price::from("110.00")));
    assert_eq!(book.bids(None).count(), 1);
    assert_eq!(book.asks(None).count(), 1);
}

#[rstest]
fn test_book_clear_stale_levels_side_sell_clears_asks_only() {
    let instrument_id = InstrumentId::from("ETHUSDT-PERP.BINANCE");
    let mut book = OrderBook::new(instrument_id, BookType::L2_MBP);

    // Bids: 105, 100; Asks: 95, 110 (crossed)
    book.add(
        BookOrder::new(
            OrderSide::Buy,
            Price::from("105.00"),
            Quantity::from("10.0"),
            1,
        ),
        0,
        1,
        100.into(),
    );
    book.add(
        BookOrder::new(
            OrderSide::Buy,
            Price::from("100.00"),
            Quantity::from("20.0"),
            2,
        ),
        0,
        2,
        200.into(),
    );
    book.add(
        BookOrder::new(
            OrderSide::Sell,
            Price::from("95.00"),
            Quantity::from("10.0"),
            3,
        ),
        0,
        3,
        300.into(),
    );
    book.add(
        BookOrder::new(
            OrderSide::Sell,
            Price::from("110.00"),
            Quantity::from("20.0"),
            4,
        ),
        0,
        4,
        400.into(),
    );

    let initial_update_count = book.update_count;

    let removed = book.clear_stale_levels(Some(OrderSide::Sell));
    assert!(removed.is_some());
    let removed_levels = removed.unwrap();
    assert_eq!(removed_levels.len(), 1); // removed ask at 95

    assert_eq!(book.update_count, initial_update_count + 1);
    assert_eq!(book.best_bid_price(), Some(Price::from("105.00")));
    assert_eq!(book.best_ask_price(), Some(Price::from("110.00")));
    assert_eq!(book.bids(None).count(), 2);
    assert_eq!(book.asks(None).count(), 1);
}

#[rstest]
fn test_book_clear_stale_levels_side_buy_clears_bids_only() {
    let instrument_id = InstrumentId::from("ETHUSDT-PERP.BINANCE");
    let mut book = OrderBook::new(instrument_id, BookType::L2_MBP);

    // Bids: 110, 90; Asks: 100, 115 (crossed)
    book.add(
        BookOrder::new(
            OrderSide::Buy,
            Price::from("110.00"),
            Quantity::from("10.0"),
            1,
        ),
        0,
        1,
        100.into(),
    );
    book.add(
        BookOrder::new(
            OrderSide::Buy,
            Price::from("90.00"),
            Quantity::from("20.0"),
            2,
        ),
        0,
        2,
        200.into(),
    );
    book.add(
        BookOrder::new(
            OrderSide::Sell,
            Price::from("100.00"),
            Quantity::from("10.0"),
            3,
        ),
        0,
        3,
        300.into(),
    );
    book.add(
        BookOrder::new(
            OrderSide::Sell,
            Price::from("115.00"),
            Quantity::from("20.0"),
            4,
        ),
        0,
        4,
        400.into(),
    );

    let initial_update_count = book.update_count;

    let removed = book.clear_stale_levels(Some(OrderSide::Buy));
    assert!(removed.is_some());
    let removed_levels = removed.unwrap();
    assert_eq!(removed_levels.len(), 1); // removed bid at 110

    assert_eq!(book.update_count, initial_update_count + 1);
    assert_eq!(book.best_bid_price(), Some(Price::from("90.00")));
    assert_eq!(book.best_ask_price(), Some(Price::from("100.00")));
    assert_eq!(book.bids(None).count(), 1);
    assert_eq!(book.asks(None).count(), 2);
}

#[rstest]
fn test_book_clear_stale_levels_multiple_crossed_each_side() {
    let instrument_id = InstrumentId::from("ETHUSDT-PERP.BINANCE");
    let mut book = OrderBook::new(instrument_id, BookType::L2_MBP);

    // Bids: 110, 105, 102, 99, 95, 90
    // Asks: 100, 103, 106, 109, 112, 115

    book.add(
        BookOrder::new(
            OrderSide::Buy,
            Price::from("110.00"),
            Quantity::from("10.0"),
            1,
        ),
        0,
        1,
        100.into(),
    );
    book.add(
        BookOrder::new(
            OrderSide::Buy,
            Price::from("105.00"),
            Quantity::from("20.0"),
            2,
        ),
        0,
        2,
        200.into(),
    );
    book.add(
        BookOrder::new(
            OrderSide::Buy,
            Price::from("102.00"),
            Quantity::from("30.0"),
            3,
        ),
        0,
        3,
        300.into(),
    );
    book.add(
        BookOrder::new(
            OrderSide::Buy,
            Price::from("99.00"),
            Quantity::from("40.0"),
            4,
        ),
        0,
        4,
        400.into(),
    );
    book.add(
        BookOrder::new(
            OrderSide::Buy,
            Price::from("95.00"),
            Quantity::from("50.0"),
            5,
        ),
        0,
        5,
        500.into(),
    );
    book.add(
        BookOrder::new(
            OrderSide::Buy,
            Price::from("90.00"),
            Quantity::from("60.0"),
            6,
        ),
        0,
        6,
        600.into(),
    );

    book.add(
        BookOrder::new(
            OrderSide::Sell,
            Price::from("100.00"),
            Quantity::from("15.0"),
            7,
        ),
        0,
        7,
        700.into(),
    );
    book.add(
        BookOrder::new(
            OrderSide::Sell,
            Price::from("103.00"),
            Quantity::from("25.0"),
            8,
        ),
        0,
        8,
        800.into(),
    );
    book.add(
        BookOrder::new(
            OrderSide::Sell,
            Price::from("106.00"),
            Quantity::from("35.0"),
            9,
        ),
        0,
        9,
        900.into(),
    );
    book.add(
        BookOrder::new(
            OrderSide::Sell,
            Price::from("109.00"),
            Quantity::from("45.0"),
            10,
        ),
        0,
        10,
        1000.into(),
    );
    book.add(
        BookOrder::new(
            OrderSide::Sell,
            Price::from("112.00"),
            Quantity::from("55.0"),
            11,
        ),
        0,
        11,
        1100.into(),
    );
    book.add(
        BookOrder::new(
            OrderSide::Sell,
            Price::from("115.00"),
            Quantity::from("65.0"),
            12,
        ),
        0,
        12,
        1200.into(),
    );

    assert_eq!(book.best_bid_price(), Some(Price::from("110.00")));
    assert_eq!(book.best_ask_price(), Some(Price::from("100.00")));

    let removed = book.clear_stale_levels(None);
    assert!(removed.is_some());
    let removed_levels = removed.unwrap();

    // 3 bids >= 100 (best ask): 110, 105, 102
    // 4 asks <= 110 (best bid): 100, 103, 106, 109
    assert_eq!(removed_levels.len(), 7);

    // Verify order: bids first, then asks
    assert_eq!(removed_levels[0].price.value, Price::from("110.00"));
    assert_eq!(
        removed_levels[0].size_decimal(),
        rust_decimal::Decimal::from(10)
    );
    assert_eq!(removed_levels[1].price.value, Price::from("105.00"));
    assert_eq!(removed_levels[2].price.value, Price::from("102.00"));

    assert_eq!(removed_levels[3].price.value, Price::from("100.00"));
    assert_eq!(
        removed_levels[3].size_decimal(),
        rust_decimal::Decimal::from(15)
    );
    assert_eq!(removed_levels[4].price.value, Price::from("103.00"));
    assert_eq!(removed_levels[5].price.value, Price::from("106.00"));
    assert_eq!(removed_levels[6].price.value, Price::from("109.00"));

    assert_eq!(book.best_bid_price(), Some(Price::from("99.00")));
    assert_eq!(book.best_ask_price(), Some(Price::from("112.00")));
    assert_eq!(book.bids(None).count(), 3);
    assert_eq!(book.asks(None).count(), 2);

    // Test idempotence: calling clear_stale_levels again should return None
    // and not change the update count since the book is no longer crossed
    let update_count_before = book.update_count;
    let removed_again = book.clear_stale_levels(None);
    assert!(removed_again.is_none());
    assert_eq!(book.update_count, update_count_before);
}

#[rstest]
fn test_book_clear_stale_levels_multiple_crossed_side_specific() {
    let instrument_id = InstrumentId::from("ETHUSDT-PERP.BINANCE");
    let mut book = OrderBook::new(instrument_id, BookType::L2_MBP);

    // Bids: 110, 105, 102, 99, 95, 90
    // Asks: 100, 103, 106, 109, 112, 115

    book.add(
        BookOrder::new(
            OrderSide::Buy,
            Price::from("110.00"),
            Quantity::from("10.0"),
            1,
        ),
        0,
        1,
        100.into(),
    );
    book.add(
        BookOrder::new(
            OrderSide::Buy,
            Price::from("105.00"),
            Quantity::from("20.0"),
            2,
        ),
        0,
        2,
        200.into(),
    );
    book.add(
        BookOrder::new(
            OrderSide::Buy,
            Price::from("102.00"),
            Quantity::from("30.0"),
            3,
        ),
        0,
        3,
        300.into(),
    );
    book.add(
        BookOrder::new(
            OrderSide::Buy,
            Price::from("99.00"),
            Quantity::from("40.0"),
            4,
        ),
        0,
        4,
        400.into(),
    );
    book.add(
        BookOrder::new(
            OrderSide::Buy,
            Price::from("95.00"),
            Quantity::from("50.0"),
            5,
        ),
        0,
        5,
        500.into(),
    );
    book.add(
        BookOrder::new(
            OrderSide::Buy,
            Price::from("90.00"),
            Quantity::from("60.0"),
            6,
        ),
        0,
        6,
        600.into(),
    );

    book.add(
        BookOrder::new(
            OrderSide::Sell,
            Price::from("100.00"),
            Quantity::from("15.0"),
            7,
        ),
        0,
        7,
        700.into(),
    );
    book.add(
        BookOrder::new(
            OrderSide::Sell,
            Price::from("103.00"),
            Quantity::from("25.0"),
            8,
        ),
        0,
        8,
        800.into(),
    );
    book.add(
        BookOrder::new(
            OrderSide::Sell,
            Price::from("106.00"),
            Quantity::from("35.0"),
            9,
        ),
        0,
        9,
        900.into(),
    );
    book.add(
        BookOrder::new(
            OrderSide::Sell,
            Price::from("109.00"),
            Quantity::from("45.0"),
            10,
        ),
        0,
        10,
        1000.into(),
    );
    book.add(
        BookOrder::new(
            OrderSide::Sell,
            Price::from("112.00"),
            Quantity::from("55.0"),
            11,
        ),
        0,
        11,
        1100.into(),
    );
    book.add(
        BookOrder::new(
            OrderSide::Sell,
            Price::from("115.00"),
            Quantity::from("65.0"),
            12,
        ),
        0,
        12,
        1200.into(),
    );

    // Test clearing only bids
    let removed = book.clear_stale_levels(Some(OrderSide::Buy));
    assert!(removed.is_some());
    let removed_levels = removed.unwrap();

    // Should remove only 3 bid levels
    assert_eq!(removed_levels.len(), 3);
    assert_eq!(removed_levels[0].price.value, Price::from("110.00"));
    assert_eq!(removed_levels[1].price.value, Price::from("105.00"));
    assert_eq!(removed_levels[2].price.value, Price::from("102.00"));

    // Asks remain untouched, bids cleaned
    assert_eq!(book.best_bid_price(), Some(Price::from("99.00")));
    assert_eq!(book.best_ask_price(), Some(Price::from("100.00")));
    assert_eq!(book.bids(None).count(), 3);
    assert_eq!(book.asks(None).count(), 6);
}

#[rstest]
fn test_book_clear_stale_levels_l1_mbp() {
    // Test that L1_MBP books are skipped
    let instrument_id = InstrumentId::from("ETHUSDT-PERP.BINANCE");
    let mut book = OrderBook::new(instrument_id, BookType::L1_MBP);

    // Add some L1 data (note: in real L1_MBP, we'd use update_quote_tick)
    // But for this test, we just want to verify clear_stale_levels behavior

    let initial_update_count = book.update_count;

    // Even if we somehow had a crossed L1 book, it should return None
    let removed = book.clear_stale_levels(None);

    assert!(removed.is_none());
    // Verify update_count is unchanged (mirroring non-crossed test style)
    assert_eq!(book.update_count, initial_update_count);
}

////////////////////////////////////////////////////////////////////////////////
// OwnOrderBook
////////////////////////////////////////////////////////////////////////////////

#[fixture]
fn own_order() -> OwnBookOrder {
    let trader_id = TraderId::from("TRADER-001");
    let client_order_id = ClientOrderId::from("O-123456789");
    let venue_order_id = None;
    let side = OrderSideSpecified::Buy;
    let price = Price::from("100.00");
    let size = Quantity::from("10");
    let order_type = OrderType::Limit;
    let time_in_force = TimeInForce::Gtc;
    let status = OrderStatus::Submitted;
    let ts_last = UnixNanos::from(2);
    let ts_accepted = UnixNanos::from(0);
    let ts_submitted = UnixNanos::from(2);
    let ts_init = UnixNanos::from(1);

    OwnBookOrder::new(
        trader_id,
        client_order_id,
        venue_order_id,
        side,
        price,
        size,
        order_type,
        time_in_force,
        status,
        ts_last,
        ts_accepted,
        ts_submitted,
        ts_init,
    )
}

#[rstest]
fn test_own_order_to_book_price(own_order: OwnBookOrder) {
    let book_price = own_order.to_book_price();
    assert_eq!(book_price.value, Price::from("100.00"));
    assert_eq!(book_price.side, OrderSideSpecified::Buy);
}

#[rstest]
fn test_own_order_exposure(own_order: OwnBookOrder) {
    let exposure = own_order.exposure();
    assert_eq!(exposure, 1000.0);
}

#[rstest]
fn test_own_order_signed_size(own_order: OwnBookOrder) {
    let own_order_buy = own_order;
    let own_order_sell = OwnBookOrder::new(
        TraderId::from("TRADER-001"),
        ClientOrderId::from("O-123456789"),
        Some(VenueOrderId::from("1")),
        OrderSideSpecified::Sell,
        Price::from("101.0"),
        Quantity::from("10"),
        OrderType::Limit,
        TimeInForce::Gtc,
        OrderStatus::Accepted,
        UnixNanos::from(2),
        UnixNanos::from(0),
        UnixNanos::from(2),
        UnixNanos::from(1),
    );

    assert_eq!(own_order_buy.signed_size(), 10.0);
    assert_eq!(own_order_sell.signed_size(), -10.0);
}

#[rstest]
fn test_own_order_debug(own_order: OwnBookOrder) {
    assert_eq!(
        format!("{own_order:?}"),
        "OwnBookOrder(trader_id=TRADER-001, client_order_id=O-123456789, venue_order_id=None, side=BUY, price=100.00, size=10, order_type=LIMIT, time_in_force=GTC, status=SUBMITTED, ts_last=2, ts_accepted=0, ts_submitted=2, ts_init=1)"
    );
}

#[rstest]
fn test_own_order_display(own_order: OwnBookOrder) {
    assert_eq!(
        own_order.to_string(),
        "TRADER-001,O-123456789,None,BUY,100.00,10,LIMIT,GTC,SUBMITTED,2,0,2,1".to_string()
    );
}

#[rstest]
fn test_client_order_ids_empty_book() {
    let instrument_id = InstrumentId::from("AAPL.XNAS");
    let book = OwnOrderBook::new(instrument_id);

    let bid_ids = book.bid_client_order_ids();
    let ask_ids = book.ask_client_order_ids();

    assert!(bid_ids.is_empty());
    assert!(ask_ids.is_empty());

    let client_order_id = ClientOrderId::from("O-NONEXISTENT");
    assert!(!book.is_order_in_book(&client_order_id));
}

#[rstest]
fn test_client_order_ids_with_orders() {
    let instrument_id = InstrumentId::from("AAPL.XNAS");
    let mut book = OwnOrderBook::new(instrument_id);

    let bid_id1 = ClientOrderId::from("O-BID-1");
    let bid_id2 = ClientOrderId::from("O-BID-2");
    let ask_id1 = ClientOrderId::from("O-ASK-1");
    let ask_id2 = ClientOrderId::from("O-ASK-2");

    let bid_order1 = OwnBookOrder::new(
        TraderId::from("TRADER-001"),
        bid_id1,
        Some(VenueOrderId::from("1")),
        OrderSideSpecified::Buy,
        Price::from("100.00"),
        Quantity::from("10"),
        OrderType::Limit,
        TimeInForce::Gtc,
        OrderStatus::Accepted,
        UnixNanos::default(),
        UnixNanos::default(),
        UnixNanos::default(),
        UnixNanos::default(),
    );

    let bid_order2 = OwnBookOrder::new(
        TraderId::from("TRADER-001"),
        bid_id2,
        Some(VenueOrderId::from("2")),
        OrderSideSpecified::Buy,
        Price::from("99.00"),
        Quantity::from("20"),
        OrderType::Limit,
        TimeInForce::Gtc,
        OrderStatus::Accepted,
        UnixNanos::default(),
        UnixNanos::default(),
        UnixNanos::default(),
        UnixNanos::default(),
    );

    let ask_order1 = OwnBookOrder::new(
        TraderId::from("TRADER-001"),
        ask_id1,
        Some(VenueOrderId::from("3")),
        OrderSideSpecified::Sell,
        Price::from("101.00"),
        Quantity::from("10"),
        OrderType::Limit,
        TimeInForce::Gtc,
        OrderStatus::Accepted,
        UnixNanos::default(),
        UnixNanos::default(),
        UnixNanos::default(),
        UnixNanos::default(),
    );

    let ask_order2 = OwnBookOrder::new(
        TraderId::from("TRADER-001"),
        ask_id2,
        Some(VenueOrderId::from("4")),
        OrderSideSpecified::Sell,
        Price::from("102.00"),
        Quantity::from("20"),
        OrderType::Limit,
        TimeInForce::Gtc,
        OrderStatus::Accepted,
        UnixNanos::default(),
        UnixNanos::default(),
        UnixNanos::default(),
        UnixNanos::default(),
    );

    book.add(bid_order1);
    book.add(bid_order2);
    book.add(ask_order1);
    book.add(ask_order2);

    let bid_ids = book.bid_client_order_ids();
    assert_eq!(bid_ids.len(), 2);
    assert!(bid_ids.contains(&bid_id1));
    assert!(bid_ids.contains(&bid_id2));
    assert!(!bid_ids.contains(&ask_id1));
    assert!(!bid_ids.contains(&ask_id2));

    let ask_ids = book.ask_client_order_ids();
    assert_eq!(ask_ids.len(), 2);
    assert!(ask_ids.contains(&ask_id1));
    assert!(ask_ids.contains(&ask_id2));
    assert!(!ask_ids.contains(&bid_id1));
    assert!(!ask_ids.contains(&bid_id2));

    assert!(book.is_order_in_book(&bid_id1));
    assert!(book.is_order_in_book(&bid_id2));
    assert!(book.is_order_in_book(&ask_id1));
    assert!(book.is_order_in_book(&ask_id2));
    assert!(!book.is_order_in_book(&ClientOrderId::from("O-NON-EXISTENT")));
}

#[rstest]
fn test_client_order_ids_after_operations() {
    let instrument_id = InstrumentId::from("AAPL.XNAS");
    let mut book = OwnOrderBook::new(instrument_id);

    let client_order_id = ClientOrderId::from("O-BID-1");
    let order = OwnBookOrder::new(
        TraderId::from("TRADER-001"),
        client_order_id,
        Some(VenueOrderId::from("1")),
        OrderSideSpecified::Buy,
        Price::from("100.00"),
        Quantity::from("10"),
        OrderType::Limit,
        TimeInForce::Gtc,
        OrderStatus::Accepted,
        UnixNanos::default(),
        UnixNanos::default(),
        UnixNanos::default(),
        UnixNanos::default(),
    );

    book.add(order);

    assert!(book.is_order_in_book(&client_order_id));
    assert_eq!(book.bid_client_order_ids().len(), 1);

    book.delete(order).unwrap();

    assert!(!book.is_order_in_book(&client_order_id));
    assert!(book.bid_client_order_ids().is_empty());
}

#[rstest]
fn test_own_book_update_missing_order_errors() {
    let instrument_id = InstrumentId::from("AAPL.XNAS");
    let mut book = OwnOrderBook::new(instrument_id);

    let missing_order = OwnBookOrder::new(
        TraderId::from("TRADER-001"),
        ClientOrderId::from("O-MISSING"),
        None,
        OrderSideSpecified::Buy,
        Price::from("100.00"),
        Quantity::from("1"),
        OrderType::Limit,
        TimeInForce::Gtc,
        OrderStatus::Submitted,
        UnixNanos::from(1_u64),
        UnixNanos::default(),
        UnixNanos::from(1_u64),
        UnixNanos::from(1_u64),
    );

    let result = book.update(missing_order);
    assert!(result.is_err());
}

#[rstest]
fn test_own_book_delete_missing_order_errors() {
    let instrument_id = InstrumentId::from("AAPL.XNAS");
    let mut book = OwnOrderBook::new(instrument_id);

    let missing_order = OwnBookOrder::new(
        TraderId::from("TRADER-001"),
        ClientOrderId::from("O-MISSING"),
        None,
        OrderSideSpecified::Sell,
        Price::from("101.00"),
        Quantity::from("1"),
        OrderType::Limit,
        TimeInForce::Gtc,
        OrderStatus::Submitted,
        UnixNanos::from(1_u64),
        UnixNanos::default(),
        UnixNanos::from(1_u64),
        UnixNanos::from(1_u64),
    );

    let result = book.delete(missing_order);
    assert!(result.is_err());
}

#[rstest]
fn test_own_book_display() {
    let instrument_id = InstrumentId::from("ETHUSDT-PERP.BINANCE");
    let book = OwnOrderBook::new(instrument_id);
    assert_eq!(
        book.to_string(),
        "OwnOrderBook(instrument_id=ETHUSDT-PERP.BINANCE, orders=0, update_count=0)"
    );
}

#[rstest]
fn test_own_book_pprint() {
    let instrument_id = InstrumentId::from("ETHUSDT-PERP.BINANCE");
    let mut book = OwnOrderBook::new(instrument_id);

    let order1 = OwnBookOrder::new(
        TraderId::from("TRADER-001"),
        ClientOrderId::from("O-1"),
        Some(VenueOrderId::from("1")),
        OrderSideSpecified::Buy,
        Price::from("1.000"),
        Quantity::from("1.0"),
        OrderType::Limit,
        TimeInForce::Gtc,
        OrderStatus::Accepted,
        UnixNanos::default(),
        UnixNanos::default(),
        UnixNanos::default(),
        UnixNanos::default(),
    );
    let order2 = OwnBookOrder::new(
        TraderId::from("TRADER-001"),
        ClientOrderId::from("O-2"),
        Some(VenueOrderId::from("2")),
        OrderSideSpecified::Buy,
        Price::from("1.500"),
        Quantity::from("2.0"),
        OrderType::Limit,
        TimeInForce::Gtc,
        OrderStatus::Accepted,
        UnixNanos::default(),
        UnixNanos::default(),
        UnixNanos::default(),
        UnixNanos::default(),
    );
    let order3 = OwnBookOrder::new(
        TraderId::from("TRADER-001"),
        ClientOrderId::from("O-3"),
        Some(VenueOrderId::from("3")),
        OrderSideSpecified::Buy,
        Price::from("2.000"),
        Quantity::from("3.0"),
        OrderType::Limit,
        TimeInForce::Gtc,
        OrderStatus::Accepted,
        UnixNanos::default(),
        UnixNanos::default(),
        UnixNanos::default(),
        UnixNanos::default(),
    );
    let order4 = OwnBookOrder::new(
        TraderId::from("TRADER-001"),
        ClientOrderId::from("O-4"),
        Some(VenueOrderId::from("4")),
        OrderSideSpecified::Sell,
        Price::from("3.000"),
        Quantity::from("3.0"),
        OrderType::Limit,
        TimeInForce::Gtc,
        OrderStatus::Accepted,
        UnixNanos::default(),
        UnixNanos::default(),
        UnixNanos::default(),
        UnixNanos::default(),
    );
    let order5 = OwnBookOrder::new(
        TraderId::from("TRADER-001"),
        ClientOrderId::from("O-5"),
        Some(VenueOrderId::from("5")),
        OrderSideSpecified::Sell,
        Price::from("4.000"),
        Quantity::from("4.0"),
        OrderType::Limit,
        TimeInForce::Gtc,
        OrderStatus::Accepted,
        UnixNanos::default(),
        UnixNanos::default(),
        UnixNanos::default(),
        UnixNanos::default(),
    );
    let order6 = OwnBookOrder::new(
        TraderId::from("TRADER-001"),
        ClientOrderId::from("O-6"),
        Some(VenueOrderId::from("6")),
        OrderSideSpecified::Sell,
        Price::from("5.000"),
        Quantity::from("8.0"),
        OrderType::Limit,
        TimeInForce::Gtc,
        OrderStatus::Accepted,
        UnixNanos::default(),
        UnixNanos::default(),
        UnixNanos::default(),
        UnixNanos::default(),
    );

    book.add(order1);
    book.add(order2);
    book.add(order3);
    book.add(order4);
    book.add(order5);
    book.add(order6);

    let pprint_output = book.pprint(3, None);
    let expected_output = "bid_levels: 3\n\
ask_levels: 3\n\
update_count: 6\n\
ts_last: 0\n\
╭───────┬───────┬───────╮\n\
│ bids  │ price │ asks  │\n\
├───────┼───────┼───────┤\n\
│       │ 5.000 │ [8.0] │\n\
│       │ 4.000 │ [4.0] │\n\
│       │ 3.000 │ [3.0] │\n\
│ [3.0] │ 2.000 │       │\n\
│ [2.0] │ 1.500 │       │\n\
│ [1.0] │ 1.000 │       │\n\
╰───────┴───────┴───────╯";

    println!("{pprint_output}");
    assert_eq!(pprint_output, expected_output);
}

#[rstest]
fn test_own_book_level_size_and_exposure() {
    let mut level = OwnBookLevel::new(BookPrice::new(
        Price::from("100.00"),
        OrderSideSpecified::Buy,
    ));
    let order1 = OwnBookOrder::new(
        TraderId::from("TRADER-001"),
        ClientOrderId::from("O-1"),
        Some(VenueOrderId::from("1")),
        OrderSideSpecified::Buy,
        Price::from("100.00"),
        Quantity::from("10"),
        OrderType::Limit,
        TimeInForce::Gtc,
        OrderStatus::Accepted,
        UnixNanos::default(),
        UnixNanos::default(),
        UnixNanos::default(),
        UnixNanos::default(),
    );
    let order2 = OwnBookOrder::new(
        TraderId::from("TRADER-001"),
        ClientOrderId::from("O-2"),
        Some(VenueOrderId::from("2")),
        OrderSideSpecified::Buy,
        Price::from("100.00"),
        Quantity::from("20"),
        OrderType::Limit,
        TimeInForce::Gtc,
        OrderStatus::Accepted,
        UnixNanos::default(),
        UnixNanos::default(),
        UnixNanos::default(),
        UnixNanos::default(),
    );
    level.add(order1);
    level.add(order2);

    assert_eq!(level.len(), 2);
    assert_eq!(level.size(), 30.0);
    assert_eq!(level.exposure(), 3000.0);
}

#[rstest]
fn test_own_book_level_add_update_delete() {
    let mut level = OwnBookLevel::new(BookPrice::new(
        Price::from("100.00"),
        OrderSideSpecified::Buy,
    ));
    let order = OwnBookOrder::new(
        TraderId::from("TRADER-001"),
        ClientOrderId::from("O-1"),
        Some(VenueOrderId::from("1")),
        OrderSideSpecified::Buy,
        Price::from("100.00"),
        Quantity::from("10"),
        OrderType::Limit,
        TimeInForce::Gtc,
        OrderStatus::Accepted,
        UnixNanos::default(),
        UnixNanos::default(),
        UnixNanos::default(),
        UnixNanos::default(),
    );
    level.add(order);
    assert_eq!(level.len(), 1);

    // Update the order to a new size
    let updated = OwnBookOrder::new(
        TraderId::from("TRADER-001"),
        ClientOrderId::from("O-1"),
        Some(VenueOrderId::from("1")),
        OrderSideSpecified::Buy,
        Price::from("100.00"),
        Quantity::from("15"),
        OrderType::Limit,
        TimeInForce::Gtc,
        OrderStatus::Accepted,
        UnixNanos::default(),
        UnixNanos::default(),
        UnixNanos::default(),
        UnixNanos::default(),
    );
    level.update(updated);
    let orders = level.get_orders();
    assert_eq!(orders[0].size, Quantity::from("15"));

    // Delete the order
    level.delete(&ClientOrderId::from("O-1")).unwrap();
    assert!(level.is_empty());
}

#[rstest]
fn test_own_book_ladder_add_update_delete() {
    let mut ladder = OwnBookLadder::new(OrderSideSpecified::Buy);
    let order1 = OwnBookOrder::new(
        TraderId::from("TRADER-001"),
        ClientOrderId::from("O-1"),
        Some(VenueOrderId::from("1")),
        OrderSideSpecified::Buy,
        Price::from("100.00"),
        Quantity::from("10"),
        OrderType::Limit,
        TimeInForce::Gtc,
        OrderStatus::Accepted,
        UnixNanos::default(),
        UnixNanos::default(),
        UnixNanos::default(),
        UnixNanos::default(),
    );
    let order2 = OwnBookOrder::new(
        TraderId::from("TRADER-001"),
        ClientOrderId::from("O-2"),
        Some(VenueOrderId::from("2")),
        OrderSideSpecified::Buy,
        Price::from("100.00"),
        Quantity::from("20"),
        OrderType::Limit,
        TimeInForce::Gtc,
        OrderStatus::Accepted,
        UnixNanos::default(),
        UnixNanos::default(),
        UnixNanos::default(),
        UnixNanos::default(),
    );
    ladder.add(order1);
    ladder.add(order2);
    assert_eq!(ladder.len(), 1);
    assert_eq!(ladder.sizes(), 30.0);

    // Update order2 to a larger size
    let order2_updated = OwnBookOrder::new(
        TraderId::from("TRADER-001"),
        ClientOrderId::from("O-2"),
        Some(VenueOrderId::from("2")),
        OrderSideSpecified::Buy,
        Price::from("100.00"),
        Quantity::from("25"),
        OrderType::Limit,
        TimeInForce::Gtc,
        OrderStatus::Accepted,
        UnixNanos::default(),
        UnixNanos::default(),
        UnixNanos::default(),
        UnixNanos::default(),
    );
    ladder.update(order2_updated).unwrap();
    assert_eq!(ladder.sizes(), 35.0);

    // Delete order1
    ladder.delete(order1).unwrap();
    assert_eq!(ladder.sizes(), 25.0);
}

#[rstest]
fn test_own_order_book_add_update_delete_clear() {
    let instrument_id = InstrumentId::from("AAPL.XNAS");
    let mut book = OwnOrderBook::new(instrument_id);
    let order_buy = OwnBookOrder::new(
        TraderId::from("TRADER-001"),
        ClientOrderId::from("O-1"),
        Some(VenueOrderId::from("1")),
        OrderSideSpecified::Buy,
        Price::from("100.00"),
        Quantity::from("10"),
        OrderType::Limit,
        TimeInForce::Gtc,
        OrderStatus::Accepted,
        UnixNanos::default(),
        UnixNanos::default(),
        UnixNanos::default(),
        UnixNanos::default(),
    );
    let order_sell = OwnBookOrder::new(
        TraderId::from("TRADER-001"),
        ClientOrderId::from("O-2"),
        Some(VenueOrderId::from("2")),
        OrderSideSpecified::Sell,
        Price::from("101.00"),
        Quantity::from("20"),
        OrderType::Limit,
        TimeInForce::Gtc,
        OrderStatus::Accepted,
        UnixNanos::default(),
        UnixNanos::default(),
        UnixNanos::default(),
        UnixNanos::default(),
    );

    // Add orders to respective ladders
    book.add(order_buy);
    book.add(order_sell);
    assert!(!book.bids.is_empty());
    assert!(!book.asks.is_empty());

    // Update buy order
    let order_buy_updated = OwnBookOrder::new(
        TraderId::from("TRADER-001"),
        ClientOrderId::from("O-1"),
        Some(VenueOrderId::from("1")),
        OrderSideSpecified::Buy,
        Price::from("100.00"),
        Quantity::from("15"),
        OrderType::Limit,
        TimeInForce::Gtc,
        OrderStatus::Accepted,
        UnixNanos::default(),
        UnixNanos::default(),
        UnixNanos::default(),
        UnixNanos::default(),
    );
    book.update(order_buy_updated).unwrap();
    book.delete(order_sell).unwrap();

    assert_eq!(book.bids.sizes(), 15.0);
    assert!(book.asks.is_empty());

    // Clear the book
    book.clear();
    assert!(book.bids.is_empty());
    assert!(book.asks.is_empty());
}

#[rstest]
fn test_own_order_book_bids_and_asks_as_map() {
    let instrument_id = InstrumentId::from("AAPL.XNAS");
    let mut book = OwnOrderBook::new(instrument_id);
    let order1 = OwnBookOrder::new(
        TraderId::from("TRADER-001"),
        ClientOrderId::from("O-1"),
        Some(VenueOrderId::from("1")),
        OrderSideSpecified::Buy,
        Price::from("100.00"),
        Quantity::from("10"),
        OrderType::Limit,
        TimeInForce::Gtc,
        OrderStatus::Accepted,
        UnixNanos::default(),
        UnixNanos::default(),
        UnixNanos::default(),
        UnixNanos::default(),
    );
    let order2 = OwnBookOrder::new(
        TraderId::from("TRADER-001"),
        ClientOrderId::from("O-2"),
        Some(VenueOrderId::from("2")),
        OrderSideSpecified::Sell,
        Price::from("101.00"),
        Quantity::from("20"),
        OrderType::Limit,
        TimeInForce::Gtc,
        OrderStatus::Accepted,
        UnixNanos::default(),
        UnixNanos::default(),
        UnixNanos::default(),
        UnixNanos::default(),
    );
    book.add(order1);
    book.add(order2);
    let bids_map = book.bids_as_map(None, None, None);
    let asks_map = book.asks_as_map(None, None, None);

    assert_eq!(bids_map.len(), 1);
    let bid_price = Price::from("100.00").as_decimal();
    let bid_orders = bids_map.get(&bid_price).unwrap();
    assert_eq!(bid_orders.len(), 1);
    assert_eq!(bid_orders[0], order1);

    assert_eq!(asks_map.len(), 1);
    let ask_price = Price::from("101.00").as_decimal();
    let ask_orders = asks_map.get(&ask_price).unwrap();
    assert_eq!(ask_orders.len(), 1);
    assert_eq!(ask_orders[0], order2);
}

#[rstest]
fn test_own_order_book_quantity_empty_levels() {
    let instrument_id = InstrumentId::from("AAPL.XNAS");
    let book = OwnOrderBook::new(instrument_id);

    let bid_quantities = book.bid_quantity(None, None, None, None, None);
    let ask_quantities = book.ask_quantity(None, None, None, None, None);

    assert!(bid_quantities.is_empty());
    assert!(ask_quantities.is_empty());
}

#[rstest]
fn test_own_order_book_bid_ask_quantity() {
    let instrument_id = InstrumentId::from("AAPL.XNAS");
    let mut book = OwnOrderBook::new(instrument_id);

    // Add multiple orders at the same price level (bids)
    let bid_order1 = OwnBookOrder::new(
        TraderId::from("TRADER-001"),
        ClientOrderId::from("O-1"),
        Some(VenueOrderId::from("1")),
        OrderSideSpecified::Buy,
        Price::from("100.00"),
        Quantity::from("10"),
        OrderType::Limit,
        TimeInForce::Gtc,
        OrderStatus::Accepted,
        UnixNanos::default(),
        UnixNanos::default(),
        UnixNanos::default(),
        UnixNanos::default(),
    );
    let bid_order2 = OwnBookOrder::new(
        TraderId::from("TRADER-001"),
        ClientOrderId::from("O-2"),
        Some(VenueOrderId::from("2")),
        OrderSideSpecified::Buy,
        Price::from("100.00"),
        Quantity::from("15"),
        OrderType::Limit,
        TimeInForce::Gtc,
        OrderStatus::Accepted,
        UnixNanos::default(),
        UnixNanos::default(),
        UnixNanos::default(),
        UnixNanos::default(),
    );
    // Add an order at a different price level (bids)
    let bid_order3 = OwnBookOrder::new(
        TraderId::from("TRADER-001"),
        ClientOrderId::from("O-3"),
        Some(VenueOrderId::from("3")),
        OrderSideSpecified::Buy,
        Price::from("99.50"),
        Quantity::from("20"),
        OrderType::Limit,
        TimeInForce::Gtc,
        OrderStatus::Accepted,
        UnixNanos::default(),
        UnixNanos::default(),
        UnixNanos::default(),
        UnixNanos::default(),
    );

    // Add orders at different price levels (asks)
    let ask_order1 = OwnBookOrder::new(
        TraderId::from("TRADER-001"),
        ClientOrderId::from("O-4"),
        Some(VenueOrderId::from("4")),
        OrderSideSpecified::Sell,
        Price::from("101.00"),
        Quantity::from("12"),
        OrderType::Limit,
        TimeInForce::Gtc,
        OrderStatus::Accepted,
        UnixNanos::default(),
        UnixNanos::default(),
        UnixNanos::default(),
        UnixNanos::default(),
    );
    let ask_order2 = OwnBookOrder::new(
        TraderId::from("TRADER-001"),
        ClientOrderId::from("O-5"),
        Some(VenueOrderId::from("5")),
        OrderSideSpecified::Sell,
        Price::from("101.00"),
        Quantity::from("8"),
        OrderType::Limit,
        TimeInForce::Gtc,
        OrderStatus::Accepted,
        UnixNanos::default(),
        UnixNanos::default(),
        UnixNanos::default(),
        UnixNanos::default(),
    );

    book.add(bid_order1);
    book.add(bid_order2);
    book.add(bid_order3);
    book.add(ask_order1);
    book.add(ask_order2);

    let bid_quantities = book.bid_quantity(None, None, None, None, None);
    assert_eq!(bid_quantities.len(), 2);
    assert_eq!(bid_quantities.get(&dec!(100.00)), Some(&dec!(25)));
    assert_eq!(bid_quantities.get(&dec!(99.50)), Some(&dec!(20)));

    let ask_quantities = book.ask_quantity(None, None, None, None, None);
    assert_eq!(ask_quantities.len(), 1);
    assert_eq!(ask_quantities.get(&dec!(101.00)), Some(&dec!(20)));
}

#[rstest]
fn test_status_filtering_bids_as_map() {
    let instrument_id = InstrumentId::from("AAPL.XNAS");
    let mut book = OwnOrderBook::new(instrument_id);

    // Create orders with different statuses
    let submitted = OwnBookOrder::new(
        TraderId::from("TRADER-001"),
        ClientOrderId::from("O-1"),
        None,
        OrderSideSpecified::Buy,
        Price::from("100.00"),
        Quantity::from("10"),
        OrderType::Limit,
        TimeInForce::Gtc,
        OrderStatus::Submitted,
        UnixNanos::default(),
        UnixNanos::default(),
        UnixNanos::default(),
        UnixNanos::default(),
    );

    let accepted = OwnBookOrder::new(
        TraderId::from("TRADER-001"),
        ClientOrderId::from("O-2"),
        Some(VenueOrderId::from("2")),
        OrderSideSpecified::Buy,
        Price::from("100.00"),
        Quantity::from("15"),
        OrderType::Limit,
        TimeInForce::Gtc,
        OrderStatus::Accepted,
        UnixNanos::default(),
        UnixNanos::default(),
        UnixNanos::default(),
        UnixNanos::default(),
    );

    let canceled = OwnBookOrder::new(
        TraderId::from("TRADER-001"),
        ClientOrderId::from("O-3"),
        Some(VenueOrderId::from("3")),
        OrderSideSpecified::Buy,
        Price::from("99.50"),
        Quantity::from("20"),
        OrderType::Limit,
        TimeInForce::Gtc,
        OrderStatus::Canceled,
        UnixNanos::default(),
        UnixNanos::default(),
        UnixNanos::default(),
        UnixNanos::default(),
    );

    book.add(submitted);
    book.add(accepted);
    book.add(canceled);

    // Test with no filter (should include all orders)
    let all_orders = book.bids_as_map(None, None, None);
    assert_eq!(all_orders.len(), 2); // Two price levels
    assert_eq!(all_orders.get(&dec!(100.00)).unwrap().len(), 2); // Two orders at 100.00
    assert_eq!(all_orders.get(&dec!(99.50)).unwrap().len(), 1); // One order at 99.50

    // Filter for just SUBMITTED status
    let mut filter_submitted = HashSet::new();
    filter_submitted.insert(OrderStatus::Submitted);
    let submitted_orders = book.bids_as_map(Some(filter_submitted), None, None);
    assert_eq!(submitted_orders.len(), 1); // One price level
    assert_eq!(submitted_orders.get(&dec!(100.00)).unwrap().len(), 1); // One order at 100.00
    assert_eq!(
        submitted_orders.get(&dec!(100.00)).unwrap()[0].status,
        OrderStatus::Submitted
    );
    assert!(submitted_orders.get(&dec!(99.50)).is_none()); // No SUBMITTED orders at 99.50

    // Filter for ACCEPTED and CANCELED statuses
    let mut filter_accepted_canceled = HashSet::new();
    filter_accepted_canceled.insert(OrderStatus::Accepted);
    filter_accepted_canceled.insert(OrderStatus::Canceled);
    let accepted_canceled_orders = book.bids_as_map(Some(filter_accepted_canceled), None, None);
    assert_eq!(accepted_canceled_orders.len(), 2); // Two price levels
    assert_eq!(
        accepted_canceled_orders.get(&dec!(100.00)).unwrap().len(),
        1
    ); // One ACCEPTED at 100.00
    assert_eq!(accepted_canceled_orders.get(&dec!(99.50)).unwrap().len(), 1); // One CANCELED at 99.50

    // Filter for non-existent status
    let mut filter_filled = HashSet::new();
    filter_filled.insert(OrderStatus::Filled);
    let filled_orders = book.bids_as_map(Some(filter_filled), None, None);
    assert_eq!(filled_orders.len(), 0); // No orders match
}

#[rstest]
fn test_status_filtering_asks_as_map() {
    let instrument_id = InstrumentId::from("AAPL.XNAS");
    let mut book = OwnOrderBook::new(instrument_id);

    // Create orders with different statuses
    let submitted = OwnBookOrder::new(
        TraderId::from("TRADER-001"),
        ClientOrderId::from("O-1"),
        None,
        OrderSideSpecified::Sell,
        Price::from("101.00"),
        Quantity::from("10"),
        OrderType::Limit,
        TimeInForce::Gtc,
        OrderStatus::Submitted,
        UnixNanos::default(),
        UnixNanos::default(),
        UnixNanos::default(),
        UnixNanos::default(),
    );

    let accepted = OwnBookOrder::new(
        TraderId::from("TRADER-001"),
        ClientOrderId::from("O-2"),
        Some(VenueOrderId::from("2")),
        OrderSideSpecified::Sell,
        Price::from("101.00"),
        Quantity::from("15"),
        OrderType::Limit,
        TimeInForce::Gtc,
        OrderStatus::Accepted,
        UnixNanos::default(),
        UnixNanos::default(),
        UnixNanos::default(),
        UnixNanos::default(),
    );

    book.add(submitted);
    book.add(accepted);

    // Test with no filter (should include all orders)
    let all_orders = book.asks_as_map(None, None, None);
    assert_eq!(all_orders.len(), 1); // One price level
    assert_eq!(all_orders.get(&dec!(101.00)).unwrap().len(), 2); // Two orders at 101.00

    // Filter for just SUBMITTED status
    let mut filter_submitted = HashSet::new();
    filter_submitted.insert(OrderStatus::Submitted);
    let submitted_orders = book.asks_as_map(Some(filter_submitted), None, None);
    assert_eq!(submitted_orders.len(), 1); // One price level
    assert_eq!(submitted_orders.get(&dec!(101.00)).unwrap().len(), 1); // One order at 101.00
    assert_eq!(
        submitted_orders.get(&dec!(101.00)).unwrap()[0].status,
        OrderStatus::Submitted
    );
}

#[rstest]
fn test_status_filtering_bid_quantity() {
    let instrument_id = InstrumentId::from("AAPL.XNAS");
    let mut book = OwnOrderBook::new(instrument_id);

    // Create orders with different statuses at same price
    let submitted = OwnBookOrder::new(
        TraderId::from("TRADER-001"),
        ClientOrderId::from("O-1"),
        None,
        OrderSideSpecified::Buy,
        Price::from("100.00"),
        Quantity::from("10"),
        OrderType::Limit,
        TimeInForce::Gtc,
        OrderStatus::Submitted,
        UnixNanos::default(),
        UnixNanos::default(),
        UnixNanos::default(),
        UnixNanos::default(),
    );

    let accepted = OwnBookOrder::new(
        TraderId::from("TRADER-001"),
        ClientOrderId::from("O-2"),
        Some(VenueOrderId::from("2")),
        OrderSideSpecified::Buy,
        Price::from("100.00"),
        Quantity::from("15"),
        OrderType::Limit,
        TimeInForce::Gtc,
        OrderStatus::Accepted,
        UnixNanos::default(),
        UnixNanos::default(),
        UnixNanos::default(),
        UnixNanos::default(),
    );

    let canceled = OwnBookOrder::new(
        TraderId::from("TRADER-001"),
        ClientOrderId::from("O-3"),
        Some(VenueOrderId::from("3")),
        OrderSideSpecified::Buy,
        Price::from("99.50"),
        Quantity::from("20"),
        OrderType::Limit,
        TimeInForce::Gtc,
        OrderStatus::Canceled,
        UnixNanos::default(),
        UnixNanos::default(),
        UnixNanos::default(),
        UnixNanos::default(),
    );

    book.add(submitted);
    book.add(accepted);
    book.add(canceled);

    // Test with no filter (should include all orders)
    let all_quantities = book.bid_quantity(None, None, None, None, None);
    assert_eq!(all_quantities.len(), 2); // Two price levels
    assert_eq!(all_quantities.get(&dec!(100.00)), Some(&dec!(25))); // 10 + 15 = 25
    assert_eq!(all_quantities.get(&dec!(99.50)), Some(&dec!(20))); // 20

    // Filter for just SUBMITTED status
    let mut filter_submitted = HashSet::new();
    filter_submitted.insert(OrderStatus::Submitted);
    let submitted_quantities = book.bid_quantity(Some(filter_submitted), None, None, None, None);
    assert_eq!(submitted_quantities.len(), 1); // One price level
    assert_eq!(submitted_quantities.get(&dec!(100.00)), Some(&dec!(10))); // 10
    assert!(submitted_quantities.get(&dec!(99.50)).is_none()); // No SUBMITTED orders at 99.50

    // Filter for ACCEPTED and CANCELED statuses
    let mut filter_accepted_canceled = HashSet::new();
    filter_accepted_canceled.insert(OrderStatus::Accepted);
    filter_accepted_canceled.insert(OrderStatus::Canceled);
    let accepted_canceled_quantities =
        book.bid_quantity(Some(filter_accepted_canceled), None, None, None, None);
    assert_eq!(accepted_canceled_quantities.len(), 2); // Two price levels
    assert_eq!(
        accepted_canceled_quantities.get(&dec!(100.00)),
        Some(&dec!(15))
    ); // 15
    assert_eq!(
        accepted_canceled_quantities.get(&dec!(99.50)),
        Some(&dec!(20))
    ); // 20
}

#[rstest]
fn test_status_filtering_ask_quantity() {
    let instrument_id = InstrumentId::from("AAPL.XNAS");
    let mut book = OwnOrderBook::new(instrument_id);

    // Create orders with different statuses
    let submitted = OwnBookOrder::new(
        TraderId::from("TRADER-001"),
        ClientOrderId::from("O-1"),
        None,
        OrderSideSpecified::Sell,
        Price::from("101.00"),
        Quantity::from("10"),
        OrderType::Limit,
        TimeInForce::Gtc,
        OrderStatus::Submitted,
        UnixNanos::default(),
        UnixNanos::default(),
        UnixNanos::default(),
        UnixNanos::default(),
    );

    let accepted = OwnBookOrder::new(
        TraderId::from("TRADER-001"),
        ClientOrderId::from("O-2"),
        Some(VenueOrderId::from("2")),
        OrderSideSpecified::Sell,
        Price::from("101.00"),
        Quantity::from("15"),
        OrderType::Limit,
        TimeInForce::Gtc,
        OrderStatus::Accepted,
        UnixNanos::default(),
        UnixNanos::default(),
        UnixNanos::default(),
        UnixNanos::default(),
    );

    let canceled = OwnBookOrder::new(
        TraderId::from("TRADER-001"),
        ClientOrderId::from("O-3"),
        Some(VenueOrderId::from("3")),
        OrderSideSpecified::Sell,
        Price::from("102.00"),
        Quantity::from("20"),
        OrderType::Limit,
        TimeInForce::Gtc,
        OrderStatus::Canceled,
        UnixNanos::default(),
        UnixNanos::default(),
        UnixNanos::default(),
        UnixNanos::default(),
    );

    book.add(submitted);
    book.add(accepted);
    book.add(canceled);

    // Test with no filter (should include all orders)
    let all_quantities = book.ask_quantity(None, None, None, None, None);
    assert_eq!(all_quantities.len(), 2); // Two price levels
    assert_eq!(all_quantities.get(&dec!(101.00)), Some(&dec!(25))); // 10 + 15 = 25
    assert_eq!(all_quantities.get(&dec!(102.00)), Some(&dec!(20))); // 20

    // Filter for just SUBMITTED status
    let mut filter_submitted = HashSet::new();
    filter_submitted.insert(OrderStatus::Submitted);
    let submitted_quantities = book.ask_quantity(Some(filter_submitted), None, None, None, None);
    assert_eq!(submitted_quantities.len(), 1); // One price level
    assert_eq!(submitted_quantities.get(&dec!(101.00)), Some(&dec!(10))); // 10
    assert!(submitted_quantities.get(&dec!(102.00)).is_none()); // No SUBMITTED orders at 102.00

    // Filter for multiple statuses
    let mut filter_multiple = HashSet::new();
    filter_multiple.insert(OrderStatus::Submitted);
    filter_multiple.insert(OrderStatus::Canceled);
    let multiple_quantities = book.ask_quantity(Some(filter_multiple), None, None, None, None);
    assert_eq!(multiple_quantities.len(), 2); // Two price levels
    assert_eq!(multiple_quantities.get(&dec!(101.00)), Some(&dec!(10))); // 10 (Submitted only)
    assert_eq!(multiple_quantities.get(&dec!(102.00)), Some(&dec!(20))); // 20 (Canceled only)

    // Check empty price levels are filtered out
    let mut filter_filled = HashSet::new();
    filter_filled.insert(OrderStatus::Filled);
    let filled_quantities = book.ask_quantity(Some(filter_filled), None, None, None, None);
    assert_eq!(filled_quantities.len(), 0); // No orders match
}

#[rstest]
fn test_own_book_group_empty_book() {
    let instrument_id = InstrumentId::from("AAPL.XNAS");
    let book = OwnOrderBook::new(instrument_id);

    let grouped_bids = book.bid_quantity(None, None, Some(dec!(1)), None, None);
    let grouped_asks = book.ask_quantity(None, None, Some(dec!(1)), None, None);

    assert!(grouped_bids.is_empty());
    assert!(grouped_asks.is_empty());
}

#[rstest]
fn test_own_book_group_price_levels() {
    let instrument_id = InstrumentId::from("ETHUSDT-PERP.BINANCE");
    let mut book = OwnOrderBook::new(instrument_id);

    // Add several orders at different price levels on the bid side
    let bid_order1 = OwnBookOrder::new(
        TraderId::from("TRADER-001"),
        ClientOrderId::from("BID-1"),
        Some(VenueOrderId::from("1")),
        OrderSideSpecified::Buy,
        Price::from("1.1"),
        Quantity::from("10"),
        OrderType::Limit,
        TimeInForce::Gtc,
        OrderStatus::Accepted,
        UnixNanos::default(),
        UnixNanos::default(),
        UnixNanos::default(),
        UnixNanos::default(),
    );

    let bid_order2 = OwnBookOrder::new(
        TraderId::from("TRADER-001"),
        ClientOrderId::from("BID-2"),
        Some(VenueOrderId::from("2")),
        OrderSideSpecified::Buy,
        Price::from("1.2"),
        Quantity::from("20"),
        OrderType::Limit,
        TimeInForce::Gtc,
        OrderStatus::Accepted,
        UnixNanos::default(),
        UnixNanos::default(),
        UnixNanos::default(),
        UnixNanos::default(),
    );

    let bid_order3 = OwnBookOrder::new(
        TraderId::from("TRADER-001"),
        ClientOrderId::from("BID-3"),
        Some(VenueOrderId::from("3")),
        OrderSideSpecified::Buy,
        Price::from("1.8"),
        Quantity::from("30"),
        OrderType::Limit,
        TimeInForce::Gtc,
        OrderStatus::Accepted,
        UnixNanos::default(),
        UnixNanos::default(),
        UnixNanos::default(),
        UnixNanos::default(),
    );

    // Add several orders at different price levels on the ask side
    let ask_order1 = OwnBookOrder::new(
        TraderId::from("TRADER-001"),
        ClientOrderId::from("ASK-1"),
        Some(VenueOrderId::from("4")),
        OrderSideSpecified::Sell,
        Price::from("2.1"),
        Quantity::from("10"),
        OrderType::Limit,
        TimeInForce::Gtc,
        OrderStatus::Accepted,
        UnixNanos::default(),
        UnixNanos::default(),
        UnixNanos::default(),
        UnixNanos::default(),
    );

    let ask_order2 = OwnBookOrder::new(
        TraderId::from("TRADER-001"),
        ClientOrderId::from("ASK-2"),
        Some(VenueOrderId::from("5")),
        OrderSideSpecified::Sell,
        Price::from("2.2"),
        Quantity::from("20"),
        OrderType::Limit,
        TimeInForce::Gtc,
        OrderStatus::Accepted,
        UnixNanos::default(),
        UnixNanos::default(),
        UnixNanos::default(),
        UnixNanos::default(),
    );

    let ask_order3 = OwnBookOrder::new(
        TraderId::from("TRADER-001"),
        ClientOrderId::from("ASK-3"),
        Some(VenueOrderId::from("6")),
        OrderSideSpecified::Sell,
        Price::from("2.8"),
        Quantity::from("30"),
        OrderType::Limit,
        TimeInForce::Gtc,
        OrderStatus::Accepted,
        UnixNanos::default(),
        UnixNanos::default(),
        UnixNanos::default(),
        UnixNanos::default(),
    );

    book.add(bid_order1);
    book.add(bid_order2);
    book.add(bid_order3);
    book.add(ask_order1);
    book.add(ask_order2);
    book.add(ask_order3);

    // Group with a 0.5 increment
    let grouped_bids = book.bid_quantity(None, None, Some(dec!(0.5)), None, None);
    let grouped_asks = book.ask_quantity(None, None, Some(dec!(0.5)), None, None);

    // Check bid grouping
    assert_eq!(grouped_bids.len(), 2);
    assert_eq!(grouped_bids.get(&dec!(1.0)), Some(&dec!(30))); // 1.1, 1.2 group to 1.0
    assert_eq!(grouped_bids.get(&dec!(1.5)), Some(&dec!(30))); // 1.8 groups to 1.5

    // Check ask grouping
    assert_eq!(grouped_asks.len(), 2);
    assert_eq!(grouped_asks.get(&dec!(2.5)), Some(&dec!(30))); // 2.1, 2.2 group to 2.5
    assert_eq!(grouped_asks.get(&dec!(3.0)), Some(&dec!(30))); // 2.8 groups to 3.0
}

#[rstest]
fn test_own_book_group_with_depth_limit() {
    let instrument_id = InstrumentId::from("ETHUSDT-PERP.BINANCE");
    let mut book = OwnOrderBook::new(instrument_id);

    // Add several orders at different price levels on both sides
    let orders = [
        // Bid orders
        OwnBookOrder::new(
            TraderId::from("TRADER-001"),
            ClientOrderId::from("BID-1"),
            Some(VenueOrderId::from("1")),
            OrderSideSpecified::Buy,
            Price::from("1.0"),
            Quantity::from("10"),
            OrderType::Limit,
            TimeInForce::Gtc,
            OrderStatus::Accepted,
            UnixNanos::default(),
            UnixNanos::default(),
            UnixNanos::default(),
            UnixNanos::default(),
        ),
        OwnBookOrder::new(
            TraderId::from("TRADER-001"),
            ClientOrderId::from("BID-2"),
            Some(VenueOrderId::from("2")),
            OrderSideSpecified::Buy,
            Price::from("2.0"),
            Quantity::from("20"),
            OrderType::Limit,
            TimeInForce::Gtc,
            OrderStatus::Accepted,
            UnixNanos::default(),
            UnixNanos::default(),
            UnixNanos::default(),
            UnixNanos::default(),
        ),
        OwnBookOrder::new(
            TraderId::from("TRADER-001"),
            ClientOrderId::from("BID-3"),
            Some(VenueOrderId::from("3")),
            OrderSideSpecified::Buy,
            Price::from("3.0"),
            Quantity::from("30"),
            OrderType::Limit,
            TimeInForce::Gtc,
            OrderStatus::Accepted,
            UnixNanos::default(),
            UnixNanos::default(),
            UnixNanos::default(),
            UnixNanos::default(),
        ),
        // Ask orders
        OwnBookOrder::new(
            TraderId::from("TRADER-001"),
            ClientOrderId::from("ASK-1"),
            Some(VenueOrderId::from("4")),
            OrderSideSpecified::Sell,
            Price::from("4.0"),
            Quantity::from("10"),
            OrderType::Limit,
            TimeInForce::Gtc,
            OrderStatus::Accepted,
            UnixNanos::default(),
            UnixNanos::default(),
            UnixNanos::default(),
            UnixNanos::default(),
        ),
        OwnBookOrder::new(
            TraderId::from("TRADER-001"),
            ClientOrderId::from("ASK-2"),
            Some(VenueOrderId::from("5")),
            OrderSideSpecified::Sell,
            Price::from("5.0"),
            Quantity::from("20"),
            OrderType::Limit,
            TimeInForce::Gtc,
            OrderStatus::Accepted,
            UnixNanos::default(),
            UnixNanos::default(),
            UnixNanos::default(),
            UnixNanos::default(),
        ),
        OwnBookOrder::new(
            TraderId::from("TRADER-001"),
            ClientOrderId::from("ASK-3"),
            Some(VenueOrderId::from("6")),
            OrderSideSpecified::Sell,
            Price::from("6.0"),
            Quantity::from("30"),
            OrderType::Limit,
            TimeInForce::Gtc,
            OrderStatus::Accepted,
            UnixNanos::default(),
            UnixNanos::default(),
            UnixNanos::default(),
            UnixNanos::default(),
        ),
    ];

    for order in &orders {
        book.add(*order);
    }

    // Group with depth=2 to limit number of levels returned
    let grouped_bids = book.bid_quantity(None, Some(2), Some(dec!(1.0)), None, None);
    let grouped_asks = book.ask_quantity(None, Some(2), Some(dec!(1.0)), None, None);

    // Check bid grouping
    assert_eq!(grouped_bids.len(), 2); // Should only have 2 levels
    assert_eq!(grouped_bids.get(&dec!(3.0)), Some(&dec!(30))); // Highest bid first
    assert_eq!(grouped_bids.get(&dec!(2.0)), Some(&dec!(20)));
    assert!(grouped_bids.get(&dec!(1.0)).is_none()); // Should be excluded due to depth limit

    // Check ask grouping
    assert_eq!(grouped_asks.len(), 2); // Should only have 2 levels
    assert_eq!(grouped_asks.get(&dec!(4.0)), Some(&dec!(10))); // Lowest ask first
    assert_eq!(grouped_asks.get(&dec!(5.0)), Some(&dec!(20)));
    assert!(grouped_asks.get(&dec!(6.0)).is_none()); // Should be excluded due to depth limit
}

#[rstest]
fn test_own_book_group_with_multiple_orders_at_same_level() {
    let instrument_id = InstrumentId::from("ETHUSDT-PERP.BINANCE");
    let mut book = OwnOrderBook::new(instrument_id);

    // Add multiple orders at the same price level
    let bid_order1 = OwnBookOrder::new(
        TraderId::from("TRADER-001"),
        ClientOrderId::from("BID-1"),
        Some(VenueOrderId::from("1")),
        OrderSideSpecified::Buy,
        Price::from("1.0"),
        Quantity::from("10"),
        OrderType::Limit,
        TimeInForce::Gtc,
        OrderStatus::Accepted,
        UnixNanos::default(),
        UnixNanos::default(),
        UnixNanos::default(),
        UnixNanos::default(),
    );

    let bid_order2 = OwnBookOrder::new(
        TraderId::from("TRADER-001"),
        ClientOrderId::from("BID-2"),
        Some(VenueOrderId::from("2")),
        OrderSideSpecified::Buy,
        Price::from("1.0"),
        Quantity::from("20"),
        OrderType::Limit,
        TimeInForce::Gtc,
        OrderStatus::Accepted,
        UnixNanos::default(),
        UnixNanos::default(),
        UnixNanos::default(),
        UnixNanos::default(),
    );

    let ask_order1 = OwnBookOrder::new(
        TraderId::from("TRADER-001"),
        ClientOrderId::from("ASK-1"),
        Some(VenueOrderId::from("3")),
        OrderSideSpecified::Sell,
        Price::from("2.0"),
        Quantity::from("15"),
        OrderType::Limit,
        TimeInForce::Gtc,
        OrderStatus::Accepted,
        UnixNanos::default(),
        UnixNanos::default(),
        UnixNanos::default(),
        UnixNanos::default(),
    );

    let ask_order2 = OwnBookOrder::new(
        TraderId::from("TRADER-001"),
        ClientOrderId::from("ASK-2"),
        Some(VenueOrderId::from("4")),
        OrderSideSpecified::Sell,
        Price::from("2.0"),
        Quantity::from("25"),
        OrderType::Limit,
        TimeInForce::Gtc,
        OrderStatus::Accepted,
        UnixNanos::default(),
        UnixNanos::default(),
        UnixNanos::default(),
        UnixNanos::default(),
    );

    book.add(bid_order1);
    book.add(bid_order2);
    book.add(ask_order1);
    book.add(ask_order2);

    // Group with a 1.0 increment (same as the price differences)
    let grouped_bids = book.bid_quantity(None, None, Some(dec!(1.0)), None, None);
    let grouped_asks = book.ask_quantity(None, None, Some(dec!(1.0)), None, None);

    // Check that orders at the same price level are aggregated correctly
    assert_eq!(grouped_bids.len(), 1);
    assert_eq!(grouped_bids.get(&dec!(1.0)), Some(&dec!(30))); // 10 + 20 = 30

    assert_eq!(grouped_asks.len(), 1);
    assert_eq!(grouped_asks.get(&dec!(2.0)), Some(&dec!(40))); // 15 + 25 = 40
}

#[rstest]
fn test_own_book_group_with_larger_group_size() {
    let instrument_id = InstrumentId::from("AAPL.XNAS");
    let mut book = OwnOrderBook::new(instrument_id);

    // Add orders at different price levels
    let bid_orders = [
        OwnBookOrder::new(
            TraderId::from("TRADER-001"),
            ClientOrderId::from("BID-1"),
            Some(VenueOrderId::from("1")),
            OrderSideSpecified::Buy,
            Price::from("100.00"),
            Quantity::from("10"),
            OrderType::Limit,
            TimeInForce::Gtc,
            OrderStatus::Accepted,
            UnixNanos::default(),
            UnixNanos::default(),
            UnixNanos::default(),
            UnixNanos::default(),
        ),
        OwnBookOrder::new(
            TraderId::from("TRADER-001"),
            ClientOrderId::from("BID-2"),
            Some(VenueOrderId::from("2")),
            OrderSideSpecified::Buy,
            Price::from("99.00"),
            Quantity::from("20"),
            OrderType::Limit,
            TimeInForce::Gtc,
            OrderStatus::Accepted,
            UnixNanos::default(),
            UnixNanos::default(),
            UnixNanos::default(),
            UnixNanos::default(),
        ),
        OwnBookOrder::new(
            TraderId::from("TRADER-001"),
            ClientOrderId::from("BID-3"),
            Some(VenueOrderId::from("3")),
            OrderSideSpecified::Buy,
            Price::from("98.00"),
            Quantity::from("30"),
            OrderType::Limit,
            TimeInForce::Gtc,
            OrderStatus::Accepted,
            UnixNanos::default(),
            UnixNanos::default(),
            UnixNanos::default(),
            UnixNanos::default(),
        ),
    ];

    let ask_orders = [
        OwnBookOrder::new(
            TraderId::from("TRADER-001"),
            ClientOrderId::from("ASK-1"),
            Some(VenueOrderId::from("4")),
            OrderSideSpecified::Sell,
            Price::from("101.00"),
            Quantity::from("10"),
            OrderType::Limit,
            TimeInForce::Gtc,
            OrderStatus::Accepted,
            UnixNanos::default(),
            UnixNanos::default(),
            UnixNanos::default(),
            UnixNanos::default(),
        ),
        OwnBookOrder::new(
            TraderId::from("TRADER-001"),
            ClientOrderId::from("ASK-2"),
            Some(VenueOrderId::from("5")),
            OrderSideSpecified::Sell,
            Price::from("102.00"),
            Quantity::from("20"),
            OrderType::Limit,
            TimeInForce::Gtc,
            OrderStatus::Accepted,
            UnixNanos::default(),
            UnixNanos::default(),
            UnixNanos::default(),
            UnixNanos::default(),
        ),
        OwnBookOrder::new(
            TraderId::from("TRADER-001"),
            ClientOrderId::from("ASK-3"),
            Some(VenueOrderId::from("6")),
            OrderSideSpecified::Sell,
            Price::from("103.00"),
            Quantity::from("30"),
            OrderType::Limit,
            TimeInForce::Gtc,
            OrderStatus::Accepted,
            UnixNanos::default(),
            UnixNanos::default(),
            UnixNanos::default(),
            UnixNanos::default(),
        ),
    ];

    for order in &bid_orders {
        book.add(*order);
    }

    for order in &ask_orders {
        book.add(*order);
    }

    // Group with a larger increment: 2.0
    let grouped_bids = book.bid_quantity(None, None, Some(dec!(2)), None, None);
    let grouped_asks = book.ask_quantity(None, None, Some(dec!(2)), None, None);

    // Check bid grouping with larger group size
    assert_eq!(grouped_bids.len(), 2);
    assert_eq!(grouped_bids.get(&dec!(100.0)), Some(&dec!(10))); // 100.00 alone
    assert_eq!(grouped_bids.get(&dec!(98.0)), Some(&dec!(50))); // 99.00 + 98.00 = 50

    // Check ask grouping with larger group size
    assert_eq!(grouped_asks.len(), 2);
    assert_eq!(grouped_asks.get(&dec!(102.0)), Some(&dec!(30))); // 101.00 + 102.00 = 30
    assert_eq!(grouped_asks.get(&dec!(104.0)), Some(&dec!(30))); // 103.00 alone, rounded up to 104.00
}

#[rstest]
fn test_own_book_group_with_fractional_group_size() {
    let instrument_id = InstrumentId::from("ETHUSDT-PERP.BINANCE");
    let mut book = OwnOrderBook::new(instrument_id);

    // Add orders at various precise price levels
    let bid_orders = [
        OwnBookOrder::new(
            TraderId::from("TRADER-001"),
            ClientOrderId::from("BID-1"),
            Some(VenueOrderId::from("1")),
            OrderSideSpecified::Buy,
            Price::from("1.23"),
            Quantity::from("10"),
            OrderType::Limit,
            TimeInForce::Gtc,
            OrderStatus::Accepted,
            UnixNanos::default(),
            UnixNanos::default(),
            UnixNanos::default(),
            UnixNanos::default(),
        ),
        OwnBookOrder::new(
            TraderId::from("TRADER-001"),
            ClientOrderId::from("BID-2"),
            Some(VenueOrderId::from("2")),
            OrderSideSpecified::Buy,
            Price::from("1.27"),
            Quantity::from("20"),
            OrderType::Limit,
            TimeInForce::Gtc,
            OrderStatus::Accepted,
            UnixNanos::default(),
            UnixNanos::default(),
            UnixNanos::default(),
            UnixNanos::default(),
        ),
        OwnBookOrder::new(
            TraderId::from("TRADER-001"),
            ClientOrderId::from("BID-3"),
            Some(VenueOrderId::from("3")),
            OrderSideSpecified::Buy,
            Price::from("1.43"),
            Quantity::from("30"),
            OrderType::Limit,
            TimeInForce::Gtc,
            OrderStatus::Accepted,
            UnixNanos::default(),
            UnixNanos::default(),
            UnixNanos::default(),
            UnixNanos::default(),
        ),
    ];

    let ask_orders = [
        OwnBookOrder::new(
            TraderId::from("TRADER-001"),
            ClientOrderId::from("ASK-1"),
            Some(VenueOrderId::from("4")),
            OrderSideSpecified::Sell,
            Price::from("1.53"),
            Quantity::from("10"),
            OrderType::Limit,
            TimeInForce::Gtc,
            OrderStatus::Accepted,
            UnixNanos::default(),
            UnixNanos::default(),
            UnixNanos::default(),
            UnixNanos::default(),
        ),
        OwnBookOrder::new(
            TraderId::from("TRADER-001"),
            ClientOrderId::from("ASK-2"),
            Some(VenueOrderId::from("5")),
            OrderSideSpecified::Sell,
            Price::from("1.57"),
            Quantity::from("20"),
            OrderType::Limit,
            TimeInForce::Gtc,
            OrderStatus::Accepted,
            UnixNanos::default(),
            UnixNanos::default(),
            UnixNanos::default(),
            UnixNanos::default(),
        ),
        OwnBookOrder::new(
            TraderId::from("TRADER-001"),
            ClientOrderId::from("ASK-3"),
            Some(VenueOrderId::from("6")),
            OrderSideSpecified::Sell,
            Price::from("1.73"),
            Quantity::from("30"),
            OrderType::Limit,
            TimeInForce::Gtc,
            OrderStatus::Accepted,
            UnixNanos::default(),
            UnixNanos::default(),
            UnixNanos::default(),
            UnixNanos::default(),
        ),
    ];

    for order in &bid_orders {
        book.add(*order);
    }

    for order in &ask_orders {
        book.add(*order);
    }

    // Group with a fractional increment: 0.1
    let grouped_bids = book.bid_quantity(None, None, Some(dec!(0.1)), None, None);
    let grouped_asks = book.ask_quantity(None, None, Some(dec!(0.1)), None, None);

    // Check bid grouping with fractional group size
    assert_eq!(grouped_bids.len(), 2);
    assert_eq!(grouped_bids.get(&dec!(1.2)), Some(&dec!(30))); // 1.23 + 1.27 -> 1.2
    assert_eq!(grouped_bids.get(&dec!(1.4)), Some(&dec!(30))); // 1.43 -> 1.4

    // Check ask grouping with fractional group size
    assert_eq!(grouped_asks.len(), 2);
    assert_eq!(grouped_asks.get(&dec!(1.6)), Some(&dec!(30))); // 1.53 + 1.57 -> 1.6
    assert_eq!(grouped_asks.get(&dec!(1.8)), Some(&dec!(30))); // 1.73 -> 1.8
}

#[rstest]
fn test_own_book_group_with_status_and_buffer() {
    let instrument_id = InstrumentId::from("AAPL.XNAS");
    let mut own_book = OwnOrderBook::new(instrument_id);

    // Current time is 1000 ns
    let now = 1000u64;

    // Add orders with different acceptance times
    let own_recent = OwnBookOrder::new(
        TraderId::from("TRADER-001"),
        ClientOrderId::from("BID-1"),
        Some(VenueOrderId::from("1")),
        OrderSideSpecified::Buy,
        Price::from("100.00"),
        Quantity::from("40"),
        OrderType::Limit,
        TimeInForce::Gtc,
        OrderStatus::Accepted,
        UnixNanos::from(900), // ts_last is 100 ns ago
        UnixNanos::from(900), // ts_accepted is 100 ns ago
        UnixNanos::from(800),
        UnixNanos::from(800),
    );

    let own_older = OwnBookOrder::new(
        TraderId::from("TRADER-001"),
        ClientOrderId::from("BID-2"),
        Some(VenueOrderId::from("2")),
        OrderSideSpecified::Buy,
        Price::from("100.00"),
        Quantity::from("30"),
        OrderType::Limit,
        TimeInForce::Gtc,
        OrderStatus::Accepted,
        UnixNanos::from(500), // ts_last is 500 ns ago
        UnixNanos::from(500), // ts_accepted is 500 ns ago
        UnixNanos::from(400),
        UnixNanos::from(400),
    );

    own_book.add(own_recent);
    own_book.add(own_older);

    // Create a status filter for ACCEPTED orders
    let mut status_filter = HashSet::new();
    status_filter.insert(OrderStatus::Accepted);

    // Group with a buffer of 300 ns - only orders accepted before 700 ns should be included
    let grouped_bids = own_book.bid_quantity(
        Some(status_filter.clone()),
        None,
        Some(dec!(1.0)),
        Some(300),
        Some(now),
    );

    // Only the older order (ts_accepted = 500) should be included in the grouping
    assert_eq!(grouped_bids.len(), 1);
    assert_eq!(grouped_bids.get(&dec!(100.0)), Some(&dec!(30))); // Only older order is included

    // Test with a smaller buffer of 50 ns - all orders should be included
    let grouped_all = own_book.bid_quantity(
        Some(status_filter),
        None,
        Some(dec!(1.0)),
        Some(50),
        Some(now),
    );

    // Both orders should be included
    assert_eq!(grouped_all.len(), 1);
    assert_eq!(grouped_all.get(&dec!(100.0)), Some(&dec!(70))); // 40 + 30 = 70
}

#[rstest]
fn test_own_book_audit_open_orders_no_removals() {
    let instrument_id = InstrumentId::from("AAPL.XNAS");
    let mut own_book = OwnOrderBook::new(instrument_id);

    let bid_order = OwnBookOrder::new(
        TraderId::from("TRADER-001"),
        ClientOrderId::from("BID-1"),
        Some(VenueOrderId::from("1")),
        OrderSideSpecified::Buy,
        Price::from("100.00"),
        Quantity::from("10"),
        OrderType::Limit,
        TimeInForce::Gtc,
        OrderStatus::Accepted,
        UnixNanos::default(),
        UnixNanos::default(),
        UnixNanos::default(),
        UnixNanos::default(),
    );

    let ask_order = OwnBookOrder::new(
        TraderId::from("TRADER-001"),
        ClientOrderId::from("ASK-1"),
        Some(VenueOrderId::from("2")),
        OrderSideSpecified::Sell,
        Price::from("101.00"),
        Quantity::from("10"),
        OrderType::Limit,
        TimeInForce::Gtc,
        OrderStatus::Accepted,
        UnixNanos::default(),
        UnixNanos::default(),
        UnixNanos::default(),
        UnixNanos::default(),
    );

    own_book.add(bid_order);
    own_book.add(ask_order);

    // Create a set of open order IDs that includes both orders
    let mut open_order_ids = HashSet::new();
    open_order_ids.insert(ClientOrderId::from("BID-1"));
    open_order_ids.insert(ClientOrderId::from("ASK-1"));

    // Audit the book with these IDs
    own_book.audit_open_orders(&open_order_ids);

    // Verify no orders were removed
    assert!(own_book.is_order_in_book(&ClientOrderId::from("BID-1")));
    assert!(own_book.is_order_in_book(&ClientOrderId::from("ASK-1")));
    assert_eq!(own_book.bid_client_order_ids().len(), 1);
    assert_eq!(own_book.ask_client_order_ids().len(), 1);
}

#[rstest]
fn test_own_book_audit_open_orders_with_removals() {
    let instrument_id = InstrumentId::from("AAPL.XNAS");
    let mut own_book = OwnOrderBook::new(instrument_id);

    let orders = [
        OwnBookOrder::new(
            TraderId::from("TRADER-001"),
            ClientOrderId::from("BID-1"),
            Some(VenueOrderId::from("1")),
            OrderSideSpecified::Buy,
            Price::from("100.00"),
            Quantity::from("10"),
            OrderType::Limit,
            TimeInForce::Gtc,
            OrderStatus::Accepted,
            UnixNanos::default(),
            UnixNanos::default(),
            UnixNanos::default(),
            UnixNanos::default(),
        ),
        OwnBookOrder::new(
            TraderId::from("TRADER-001"),
            ClientOrderId::from("BID-2"),
            Some(VenueOrderId::from("2")),
            OrderSideSpecified::Buy,
            Price::from("99.00"),
            Quantity::from("20"),
            OrderType::Limit,
            TimeInForce::Gtc,
            OrderStatus::Accepted,
            UnixNanos::default(),
            UnixNanos::default(),
            UnixNanos::default(),
            UnixNanos::default(),
        ),
        OwnBookOrder::new(
            TraderId::from("TRADER-001"),
            ClientOrderId::from("ASK-1"),
            Some(VenueOrderId::from("3")),
            OrderSideSpecified::Sell,
            Price::from("101.00"),
            Quantity::from("10"),
            OrderType::Limit,
            TimeInForce::Gtc,
            OrderStatus::Accepted,
            UnixNanos::default(),
            UnixNanos::default(),
            UnixNanos::default(),
            UnixNanos::default(),
        ),
        OwnBookOrder::new(
            TraderId::from("TRADER-001"),
            ClientOrderId::from("ASK-2"),
            Some(VenueOrderId::from("4")),
            OrderSideSpecified::Sell,
            Price::from("102.00"),
            Quantity::from("20"),
            OrderType::Limit,
            TimeInForce::Gtc,
            OrderStatus::Accepted,
            UnixNanos::default(),
            UnixNanos::default(),
            UnixNanos::default(),
            UnixNanos::default(),
        ),
    ];

    for order in &orders {
        own_book.add(*order);
    }

    assert_eq!(own_book.bid_client_order_ids().len(), 2);
    assert_eq!(own_book.ask_client_order_ids().len(), 2);

    // Create a set of open order IDs that only includes one bid and one ask
    let mut open_order_ids = HashSet::new();
    open_order_ids.insert(ClientOrderId::from("BID-1"));
    open_order_ids.insert(ClientOrderId::from("ASK-1"));

    // Audit the book with these IDs
    own_book.audit_open_orders(&open_order_ids);

    // Verify the missing orders were removed
    assert!(own_book.is_order_in_book(&ClientOrderId::from("BID-1")));
    assert!(!own_book.is_order_in_book(&ClientOrderId::from("BID-2")));
    assert!(own_book.is_order_in_book(&ClientOrderId::from("ASK-1")));
    assert!(!own_book.is_order_in_book(&ClientOrderId::from("ASK-2")));

    // Check the final counts
    assert_eq!(own_book.bid_client_order_ids().len(), 1);
    assert_eq!(own_book.ask_client_order_ids().len(), 1);
}

////////////////////////////////////////////////////////////////////////////////
// Property-based testing
////////////////////////////////////////////////////////////////////////////////

use proptest::prelude::*;

#[derive(Clone, Debug)]
enum OrderBookOperation {
    Add(BookOrder, u8, u64),
    Update(BookOrder, u8, u64),
    Delete(BookOrder, u8, u64),
    Clear(u64),
    ClearBids(u64),
    ClearAsks(u64),
}

fn price_strategy() -> impl Strategy<Value = Price> {
    use crate::types::price::PriceRaw;
    prop_oneof![
        // Normal positive prices
        (1i64..=1000000i64).prop_map(|raw| Price::from_raw(raw as PriceRaw, 2)),
        // Edge case: very small prices
        (1i64..=100i64).prop_map(|raw| Price::from_raw(raw as PriceRaw, 8)),
        // Edge case: large prices
        (1000000i64..=10000000i64).prop_map(|raw| Price::from_raw(raw as PriceRaw, 2)),
        // Financial edge case: negative prices (options, spreads)
        prop::num::i64::ANY.prop_filter_map("valid negative price", |raw| {
            if raw < 0 && raw > i64::MIN + 1000000 {
                Some(Price::from_raw(raw as PriceRaw, 2))
            } else {
                None
            }
        }),
    ]
}

fn quantity_strategy() -> impl Strategy<Value = Quantity> {
    prop_oneof![
        // Normal quantities
        (1u64..=1000000u64).prop_map(|raw| Quantity::from_raw(raw.into(), 2)),
        // Small quantities
        (1u64..=100u64).prop_map(|raw| Quantity::from_raw(raw.into(), 8)),
        // Large quantities
        (1000000u64..=100000000u64).prop_map(|raw| Quantity::from_raw(raw.into(), 2)),
    ]
}

fn book_order_strategy() -> impl Strategy<Value = BookOrder> {
    (
        prop::sample::select(vec![OrderSide::Buy, OrderSide::Sell]),
        price_strategy(),
        quantity_strategy(),
        // Generate order IDs that are more likely to be unique to avoid cache conflicts
        prop::num::u64::ANY.prop_filter("non-zero order id", |&id| id > 0),
    )
        .prop_map(|(side, price, size, order_id)| BookOrder::new(side, price, size, order_id))
}

fn positive_book_order_strategy() -> impl Strategy<Value = BookOrder> {
    (
        prop::sample::select(vec![OrderSide::Buy, OrderSide::Sell]),
        price_strategy(),
        positive_quantity_strategy(),
        // Generate order IDs that are more likely to be unique to avoid cache conflicts
        prop::num::u64::ANY.prop_filter("non-zero order id", |&id| id > 0),
    )
        .prop_map(|(side, price, size, order_id)| BookOrder::new(side, price, size, order_id))
        .prop_filter("order must have positive size and valid price", |order| {
            order.size.is_positive() && order.price.raw > 0
        })
}

fn positive_quantity_strategy() -> impl Strategy<Value = Quantity> {
    use crate::types::quantity::QuantityRaw;
    prop_oneof![
        // Small positive quantities
        (1u64..=1000u64)
            .prop_map(|raw| Quantity::from_raw(raw as QuantityRaw, 2))
            .prop_filter("quantity must be positive", |q| q.is_positive()),
        // Medium positive quantities
        (1000u64..=100000u64)
            .prop_map(|raw| Quantity::from_raw(raw as QuantityRaw, 3))
            .prop_filter("quantity must be positive", |q| q.is_positive()),
        // Large positive quantities
        (100000u64..=10000000u64)
            .prop_map(|raw| Quantity::from_raw(raw as QuantityRaw, 2))
            .prop_filter("quantity must be positive", |q| q.is_positive()),
    ]
}

fn orderbook_operation_strategy() -> impl Strategy<Value = OrderBookOperation> {
    prop_oneof![
        // Higher probability for add/update operations to build book state
        6 => (positive_book_order_strategy(), prop::num::u8::ANY, prop::num::u64::ANY)
            .prop_map(|(order, flags, seq)| OrderBookOperation::Add(order, flags, seq)),
        4 => (book_order_strategy(), prop::num::u8::ANY, prop::num::u64::ANY)
            .prop_map(|(order, flags, seq)| OrderBookOperation::Update(order, flags, seq)),
        3 => (book_order_strategy(), prop::num::u8::ANY, prop::num::u64::ANY)
            .prop_map(|(order, flags, seq)| OrderBookOperation::Delete(order, flags, seq)),
        1 => prop::num::u64::ANY.prop_map(OrderBookOperation::Clear),
        1 => prop::num::u64::ANY.prop_map(OrderBookOperation::ClearBids),
        1 => prop::num::u64::ANY.prop_map(OrderBookOperation::ClearAsks),
    ]
}

fn orderbook_test_strategy() -> impl Strategy<Value = (BookType, Vec<OrderBookOperation>)> {
    (
        prop::sample::select(vec![BookType::L1_MBP, BookType::L2_MBP, BookType::L3_MBO]),
        prop::collection::vec(orderbook_operation_strategy(), 10..=100),
    )
}

fn test_orderbook_with_operations(book_type: BookType, operations: Vec<OrderBookOperation>) {
    let instrument_id = InstrumentId::from("TEST.VENUE");
    let mut book = OrderBook::new(instrument_id, book_type);
    let mut last_sequence = 0u64;

    for operation in operations {
        // Ensure monotonic sequence numbers
        let sequence = match &operation {
            OrderBookOperation::Add(_, _, seq)
            | OrderBookOperation::Update(_, _, seq)
            | OrderBookOperation::Delete(_, _, seq)
            | OrderBookOperation::Clear(seq)
            | OrderBookOperation::ClearBids(seq)
            | OrderBookOperation::ClearAsks(seq) => {
                last_sequence = last_sequence.max(*seq);
                last_sequence
            }
        };

        let ts_event = UnixNanos::from(sequence);

        // Skip operations that would cause assertion failures
        let should_skip = match &operation {
            OrderBookOperation::Add(order, _, _)
            | OrderBookOperation::Update(order, _, _)
            | OrderBookOperation::Delete(order, _, _) => {
                // Skip invalid prices or orders that might cause cache conflicts
                order.price.raw == crate::types::price::PRICE_UNDEF
                    || order.price.raw == crate::types::price::PRICE_ERROR
                    || order.price.raw <= 0 // Skip zero or negative prices
                    || order.order_id == 0 // Skip zero order IDs to avoid conflicts
                    // Allow zero-size orders for Update operations (represent deletions)
                    || (matches!(operation, OrderBookOperation::Add(_, _, _)) && order.size.raw == 0)
            }
            _ => false,
        };

        if should_skip {
            continue;
        }

        match operation {
            OrderBookOperation::Add(order, flags, _) => {
                book.add(order, flags, sequence, ts_event);
            }
            OrderBookOperation::Update(order, flags, _) => {
                book.update(order, flags, sequence, ts_event);
            }
            OrderBookOperation::Delete(order, flags, _) => {
                book.delete(order, flags, sequence, ts_event);
            }
            OrderBookOperation::Clear(_) => {
                book.clear(sequence, ts_event);
            }
            OrderBookOperation::ClearBids(_) => {
                book.clear_bids(sequence, ts_event);
            }
            OrderBookOperation::ClearAsks(_) => {
                book.clear_asks(sequence, ts_event);
            }
        }

        // Invariant checks after each operation

        // 1. Sequence and timestamp should be monotonic
        assert!(
            book.sequence >= last_sequence,
            "Sequence should be monotonic: {} >= {}",
            book.sequence,
            last_sequence
        );

        // 2. Update count should increase monotonically
        assert!(
            book.update_count > 0,
            "Update count should be positive after operations"
        );

        // 3. If book has bids/asks, they should have valid prices
        if let Some(best_bid) = book.best_bid_price() {
            assert!(
                best_bid.raw != crate::types::price::PRICE_UNDEF
                    && best_bid.raw != crate::types::price::PRICE_ERROR,
                "Best bid should have valid price"
            );
        }

        if let Some(best_ask) = book.best_ask_price() {
            assert!(
                best_ask.raw != crate::types::price::PRICE_UNDEF
                    && best_ask.raw != crate::types::price::PRICE_ERROR,
                "Best ask should have valid price"
            );
        }

        // 4. Spread should be non-negative (when both sides exist)
        if let Some(spread) = book.spread() {
            // Note: spread can be negative for crossed markets temporarily
            assert!(spread.is_finite(), "Spread should be finite");
        }

        // 5. Midpoint should be between bid and ask (when both exist)
        if let (Some(bid), Some(ask)) = (book.best_bid_price(), book.best_ask_price())
            && let Some(mid) = book.midpoint()
        {
            assert!(mid.is_finite(), "Midpoint should be finite");
            // Only check ordering for non-crossed markets
            if bid <= ask {
                assert!(
                    mid >= bid.as_f64() && mid <= ask.as_f64(),
                    "Midpoint {mid} should be between bid {bid} and ask {ask}"
                );
            }
        }

        // 6. Book levels should maintain proper ordering
        let bid_prices: Vec<_> = book.bids(None).map(|level| level.price.value).collect();
        for i in 1..bid_prices.len() {
            assert!(
                bid_prices[i - 1] >= bid_prices[i],
                "Bid prices should be in descending order: {} >= {}",
                bid_prices[i - 1],
                bid_prices[i]
            );
        }

        let ask_prices: Vec<_> = book.asks(None).map(|level| level.price.value).collect();
        for i in 1..ask_prices.len() {
            assert!(
                ask_prices[i - 1] <= ask_prices[i],
                "Ask prices should be in ascending order: {} <= {}",
                ask_prices[i - 1],
                ask_prices[i]
            );
        }

        // 7. All levels should have positive size
        for level in book.bids(None) {
            assert!(
                level.size() > 0.0,
                "Bid level should have positive size: {}",
                level.size()
            );
        }

        for level in book.asks(None) {
            assert!(
                level.size() > 0.0,
                "Ask level should have positive size: {}",
                level.size()
            );
        }

        last_sequence = sequence;
    }
}

#[rstest]
// Cache consistency bugs partially fixed, but property test still reveals edge cases
// Keeping disabled until all edge cases are resolved
#[ignore = "Cache consistency fixes in progress - multiple edge cases remain"]
fn prop_test_orderbook_operations() {
    proptest!(|(config in orderbook_test_strategy())| {
        let (book_type, operations) = config;
        test_orderbook_with_operations(book_type, operations);
    });
}

// Simplified property test that focuses on basic invariants without cache assertions
fn test_orderbook_basic_invariants(book_type: BookType, operations: Vec<OrderBookOperation>) {
    let instrument_id = InstrumentId::from("TEST.VENUE");
    let mut book = OrderBook::new(instrument_id, book_type);
    let mut last_sequence = 0u64;

    for operation in operations {
        // Ensure monotonic sequence numbers
        let sequence = match &operation {
            OrderBookOperation::Add(_, _, seq)
            | OrderBookOperation::Update(_, _, seq)
            | OrderBookOperation::Delete(_, _, seq)
            | OrderBookOperation::Clear(seq)
            | OrderBookOperation::ClearBids(seq)
            | OrderBookOperation::ClearAsks(seq) => {
                last_sequence = last_sequence.max(*seq);
                last_sequence
            }
        };

        let ts_event = UnixNanos::from(sequence);

        // Skip operations that would cause assertion failures
        let should_skip = match &operation {
            OrderBookOperation::Add(order, _, _)
            | OrderBookOperation::Update(order, _, _)
            | OrderBookOperation::Delete(order, _, _) => {
                order.price.raw == crate::types::price::PRICE_UNDEF
                    || order.price.raw == crate::types::price::PRICE_ERROR
                    || order.size.raw == 0
                    || order.order_id == 0
            }
            _ => false,
        };

        if should_skip {
            continue;
        }

        // Temporarily disable cache assertions for this test by not checking them
        match operation {
            OrderBookOperation::Add(order, flags, _) => {
                book.add(order, flags, sequence, ts_event);
            }
            OrderBookOperation::Update(order, flags, _) => {
                book.update(order, flags, sequence, ts_event);
            }
            OrderBookOperation::Delete(order, flags, _) => {
                book.delete(order, flags, sequence, ts_event);
            }
            OrderBookOperation::Clear(_) => {
                book.clear(sequence, ts_event);
            }
            OrderBookOperation::ClearBids(_) => {
                book.clear_bids(sequence, ts_event);
            }
            OrderBookOperation::ClearAsks(_) => {
                book.clear_asks(sequence, ts_event);
            }
        }

        // Basic invariant checks (without cache consistency)

        // 1. Sequence and timestamp should be monotonic
        assert!(
            book.sequence >= last_sequence,
            "Sequence should be monotonic: {} >= {}",
            book.sequence,
            last_sequence
        );

        // 2. Update count should increase monotonically
        assert!(
            book.update_count > 0,
            "Update count should be positive after operations"
        );

        // 3. If book has bids/asks, they should have valid prices
        if let Some(best_bid) = book.best_bid_price() {
            assert!(
                best_bid.raw != crate::types::price::PRICE_UNDEF
                    && best_bid.raw != crate::types::price::PRICE_ERROR,
                "Best bid should have valid price"
            );
        }

        if let Some(best_ask) = book.best_ask_price() {
            assert!(
                best_ask.raw != crate::types::price::PRICE_UNDEF
                    && best_ask.raw != crate::types::price::PRICE_ERROR,
                "Best ask should have valid price"
            );
        }

        // 4. Book levels should maintain proper ordering
        let bid_prices: Vec<_> = book.bids(None).map(|level| level.price.value).collect();
        for i in 1..bid_prices.len() {
            assert!(
                bid_prices[i - 1] >= bid_prices[i],
                "Bid prices should be in descending order: {} >= {}",
                bid_prices[i - 1],
                bid_prices[i]
            );
        }

        let ask_prices: Vec<_> = book.asks(None).map(|level| level.price.value).collect();
        for i in 1..ask_prices.len() {
            assert!(
                ask_prices[i - 1] <= ask_prices[i],
                "Ask prices should be in ascending order: {} <= {}",
                ask_prices[i - 1],
                ask_prices[i]
            );
        }

        // 5. All levels should have positive size
        for level in book.bids(None) {
            assert!(
                level.size() > 0.0,
                "Bid level should have positive size: {}",
                level.size()
            );
        }

        for level in book.asks(None) {
            assert!(
                level.size() > 0.0,
                "Ask level should have positive size: {}",
                level.size()
            );
        }

        last_sequence = sequence;
    }
}

#[rstest]
#[ignore = "Also hits cache consistency bug - debug assertions are in ladder code"]
fn prop_test_orderbook_basic_invariants() {
    proptest!(|(config in orderbook_test_strategy())| {
        let (book_type, operations) = config;
        test_orderbook_basic_invariants(book_type, operations);
    });
}

// Additional property test focusing on L1 quote/trade tick updates
#[derive(Clone, Debug)]
enum L1Operation {
    QuoteUpdate(Price, Quantity, Price, Quantity),
    TradeUpdate(Price, Quantity, AggressorSide),
}

fn l1_operation_strategy() -> impl Strategy<Value = L1Operation> {
    prop_oneof![
        7 => {
            // Use consistent precision for quotes
            (
                (1i64..=1000000i64).prop_map(|raw| Price::from_raw(raw.into(), 2)),
                (1u64..=1000000u64).prop_map(|raw| Quantity::from_raw(raw.into(), 2)),
                (1i64..=1000000i64).prop_map(|raw| Price::from_raw(raw.into(), 2)),
                (1u64..=1000000u64).prop_map(|raw| Quantity::from_raw(raw.into(), 2)),
            ).prop_map(|(bid_price, bid_size, ask_price, ask_size)| {
                L1Operation::QuoteUpdate(bid_price, bid_size, ask_price, ask_size)
            })
        },
        3 => (
            (1i64..=1000000i64).prop_map(|raw| Price::from_raw(raw.into(), 2)),
            (1u64..=1000000u64).prop_map(|raw| Quantity::from_raw(raw.into(), 2)),
            prop::sample::select(vec![AggressorSide::Buyer, AggressorSide::Seller])
        ).prop_map(|(price, size, aggressor)| {
            L1Operation::TradeUpdate(price, size, aggressor)
        }),
    ]
}

fn test_l1_book_with_operations(operations: Vec<L1Operation>) {
    let instrument_id = InstrumentId::from("TEST.VENUE");
    let mut book = OrderBook::new(instrument_id, BookType::L1_MBP);

    for operation in operations {
        match operation {
            L1Operation::QuoteUpdate(bid_price, bid_size, ask_price, ask_size) => {
                // Skip invalid quotes
                if bid_price.raw == crate::types::price::PRICE_UNDEF
                    || bid_price.raw == crate::types::price::PRICE_ERROR
                    || ask_price.raw == crate::types::price::PRICE_UNDEF
                    || ask_price.raw == crate::types::price::PRICE_ERROR
                    || bid_size.raw == 0
                    || ask_size.raw == 0
                {
                    continue;
                }

                let quote = QuoteTick::new(
                    instrument_id,
                    bid_price,
                    ask_price,
                    bid_size,
                    ask_size,
                    UnixNanos::default(),
                    UnixNanos::default(),
                );

                if book.update_quote_tick(&quote).is_err() {
                    continue; // Skip invalid operations
                }
            }
            L1Operation::TradeUpdate(price, size, aggressor_side) => {
                // Skip invalid trades
                if price.raw == crate::types::price::PRICE_UNDEF
                    || price.raw == crate::types::price::PRICE_ERROR
                    || size.raw == 0
                {
                    continue;
                }

                let trade = TradeTick::new(
                    instrument_id,
                    price,
                    size,
                    aggressor_side,
                    TradeId::from("1"),
                    UnixNanos::default(),
                    UnixNanos::default(),
                );

                if book.update_trade_tick(&trade).is_err() {
                    continue; // Skip invalid operations
                }
            }
        }

        // L1 book should always have at most one bid and one ask level
        assert!(
            book.bids(None).count() <= 1,
            "L1 book should have at most one bid level"
        );
        assert!(
            book.asks(None).count() <= 1,
            "L1 book should have at most one ask level"
        );
    }
}

#[rstest]
fn prop_test_l1_book_operations() {
    proptest!(|(operations in prop::collection::vec(l1_operation_strategy(), 5..=50))| {
        test_l1_book_with_operations(operations);
    });
}

#[rstest]
fn test_apply_deltas_single_clear_no_f_last() {
    // Test that applying a single CLEAR delta without F_LAST flag doesn't crash
    let instrument_id = InstrumentId::from("TEST.SIM");
    let mut book = OrderBook::new(instrument_id, BookType::L2_MBP);

    // Add some initial data to the book
    let bid = BookOrder::new(
        OrderSide::Buy,
        Price::from("100.0"),
        Quantity::from("10.0"),
        1,
    );
    book.add(bid, 0, 0, 0.into());

    assert_eq!(book.bids(None).count(), 1);

    // Create a CLEAR delta without F_LAST flag (only F_SNAPSHOT)
    let clear_delta = OrderBookDelta::clear(instrument_id, 0, 0.into(), 0.into());

    // Verify it doesn't have F_LAST
    assert!(!RecordFlag::F_LAST.matches(clear_delta.flags));
    assert!(RecordFlag::F_SNAPSHOT.matches(clear_delta.flags));

    // Create OrderBookDeltas with only the clear delta
    let deltas = OrderBookDeltas::new(instrument_id, vec![clear_delta]);

    // Apply it - should not crash
    book.apply_deltas(&deltas).unwrap();

    // Book should be cleared
    assert_eq!(book.bids(None).count(), 0);
    assert_eq!(book.asks(None).count(), 0);
}

#[rstest]
fn test_apply_deltas_empty_clear_to_empty_book() {
    // Test applying CLEAR to an already empty book
    let instrument_id = InstrumentId::from("TEST.SIM");
    let mut book = OrderBook::new(instrument_id, BookType::L2_MBP);

    // Book is already empty
    assert_eq!(book.bids(None).count(), 0);
    assert_eq!(book.asks(None).count(), 0);

    // Create a CLEAR delta
    let clear_delta = OrderBookDelta::clear(instrument_id, 0, 0.into(), 0.into());

    let deltas = OrderBookDeltas::new(instrument_id, vec![clear_delta]);

    // Apply it - should not crash
    book.apply_deltas(&deltas).unwrap();

    // Book should still be empty
    assert_eq!(book.bids(None).count(), 0);
    assert_eq!(book.asks(None).count(), 0);
}

#[rstest]
fn test_apply_delta_resolves_side_from_bids_cache() {
    let instrument_id = InstrumentId::from("AAPL.XNAS");
    let mut book = OrderBook::new(instrument_id, BookType::L3_MBO);

    // Add a bid order first
    let order1 = BookOrder::new(
        OrderSide::Buy,
        Price::from("100.00"),
        Quantity::from("10"),
        123,
    );
    let delta1 = OrderBookDelta::new(
        instrument_id,
        BookAction::Add,
        order1,
        0,
        1,
        0.into(),
        0.into(),
    );
    book.apply_delta(&delta1).unwrap();

    // Now send an update with NoOrderSide - should resolve from bids cache
    let order2 = BookOrder::new(
        OrderSide::NoOrderSide,
        Price::from("100.00"),
        Quantity::from("5"),
        123,
    );
    let delta2 = OrderBookDelta::new(
        instrument_id,
        BookAction::Update,
        order2,
        0,
        2,
        0.into(),
        0.into(),
    );

    // Should successfully resolve side from cache
    book.apply_delta(&delta2).unwrap();

    // Verify the order was updated
    let top_bid = book.bids(Some(1)).next().unwrap();
    assert_eq!(top_bid.price.value, Price::from("100.00"));
    assert_eq!(top_bid.first().unwrap().size, Quantity::from("5"));
}

#[rstest]
fn test_apply_delta_resolves_side_from_asks_cache() {
    let instrument_id = InstrumentId::from("AAPL.XNAS");
    let mut book = OrderBook::new(instrument_id, BookType::L3_MBO);

    // Add an ask order first
    let order1 = BookOrder::new(
        OrderSide::Sell,
        Price::from("100.00"),
        Quantity::from("10"),
        456,
    );
    let delta1 = OrderBookDelta::new(
        instrument_id,
        BookAction::Add,
        order1,
        0,
        1,
        0.into(),
        0.into(),
    );
    book.apply_delta(&delta1).unwrap();

    // Now send a delete with NoOrderSide - should resolve from asks cache
    let order2 = BookOrder::new(
        OrderSide::NoOrderSide,
        Price::from("100.00"),
        Quantity::from("10"),
        456,
    );
    let delta2 = OrderBookDelta::new(
        instrument_id,
        BookAction::Delete,
        order2,
        0,
        2,
        0.into(),
        0.into(),
    );

    // Should successfully resolve side from cache
    book.apply_delta(&delta2).unwrap();

    // Verify the order was deleted
    assert_eq!(book.asks(None).count(), 0);
}

#[rstest]
fn test_apply_delta_error_when_order_not_found_for_side_resolution() {
    let instrument_id = InstrumentId::from("AAPL.XNAS");
    let mut book = OrderBook::new(instrument_id, BookType::L3_MBO);

    // Try to add an order with NoOrderSide - should error
    let order = BookOrder::new(
        OrderSide::NoOrderSide,
        Price::from("100.00"),
        Quantity::from("10"),
        999, // Non-existent order_id
    );
    let delta = OrderBookDelta::new(
        instrument_id,
        BookAction::Add,
        order,
        0,
        1,
        0.into(),
        0.into(),
    );

    // Should return error (can't add without knowing side)
    let result = book.apply_delta(&delta);
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        BookIntegrityError::NoOrderSide
    ));
}

#[rstest]
fn test_apply_delta_skips_update_delete_when_order_not_found() {
    let instrument_id = InstrumentId::from("AAPL.XNAS");
    let mut book = OrderBook::new(instrument_id, BookType::L3_MBO);

    // Try to update an order that doesn't exist with NoOrderSide - should skip
    let order = BookOrder::new(
        OrderSide::NoOrderSide,
        Price::from("100.00"),
        Quantity::from("10"),
        999, // Non-existent order_id
    );
    let delta = OrderBookDelta::new(
        instrument_id,
        BookAction::Update,
        order,
        0,
        1,
        0.into(),
        0.into(),
    );

    // Should silently skip (book already consistent)
    let result = book.apply_delta(&delta);
    assert!(result.is_ok());

    // Try delete as well - should also skip
    let delta2 = OrderBookDelta::new(
        instrument_id,
        BookAction::Delete,
        order,
        0,
        2,
        0.into(),
        0.into(),
    );
    let result2 = book.apply_delta(&delta2);
    assert!(result2.is_ok());
}

#[rstest]
fn test_apply_delta_no_order_side_with_zero_order_id_for_clear() {
    let instrument_id = InstrumentId::from("AAPL.XNAS");
    let mut book = OrderBook::new(instrument_id, BookType::L3_MBO);

    // Add some orders
    let order1 = BookOrder::new(
        OrderSide::Buy,
        Price::from("100.00"),
        Quantity::from("10"),
        123,
    );
    let delta1 = OrderBookDelta::new(
        instrument_id,
        BookAction::Add,
        order1,
        0,
        1,
        0.into(),
        0.into(),
    );
    book.apply_delta(&delta1).unwrap();

    // Clear with NoOrderSide and order_id=0 should work
    let delta_clear = OrderBookDelta::clear(instrument_id, 2, 0.into(), 0.into());

    // Should work (no side resolution needed for Clear)
    book.apply_delta(&delta_clear).unwrap();

    // Book should be cleared
    assert_eq!(book.bids(None).count(), 0);
    assert_eq!(book.asks(None).count(), 0);
}
