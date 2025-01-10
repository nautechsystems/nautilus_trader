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

from nautilus_trader.adapters.okx.common.enums import OKXInstrumentType
from nautilus_trader.adapters.okx.endpoints.account.balance import OKXAccountBalanceEndpoint
from nautilus_trader.adapters.okx.endpoints.account.balance import OKXAccountBalanceGetParams
from nautilus_trader.adapters.okx.endpoints.account.positions import OKXAccountPositionsEndpoint
from nautilus_trader.adapters.okx.endpoints.account.positions import OKXAccountPositionsGetParams
from nautilus_trader.adapters.okx.endpoints.account.trade_fee import OKXTradeFeeEndpoint
from nautilus_trader.adapters.okx.endpoints.account.trade_fee import OKXTradeFeeGetParams
from nautilus_trader.adapters.okx.http.client import OKXHttpClient
from nautilus_trader.adapters.okx.schemas.account.balance import OKXAccountBalanceData
from nautilus_trader.adapters.okx.schemas.account.positions import OKXAccountPositionsResponse
from nautilus_trader.adapters.okx.schemas.account.trade_fee import OKXTradeFee
from nautilus_trader.common.component import LiveClock
from nautilus_trader.core.correctness import PyCondition


class OKXAccountHttpAPI:
    def __init__(
        self,
        client: OKXHttpClient,
        clock: LiveClock,
    ) -> None:
        PyCondition.not_none(client, "client")
        self.client = client
        self._clock = clock
        self.base_endpoint = "/api/v5/account"
        # self.default_settle_coin = "USDT" # TODO: needed?

        self._endpoint_fee_rate = OKXTradeFeeEndpoint(client, self.base_endpoint)
        self._endpoint_balance = OKXAccountBalanceEndpoint(client, self.base_endpoint)
        self._endpoint_positions = OKXAccountPositionsEndpoint(client, self.base_endpoint)

    async def fetch_trade_fee(
        self,
        instType: OKXInstrumentType,
        uly: str | None = None,
        instFamily: str | None = None,
        instId: str | None = None,
    ) -> OKXTradeFee:
        response = await self._endpoint_fee_rate.get(
            OKXTradeFeeGetParams(
                instType=instType,
                uly=uly,
                instFamily=instFamily,
                instId=instId,
            ),
        )
        return response.data[0]

    async def fetch_balance(self, ccy: str | None = None) -> OKXAccountBalanceData:
        response = await self._endpoint_balance.get(
            OKXAccountBalanceGetParams(ccy=ccy),
        )
        return response.data[0]

    async def fetch_positions(
        self,
        instType: OKXInstrumentType | None = None,
        instId: str | None = None,
        posId: str | None = None,
    ) -> OKXAccountPositionsResponse:
        response = await self._endpoint_positions.get(
            OKXAccountPositionsGetParams(instType=instType, instId=instId, posId=posId),
        )
        return response
