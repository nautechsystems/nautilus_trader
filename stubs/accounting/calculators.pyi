import datetime as dt
from decimal import Decimal
from typing import Any

import pandas as pd

from nautilus_trader.core.nautilus_pyo3 import InstrumentId

class RolloverInterestCalculator:
    """
    Provides rollover interest rate calculations.

    If rate_data_csv_path is empty then will default to the included short-term
    interest rate data csv (data since 1956).

    Parameters
    ----------
    data : str
        The short term interest rate data.
    """

    def __init__(self, data: pd.DataFrame): ...
    def get_rate_data(self) -> pd.DataFrame:
        """
        Return the short-term interest rate dataframe.

        Returns
        -------
        pd.DataFrame

        """
        ...
    def calc_overnight_rate(self, instrument_id: InstrumentId, date: dt.date) -> Decimal:
        """
        Return the rollover interest rate between the given base currency and quote currency.

        Parameters
        ----------
        instrument_id : InstrumentId
            The forex instrument ID for the calculation.
        date : date
            The date for the overnight rate.

        Returns
        -------
        Decimal

        Raises
        ------
        ValueError
            If `instrument_id.symbol` length is not in range [6, 7].

        Notes
        -----
        1% = 0.01 bp

        """
        ...