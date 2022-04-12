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

from typing import Dict

import pydantic
from pydantic import ConstrainedStr


class DataEngineConfig(pydantic.BaseModel):
    """
    Configuration for ``DataEngine`` instances.

    Parameters
    ----------
    debug : bool
        If debug mode is active (will provide extra debug logging).
    """

    debug: bool = False


class RiskEngineConfig(pydantic.BaseModel):
    """
    Configuration for ``RiskEngine`` instances.

    Parameters
    ----------
    bypass : bool
        If True then all risk checks are bypassed (will still check for duplicate IDs).
    max_order_rate : str, default 100/00:00:01
        The maximum order rate per timedelta.
    max_notional_per_order : Dict[str, str]
        The maximum notional value of an order per instrument ID.
        The value should be a valid decimal format.
    debug : bool
        If debug mode is active (will provide extra debug logging).
    """

    bypass: bool = False
    max_order_rate: ConstrainedStr = ConstrainedStr("100/00:00:01")
    max_notional_per_order: Dict[str, str] = {}
    debug: bool = False


class ExecEngineConfig(pydantic.BaseModel):
    """
    Configuration for ``ExecutionEngine`` instances.

    Parameters
    ----------
    load_cache : bool, default True
        If the cache should be loaded on initialization.
    allow_cash_positions : bool, default False
        If unleveraged spot cash assets should track positions.
    debug : bool
        If debug mode is active (will provide extra debug logging).
    """

    load_cache: bool = True
    allow_cash_positions: bool = False
    debug: bool = False
