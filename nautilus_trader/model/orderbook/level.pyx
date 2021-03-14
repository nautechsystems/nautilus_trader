
from nautilus_trader.model.orderbook.order cimport Order

#TODO - Instead of a Level.orders being a list (python-land) could use structured arrays?
# https://docs.scipy.org/doc/numpy-1.13.0/user/basics.rec.html

cdef class Level:
    """ A Orderbook level; A price level on one side of the Orderbook with one or more individual Orders"""
    def __init__(self, orders = None):
        self.orders = orders or []
        self.order_index = {order.id: idx for idx, order in enumerate(orders)}

    cpdef void add(self, Order order):
        """
        Add an order to this level.
        :param order: New order
        :return:
        """
        self._check_price(order=order)
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
            existing = self.orders[self.order_index[order.id]]
            existing.update_volume(volume=order.volume)

    cpdef void delete(self, Order order):
        """
        Delete an Order from this level
        :param order: Quantity of volume to delete
        :return:
        """
        idx = self.order_index[order.id]
        del self.orders[idx]

    def _check_price(self, Order order):
        err = "Order passed to `update` has wrong price! Should be handled in Ladder"
        assert order.price == self.orders[0].price, err

    cpdef public double volume(self):
        return sum([order.volume for order in self.orders])

    cpdef public double price(self):
        return self.orders[0].price
