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


class RiskEngineConfig(pydantic.BaseModel):
    """
    Configuration for ``RiskEngine`` instances.

    Parameters
    ----------
    bypass : bool
        If True then all risk checks are bypassed (will still check for duplicate IDs).
    max_order_rate : str, default=100/00:00:01
        The maximum order rate per timedelta.
    max_notional_per_order : Dict[str, str]
        The maximum notional value of an order per instrument ID.
        The value should be a valid decimal format.
    """

    bypass: bool = False
    max_order_rate: ConstrainedStr = ConstrainedStr("100/00:00:01")
    max_notional_per_order: Dict[str, str] = {}
