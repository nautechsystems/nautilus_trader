# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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
Define the base class for dYdX endpoints.
"""

from typing import Any

import msgspec

from nautilus_trader.adapters.dydx.common.enums import DYDXEndpointType
from nautilus_trader.adapters.dydx.http.client import DYDXHttpClient
from nautilus_trader.core.nautilus_pyo3 import HttpMethod


class DYDXHttpEndpoint:
    """
    Define the base class for dYdX endpoints.
    """

    def __init__(
        self,
        client: DYDXHttpClient,
        endpoint_type: DYDXEndpointType,
        url_path: str | None = None,
    ) -> None:
        """
        Construct the base class for dYdX endpoints.
        """
        self.client = client
        self.endpoint_type = endpoint_type
        self.url_path = url_path

        self.decoder = msgspec.json.Decoder()
        self.encoder = msgspec.json.Encoder()

        self._method_request: dict[DYDXEndpointType, Any] = {
            DYDXEndpointType.NONE: self.client.send_request,
            DYDXEndpointType.ACCOUNT: self.client.send_request,
        }

    async def _method(
        self,
        method_type: HttpMethod,
        params: Any | None = None,
        url_path: str | None = None,
    ) -> bytes:
        payload: dict = self.decoder.decode(self.encoder.encode(params))
        method_call = self._method_request[self.endpoint_type]
        raw: bytes = await method_call(
            http_method=method_type,
            url_path=url_path or self.url_path,
            payload=payload,
        )
        return raw
