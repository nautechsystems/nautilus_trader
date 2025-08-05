from datetime import datetime

from stubs.indicators.base.indicator import Indicator
from stubs.model.data import Bar

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
    def _reset(self) -> None: ...

