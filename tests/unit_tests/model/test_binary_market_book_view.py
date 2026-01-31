from nautilus_trader.core import nautilus_pyo3

from .test_orderbook_pyo3 import populate_book


def test_binary_market_book_view_creation(book: nautilus_pyo3.OrderBook):
    populate_book(
        book,
        bids=[
            (0.40, 100),
            (0.39, 100),
        ],
        asks=[
            (0.60, 100),
            (0.61, 200),
        ],
    )

    instrument_id = nautilus_pyo3.InstrumentId.from_str("YES.XNAS")
    own_book = nautilus_pyo3.OwnOrderBook(instrument_id)

    order = nautilus_pyo3.OwnBookOrder(
        trader_id=nautilus_pyo3.TraderId("TRADER-001"),
        client_order_id=nautilus_pyo3.ClientOrderId("O-123"),
        venue_order_id=nautilus_pyo3.VenueOrderId("1"),
        side=nautilus_pyo3.OrderSide.BUY,
        price=nautilus_pyo3.Price(0.40, 2),
        size=nautilus_pyo3.Quantity(20, 0),
        order_type=nautilus_pyo3.OrderType.LIMIT,
        time_in_force=nautilus_pyo3.TimeInForce.GTC,
        status=nautilus_pyo3.OrderStatus.ACCEPTED,
        ts_last=2,
        ts_accepted=2,
        ts_submitted=1,
        ts_init=1,
    )
    own_book.add(order)

    instrument_no_id = nautilus_pyo3.InstrumentId.from_str("NO.XNAS")
    own_synthetic_book = nautilus_pyo3.OwnOrderBook(instrument_no_id)
    synthetic_bid = nautilus_pyo3.OwnBookOrder(
        trader_id=nautilus_pyo3.TraderId("TRADER-001"),
        client_order_id=nautilus_pyo3.ClientOrderId("O-1"),
        venue_order_id=nautilus_pyo3.VenueOrderId("2"),
        side=nautilus_pyo3.OrderSide.BUY,
        price=nautilus_pyo3.Price(0.40, 2),
        size=nautilus_pyo3.Quantity(100, 0),
        order_type=nautilus_pyo3.OrderType.LIMIT,
        time_in_force=nautilus_pyo3.TimeInForce.GTC,
        status=nautilus_pyo3.OrderStatus.ACCEPTED,
        ts_last=2,
        ts_accepted=2,
        ts_submitted=1,
        ts_init=1,
    )
    own_synthetic_book.add(synthetic_bid)
    book_view = nautilus_pyo3.BinaryMarketBookView(book, own_book, own_synthetic_book)

    assert book_view.book.best_bid_size() == 80

    # synthetic bid covers 0.60 whole level
    assert book_view.book.best_ask_size() == 200
    assert book_view.book.best_ask_price() == 0.61
