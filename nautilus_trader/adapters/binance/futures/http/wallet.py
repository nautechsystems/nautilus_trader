# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2022 Nautech Systems Pty Ltd. All rights reserved.
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

from typing import Dict, List, Optional

import msgspec

from nautilus_trader.adapters.binance.http.client import BinanceHttpClient


class BinanceFuturesWalletHttpAPI:
    """
    Provides access to the `Binance Futures` Wallet HTTP REST API.

    Parameters
    ----------
    client : BinanceHttpClient
        The Binance REST API client.
    """

    def __init__(self, client: BinanceHttpClient):
        self.client = client

    async def commission_rate(
        self,
        symbol: Optional[str] = None,
        recv_window: Optional[int] = None,
    ) -> List[Dict[str, str]]:
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
        list[dict[str, str]]

        References
        ----------
        https://binance-docs.github.io/apidocs/spot/en/#trade-fee-user_data

        """
        payload: Dict[str, str] = {}
        if symbol is not None:
            payload["symbol"] = symbol
        if recv_window is not None:
            payload["recv_window"] = str(recv_window)

        raw: bytes = await self.client.sign_request(
            http_method="GET",
            url_path="/fapi/v1/commissionRate",
            payload=payload,
        )

        return msgspec.json.decode(raw)
