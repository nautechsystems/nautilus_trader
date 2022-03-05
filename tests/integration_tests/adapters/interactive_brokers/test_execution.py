from unittest.mock import patch

import pytest
from ib_insync import OrderStatus

from nautilus_trader.adapters.interactive_brokers.config import InteractiveBrokersExecClientConfig
from nautilus_trader.adapters.interactive_brokers.factories import (
    InteractiveBrokersLiveExecClientFactory,
)
from tests.integration_tests.adapters.interactive_brokers.base import InteractiveBrokersTestBase
from tests.integration_tests.adapters.interactive_brokers.test_kit import IBExecTestStubs


class TestInteractiveBrokersData(InteractiveBrokersTestBase):
    def setup(self):
        super().setup()
        with patch("nautilus_trader.adapters.interactive_brokers.factories.get_cached_ib_client"):
            self.exec_client = InteractiveBrokersLiveExecClientFactory.create(
                loop=self.loop,
                name="IB",
                config=InteractiveBrokersExecClientConfig(  # noqa: S106
                    username="test", password="test"
                ),
                msgbus=self.msgbus,
                cache=self.cache,
                clock=self.clock,
                logger=self.logger,
            )

    def instrument_setup(self, instrument, contract_details):
        self.exec_client.instrument_provider.contract_details[instrument.id] = contract_details
        self.exec_client.instrument_provider.contract_id_to_instrument_id[
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
        pass

    @pytest.mark.asyncio
    async def test_on_new_order(self, event_loop):
        # Arrange
        trade = IBExecTestStubs.trade_response(order_status=OrderStatus.PreSubmitted)

        # Act
        with patch.object(self.exec_client, "generate_order_accepted") as mock:
            self.exec_client._on_new_order(trade)

        # Assert
        mock_call = mock.method_calls[0]
        assert mock_call
