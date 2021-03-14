from nautilus_trader.model.c_enums.order_side import OrderSide

from nautilus_trader.model.orderbook.level import Level
from nautilus_trader.model.orderbook.order import Order


def test_init():
    level = Level(orders=[Order(price=10, volume=100, side=OrderSide.BUY)])
    assert len(level.orders) == 1


def test_update():
    order = Order(price=10, volume=100, side=OrderSide.BUY)
    level = Level(orders=[order])
    assert level.volume() == 100
    order.update_volume(volume=50)
    level.update(order=order)
    assert level.volume() == 50


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
