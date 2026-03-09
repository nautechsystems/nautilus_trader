use iai::black_box;
use nautilus_model::{
    data::{BookOrder, OrderBookDelta, OrderBookDeltas},
    enums::{BookAction, BookType, OrderSide},
    identifiers::InstrumentId,
    orderbook::OrderBook,
    types::{Price, Quantity},
};

fn bench_orderbook_add() {
    let instrument_id = InstrumentId::from("AAPL.XNAS");
    let mut book = OrderBook::new(instrument_id, BookType::L3_MBO);
    let order = BookOrder::new(OrderSide::Buy, Price::from("100.0"), Quantity::from(100), 1);

    book.add(order, 0, 1, 1.into());
    black_box(());
}

fn bench_orderbook_update() {
    let instrument_id = InstrumentId::from("AAPL.XNAS");
    let mut book = OrderBook::new(instrument_id, BookType::L3_MBO);
    let order = BookOrder::new(OrderSide::Buy, Price::from("100.0"), Quantity::from(100), 1);
    book.add(order, 0, 1, 1.into());

    let updated_order = BookOrder::new(
        OrderSide::Buy,
        Price::from("101.0"),
        Quantity::from("2.0"),
        1,
    );

    book.update(updated_order, 0, 2, 2.into());
    black_box(());
}

fn bench_orderbook_delete() {
    let instrument_id = InstrumentId::from("AAPL.XNAS");
    let mut book = OrderBook::new(instrument_id, BookType::L3_MBO);
    let order = BookOrder::new(OrderSide::Buy, Price::from("100.0"), Quantity::from(100), 1);
    book.add(order, 0, 1, 1.into());

    book.delete(order, 0, 2, 2.into());
    black_box(());
}

fn bench_orderbook_apply_deltas() {
    let instrument_id = InstrumentId::from("AAPL.XNAS");
    let mut book = OrderBook::new(instrument_id, BookType::L3_MBO);
    let deltas = OrderBookDeltas {
        instrument_id,
        deltas: vec![OrderBookDelta {
            instrument_id,
            action: BookAction::Add,
            order: BookOrder::new(OrderSide::Buy, Price::from("100.0"), Quantity::from(100), 1),
            flags: 0,
            sequence: 1,
            ts_event: 1.into(),
            ts_init: 2.into(),
        }],
        flags: 0,
        sequence: 1,
        ts_event: 1.into(),
        ts_init: 2.into(),
    };

    book.apply_deltas(&deltas).unwrap();
    black_box(());
}

iai::main!(
    bench_orderbook_add,
    bench_orderbook_update,
    bench_orderbook_delete,
    bench_orderbook_apply_deltas,
);
