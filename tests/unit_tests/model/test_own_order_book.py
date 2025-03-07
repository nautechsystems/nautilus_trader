# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
#  You may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
#
#  Unless required by applicable law or agreed to in writing, software
#  distributed under the License is distributed on an "AS IS" BASIS,
#  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
#  See the License for the specific language governing permissions and
#  limitations under the License.
# -------------------------------------------------------------------------------------------------

from decimal import Decimal

import pytest

from nautilus_trader.core.nautilus_pyo3 import ClientOrderId
from nautilus_trader.core.nautilus_pyo3 import InstrumentId
from nautilus_trader.core.nautilus_pyo3 import OrderSide
from nautilus_trader.core.nautilus_pyo3 import OrderStatus
from nautilus_trader.core.nautilus_pyo3 import OrderType
from nautilus_trader.core.nautilus_pyo3 import OwnBookOrder
from nautilus_trader.core.nautilus_pyo3 import OwnOrderBook
from nautilus_trader.core.nautilus_pyo3 import Price
from nautilus_trader.core.nautilus_pyo3 import Quantity
from nautilus_trader.core.nautilus_pyo3 import TimeInForce
from nautilus_trader.core.nautilus_pyo3 import TraderId
from nautilus_trader.core.nautilus_pyo3 import VenueOrderId


# ------------------------------------------------------------------------------
# OwnOrder Tests
# ------------------------------------------------------------------------------
def test_own_book_order_creation():
    order = OwnBookOrder(
        trader_id=TraderId("TRADER-001"),
        client_order_id=ClientOrderId("O-12345"),
        venue_order_id=VenueOrderId("1"),
        side=OrderSide.BUY,
        price=Price(100.0, 2),
        size=Quantity(10.0, 0),
        order_type=OrderType.LIMIT,
        time_in_force=TimeInForce.GTC,
        status=OrderStatus.ACCEPTED,
        ts_last=2,
        ts_accepted=2,
        ts_submitted=1,
        ts_init=1,
    )

    assert order.client_order_id == ClientOrderId("O-12345")
    assert order.side == OrderSide.BUY
    assert order.price == Price(100.0, 2)
    assert order.size == Quantity(10.0, 0)
    assert order.order_type == OrderType.LIMIT
    assert order.time_in_force == TimeInForce.GTC


def test_own_book_order_exposure():
    order = OwnBookOrder(
        trader_id=TraderId("TRADER-001"),
        client_order_id=ClientOrderId("O-12345"),
        venue_order_id=VenueOrderId("1"),
        side=OrderSide.BUY,
        price=Price(100.0, 2),
        size=Quantity(10.0, 0),
        order_type=OrderType.LIMIT,
        time_in_force=TimeInForce.GTC,
        status=OrderStatus.ACCEPTED,
        ts_last=2,
        ts_accepted=2,
        ts_submitted=1,
        ts_init=1,
    )

    assert order.exposure() == 1000.0  # 100.0 * 10.0


@pytest.mark.parametrize(
    "side,expected",
    [
        (OrderSide.BUY, 10.0),
        (OrderSide.SELL, -10.0),
    ],
)
def test_own_book_order_signed_size(side, expected):
    order = OwnBookOrder(
        trader_id=TraderId("TRADER-001"),
        client_order_id=ClientOrderId("O-12345"),
        venue_order_id=VenueOrderId("1"),
        side=side,
        price=Price(100.0, 2),
        size=Quantity(10.0, 0),
        order_type=OrderType.LIMIT,
        time_in_force=TimeInForce.GTC,
        status=OrderStatus.ACCEPTED,
        ts_last=2,
        ts_accepted=2,
        ts_submitted=1,
        ts_init=1,
    )

    assert order.signed_size() == expected


def test_own_book_order_repr():
    order = OwnBookOrder(
        trader_id=TraderId("TRADER-001"),
        client_order_id=ClientOrderId("O-12345"),
        venue_order_id=VenueOrderId("1"),
        side=OrderSide.BUY,
        price=Price(100.0, 2),
        size=Quantity(10.0, 0),
        order_type=OrderType.LIMIT,
        time_in_force=TimeInForce.GTC,
        status=OrderStatus.ACCEPTED,
        ts_last=2,
        ts_accepted=2,
        ts_submitted=1,
        ts_init=1,
    )

    expected_repr = (
        'OwnBookOrder(trader_id=TRADER-001, client_order_id=O-12345, venue_order_id=Some(u!("1")), side=BUY, price=100.00, size=10, '
        "order_type=LIMIT, time_in_force=GTC, status=ACCEPTED, ts_last=2, ts_accepted=2, ts_submitted=1, ts_init=1)"
    )
    assert repr(order) == expected_repr
    assert str(order) == 'TRADER-001,O-12345,Some(u!("1")),BUY,100.00,10,LIMIT,GTC,ACCEPTED,2,2,1,1'


