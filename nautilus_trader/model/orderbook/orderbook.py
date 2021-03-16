import logging

from nautilus_trader.model.c_enums.order_side import OrderSide

from nautilus_trader.model.orderbook.ladder import Ladder
from nautilus_trader.model.orderbook.level import Level


class OrderbookProxy:
    """
    An Orderbook proxy - A L3 Orderbook that can be proxied to L3/L2/L1 Orderbook classes.
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

    def clear(self):
        """ Clear the entire orderbook """
        self.bids = Ladder(reverse=True)
        self.asks = Ladder(reverse=False)

    def _check_integrity(self, deep=True):
        if self.best_bid is None or self.best_ask is None:
            return True
        if not self.best_bid.price < self.best_ask.price:
            logging.warning("Price in cross")
            return False
        if deep:
            if not [lvl.price for lvl in self.bids.price_levels] == sorted([lvl.price for lvl in self.bids.price_levels]):
                return False
            if not [lvl.price for lvl in self.asks.price_levels] == sorted([lvl.price for lvl in self.asks.price_levels], reverse=True):
                return False
        return True

    @property
    def best_bid(self) -> Level:
        return self.bids.top

    @property
    def best_ask(self) -> Level:
        return self.asks.top


class L3Orderbook:
    """ A L3 Orderbook. Should map directly to functionality of the OrderbookProxy """

    def __init__(self):
        self._orderbook = OrderbookProxy()

    @property
    def bids(self):
        return self._orderbook.bids

    @property
    def asks(self):
        return self._orderbook.asks

    def add(self, order):
        self._orderbook.add(order=order)

    def update(self, order):
        self._orderbook.update(order=order)

    def delete(self, order):
        self._orderbook.delete(order=order)

    def _check_integrity(self, deep=True):
        return self._orderbook._check_integrity(deep=deep)

    @property
    def best_bid(self):
        return self._orderbook.best_bid

    @property
    def best_ask(self):
        return self._orderbook.best_ask

    def repr(self):
        from nautilus_trader.model.orderbook.util import pprint_ob
        return pprint_ob(self)

# cdef class L2Orderbook:
#     """ A L2 Orderbook. An Orderbook where price `Levels` are only made up of a single order """
#
#     cpdef add(self, Order order):
#         """
#         If this `order.price` exists, need to remove and replace with `order`
#         :param order:
#         :return:
#         """
#         # self._orderbook.add(order=order)
#         raise NotImplemented
#
#     cpdef update(self, Order order):
#         """
#         If this `order.price` exists, need to remove and replace with `order`
#         :param order:
#         :return:
#         """
#         # self._orderbook.update(order=order)
#         raise NotImplemented
#
#     cpdef delete (self, Order order):
#         """
#         Delete this order (and the entire level for L2)
#         :param order:
#         :return:
#         """
#         # self._orderbook.delete(order=order)
#         raise NotImplemented
#
#
# cdef class L1Orderbook:
#     """ A L1 Orderbook. An Orderbook that has only has a single (top) level """
#     cdef OrderbookProxy _orderbook
#
#     cpdef add(self, Order order):
#         """
#         Need to remove previous `Level` and add new Level for `order`
#
#         :param order:
#         :return:
#         """
#         # self._orderbook.add(order=order)
#         raise NotImplemented
#
#     cpdef update(self, Order order):
#         """
#         If the price has changes, need to need to remove previous `Level` and add new Level for `order`
#
#         :param order:
#         :return:
#         """
#         # self._orderbook.update(order=order)
#         raise NotImplemented
#
#     cpdef delete (self, Order order):
#         # self._orderbook.delete(order=order)
#         raise NotImplemented


#     def top(self, n=1, side=None):
#         if side is None:
#             return {BID: self.bids.top(n=n), ASK: self.asks.top(n=n)}
#         elif side.lower() == "bid":
#             return self.bids.top(n=n)
#         elif side.lower() == "ask":
#             return self.asks.top(n=n)
#         else:
#             raise KeyError("Side should be one of (None, 'bid', 'ask')")
#
#     @property
#     def top_level(self) -> Dict[OrderSide, Level]:
#         return {BID: self.bids.top_level, ASK: self.asks.top_level}
#
#     @property
#     def best_bid(self):
#         return self.top_level[BID]
#
#     @property
#     def best_ask(self):
#         return self.top_level[ASK]
#
#     @property
#     def in_cross(self):
#         if self.best_bid is None or self.best_ask is None:
#             return False
#         return self.best_bid.price >= self.best_ask.price
#
#     def auction_match(self, on=None, remove_from_book=False):
#         """
#         Perform an auction match on this Orderbook to find any in-cross orders in the bid and ask Ladders.
#         :param on: {'volume', 'exposure'}
#         :param remove_from_book: Whether to remove the orders from this book
#         """
#         on = on or self.default_on
#         traded_bids, traded_asks = self.bids.auction_match(self.asks, on=on)
#         if remove_from_book:
#             for order in traded_bids + traded_asks:
#                 if order.side == BID:
#                     self.bids.delete(order)
#                 else:
#                     self.asks.delete(order)
#         self._remove_fak_orders()
#         return traded_bids + traded_asks

#     def __repr__(self):
#         return "Orderbook(%s)" % (" @ ".join(map(str, [self.bids.top_level, self.asks.top_level])))
