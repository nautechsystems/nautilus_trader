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

from typing import Any

import msgspec

from nautilus_trader.adapters.binance.common.enums import BinanceSecurityType
from nautilus_trader.adapters.binance.common.symbol import BinanceSymbol
from nautilus_trader.adapters.binance.common.symbol import BinanceSymbols
from nautilus_trader.adapters.binance.http.client import BinanceHttpClient
from nautilus_trader.core.nautilus_pyo3 import HttpMethod


def enc_hook(obj: Any) -> Any:
    if isinstance(obj, BinanceSymbol) or isinstance(obj, BinanceSymbols):
        return str(obj)  # serialize BinanceSymbol as string.
    else:
        raise TypeError(f"Objects of type {type(obj)} are not supported")


class BinanceHttpEndpoint:
    """
    Base functionality of endpoints connecting to the Binance REST API.

    Warnings
    --------
    This class should not be used directly, but through a concrete subclass.

    """

    def __init__(
        self,
        client: BinanceHttpClient,
        methods_desc: dict[HttpMethod, BinanceSecurityType],
        url_path: str,
    ):
        self.client = client
        self.methods_desc = methods_desc
        self.url_path = url_path

        self.decoder = msgspec.json.Decoder()
        self.encoder = msgspec.json.Encoder(enc_hook=enc_hook)

        self._method_request = {
            BinanceSecurityType.NONE: self.client.send_request,
            BinanceSecurityType.USER_STREAM: self.client.send_request,
            BinanceSecurityType.MARKET_DATA: self.client.send_request,
            BinanceSecurityType.TRADE: self.client.sign_request,
            BinanceSecurityType.MARGIN: self.client.sign_request,
            BinanceSecurityType.USER_DATA: self.client.sign_request,
        }

    async def _method(
        self,
        method_type: HttpMethod,
        params: Any,
        ratelimiter_keys: list[str] | None = None,
    ) -> bytes:
        payload: dict = self.decoder.decode(self.encoder.encode(params))
        if self.methods_desc[method_type] is None:
            raise RuntimeError(
                f"{method_type.name} not available for {self.url_path}",
            )
        raw: bytes = await self._method_request[self.methods_desc[method_type]](
            http_method=method_type,
            url_path=self.url_path,
            payload=payload,
            ratelimiter_keys=ratelimiter_keys,
        )
        return raw
