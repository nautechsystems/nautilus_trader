from nautilus_trader.core.data import Data


class TestCoreData:
    def test_fully_qualified_name_returns_expected(self):
        # Arrange, Act
        result = Data.fully_qualified_name()

        # Assert
        assert result == "nautilus_trader.core.data:Data"
