from nautilus_trader.indicators import DoubleExponentialMovingAverage
from nautilus_trader.indicators import ExponentialMovingAverage
from nautilus_trader.indicators import HullMovingAverage
from nautilus_trader.indicators import MovingAverageFactory
from nautilus_trader.indicators import MovingAverageType
from nautilus_trader.indicators import SimpleMovingAverage
from nautilus_trader.indicators import VariableIndexDynamicAverage
from nautilus_trader.indicators import WeightedMovingAverage
from nautilus_trader.indicators import WilderMovingAverage
from nautilus_trader.test_kit.providers import TestInstrumentProvider


AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy("AUD/USD")


class TestMaFactory:
    def test_simple_returns_expected_indicator(self):
        # Arrange, Act
        indicator = MovingAverageFactory.create(10, MovingAverageType.SIMPLE)

        # Assert
        assert isinstance(indicator, SimpleMovingAverage)

    def test_exponential_returns_expected_indicator(self):
        # Arrange, Act
        indicator = MovingAverageFactory.create(10, MovingAverageType.EXPONENTIAL)

        # Assert
        assert isinstance(indicator, ExponentialMovingAverage)

    def test_hull_returns_expected_indicator(self):
        # Arrange, Act
        indicator = MovingAverageFactory.create(10, MovingAverageType.HULL)

        # Assert
        assert isinstance(indicator, HullMovingAverage)

    def test_weighted_returns_expected_indicator(self):
        # Arrange, Act
        indicator = MovingAverageFactory.create(10, MovingAverageType.WEIGHTED)

        # Assert
        assert isinstance(indicator, WeightedMovingAverage)

    def test_wilde_returns_expected_indicator(self):
        # Arrange, Act
        indicator = MovingAverageFactory.create(10, MovingAverageType.WILDER)

        # Assert
        assert isinstance(indicator, WilderMovingAverage)

    def test_double_exponential_returns_expected_indicator(self):
        # Arrange, Act
        indicator = MovingAverageFactory.create(10, MovingAverageType.DOUBLE_EXPONENTIAL)

        # Assert
        assert isinstance(indicator, DoubleExponentialMovingAverage)

    def test_variable_index_dynamic_returns_expected_indicator(self):
        # Arrange, Act
        indicator = MovingAverageFactory.create(
            10,
            MovingAverageType.VARIABLE_INDEX_DYNAMIC,
            cmo_ma_type=MovingAverageType.SIMPLE,
        )

        # Assert
        assert isinstance(indicator, VariableIndexDynamicAverage)
