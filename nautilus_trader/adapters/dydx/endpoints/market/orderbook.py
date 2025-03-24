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
Define the orderbook snapshot endpoint.
"""


import msgspec

from nautilus_trader.adapters.dydx.common.enums import DYDXEndpointType
from nautilus_trader.adapters.dydx.endpoints.endpoint import DYDXHttpEndpoint
from nautilus_trader.adapters.dydx.http.client import DYDXHttpClient
from nautilus_trader.adapters.dydx.schemas.ws import DYDXWsOrderbookMessageSnapshotContents
from nautilus_trader.core.nautilus_pyo3 import HttpMethod


class DYDXOrderBookSnapshotEndpoint(DYDXHttpEndpoint):
    """
    Define the order book snapshot endpoint.
    """

    def __init__(self, client: DYDXHttpClient) -> None:
        """
        Define the order book snapshot endpoint.

        Parameters
        ----------
        client : DYDXHttpClient
            The HTTP client.

        """
        url_path = "/orderbooks/perpetualMarket/"
        super().__init__(
            client=client,
            url_path=url_path,
            endpoint_type=DYDXEndpointType.NONE,
            name="DYDXOrderBookSnapshotEndpoint",
        )
        self.method_type = HttpMethod.GET
        self._decoder = msgspec.json.Decoder(DYDXWsOrderbookMessageSnapshotContents)

    async def get(self, symbol: str) -> DYDXWsOrderbookMessageSnapshotContents | None:
        """
        Call the endpoint to request an order book snapshot.

        Parameters
        ----------
        symbol : str
            The ticker or symbol to request the order book snapshot for.

        Returns
        -------
        DYDXWsOrderbookMessageSnapshotContents | None
            The order book snapshot message.

        """
        url_path = f"/orderbooks/perpetualMarket/{symbol}"
        raw = await self._method(self.method_type, url_path=url_path)

        if raw is not None:
            return self._decoder.decode(raw)

        return None
