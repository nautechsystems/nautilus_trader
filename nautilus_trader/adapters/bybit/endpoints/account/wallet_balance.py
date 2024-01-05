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

import msgspec

from nautilus_trader.adapters.bybit.common.enums import BybitEndpointType
from nautilus_trader.adapters.bybit.endpoints.endpoint import BybitHttpEndpoint
from nautilus_trader.adapters.bybit.http.client import BybitHttpClient
from nautilus_trader.adapters.bybit.schemas.account.balance import BybitWalletBalanceResponse
from nautilus_trader.core.nautilus_pyo3 import HttpMethod


class BybitWalletBalanceGetParameters(msgspec.Struct, omit_defaults=True, frozen=False):
    accountType: str | None = None
    coin: str | None = None


class BybitWalletBalanceEndpoint(BybitHttpEndpoint):
    def __init__(
        self,
        client: BybitHttpClient,
        base_endpoint: str,
    ) -> None:
        self.http_method = HttpMethod.GET
        url_path = base_endpoint + "/account/wallet-balance"
        super().__init__(
            client=client,
            endpoint_type=BybitEndpointType.ACCOUNT,
            url_path=url_path,
        )
        self._get_resp_decoder = msgspec.json.Decoder(BybitWalletBalanceResponse)

    async def get(self, parameters: BybitWalletBalanceGetParameters) -> BybitWalletBalanceResponse:
        raw = await self._method(self.http_method, parameters)
        try:
            return self._get_resp_decoder.decode(raw)
        except Exception as e:
            decoded_raw = raw.decode("utf-8")
            raise RuntimeError(
                f"Failed to decode response wallet balance response: {decoded_raw}",
            ) from e
