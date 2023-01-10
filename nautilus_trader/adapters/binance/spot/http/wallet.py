# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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

from typing import Optional

import msgspec

from nautilus_trader.adapters.binance.common.enums import BinanceAccountType
from nautilus_trader.adapters.binance.common.enums import BinanceMethodType
from nautilus_trader.adapters.binance.common.enums import BinanceSecurityType
from nautilus_trader.adapters.binance.common.schemas.symbol import BinanceSymbol
from nautilus_trader.adapters.binance.http.client import BinanceHttpClient
from nautilus_trader.adapters.binance.http.endpoint import BinanceHttpEndpoint
from nautilus_trader.adapters.binance.spot.schemas.wallet import BinanceSpotTradeFees


class BinanceSpotTradeFeeHttp(BinanceHttpEndpoint):
    """
    Endpoint of maker/taker trade fee information

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
            BinanceMethodType.GET: BinanceSecurityType.USER_DATA,
        }
        super().__init__(
            client,
            methods,
            base_endpoint + "tradeFee",
        )
        self.get_resp_decoder = msgspec.json.Decoder(BinanceSpotTradeFees)

    class GetParameters(msgspec.Struct, omit_defaults=True, frozen=True):
        """
        GET parameters for fetching trade fees

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
        symbol: Optional[BinanceSymbol] = None
        recvWindow: Optional[str] = None

    async def _get(self, parameters: GetParameters) -> BinanceSpotTradeFees:
        method_type = BinanceMethodType.GET
        raw = await self._method(method_type, parameters)
        return self.get_resp_decoder.decode(raw)

    async def request_trade_fees(
        self,
        timestamp: str,
        symbol: Optional[BinanceSymbol] = None,
        recv_window: Optional[str] = None,
    ) -> BinanceSpotTradeFees:
        fees = await self._get(
            parameters=self.GetParameters(
                timestamp=timestamp,
                symbol=symbol,
                recvWindow=recv_window,
            ),
        )
        return fees


class BinanceSpotWalletHttpAPI:
    """
    Provides access to the `Binance Spot/Margin` Wallet HTTP REST API.

    Parameters
    ----------
    client : BinanceHttpClient
        The Binance REST API client.
    """

    def __init__(
        self,
        client: BinanceHttpClient,
        account_type: BinanceAccountType,
    ):
        self.client = client
        self.base_endpoint = "/sapi/v1/asset/"

        if not account_type.is_spot_or_margin:
            raise RuntimeError(  # pragma: no cover (design-time error)
                f"`BinanceAccountType` not SPOT, MARGIN_CROSS or MARGIN_ISOLATED, was {account_type}",  # pragma: no cover
            )

        self.endpoint_trade_fee = BinanceSpotTradeFeeHttp(client, self.base_endpoint)
