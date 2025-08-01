from nautilus_trader.backtest.config import FXRolloverInterestConfig
from nautilus_trader.backtest.config import SimulationModuleConfig
from nautilus_trader.core.data import Data
from nautilus_trader.core.nautilus_pyo3 import Actor
from nautilus_trader.core.nautilus_pyo3 import AssetClass
from nautilus_trader.core.nautilus_pyo3 import InstrumentId
from nautilus_trader.core.nautilus_pyo3 import Money
from nautilus_trader.core.nautilus_pyo3 import Price
from nautilus_trader.core.nautilus_pyo3 import PriceType
from nautilus_trader.core.nautilus_pyo3 import Logger
from nautilus_trader.core.nautilus_pyo3 import OrderBook
from nautilus_trader.core.nautilus_pyo3 import Position
from nautilus_trader.core.nautilus_pyo3 import Currency
from nautilus_trader.backtest.exchange import SimulatedExchange
import pandas as pd
import datetime as dt
import pytz


_TZ_US_EAST: pytz.tzinfo.DstTzInfo


class SimulationModule(Actor):
    """
    The base class for all simulation modules.

    Warnings
    --------
    This class should not be used directly, but through a concrete subclass.
    """

    def __init__(self, config: SimulationModuleConfig): ...
    def __repr__(self) -> str: ...
    def register_venue(self, exchange: SimulatedExchange) -> None:
        """
        Register the given simulated exchange with the module.

        Parameters
        ----------
        exchange : SimulatedExchange
            The exchange to register.

        """
        ...
    def pre_process(self, data: Data) -> None:
        """Abstract method `pre_process` (implement in subclass)."""
        ...
    def process(self, ts_now: int) -> None:
        """Abstract method (implement in subclass)."""
        ...
    def log_diagnostics(self, logger: Logger) -> None:
        """Abstract method (implement in subclass)."""
        ...
    def reset(self) -> None:
        """Abstract method (implement in subclass)."""
        ...


class FXRolloverInterestModule(SimulationModule):
    """
    Provides an FX rollover interest simulation module.

    Parameters
    ----------
    config  : FXRolloverInterestConfig
    """

    def __init__(self, config: FXRolloverInterestConfig): ...
    def process(self, ts_now: int) -> None:
        """
        Process the given tick through the module.

        Parameters
        ----------
        ts_now : uint64_t
            The current UNIX timestamp (nanoseconds) in the simulated exchange.

        """
        ...
    def log_diagnostics(self, logger: Logger) -> None:
        """
        Log diagnostics out to the `BacktestEngine` logger.

        Parameters
        ----------
        logger : Logger
            The logger to log to.

        """
        ...
    def reset(self) -> None: ...

