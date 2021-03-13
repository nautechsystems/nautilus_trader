from nautilus_trader.model.c_enums.order_side import OrderSide

from nautilus_trader.model.orderbook.level import Order, L2Level


# ---- L2 Tests ----- #


def test_init():
    level = L2Level(orders=[Order(price=10, volume=100, side=OrderSide.BUY)])
    assert len(level.orders) == 1


def test_update():
    level = L2Level(orders=[Order(price=10, volume=100, side=OrderSide.BUY)])
    assert level.volume() == 100
    level.update(volume=50)
    assert level.volume() == 50


# ---- L3 Tests ----- #

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
# def test_delete_order_id():
#     l = Level(
#         orders=[
#             Order(price=100, volume=20, side=OrderSide.BUY, id="1"),
#             Order(price=100, volume=50, side=OrderSide.BUY, id="2"),
#         ]
#     )
#     l.delete(id="2")
#     remaining = [Order(price=100, volume=20, side=OrderSide.BUY, id="1")]
#     assert l.orders == remaining
#
#     l.delete(id="1")
#     remaining = []
#     assert l.orders == remaining
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
#
#
# def test_dict():
#     lvl = Level(orders=[Order(price=10, volume=0, side=OrderSide.BUY, id="1")])
#     result = lvl.dict()
#     expected = {"orders": [{"order_id": "1", "price": 10.0, "side": OrderSide.BUY, "volume": 0.0}], "price": 10.0, "side": OrderSide.BUY}
#     assert result == expected
#
#
# def test_serialization():
#     lvl = Level(orders=[Order(price=10, volume=0, side=OrderSide.BUY)])
#     raw = lvl.dumps()
#     assert len(raw) == 273
#     result = lvl.loads(raw)
#     assert result == lvl


# def test_order_id_orders():
#     lvl = Level(price=10, side=OrderSide.BUY)
#     lvl.add(order=Order(price=10, volume=1, side=OrderSide.BUY, id="1"))
#     lvl.add(order=Order(price=10, volume=1, side=OrderSide.BUY, id="2"))
#     assert tuple(lvl.order_index) == ("1", "2")
#     lvl.delete(id="2")
#     assert tuple(lvl.order_index) == ("1",)
#     lvl.delete(order=Order(price=10, volume=1, side=OrderSide.BUY, id="1"))
#     assert tuple(lvl.order_index) == ()
#     assert tuple(lvl.order_index) == ()
