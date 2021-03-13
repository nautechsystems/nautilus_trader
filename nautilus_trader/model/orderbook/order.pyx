cdef class Order:
    def __init__(self, float price, float volume, OrderSide side):
        self.price = price
        self.volume = volume
        self.side = side

    # @property
    # cdef float exposure(self):
    #     return self.price * self.volume
    #
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
