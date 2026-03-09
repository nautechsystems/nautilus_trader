import re
from typing import Any

import pandas as pd

from nautilus_trader.model.orders import Order
from nautilus_trader.model.position import Position


class PortfolioStatistic:
    """
    The base class for all portfolio performance statistics.

    Notes
    -----
    The return value should be a JSON serializable primitive.

    """

    @classmethod
    def fully_qualified_name(cls) -> str:
        """
        Return the fully qualified name for the `PortfolioStatistic` class.

        Returns
        -------
        str

        References
        ----------
        https://www.python.org/dev/peps/pep-3155/

        """
        return cls.__module__ + ":" + cls.__qualname__

    @property
    def name(self) -> str:
        """
        Return the name for the statistic.

        Returns
        -------
        str

        """
        klass = type(self).__name__
        matches = re.finditer(".+?(?:(?<=[a-z])(?=[A-Z])|(?<=[A-Z])(?=[A-Z][a-z])|$)", klass)
        return " ".join([m.group(0) for m in matches])

    def calculate_from_returns(self, returns: pd.Series) -> Any | None:
        """
        Calculate the statistic value from the given raw returns.

        Parameters
        ----------
        returns : pd.Series
            The returns to use for the calculation.

        Returns
        -------
        Any or ``None``
            A JSON serializable primitive.

        """
        # Override in implementation

    def calculate_from_realized_pnls(self, realized_pnls: pd.Series) -> Any | None:
        """
        Calculate the statistic value from the given raw realized PnLs.

        Parameters
        ----------
        realized_pnls : pd.Series
            The raw PnLs for the calculation.

        Returns
        -------
        Any or ``None``
            A JSON serializable primitive.

        """
        # Override in implementation

    def calculate_from_orders(self, orders: list[Order]) -> Any | None:
        """
        Calculate the statistic value from the given orders.

        Parameters
        ----------
        orders : list[Order]
            The positions to use for the calculation.

        Returns
        -------
        Any or ``None``
            A JSON serializable primitive.

        """
        # Override in implementation

    def calculate_from_positions(self, positions: list[Position]) -> Any | None:
        """
        Calculate the statistic value from the given positions.

        Parameters
        ----------
        positions : list[Position]
            The positions to use for the calculation.

        Returns
        -------
        Any or ``None``
            A JSON serializable primitive.

        """
        # Override in implementation

    def _check_valid_returns(self, returns: pd.Series) -> bool:
        return not (returns is None or returns.empty or returns.isna().all())

    def _downsample_to_daily_bins(self, returns: pd.Series) -> pd.Series:
        return returns.dropna().resample("1D").sum()
