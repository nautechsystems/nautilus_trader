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

import logging

from nautilus_trader.model.c_enums.order_side import OrderSide
from nautilus_trader.model.orderbook.ladder import Ladder
from nautilus_trader.model.orderbook.level import Level


class OrderBookProxy:
    """
    Provides an order book proxy - A L3 order book that can be proxied to L3/L2/L1 OrderBook classes.
    """

    def __init__(self):
        self.bids = Ladder(reverse=True)
        self.asks = Ladder(reverse=False)

    def add(self, order):
        if order.side == OrderSide.BUY:
            self.bids.add(order=order)
        elif order.side == OrderSide.SELL:
            self.asks.add(order=order)

    def update(self, order):
        if order.side == OrderSide.BUY:
            self.bids.update(order=order)
        elif order.side == OrderSide.SELL:
            self.asks.update(order=order)

    def delete(self, order):
        if order.side == OrderSide.BUY:
            self.bids.delete(order=order)
        elif order.side == OrderSide.SELL:
            self.asks.delete(order=order)

    def clear_bids(self):
        self.bids = Ladder(reverse=True)

    def clear_asks(self):
        self.asks = Ladder(reverse=True)

    def clear(self):
        """ Clear the entire orderbook """
        self.clear_bids()
        self.clear_asks()

    def _check_integrity(self, deep=True):
        if self.best_bid is None or self.best_ask is None:
            return True
        if not self.best_bid.price < self.best_ask.price:
            logging.warning("Price in cross")
            return False
        if deep:
            if not [lvl.price for lvl in self.bids.price_levels] == sorted(
                [lvl.price for lvl in self.bids.price_levels]
            ):
                return False
            if not [lvl.price for lvl in self.asks.price_levels] == sorted(
                [lvl.price for lvl in self.asks.price_levels], reverse=True
            ):
                return False
        return True

    @property
    def best_bid(self) -> Level:
        return self.bids.top

    @property
    def best_ask(self) -> Level:
        return self.asks.top


class OrderBookMixin:
    """ Shared methods for order book subclasses """

    def _check_integrity(self, deep=True):
        return self._orderbook._check_integrity(deep=deep)

    @property
    def bids(self):
        return self._orderbook.bids

    @property
    def asks(self):
        return self._orderbook.asks

    @property
    def best_bid(self):
        return self._orderbook.best_bid

    @property
    def best_ask(self):
        return self._orderbook.best_ask

    @property
    def spread(self):
        bid = self.best_bid
        ask = self.best_ask
        if bid and ask:
            return ask - bid

    def best_bid_price(self):
        bid = self.best_bid
        if bid:
            return bid.price

    def best_ask_price(self):
        ask = self.best_ask
        if ask:
            return ask.price

    def best_bid_qty(self):
        bid = self.best_bid  # type: Level
        if bid:
            return bid.volume

    def best_ask_qty(self):
        ask = self.best_ask  # type: Level
        if ask:
            return ask.volume

    def repr(self):
        from nautilus_trader.model.orderbook.util import pprint_ob

        return pprint_ob(self)


class L3OrderBook(OrderBookMixin):
    """ A L3 OrderBook. Should map directly to functionality of the OrderBookProxy """

    def __init__(self):
        self._orderbook = OrderBookProxy()

    def add(self, order):
        self._orderbook.add(order=order)

    def update(self, order):
        self._orderbook.update(order=order)

    def delete(self, order):
        self._orderbook.delete(order=order)


class L2OrderBook(OrderBookMixin):
    """ A L2 Orderbook. An Orderbook where price `Levels` are only made up of a single order """

    def __init__(self):
        self._orderbook = OrderBookProxy()

    def add(self, order):
        """
        Add a new order to this L2 Orderbook
        :param order:
        :return:
        """
        self._process_order(order=order)
        self._orderbook.add(order=order)

    def update(self, order):
        """
        If this `order.price` exists, need to remove and replace with `order`
        :param order:
        :return:
        """
        self._process_order(order=order)
        self._remove_if_exists(order)
        self._orderbook.update(order=order)

    def delete(self, order):
        """
        Delete this order (and the entire level for L2)
        :param order:
        :return:
        """
        self._process_order(order=order)
        self._orderbook.delete(order=order)

    def _process_order(self, order):
        """
        Because L2 Orderbook only has one order per level, we replace the order.id with a price level, which will let
        us easily process the order in the proxy orderbook.
        """
        order.id = str(order.price)
        return order

    def _remove_if_exists(self, order):
        """
        For a L2 orderbook, an order update means a whole level update. If this level exists, remove it so we can
        insert the new level
        """
        if order.side == OrderSide.BUY and order.price in self.bids.prices:
            self.delete(order)
        elif order.side == OrderSide.SELL and order.price in self.asks.prices:
            self.delete(order)

    def _check_integrity(self, deep=True):
        """ For L2 Orderbook, ensure only one order per level in addition to normal orderbook checks """
        if not self._orderbook._check_integrity(deep=deep):
            return False
        for level in self._orderbook.bids.levels + self._orderbook.asks.levels:
            assert len(level.orders) == 1
        return True


class L1OrderBook(OrderBookMixin):
    """ A L1 Orderbook that has only has a single (top) level """

    def __init__(self):
        self._orderbook = OrderBookProxy()

    def add(self, order):
        """
        Add an order to this L1 Orderbook - will call self.update(order=order) internally

        :param order:
        :return:
        """
        raise NotImplementedError("Use `update(order)` for L1Orderbook")

    def update(self, order):
        """
        Update an order in this L1 Orderbook

        :param order:
        :return:
        """
        # Because of the way we typically get updates from a L1 orderbook (bid and ask updates at the same time), its
        # quite probable that the last bid is now the ask price we are trying to insert (or vice versa). We just need to
        # add some extra protection against this if we are calling `_check_integrity` on each individual update .
        if (
            order.side == OrderSide.BUY
            and self.best_ask
            and order.price >= self.best_ask_price()
        ):
            self._orderbook.clear_asks()
        elif (
            order.side == OrderSide.SELL
            and self.best_bid
            and order.price <= self.best_bid_price()
        ):
            self._orderbook.clear_bids()
        self._orderbook.update(order=self._process_order(order=order))

    def delete(self, order):
        self._orderbook.delete(order=self._process_order(order=order))

    def _process_order(self, order):
        """
        Because L1 Orderbook only has one level per side, we replace the order.id with the name of the side, which will
        let us easily process the order in the proxy orderbook.
        """
        order.id = str(order.side)
        return order

    def _check_integrity(self, deep=True):
        """ For L1 Orderbook, ensure only one level per side in addition to normal orderbook checks """
        if not self._orderbook._check_integrity(deep=deep):
            return False
        assert len(self._orderbook.bids.levels) <= 1
        assert len(self._orderbook.asks.levels) <= 1
        return True
