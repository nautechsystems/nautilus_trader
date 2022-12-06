# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2022 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
#  You may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
#
#  Unless required by applicable law or agreed to in writing, software
#  distributed under the License is distributed on an "AS IS" BASIS,
#  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
#  See the License for the specific language governing permissions and
#  limitations under the License.
# -------------------------------------------------------------------------------------------------
import asyncio
from typing import Optional

import pytest

from nautilus_trader.common.providers import InstrumentProvider
from nautilus_trader.live.factories import LiveDataClientFactory
from nautilus_trader.live.factories import LiveExecClientFactory
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.instruments.base import Instrument
from tests.integration_tests.adapters._template.common import TestBaseClient


class TestBaseExecClient(TestBaseClient):
    def setup(
        self,
        venue: Venue,
        instrument: Instrument,
        exec_client_factory: Optional[LiveExecClientFactory] = None,
        exec_client_config: Optional[dict] = None,
        data_client_factory: Optional[LiveDataClientFactory] = None,
        data_client_config: Optional[dict] = None,
        instrument_provider: Optional[InstrumentProvider] = None,
    ):
        super().setup(
            venue=venue,
            instrument=instrument,
            exec_client_config=exec_client_config,
            exec_client_factory=exec_client_factory,
            data_client_config=data_client_config,
            data_client_factory=data_client_factory,
            instrument_provider=instrument_provider,
        )

    # --- HELPER FUNCTIONS ----------------------------------------------------------- #

    async def submit_order(self, order):
        self.strategy.submit_order(order)
        await asyncio.sleep(0)

    async def accept_order(self, order, venue_order_id: Optional[VenueOrderId] = None):
        self.strategy.submit_order(order)
        await asyncio.sleep(0)
        self.exec_client.generate_order_accepted(
            strategy_id=order.strategy_id,
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            venue_order_id=venue_order_id or order.venue_order_id,
            ts_event=0,
        )
        return order

    # --- BASE TESTS ----------------------------------------------------------------- #

    @pytest.mark.asyncio
    @pytest.mark.skip(reason="base_class")
    async def test_connect(self):
        self.exec_client.connect()
        await asyncio.sleep(0)
        assert self.exec_client.is_connected

    @pytest.mark.skip(reason="base_class")
    def test_disconnect(self):
        self.exec_client.connect()
        self.exec_client.disconnect()
        assert not self.exec_client.is_connected

    @pytest.mark.skip(reason="base_class")
    def test_submit_order(self):
        raise NotImplementedError

    @pytest.mark.skip(reason="base_class")
    def test_submit_bracket_order(self):
        raise NotImplementedError

    @pytest.mark.skip(reason="base_class")
    def test_modify_order(self):
        raise NotImplementedError

    @pytest.mark.skip(reason="base_class")
    def test_cancel_order(self):
        raise NotImplementedError

    @pytest.mark.skip(reason="base_class")
    def test_generate_order_status_report(self):
        raise NotImplementedError

    # TODO - do we want to do something like this
    # @pytest.mark.asyncio
    # async def test_submit_order(self):
    #     # Arrange
    #     order = TestExecStubs.market_order(instrument_id=self.instrument.id)
    #     command = TestCommandStubs.submit_order_command(order=order)
    #     self.exec_client.submit_order(command)
    #     await asyncio.sleep(0)
    #
    # def assert_order_submitted(self):
    #     raise NotImplementedError
