from stubs.indicators.average.moving_average import MovingAverage
from stubs.indicators.average.moving_average import MovingAverageType

class MovingAverageFactory:

    @staticmethod
    def create(
        period: int,
        ma_type: MovingAverageType,
        **kwargs,
    ) -> MovingAverage: ...
