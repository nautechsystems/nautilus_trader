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

from nautilus_trader.adapters.okx.common.enums import OKXContractType
from nautilus_trader.adapters.okx.common.enums import OKXInstrumentType
from nautilus_trader.adapters.okx.common.enums import OKXTradeMode
from nautilus_trader.adapters.okx.endpoints.public.instruments import OKXInstrumentsEndpoint
from nautilus_trader.adapters.okx.endpoints.public.instruments import OKXInstrumentsGetParams
from nautilus_trader.adapters.okx.endpoints.public.position_tiers import OKXPositionTiersEndpoint
from nautilus_trader.adapters.okx.endpoints.public.position_tiers import OKXPositionTiersGetParams
from nautilus_trader.adapters.okx.http.client import OKXHttpClient
from nautilus_trader.adapters.okx.schemas.public.instrument import OKXInstrument
from nautilus_trader.adapters.okx.schemas.public.instrument import OKXInstrumentList
from nautilus_trader.adapters.okx.schemas.public.position_tiers import OKXPositionTiersResponse
from nautilus_trader.common.component import LiveClock
from nautilus_trader.core.correctness import PyCondition


class OKXPublicHttpAPI:
    def __init__(
        self,
        client: OKXHttpClient,
        clock: LiveClock,
    ) -> None:
        PyCondition.not_none(client, "client")
        self.client = client
        self._clock = clock
        self.base_endpoint = "/api/v5/public"

        self._endpoint_instruments = OKXInstrumentsEndpoint(client, self.base_endpoint)
        self._endpoint_position_tiers = OKXPositionTiersEndpoint(client, self.base_endpoint)

    def _get_url(self, url: str) -> str:
        return self.base_endpoint + url

    async def fetch_instruments(
        self,
        instType: OKXInstrumentType,
        ctType: OKXContractType | None = None,
        uly: str | None = None,
        instFamily: str | None = None,
        instId: str | None = None,
    ) -> OKXInstrumentList:
        response = await self._endpoint_instruments.get(
            OKXInstrumentsGetParams(
                instType=instType,
                uly=uly,
                instFamily=instFamily,
                instId=instId,
            ),
        )
        if ctType:
            return [i for i in response.data if i.ctType == ctType]  # type: ignore
        return response.data

    async def fetch_instrument(
        self,
        instType: OKXInstrumentType,
        instId: str,
        uly: str | None = None,
        instFamily: str | None = None,
    ) -> OKXInstrument:
        response = await self._endpoint_instruments.get(
            OKXInstrumentsGetParams(
                instType=instType,
                uly=uly,
                instFamily=instFamily,
                instId=instId,
            ),
        )
        return response.data[0]

    async def fetch_position_tiers(
        self,
        instType: OKXInstrumentType,
        tdMode: OKXTradeMode,
        uly: str | None = None,
        instFamily: str | None = None,
        instId: str | None = None,
        ccy: str | None = None,
        tier: str | None = None,
    ) -> OKXPositionTiersResponse:
        response = await self._endpoint_position_tiers.get(
            OKXPositionTiersGetParams(
                instType=instType,
                tdMode=tdMode,
                uly=uly,
                instFamily=instFamily,
                instId=instId,
                ccy=ccy,
                tier=tier,
            ),
        )
        return response
