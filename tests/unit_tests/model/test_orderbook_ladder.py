# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2021 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.model.enums import DepthType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.orderbook.ladder import Ladder
from nautilus_trader.model.orderbook.order import Order
from tests.test_kit.stubs import TestStubs


@pytest.fixture()
def asks():
    return TestStubs.order_book(bid_price=10, ask_price=15).asks


@pytest.fixture()
def bids():
    return TestStubs.order_book(bid_price=10, ask_price=15).bids


def test_init():
    ladder = Ladder(is_bid=False)
    assert ladder


def test_reverse(asks):
    assert not asks.reverse()


def test_insert():
    orders = [
        Order(price=Price(100), volume=Quantity(10), side=OrderSide.BUY),
        Order(price=Price(100), volume=Quantity(1), side=OrderSide.BUY),
        Order(price=Price(105), volume=Quantity(20), side=OrderSide.BUY),
    ]
    ladder = Ladder(is_bid=False)
    for order in orders:
        ladder.add(order=order)
    ladder.add(
        order=Order(price=Price("100.0"), volume=Quantity("10.0"), side=OrderSide.BUY)
    )
    ladder.add(
        order=Order(price=Price("101.0"), volume=Quantity("5.0"), side=OrderSide.BUY)
    )
    ladder.add(
        order=Order(price=Price("101.0"), volume=Quantity("5.0"), side=OrderSide.BUY)
    )

    expected = [
        (100, 21),
        (101, 10),
        (105, 20),
    ]
    result = [(level.price(), level.volume()) for level in ladder.levels]
    assert result == expected


def test_delete_individual_order(asks):
    orders = [
        Order(price=Price(100), volume=Quantity(10), side=OrderSide.BUY, id="1"),
        Order(price=Price(100), volume=Quantity(5), side=OrderSide.BUY, id="2"),
    ]
    ladder = TestStubs.ladder(is_bid=True, orders=orders)
    ladder.delete(orders[0])
    assert ladder.volumes() == [5.0]


def test_delete_level():
    orders = [Order(price=Price(100), volume=Quantity(10), side=OrderSide.BUY)]
    ladder = TestStubs.ladder(is_bid=True, orders=orders)
    ladder.delete(orders[0])
    assert ladder.levels == []


def test_update_level():
    order = Order(price=Price(100), volume=Quantity(10), side=OrderSide.BUY, id="1")
    ladder = TestStubs.ladder(is_bid=True, orders=[order])
    order.update_volume(volume=Quantity("20.0"))
    ladder.update(order)
    assert ladder.levels[0].volume() == 20


def test_update_no_volume(bids):
    order = bids.levels[0].orders[0]
    order.update_volume(volume=Quantity(0))
    bids.update(order)
    assert order.price not in bids.prices()


def test_top_level(bids, asks):
    assert bids.top().price() == Price("10")
    assert asks.top().price() == Price("15")


def test_exposure():
    orders = [
        Order(price=Price(100), volume=Quantity(10), side=OrderSide.SELL),
        Order(price=Price(101), volume=Quantity(10), side=OrderSide.SELL),
        Order(price=Price(105), volume=Quantity(5), side=OrderSide.SELL),
    ]
    ladder = TestStubs.ladder(is_bid=True, orders=orders)
    assert tuple(ladder.exposures()) == (1000.0, 1010.0, 525.0)


def test_depth_at_price_no_trade(bids, asks):
    result = asks.depth_at_price(price=Price(12))
    assert result == 0.0

    result = bids.depth_at_price(price=Price(12))
    assert result == 0.0


def test_depth_at_price_middle(bids, asks):
    result = asks.depth_at_price(price=Price("15.5"))
    assert result == 10.0
    result = asks.depth_at_price(price=Price(16))
    assert result == 20.0
    result = bids.depth_at_price(price=Price("9.1"))
    assert result == 10.0


def test_depth_at_price_all_levels(bids, asks):
    result = asks.depth_at_price(price=Price(20))
    assert result == 30

    result = bids.depth_at_price(price=Price(1))
    assert result == 30


def test_depth_at_price_exposure(bids, asks):
    result = asks.depth_at_price(price=Price("15.1"), depth_type=DepthType.EXPOSURE)
    assert result == 150

    result = bids.depth_at_price(price=Price(1), depth_type=DepthType.EXPOSURE)
    assert result == 270


def test_volume_fill_price_amounts(asks):
    price = asks.volume_fill_price(Quantity(10))
    assert price == 15
    price = asks.volume_fill_price(Quantity(20))
    assert price == 15.5
    price = asks.volume_fill_price(Quantity(30))
    assert price == 16


def test_volume_fill_price_partial(asks):
    price = asks.volume_fill_price(Quantity(31), partial_ok=False)
    assert price is None

    price = asks.volume_fill_price(Quantity(31), partial_ok=True)
    assert price == 16
