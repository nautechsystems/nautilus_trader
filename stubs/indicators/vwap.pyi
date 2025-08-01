import datetime
import pandas as pd
from nautilus_trader.model.data import Bar
from nautilus_trader.indicators.base.indicator import Indicator


class VolumeWeightedAveragePrice(Indicator):
    """
    An indicator which calculates the volume weighted average price for the day.
    """

    value: float

    def __init__(self) -> None: ...
    def handle_bar(self, bar: Bar) -> None:
        """
        Update the indicator with the given bar.

        Parameters
        ----------
        bar : Bar
            The update bar.

        """
        ...
    def update_raw(self, price: float, volume: float, timestamp: datetime) -> None:
        """
        Update the indicator with the given raw values.

        Parameters
        ----------
        price : double
            The update price.
        volume : double
            The update volume.
        timestamp : datetime
            The current timestamp.

        """
        ...
    def _reset(self) -> None: ...

