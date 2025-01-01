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

from nautilus_trader.adapters.env import get_env_key


def get_api_key(is_demo: bool, is_testnet: bool) -> str:
    if is_demo and is_testnet:
        raise ValueError("Invalid configuration: both `is_demo` and `is_testnet` were True")

    if is_demo:
        key = get_env_key("BYBIT_DEMO_API_KEY")
        if not key:
            raise ValueError(
                "BYBIT_DEMO_API_KEY environment variable not set",
            )
        return key
    elif is_testnet:
        key = get_env_key("BYBIT_TESTNET_API_KEY")
        if not key:
            raise ValueError(
                "BYBIT_TESTNET_API_KEY environment variable not set",
            )
        return key
    else:
        key = get_env_key("BYBIT_API_KEY")
        if not key:
            raise ValueError("BYBIT_API_KEY environment variable not set")
        return key


def get_api_secret(is_demo: bool, is_testnet: bool) -> str:
    if is_demo and is_testnet:
        raise ValueError("Invalid configuration: both `is_demo` and `is_testnet` were True")

    if is_demo:
        secret = get_env_key("BYBIT_DEMO_API_SECRET")
        if not secret:
            raise ValueError(
                "BYBIT_DEMO_API_SECRET environment variable not set",
            )
        return secret
    elif is_testnet:
        secret = get_env_key("BYBIT_TESTNET_API_SECRET")
        if not secret:
            raise ValueError(
                "BYBIT_TESTNET_API_SECRET environment variable not set",
            )
        return secret
    else:
        secret = get_env_key("BYBIT_API_SECRET")
        if not secret:
            raise ValueError("BYBIT_API_SECRET environment variable not set")
        return secret
