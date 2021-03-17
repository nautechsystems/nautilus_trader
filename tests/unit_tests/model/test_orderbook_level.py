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

from nautilus_trader.model.c_enums.order_side import OrderSide
from nautilus_trader.model.orderbook.level import Level
from nautilus_trader.model.orderbook.order import Order


@pytest.fixture
def empty_level():
    return Level()


def test_init(empty_level):
    assert len(empty_level.orders) == 0


def test_add(empty_level):
    order = Order(price=10, volume=100, side=OrderSide.BUY, id="1")
    empty_level.add(order=order)
    assert len(empty_level.orders) == 1


def test_update():
    order = Order(price=10, volume=100, side=OrderSide.BUY)
    level = Level(orders=[order])
    assert level.volume == 100
    order.update_volume(volume=50)
    level.update(order=order)
    assert level.volume == 50


# def test_init_orders():
#     orders = [Order(price=100, volume=10, side=OrderSide.SELL, id='1'),
#               Order(price=100, volume=1, side=OrderSide.SELL, id='2')]
#     l = Level(orders=orders)
#     assert len(l.orders) == 2
#     assert l.order_index == {'1': 0, '2': 1}
#
#
# def test_add():
#     l = Level(orders=[Order(price=100, volume=10, side=OrderSide.BUY), Order(price=100, volume=1, side=OrderSide.BUY)])
#     assert l.volume == 11
#     l.add(order=Order(price=100, volume=5, side=OrderSide.BUY))
#     assert l.volume == 16
#
#
# def test_delete_order():
#     l = Level(orders=[Order(price=100, volume=100, side=OrderSide.BUY, id="1")])
#     l.delete(order=Order(price=100, volume=20, side=OrderSide.BUY))
#     assert l.volume == 80
#
#
# def test_zero_volume_level():
#     l = Level(orders=[Order(price=10, volume=0, side=OrderSide.BUY)])
#     assert l.volume == 0
#
#
# def test_equality():
#     assert not Level(orders=[Order(price=10, volume=0, side=OrderSide.BUY)]) == None
#     assert not Level(orders=[Order(price=10, volume=0, side=OrderSide.BUY)]) == Level(
#         orders=[Order(price=10, volume=1, side=OrderSide.SELL)]
#     )
