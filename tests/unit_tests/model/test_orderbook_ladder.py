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
from nautilus_trader.model.orderbook.ladder import Ladder
from nautilus_trader.model.orderbook.order import Order


def test_init():
    ladder = Ladder(reverse=False)
    assert ladder


def test_insert():
    orders = [
        Order(price=100, volume=10, side=OrderSide.BUY),
        Order(price=100, volume=1, side=OrderSide.BUY),
        Order(price=105, volume=20, side=OrderSide.BUY),
    ]
    ladder = Ladder(reverse=False)
    for order in orders:
        ladder.add(order=order)
    ladder.add(order=Order(price=100, volume=10, side=OrderSide.BUY))
    ladder.add(order=Order(price=101, volume=5, side=OrderSide.BUY))
    ladder.add(order=Order(price=101, volume=5, side=OrderSide.BUY))

    expected = [
        (100, 21),
        (101, 10),
        (105, 20),
    ]
    result = [(level.price, level.volume) for level in ladder.levels]
    assert result == expected


# @pytest.mark.skip
# def test_delete_order():
#     l = Ladder.from_orders(
#         [Order(price=100, volume=10, side=OrderSide.BUY, order_id="1"),
#          Order(price=100, volume=5, side=OrderSide.BUY, order_id="2")]
#     )
#     # TODO ladder.delete order - do we need this?
#     l.delete(order=Order(price=100, volume=1, side=OrderSide.BUY))


