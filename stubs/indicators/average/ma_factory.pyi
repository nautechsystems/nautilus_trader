from stubs.indicators.average.moving_average import MovingAverage
from stubs.indicators.average.moving_average import MovingAverageType

class MovingAverageFactory:
    """
    Provides a factory to construct different moving average indicators.
    """

    @staticmethod
    def create(
        period: int,
        ma_type: MovingAverageType,
        **kwargs,
    ) -> MovingAverage:
        """
        Create a moving average indicator corresponding to the given ma_type.

        Parameters
        ----------
        period : int
            The period of the moving average (> 0).
        ma_type : MovingAverageType
            The moving average type.

        Returns
        -------
        MovingAverage

        Raises
        ------
        ValueError
            If `period` is not positive (> 0).

        """
        ...
