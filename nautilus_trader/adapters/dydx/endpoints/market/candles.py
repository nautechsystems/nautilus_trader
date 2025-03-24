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
Define the candles / bars endpoint.
"""

# ruff: noqa: N815

import datetime

import msgspec

from nautilus_trader.adapters.dydx.common.enums import DYDXCandlesResolution
from nautilus_trader.adapters.dydx.common.enums import DYDXEndpointType
from nautilus_trader.adapters.dydx.endpoints.endpoint import DYDXHttpEndpoint
from nautilus_trader.adapters.dydx.http.client import DYDXHttpClient
from nautilus_trader.adapters.dydx.schemas.ws import DYDXCandle
from nautilus_trader.core.nautilus_pyo3 import HttpMethod


class DYDXCandlesGetParams(msgspec.Struct, omit_defaults=True):
    """
    Represent the dYdX list perpetual markets parameters.
    """

    resolution: DYDXCandlesResolution
    limit: int | None = None
    fromISO: datetime.datetime | None = None
    toISO: datetime.datetime | None = None


class DYDXCandlesResponse(msgspec.Struct, forbid_unknown_fields=True):
    """
    Represent the dYdX candles response object.
    """

    candles: list[DYDXCandle]


class DYDXCandlesEndpoint(DYDXHttpEndpoint):
    """
    Define the bars endpoint.
    """

    def __init__(self, client: DYDXHttpClient) -> None:
        """
        Define the bars endpoint.
        """
        url_path = "/candles/perpetualMarkets/"
        super().__init__(
            client=client,
            url_path=url_path,
            endpoint_type=DYDXEndpointType.NONE,
            name="DYDXCandlesEndpoint",
        )
        self.method_type = HttpMethod.GET
        self._decoder = msgspec.json.Decoder(DYDXCandlesResponse)

    async def get(self, symbol: str, params: DYDXCandlesGetParams) -> DYDXCandlesResponse | None:
        """
        Call the bars endpoint.
        """
        url_path = f"/candles/perpetualMarkets/{symbol}"
        raw = await self._method(self.method_type, params=params, url_path=url_path)

        if raw is not None:
            return self._decoder.decode(raw)

        return None
