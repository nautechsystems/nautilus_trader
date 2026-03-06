from py_clob_client.clob_types import OrderType

from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.enums import time_in_force_to_str


def convert_tif_to_polymarket_order_type(time_in_force) -> str:
    match time_in_force:
        case TimeInForce.GTC:
            return OrderType.GTC
        case TimeInForce.GTD:
            return OrderType.GTD
        case TimeInForce.FOK:
            return OrderType.FOK
        case TimeInForce.IOC:
            return OrderType.FAK
        case _:
            time_in_force_str = time_in_force_to_str(time_in_force)
            raise ValueError(f"invalid `TimeInForce` for conversion, was {time_in_force_str}")
