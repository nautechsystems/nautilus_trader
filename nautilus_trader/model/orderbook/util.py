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
Utility functions for working with OrderBook models.

These used to reside on `Orderbook` and `Level` but have been moved here for
simplification and to determine if they are still useful.

"""

from nautilus_trader.model.orderbook.ladder import Ladder
from nautilus_trader.model.orderbook.order import Order


# def cumulative(ladder, attrib="volume"):
#     """
#     >>> orders = [(100, 10), (90, 5), (80, 1)]
#
#     >>> bids = Ladder(levels=[Order(price, volume, BID) for price, volume in orders], side=BID)
#     >>> tuple(bids.cumulative('volume'))
#     (10, 15, 16)
#
#     >>> asks = Ladder(levels=[Order(price, volume, ASK) for price, volume in orders], side=ASK)
#     >>> tuple(asks.cumulative('volume'))
#     (1, 6, 16)
#     """
#     values = tuple(ladder.get_attrib(attrib))
#     if ladder.reverse:
#         values = reversed(values)
#     return accumulate(values)


def check_for_trade(ladder: Ladder, order: Order):
    """
    Run an auction match on this order to see if any would trade
    :param order:
    :return: trade, order
    """
    ladder_trades, order_trades = auction_match(
        ladder1=ladder, ladder2=Ladder.from_orders([order])
    )
    traded_volume = sum((t.volume for t in ladder_trades))

    remaining_order = None
    if order.volume != traded_volume:
        remaining_order = Order(
            price=order.price,
            volume=order.volume - traded_volume,
            side=order.side,
        )

    return ladder_trades, remaining_order


def auction_match(ladder1, ladder2, on="volume"):
    """
    Combine two opposing ladders (bids/asks) to see if any orders would trade
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
