# import pytest
#
# from cumulo.protocol.enums import BID, ASK, OrderSide
# from cumulo.protocol.models.ladder import Ladder
# from cumulo.protocol.models.level import Level
# from cumulo.protocol.models.order import Order
#
#
# def test_init():
#     # 15.2 µs ± 59.4 ns per loop
#     orders = [
#         Order(price=100, volume=10, side=ASK),
#         Order(price=100, volume=1, side=ASK),
#         Order(price=105, volume=20, side=ASK),
#     ]
#     l = Ladder.from_orders(orders=orders)
#     cmp = [Level(orders=[Order(price=100, volume=11, side=ASK)]), Level(orders=[Order(price=105, volume=20, side=ASK)])]
#     assert l.levels == cmp
#     assert tuple(l.exposures) == (1100, 2100)
#     assert tuple(l.cumulative("exposure")) == (1100, 3200)
#     assert tuple(l.cumulative("volume")) == (11, 31)
#     assert l.side == ASK
#
#
# def test_insert():
#     # 14.5 µs ± 139 ns per loop
#     orders = [
#         Order(price=100, volume=10, side=BID),
#         Order(price=100, volume=1, side=BID),
#         Order(price=105, volume=20, side=BID),
#     ]
#     ladder = Ladder.from_orders(orders=orders)
#     ladder.insert(order=Order(price=100, volume=10, side=BID))
#     ladder.insert(order=Order(price=101, volume=5, side=BID))
#     ladder.insert(order=Order(price=101, volume=5, side=BID))
#
#     expected = [
#         Level(orders=[Order(price=100, volume=21, side=BID)]),
#         Level(orders=[Order(price=101, volume=10, side=BID)]),
#         Level(orders=[Order(price=105, volume=20, side=BID)]),
#     ]
#     assert all([(r.price, r.volume) == (e.price, e.volume) for r, e in zip(ladder.levels, expected)])
#
#
# @pytest.mark.skip
# def test_delete_order():
#     l = Ladder.from_orders(
#         [Order(price=100, volume=10, side=BID, order_id="1"), Order(price=100, volume=5, side=BID, order_id="2")]
#     )
#     # TODO ladder.delete order - do we need this?
#     l.delete(order=Order(price=100, volume=1, side=BID))
#
#
# def test_delete_order_by_id():
#     orders = [Order(price=100, volume=10, side=BID, order_id="1"), Order(price=100, volume=5, side=BID, order_id="2")]
#     l = Ladder.from_orders(orders=orders)
#     l.delete(order_id="1")
#     expected = [Level(orders=[Order(price=100, volume=5, side=BID, order_id="2")])]
#     assert l.levels == expected
#     assert len(l.levels[0].orders) == 1
#
#
# def test_delete_order_id():
#     l = Ladder.from_orders(
#         [Order(price=100, volume=10, side=BID, order_id="1"), Order(price=100, volume=10, side=BID, order_id="2")]
#     )
#     l.delete(order_id="2")
#     assert l.levels[0].orders == [Order(price=100, volume=10, side=BID, order_id="1")]
#
#
# def test_delete_level():
#     orders = [Order(price=100, volume=10, side=BID)]
#     l = Ladder.from_orders(orders=orders)
#     l.delete(level=Level(orders=[Order(price=100, volume=1, side=BID)]))
#     assert l.levels == []
#
#
# def test_update_level():
#     l = Ladder.from_orders([Order(price=100, volume=10, side=BID, order_id="1")])
#     l.update(level=Level.from_level(price=100, volume=20, side=BID))
#     assert l.levels[0].volume == 20
#
#
# def test_update_order_id():
#     l = Ladder.from_orders([Order(price=100, volume=10, side=BID, order_id="1")])
#     l.update(order=Order(price=100, volume=1, side=BID, order_id="1"))
#     assert l.levels[0].volume == 1
#
#
# def test_exposure():
#     orders = [
#         Order(price=100, volume=10, side=ASK),
#         Order(price=101, volume=10, side=ASK),
#         Order(price=105, volume=5, side=ASK),
#         Order(price=110, volume=5, side=ASK),
#         Order(price=130, volume=100, side=ASK),
#     ]
#     l = Ladder.from_orders(orders=orders)
#     assert tuple(l.exposures) == (1000, 1010, 525, 550, 13000)
#     assert tuple(l.cumulative("exposure")) == (1000, 2010, 2535, 3085, 16085)
#
#
# def test_from_orders():
#     def order_iterable():
#         test_orders = [Order(price=1.01, volume=12.11, side=BID), Order(price=5.8, volume=2.85, side=BID)]
#         for order in test_orders:
#             yield order
#
#     ladder = Ladder.from_orders(orders=order_iterable())
#     assert ladder.depth_at_price(5.8) == 2.85
#
#
# def test_check_for_trade():
#     bids = Ladder.from_orders([Order(price=103, volume=5, side=BID), Order(price=102, volume=10, side=BID)])
#     order = Order(price=100, volume=10, side=ASK)
#     trades, new_order = bids.check_for_trade(order)
#     assert trades[0].price == 103
#     assert trades[0].volume == 5
#     assert trades[1].price == 102
#     assert trades[1].volume == 5
#     assert new_order is None
#
#
# def test_insert_price():
#     bids = Ladder.from_orders([Order(price=100, volume=5, side=ASK)])
#     order = Order(price=1000, volume=3, side=BID)
#     trades, new_order = bids.check_for_trade(order)
#     assert trades[0].price == 100
#     assert trades[0].volume == 3
#
#
# def test_insert_remaining():
#     bids = Ladder.from_orders([Order(price=103, volume=1, side=BID), Order(price=102, volume=1, side=BID)])
#     order = Order(price=100, volume=3, side=ASK)
#     trades, new_order = bids.check_for_trade(order)
#     assert trades[0].price == 103
#     assert trades[0].volume == 1
#     assert new_order.price == 100
#     assert new_order.volume == 1
#
#
# def test_update_no_volume(bids):
#     order = Order(price=2.0, volume=0, side=BID)
#     bids.update(level=Level(orders=[order]))
#     assert order.price not in bids.prices
#
#
# def test_top_level(bids, asks):
#     assert bids.top_level.price == 11.0
#     assert asks.top_level.price == 1.20
#
#
# def test_top_level_empty(bids, asks):
#     bids = Ladder(side=OrderSide.BID)
#     assert bids.top_level is None
#
#
# def test_slice(orders, bids, asks):
#     result = bids.top(2)
#     expected = orders[:2]
#     assert all(r.price == e["price"] for r, e in zip(result, expected))
#
#     result = asks.top(2)
#     expected = list(reversed(orders[-2:]))
#     assert all(r.price == e["price"] for r, e in zip(result, expected))
#
#
# def test_slice_reversed(orders, bids, asks):
#     bids.reverse = not bids.reverse
#     result = bids.top(2)
#     expected = list(reversed(orders[-2:]))
#     assert all(r.price == e["price"] for r, e in zip(result, expected))
#
#     asks.reverse = not asks.reverse
#     result = asks.top(2)
#     expected = orders[:2]
#     assert all(r.price == e["price"] for r, e in zip(result, expected))
#
#
# def test_order_id_prices():
#     orders = [Order(price=103, volume=1, side=BID, order_id="1"), Order(price=102, volume=1, side=BID, order_id="2")]
#     ladder = Ladder.from_orders(orders=orders)
#     assert ladder.order_id_prices == {"1": 103, "2": 102}
#     ladder.insert(order=Order(price=102, volume=1, side=BID, order_id="3"))
#     assert ladder.order_id_prices == {"1": 103, "2": 102, "3": 102}
#     ladder.delete(order_id="1")
#     assert ladder.order_id_prices == {"2": 102, "3": 102}
#
#     # TODO ladder.delete order - do we need this?
#     # ladder.delete(order=Order(price=102, volume=1, side=BID, order_id='2'))
#     # assert ladder.order_id_prices == {'3': 102}
#
#
# def test_dict():
#     orders = [Order(price=103, volume=1, side=BID, order_id="a"), Order(price=102, volume=1, side=BID, order_id="b")]
#     ladder = Ladder.from_orders(orders=orders)
#     result = ladder.dict()
#     expected = {
#         "exchange_order_ids": False,
#         "levels": [
#             {"orders": [{"order_id": "b", "price": 102.0, "side": BID, "volume": 1.0}], "price": 102.0, "side": BID},
#             {"orders": [{"order_id": "a", "price": 103.0, "side": BID, "volume": 1.0}], "price": 103.0, "side": BID},
#         ],
#         "reverse": True,
#         "side": BID,
#     }
#
#     assert result == expected
#
#
# def test_serialization():
#     ladder = Ladder.from_orders([Order(price=103, volume=1, side=BID), Order(price=102, volume=1, side=BID)])
#     raw = ladder.dumps()
#     assert len(raw) == 673
#     result = ladder.loads(raw)
#     assert result == ladder
#     result = ladder.loads(ladder.dumps())
#     assert result.order_id_prices == ladder.order_id_prices
