from enum import Enum
from enum import unique
from typing import ClassVar

from nautilus_trader.model.enums import PriceType
from stubs.indicators.base.indicator import Indicator

@unique
class MovingAverageType(Enum):
    """
    Represents the type of moving average.
    """

    SIMPLE: ClassVar[int] = 0
    EXPONENTIAL: ClassVar[int] = 1
    WEIGHTED: ClassVar[int] = 2
    HULL: ClassVar[int] = 3
    ADAPTIVE: ClassVar[int] = 4
    WILDER: ClassVar[int] = 5
    DOUBLE_EXPONENTIAL: ClassVar[int] = 6
    VARIABLE_INDEX_DYNAMIC: ClassVar[int] = 7


class MovingAverage(Indicator):
    """
    The base class for all moving average type indicators.

    Parameters
    ----------
    period : int
        The rolling window period for the indicator (> 0).
    params : list
        The initialization parameters for the indicator.
    price_type : PriceType, optional
        The specified price type for extracting values from quotes.

    Warnings
    --------
    This class should not be used directly, but through a concrete subclass.
    """

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
    def update_raw(self, value: float) -> None:
        """
        Update the indicator with the given raw value.

        Parameters
        ----------
        value : double
            The update value.

        """
        ...
    def _increment_count(self) -> None: ...
    def _reset(self) -> None: ...
    def _reset_ma(self) -> None: ...
