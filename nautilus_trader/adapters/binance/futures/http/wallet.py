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
from nautilus_trader.adapters.binance.futures.schemas.wallet import BinanceFuturesCommissionRate
from nautilus_trader.adapters.binance.http.client import BinanceHttpClient
from nautilus_trader.adapters.binance.http.endpoint import BinanceHttpEndpoint
from nautilus_trader.common.clock import LiveClock


class BinanceFuturesCommissionRateHttp(BinanceHttpEndpoint):
    """
    Endpoint of maker/taker commission rate information.

    `GET /fapi/v1/commissionRate`
    `GET /dapi/v1/commissionRate`

    References
    ----------
    https://binance-docs.github.io/apidocs/futures/en/#user-commission-rate-user_data
    https://binance-docs.github.io/apidocs/delivery/en/#user-commission-rate-user_data
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
            base_endpoint + "commissionRate",
        )
        self._get_resp_decoder = msgspec.json.Decoder(BinanceFuturesCommissionRate)

    class GetParameters(msgspec.Struct, omit_defaults=True, frozen=True):
        """
        GET parameters for fetching commission rate.

        Parameters
        ----------
        symbol : BinanceSymbol
            Receive commission rate of the provided symbol.
        timestamp : str
            Millisecond timestamp of the request.
        recvWindow : str, optional
            The number of milliseconds after timestamp the request is valid.
        """

        timestamp: str
        symbol: BinanceSymbol
        recvWindow: Optional[str] = None

    async def _get(self, parameters: GetParameters) -> BinanceFuturesCommissionRate:
        method_type = BinanceMethodType.GET
        raw = await self._method(method_type, parameters)
        return self._get_resp_decoder.decode(raw)


class BinanceFuturesWalletHttpAPI:
    """
    Provides access to the `Binance Futures` Wallet HTTP REST API.

    Parameters
    ----------
    client : BinanceHttpClient
        The Binance REST API client.
    """

    def __init__(
        self,
        client: BinanceHttpClient,
        clock: LiveClock,
        account_type: BinanceAccountType = BinanceAccountType.FUTURES_USDT,
    ):
        self.client = client
        self._clock = clock

        if account_type == BinanceAccountType.FUTURES_USDT:
            self.base_endpoint = "/fapi/v1/"
        elif account_type == BinanceAccountType.FUTURES_COIN:
            self.base_endpoint = "/dapi/v1/"

        if not account_type.is_futures:
            raise RuntimeError(  # pragma: no cover (design-time error)
                f"`BinanceAccountType` not FUTURES_USDT or FUTURES_COIN, was {account_type}",  # pragma: no cover
            )

        self._endpoint_futures_commission_rate = BinanceFuturesCommissionRateHttp(
            client,
            self.base_endpoint,
        )

    def _timestamp(self) -> str:
        """Create Binance timestamp from internal clock."""
        return str(self._clock.timestamp_ms())

    async def query_futures_commission_rate(
        self,
        symbol: str,
        recv_window: Optional[str] = None,
    ) -> BinanceFuturesCommissionRate:
        """Get Futures commission rates for a given symbol."""
        rate = await self._endpoint_futures_commission_rate._get(
            parameters=self._endpoint_futures_commission_rate.GetParameters(
                timestamp=self._timestamp(),
                symbol=BinanceSymbol(symbol),
                recvWindow=recv_window,
            ),
        )
        return rate
