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
"""
Read the authentication secrets from environment variables.
"""

from nautilus_trader.adapters.env import get_env_key


def get_wallet_address(is_testnet: bool) -> str:
    """
    Return the wallet address for dYdX.
    """
    if is_testnet:
        return get_env_key("DYDX_TESTNET_WALLET_ADDRESS")

    return get_env_key("DYDX_WALLET_ADDRESS")


def get_mnemonic(is_testnet: bool) -> str:
    """
    Return the wallet mnemonic for dYdX.
    """
    if is_testnet:
        return get_env_key("DYDX_TESTNET_MNEMONIC")

    return get_env_key("DYDX_MNEMONIC")
