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

import msgspec

from nautilus_trader.adapters.env import get_env_key


def get_polymarket_api_key() -> str:
    return get_env_key("POLYMARKET_API_KEY")


def get_polymarket_api_secret() -> str:
    return get_env_key("POLYMARKET_API_SECRET")


def get_polymarket_passphrase() -> str:
    return get_env_key("POLYMARKET_PASSPHRASE")


def get_polymarket_private_key() -> str:
    return get_env_key("POLYMARKET_PK")


def get_polymarket_funder() -> str:
    return get_env_key("POLYMARKET_FUNDER")


class PolymarketWebSocketAuth(msgspec.Struct, frozen=True):
    apiKey: str
    secret: str
    passphrase: str
