import pytest
from nautilus_trader.model.c_enums.order_side import OrderSide

from nautilus_trader.model.orderbook.ladder import Ladder
from nautilus_trader.model.orderbook.order import Order


@pytest.fixture
def ladder():
    ladder = Ladder()
    orders = [
        Order(price=100, volume=10, side=OrderSide.SELL),
        Order(price=100, volume=1, side=OrderSide.SELL),
        Order(price=105, volume=20, side=OrderSide.SELL),
    ]
    for order in orders:
        ladder.add(order=order)
    return ladder


def test_cumulative():
    assert tuple(ladder.cumulative("exposure")) == (1100, 3200)
    assert tuple(ladder.cumulative("volume")) == (11, 31)
