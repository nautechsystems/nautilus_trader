from nautilus_trader.core.model import TriggerType
from nautilus_trader.model.events.order import OrderInitialized
from nautilus_trader.model.objects import Price
from nautilus_trader.model.orders.base import Order

class MarketIfTouchedOrder(Order):
    trigger_price: Price
    """The order trigger price (STOP)."""

    trigger_type: TriggerType
    """The trigger type for the order."""

    expire_time_ns: int
    """The order expiration (UNIX epoch nanoseconds), zero for no expiration."""

    @classmethod
    def create(cls, init: OrderInitialized) -> MarketIfTouchedOrder: ...
