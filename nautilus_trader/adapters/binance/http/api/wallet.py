# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2021 Nautech Systems Pty Ltd. All rights reserved.
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
#
#  Heavily refactored from MIT licensed github.com/binance/binance-connector-python
#  Original author: Jeremy https://github.com/2pd
# -------------------------------------------------------------------------------------------------

from typing import Dict, Optional

from nautilus_trader.adapters.binance.http.client import BinanceHttpClient
from nautilus_trader.core.correctness import PyCondition


class BinanceWalletHttpAPI:
    """
    Provides access to the `Binance Wallet` HTTP REST API.
    """

    BASE_ENDPOINT = "/sapi/v1/"

    def __init__(self, client: BinanceHttpClient):
        """
        Initialize a new instance of the ``BinanceWalletHttpAPI`` class.

        Parameters
        ----------
        client : BinanceHttpClient
            The Binance REST API client.

        """
        PyCondition.not_none(client, "client")

        self.client = client

    async def trade_fee(
        self,
        symbol: Optional[str] = None,
        recv_window: Optional[int] = None,
    ) -> bytes:
        """
        Fetch trade fee.

        `GET /sapi/v1/asset/tradeFee`

        Parameters
        ----------
        symbol : str, optional
            The trading pair. If None then queries for all symbols.
        recv_window : int, optional
            The acceptable receive window for the response.

        Returns
        -------
        bytes
            The raw response content.

        References
        ----------
        https://binance-docs.github.io/apidocs/spot/en/#trade-fee-user_data

        """
        payload: Dict[str, str] = {}
        if symbol is not None:
            payload["symbol"] = symbol
        if recv_window is not None:
            payload["recv_window"] = str(recv_window)

        return await self.client.sign_request(
            http_method="GET",
            url_path=self.BASE_ENDPOINT + "asset/tradeFee",
            payload=payload,
        )
