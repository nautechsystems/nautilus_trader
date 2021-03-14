from cpython.object cimport  Py_LT, Py_EQ, Py_GT, Py_LE, Py_NE, Py_GE
from nautilus_trader.model.orderbook.order cimport Order

#TODO - Instead of a Level.orders being a list (python-land) could use structured arrays?
# https://docs.scipy.org/doc/numpy-1.13.0/user/basics.rec.html

cdef class Level:
    """ A Orderbook level; A price level on one side of the Orderbook with one or more individual Orders"""
    def __init__(self, orders=None):
        self.orders = []
        self.order_index = dict()
        for order in (orders or []):
            self.add(order)

    cpdef void add(self, Order order):
        """
        Add an order to this level.
        :param order: New order
        :return:
        """
        self._check_price(order=order)
        self.order_index[order.id] = len(self.orders)
        self.orders.append(order)

    cpdef void update(self, Order order):
        """
        Update an order on this level.
        :param order: New order
        :return:
        """
        # TODO - Is creating an Order object at the top level every time we want to update something too costly?
        #  Should we just use an order_id and price/volume here?
        self._check_price(order=order)
        if order.volume == 0:
            self.delete(order=order)
        else:
            existing = self._get_order(order.id)
            existing.update_volume(volume=order.volume)

    cpdef void delete(self, Order order):
        """
        Delete an Order from this level
        :param order: Quantity of volume to delete
        :return:
        """
        idx = self.order_index[order.id]
        del self.orders[idx]

    cpdef _get_order(self, str order_id):
        return self.orders[self.order_index[order_id]]

    cpdef _check_price(self, Order order):
        if not self.orders:
            return True
        err = "Order passed to `update` has wrong price! Should be handled in Ladder"
        assert order.price == self.orders[0].price, err


    cpdef public double volume(self):
        return sum([order.volume for order in self.orders])

    cpdef public double price(self):
        return self.orders[0].price

    def __richcmp__(self, other, int op):
        if op == Py_LT:
            return self.price() < other.price()
        elif op == Py_EQ:
            return self.price() == other.price()
        elif op == Py_GT:
            return self.price() > other.price()
        elif op == Py_LE:
            return self.price() <= other.price()
        elif op == Py_NE:
            return self.price() != other.price()
        elif op == Py_GE:
            return self.price() >= other.price()
        else:
            raise KeyError
