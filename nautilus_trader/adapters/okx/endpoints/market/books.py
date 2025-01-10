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

import msgspec

from nautilus_trader.adapters.okx.common.enums import OKXEndpointType
from nautilus_trader.adapters.okx.endpoints.endpoint import OKXHttpEndpoint
from nautilus_trader.adapters.okx.http.client import OKXHttpClient
from nautilus_trader.adapters.okx.schemas.market.books import OKXOrderBookSnapshotResponse
from nautilus_trader.core.nautilus_pyo3 import HttpMethod


class OKXBooksGetParams(msgspec.Struct, omit_defaults=True, frozen=True):
    instId: str | None = None
    sz: str = "50"  # depth, okx defaults to 1 but we use here 50 to match ws channel book50-l2-tbt

    def validate(self) -> None:
        assert int(self.sz) <= 400, "OKX's max order book depth (`sz`) is 400"


class OKXBooksEndpoint(OKXHttpEndpoint):
    def __init__(
        self,
        client: OKXHttpClient,
        base_endpoint: str,
    ) -> None:
        url_path = base_endpoint + "/books"
        super().__init__(
            client=client,
            endpoint_type=OKXEndpointType.MARKET,
            url_path=url_path,
        )
        self._resp_decoder_books = msgspec.json.Decoder(OKXOrderBookSnapshotResponse)

    async def get(self, params: OKXBooksGetParams) -> OKXOrderBookSnapshotResponse:
        # Validate
        params.validate()

        raw = await self._method(HttpMethod.GET, params)  # , ratelimiter_keys=[self.url_path])
        try:
            return self._resp_decoder_books.decode(raw)
        except Exception as e:
            raise RuntimeError(
                f"Failed to decode response from {self.url_path}: {raw.decode()} from error: {e}",
            )
