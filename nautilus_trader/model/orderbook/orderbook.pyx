from nautilus_trader.model.orderbook.ladder import Ladder

cdef class Orderbook:
    cdef Ladder bids
    cdef Ladder asks
#     default_on = "volume"
#     exchange_order_ids: bool = Field(default=False, description="Do we receive order_ids from the exchange")
#     bids: Optional[Ladder] = None
#     asks: Optional[Ladder] = None
#     order_id_side: Dict[AnyStr, OrderSide] = Field(default_factory=dict)
#
#     def _check_for_trade(self, order):
#         """
#         Given an order,
#         :param order:
#         :return:
#         """
#         if order.side == BID and (self.asks.top_level and order.price > self.asks.top_level.price):
#             return self.asks.check_for_trade(order=order)
#         elif order.side == ASK and (self.bids.top_level and order.price < self.bids.top_level.price):
#             return self.bids.check_for_trade(order=order)
#         return [], order
#
#     def add(self, *_, order: Order, remove_trades=False):
#         """
#         Insert an order into this orderbook
#         :param order: The order to insert
#         :param remove_trades: Remove passive orders if `order` is in cross
#         :return:
#         """
#         assert order.side
#         trades, order = self._check_for_trade(order)
#         if order is not None:
#             if order.side == BID:
#                 self.bids.insert(order)
#             elif order.side == ASK:
#                 self.asks.insert(order)
#         if remove_trades:
#             for order in trades:
#                 if order.side == BID:
#                     self.bids.delete(order_id=order.order_id)
#                 elif order.side == ASK:
#                     self.asks.delete(order_id=order.order_id)
#         return trades
#
#     def remove(self, *_, order: Order = None, level: Level = None, order_id: str = None):
#         """
#         Delete or trade an order in this orderbook
#
#         :param order: Order to delete/trade
#         :param level: Level to delete
#         :param order_id: order_id to delete
#         :return:
#         """
#         assert_one(values=[order, level, order_id], error="Must pass `order`, `level` or `order_id`")
#         if level is not None:
#             return self._delete_level(level=level)
#         elif order is not None:
#             return self._delete_order(order=order)
#         elif order_id is not None:
#             return self._delete_order_id(order_id=order_id)
#
#     def update(self, *_, order: Order = None, level: Level = None):
#         assert_one(values=[order, level], error="Must pass `order`, `level` or `order_id`")
#         if level is not None:
#             return self._update_level(level=level)
#         elif order is not None:
#             return self._update_order(order=order)
#
#     def _delete_level(self, level: Level):
#         if level.side == BID:
#             self.bids.delete(level=level)
#         elif level.side == ASK:
#             self.asks.delete(level=level)
#
#     def _delete_order(self, order: Order):
#         if order.side == BID:
#             self.bids.delete(order=order)
#         elif order.side == ASK:
#             self.asks.delete(order=order)
#
#     def _delete_order_id(self, order_id: str):
#         side = self.order_id_side[order_id]
#         if side == BID:
#             self.bids.delete(order_id=order_id)
#         elif side == ASK:
#             self.asks.delete(order_id=order_id)
#
#     def _update_order(self, order: Order):
#         self.order_id_side[order.order_id] = order.side
#         if order.side == BID:
#             self.bids.update(order=order)
#         elif order.side == ASK:
#             self.asks.update(order=order)
#
#     def _update_level(self, level: Level):
#         if level.side == BID:
#             self.bids.update(level=level)
#         elif level.side == ASK:
#             self.asks.update(level=level)
#
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
#
#     def _remove_fak_orders(self):
#         for order in list(self.bids.iter_orders()) + list(self.asks.iter_orders()):
#             if order.order_type == FAK:
#                 if order.side == BID:
#                     self.bids.delete(order)
#                 elif order.side == ASK:
#                     self.asks.delete(order)
#
#     def transform(self, func, bids=True, asks=True):
#         if bids:
#             self.bids = Ladder.from_orders([func(order) for order in self.bids.iter_orders()])
#         if asks:
#             self.asks = Ladder.from_orders([func(order) for order in self.asks.iter_orders()])
#
#     def iter_orders(self):
#         for ladder in (self.bids, self.asks):
#             for order in ladder.iter_orders():
#                 yield order
#
#     def dict(self, **kwargs):
#         kwargs.update(dict(exclude={"order_id_side"}))
#         return super().dict(**kwargs)
#
#     def pprint(self, num_levels=3):
#         from tabulate import tabulate
#
#         empty = Level(orders=[], price=0, side=BID)
#         prices = reversed([lvl.price for lvl in self.bids.levels[:num_levels] + self.asks.levels[:num_levels]])
#         data = [
#             {
#                 "bids": [order.order_id for order in self.bids.price_levels.get(price, empty).orders] or None,
#                 "price": price,
#                 "asks": [order.order_id for order in self.asks.price_levels.get(price, empty).orders] or None,
#             }
#             for price in prices
#         ]
#         return tabulate(data, headers="keys", numalign="center", floatfmt=".2f", tablefmt="fancy")
#
#     def flatten(self, n_levels=1):
#         def flatten_lvl(level: Level, side, n):
#             return {f"orderbook_{side}_{k}_{n}": getattr(level, k) for k in ["price", "volume"]}
#
#         return merge_dicts(
#             *[
#                 flatten_lvl(level=level, side=side, n=i + 1)
#                 for side in ("bid", "ask")
#                 for i, level in enumerate(self.top(side=side, n=n_levels))
#             ]
#         )
#
#     def __repr__(self):
#         return "Orderbook(%s)" % (" @ ".join(map(str, [self.bids.top_level, self.asks.top_level])))
