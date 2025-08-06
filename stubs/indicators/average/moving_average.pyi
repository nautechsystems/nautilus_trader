from enum import Enum
from enum import unique
from typing import ClassVar

from nautilus_trader.model.enums import PriceType
from stubs.indicators.base.indicator import Indicator

@unique
class MovingAverageType(Enum):

    SIMPLE: ClassVar[int] = 0
    EXPONENTIAL: ClassVar[int] = 1
    WEIGHTED: ClassVar[int] = 2
    HULL: ClassVar[int] = 3
    ADAPTIVE: ClassVar[int] = 4
    WILDER: ClassVar[int] = 5
    DOUBLE_EXPONENTIAL: ClassVar[int] = 6
    VARIABLE_INDEX_DYNAMIC: ClassVar[int] = 7


class MovingAverage(Indicator):

    period: int
    price_type: PriceType
    value: float
    count: int

    def __init__(
        self,
        period: int,
        params: list,
        price_type: PriceType,
    ) -> None: ...
    def update_raw(self, value: float) -> None: ...
    def _increment_count(self) -> None: ...
    def _reset(self) -> None: ...
    def _reset_ma(self) -> None: ...
