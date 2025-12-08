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

from __future__ import annotations

import os

from nautilus_trader.adapters.lighter.constants import ENV_ACCOUNT_INDEX
from nautilus_trader.adapters.lighter.constants import ENV_ACCOUNT_INDEX_TESTNET
from nautilus_trader.adapters.lighter.constants import ENV_API_KEY_PRIVATE_KEY
from nautilus_trader.adapters.lighter.constants import ENV_API_KEY_PRIVATE_KEY_TESTNET


def resolve_api_key_private_key(value: str | None, *, testnet: bool) -> str | None:
    """
    Resolve the API key private key, preferring an explicit value over environment variables.
    """

    if value:
        return value

    env_name = ENV_API_KEY_PRIVATE_KEY_TESTNET if testnet else ENV_API_KEY_PRIVATE_KEY
    return os.environ.get(env_name)


def resolve_account_index(value: int | None, *, testnet: bool) -> int | None:
    """
    Resolve the account index from a provided value or environment variables.

    Raises
    ------
    ValueError
        If the environment variable is set but not an integer.
    """

    if value is not None:
        return value

    env_name = ENV_ACCOUNT_INDEX_TESTNET if testnet else ENV_ACCOUNT_INDEX
    env_value = os.environ.get(env_name)
    if env_value is None:
        return None

    try:
        return int(env_value)
    except ValueError as exc:
        raise ValueError(f"{env_name} must be an integer if provided") from exc