# def test_init():
#     ladder = Ladder()
#     orders = [
#         Order(price=100, volume=10, side=OrderSide.SELL),
#         Order(price=100, volume=1, side=OrderSide.SELL),
#         Order(price=105, volume=20, side=OrderSide.SELL),
#     ]
#     for order in orders:
#         ladder.add(order=order)
#     cmp = [Level(orders=[Order(price=100, volume=11, side=OrderSide.SELL)]), Level(orders=[Order(price=105, volume=20, side=OrderSide.SELL)])]
#     assert ladder.levels == cmp
#     assert tuple(ladder.exposures) == (1100, 2100)
#     # TODO These have been moved
#     # assert tuple(ladder.cumulative("exposure")) == (1100, 3200)
#     # assert tuple(ladder.cumulative("volume")) == (11, 31)
#
#
# def test_insert():
#     orders = [
#         Order(price=100, volume=10, side=OrderSide.BUY),
#         Order(price=100, volume=1, side=OrderSide.BUY),
#         Order(price=105, volume=20, side=OrderSide.BUY),
#     ]
#     ladder = Ladder.from_orders(orders=orders)
#     ladder.insert(order=Order(price=100, volume=10, side=OrderSide.BUY))
#     ladder.insert(order=Order(price=101, volume=5, side=OrderSide.BUY))
#     ladder.insert(order=Order(price=101, volume=5, side=OrderSide.BUY))
#
#     expected = [
#         Level(orders=[Order(price=100, volume=21, side=OrderSide.BUY)]),
#         Level(orders=[Order(price=101, volume=10, side=OrderSide.BUY)]),
#         Level(orders=[Order(price=105, volume=20, side=OrderSide.BUY)]),
#     ]
#     assert all([(r.price, r.volume) == (e.price, e.volume) for r, e in zip(ladder.levels, expected)])
#
#
# @pytest.mark.skip
# def test_delete_order():
#     l = Ladder.from_orders(
#         [Order(price=100, volume=10, side=OrderSide.BUY, order_id="1"), Order(price=100, volume=5, side=OrderSide.BUY, order_id="2")]
#     )
#     # TODO ladder.delete order - do we need this?
#     l.delete(order=Order(price=100, volume=1, side=OrderSide.BUY))
#
#
# def test_delete_order_by_id():
#     orders = [Order(price=100, volume=10, side=OrderSide.BUY, order_id="1"), Order(price=100, volume=5, side=OrderSide.BUY, order_id="2")]
#     l = Ladder.from_orders(orders=orders)
#     l.delete(order_id="1")
#     expected = [Level(orders=[Order(price=100, volume=5, side=OrderSide.BUY, order_id="2")])]
#     assert l.levels == expected
#     assert len(l.levels[0].orders) == 1
#
#
# def test_delete_order_id():
#     l = Ladder.from_orders(
#         [Order(price=100, volume=10, side=OrderSide.BUY, order_id="1"), Order(price=100, volume=10, side=OrderSide.BUY, order_id="2")]
#     )
#     l.delete(order_id="2")
#     assert l.levels[0].orders == [Order(price=100, volume=10, side=OrderSide.BUY, order_id="1")]
#
#
# def test_delete_level():
#     orders = [Order(price=100, volume=10, side=OrderSide.BUY)]
#     l = Ladder.from_orders(orders=orders)
#     l.delete(level=Level(orders=[Order(price=100, volume=1, side=OrderSide.BUY)]))
#     assert l.levels == []
#
#
# def test_update_level():
#     l = Ladder.from_orders([Order(price=100, volume=10, side=OrderSide.BUY, order_id="1")])
#     l.update(level=Level.from_level(price=100, volume=20, side=OrderSide.BUY))
#     assert l.levels[0].volume == 20
#
#
# def test_update_order_id():
#     l = Ladder.from_orders([Order(price=100, volume=10, side=OrderSide.BUY, order_id="1")])
#     l.update(order=Order(price=100, volume=1, side=OrderSide.BUY, order_id="1"))
#     assert l.levels[0].volume == 1
#
#
# def test_exposure():
#     orders = [
#         Order(price=100, volume=10, side=OrderSide.SELL),
#         Order(price=101, volume=10, side=OrderSide.SELL),
#         Order(price=105, volume=5, side=OrderSide.SELL),
#         Order(price=110, volume=5, side=OrderSide.SELL),
#         Order(price=130, volume=100, side=OrderSide.SELL),
#     ]
#     l = Ladder.from_orders(orders=orders)
#     assert tuple(l.exposures) == (1000, 1010, 525, 550, 13000)
#     assert tuple(l.cumulative("exposure")) == (1000, 2010, 2535, 3085, 16085)
#
#
# def test_from_orders():
#     def order_iterable():
#         test_orders = [Order(price=1.01, volume=12.11, side=OrderSide.BUY), Order(price=5.8, volume=2.85, side=OrderSide.BUY)]
#         for order in test_orders:
#             yield order
#
#     ladder = Ladder.from_orders(orders=order_iterable())
#     assert ladder.depth_at_price(5.8) == 2.85
#
#
# def test_check_for_trade():
#     bids = Ladder.from_orders([Order(price=103, volume=5, side=OrderSide.BUY), Order(price=102, volume=10, side=OrderSide.BUY)])
#     order = Order(price=100, volume=10, side=OrderSide.SELL)
#     trades, new_order = bids.check_for_trade(order)
#     assert trades[0].price == 103
#     assert trades[0].volume == 5
#     assert trades[1].price == 102
#     assert trades[1].volume == 5
#     assert new_order is None
#
#
# def test_insert_price():
#     bids = Ladder.from_orders([Order(price=100, volume=5, side=OrderSide.SELL)])
#     order = Order(price=1000, volume=3, side=OrderSide.BUY)
#     trades, new_order = bids.check_for_trade(order)
#     assert trades[0].price == 100
#     assert trades[0].volume == 3
#
#
# def test_insert_remaining():
#     bids = Ladder.from_orders([Order(price=103, volume=1, side=OrderSide.BUY), Order(price=102, volume=1, side=OrderSide.BUY)])
#     order = Order(price=100, volume=3, side=OrderSide.SELL)
#     trades, new_order = bids.check_for_trade(order)
#     assert trades[0].price == 103
#     assert trades[0].volume == 1
#     assert new_order.price == 100
#     assert new_order.volume == 1
#
#
# def test_update_no_volume(bids):
#     order = Order(price=2.0, volume=0, side=OrderSide.BUY)
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
#     bids = Ladder(side=OrderSide.OrderSide.BUY)
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
#     orders = [Order(price=103, volume=1, side=OrderSide.BUY, order_id="1"), Order(price=102, volume=1, side=OrderSide.BUY, order_id="2")]
#     ladder = Ladder.from_orders(orders=orders)
#     assert ladder.order_id_prices == {"1": 103, "2": 102}
#     ladder.insert(order=Order(price=102, volume=1, side=OrderSide.BUY, order_id="3"))
#     assert ladder.order_id_prices == {"1": 103, "2": 102, "3": 102}
#     ladder.delete(order_id="1")
#     assert ladder.order_id_prices == {"2": 102, "3": 102}
#
#     # TODO ladder.delete order - do we need this?
#     # ladder.delete(order=Order(price=102, volume=1, side=OrderSide.BUY, order_id='2'))
#     # assert ladder.order_id_prices == {'3': 102}
