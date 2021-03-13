
from model.orderbook.order import Order

#TODO - Instead of a Level.orders being a list (python-land) could use structured arrays?
# https://docs.scipy.org/doc/numpy-1.13.0/user/basics.rec.html

cdef class Level:
    def __init__(self, orders = None):
        self.orders = orders or []
        self.order_index = {order.id: idx for idx, order in enumerate(orders)}

    cpdef void add(self):
        raise NotImplemented

    cpdef void update(self):
        raise NotImplemented

    cpdef void delete(self):
        raise NotImplemented


#TODO Cython subclassing is slow ??
cdef class L2Level(Level):
    """ A L2 Orderbook level. Only supports updating volume at this  """

    # TODO I don't think we need this? Should only be creating a level from Orderbook
    cpdef add(self, order):
        raise NotImplemented("Add should not be called for L2 Orderbook")

    cpdef update(self, float volume):
        """
        Update the volume on this level.

        Applicable for exchanges that send level updates only

        :param volume: New volume
        :return:
        """
        assert len(self.orders) == 1
        if volume == 0:
            self.orders = []
        else:
            self.orders[0].update_volume(volume=volume)

    # TODO I don't think we need this either
    cpdef delete(self, float volume):
        """
        Delete the `volume` from this level
        :param volume: Quantity of volume to delete
        :return:
        """
        # self.orders[0].volume = self.orders[0].volume - volume
        raise NotImplemented()


cdef class L3Level(Level):
    """
    An L3 Orderbook Level
    """
    def add(self, *, Order order):
        self.orders.append(order)
        self.order_index[order.id] = len(self.orders)

    def update(self, order_id: str, volume: float):
        """
        Update an order on this level.

        :param order_id: New order to update
        :param volume: New volume
        :return:
        """
        idx = self.order_index[order_id]
        self.orders[idx].update_volume(volume=volume)

    cdef delete(self, str order_id):
        order_idx = self.order_index.pop(order_id)
        return self.orders.pop(order_idx)
