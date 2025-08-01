from nautilus_trader.indicators.average.ma_factory import MovingAverageType
from nautilus_trader.core.nautilus_pyo3 import PriceType
from nautilus_trader.indicators.average.moving_average import MovingAverage
from nautilus_trader.core.nautilus_pyo3 import ChandeMomentumOscillator
from nautilus_trader.core.nautilus_pyo3 import Bar
from nautilus_trader.core.nautilus_pyo3 import QuoteTick
from nautilus_trader.core.nautilus_pyo3 import TradeTick
from nautilus_trader.core.nautilus_pyo3 import Price


class VariableIndexDynamicAverage(MovingAverage):
    """
    Variable Index Dynamic Average (VIDYA) was developed by Tushar Chande. It is
    similar to an Exponential Moving Average, but it has a dynamically adjusted
    lookback period dependent on relative price volatility as measured by Chande
    Momentum Oscillator (CMO). When volatility is high, VIDYA reacts faster to
    price changes. It is often used as moving average or trend identifier.

    Parameters
    ----------
    period : int
        The rolling window period for the indicator (> 0).
    price_type : PriceType
        The specified price type for extracting values from quotes.
    cmo_ma_type : int
        The moving average type for CMO indicator.

    Raises
    ------
    ValueError
        If `period` is not positive (> 0).
        If `cmo_ma_type` is ``VARIABLE_INDEX_DYNAMIC``.
    """

    cmo: ChandeMomentumOscillator
    cmo_pct: float
    alpha: float
    value: float

    def __init__(
        self,
        period: int,
        price_type: PriceType = PriceType.LAST,
        cmo_ma_type: MovingAverageType = MovingAverageType.SIMPLE,
    ) -> None: ...
    def handle_quote_tick(self, tick: QuoteTick) -> None:
        """
        Update the indicator with the given quote tick.

        Parameters
        ----------
        tick : QuoteTick
            The update tick to handle.

        """
        ...
    def handle_trade_tick(self, tick: TradeTick) -> None:
        """
        Update the indicator with the given trade tick.

        Parameters
        ----------
        tick : TradeTick
            The update tick to handle.

        """
        ...
    def handle_bar(self, bar: Bar) -> None:
        """
        Update the indicator with the given bar.

        Parameters
        ----------
        bar : Bar
            The update bar to handle.

        """
        ...
    def update_raw(self, value: float) -> None:
        """
        Update the indicator with the given raw value.

        Parameters
        ----------
        value : double
            The update value.

        """
        ...
