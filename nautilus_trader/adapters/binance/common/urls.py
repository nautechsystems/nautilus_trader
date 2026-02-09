# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.adapters.binance.common.enums import BinanceAccountType
from nautilus_trader.adapters.binance.common.enums import BinanceEnvironment


def get_http_base_url(  # noqa: C901 (URL dispatch)
    account_type: BinanceAccountType,
    environment: BinanceEnvironment,
    is_us: bool,
) -> str:
    if environment == BinanceEnvironment.TESTNET:
        if account_type.is_spot_or_margin:
            return "https://testnet.binance.vision"
        elif (
            account_type == BinanceAccountType.USDT_FUTURES
            or account_type == BinanceAccountType.COIN_FUTURES
        ):
            return "https://testnet.binancefuture.com"
        else:
            raise RuntimeError(  # pragma: no cover (design-time error)
                f"invalid `BinanceAccountType`, was {account_type}",  # pragma: no cover
            )

    if environment == BinanceEnvironment.DEMO:
        if account_type.is_spot_or_margin:
            return "https://demo-api.binance.com"
        elif account_type.is_futures:
            # Futures demo uses same URLs as futures testnet
            return "https://testnet.binancefuture.com"
        else:
            raise RuntimeError(  # pragma: no cover (design-time error)
                f"invalid `BinanceAccountType`, was {account_type}",  # pragma: no cover
            )

    top_level_domain: str = "us" if is_us else "com"
    if account_type.is_spot:
        return f"https://api.binance.{top_level_domain}"
    elif account_type.is_margin:
        return f"https://sapi.binance.{top_level_domain}"
    elif account_type == BinanceAccountType.USDT_FUTURES:
        return f"https://fapi.binance.{top_level_domain}"
    elif account_type == BinanceAccountType.COIN_FUTURES:
        return f"https://dapi.binance.{top_level_domain}"
    else:
        raise RuntimeError(  # pragma: no cover (design-time error)
            f"invalid `BinanceAccountType`, was {account_type}",  # pragma: no cover
        )


def get_ws_api_base_url(  # noqa: C901 (URL dispatch)
    account_type: BinanceAccountType,
    environment: BinanceEnvironment,
    is_us: bool,
) -> str:
    """
    Return the WebSocket API base URL for user data streams.

    This is the new authenticated WebSocket API endpoint that replaces listenKey.

    """
    if environment == BinanceEnvironment.TESTNET:
        if account_type.is_spot_or_margin:
            return "wss://testnet.binance.vision/ws-api/v3"
        elif account_type == BinanceAccountType.USDT_FUTURES:
            return "wss://testnet.binancefuture.com/ws-fapi/v1"
        elif account_type == BinanceAccountType.COIN_FUTURES:
            raise ValueError("no WS API testnet for COIN-M futures")
        else:
            raise RuntimeError(
                f"invalid `BinanceAccountType`, was {account_type}",
            )

    if environment == BinanceEnvironment.DEMO:
        if account_type.is_spot_or_margin:
            return "wss://demo-ws-api.binance.com/ws-api/v3"
        elif account_type == BinanceAccountType.USDT_FUTURES:
            # Futures demo uses same WS API as futures testnet
            return "wss://testnet.binancefuture.com/ws-fapi/v1"
        elif account_type == BinanceAccountType.COIN_FUTURES:
            raise ValueError("no WS API demo for COIN-M futures")
        else:
            raise RuntimeError(
                f"invalid `BinanceAccountType`, was {account_type}",
            )

    top_level_domain: str = "us" if is_us else "com"
    if account_type.is_spot_or_margin:
        return f"wss://ws-api.binance.{top_level_domain}:443/ws-api/v3"
    elif account_type == BinanceAccountType.USDT_FUTURES:
        return f"wss://ws-fapi.binance.{top_level_domain}/ws-fapi/v1"
    elif account_type == BinanceAccountType.COIN_FUTURES:
        return f"wss://ws-dapi.binance.{top_level_domain}/ws-dapi/v1"
    else:
        raise RuntimeError(
            f"invalid `BinanceAccountType`, was {account_type}",
        )


def get_ws_base_url(  # noqa: C901 (URL dispatch)
    account_type: BinanceAccountType,
    environment: BinanceEnvironment,
    is_us: bool,
) -> str:
    if environment == BinanceEnvironment.TESTNET:
        if account_type.is_spot_or_margin:
            return "wss://stream.testnet.binance.vision"
        elif account_type == BinanceAccountType.USDT_FUTURES:
            return "wss://stream.binancefuture.com"
        elif account_type == BinanceAccountType.COIN_FUTURES:
            return "wss://dstream.binancefuture.com"
        else:
            raise RuntimeError(  # pragma: no cover (design-time error)
                f"invalid `BinanceAccountType`, was {account_type}",  # pragma: no cover
            )

    if environment == BinanceEnvironment.DEMO:
        if account_type.is_spot_or_margin:
            return "wss://demo-stream.binance.com"
        elif account_type == BinanceAccountType.USDT_FUTURES:
            # Futures demo uses same WS URLs as futures testnet
            return "wss://stream.binancefuture.com"
        elif account_type == BinanceAccountType.COIN_FUTURES:
            return "wss://dstream.binancefuture.com"
        else:
            raise RuntimeError(  # pragma: no cover (design-time error)
                f"invalid `BinanceAccountType`, was {account_type}",  # pragma: no cover
            )

    top_level_domain: str = "us" if is_us else "com"
    if account_type.is_spot_or_margin:
        return f"wss://stream.binance.{top_level_domain}:9443"
    elif account_type == BinanceAccountType.USDT_FUTURES:
        return f"wss://fstream.binance.{top_level_domain}"
    elif account_type == BinanceAccountType.COIN_FUTURES:
        return f"wss://dstream.binance.{top_level_domain}"
    else:
        raise RuntimeError(
            f"invalid `BinanceAccountType`, was {account_type}",
        )  # pragma: no cover (design-time error)
