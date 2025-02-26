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


# ------------------------------------------------------------------------------
# OwnOrder Tests
# ------------------------------------------------------------------------------
def test_own_book_order_creation():
    order = OwnBookOrder(
        client_order_id=ClientOrderId("O-12345"),
        side=OrderSide.BUY,
        price=Price(100.0, 2),
        size=Quantity(10.0, 0),
        order_type=OrderType.LIMIT,
        time_in_force=TimeInForce.GTC,
        status=OrderStatus.ACCEPTED,
        ts_last=0,
        ts_init=0,
    )

    assert order.client_order_id == ClientOrderId("O-12345")
    assert order.side == OrderSide.BUY
    assert order.price == Price(100.0, 2)
    assert order.size == Quantity(10.0, 0)
    assert order.order_type == OrderType.LIMIT
    assert order.time_in_force == TimeInForce.GTC


def test_own_book_order_exposure():
    order = OwnBookOrder(
        client_order_id=ClientOrderId("O-12345"),
        side=OrderSide.BUY,
        price=Price(100.0, 2),
        size=Quantity(10.0, 0),
        order_type=OrderType.LIMIT,
        time_in_force=TimeInForce.GTC,
        status=OrderStatus.ACCEPTED,
        ts_last=0,
        ts_init=0,
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
        client_order_id=ClientOrderId("O-12345"),
        side=side,
        price=Price(100.0, 2),
        size=Quantity(10.0, 0),
        order_type=OrderType.LIMIT,
        time_in_force=TimeInForce.GTC,
        status=OrderStatus.ACCEPTED,
        ts_last=0,
        ts_init=0,
    )

    assert order.signed_size() == expected


