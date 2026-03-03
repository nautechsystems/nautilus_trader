import pytest

from nautilus_trader.core import nautilus_pyo3

from .test_orderbook_pyo3 import populate_book


def test_binary_market_book_view_creation() -> None:
    book_type = nautilus_pyo3.BookType.L2_MBP
    instrument_id = nautilus_pyo3.InstrumentId.from_str("YES.XNAS")
    book = nautilus_pyo3.OrderBook(instrument_id, book_type)
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
    combined_own = own_book.combined_with_opposite(own_synthetic_book)
    filtered = book.filtered_view(combined_own)

    assert filtered.best_bid_size() == 80

    # synthetic bid covers 0.60 whole level
    assert filtered.best_ask_size() == 200
    assert filtered.best_ask_price() == 0.61


def test_order_book_filtered_view_book_and_own_book_instrument_mismatch() -> None:
    book_type = nautilus_pyo3.BookType.L2_MBP
    instrument_yes_id = nautilus_pyo3.InstrumentId.from_str("YES.XNAS")
    instrument_no_id = nautilus_pyo3.InstrumentId.from_str("NO.XNAS")

    book = nautilus_pyo3.OrderBook(instrument_yes_id, book_type)
    own_book = nautilus_pyo3.OwnOrderBook(instrument_no_id)

    with pytest.raises(
        ValueError,
        match=r"Instrument ID mismatch: book=YES.XNAS, own_book=NO.XNAS",
    ):
        book.filtered_view(own_book)


def test_own_order_book_combined_with_opposite_instrument_must_differ() -> None:
    instrument_yes_id = nautilus_pyo3.InstrumentId.from_str("YES.XNAS")

    own_book = nautilus_pyo3.OwnOrderBook(instrument_yes_id)
    own_synthetic_book = nautilus_pyo3.OwnOrderBook(instrument_yes_id)

    with pytest.raises(
        ValueError,
        match=r"Opposite own book must have different instrument ID: book=YES.XNAS, opposite=YES.XNAS",
    ):
        own_book.combined_with_opposite(own_synthetic_book)


def test_order_book_filtered_view_optional_books() -> None:
    book_type = nautilus_pyo3.BookType.L2_MBP
    instrument_id = nautilus_pyo3.InstrumentId.from_str("YES.XNAS")
    book = nautilus_pyo3.OrderBook(instrument_id, book_type)
    populate_book(
        book,
        bids=[
            (0.40, 100),
        ],
        asks=[
            (0.60, 200),
        ],
    )

    filtered = book.filtered_view()

    assert filtered.best_bid_size() == 100
    assert filtered.best_ask_size() == 200