# ------------------------------------------------------------------------------
# OwnOrderBook Tests
# ------------------------------------------------------------------------------
def test_own_order_book_creation():
    """
    Simple creation check.
    """
    instrument_id = InstrumentId.from_str("AAPL.XNAS")
    book = OwnOrderBook(instrument_id)

    assert book.instrument_id == instrument_id
    assert book.update_count == 0
    assert book.ts_last == 0


def test_own_order_book_add_update_delete():
    """
    Test adding, updating, and deleting a single order in OwnOrderBook.

    Verifies that event count increments as expected.

    """
    instrument_id = InstrumentId.from_str("AAPL.XNAS")
    book = OwnOrderBook(instrument_id)

    # Create initial order
    order = OwnBookOrder(
        trader_id=TraderId("TRADER-001"),
        client_order_id=ClientOrderId("O-123"),
        venue_order_id=VenueOrderId("1"),
        side=OrderSide.BUY,
        price=Price(100.0, 2),
        size=Quantity(10.0, 0),
        order_type=OrderType.LIMIT,
        time_in_force=TimeInForce.GTC,
        status=OrderStatus.ACCEPTED,
        ts_last=2,
        ts_accepted=2,
        ts_submitted=1,
        ts_init=1,
    )

    # 1) Add order
    book.add(order)
    assert book.update_count == 1  # Add increments the event count
    bids_map = book.bids_to_dict()
    assert len(bids_map) == 1
    assert Price(100.0, 2).as_decimal() in bids_map

    # 2) Update order (increase size from 10 -> 15)
    updated_order = OwnBookOrder(
        trader_id=TraderId("TRADER-001"),
        client_order_id=ClientOrderId("O-123"),
        venue_order_id=VenueOrderId("1"),
        side=OrderSide.BUY,
        price=Price(100.0, 2),
        size=Quantity(15.0, 0),
        order_type=OrderType.LIMIT,
        time_in_force=TimeInForce.GTC,
        status=OrderStatus.ACCEPTED,
        ts_last=2,
        ts_accepted=2,
        ts_submitted=1,
        ts_init=1,
    )
    book.update(updated_order)
    assert book.update_count == 2  # Update increments the event count

    # Check updated size
    bids_map = book.bids_to_dict()
    orders_at_price = bids_map[Price(100.0, 2).as_decimal()]
    assert len(orders_at_price) == 1
    assert orders_at_price[0].size == Quantity(15.0, 0)

    # 3) Delete order
    book.delete(order)
    # Depending on how your book logic is implemented,
    # count might now be 3 (since delete is an event).
    assert book.update_count == 3, "Delete should increment event count"

    # Confirm no bids left
    assert len(book.bids_to_dict()) == 0


def test_own_order_book_clear():
    """
    Clearing the book should remove all orders and increment event count.
    """
    instrument_id = InstrumentId.from_str("AAPL.XNAS")
    book = OwnOrderBook(instrument_id)

    # Add a single order (BUY)
    book.add(
        OwnBookOrder(
            trader_id=TraderId("TRADER-001"),
            client_order_id=ClientOrderId("O-123"),
            venue_order_id=VenueOrderId("1"),
            side=OrderSide.BUY,
            price=Price(100.0, 2),
            size=Quantity(10.0, 0),
            order_type=OrderType.LIMIT,
            time_in_force=TimeInForce.GTC,
            status=OrderStatus.ACCEPTED,
            ts_last=2,
            ts_accepted=2,
            ts_submitted=1,
            ts_init=1,
        ),
    )
    assert book.update_count == 1

    # Call clear() -> typically increments event count
    book.clear()
    assert book.update_count == 1
    assert len(book.bids_to_dict()) == 0
    assert len(book.asks_to_dict()) == 0


