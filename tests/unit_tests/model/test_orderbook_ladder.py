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
    return TestStubs.order_book(bid_price=10.0, ask_price=15.0).asks


@pytest.fixture()
def bids():
    return TestStubs.order_book(bid_price=10.0, ask_price=15.0).bids


def test_init():
    ladder = Ladder(reverse=False, price_precision=2, size_precision=2)
    assert ladder


def test_reverse(asks):
    assert not asks.reverse


def test_insert():
    orders = [
        Order(price=100.0, volume=10.0, side=OrderSide.BUY),
        Order(price=100.0, volume=1.0, side=OrderSide.BUY),
        Order(price=105.0, volume=20.0, side=OrderSide.BUY),
    ]
    ladder = Ladder(reverse=False, price_precision=0, size_precision=0)
    for order in orders:
        ladder.add(order=order)
    ladder.add(order=Order(price=100.0, volume=10.0, side=OrderSide.BUY))
    ladder.add(order=Order(price=101.0, volume=5.0, side=OrderSide.BUY))
    ladder.add(order=Order(price=101.0, volume=5.0, side=OrderSide.BUY))

    expected = [
        (100, 21),
        (101, 10),
        (105, 20),
    ]
    result = [(level.price, level.volume()) for level in ladder.levels]
    assert result == expected


def test_delete_individual_order(asks):
    orders = [
        Order(price=100.0, volume=10.0, side=OrderSide.BUY, id="1"),
        Order(price=100.0, volume=5.0, side=OrderSide.BUY, id="2"),
    ]
    ladder = TestStubs.ladder(reverse=True, orders=orders)
    ladder.delete(orders[0])
    assert ladder.volumes() == [5.0]


def test_delete_level():
    orders = [Order(price=100.0, volume=10.0, side=OrderSide.BUY)]
    ladder = TestStubs.ladder(reverse=True, orders=orders)
    ladder.delete(orders[0])
    assert ladder.levels == []


def test_update_level():
    order = Order(price=100.0, volume=10.0, side=OrderSide.BUY, id="1")
    ladder = TestStubs.ladder(reverse=True, orders=[order])
    order.update_volume(volume=20.0)
    ladder.update(order)
    assert ladder.levels[0].volume() == 20


def test_update_no_volume(bids):
    order = bids.levels[0].orders[0]
    order.update_volume(volume=0.0)
    bids.update(order)
    assert order.price not in bids.prices()


def test_top_level(bids, asks):
    assert bids.top().price == Price("10")
    assert asks.top().price == Price("15")


def test_exposure():
    orders = [
        Order(price=100.0, volume=10.0, side=OrderSide.SELL),
        Order(price=101.0, volume=10.0, side=OrderSide.SELL),
        Order(price=105.0, volume=5.0, side=OrderSide.SELL),
    ]
    ladder = TestStubs.ladder(reverse=True, orders=orders)
    assert tuple(ladder.exposures()) == (1000.0, 1010.0, 525.0)


def test_depth_at_price_no_trade(bids, asks):
    result = asks.depth_at_price(price=Price(12))
    assert result == (Price("0"), Quantity("0"))

    result = bids.depth_at_price(price=Price(12))
    assert result == (Price("0"), Quantity("0"))


def test_depth_at_price_middle(bids, asks):
    result = asks.depth_at_price(price=Price("15.5"))
    assert result == (15, 10)
    result = asks.depth_at_price(price=Price(16))
    assert result == (Price("15.6667"), 30)
    result = bids.depth_at_price(price=Price("9.1"))
    assert result == (10.0, 10.0)


def test_depth_at_price_all_levels(bids, asks):
    result = asks.depth_at_price(price=Price(20))
    assert result == (Price("16.3333"), 60)

    result = bids.depth_at_price(price=Price(1))
    assert result == (Price("8.6667"), 60)


def test_depth_at_price_exposure(bids, asks):
    result = asks.depth_at_price(price=Price("15.1"), depth_type=DepthType.EXPOSURE)
    assert result == (15, 150)

    result = bids.depth_at_price(price=Price(1), depth_type=DepthType.EXPOSURE)
    assert result == (Price("8.7308"), 520)


def test_volume_fill_price_amounts(bids, asks):
    assert asks.volume_fill_price(Quantity(11)) == (Price("15.0909"), 11)
    assert asks.volume_fill_price(Quantity(10)) == (15.0, 10)
    assert asks.volume_fill_price(Quantity(30)) == (Price("15.6667"), 30)

    assert bids.volume_fill_price(Quantity(11)) == (Price("9.9091"), 11)
    assert bids.volume_fill_price(Quantity(10)) == (10.0, 10)
    assert bids.volume_fill_price(Quantity(30)) == (Price("9.3333"), 30)


def test_volume_fill_price_partial(asks):
    assert asks.volume_fill_price(Quantity(100)) == (
        Price("16.3333"),
        Quantity("60.0000"),
    )


def test_exposure_fill_price(asks):
    result = asks.exposure_fill_price(exposure=200)
    print(result)
    assert result == (Price("15.2500"), Quantity("200.0000"))


def test_repr(asks):
    expected = (
        "Ladder([Level(price=15.0, orders=[Order(15.0, 10.0, SELL, 15.0)]), "
        "Level(price=16.0, orders=[Order(16.0, 20.0, SELL, 16.0)]), Level(price=17.0, "
        "orders=[Order(17.0, 30.0, SELL, 17.0)])])"
    )
    assert str(asks) == expected
