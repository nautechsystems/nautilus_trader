from nautilus_trader.core.nautilus_pyo3 import Bar
from nautilus_trader.core.nautilus_pyo3 import KeltnerChannel
from nautilus_trader.core.nautilus_pyo3 import MovingAverageType
from stubs.indicators.base.indicator import Indicator

class KeltnerPosition(Indicator):
    """
    An indicator which calculates the relative position of the given price
    within a defined Keltner channel. This provides a measure of the relative
    'extension' of a market from the mean, as a multiple of volatility.

    Parameters
    ----------
    period : int
        The rolling window period for the indicator (> 0).
    k_multiplier : double
        The multiplier for the ATR (> 0).
    ma_type : MovingAverageType
        The moving average type for the middle band (cannot be None).
    ma_type_atr : MovingAverageType
        The moving average type for the internal ATR (cannot be None).
    use_previous : bool
        The boolean flag indicating whether previous price values should be used.
    atr_floor : double
        The ATR floor (minimum) output value for the indicator (>= 0).
    """

    period: int
    k_multiplier: float
    value: float
    _kc: KeltnerChannel

    def __init__(
        self,
        period: int,
        k_multiplier: float,
        ma_type: MovingAverageType = MovingAverageType.EXPONENTIAL,
        ma_type_atr: MovingAverageType = MovingAverageType.SIMPLE,
        use_previous: bool = True,
        atr_floor: float = 0,
    ) -> None: ...
    @property
    def name(self) -> str: ...
    @property
    def period(self) -> int: ...
    @property
    def k_multiplier(self) -> float: ...
    @property
    def use_previous(self) -> bool: ...
    @property
    def atr_floor(self) -> float: ...
    @property
    def initialized(self) -> bool: ...
    @property
    def has_inputs(self) -> bool: ...
    @property
    def value(self) -> float: ...
    def handle_bar(self, bar: Bar) -> None: ...
    def update_raw(
        self,
        high: float,
        low: float,
        close: float,
    ) -> None: ...
    def reset(self) -> None: ...