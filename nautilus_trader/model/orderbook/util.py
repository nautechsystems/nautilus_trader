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

"""
Utility functions for working with Orderbook models. These used to reside on `Orderbook` and `Level` but have been
moved here for simplification and to determine if they are still useful.

"""
from bisect import bisect
import copy
from itertools import accumulate
from itertools import islice
from operator import attrgetter

from nautilus_trader.model.orderbook.ladder import Ladder
from nautilus_trader.model.orderbook.order import Order


def cumulative(ladder, attrib="volume"):
    """
    >>> orders = [(100, 10), (90, 5), (80, 1)]

    >>> bids = Ladder(levels=[Order(price, volume, BID) for price, volume in orders], side=BID)
    >>> tuple(bids.cumulative('volume'))
    (10, 15, 16)

    >>> asks = Ladder(levels=[Order(price, volume, ASK) for price, volume in orders], side=ASK)
    >>> tuple(asks.cumulative('volume'))
    (1, 6, 16)
    """
    values = tuple(ladder.get_attrib(attrib))
    if ladder.reverse:
        values = reversed(values)
    return accumulate(values)


def check_for_trade(orderbook, order: Order):
    """
    Run an auction match on this order to see if any would trade
    :param order:
    :return: trade, order
    """
    ladder_trades, order_trades = auction_match(other=Ladder.from_orders([order]))
    traded_volume = sum((t.volume for t in ladder_trades))

    remaining_order = None
    if order.volume != traded_volume:
        remaining_order = Order(
            price=order.price, volume=order.volume - traded_volume, side=order.side
        )

    return ladder_trades, remaining_order


def bisect_idx_depth(v, values, reverse):
    """
    Returns the depth index of v in values
    >>> l = Ladder(side=ASK)
    >>> l.bisect_idx_depth(10, [5, 7, 11], side=ASK)
    2
    >>> l = Ladder(side=ASK)
    >>> l.bisect_idx_depth(0.1, [0.1, 0.3, 0.5], side=ASK)
    0
    """
    values = tuple(values)
    if v in values:
        idx = values.index(v)
    else:
        idx = bisect(values, v)
    if reverse:
        idx = len(values) - idx
    return idx


def depth_at_price(ladder, price, depth_type="volume"):
    """
    Find the depth (volume or exposure) that would be filled at a given price
    >>> orders = [(100, 6), (90, 3), (85, 15), (80, 10), (70, 1)]
    >>> bids = Ladder.from_orders(orders=[Order(price=p, volume=v, side=BID) for p, v in orders])
    >>> bids.depth_at_price(82)
    24.0

    >>> bids = Ladder.from_orders(orders=[Order(price=p, volume=v, side=BID) for p, v in orders])
    >>> bids.depth_at_price(60)
    35.0

    >>> asks = Ladder.from_orders(orders=[Order(price=p, volume=v, side=ASK) for p, v in orders])
    >>> asks.depth_at_price(70)
    1.0

    >>> asks = Ladder.from_orders(orders=[Order(price=p, volume=v, side=ASK) for p, v in orders])
    >>> asks.depth_at_price(82)
    11.0
    """

    idx = bisect_idx_depth(
        v=price, values=ladder.get_attrib("price"), reverse=ladder.reverse
    )
    values = tuple(ladder.get_attrib(depth_type))
    if ladder.reverse:
        values = reversed(values)
    if idx == 0:
        idx = 1
    return sum(islice(values, 0, idx))


def depth_for_volume(ladder, value, depth_type="volume"):
    """
    Find the levels in this ladder required to fill a certain volume/exposure

    :param value: volume to be filled
    :param depth_type: {'volume', 'exposure'}
    :return:
    >>> orders = [(100, 6), (90, 3), (85, 15), (80, 10), (70, 1)]

    >>> bids = Ladder([Order(price, volume, BID) for price, volume in orders], side=BID)
    >>> bids.depth_for_volume(15)
    [<Order(price=100, side=OrderSide.BID, volume=6)>, <Order(price=90, side=OrderSide.BID, volume=3)>, <Order(price=85, side=OrderSide.BID, volume=6)>]

    >>> asks = Ladder([Order(price, volume, ASK) for price, volume in orders], side=ASK)
    >>> asks.depth_for_volume(15)
    [<Order(price=70, side=OrderSide.ASK, volume=1)>, <Order(price=80, side=OrderSide.ASK, volume=10)>, <Order(price=85, side=OrderSide.ASK, volume=4)>]
    """
    depth = tuple(ladder.cumulative(depth_type))
    levels = ladder.levels
    idx = ladder.bisect_idx_depth(v=value, values=depth, reverse=ladder.reverse)
    if ladder.reverse:
        idx = len(depth) - idx
        levels = tuple(reversed(levels))
    orders = sum(map(attrgetter("orders"), levels[: idx + 1]), [])
    orders = [copy.copy(order) for order in orders]

    if len(orders) == 0:
        return ()
    if (
        len(orders) == 1
    ):  # We are totally filled within the first order, just take our value
        remaining_volume = value
    else:  # We have multiple orders, but we won't necessarily take the full volume on the last order
        remaining_volume = value - depth[idx - 1]
    if (
        depth_type == "exposure"
    ):  # Can't set a value for exposure, need to adjust via volume
        remaining_volume = remaining_volume / orders[-1].price
    orders[-1] = orders[-1].replace(volume=remaining_volume)
    return orders


