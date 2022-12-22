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

from typing import Optional

import msgspec

from nautilus_trader.adapters.binance.common.enums import BinanceAccountType
from nautilus_trader.adapters.binance.common.functions import convert_symbols_list_to_json_array
from nautilus_trader.adapters.binance.common.functions import format_symbol
from nautilus_trader.adapters.binance.http.client import BinanceHttpClient
from nautilus_trader.adapters.binance.http.market import BinanceMarketHttpAPI
from nautilus_trader.adapters.binance.spot.schemas.market import BinanceSpotExchangeInfo


class BinanceSpotMarketHttpAPI(BinanceMarketHttpAPI):
    """
    Provides access to the `Binance Spot` Market HTTP REST API.

    Parameters
    ----------
    client : BinanceHttpClient
        The Binance REST API client.
    account_type : BinanceAccountType
        The Binance account type, used to select the endpoint

    """

    def __init__(
        self,
        client: BinanceHttpClient,
        account_type: BinanceAccountType,
    ):
        super().__init__(
            client=client,
            account_type=account_type,
        )

        self._decoder_exchange_info = msgspec.json.Decoder(BinanceSpotExchangeInfo)

    async def exchange_info(
        self,
        symbol: Optional[str] = None,
        symbols: Optional[list[str]] = None,
    ) -> BinanceSpotExchangeInfo:
        """
        Get current exchange trading rules and symbol information.
        Only either `symbol` or `symbols` should be passed.

        Exchange Information.
        `GET /api/v3/exchangeinfo`

        Parameters
        ----------
        symbol : str, optional
            The trading pair.
        symbols : list[str], optional
            The list of trading pairs.

        Returns
        -------
        BinanceSpotExchangeInfo

        References
        ----------
        https://binance-docs.github.io/apidocs/spot/en/#exchange-information

        """
        if symbol and symbols:
            raise ValueError("`symbol` and `symbols` cannot be sent together")

        payload: dict[str, str] = {}
        if symbol is not None:
            payload["symbol"] = format_symbol(symbol)
        if symbols is not None:
            payload["symbols"] = convert_symbols_list_to_json_array(symbols)

        raw: bytes = await self.client.query(
            url_path=self.base_endpoint + "exchangeInfo",
            payload=payload,
        )

        return self._decoder_exchange_info.decode(raw)
