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

import pkgutil

import pytest

from nautilus_trader.adapters.bybit.common.enums import BybitInstrumentType
from nautilus_trader.adapters.bybit.http.client import BybitHttpClient
from nautilus_trader.adapters.bybit.providers import BybitInstrumentProvider
from nautilus_trader.common.logging import Logger
from nautilus_trader.core.nautilus_pyo3.network import HttpClient

from nautilus_trader.common.clock import LiveClock

from nautilus_trader.common.providers import InstrumentProvider
from nautilus_trader.config import InstrumentProviderConfig
from tests.integration_tests.adapters.bybit.conftest import bybit_http_client, live_logger, live_clock
from tests.integration_tests.adapters.bybit.utils.get_mock import get_mock


class TestBybitInstrumentProvider:

    def setup(self):
        self.clock = LiveClock()
        self.live_logger = Logger(clock=self.clock)
        self.http_client: BybitHttpClient = BybitHttpClient(
            clock=self.clock,
            logger=self.live_logger,
            api_key="BYBIT_API_KEY",
            api_secret="BYBIT_API_SECRET",
            base_url="https://api-testnet.bybit.com",
        )

    def get_target_instrument_provider(
        self,
        instrument_type: BybitInstrumentType
    )->InstrumentProvider:
        return BybitInstrumentProvider(
            client=self.http_client,
            logger=self.live_logger,
            clock=self.clock,
            instrument_type=instrument_type,
            is_testnet=True,
            config=InstrumentProviderConfig(load_all=True)
        )

    @pytest.mark.asyncio()
    async def test_spot_load_all_async(
            self,
            monkeypatch
    ):
        instrument_provider = self.get_target_instrument_provider(BybitInstrumentType.SPOT)
        response = pkgutil.get_data(
            "tests.integration_tests.adapters.bybit.resources.http_responses.spot",
            "instruments.json"
        )
        monkeypatch.setattr(HttpClient, "request", get_mock(response))
        await instrument_provider.load_all_async()

    @pytest.mark.asyncio()
    async def test_linear_load_all_async(self,monkeypatch):
        instrument_provider = self.get_target_instrument_provider(BybitInstrumentType.LINEAR)
        response = pkgutil.get_data(
            "tests.integration_tests.adapters.bybit.resources.http_responses.linear",
            "instruments.json"
        )
        monkeypatch.setattr(HttpClient, "request", get_mock(response))
        await instrument_provider.load_all_async()
        instruments = instrument_provider.get_all()
        instruments_ids = list(instruments.keys())
        assert len(instruments_ids) == 2
        assert str(instruments_ids[0]) == 'BTCUSDT-PERP.BYBIT'
        assert str(instruments_ids[1]) == 'ETHUSDT-PERP.BYBIT'

    @pytest.mark.asyncio()
    async def test_options_load_all_async(self,monkeypatch):
        instrument_provider = self.get_target_instrument_provider(BybitInstrumentType.OPTION)
        response = pkgutil.get_data(
            "tests.integration_tests.adapters.bybit.resources.http_responses.option",
            "instruments.json"
        )
        monkeypatch.setattr(HttpClient, "request", get_mock(response))
        await instrument_provider.load_all_async()
        instruments = instrument_provider.get_all()
        instruments_ids = list(instruments.keys())
        assert len(instruments_ids) == 2
        assert str(instruments_ids[0]) == 'BTCUSDT-PERP.BYBIT'
        assert str(instruments_ids[1]) == 'ETHUSDT-PERP.BYBIT'




