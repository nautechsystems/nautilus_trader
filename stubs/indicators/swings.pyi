from collections import deque
import pandas as pd
import datetime



class Swings(Indicator):
    """
    A swing indicator which calculates and stores various swing metrics.

    Parameters
    ----------
    period : int
        The rolling window period for the indicator (> 0).
    """

    period: int
    _high_inputs: deque
    _low_inputs: deque
    direction: int
    changed: bool
    high_datetime: datetime.datetime | None
    low_datetime: datetime.datetime | None
    high_price: float
    low_price: float
    length: float
    duration: int
    since_high: int
    since_low: int

    def __init__(self, period: int) -> None: ...
    def handle_bar(self, bar: Bar) -> None:
        """
        Update the indicator with the given bar.

        Parameters
        ----------
        bar : Bar
            The update bar.

        """
        ...
    def update_raw(self, high: float, low: float, timestamp: datetime) -> None:
        """
        Update the indicator with the given raw values.

        Parameters
        ----------
        high : double
            The high price.
        low : double
            The low price.
        timestamp : datetime
            The current timestamp.

        """
        ...
    def _reset(self) -> None: ...