@pytest.mark.parametrize(
    "side",
    [
        OrderSide.BUY,
        OrderSide.SELL,
    ],
)
def test_own_order_book_bids_asks_as_map(side):
    """
    Tests that adding a single order appears in the correct side's map, under the
    expected price key, and that the order is the same one added.
    """
    instrument_id = InstrumentId.from_str("AAPL.XNAS")
    book = OwnOrderBook(instrument_id)

    # Create an order matching the parameterized side (BUY or SELL)
    order = OwnBookOrder(
        trader_id=TraderId("TRADER-001"),
        client_order_id=ClientOrderId("O-123"),
        venue_order_id=VenueOrderId("1"),
        side=side,
        price=Price(100.0, 2),
        size=Quantity(10.0, 0),
        order_type=OrderType.LIMIT,
        time_in_force=TimeInForce.GTC,
        status=OrderStatus.ACCEPTED,
        ts_last=2,
        ts_accepted=2,
        ts_submitted=1,
        ts_init=1,
    )
    book.add(order)

    # If it's a BUY, it should appear in bids_to_dict()
    if side == OrderSide.BUY:
        bids_map = book.bids_to_dict()
        assert len(bids_map) == 1
        orders_at_price = bids_map[Price(100.0, 2).as_decimal()]
        assert len(orders_at_price) == 1
        assert orders_at_price[0] == order

        # As a sanity check, the ask side should be empty
        assert len(book.asks_to_dict()) == 0
    else:
        # It's a SELL, so it should appear in asks_to_dict()
        asks_map = book.asks_to_dict()
        assert len(asks_map) == 1
        orders_at_price = asks_map[Price(100.0, 2).as_decimal()]
        assert len(orders_at_price) == 1
        assert orders_at_price[0] == order

        # The bid side should be empty
        assert len(book.bids_to_dict()) == 0


def test_own_order_book_fifo_same_price():
    """
    Verify FIFO insertion order: multiple orders at the same price level
    should appear in the order they were added.
    """
    instrument_id = InstrumentId.from_str("AAPL.XNAS")
    book = OwnOrderBook(instrument_id)

    # Add multiple orders at the same price
    order1 = OwnBookOrder(
        trader_id=TraderId("TRADER-001"),
        client_order_id=ClientOrderId("O-1"),
        venue_order_id=VenueOrderId("1"),
        side=OrderSide.BUY,
        price=Price(100.0, 2),
        size=Quantity(10.0, 0),
        order_type=OrderType.LIMIT,
        time_in_force=TimeInForce.GTC,
        status=OrderStatus.ACCEPTED,
        ts_last=2,
        ts_accepted=2,
        ts_submitted=1,
        ts_init=1,
    )
    order2 = OwnBookOrder(
        trader_id=TraderId("TRADER-001"),
        client_order_id=ClientOrderId("O-2"),
        venue_order_id=VenueOrderId("2"),
        side=OrderSide.BUY,
        price=Price(100.0, 2),
        size=Quantity(5.0, 0),
        order_type=OrderType.LIMIT,
        time_in_force=TimeInForce.GTC,
        status=OrderStatus.ACCEPTED,
        ts_last=2,
        ts_accepted=2,
        ts_submitted=1,
        ts_init=1,
    )
    book.add(order1)
    book.add(order2)

    # Check FIFO
    bids_map = book.bids_to_dict()
    assert len(bids_map) == 1
    price_decimal = Price(100.0, 2).as_decimal()
    orders_at_price = bids_map[price_decimal]
    assert len(orders_at_price) == 2

    assert orders_at_price[0] == order1
    assert orders_at_price[1] == order2


