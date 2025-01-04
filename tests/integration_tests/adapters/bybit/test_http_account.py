# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

import msgspec
import pytest

from nautilus_trader.adapters.bybit.common.enums import BybitProductType
from nautilus_trader.adapters.bybit.http.account import BybitAccountHttpAPI
from nautilus_trader.adapters.bybit.http.client import BybitHttpClient
from nautilus_trader.adapters.bybit.schemas.account.fee_rate import BybitFeeRateResponse
from nautilus_trader.common.component import LiveClock
from nautilus_trader.core.nautilus_pyo3 import HttpClient
from tests.integration_tests.adapters.bybit.utils.get_mock import get_mock


class TestBybitAccountHttpApi:
    def setup(self):
        clock = LiveClock()
        self.client = BybitHttpClient(
            clock=clock,
            api_key="SOME_BYBIT_API_KEY",
            api_secret="SOME_BYBIT_API_SECRET",
            base_url="https://api-testnet.bybit.com",
        )
        self.http_api = BybitAccountHttpAPI(
            client=self.client,
            clock=clock,
        )

    @pytest.mark.asyncio()
    async def test_fee_rate(self, monkeypatch):
        response = pkgutil.get_data(
            "tests.integration_tests.adapters.bybit.resources.http_responses",
            "fee_rate.json",
        )
        response_decoded = msgspec.json.Decoder(BybitFeeRateResponse).decode(response)

        monkeypatch.setattr(HttpClient, "request", get_mock(response))
        fee_rate = await self.http_api.fetch_fee_rate(
            product_type=BybitProductType.SPOT,
        )
        assert fee_rate == response_decoded.result.list
