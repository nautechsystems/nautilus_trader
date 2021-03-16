from nautilus_trader.common.uuid import UUIDFactory
from nautilus_trader.model.enums import OrderSide

uuid_factory = UUIDFactory()


class Order:
    def __init__(self, price, volume, side: OrderSide, id: str = None):
        self.price = price
        self.volume = volume
        self.side = side
        self.id = id or str(uuid_factory.generate())

    def update_price(self, price):
        self.price = price

    def update_volume(self, volume):
        self.volume = volume

    def __eq__(self, other):
        return self.id == other.id

    def __repr__(self):
        return f"Order({self.__dict__})"

    # TODO
    # @property
    # cpdef inline double exposure(self):
    #     return self.price * self.volume

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