def test_own_order_book_price_change():
    """
    If an order's price changes (update call), it should be removed from the old price
    level and inserted at the new price level.
    """
    instrument_id = InstrumentId.from_str("AAPL.XNAS")
    book = OwnOrderBook(instrument_id)

    order = OwnBookOrder(
        trader_id=TraderId("TRADER-001"),
        client_order_id=ClientOrderId("O-777"),
        venue_order_id=VenueOrderId("1"),
        side=OrderSide.BUY,
        price=Price(100.0, 2),
        size=Quantity(10.0, 0),
        order_type=OrderType.LIMIT,
        time_in_force=TimeInForce.GTC,
        status=OrderStatus.ACCEPTED,
        ts_last=10,
        ts_accepted=10,
        ts_submitted=1,
        ts_init=0,
    )
    book.add(order)
    assert len(book.bids_to_dict()) == 1

    # Update to new price=101
    updated = OwnBookOrder(
        trader_id=TraderId("TRADER-001"),
        client_order_id=ClientOrderId("O-777"),
        venue_order_id=VenueOrderId("1"),
        side=OrderSide.BUY,
        price=Price(101.0, 2),
        size=Quantity(10.0, 0),
        order_type=OrderType.LIMIT,
        time_in_force=TimeInForce.GTC,
        status=OrderStatus.ACCEPTED,
        ts_last=11,
        ts_accepted=11,
        ts_submitted=1,
        ts_init=0,
    )
    book.update(updated)

    # The old price (100) should be removed
    old_price_decimal = Price(100.0, 2).as_decimal()
    assert old_price_decimal not in book.bids_to_dict()

    # The new price (101) should have the order
    new_price_decimal = Price(101.0, 2).as_decimal()
    bids_map = book.bids_to_dict()
    assert new_price_decimal in bids_map
    new_orders = bids_map[new_price_decimal]
    assert len(new_orders) == 1
    assert new_orders[0] == updated
    assert book.ts_last == 11


def test_own_order_book_bid_ask_quantity():
    instrument_id = InstrumentId.from_str("AAPL.XNAS")
    book = OwnOrderBook(instrument_id)

    # Add multiple orders at the same price level (bids)
    bid_order1 = OwnBookOrder(
        trader_id=TraderId("TRADER-001"),
        client_order_id=ClientOrderId("O-1"),
        venue_order_id=VenueOrderId("1"),
        side=OrderSide.BUY,
        price=Price(100.0, 2),
        size=Quantity(10.0, 0),
        order_type=OrderType.LIMIT,
        time_in_force=TimeInForce.GTC,
        status=OrderStatus.ACCEPTED,
        ts_last=2,
        ts_accepted=2,
        ts_submitted=1,
        ts_init=1,
    )
    bid_order2 = OwnBookOrder(
        trader_id=TraderId("TRADER-001"),
        client_order_id=ClientOrderId("O-2"),
        venue_order_id=VenueOrderId("2"),
        side=OrderSide.BUY,
        price=Price(100.0, 2),
        size=Quantity(15.0, 0),
        order_type=OrderType.LIMIT,
        time_in_force=TimeInForce.GTC,
        status=OrderStatus.ACCEPTED,
        ts_last=2,
        ts_accepted=2,
        ts_submitted=1,
        ts_init=1,
    )
    # Add an order at a different price level (bids)
    bid_order3 = OwnBookOrder(
        trader_id=TraderId("TRADER-001"),
        client_order_id=ClientOrderId("O-3"),
        venue_order_id=VenueOrderId("3"),
        side=OrderSide.BUY,
        price=Price(99.5, 2),
        size=Quantity(20.0, 0),
        order_type=OrderType.LIMIT,
        time_in_force=TimeInForce.GTC,
        status=OrderStatus.ACCEPTED,
        ts_last=2,
        ts_accepted=2,
        ts_submitted=1,
        ts_init=1,
    )

    # Add orders at different price levels (asks)
    ask_order1 = OwnBookOrder(
        trader_id=TraderId("TRADER-001"),
        client_order_id=ClientOrderId("O-4"),
        venue_order_id=VenueOrderId("4"),
        side=OrderSide.SELL,
        price=Price(101.0, 2),
        size=Quantity(12.0, 0),
        order_type=OrderType.LIMIT,
        time_in_force=TimeInForce.GTC,
        status=OrderStatus.ACCEPTED,
        ts_last=2,
        ts_accepted=2,
        ts_submitted=1,
        ts_init=1,
    )
    ask_order2 = OwnBookOrder(
        trader_id=TraderId("TRADER-001"),
        client_order_id=ClientOrderId("O-5"),
        venue_order_id=VenueOrderId("5"),
        side=OrderSide.SELL,
        price=Price(101.0, 2),
        size=Quantity(8.0, 0),
        order_type=OrderType.LIMIT,
        time_in_force=TimeInForce.GTC,
        status=OrderStatus.ACCEPTED,
        ts_last=2,
        ts_accepted=2,
        ts_submitted=1,
        ts_init=1,
    )

    book.add(bid_order1)
    book.add(bid_order2)
    book.add(bid_order3)
    book.add(ask_order1)
    book.add(ask_order2)

    bid_quantities = book.bid_quantity()
    assert len(bid_quantities) == 2
    assert bid_quantities[Price(100.0, 2).as_decimal()] == Decimal("25")
    assert bid_quantities[Price(99.5, 2).as_decimal()] == Decimal("20")

    ask_quantities = book.ask_quantity()
    assert len(ask_quantities) == 1
    assert ask_quantities[Price(101.0, 2).as_decimal()] == Decimal("20")


