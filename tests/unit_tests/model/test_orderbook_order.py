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

from nautilus_trader.model.c_enums.order_side import OrderSide
from nautilus_trader.model.orderbook.data import Order


def test_init():
    order = Order(price=100.0, size=10.0, side=OrderSide.BUY)
    assert order.price == 100
    assert order.size == 10
    assert order.side == OrderSide.BUY


def test_order_id():
    order = Order(price=100.0, size=10.0, side=OrderSide.BUY, id="1")
    assert order.id == "1"

    order = Order(price=100.0, size=10.0, side=OrderSide.BUY)
    assert len(order.id) == 36


def test_update_price():
    order = Order(price=100.0, size=10.0, side=OrderSide.BUY)
    order.update_price(price=90.0)
    assert order.price == 90.0


def test_update_volume():
    order = Order(price=100.0, size=10.0, side=OrderSide.BUY)
    order.update_size(size=5.0)
    assert order.size == 5.0


def test_update_id():
    order = Order(price=100.0, size=10.0, side=OrderSide.BUY, id="1")
    order.update_id(value="2")
    assert order.id == "2"


def test_signed_volume():
    order = Order(price=10.0, size=1.0, side=OrderSide.BUY)
    assert order.size == 1 and order.signed_size() == 1.0

    order = Order(price=10.0, size=5.0, side=OrderSide.SELL)
    assert order.size == 5 and order.signed_size() == -5.0

    order = Order(price=10.0, size=0.0, side=OrderSide.SELL)
    assert order.size == 0.0 and order.signed_size() == 0.0


def test_exposure():
    order = Order(price=100.0, size=10.0, side=OrderSide.BUY)
    assert order.exposure() == 1000.0


def test_hash_str_and_repr():
    # Arrange
    order = Order(price=10, size=5, side=OrderSide.BUY)

    # Act, Assert
    assert isinstance(hash(order), int)
    assert str(order) == f"Order(10.0, 5.0, BUY, {order.id})"
    assert repr(order) == f"Order(10.0, 5.0, BUY, {order.id})"


def test_to_dict_returns_expected_dict():
    # Arrange
    order = Order(price=10, size=5, side=OrderSide.BUY, id="1")

    # Act
    result = Order.to_dict(order)

    # Assert
    assert result == {
        "type": "Order",
        "id": "1",
        "price": 10.0,
        "side": "BUY",
        "size": 5.0,
    }


def test_from_dict_returns_expected_order():
    # Arrange
    order = Order(price=10, size=5, side=OrderSide.BUY)

    # Act
    result = Order.from_dict(Order.to_dict(order))

    # Assert
    assert result == order
