# import bisect
# import copy
# import logging
# from itertools import accumulate, islice
# from operator import attrgetter
# from typing import List
#
# from nautilus_trader.model.orderbook.level import Level
# from nautilus_trader.model.orderbook.order import Order
#
# logger = logging.getLogger(__name__)
#
#
# cdef class Ladder:
#     cdef list levels
#     cdef bool reverse
#     # price_levels: Dict = Field(default_factory=dict)
#     # order_id_prices: Dict = Field(default_factory=dict)
#     # exchange_order_ids: bool = Field(default=False, description="Do we receive order_ids from the exchange")
#
#     # @classmethod
#     # def from_orders(cls, orders, **kwargs):
#     #     orders = list(orders)  # means we can handle generators
#     #     side = one(set(lvl.side for lvl in orders))
#     #     level_groups = groupby(orders, key=attrgetter("price"))
#     #     return cls(levels=[Level(orders=list(orders)) for _, orders in level_groups], side=side, **kwargs)
#
#     def insert(self, order: Order):
#         if order.price in self.prices:
#             idx = tuple(self.prices).index(order.price)
#             self.levels[idx].insert(order)
#         else:
#             self._insert_level(level=Level(orders=[order]))
#         self.order_id_prices[order.order_id] = order.price
#
#     def delete(self, *_, order: Order = None, level: Level = None, order_id: str = None):
#         """
#         :param order: Order to delete/trade
#         :param level: Level to delete/trade
#         :param order_id: order_id to find and delete
#         :return:
#         """
#         assert_one([order, level, order_id], "Must pass one and only one of `order`, `level` or `order_id`")
#         if order is not None:
#             raise NotImplementedError
#             # return self._delete_order(order=order)
#         elif order_id is not None:
#             return self._delete_order_id(order_id=order_id)
#         elif level is not None:
#             return self._delete_level(level=level)
#
#     def _delete_order(self, order: Order = None):
#         price_idx = tuple(self.prices).index(order.price)
#         deleted_orders = self.levels[price_idx].delete(order=order)
#         for del_order in deleted_orders:
#             del self.order_id_prices[del_order.order_id]
#         self._delete_level_by_price(price=order.price, only_if_empty=True)
#         return deleted_orders
#
#     def _delete_order_id(self, order_id):
#         price = self.order_id_prices[order_id]
#         price_idx = tuple(self.prices).index(price)
#         del self.order_id_prices[order_id]
#         order = self.levels[price_idx].delete(order_id=order_id)
#         self._delete_level_by_price(price=price, only_if_empty=True)
#         return order
#
#     def _delete_level(self, level: Level):
#         return self._delete_level_by_price(price=level.price, only_if_empty=False)
#
#     def _delete_level_by_price(self, price: float, only_if_empty=True):
#         if only_if_empty and self.price_levels[price].orders:
#             return
#         prices = tuple(self.prices)
#         if price not in prices:
#             logger.warning(f"Price {price} not in prices: {prices}")
#             return
#         price_idx = tuple(self.prices).index(price)
#         return self.levels.pop(price_idx)
#
#     def update(self, *_, order: Order = None, level: Level = None, order_update_drops_priority=False):
#         """
#         Update a level or order. Some caveats; because we can't know what the original order or level is when processing
#         a completed new price, this method assumes that:
#         - The `level` update passed contains ONLY THE UPDATED VOLUME
#         - The `order` passed contains the `order_id` to be updated.
#
#         :param level: Level to update
#         :param order: Order to update
#         :param order_update_drops_priority: Whether an order update causes an order to lose its priority
#         :return:
#         """
#         assert not order_update_drops_priority, "order_update_drops_priority not implemented yet"
#         assert_one(values=(level, order), error="Must pass one and only one of `level` or `order_id`")
#         if level is not None:
#             self._update_level(level=level)
#         elif order is not None:
#             self._update_order(order=order, order_update_drops_priority=order_update_drops_priority)
#
#     def _update_level(self, level: Level):
#         if level.price not in self.prices:
#             self._insert_level(level=level)
#         elif level.volume == 0:
#             self._delete_level(level=level)
#         else:
#             price_idx = tuple(self.prices).index(level.price)
#             self.levels[price_idx].update(volume=level.volume)
#
#     def _insert_level(self, level):
#         self.levels.insert(bisect.bisect(self.levels, level), level)
#         self.price_levels[level.price] = level
#
#     def _update_order(self, *_, order: Order, order_update_drops_priority: bool = False):
#         assert not order_update_drops_priority, "order_update_drops_priority not implemented yet"
#         if order.order_id not in self.order_id_prices:
#             return self.insert(order=order)
#         # Find the existing order
#         price = self.order_id_prices[order.order_id]
#         level = self.price_levels[price]
#         existing_order = level.order_id_orders[order.order_id]
#         if order.price == existing_order.price:
#             # This update contains a volume update
#             level.update(order_id=order.order_id, volume=order.volume)
#         else:
#             # New price for this order, delete and insert
#             if self.exchange_order_ids:
#                 self.delete(order_id=order.order_id)
#                 self.insert(order=order)
#             else:
#                 self.delete(order=order)
#                 self.insert(order=order)
#
#     def get_attrib(self, attrib):
#         return map(attrgetter(attrib), self.levels)
#
#     @property
#     def prices(self):
#         return self.get_attrib("price")
#
#     @property
#     def volume(self):
#         return self.get_attrib("volume")
#
#     @property
#     def exposures(self):
#         return self.get_attrib("exposure")
#
#     @property
#     def top_level(self) -> Level:
#         top = self.top(1)
#         if top:
#             return top[0]
#
#     def top(self, n=1) -> List[Level]:
#         if not self.levels:
#             return []
#         n = n or len(self.levels)
#         return list(reversed(self.levels[-n:])) if self.reverse else self.levels[:n]
#
#     def cumulative(self, attrib="volume"):
#         """
#         >>> orders = [(100, 10), (90, 5), (80, 1)]
#
#         >>> bids = Ladder(levels=[Order(price, volume, BID) for price, volume in orders], side=BID)
#         >>> tuple(bids.cumulative('volume'))
#         (10, 15, 16)
#
#         >>> asks = Ladder(levels=[Order(price, volume, ASK) for price, volume in orders], side=ASK)
#         >>> tuple(asks.cumulative('volume'))
#         (1, 6, 16)
#         """
#         values = tuple(self.get_attrib(attrib))
#         if self.reverse:
#             values = reversed(values)
#         return accumulate(values)
#
#     @staticmethod
#     def bisect_idx_depth(v, values, reverse):
#         """
#         Returns the depth index of v in values
#         >>> l = Ladder(side=ASK)
#         >>> l.bisect_idx_depth(10, [5, 7, 11], side=ASK)
#         2
#         >>> l = Ladder(side=ASK)
#         >>> l.bisect_idx_depth(0.1, [0.1, 0.3, 0.5], side=ASK)
#         0
#         """
#         values = tuple(values)
#         if v in values:
#             idx = values.index(v)
#         else:
#             idx = bisect.bisect(values, v)
#         if reverse:
#             idx = len(values) - idx
#         return idx
#
#     def depth_at_price(self, price, depth_type="volume"):
#         """
#         Find the depth (volume or exposure) that would be filled at a given price
#         >>> orders = [(100, 6), (90, 3), (85, 15), (80, 10), (70, 1)]
#         >>> bids = Ladder.from_orders(orders=[Order(price=p, volume=v, side=BID) for p, v in orders])
#         >>> bids.depth_at_price(82)
#         24.0
#
#         >>> bids = Ladder.from_orders(orders=[Order(price=p, volume=v, side=BID) for p, v in orders])
#         >>> bids.depth_at_price(60)
#         35.0
#
#         >>> asks = Ladder.from_orders(orders=[Order(price=p, volume=v, side=ASK) for p, v in orders])
#         >>> asks.depth_at_price(70)
#         1.0
#
#         >>> asks = Ladder.from_orders(orders=[Order(price=p, volume=v, side=ASK) for p, v in orders])
#         >>> asks.depth_at_price(82)
#         11.0
#         """
#
#         idx = self.bisect_idx_depth(v=price, values=self.get_attrib("price"), reverse=self.reverse)
#         values = tuple(self.get_attrib(depth_type))
#         if self.reverse:
#             values = reversed(values)
#         if idx == 0:
#             idx = 1
#         return sum(islice(values, 0, idx))
#
#     def depth_for_volume(self, value, depth_type="volume"):
#         """
#         Find the levels in this ladder required to fill a certain volume/exposure
#
#         :param value: volume to be filled
#         :param depth_type: {'volume', 'exposure'}
#         :return:
#         >>> orders = [(100, 6), (90, 3), (85, 15), (80, 10), (70, 1)]
#
#         >>> bids = Ladder([Order(price, volume, BID) for price, volume in orders], side=BID)
#         >>> bids.depth_for_volume(15)
#         [<Order(price=100, side=OrderSide.BID, volume=6)>, <Order(price=90, side=OrderSide.BID, volume=3)>, <Order(price=85, side=OrderSide.BID, volume=6)>]
#
#         >>> asks = Ladder([Order(price, volume, ASK) for price, volume in orders], side=ASK)
#         >>> asks.depth_for_volume(15)
#         [<Order(price=70, side=OrderSide.ASK, volume=1)>, <Order(price=80, side=OrderSide.ASK, volume=10)>, <Order(price=85, side=OrderSide.ASK, volume=4)>]
#         """
#         depth = tuple(self.cumulative(depth_type))
#         levels = self.levels
#         idx = self.bisect_idx_depth(v=value, values=depth, reverse=self.reverse)
#         if self.reverse:
#             idx = len(depth) - idx
#             levels = tuple(reversed(levels))
#         orders = sum(map(attrgetter("orders"), levels[: idx + 1]), [])
#         orders = [copy.copy(order) for order in orders]
#
#         if len(orders) == 0:
#             return ()
#         if len(orders) == 1:  # We are totally filled within the first order, just take our value
#             remaining_volume = value
#         else:  # We have multiple orders, but we won't necessarily take the full volume on the last order
#             remaining_volume = value - depth[idx - 1]
#         if depth_type == "exposure":  # Can't set a value for exposure, need to adjust via volume
#             remaining_volume = remaining_volume / orders[-1].price
#         orders[-1] = orders[-1].replace(volume=remaining_volume)
#         return orders
#
#     def exposure_fill_price(self, exposure):
#         """
#         Returns the average price that a certain exposure order would be filled at
#
#         >>> l = Ladder([Order(100, 1, BID), Order(50, 2, BID), Order(30, 10, BID)], side=BID)
#         >>> l.exposure_fill_price(200)
#         75.0
#         >>> l = Ladder([Order(100, 1, BID), Order(50, 2, BID), Order(30, 10, BID)], side=BID)
#         >>> l.exposure_fill_price(50)
#         100.0
#         """
#         orders = self.depth_for_volume(exposure, depth_type="exposure")
#         if not orders:
#             return
#         return sum(p * s / exposure for p, s in map(attrgetter("price", "exposure"), orders))
#
#     def volume_fill_price(self, volume):
#         """
#         Returns the average price that a certain volume order would be filled at
#
#         >>> l = Ladder([Order(100, 1, BID), Order(50, 2, BID), Order(30, 10, BID)], side=BID)
#         >>> l.volume_fill_price(2)
#         75.0
#         """
#         orders = self.depth_for_volume(volume, depth_type="volume")
#         return sum(p * s / volume for p, s in map(attrgetter("price", "volume"), orders))
#
#     def auction_match(self, other: "Ladder", on="volume"):
#         """
#         >>> l1 = Ladder(levels=[Order(103, 5, BID), Order(102, 10, BID), Order(100, 5, BID), Order(90, 5, BID)], side=BID)
#         >>> l2 = Ladder(levels=[Order(100, 10, ASK), Order(101, 10, ASK), Order(105, 5, ASK), Order(110, 5, ASK)], side=ASK)
#         >>> l1.auction_match(l2, on='volume')
#         (101.125, [<Order(price=103, side=OrderSide.BID, volume=5)>, <Order(price=102, side=OrderSide.BID, volume=10)>, <Order(price=100, side=OrderSide.BID, volume=5)>], [<Order(price=100, side=OrderSide.ASK, volume=10)>, <Order(price=101, side=OrderSide.ASK, volume=10)>])
#         """
#         default = [], []
#         assert self.side != other.side
#         if not (self.top_level and other.top_level):
#             return default
#         self_exposure = self.depth_at_price(other.top_level.price, depth_type=on)
#         other_exposure = other.depth_at_price(self.top_level.price, depth_type=on)
#         matched_exposure = min(self_exposure, other_exposure)
#
#         if matched_exposure == 0:
#             return default
#
#         traded_self = self.depth_for_volume(matched_exposure, depth_type=on)
#         traded_other = other.depth_for_volume(matched_exposure, depth_type=on)
#         return traded_self, traded_other
#
#     @staticmethod
#     def match_orders(traded_bids, traded_asks):
#         def match(bids, asks):
#             assert sum([o.volume for o in bids]) == sum([o.volume for o in asks])
#             bid_iter, ask_iter = iter(bids), iter(asks)
#             bid, ask = next(bid_iter), next(ask_iter)
#             while True:
#                 if bid.volume == ask.volume:
#                     yield (bid, ask)
#                     bid, ask = next(bid_iter), next(ask_iter)
#                 if bid.volume > ask.volume:
#                     yield (bid.copy(volume=ask.volume), ask)
#                     bid.volume -= ask.volume
#                     ask = next(ask_iter)
#                 if bid.volume < ask.volume:
#                     yield (bid, ask.copy(volume=bid.volume))
#                     ask.volume -= bid.volume
#                     bid = next(bid_iter)
#
#         matched = {}
#         matched_orders = list(match(bids=traded_bids, asks=traded_asks))
#         for bid, ask in matched_orders:
#             matched.setdefault(bid.order_id, list())
#             matched.setdefault(ask.order_id, list())
#             matched[bid.order_id].append(ask)
#             matched[ask.order_id].append(bid)
#         return matched
#
#     def check_for_trade(self, order):
#         """
#         Run an auction match on this order to see if any would trade
#         :param order:
#         :return: trade, order
#         """
#         assert order.side != self.side
#
#         ladder_trades, order_trades = self.auction_match(other=Ladder.from_orders([order]))
#         traded_volume = sum((t.volume for t in ladder_trades))
#
#         remaining_order = None
#         if order.volume != traded_volume:
#             remaining_order = Order(price=order.price, volume=order.volume - traded_volume, side=order.side)
#
#         return ladder_trades, remaining_order
#
#     def iter_orders(self):
#         for level in self.levels:
#             for order in level.iter_orders():
#                 yield order
#
#     def dict(self, **kwargs):
#         kwargs.update(dict(exclude={"price_levels", "order_id_prices"}))
#         return super().dict(**kwargs)
