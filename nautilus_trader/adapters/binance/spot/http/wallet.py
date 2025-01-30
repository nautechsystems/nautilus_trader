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

from nautilus_trader.adapters.binance.common.enums import BinanceAccountType
from nautilus_trader.adapters.binance.common.enums import BinanceSecurityType
from nautilus_trader.adapters.binance.common.symbol import BinanceSymbol
from nautilus_trader.adapters.binance.http.client import BinanceHttpClient
from nautilus_trader.adapters.binance.http.endpoint import BinanceHttpEndpoint
from nautilus_trader.adapters.binance.spot.schemas.wallet import BinanceSpotTradeFee
from nautilus_trader.common.component import LiveClock
from nautilus_trader.core.nautilus_pyo3 import HttpMethod


class BinanceSpotTradeFeeHttp(BinanceHttpEndpoint):
    """
    Endpoint of maker/taker trade fee information.

    `GET /sapi/v1/asset/tradeFee`

    References
    ----------
    https://binance-docs.github.io/apidocs/spot/en/#trade-fee-user_data

    """

    def __init__(
        self,
        client: BinanceHttpClient,
        base_endpoint: str,
    ):
        methods = {
            HttpMethod.GET: BinanceSecurityType.USER_DATA,
        }
        super().__init__(
            client,
            methods,
            base_endpoint + "tradeFee",
        )
        self._get_obj_resp_decoder = msgspec.json.Decoder(BinanceSpotTradeFee)
        self._get_arr_resp_decoder = msgspec.json.Decoder(list[BinanceSpotTradeFee])

    class GetParameters(msgspec.Struct, omit_defaults=True, frozen=True):
        """
        GET parameters for requesting trade fees.

        Parameters
        ----------
        symbol : BinanceSymbol
            Optional symbol to receive individual trade fee
        recvWindow : str
            Optional number of milliseconds after timestamp the request is valid
        timestamp : str
            Millisecond timestamp of the request

        """

        timestamp: str
        symbol: BinanceSymbol | None = None
        recvWindow: str | None = None

    async def get(self, params: GetParameters) -> list[BinanceSpotTradeFee]:
        method_type = HttpMethod.GET
        raw = await self._method(method_type, params)
        if params.symbol is not None:
            return [self._get_obj_resp_decoder.decode(raw)]
        else:
            return self._get_arr_resp_decoder.decode(raw)


class BinanceSpotWalletHttpAPI:
    """
    Provides access to the Binance Spot/Margin Wallet HTTP REST API.

    Parameters
    ----------
    client : BinanceHttpClient
        The Binance REST API client.

    """

    def __init__(
        self,
        client: BinanceHttpClient,
        clock: LiveClock,
        account_type: BinanceAccountType = BinanceAccountType.SPOT,
    ):
        self.client = client
        self._clock = clock
        self.base_endpoint = "/sapi/v1/asset/"

        if not account_type.is_spot_or_margin:
            raise RuntimeError(  # pragma: no cover (design-time error)
                f"`BinanceAccountType` not SPOT, MARGIN or ISOLATED_MARGIN, was {account_type}",  # pragma: no cover
            )

        self._endpoint_spot_trade_fee = BinanceSpotTradeFeeHttp(client, self.base_endpoint)

    def _timestamp(self) -> str:
        """
        Create Binance timestamp from internal clock.
        """
        return str(self._clock.timestamp_ms())

    async def query_spot_trade_fees(
        self,
        symbol: str | None = None,
        recv_window: str | None = None,
    ) -> list[BinanceSpotTradeFee]:
        fees = await self._endpoint_spot_trade_fee.get(
            params=self._endpoint_spot_trade_fee.GetParameters(
                timestamp=self._timestamp(),
                symbol=BinanceSymbol(symbol) if symbol is not None else None,
                recvWindow=recv_window,
            ),
        )
        return fees
