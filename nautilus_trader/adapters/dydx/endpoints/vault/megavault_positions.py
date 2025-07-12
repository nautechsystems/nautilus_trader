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
Define the MegaVault positions endpoint.
"""
import msgspec

from nautilus_trader.adapters.dydx.common.enums import DYDXEndpointType
from nautilus_trader.adapters.dydx.endpoints.endpoint import DYDXHttpEndpoint
from nautilus_trader.adapters.dydx.http.client import DYDXHttpClient
from nautilus_trader.core.nautilus_pyo3 import HttpMethod


class DYDXMegaVaultPositionsGetParams(msgspec.Struct, omit_defaults=True):
    """
    Represent the MegaVault positions request parameters.
    """

    # No params for this endpoint


class DYDXVaultPosition(msgspec.Struct, forbid_unknown_fields=True):
    """
    Represent a single MegaVault position object.
    """

    # Based on dYdX API documentation
    market: str
    side: str
    size: str
    maxSize: str
    entryPrice: str
    realizedPnl: str
    createdAt: str
    updatedAt: str


class DYDXMegaVaultPositionResponse(msgspec.Struct, forbid_unknown_fields=True):
    """
    Represent the MegaVault positions response object.
    """

    positions: list[DYDXVaultPosition]


class DYDXMegaVaultPositionsEndpoint(DYDXHttpEndpoint):
    """
    Define the MegaVault positions endpoint.
    """

    def __init__(self, client: DYDXHttpClient) -> None:
        url_path = "/v4/vault/v1/megavault/positions"
        super().__init__(
            client=client,
            url_path=url_path,
            endpoint_type=DYDXEndpointType.NONE,
            name="DYDXMegaVaultPositionsEndpoint",
        )
        self.method_type = HttpMethod.GET
        self._decoder = msgspec.json.Decoder(DYDXMegaVaultPositionResponse)

    async def get(self) -> DYDXMegaVaultPositionResponse | None:
        """
        Call the MegaVault positions endpoint.
        """
        raw = await self._method(self.method_type)
        if raw is not None:
            return self._decoder.decode(raw)
        return None