def test_own_order_book_quantity_empty_levels():
    instrument_id = InstrumentId.from_str("AAPL.XNAS")
    book = OwnOrderBook(instrument_id)

    # Test on empty book
    bid_quantities = book.bid_quantity()
    ask_quantities = book.ask_quantity()

    assert len(bid_quantities) == 0
    assert len(ask_quantities) == 0


@pytest.mark.parametrize(
    "orders,expected_bid_quantities,expected_ask_quantities",
    [
        # Test case 1: Multiple orders at same price level
        (
            [
                (OrderSide.BUY, 100.0, 10.0),
                (OrderSide.BUY, 100.0, 15.0),
                (OrderSide.SELL, 101.0, 20.0),
            ],
            {Decimal("100.00"): Decimal("25")},
            {Decimal("101.00"): Decimal("20")},
        ),
        # Test case 2: Multiple price levels
        (
            [
                (OrderSide.BUY, 100.0, 10.0),
                (OrderSide.BUY, 99.0, 5.0),
                (OrderSide.SELL, 101.0, 7.0),
                (OrderSide.SELL, 102.0, 3.0),
            ],
            {Decimal("100.00"): Decimal("10"), Decimal("99.00"): Decimal("5")},
            {Decimal("101.00"): Decimal("7"), Decimal("102.00"): Decimal("3")},
        ),
        # Test case 3: Only buy orders
        (
            [
                (OrderSide.BUY, 100.0, 10.0),
                (OrderSide.BUY, 99.0, 5.0),
            ],
            {Decimal("100.00"): Decimal("10"), Decimal("99.00"): Decimal("5")},
            {},
        ),
        # Test case 4: Only sell orders
        (
            [
                (OrderSide.SELL, 101.0, 7.0),
                (OrderSide.SELL, 102.0, 3.0),
            ],
            {},
            {Decimal("101.00"): Decimal("7"), Decimal("102.00"): Decimal("3")},
        ),
    ],
)
def test_own_order_book_quantities_parametrized(
    orders,
    expected_bid_quantities,
    expected_ask_quantities,
):
    instrument_id = InstrumentId.from_str("AAPL.XNAS")
    book = OwnOrderBook(instrument_id)

    # Add orders based on the test parameters
    for i, (side, price, size) in enumerate(orders):
        order = OwnBookOrder(
            trader_id=TraderId("TRADER-001"),
            client_order_id=ClientOrderId(f"O-{i+1}"),
            venue_order_id=VenueOrderId("i"),
            side=side,
            price=Price(price, 2),
            size=Quantity(size, 0),
            order_type=OrderType.LIMIT,
            time_in_force=TimeInForce.GTC,
            status=OrderStatus.ACCEPTED,
            ts_last=2,
            ts_accepted=2,
            ts_submitted=1,
            ts_init=1,
        )
        book.add(order)

    bid_quantities = book.bid_quantity()
    ask_quantities = book.ask_quantity()

    assert dict(bid_quantities) == expected_bid_quantities
    assert dict(ask_quantities) == expected_ask_quantities


