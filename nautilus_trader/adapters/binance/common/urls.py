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

from urllib.parse import urlparse

from nautilus_trader.adapters.binance.common.enums import BinanceAccountType
from nautilus_trader.adapters.binance.common.enums import BinanceEnvironment


def _is_usdm_ws_host(base_url: str) -> bool:
    hostname = urlparse(base_url).hostname
    if hostname is None:
        return False
    # Matches fstream.binance.com, fstream-mm.binance.com, fstream-auth.binance.com,
    # and their .us counterparts, without accepting arbitrary substrings.
    return hostname.startswith("fstream") and hostname.endswith(
        (".binance.com", ".binance.us"),
    )


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
        elif account_type == BinanceAccountType.USDT_FUTURES:
            return "https://demo-fapi.binance.com"
        elif account_type == BinanceAccountType.COIN_FUTURES:
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
            return "wss://ws-api.testnet.binance.vision/ws-api/v3"
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
        return f"wss://fstream.binance.{top_level_domain}/market"
    elif account_type == BinanceAccountType.COIN_FUTURES:
        return f"wss://dstream.binance.{top_level_domain}"
    else:
        raise RuntimeError(
            f"invalid `BinanceAccountType`, was {account_type}",
        )  # pragma: no cover (design-time error)


def get_ws_public_base_url(
    account_type: BinanceAccountType,
    environment: BinanceEnvironment,
    is_us: bool,
) -> str:
    """
    Return the WebSocket public stream base URL for high-frequency book data.

    USD-M Futures mainnet uses `wss://fstream.binance.com/public`.
    All other account types and environments fall back to `get_ws_base_url`.

    """
    if (
        environment not in (BinanceEnvironment.TESTNET, BinanceEnvironment.DEMO)
        and account_type == BinanceAccountType.USDT_FUTURES
    ):
        top_level_domain: str = "us" if is_us else "com"
        return f"wss://fstream.binance.{top_level_domain}/public"

    return get_ws_base_url(account_type, environment, is_us)


def get_ws_private_base_url(
    account_type: BinanceAccountType,
    environment: BinanceEnvironment,
    is_us: bool,
) -> str:
    """
    Return the WebSocket private stream base URL for user data.

    USD-M Futures mainnet uses `wss://fstream.binance.com/private`.
    All other account types and environments fall back to `get_ws_base_url`.

    """
    if (
        environment not in (BinanceEnvironment.TESTNET, BinanceEnvironment.DEMO)
        and account_type == BinanceAccountType.USDT_FUTURES
    ):
        top_level_domain: str = "us" if is_us else "com"
        return f"wss://fstream.binance.{top_level_domain}/private"

    return get_ws_base_url(account_type, environment, is_us)


def get_usdm_ws_route_base_url(base_url: str, route: str) -> str:
    """
    Return a routed USD-M Futures WebSocket base URL derived from an override.

    Binance now routes USD-M Futures mainnet traffic by category. This helper
    accepts either a root override (for example `wss://fstream.binance.com`) or
    a routed/transport-specific override such as `/market`, `/public/ws`, or
    `/private/stream`, then rebuilds the base URL for the requested route.

    URLs that do not point at `fstream.binance.com` (for example local test
    endpoints) are returned unchanged.

    Parameters
    ----------
    base_url : str
        The custom WebSocket base URL override.
    route : str
        The USD-M Futures route: `market`, `public`, or `private`.

    Returns
    -------
    str

    Raises
    ------
    ValueError
        If `route` is invalid.

    """
    if route not in {"market", "public", "private"}:
        raise ValueError(f"invalid USD-M WebSocket route, was {route!r}")

    if not _is_usdm_ws_host(base_url):
        return base_url

    normalized = base_url.rstrip("/")
    suffixes = (
        "/market/ws",
        "/market/stream",
        "/public/ws",
        "/public/stream",
        "/private/ws",
        "/private/stream",
        "/market",
        "/public",
        "/private",
        "/ws",
        "/stream",
    )

    for suffix in suffixes:
        if normalized.endswith(suffix):
            normalized = normalized[: -len(suffix)]
            break

    return f"{normalized}/{route}"