def exposure_fill_price(ladder, exposure):
    """
    Returns the average price that a certain exposure order would be filled at

    >>> l = Ladder([Order(100, 1, BID), Order(50, 2, BID), Order(30, 10, BID)], side=BID)
    >>> l.exposure_fill_price(200)
    75.0
    >>> l = Ladder([Order(100, 1, BID), Order(50, 2, BID), Order(30, 10, BID)], side=BID)
    >>> l.exposure_fill_price(50)
    100.0
    """
    orders = ladder.depth_for_volume(exposure, depth_type="exposure")
    if not orders:
        return
    return sum(
        p * s / exposure for p, s in map(attrgetter("price", "exposure"), orders)
    )


def volume_fill_price(ladder, volume):
    """
    Returns the average price that a certain volume order would be filled at

    >>> l = Ladder([Order(100, 1, BID), Order(50, 2, BID), Order(30, 10, BID)], side=BID)
    >>> volume_fill_price(l, 2)
    75.0
    """
    orders = ladder.depth_for_volume(volume, depth_type="volume")
    return sum(p * s / volume for p, s in map(attrgetter("price", "volume"), orders))


def auction_match(ladder1, ladder2, on="volume"):
    """
    >>> l1 = Ladder(levels=[Order(103, 5, BID), Order(102, 10, BID), Order(100, 5, BID), Order(90, 5, BID)], side=BID)
    >>> l2 = Ladder(levels=[Order(100, 10, ASK), Order(101, 10, ASK), Order(105, 5, ASK), Order(110, 5, ASK)], side=ASK)
    >>> l1.auction_match(l2, on='volume')
    (101.125, [<Order(price=103, side=OrderSide.BID, volume=5)>, <Order(price=102, side=OrderSide.BID, volume=10)>, <Order(price=100, side=OrderSide.BID, volume=5)>], [<Order(price=100, side=OrderSide.ASK, volume=10)>, <Order(price=101, side=OrderSide.ASK, volume=10)>])
    """
    default = [], []
    assert ladder1.side != ladder2.side
    if not (ladder1.top_level and ladder2.top_level):
        return default
    ladder1_exposure = ladder1.depth_at_price(ladder2.top_level.price, depth_type=on)
    ladder2_exposure = ladder2.depth_at_price(ladder1.top_level.price, depth_type=on)
    matched_exposure = min(ladder1_exposure, ladder2_exposure)

    if matched_exposure == 0:
        return default

    traded_self = ladder1.depth_for_volume(matched_exposure, depth_type=on)
    traded_other = ladder2.depth_for_volume(matched_exposure, depth_type=on)
    return traded_self, traded_other


def match_orders(traded_bids, traded_asks):
    def match(bids, asks):
        assert sum([o.volume for o in bids]) == sum([o.volume for o in asks])
        bid_iter, ask_iter = iter(bids), iter(asks)
        bid, ask = next(bid_iter), next(ask_iter)
        while True:
            if bid.volume == ask.volume:
                yield (bid, ask)
                bid, ask = next(bid_iter), next(ask_iter)
            if bid.volume > ask.volume:
                yield (bid.copy(volume=ask.volume), ask)
                bid.volume -= ask.volume
                ask = next(ask_iter)
            if bid.volume < ask.volume:
                yield (bid, ask.copy(volume=bid.volume))
                ask.volume -= bid.volume
                bid = next(bid_iter)

    matched = {}
    matched_orders = list(match(bids=traded_bids, asks=traded_asks))
    for bid, ask in matched_orders:
        matched.setdefault(bid.order_id, list())
        matched.setdefault(ask.order_id, list())
        matched[bid.order_id].append(ask)
        matched[ask.order_id].append(bid)
    return matched


def pprint_ob(orderbook, num_levels=3):
    from tabulate import tabulate

    levels = reversed(
        [
            lvl
            for lvl in orderbook.bids.levels[-num_levels:]
            + orderbook.asks.levels[:num_levels]
        ]
    )
    data = [
        {
            "bids": [
                order.id for order in level.orders if level in orderbook.bids.levels
            ]
            or None,
            "price": level.price,
            "asks": [
                order.id for order in level.orders if level in orderbook.asks.levels
            ]
            or None,
        }
        for level in levels
    ]
    return tabulate(
        data, headers="keys", numalign="center", floatfmt=".2f", tablefmt="fancy"
    )
