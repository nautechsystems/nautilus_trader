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

from typing import Any

import msgspec

from nautilus_trader.adapters.binance.common.enums import BinanceAccountType
from nautilus_trader.adapters.binance.common.enums import BinanceSecurityType
from nautilus_trader.adapters.binance.http.client import BinanceHttpClient
from nautilus_trader.adapters.binance.http.endpoint import BinanceHttpEndpoint
from nautilus_trader.adapters.binance.papi.types import BinancePortfolioMarginBalance
from nautilus_trader.adapters.binance.papi.types import BinancePortfolioMarginPositionRisk
from nautilus_trader.common.component import LiveClock
from nautilus_trader.core.nautilus_pyo3 import HttpMethod


class BinancePortfolioMarginBalanceEndpoint(BinanceHttpEndpoint):
    """
    Endpoint for getting Portfolio Margin account balance.
    """

    def __init__(self, client: BinanceHttpClient):
        methods = {
            HttpMethod.GET: BinanceSecurityType.USER_DATA,
        }
        url_path = "/papi/v1/balance"
        super().__init__(client, methods, url_path)
        self._resp_decoder = msgspec.json.Decoder(list[BinancePortfolioMarginBalance])

    class GetParameters(msgspec.Struct):
        """
        GET /papi/v1/balance parameters.
        """

        recvWindow: int | None = None
        timestamp: int | None = None

    async def get(self, parameters: GetParameters) -> list[BinancePortfolioMarginBalance]:
        method_type = HttpMethod.GET
        raw: bytes = await self._method(method_type, parameters)
        return self._resp_decoder.decode(raw)


class BinancePortfolioMarginPositionRiskEndpoint(BinanceHttpEndpoint):
    """
    Endpoint for getting Portfolio Margin position risk information.
    """

    def __init__(self, client: BinanceHttpClient, position_type: str):
        """
        Parameters
        ----------
        client : BinanceHttpClient
            The HTTP client.
        position_type : str
            Either "um" for USD-M futures or "cm" for COIN-M futures.
        """
        methods = {
            HttpMethod.GET: BinanceSecurityType.USER_DATA,
        }
        url_path = f"/papi/v1/{position_type}/positionRisk"
        super().__init__(client, methods, url_path)
        self._resp_decoder = msgspec.json.Decoder(list[BinancePortfolioMarginPositionRisk])

    class GetParameters(msgspec.Struct):
        """
        GET /papi/v1/{type}/positionRisk parameters.
        """

        symbol: str | None = None
        recvWindow: int | None = None
        timestamp: int | None = None

    async def get(self, parameters: GetParameters) -> list[BinancePortfolioMarginPositionRisk]:
        method_type = HttpMethod.GET
        raw: bytes = await self._method(method_type, parameters)
        return self._resp_decoder.decode(raw)


class BinancePortfolioMarginAccountHttpAPI:
    """
    Provides access to the Binance Portfolio Margin HTTP account endpoints.

    Parameters
    ----------
    client : BinanceHttpClient
        The Binance HTTP client.
    clock : LiveClock
        The clock for the client.
    account_type : BinanceAccountType
        The account type for the client.

    """

    def __init__(
        self,
        client: BinanceHttpClient,
        clock: LiveClock,
        account_type: BinanceAccountType,
    ) -> None:
        self.client = client
        self._clock = clock
        self._account_type = account_type

        if account_type != BinanceAccountType.PORTFOLIO_MARGIN:
            raise ValueError(f"Invalid account_type: {account_type}")

        # Initialize endpoints
        self._balance = BinancePortfolioMarginBalanceEndpoint(client)
        self._um_position_risk = BinancePortfolioMarginPositionRiskEndpoint(client, "um")
        self._cm_position_risk = BinancePortfolioMarginPositionRiskEndpoint(client, "cm")

    async def query_balance(self) -> list[BinancePortfolioMarginBalance]:
        """
        Query Portfolio Margin account balance.
        
        Returns
        -------
        list[BinancePortfolioMarginBalance]
            The portfolio margin account balances.
        """
        timestamp = self._clock.timestamp_ns() // 1_000_000
        response = await self._balance.get(
            BinancePortfolioMarginBalanceEndpoint.GetParameters(
                timestamp=timestamp,
            ),
        )
        return response

    async def query_um_position_risk(
        self, 
        symbol: str | None = None,
    ) -> list[BinancePortfolioMarginPositionRisk]:
        """
        Query USD-M futures position risk information.
        
        Parameters
        ----------
        symbol : str, optional
            The symbol to query. If None, all positions are returned.
            
        Returns
        -------
        list[BinancePortfolioMarginPositionRisk]
            The position risk information.
        """
        timestamp = self._clock.timestamp_ns() // 1_000_000
        response = await self._um_position_risk.get(
            BinancePortfolioMarginPositionRiskEndpoint.GetParameters(
                symbol=symbol,
                timestamp=timestamp,
            ),
        )
        return response

    async def query_cm_position_risk(
        self, 
        symbol: str | None = None,
    ) -> list[BinancePortfolioMarginPositionRisk]:
        """
        Query COIN-M futures position risk information.
        
        Parameters
        ----------
        symbol : str, optional
            The symbol to query. If None, all positions are returned.
            
        Returns
        -------
        list[BinancePortfolioMarginPositionRisk]
            The position risk information.
        """
        timestamp = self._clock.timestamp_ns() // 1_000_000
        response = await self._cm_position_risk.get(
            BinancePortfolioMarginPositionRiskEndpoint.GetParameters(
                symbol=symbol,
                timestamp=timestamp,
            ),
        )
        return response
