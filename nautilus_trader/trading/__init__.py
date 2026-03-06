"""
The `trading` subpackage groups all trading domain specific components and tooling.

This is a top-level package where the majority of users will interface with the
framework. Custom trading strategies can be implemented by inheriting from the
`Strategy` base class.

"""

from nautilus_trader.trading.controller import Controller
from nautilus_trader.trading.strategy import Strategy
from nautilus_trader.trading.trader import Trader


__all__ = [
    "Controller",
    "Strategy",
    "Trader",
]
