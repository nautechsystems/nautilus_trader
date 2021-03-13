# from cumulo.protocol.enums import BID, ASK
#
# from cumulo.protocol.models.level import Level, Order
#
#
# def test_init_orders():
#     orders = [Order(price=100, volume=10, side=ASK), Order(price=100, volume=1, side=ASK)]
#     l = Level(orders=orders)
#     assert l.price == 100
#     assert l.volume == 11
#     assert l.exposure == 1100
#     assert l.side == ASK
#
#
# def test_insert():
#     l = Level(orders=[Order(price=100, volume=10, side=BID), Order(price=100, volume=1, side=BID)])
#     assert l.volume == 11
#     l.insert(order=Order(price=100, volume=5, side=BID))
#     assert l.volume == 16
#
#
# def test_delete_order():
#     l = Level(orders=[Order(price=100, volume=100, side=BID, order_id="1")])
#     l.delete(order=Order(price=100, volume=20, side=BID))
#     assert l.volume == 80
#
#
# def test_delete_order_id():
#     l = Level(
#         orders=[
#             Order(price=100, volume=20, side=BID, order_id="1"),
#             Order(price=100, volume=50, side=BID, order_id="2"),
#         ]
#     )
#     l.delete(order_id="2")
#     remaining = [Order(price=100, volume=20, side=BID, order_id="1")]
#     assert l.orders == remaining
#
#     l.delete(order_id="1")
#     remaining = []
#     assert l.orders == remaining
#
#
# def test_zero_volume_level():
#     l = Level(orders=[Order(price=10, volume=0, side=BID)])
#     assert l.volume == 0
#
#
# def test_equality():
#     assert not Level(orders=[Order(price=10, volume=0, side=BID)]) == None
#     assert not Level(orders=[Order(price=10, volume=0, side=BID)]) == Level(
#         orders=[Order(price=10, volume=1, side=ASK)]
#     )
#
#
# def test_dict():
#     lvl = Level(orders=[Order(price=10, volume=0, side=BID, order_id="1")])
#     result = lvl.dict()
#     expected = {"orders": [{"order_id": "1", "price": 10.0, "side": BID, "volume": 0.0}], "price": 10.0, "side": BID}
#     assert result == expected
#
#
# def test_serialization():
#     lvl = Level(orders=[Order(price=10, volume=0, side=BID)])
#     raw = lvl.dumps()
#     assert len(raw) == 273
#     result = lvl.loads(raw)
#     assert result == lvl
#
#
# def test_order_id_orders():
#     lvl = Level(price=10, side=BID)
#     lvl.insert(order=Order(price=10, volume=1, side=BID, order_id="1"))
#     lvl.insert(order=Order(price=10, volume=1, side=BID, order_id="2"))
#     assert tuple(lvl.order_id_orders) == ("1", "2")
#     lvl.delete(order_id="2")
#     assert tuple(lvl.order_id_orders) == ("1",)
#     lvl.delete(order=Order(price=10, volume=1, side=BID, order_id="1"))
#     assert tuple(lvl.order_id_orders) == ()
