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

from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.orderbook.data import BookOrder
from nautilus_trader.model.orderbook.level import Level


def test_init():
    level = Level(price=10.0)
    assert len(level.orders) == 0


def test_add():
    level = Level(price=10.0)
    order = BookOrder(price=10.0, size=100.0, side=OrderSide.BUY, id="1")
    level.add(order=order)
    assert len(level.orders) == 1


def test_update():
    level = Level(price=10.0)
    order = BookOrder(price=10.0, size=100.0, side=OrderSide.BUY)
    level.add(order)
    assert level.volume() == 100.0
    order.update_size(size=50.0)
    level.update(order=order)
    assert level.volume() == 50.0


def test_delete_order():
    level = Level(price=100.0)
    orders = [
        BookOrder(price=100.0, size=50.0, side=OrderSide.BUY, id="1"),
        BookOrder(price=100.0, size=50.0, side=OrderSide.BUY, id="2"),
    ]
    level.bulk_add(orders=orders)
    level.delete(order=orders[1])
    assert level.volume() == 50.0


def test_zero_volume_level():
    level = Level(price=10.0)
    level.bulk_add(orders=[BookOrder(price=10.0, size=0.0, side=OrderSide.BUY)])
    assert level.volume() == 0.0


def test_level_comparison():
    level1 = Level(price=10.0)
    level2 = Level(price=11.0)

    level1.add(BookOrder(price=10.0, size=0.0, side=OrderSide.BUY))
    level2.add(BookOrder(price=11.0, size=0.0, side=OrderSide.BUY))

    assert level2 >= level1
    assert level1 < level2
    assert level1 != level2


def test_level_repr():
    level = Level(price=10.0)
    level.add(BookOrder(price=10.0, size=0.0, side=OrderSide.BUY, id="1"))

    expected = "Level(price=10.0, orders=[BookOrder(10.0, 0.0, BUY, 1)])"
    assert str(level) == expected
