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
"""
Define the dYdX API for calling market endpoints.
"""

import pandas as pd

from nautilus_trader.adapters.dydx.common.enums import DYDXCandlesResolution
from nautilus_trader.adapters.dydx.endpoints.market.candles import DYDXCandlesEndpoint
from nautilus_trader.adapters.dydx.endpoints.market.candles import DYDXCandlesGetParams
from nautilus_trader.adapters.dydx.endpoints.market.candles import DYDXCandlesResponse

# fmt: off
from nautilus_trader.adapters.dydx.endpoints.market.instruments_info import DYDXListPerpetualMarketsEndpoint
from nautilus_trader.adapters.dydx.endpoints.market.instruments_info import DYDXListPerpetualMarketsResponse
from nautilus_trader.adapters.dydx.endpoints.market.instruments_info import ListPerpetualMarketsGetParams
from nautilus_trader.adapters.dydx.endpoints.market.orderbook import DYDXOrderBookSnapshotEndpoint
from nautilus_trader.adapters.dydx.http.client import DYDXHttpClient
from nautilus_trader.adapters.dydx.schemas.ws import DYDXWsOrderbookMessageSnapshotContents

# fmt: on
from nautilus_trader.common.component import LiveClock
from nautilus_trader.core.correctness import PyCondition


class DYDXMarketHttpAPI:
    """
    Define the dYdX API for calling market endpoints.
    """

    def __init__(
        self,
        client: DYDXHttpClient,
        clock: LiveClock,
    ) -> None:
        """
        Define the dYdX API for calling market endpoints.
        """
        PyCondition.not_none(client, "client")
        self.client = client
        self._clock = clock

        self._endpoint_instruments = DYDXListPerpetualMarketsEndpoint(client)
        self._endpoint_candles = DYDXCandlesEndpoint(client)
        self._endpoint_orderbook = DYDXOrderBookSnapshotEndpoint(client)

    async def fetch_instruments(
        self,
        symbol: str | None = None,
        limit: int | None = None,
    ) -> DYDXListPerpetualMarketsResponse | None:
        """
        Fetch all the instruments for the dYdX venue.
        """
        return await self._endpoint_instruments.get(
            ListPerpetualMarketsGetParams(ticker=symbol, limit=limit),
        )

    async def get_orderbook(self, symbol: str) -> DYDXWsOrderbookMessageSnapshotContents | None:
        """
        Request an orderbook snapshot.

        Parameters
        ----------
        symbol : str
            The ticker or symbol to request the order book snapshot for.

        Returns
        -------
        DYDXWsOrderbookMessageSnapshotContents | None
            The order book snapshot message.

        """
        return await self._endpoint_orderbook.get(symbol=symbol)

    async def get_candles(
        self,
        symbol: str,
        resolution: DYDXCandlesResolution,
        limit: int | None = None,
        start: pd.Timestamp | None = None,
        end: pd.Timestamp | None = None,
    ) -> DYDXCandlesResponse | None:
        """
        Fetch the bars from the dYdX venue.
        """
        start_date = start.to_pydatetime() if start is not None else start
        end_date = end.to_pydatetime() if end is not None else end
        return await self._endpoint_candles.get(
            symbol=symbol,
            params=DYDXCandlesGetParams(
                resolution=resolution,
                limit=limit,
                fromISO=start_date,
                toISO=end_date,
            ),
        )
