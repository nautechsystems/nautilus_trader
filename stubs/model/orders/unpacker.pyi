from typing import Any

from stubs.model.events.order import OrderInitialized
from stubs.model.orders.base import Order

class OrderUnpacker:

    @staticmethod
    def unpack(values: dict[str, Any]) -> Order: ...
    @staticmethod
    def from_init(init: OrderInitialized) -> Order: ...
