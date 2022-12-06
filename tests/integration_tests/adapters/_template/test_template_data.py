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
from nautilus_trader.model.instruments.base import Instrument
from tests.integration_tests.adapters._template.common import TestBaseClient


class TestBaseDataClient(TestBaseClient):
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

    @pytest.mark.skip(reason="base_class")
    @pytest.mark.asyncio
    async def test_connect(self):
        self.data_client.connect()
        await asyncio.sleep(0)
        assert self.data_client.is_connected

    @pytest.mark.skip(reason="base_class")
    @pytest.mark.asyncio
    async def test_subscribe_trade_ticks(self):
        self.data_client.subscribe_trade_ticks(self.instrument)
