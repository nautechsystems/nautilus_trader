from nautilus_trader.core.model import TriggerType
from nautilus_trader.model.events.order import OrderInitialized
from nautilus_trader.model.objects import Price, Quantity
from nautilus_trader.model.orders.base import Order

class LimitIfTouchedOrder(Order):
    price: Price
    trigger_price: Price
    trigger_type: TriggerType
    expire_time_ns: int
    display_qty: Quantity
    is_triggered: bool
    ts_triggered: int

    @staticmethod
    def create(init: OrderInitialized) -> "LimitIfTouchedOrder": ...
