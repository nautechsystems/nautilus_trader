from datetime import datetime

from stubs.indicators.base.indicator import Indicator
from stubs.model.data import Bar

class VolumeWeightedAveragePrice(Indicator):

    value: float

    def __init__(self) -> None: ...
    def handle_bar(self, bar: Bar) -> None: ...
    def update_raw(self, price: float, volume: float, timestamp: datetime) -> None: ...
    def _reset(self) -> None: ...

