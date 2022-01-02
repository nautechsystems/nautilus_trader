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

import pkgutil
from typing import Any, Dict

import orjson
import pytest

from nautilus_trader.adapters.ftx.http.client import FTXHttpClient
from nautilus_trader.adapters.ftx.providers import FTXInstrumentProvider
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import Venue


class TestFTXInstrumentProvider:
    @pytest.mark.asyncio
    async def test_load_all_async(
        self,
        ftx_http_client,
        live_logger,
        monkeypatch,
    ):
        # Arrange: prepare data for monkey patch
        response1 = pkgutil.get_data(
            package="tests.integration_tests.adapters.ftx.resources.responses",
            resource="account_info.json",
        )

        response2 = pkgutil.get_data(
            package="tests.integration_tests.adapters.ftx.resources.responses",
            resource="markets.json",
        )

        responses = [response2, response1]

        # Mock coroutine for patch
        async def mock_send_request(
            self,  # noqa (needed for mock)
            http_method: str,  # noqa (needed for mock)
            url_path: str,  # noqa (needed for mock)
            headers: Dict[str, Any] = None,  # noqa (needed for mock)
            payload: Dict[str, str] = None,  # noqa (needed for mock)
        ) -> bytes:
            return orjson.loads(responses.pop())

        # Apply mock coroutine to client
        monkeypatch.setattr(
            target=FTXHttpClient,
            name="_send_request",
            value=mock_send_request,
        )

        self.provider = FTXInstrumentProvider(
            client=ftx_http_client,
            logger=live_logger,
        )

        # Act
        await self.provider.load_all_async()

        # Assert
        assert self.provider.count == 6
        assert self.provider.find(InstrumentId(Symbol("1INCH-PERP"), Venue("FTX"))) is not None
        assert self.provider.find(InstrumentId(Symbol("1INCH-1231"), Venue("FTX"))) is not None
        assert self.provider.find(InstrumentId(Symbol("1INCH/USD"), Venue("FTX"))) is not None
        assert self.provider.find(InstrumentId(Symbol("AAPL-1231"), Venue("FTX"))) is not None
        assert self.provider.find(InstrumentId(Symbol("AAPL/USD"), Venue("FTX"))) is not None
        assert self.provider.find(InstrumentId(Symbol("AAVE-PERP"), Venue("FTX"))) is not None
        assert len(self.provider.currencies()) == 3
        assert "1INCH" in self.provider.currencies()
        assert "USD" in self.provider.currencies()
        # assert "AAPL" not in self.provider.currencies()  # TODO: Tokenized equities
