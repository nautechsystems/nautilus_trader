from ib_insync import LimitOrder as IBLimitOrder
from ib_insync import MarketOrder as IBMarketOrder
from ib_insync import Order as IBOrder

from nautilus_trader.model.c_enums.order_side import OrderSideParser
from nautilus_trader.model.orders.base import Order as NautilusOrder
from nautilus_trader.model.orders.limit import LimitOrder as NautilusLimitOrder
from nautilus_trader.model.orders.market import MarketOrder as NautilusMarketOrder


def nautilus_order_to_ib_order(order: NautilusOrder) -> IBOrder:
    if isinstance(order, NautilusMarketOrder):
        return IBMarketOrder(
            action=OrderSideParser.to_str_py(order.side),
            totalQuantity=order.quantity.as_double(),
        )
    elif isinstance(order, NautilusLimitOrder):
        # TODO - Time in force, etc
        return IBLimitOrder(
            action=OrderSideParser.to_str_py(order.side),
            lmtPrice=order.price.as_double(),
            totalQuantity=order.quantity.as_double(),
        )
    else:
        raise NotImplementedError(f"IB order type not implemented {type(order)} for {order}")
