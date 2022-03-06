from unittest.mock import patch

import pytest
from ib_insync import Contract
from ib_insync import LimitOrder

from tests.integration_tests.adapters.interactive_brokers.base import InteractiveBrokersTestBase
from tests.integration_tests.adapters.interactive_brokers.test_kit import IBExecTestStubs
from tests.integration_tests.adapters.interactive_brokers.test_kit import IBTestStubs
from tests.test_kit.stubs import TestStubs


class TestInteractiveBrokersData(InteractiveBrokersTestBase):
    def setup(self):
        super().setup()
        self.instrument = IBTestStubs.instrument("AAPL")
        self.contract_details = IBTestStubs.contract_details("AAPL")
        self.contract = self.contract_details.contract

    def instrument_setup(self, instrument=None, contract_details=None):
        instrument = instrument or self.instrument
        contract_details = contract_details or self.contract_details
        self.exec_client._instrument_provider.contract_details[instrument.id] = contract_details
        self.exec_client._instrument_provider.contract_id_to_instrument_id[
            contract_details.contract.conId
        ] = instrument.id

    @pytest.mark.asyncio
    async def test_factory(self, event_loop):
        # Act
        exec_client = self.exec_client

        # Assert
        assert exec_client is not None

    @pytest.mark.asyncio
    async def test_place_order(self, event_loop):
        # Arrange
        instrument = IBTestStubs.instrument("AAPL")
        contract_details = IBTestStubs.contract_details("AAPL")
        self.instrument_setup(instrument=instrument, contract_details=contract_details)
        order = TestStubs.limit_order(
            instrument_id=instrument.id,
        )
        command = TestStubs.submit_order_command(order=order)

        # Act
        with patch.object(self.exec_client._client, "placeOrder") as mock:
            self.exec_client.submit_order(command=command)

        # Assert
        expected = {
            "contract": Contract(
                secType="STK",
                conId=265598,
                symbol="AAPL",
                exchange="SMART",
                primaryExchange="NASDAQ",
                currency="USD",
                localSymbol="AAPL",
                tradingClass="NMS",
            ),
            "order": LimitOrder(action="BUY", totalQuantity=10.0, lmtPrice=0.5),
        }
        name, args, kwargs = mock.mock_calls[0]
        # Can't directly compare kwargs for some reason?
        assert kwargs["contract"] == expected["contract"]
        assert kwargs["order"].action == expected["order"].action
        assert kwargs["order"].totalQuantity == expected["order"].totalQuantity
        assert kwargs["order"].lmtPrice == expected["order"].lmtPrice

    @pytest.mark.asyncio
    async def test_on_new_order(self, event_loop):
        # Arrange
        self.instrument_setup()
        self.exec_client._client_order_id_to_strategy_id[
            TestStubs.client_order_id()
        ] = TestStubs.strategy_id()
        self.exec_client._venue_order_id_to_client_order_id[1] = TestStubs.client_order_id()
        trade = IBExecTestStubs.trade_pre_submit()

        # Act
        with patch.object(self.exec_client, "generate_order_submitted") as mock:
            self.exec_client._on_new_order(trade)

        # Assert
        name, args, kwargs = mock.mock_calls[0]
        expected = {
            "strategy_id": TestStubs.strategy_id(),
            "instrument_id": self.instrument.id,
            "client_order_id": TestStubs.client_order_id(),
            "ts_event": 1646449586871811000,
        }
        assert kwargs == expected