def test_bids_to_dict_with_status_filter():
    """
    Test filtering orders by status in bids_to_dict method.
    """
    instrument_id = InstrumentId.from_str("AAPL.XNAS")
    book = OwnOrderBook(instrument_id)

    # Add orders with different statuses
    submitted_order = OwnBookOrder(
        trader_id=TraderId("TRADER-001"),
        client_order_id=ClientOrderId("O-1"),
        venue_order_id=VenueOrderId("1"),
        side=OrderSide.BUY,
        price=Price(100.0, 2),
        size=Quantity(10.0, 0),
        order_type=OrderType.LIMIT,
        time_in_force=TimeInForce.GTC,
        status=OrderStatus.SUBMITTED,
        ts_last=2,
        ts_accepted=2,
        ts_submitted=1,
        ts_init=1,
    )

    accepted_order = OwnBookOrder(
        trader_id=TraderId("TRADER-001"),
        client_order_id=ClientOrderId("O-2"),
        venue_order_id=VenueOrderId("2"),
        side=OrderSide.BUY,
        price=Price(100.0, 2),
        size=Quantity(15.0, 0),
        order_type=OrderType.LIMIT,
        time_in_force=TimeInForce.GTC,
        status=OrderStatus.ACCEPTED,
        ts_last=2,
        ts_accepted=2,
        ts_submitted=1,
        ts_init=1,
    )

    canceled_order = OwnBookOrder(
        trader_id=TraderId("TRADER-001"),
        client_order_id=ClientOrderId("O-3"),
        venue_order_id=VenueOrderId("3"),
        side=OrderSide.BUY,
        price=Price(99.5, 2),
        size=Quantity(20.0, 0),
        order_type=OrderType.LIMIT,
        time_in_force=TimeInForce.GTC,
        status=OrderStatus.CANCELED,
        ts_last=3,
        ts_accepted=2,
        ts_submitted=1,
        ts_init=1,
    )

    book.add(submitted_order)
    book.add(accepted_order)
    book.add(canceled_order)

    # Test with no filter
    all_orders = book.bids_to_dict()
    assert len(all_orders) == 2  # Two price levels
    assert len(all_orders[Price(100.0, 2).as_decimal()]) == 2  # Two orders at 100.00

    # Test with single status filter
    submitted_orders = book.bids_to_dict(status={OrderStatus.SUBMITTED})
    assert len(submitted_orders) == 1  # One price level
    assert len(submitted_orders[Price(100.0, 2).as_decimal()]) == 1
    assert submitted_orders[Price(100.0, 2).as_decimal()][0].status == OrderStatus.SUBMITTED

    # Test with multiple status filter
    filtered_orders = book.bids_to_dict(status={OrderStatus.ACCEPTED, OrderStatus.CANCELED})
    assert len(filtered_orders) == 2  # Two price levels
    assert len(filtered_orders[Price(100.0, 2).as_decimal()]) == 1
    assert filtered_orders[Price(100.0, 2).as_decimal()][0].status == OrderStatus.ACCEPTED
    assert len(filtered_orders[Price(99.5, 2).as_decimal()]) == 1
    assert filtered_orders[Price(99.5, 2).as_decimal()][0].status == OrderStatus.CANCELED

    # Test with non-existent status
    empty_orders = book.bids_to_dict(status={OrderStatus.FILLED})
    assert len(empty_orders) == 0


def test_bid_quantity_with_status_filter():
    """
    Test filtering by status in bid_quantity method.
    """
    instrument_id = InstrumentId.from_str("AAPL.XNAS")
    book = OwnOrderBook(instrument_id)

    # Add orders with different statuses
    submitted_order = OwnBookOrder(
        trader_id=TraderId("TRADER-001"),
        client_order_id=ClientOrderId("O-1"),
        venue_order_id=None,
        side=OrderSide.BUY,
        price=Price(100.0, 2),
        size=Quantity(10.0, 0),
        order_type=OrderType.LIMIT,
        time_in_force=TimeInForce.GTC,
        status=OrderStatus.SUBMITTED,
        ts_last=1,
        ts_accepted=0,
        ts_submitted=1,
        ts_init=1,
    )

    accepted_order = OwnBookOrder(
        trader_id=TraderId("TRADER-001"),
        client_order_id=ClientOrderId("O-2"),
        venue_order_id=VenueOrderId("2"),
        side=OrderSide.BUY,
        price=Price(100.0, 2),
        size=Quantity(15.0, 0),
        order_type=OrderType.LIMIT,
        time_in_force=TimeInForce.GTC,
        status=OrderStatus.ACCEPTED,
        ts_last=2,
        ts_accepted=2,
        ts_submitted=1,
        ts_init=1,
    )

    book.add(submitted_order)
    book.add(accepted_order)

    # Test with no filter
    all_quantities = book.bid_quantity()
    assert len(all_quantities) == 1  # One price level
    assert all_quantities[Price(100.0, 2).as_decimal()] == Decimal("25")  # 10 + 15

    # Test with status filter
    submitted_quantities = book.bid_quantity(status={OrderStatus.SUBMITTED})
    assert len(submitted_quantities) == 1
    assert submitted_quantities[Price(100.0, 2).as_decimal()] == Decimal("10")

    # Test with non-existent status
    empty_quantities = book.bid_quantity(status={OrderStatus.FILLED})
    assert len(empty_quantities) == 0


