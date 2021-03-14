import logging
from bisect import bisect
from typing import List

from nautilus_trader.model.orderbook.level cimport Level
from nautilus_trader.model.orderbook.order cimport Order

logger = logging.getLogger(__name__)

cdef class Ladder:
    def __init__(self, bint reverse):
        self.levels = [] # type: List[Level]
        self.reverse = reverse
        self.price_levels = dict()
        self.order_id_prices = dict()

    cpdef void add(self, Order order):
        # Level exists, add new order
        if order.price in self.prices:
            idx = tuple(self.prices).index(order.price)
            self.levels[idx].add(order=order)
        # New price, create Level
        else:
            level = Level(orders=[order])
            self.levels.insert(bisect(self.levels, level), level)
            self.price_levels[order.price] = level
        self.order_id_prices[order.id] = order.price

    cpdef void update(self, order: Order):
        if order.id not in self.order_id_prices:
            self.add(order=order)
        # Find the existing order
        price = self.order_id_prices[order.id]
        level = self.price_levels[price]
        existing_order = level._get_order(order.id)
        if order.price == existing_order.price:
            # This update contains a volume update
            level.update(order=order)
        else:
            # New price for this order, delete and insert
            self.delete(order=order)
            self.add(order=order)

    cpdef void delete(self, Order order):
        price_idx = self.price_levels[order.price]
        level = self.levels[price_idx] # type: Level
        level.delete(order=order)
        del self.order_id_prices[order.id]
        if not level.orders:
            del self.levels[price_idx]
            del self.price_levels[order.price]

    cpdef depth(self, int n=1):
        if not self.levels:
            return []
        n = n or len(self.levels)
        return list(reversed(self.levels[-n:])) if self.reverse else self.levels[:n]

    @property
    def prices(self):
        return [level.orders[0].price for level in self.levels]

    @property
    def volumes(self):
        return [level.price for level in self.levels]

    @property
    def exposures(self):
        return [level.price for level in self.levels]

    @property
    def top(self):
        top = self.depth(1)
        if top:
            return top[0]
    #
    # def iter_orders(self):
    #     for level in self.levels:
    #         for order in level.iter_orders():
    #             yield order

# #TODO Cython subclassing is slow ??
# cdef class L2Ladder(LadderMixin):
#     cpdef add(self, level: L2Level):
#         order = level.orders[0]
#         if order.price in self.prices:
#             idx = tuple(self.prices).index(order.price)
#             self.levels[idx].add(order)
#         else:
#             level = L2Level(orders=[order])
#             self.levels.insert(bisect(self.levels, level), level)
#             self.price_levels[order.price] = level
#         self.order_id_prices[order.id] = order.price
#
#     cpdef update(self, level: L2Level):
#         """
#         Update a level.
#
#         :param level: Level to update
#         :return:
#         """
#         if level.price not in self.prices:
#             self.add(level=level)
#         elif level.volume == 0:
#             self.delete(price=level.orders[0].price)
#         else:
#             price_idx = tuple(self.prices).index(level.price)
#             self.levels[price_idx].update(volume=level.volume)
#
#     cpdef delete(self, price: float):
#         prices = tuple(self.prices)
#         if price not in prices:
#             logger.warning(f"Price {price} not in prices: {prices}")
#             return
#         price_idx = tuple(self.prices).index(price)
#         return self.levels.pop(price_idx)

# cdef class L3Ladder(LadderMixin):
#     cpdef add(self):
#         raise NotImplemented
#
#     cpdef update(self, order: Order, order_update_drops_priority: bool = False):
#         assert not order_update_drops_priority, "order_update_drops_priority not implemented yet"
#         if order.order_id not in self.order_id_prices:
#             return self.add(order=order)
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
#                 self.delete(order_id=order.id)
#                 self.insert(order=order)

# cpdef delete(self, str order_id):
#     price_idx = tuple(self.prices).index(order.price)
#     deleted_orders = self.levels[price_idx].delete(order=order)
#     for del_order in deleted_orders:
#         del self.order_id_prices[del_order.order_id]
#     self._delete_level_by_price(price=order.price, only_if_empty=True)
#     return deleted_orders
