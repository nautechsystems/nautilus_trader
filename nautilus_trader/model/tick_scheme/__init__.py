"""
Defines a scheme for modeling the tick space for various instruments.
"""

from nautilus_trader.model.tick_scheme.base import get_tick_scheme
from nautilus_trader.model.tick_scheme.base import register_tick_scheme
from nautilus_trader.model.tick_scheme.implementations.fixed import FOREX_3DECIMAL_TICK_SCHEME
from nautilus_trader.model.tick_scheme.implementations.fixed import FOREX_5DECIMAL_TICK_SCHEME
from nautilus_trader.model.tick_scheme.implementations.fixed import FixedTickScheme
from nautilus_trader.model.tick_scheme.implementations.tiered import TOPIX100_TICK_SCHEME
from nautilus_trader.model.tick_scheme.implementations.tiered import TieredTickScheme


register_tick_scheme(TOPIX100_TICK_SCHEME)
register_tick_scheme(FOREX_3DECIMAL_TICK_SCHEME)
register_tick_scheme(FOREX_5DECIMAL_TICK_SCHEME)

__all__ = [
    "FOREX_3DECIMAL_TICK_SCHEME",
    "FOREX_5DECIMAL_TICK_SCHEME",
    "TOPIX100_TICK_SCHEME",
    "FixedTickScheme",
    "TieredTickScheme",
    "get_tick_scheme",
    "register_tick_scheme",
]
