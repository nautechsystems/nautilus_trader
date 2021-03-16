import functools


# from cpython.object cimport  Py_LT, Py_EQ, Py_GT, Py_LE, Py_NE, Py_GE

# TODO - Instead of a Level.orders being a list (python-land) could use structured arrays?
# https://docs.scipy.org/doc/numpy-1.13.0/user/basics.rec.html
import logging


@functools.total_ordering
class Level:
    """ A Orderbook level; A price level on one side of the Orderbook with one or more individual Orders"""

    def __init__(self, orders=None):
        self.orders = []
        for order in (orders or []):
            self.add(order)

    def add(self, order):
        """
        Add an order to this level.
        :param order: New order
        :return:
        """
        self._check_price(order=order)
        self.orders.append(order)

    def update(self, order):
        """
        Update an order on this level.
        :param order: New order
        :return:
        """
        self._check_price(order=order)
        if order.volume == 0:
            self.delete(order=order)
        else:
            existing = self.orders[self.orders.index(order)]
            if existing is None:
                logging.warning(f"Tried to update unknown order: {order}")
                return
            existing.update_volume(volume=order.volume)

    def delete(self, order):
        """
        Delete an Order from this level
        :param order: Quantity of volume to delete
        :return:
        """
        self.orders.remove(order)

    def _check_price(self, order):
        if not self.orders:
            return True
        err = "Order passed to `update` has wrong price! Should be handled in Ladder"
        assert order.price == self.orders[0].price, err

    @property
    def volume(self):
        return sum([order.volume for order in self.orders])

    @property
    def price(self):
        return self.orders[0].price

    def __eq__(self, other):
        return self.price == other.price

    def __le__(self, other):
        return self.price <= other.price

    def __repr__(self):
        return f"Level(price={self.price}, orders={self.orders[:5]})"

    # def __richcmp__(self, other, op):
    #     if op == Py_LT:
    #         return self.price() < other.price()
    #     elif op == Py_EQ:
    #         return self.price() == other.price()
    #     elif op == Py_GT:
    #         return self.price() > other.price()
    #     elif op == Py_LE:
    #         return self.price() <= other.price()
    #     elif op == Py_NE:
    #         return self.price() != other.price()
    #     elif op == Py_GE:
    #         return self.price() >= other.price()
    #     else:
    #         raise KeyError
