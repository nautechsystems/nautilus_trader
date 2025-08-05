from nautilus_trader.model.enums import PriceType
from stubs.indicators.average.moving_average import MovingAverage
from stubs.model.data import Bar
from stubs.model.data import QuoteTick
from stubs.model.data import TradeTick

class AdaptiveMovingAverage(MovingAverage):
    """
    An indicator which calculates an adaptive moving average (AMA) across a
    rolling window. Developed by Perry Kaufman, the AMA is a moving average
    designed to account for market noise and volatility. The AMA will closely
    follow prices when the price swings are relatively small and the noise is
    low. The AMA will increase lag when the price swings increase.

    Parameters
    ----------
    period_er : int
        The period for the internal `EfficiencyRatio` indicator (> 0).
    period_alpha_fast : int
        The period for the fast smoothing constant (> 0).
    period_alpha_slow : int
        The period for the slow smoothing constant (> 0 < alpha_fast).
    price_type : PriceType
        The specified price type for extracting values from quotes.
    """

    period_er: int
    period_alpha_fast: int
    period_alpha_slow: int
    alpha_fast: float
    alpha_slow: float
    alpha_diff: float
    value: float
    def __init__(
        self,
        period_er: int,
        period_alpha_fast: int,
        period_alpha_slow: int,
        price_type: PriceType = ...,
    ) -> None: ...
    def handle_quote_tick(self, tick: QuoteTick) -> None:
        """
        Update the indicator with the given quote tick.

        Parameters
        ----------
        tick : QuoteTick
            The update tick to handle.

        """
    def handle_trade_tick(self, tick: TradeTick) -> None:
        """
        Update the indicator with the given trade tick.

        Parameters
        ----------
        tick : TradeTick
            The update tick to handle.

        """
    def handle_bar(self, bar: Bar) -> None:
        """
        Update the indicator with the given bar.

        Parameters
        ----------
        bar : Bar
            The update bar to handle.

        """
    def update_raw(self, value: float) -> None:
        """
        Update the indicator with the given raw value.

        Parameters
        ----------
        value : double
            The update value.

        """
    def _reset_ma(self) -> None: ...
