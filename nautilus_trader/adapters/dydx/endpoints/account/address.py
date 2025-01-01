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
Provide the Get Address HTTP endpoint.
"""

import msgspec

from nautilus_trader.adapters.dydx.common.enums import DYDXEndpointType
from nautilus_trader.adapters.dydx.endpoints.endpoint import DYDXHttpEndpoint
from nautilus_trader.adapters.dydx.http.client import DYDXHttpClient
from nautilus_trader.adapters.dydx.schemas.account.address import DYDXAddressResponse
from nautilus_trader.core.nautilus_pyo3 import HttpMethod


class DYDXGetAddressGetParams(msgspec.Struct, omit_defaults=True, frozen=True):
    """
    Define the parameters for the Get Address endpoint.
    """

    address: str


class DYDXGetAddressEndpoint(DYDXHttpEndpoint):
    """
    Provide the Get Address HTTP endpoint.
    """

    def __init__(
        self,
        client: DYDXHttpClient,
    ) -> None:
        """
        Construct a new get address HTTP endpoint.
        """
        super().__init__(
            client=client,
            endpoint_type=DYDXEndpointType.ACCOUNT,
            name="DYDXGetAddressEndpoint",
        )
        self.http_method = HttpMethod.GET
        self._get_resp_decoder = msgspec.json.Decoder(DYDXAddressResponse)

    async def get(self, params: DYDXGetAddressGetParams) -> DYDXAddressResponse | None:
        """
        Call the endpoint to list the instruments.
        """
        url_path = f"/addresses/{params.address}"
        raw = await self._method(self.http_method, params=None, url_path=url_path)

        if raw is not None:
            return self._get_resp_decoder.decode(raw)

        return None
