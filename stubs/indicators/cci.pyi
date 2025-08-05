from stubs.indicators.average.moving_average import MovingAverageType
from stubs.indicators.base.indicator import Indicator
from stubs.model.data import Bar

class CommodityChannelIndex(Indicator):
    """
    Commodity Channel Index is a momentum oscillator used to primarily identify
    overbought and oversold levels relative to a mean.

    Parameters
    ----------
    period : int
        The rolling window period for the indicator (> 0).
    scalar : double
        A positive float to scale the bands
    ma_type : MovingAverageType
        The moving average type for prices.

    References
    ----------
    https://www.tradingview.com/support/solutions/43000502001-commodity-channel-index-cci/
    """

    period: int
    scalar: float
    value: float

    def __init__(
        self,
        period: int,
        scalar: float = 0.015,
        ma_type: MovingAverageType = ...,
    ) -> None: ...
    def handle_bar(self, bar: Bar) -> None:
        """
        Update the indicator with the given bar.

        Parameters
        ----------
        bar : Bar
            The update bar.

        """
    def update_raw(
        self,
        high: float,
        low: float,
        close: float,
    ) -> None:
        """
        Update the indicator with the given raw values.

        Parameters
        ----------
        high : double
            The high price.
        low : double
            The low price.
        close : double
            The close price.

        """
    def _reset(self) -> None:
        """
        Reset the indicator.

        All stateful fields are reset to their initial value.
        """
