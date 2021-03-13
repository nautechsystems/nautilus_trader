from nautilus_trader.common.uuid import UUIDFactory

uuid_factory = UUIDFactory()

cdef class Order:
    def __init__(self, float price, float volume, OrderSide side, str id = None):
        self.price = price
        self.volume = volume
        self.side = side
        self.id = id or str(uuid_factory.generate())

    cpdef public void update_price(self, float price):
        self.price = price

    cpdef public void update_volume(self, float volume):
        self.volume = volume

    # @property
    # def side_sign(self):
    #     if self.side == OrderSide.BUY:
    #         return self.volume
    #     else:
    #         return -1.0 * self.volume
    #
    # @property
    # def signed_volume(self):
    #     return self.volume * self.side_sign
