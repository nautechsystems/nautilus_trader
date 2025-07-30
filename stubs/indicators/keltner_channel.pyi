from nautilus_trader.indicators.average.ma_factory import MovingAverageType
from nautilus_trader.core.nautilus_pyo3 import Bar
from nautilus_trader.core.nautilus_pyo3 import MovingAverageType
from nautilus_trader.core.nautilus_pyo3 import Indicator


class KeltnerChannel(Indicator):
    """
    The Keltner channel is a volatility based envelope set above and below a
    central moving average. Traditionally the middle band is an EMA based on the
    typical price (high + low + close) / 3, the upper band is the middle band
    plus the ATR. The lower band is the middle band minus the ATR.

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
    def upper(self) -> float: ...
    @property
    def middle(self) -> float: ...
    @property
    def lower(self) -> float: ...
    def update_raw(
        self,
        high: float,
        low: float,
        close: float,
    ) -> None: ...
    def handle_bar(self, bar: Bar) -> None: ...
    def reset(self) -> None: ...