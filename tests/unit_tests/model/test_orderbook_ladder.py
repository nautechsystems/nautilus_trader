# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2022 Nautech Systems Pty Ltd. All rights reserved.
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

import pytest

from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.orderbook.data import BookOrder
from nautilus_trader.model.orderbook.ladder import Ladder
from nautilus_trader.test_kit.stubs.data import TestDataStubs


@pytest.fixture()
def asks():
    return TestDataStubs.order_book(bid_price=10.0, ask_price=15.0).asks


@pytest.fixture()
def bids():
    return TestDataStubs.order_book(bid_price=10.0, ask_price=15.0).bids


def test_init():
    ladder = Ladder(reverse=False, price_precision=2, size_precision=2)
    assert ladder


def test_reverse(asks):
    assert not asks.is_reversed


def test_insert():
    orders = [
        BookOrder(price=100.0, size=10.0, side=OrderSide.BUY),
        BookOrder(price=100.0, size=1.0, side=OrderSide.BUY),
        BookOrder(price=105.0, size=20.0, side=OrderSide.BUY),
    ]
    ladder = Ladder(reverse=False, price_precision=0, size_precision=0)
    for order in orders:
        ladder.add(order=order)
    ladder.add(order=BookOrder(price=100.0, size=10.0, side=OrderSide.BUY))
    ladder.add(order=BookOrder(price=101.0, size=5.0, side=OrderSide.BUY))
    ladder.add(order=BookOrder(price=101.0, size=5.0, side=OrderSide.BUY))

    expected = [
        (100, 21),
        (101, 10),
        (105, 20),
    ]
    result = [(level.price, level.volume()) for level in ladder.levels]
    assert result == expected


def test_delete_individual_order(asks):
    orders = [
        BookOrder(price=100.0, size=10.0, side=OrderSide.BUY, id="1"),
        BookOrder(price=100.0, size=5.0, side=OrderSide.BUY, id="2"),
    ]
    ladder = TestDataStubs.ladder(reverse=True, orders=orders)
    ladder.delete(orders[0])
    assert ladder.volumes() == [5.0]


def test_delete_level():
    orders = [BookOrder(price=100.0, size=10.0, side=OrderSide.BUY)]
    ladder = TestDataStubs.ladder(reverse=True, orders=orders)
    ladder.delete(orders[0])
    assert ladder.levels == []


def test_update_level():
    order = BookOrder(price=100.0, size=10.0, side=OrderSide.BUY, id="1")
    ladder = TestDataStubs.ladder(reverse=True, orders=[order])
    order.update_size(size=20.0)
    ladder.update(order)
    assert ladder.levels[0].volume() == 20


def test_update_no_volume(bids):
    order = bids.levels[0].orders[0]
    order.update_size(size=0.0)
    bids.update(order)
    assert order.price not in bids.prices()


def test_top_level(bids, asks):
    assert bids.top().price == Price.from_str("10")
    assert asks.top().price == Price.from_str("15")


def test_exposure():
    orders = [
        BookOrder(price=100.0, size=10.0, side=OrderSide.SELL),
        BookOrder(price=101.0, size=10.0, side=OrderSide.SELL),
        BookOrder(price=105.0, size=5.0, side=OrderSide.SELL),
    ]
    ladder = TestDataStubs.ladder(reverse=True, orders=orders)
    assert tuple(ladder.exposures()) == (525.0, 1000.0, 1010.0)
    assert ladder.is_reversed


def test_repr(asks):
    expected = (
        "Ladder([Level(price=15.0, orders=[BookOrder(15.0, 10.0, SELL, 15.00000)]), "
        "Level(price=16.0, orders=[BookOrder(16.0, 20.0, SELL, 16.00000)]), Level(price=17.0, "
        "orders=[BookOrder(17.0, 30.0, SELL, 17.00000)])])"
    )

    assert str(asks) == expected


def test_simulate_order_fills_no_trade(asks):
    fills = asks.simulate_order_fills(
        order=BookOrder(price=10, size=10, side=OrderSide.BUY, id="1"),
    )
    assert fills == []


def test_simulate_order_fills_single(asks):
    fills = asks.simulate_order_fills(
        order=BookOrder(price=15, size=10, side=OrderSide.BUY, id="1"),
    )
    assert fills == [(Price.from_str("15.0000"), Quantity.from_str("10.0000"))]


def test_simulate_order_fills_multiple_levels(asks):
    fills = asks.simulate_order_fills(
        order=BookOrder(price=20, size=20, side=OrderSide.BUY, id="1"),
    )
    expected = [
        (Price.from_str("15.0000"), Quantity.from_str("10.0000")),
        (Price.from_str("16.0000"), Quantity.from_str("10.0000")),
    ]
    assert fills == expected


def test_simulate_order_fills_whole_ladder(asks):
    fills = asks.simulate_order_fills(
        order=BookOrder(price=100, size=1000, side=OrderSide.BUY, id="1"),
    )
    expected = [
        (Price.from_str("15.0000"), Quantity.from_str("10.0000")),
        (Price.from_str("16.0000"), Quantity.from_str("20.0000")),
        (Price.from_str("17.0000"), Quantity.from_str("30.0000")),
    ]
    assert fills == expected


def test_simulate_order_fills_l3():
    ladder = Ladder(False, 4, 4)
    orders = [
        BookOrder(price=15, size=1, side=OrderSide.SELL, id="1"),
        BookOrder(price=16, size=2, side=OrderSide.SELL, id="2"),
        BookOrder(price=16, size=3, side=OrderSide.SELL, id="3"),
        BookOrder(price=20, size=10, side=OrderSide.SELL, id="4"),
    ]
    for order in orders:
        ladder.add(order)

    fills = ladder.simulate_order_fills(
        order=BookOrder(price=16.5, size=4, side=OrderSide.BUY, id="1"),
    )
    expected = [
        (Price.from_str("15.0000"), Quantity.from_str("1.0000")),
        (Price.from_str("16.0000"), Quantity.from_str("2.0000")),
        (Price.from_str("16.0000"), Quantity.from_str("1.0000")),
    ]
    assert fills == expected
