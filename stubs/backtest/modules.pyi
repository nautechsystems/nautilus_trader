import pytz

from nautilus_trader.backtest.config import FXRolloverInterestConfig
from nautilus_trader.backtest.config import SimulationModuleConfig
from stubs.backtest.exchange import SimulatedExchange
from stubs.common.actor import Actor
from stubs.common.component import Logger
from stubs.core.data import Data

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

