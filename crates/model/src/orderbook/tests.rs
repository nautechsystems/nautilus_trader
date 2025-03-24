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
    data::{QuoteTick, TradeTick, depth::OrderBookDepth10, order::BookOrder, stubs::*},
    enums::{
        AggressorSide, BookType, OrderSide, OrderSideSpecified, OrderStatus, OrderType, TimeInForce,
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
#[case::too_many_levels_l1(
    BookType::L1_MBP,
    vec![
        (OrderSide::Buy, "99.00", 100, 1001),
        (OrderSide::Buy, "98.00", 100, 1002),
    ],
    Err(BookIntegrityError::TooManyLevels(OrderSide::Buy, 2))
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
    book.add(bid2, 0, 0, 1.into());
    book.add(bid3, 0, 0, 1.into());
    book.add(ask1, 0, 0, 1.into());
    book.add(ask2, 0, 0, 1.into());
    book.add(ask3, 0, 0, 1.into());

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

    assert_eq!(book.best_bid_price().unwrap().as_f64(), 99.00);
    assert_eq!(book.best_ask_price().unwrap().as_f64(), 100.00);
    assert_eq!(book.best_bid_size().unwrap().as_f64(), 100.0);
    assert_eq!(book.best_ask_size().unwrap().as_f64(), 100.0);
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

    let pprint_output = book.pprint(3);

    let expected_output = "╭───────┬───────┬───────╮\n\
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
    let bid_orders = vec![
        BookOrder::new(
            OrderSide::Buy,
            Price::from("100.00"),
            Quantity::from(100),
            1,
        ),
        BookOrder::new(OrderSide::Buy, Price::from("99.00"), Quantity::from(200), 2),
        BookOrder::new(OrderSide::Buy, Price::from("98.00"), Quantity::from(300), 3),
    ];

    let ask_orders = vec![
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
            ((i + bid_orders.len()) as u64).into(),
            ((i + bid_orders.len()) as u64).into(),
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
        bid_id1.clone(),
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
        bid_id2.clone(),
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
        ask_id1.clone(),
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
        ask_id2.clone(),
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
        client_order_id.clone(),
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
fn test_own_book_display() {
    let instrument_id = InstrumentId::from("ETHUSDT-PERP.BINANCE");
    let book = OwnOrderBook::new(instrument_id);
    assert_eq!(
        book.to_string(),
        "OwnOrderBook(instrument_id=ETHUSDT-PERP.BINANCE, orders=0, update_count=0)"
    );
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
    let order_updated = OwnBookOrder::new(
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
    level.update(order_updated);
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

    let bid_quantities = book.bid_quantity(None, None, None);
    let ask_quantities = book.ask_quantity(None, None, None);

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

    let bid_quantities = book.bid_quantity(None, None, None);
    assert_eq!(bid_quantities.len(), 2);
    assert_eq!(bid_quantities.get(&dec!(100.00)), Some(&dec!(25)));
    assert_eq!(bid_quantities.get(&dec!(99.50)), Some(&dec!(20)));

    let ask_quantities = book.ask_quantity(None, None, None);
    assert_eq!(ask_quantities.len(), 1);
    assert_eq!(ask_quantities.get(&dec!(101.00)), Some(&dec!(20)));
}

#[rstest]
fn test_status_filtering_bids_as_map() {
    let instrument_id = InstrumentId::from("AAPL.XNAS");
    let mut book = OwnOrderBook::new(instrument_id);

    // Create orders with different statuses
    let order_submitted = OwnBookOrder::new(
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

    let order_accepted = OwnBookOrder::new(
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

    let order_canceled = OwnBookOrder::new(
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

    book.add(order_submitted);
    book.add(order_accepted);
    book.add(order_canceled);

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
    let order_submitted = OwnBookOrder::new(
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

    let order_accepted = OwnBookOrder::new(
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

    book.add(order_submitted);
    book.add(order_accepted);

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
    let order_submitted = OwnBookOrder::new(
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

    let order_accepted = OwnBookOrder::new(
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

    let order_canceled = OwnBookOrder::new(
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

    book.add(order_submitted);
    book.add(order_accepted);
    book.add(order_canceled);

    // Test with no filter (should include all orders)
    let all_quantities = book.bid_quantity(None, None, None);
    assert_eq!(all_quantities.len(), 2); // Two price levels
    assert_eq!(all_quantities.get(&dec!(100.00)), Some(&dec!(25))); // 10 + 15 = 25
    assert_eq!(all_quantities.get(&dec!(99.50)), Some(&dec!(20))); // 20

    // Filter for just SUBMITTED status
    let mut filter_submitted = HashSet::new();
    filter_submitted.insert(OrderStatus::Submitted);
    let submitted_quantities = book.bid_quantity(Some(filter_submitted), None, None);
    assert_eq!(submitted_quantities.len(), 1); // One price level
    assert_eq!(submitted_quantities.get(&dec!(100.00)), Some(&dec!(10))); // 10
    assert!(submitted_quantities.get(&dec!(99.50)).is_none()); // No SUBMITTED orders at 99.50

    // Filter for ACCEPTED and CANCELED statuses
    let mut filter_accepted_canceled = HashSet::new();
    filter_accepted_canceled.insert(OrderStatus::Accepted);
    filter_accepted_canceled.insert(OrderStatus::Canceled);
    let accepted_canceled_quantities =
        book.bid_quantity(Some(filter_accepted_canceled), None, None);
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
    let order_submitted = OwnBookOrder::new(
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

    let order_accepted = OwnBookOrder::new(
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

    let order_canceled = OwnBookOrder::new(
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

    book.add(order_submitted);
    book.add(order_accepted);
    book.add(order_canceled);

    // Test with no filter (should include all orders)
    let all_quantities = book.ask_quantity(None, None, None);
    assert_eq!(all_quantities.len(), 2); // Two price levels
    assert_eq!(all_quantities.get(&dec!(101.00)), Some(&dec!(25))); // 10 + 15 = 25
    assert_eq!(all_quantities.get(&dec!(102.00)), Some(&dec!(20))); // 20

    // Filter for just SUBMITTED status
    let mut filter_submitted = HashSet::new();
    filter_submitted.insert(OrderStatus::Submitted);
    let submitted_quantities = book.ask_quantity(Some(filter_submitted), None, None);
    assert_eq!(submitted_quantities.len(), 1); // One price level
    assert_eq!(submitted_quantities.get(&dec!(101.00)), Some(&dec!(10))); // 10
    assert!(submitted_quantities.get(&dec!(102.00)).is_none()); // No SUBMITTED orders at 102.00

    // Filter for multiple statuses
    let mut filter_multiple = HashSet::new();
    filter_multiple.insert(OrderStatus::Submitted);
    filter_multiple.insert(OrderStatus::Canceled);
    let multiple_quantities = book.ask_quantity(Some(filter_multiple), None, None);
    assert_eq!(multiple_quantities.len(), 2); // Two price levels
    assert_eq!(multiple_quantities.get(&dec!(101.00)), Some(&dec!(10))); // 10 (Submitted only)
    assert_eq!(multiple_quantities.get(&dec!(102.00)), Some(&dec!(20))); // 20 (Canceled only)

    // Check empty price levels are filtered out
    let mut filter_filled = HashSet::new();
    filter_filled.insert(OrderStatus::Filled);
    let filled_quantities = book.ask_quantity(Some(filter_filled), None, None);
    assert_eq!(filled_quantities.len(), 0); // No orders match
}

#[rstest]
fn test_own_book_group_empty_book() {
    let instrument_id = InstrumentId::from("AAPL.XNAS");
    let book = OwnOrderBook::new(instrument_id);

    let grouped_bids = book.group_bids(dec!(1), None, None, None, None);
    let grouped_asks = book.group_asks(dec!(1), None, None, None, None);

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
    let grouped_bids = book.group_bids(dec!(0.5), None, None, None, None);
    let grouped_asks = book.group_asks(dec!(0.5), None, None, None, None);

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
    let grouped_bids = book.group_bids(dec!(1.0), Some(2), None, None, None);
    let grouped_asks = book.group_asks(dec!(1.0), Some(2), None, None, None);

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
    let grouped_bids = book.group_bids(dec!(1.0), None, None, None, None);
    let grouped_asks = book.group_asks(dec!(1.0), None, None, None, None);

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
    let grouped_bids = book.group_bids(dec!(2), None, None, None, None);
    let grouped_asks = book.group_asks(dec!(2), None, None, None, None);

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
    let grouped_bids = book.group_bids(dec!(0.1), None, None, None, None);
    let grouped_asks = book.group_asks(dec!(0.1), None, None, None, None);

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
    let grouped_bids = own_book.group_bids(
        dec!(1.0),
        None,
        Some(status_filter.clone()),
        Some(300),
        Some(now),
    );

    // Only the older order (ts_accepted = 500) should be included in the grouping
    assert_eq!(grouped_bids.len(), 1);
    assert_eq!(grouped_bids.get(&dec!(100.0)), Some(&dec!(30))); // Only older order is included

    // Test with a smaller buffer of 50 ns - all orders should be included
    let grouped_all =
        own_book.group_bids(dec!(1.0), None, Some(status_filter), Some(50), Some(now));

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
