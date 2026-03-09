from nautilus_trader.common.component import Component


class TestCommonComponent:
    def test_fully_qualified_name_returns_expected(self):
        # Arrange, Act
        result = Component.fully_qualified_name()

        # Assert
        assert result == "nautilus_trader.common.component:Component"
