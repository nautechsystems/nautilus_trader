from ib_insync import LimitOrder as IBLimitOrder
from ib_insync import MarketOrder as IBMarketOrder

from nautilus_trader.adapters.interactive_brokers.parsing.execution import (
    nautilus_order_to_ib_order,
)
from tests.integration_tests.adapters.interactive_brokers.base import InteractiveBrokersTestBase
from tests.integration_tests.adapters.interactive_brokers.test_kit import IBTestStubs
from tests.test_kit.stubs.execution import TestExecStubs


class TestInteractiveBrokersData(InteractiveBrokersTestBase):
    def setup(self):
        super().setup()
        self.instrument = IBTestStubs.instrument("AAPL")

    def test_nautilus_order_to_ib_market_order(self):
        # Arrange
        nautilus_market_order = TestExecStubs.market_order(instrument_id=self.instrument.id)

        # Act
        result = nautilus_order_to_ib_order(nautilus_market_order)

        # Assert
        expected = IBMarketOrder(action="BUY", totalQuantity=100.0)
        assert result.action == expected.action
        assert result.totalQuantity == expected.totalQuantity

    def test_nautilus_order_to_ib_limit_order(self):
        # Arrange
        nautilus_market_order = TestExecStubs.limit_order(instrument_id=self.instrument.id)

        # Act
        result = nautilus_order_to_ib_order(nautilus_market_order)

        # Assert
        expected = IBLimitOrder(action="BUY", totalQuantity=100.0, lmtPrice=55.0)
        assert result.action == expected.action
        assert result.totalQuantity == expected.totalQuantity
        assert result.lmtPrice == expected.lmtPrice
