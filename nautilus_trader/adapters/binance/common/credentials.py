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

from nautilus_trader.adapters.binance.common.enums import BinanceAccountType
from nautilus_trader.adapters.env import get_env_key


def get_api_key(account_type: BinanceAccountType, is_testnet: bool) -> str:
    if is_testnet:
        if account_type.is_spot_or_margin:
            return get_env_key("BINANCE_TESTNET_API_KEY")
        else:
            return get_env_key("BINANCE_FUTURES_TESTNET_API_KEY")

    if account_type.is_spot_or_margin:
        return get_env_key("BINANCE_API_KEY")
    else:
        return get_env_key("BINANCE_FUTURES_API_KEY")


def get_api_secret(account_type: BinanceAccountType, is_testnet: bool) -> str:
    if is_testnet:
        if account_type.is_spot_or_margin:
            return get_env_key("BINANCE_TESTNET_API_SECRET")
        else:
            return get_env_key("BINANCE_FUTURES_TESTNET_API_SECRET")

    if account_type.is_spot_or_margin:
        return get_env_key("BINANCE_API_SECRET")
    else:
        return get_env_key("BINANCE_FUTURES_API_SECRET")


def get_rsa_private_key(account_type: BinanceAccountType, is_testnet: bool) -> str:
    if is_testnet:
        if account_type.is_spot_or_margin:
            return get_env_key("BINANCE_TESTNET_RSA_PK")
        else:
            return get_env_key("BINANCE_FUTURES_TESTNET_RSA_PK")

    if account_type.is_spot_or_margin:
        return get_env_key("BINANCE_RSA_PK")
    else:
        return get_env_key("BINANCE_FUTURES_RSA_PK")


def get_ed25519_private_key(account_type: BinanceAccountType, is_testnet: bool) -> str:
    if is_testnet:
        if account_type.is_spot_or_margin:
            return get_env_key("BINANCE_TESTNET_ED25519_PK")
        else:
            return get_env_key("BINANCE_FUTURES_TESTNET_ED25519_PK")

    if account_type.is_spot_or_margin:
        return get_env_key("BINANCE_ED25519_PK")
    else:
        return get_env_key("BINANCE_FUTURES_ED25519_PK")