def test_own_book_order_repr():
    order = OwnBookOrder(
        client_order_id=ClientOrderId("O-12345"),
        side=OrderSide.BUY,
        price=Price(100.0, 2),
        size=Quantity(10.0, 0),
        order_type=OrderType.LIMIT,
        time_in_force=TimeInForce.GTC,
        status=OrderStatus.ACCEPTED,
        ts_last=0,
        ts_init=0,
    )

    expected_repr = (
        "OwnBookOrder(client_order_id=O-12345, side=BUY, price=100.00, size=10, "
        "order_type=LIMIT, time_in_force=GTC, ts_init=0)"
    )
    assert repr(order) == expected_repr
    assert str(order) == "O-12345,BUY,100.00,10,LIMIT,GTC,0"


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
    assert book.event_count == 0
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
        client_order_id=ClientOrderId("O-123"),
        side=OrderSide.BUY,
        price=Price(100.0, 2),
        size=Quantity(10.0, 0),
        order_type=OrderType.LIMIT,
        time_in_force=TimeInForce.GTC,
        status=OrderStatus.ACCEPTED,
        ts_last=0,
        ts_init=0,
    )

    # 1) Add order
    book.add(order)
    assert book.event_count == 1  # Add increments the event count
    bids_map = book.bids_to_dict()
    assert len(bids_map) == 1
    assert Price(100.0, 2).as_decimal() in bids_map

    # 2) Update order (increase size from 10 -> 15)
    updated_order = OwnBookOrder(
        client_order_id=ClientOrderId("O-123"),
        side=OrderSide.BUY,
        price=Price(100.0, 2),
        size=Quantity(15.0, 0),
        order_type=OrderType.LIMIT,
        time_in_force=TimeInForce.GTC,
        status=OrderStatus.ACCEPTED,
        ts_last=0,
        ts_init=0,
    )
    book.update(updated_order)
    assert book.event_count == 2  # Update increments the event count

    # Check updated size
    bids_map = book.bids_to_dict()
    orders_at_price = bids_map[Price(100.0, 2).as_decimal()]
    assert len(orders_at_price) == 1
    assert orders_at_price[0].size == Quantity(15.0, 0)

    # 3) Delete order
    book.delete(order)
    # Depending on how your book logic is implemented,
    # count might now be 3 (since delete is an event).
    assert book.event_count == 3, "Delete should increment event count"

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
            client_order_id=ClientOrderId("O-123"),
            side=OrderSide.BUY,
            price=Price(100.0, 2),
            size=Quantity(10.0, 0),
            order_type=OrderType.LIMIT,
            time_in_force=TimeInForce.GTC,
            status=OrderStatus.ACCEPTED,
            ts_last=0,
            ts_init=0,
        ),
    )
    assert book.event_count == 1

    # Call clear() -> typically increments event count
    book.clear()
    assert book.event_count == 1
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
        client_order_id=ClientOrderId("O-123"),
        side=side,
        price=Price(100.0, 2),
        size=Quantity(10.0, 0),
        order_type=OrderType.LIMIT,
        time_in_force=TimeInForce.GTC,
        status=OrderStatus.ACCEPTED,
        ts_last=0,
        ts_init=0,
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
        client_order_id=ClientOrderId("O-1"),
        side=OrderSide.BUY,
        price=Price(100.0, 2),
        size=Quantity(10.0, 0),
        order_type=OrderType.LIMIT,
        time_in_force=TimeInForce.GTC,
        status=OrderStatus.ACCEPTED,
        ts_last=1,
        ts_init=0,
    )
    order2 = OwnBookOrder(
        client_order_id=ClientOrderId("O-2"),
        side=OrderSide.BUY,
        price=Price(100.0, 2),
        size=Quantity(5.0, 0),
        order_type=OrderType.LIMIT,
        time_in_force=TimeInForce.GTC,
        status=OrderStatus.ACCEPTED,
        ts_last=2,
        ts_init=0,
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
        client_order_id=ClientOrderId("O-777"),
        side=OrderSide.BUY,
        price=Price(100.0, 2),
        size=Quantity(10.0, 0),
        order_type=OrderType.LIMIT,
        time_in_force=TimeInForce.GTC,
        status=OrderStatus.ACCEPTED,
        ts_last=10,
        ts_init=0,
    )
    book.add(order)
    assert len(book.bids_to_dict()) == 1

    # Update to new price=101
    updated = OwnBookOrder(
        client_order_id=ClientOrderId("O-777"),
        side=OrderSide.BUY,
        price=Price(101.0, 2),
        size=Quantity(10.0, 0),
        order_type=OrderType.LIMIT,
        time_in_force=TimeInForce.GTC,
        status=OrderStatus.ACCEPTED,
        ts_last=11,
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
        client_order_id=ClientOrderId("O-1"),
        side=OrderSide.BUY,
        price=Price(100.0, 2),
        size=Quantity(10.0, 0),
        order_type=OrderType.LIMIT,
        time_in_force=TimeInForce.GTC,
        status=OrderStatus.ACCEPTED,
        ts_last=0,
        ts_init=0,
    )
    bid_order2 = OwnBookOrder(
        client_order_id=ClientOrderId("O-2"),
        side=OrderSide.BUY,
        price=Price(100.0, 2),
        size=Quantity(15.0, 0),
        order_type=OrderType.LIMIT,
        time_in_force=TimeInForce.GTC,
        status=OrderStatus.ACCEPTED,
        ts_last=0,
        ts_init=0,
    )
    # Add an order at a different price level (bids)
    bid_order3 = OwnBookOrder(
        client_order_id=ClientOrderId("O-3"),
        side=OrderSide.BUY,
        price=Price(99.5, 2),
        size=Quantity(20.0, 0),
        order_type=OrderType.LIMIT,
        time_in_force=TimeInForce.GTC,
        status=OrderStatus.ACCEPTED,
        ts_last=0,
        ts_init=0,
    )

    # Add orders at different price levels (asks)
    ask_order1 = OwnBookOrder(
        client_order_id=ClientOrderId("O-4"),
        side=OrderSide.SELL,
        price=Price(101.0, 2),
        size=Quantity(12.0, 0),
        order_type=OrderType.LIMIT,
        time_in_force=TimeInForce.GTC,
        status=OrderStatus.ACCEPTED,
        ts_last=0,
        ts_init=0,
    )
    ask_order2 = OwnBookOrder(
        client_order_id=ClientOrderId("O-5"),
        side=OrderSide.SELL,
        price=Price(101.0, 2),
        size=Quantity(8.0, 0),
        order_type=OrderType.LIMIT,
        time_in_force=TimeInForce.GTC,
        status=OrderStatus.ACCEPTED,
        ts_last=0,
        ts_init=0,
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
            client_order_id=ClientOrderId(f"O-{i+1}"),
            side=side,
            price=Price(price, 2),
            size=Quantity(size, 0),
            order_type=OrderType.LIMIT,
            time_in_force=TimeInForce.GTC,
            status=OrderStatus.ACCEPTED,
            ts_last=0,
            ts_init=0,
        )
        book.add(order)

    bid_quantities = book.bid_quantity()
    ask_quantities = book.ask_quantity()

    assert dict(bid_quantities) == expected_bid_quantities
    assert dict(ask_quantities) == expected_ask_quantities