def test_mixed_status_filtering():
    """
    Test filtering with orders of different statuses at different prices.
    """
    instrument_id = InstrumentId.from_str("AAPL.XNAS")
    book = OwnOrderBook(instrument_id)

    # Add bid orders with varied statuses and prices
    book.add(
        OwnBookOrder(
            trader_id=TraderId("TRADER-001"),
            client_order_id=ClientOrderId("O-1"),
            venue_order_id=None,
            side=OrderSide.BUY,
            price=Price(100.0, 2),
            size=Quantity(10.0, 0),
            order_type=OrderType.LIMIT,
            time_in_force=TimeInForce.GTC,
            status=OrderStatus.SUBMITTED,
            ts_last=2,
            ts_accepted=2,
            ts_submitted=1,
            ts_init=1,
        ),
    )

    book.add(
        OwnBookOrder(
            trader_id=TraderId("TRADER-001"),
            client_order_id=ClientOrderId("O-2"),
            venue_order_id=VenueOrderId("2"),
            side=OrderSide.BUY,
            price=Price(100.0, 2),
            size=Quantity(20.0, 0),
            order_type=OrderType.LIMIT,
            time_in_force=TimeInForce.GTC,
            status=OrderStatus.ACCEPTED,
            ts_last=2,
            ts_accepted=2,
            ts_submitted=1,
            ts_init=1,
        ),
    )

    book.add(
        OwnBookOrder(
            trader_id=TraderId("TRADER-001"),
            client_order_id=ClientOrderId("O-3"),
            venue_order_id=None,
            side=OrderSide.BUY,
            price=Price(99.0, 2),
            size=Quantity(15.0, 0),
            order_type=OrderType.LIMIT,
            time_in_force=TimeInForce.GTC,
            status=OrderStatus.SUBMITTED,
            ts_last=1,
            ts_accepted=0,
            ts_submitted=1,
            ts_init=1,
        ),
    )

    book.add(
        OwnBookOrder(
            trader_id=TraderId("TRADER-001"),
            client_order_id=ClientOrderId("O-4"),
            venue_order_id=None,
            side=OrderSide.SELL,
            price=Price(101.0, 2),
            size=Quantity(5.0, 0),
            order_type=OrderType.LIMIT,
            time_in_force=TimeInForce.GTC,
            status=OrderStatus.SUBMITTED,
            ts_last=1,
            ts_accepted=0,
            ts_submitted=1,
            ts_init=1,
        ),
    )

    book.add(
        OwnBookOrder(
            trader_id=TraderId("TRADER-001"),
            client_order_id=ClientOrderId("O-5"),
            venue_order_id=VenueOrderId("5"),
            side=OrderSide.SELL,
            price=Price(101.0, 2),
            size=Quantity(25.0, 0),
            order_type=OrderType.LIMIT,
            time_in_force=TimeInForce.GTC,
            status=OrderStatus.ACCEPTED,
            ts_last=2,
            ts_accepted=2,
            ts_submitted=1,
            ts_init=1,
        ),
    )

    # Filter both sides by SUBMITTED status
    submitted_bids = book.bids_to_dict(status={OrderStatus.SUBMITTED})
    submitted_asks = book.asks_to_dict(status={OrderStatus.SUBMITTED})

    assert len(submitted_bids) == 2
    assert len(submitted_bids[Price(100.0, 2).as_decimal()]) == 1
    assert len(submitted_bids[Price(99.0, 2).as_decimal()]) == 1
    assert submitted_bids[Price(100.0, 2).as_decimal()][0].size == Quantity(10.0, 0)

    assert len(submitted_asks) == 1
    assert len(submitted_asks[Price(101.0, 2).as_decimal()]) == 1
    assert submitted_asks[Price(101.0, 2).as_decimal()][0].size == Quantity(5.0, 0)

    # Check quantities with ACCEPTED filter
    accepted_bid_qty = book.bid_quantity(status={OrderStatus.ACCEPTED})
    accepted_ask_qty = book.ask_quantity(status={OrderStatus.ACCEPTED})

    assert len(accepted_bid_qty) == 1
    assert accepted_bid_qty[Price(100.0, 2).as_decimal()] == Decimal("20")

    assert len(accepted_ask_qty) == 1
    assert accepted_ask_qty[Price(101.0, 2).as_decimal()] == Decimal("25")
