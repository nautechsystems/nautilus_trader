# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.model.data.book import BookOrder
from nautilus_trader.model.enums import OrderSide


def test_init():
    order = BookOrder(price=100.0, size=10.0, side=OrderSide.BUY)
    assert order.price == 100
    assert order.size == 10
    assert order.side == OrderSide.BUY


def test_order_id():
    order = BookOrder(price=100.0, size=10.0, side=OrderSide.BUY, order_id="1")
    assert order.order_id == "1"

    order = BookOrder(price=100.0, size=10.0, side=OrderSide.BUY)
    print(order.order_id)
    assert len(order.order_id) == 36


def test_update_price():
    order = BookOrder(price=100.0, size=10.0, side=OrderSide.BUY)
    order.update_price(price=90.0)
    assert order.price == 90.0


def test_update_volume():
    order = BookOrder(price=100.0, size=10.0, side=OrderSide.BUY)
    order.update_size(size=5.0)
    assert order.size == 5.0


def test_update_order_id():
    order = BookOrder(price=100.0, size=10.0, side=OrderSide.BUY, order_id="1")
    order.update_order_id(value="2")
    assert order.order_id == "2"


def test_signed_volume():
    order = BookOrder(price=10.0, size=1.0, side=OrderSide.BUY)
    assert order.size == 1
    assert order.signed_size() == 1.0

    order = BookOrder(price=10.0, size=5.0, side=OrderSide.SELL)
    assert order.size == 5
    assert order.signed_size() == -5.0

    order = BookOrder(price=10.0, size=0.0, side=OrderSide.SELL)
    assert order.size == 0.0
    assert order.signed_size() == 0.0


def test_exposure():
    order = BookOrder(price=100.0, size=10.0, side=OrderSide.BUY)
    assert order.exposure() == 1000.0


def test_hash_str_and_repr():
    # Arrange
    order = BookOrder(price=10, size=5, side=OrderSide.BUY)

    # Act, Assert
    assert isinstance(hash(order), int)
    assert str(order) == f"BookOrder(10.0, 5.0, BUY, {order.order_id})"
    assert repr(order) == f"BookOrder(10.0, 5.0, BUY, {order.order_id})"


def test_to_dict_returns_expected_dict():
    # Arrange
    order = BookOrder(price=10, size=5, side=OrderSide.BUY, order_id="1")

    # Act
    result = BookOrder.to_dict(order)

    # Assert
    assert result == {
        "type": "BookOrder",
        "order_id": "1",
        "price": 10.0,
        "side": "BUY",
        "size": 5.0,
    }


def test_from_dict_returns_expected_order():
    # Arrange
    order = BookOrder(price=10, size=5, side=OrderSide.BUY)

    # Act
    result = BookOrder.from_dict(BookOrder.to_dict(order))

    # Assert
    assert result == order
