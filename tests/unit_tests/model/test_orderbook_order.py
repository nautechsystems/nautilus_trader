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

import pytest

from nautilus_trader.model.data.book import BookOrder
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity


@pytest.mark.parametrize(
    ("side"),
    [
        OrderSide.BUY,
        OrderSide.SELL,
    ],
)
def test_init(side: OrderSide) -> None:
    order = BookOrder(
        price=Price.from_str("100"),
        size=Quantity.from_str("10"),
        side=side,
        order_id=1,
    )
    assert order.side == side
    assert order.price == 100
    assert order.size == 10
    assert order.order_id == 1


def test_signed_size():
    order = BookOrder(
        price=Price.from_str("10.0"),
        size=Quantity.from_str("1"),
        side=OrderSide.BUY,
        order_id=1,
    )
    assert order.size == 1
    assert order.signed_size() == 1.0

    order = BookOrder(
        price=Price.from_str("10.0"),
        size=Quantity.from_str("5"),
        side=OrderSide.SELL,
        order_id=2,
    )
    assert order.size == 5
    assert order.signed_size() == -5.0

    order = BookOrder(
        price=Price.from_str("10.0"),
        size=Quantity.zero(),
        side=OrderSide.SELL,
        order_id=3,
    )
    assert order.size == 0.0
    assert order.signed_size() == 0.0


def test_exposure():
    order = BookOrder(
        price=Price.from_str("100.0"),
        size=Quantity.from_str("10"),
        side=OrderSide.BUY,
        order_id=1,
    )
    assert order.exposure() == 1000.0


def test_hash_str_and_repr():
    # Arrange
    order = BookOrder(
        price=Price.from_str("100.0"),
        size=Quantity.from_str("5"),
        side=OrderSide.BUY,
        order_id=1,
    )

    # Act, Assert
    assert isinstance(hash(order), int)
    assert str(order) == r"BookOrder { side: Buy, price: 100.0, size: 5, order_id: 1 }"
    assert repr(order) == r"BookOrder { side: Buy, price: 100.0, size: 5, order_id: 1 }"


def test_to_dict_returns_expected_dict():
    # Arrange
    order = BookOrder(
        price=Price.from_str("100.00"),
        size=Quantity.from_str("5"),
        side=OrderSide.BUY,
        order_id=1,
    )

    # Act
    result = BookOrder.to_dict(order)

    # Assert
    assert result == {
        "type": "BookOrder",
        "side": "BUY",
        "price": "100.00",
        "size": "5",
        "order_id": 1,
    }


def test_from_dict_returns_expected_order():
    # Arrange
    order = BookOrder(
        price=Price.from_str("100.0"),
        size=Quantity.from_str("5"),
        side=OrderSide.BUY,
        order_id=1,
    )

    # Act
    result = BookOrder.from_dict(BookOrder.to_dict(order))

    # Assert
    assert result == order
