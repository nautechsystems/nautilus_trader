import logging
from bisect import bisect
from typing import List

from nautilus_trader.model.orderbook.level import Level
from nautilus_trader.model.orderbook.order import Order

logger = logging.getLogger(__name__)

cdef class Ladder:
    cdef list levels
    cdef boolean reverse
    cdef dict price_levels
    cdef dict order_id_prices

    cpdef add(self, order: Order):
        if order.price in self.prices:
            idx = tuple(self.prices).index(order.price)
            self.levels[idx].insert(order)
        else:
            level = Level(orders=[order])
            self.levels.insert(bisect(self.levels, level), level)
            self.price_levels[level.price] = level
        self.order_id_prices[order.order_id] = order.price

    cpdef update(self, ):
        raise NotImplemented

    cpdef delete(self):
        raise NotImplemented

    @property
    def prices(self):
        return [level.price for level in self.levels]

    @property
    def volumes(self):
        return [level.price for level in self.levels]

    @property
    def exposures(self):
        return [level.price for level in self.levels]

    @property
    def top_level(self) -> Level:
        top = self.top(1)
        if top:
            return top[0]

    def top(self, n=1) -> List[Level]:
        if not self.levels:
            return []
        n = n or len(self.levels)
        return list(reversed(self.levels[-n:])) if self.reverse else self.levels[:n]

    def iter_orders(self):
        for level in self.levels:
            for order in level.iter_orders():
                yield order


#TODO Cython subclassing is slow ??
cdef class L2Ladder(Ladder):

    def update(self, level: Level):
        """
        Update a level.

        :param level: Level to update
        :param order: Order to update
        :param order_update_drops_priority: Whether an order update causes an order to lose its priority
        :return:
        """
        if level.price not in self.prices:
            self._insert_level(level=level)
        elif level.volume == 0:
            self._delete_level(level=level)
        else:
            price_idx = tuple(self.prices).index(level.price)
            self.levels[price_idx].update(volume=level.volume)

    def delete(self, price: float):
        prices = tuple(self.prices)
        if price not in prices:
            logger.warning(f"Price {price} not in prices: {prices}")
            return
        price_idx = tuple(self.prices).index(price)
        return self.levels.pop(price_idx)


cdef class L3Ladder(Ladder):

    def update(self, *_, order: Order, order_update_drops_priority: bool = False):
        assert not order_update_drops_priority, "order_update_drops_priority not implemented yet"
        if order.order_id not in self.order_id_prices:
            return self.insert(order=order)
        # Find the existing order
        price = self.order_id_prices[order.order_id]
        level = self.price_levels[price]
        existing_order = level.order_id_orders[order.order_id]
        if order.price == existing_order.price:
            # This update contains a volume update
            level.update(order_id=order.order_id, volume=order.volume)
        else:
            # New price for this order, delete and insert
            if self.exchange_order_ids:
                self.delete(order_id=order.order_id)
                self.insert(order=order)
            else:
                self.delete(order=order)
                self.insert(order=order)

    cpdef delete(self, str order_id):
        price_idx = tuple(self.prices).index(order.price)
        deleted_orders = self.levels[price_idx].delete(order=order)
        for del_order in deleted_orders:
            del self.order_id_prices[del_order.order_id]
        self._delete_level_by_price(price=order.price, only_if_empty=True)
        return deleted_orders
