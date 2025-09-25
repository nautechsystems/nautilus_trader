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

from enum import Enum
from enum import unique


@unique
class HyperliquidProductType(str, Enum):
    """
    Supported Hyperliquid product types for instrument discovery.
    """

    SPOT = "spot"
    PERP = "perp"

    @property
    def is_spot(self) -> bool:
        return self is HyperliquidProductType.SPOT

    @property
    def is_perp(self) -> bool:
        return self is HyperliquidProductType.PERP


DEFAULT_PRODUCT_TYPES = frozenset({HyperliquidProductType.SPOT, HyperliquidProductType.PERP})
