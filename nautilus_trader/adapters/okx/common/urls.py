# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.adapters.env import get_env_key_or
from nautilus_trader.adapters.okx.common.enums import OKXWsBaseUrlType


def get_http_base_url() -> str:
    return get_env_key_or("OKX_BASE_URL_HTTP", "https://www.okx.com")


def get_ws_base_url(ws_base_url_type: OKXWsBaseUrlType, is_demo: bool) -> str:
    if is_demo:
        match ws_base_url_type:
            case OKXWsBaseUrlType.PUBLIC:
                return get_env_key_or(
                    "OKX_DEMO_BASE_URL_PUBLIC_WS",
                    "wss://wspap.okx.com:8443/ws/v5/public",
                )
            case OKXWsBaseUrlType.PRIVATE:
                return get_env_key_or(
                    "OKX_DEMO_BASE_URL_PRIVATE_WS",
                    "wss://wspap.okx.com:8443/ws/v5/private",
                )
            case OKXWsBaseUrlType.BUSINESS:
                return get_env_key_or(
                    "OKX_DEMO_BASE_URL_BUSINESS_WS",
                    "wss://wspap.okx.com:8443/ws/v5/business",
                )
            case _:
                raise ValueError(
                    f"unknown websocket base url type {ws_base_url_type} - must be one of "
                    f"{list(OKXWsBaseUrlType)}",
                )

    match ws_base_url_type:
        case OKXWsBaseUrlType.PUBLIC:
            return get_env_key_or("OKX_BASE_URL_PUBLIC_WS", "wss://ws.okx.com:8443/ws/v5/public")
        case OKXWsBaseUrlType.PRIVATE:
            return get_env_key_or("OKX_BASE_URL_PRIVATE_WS", "wss://ws.okx.com:8443/ws/v5/private")
        case OKXWsBaseUrlType.BUSINESS:
            return get_env_key_or(
                "OKX_BASE_URL_BUSINESS_WS",
                "wss://ws.okx.com:8443/ws/v5/business",
            )
        case _:
            raise ValueError(
                f"unknown websocket base url type {ws_base_url_type} - must be one of "
                f"{list(OKXWsBaseUrlType)}",
            )
