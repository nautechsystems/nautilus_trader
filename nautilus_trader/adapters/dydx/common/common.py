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

from nautilus_trader.config import NautilusConfig
from nautilus_trader.model.objects import Price


class DYDXOrderTags(NautilusConfig, frozen=True, repr_omit_defaults=True):
    """
    Used to attach to Nautilus Order Tags for dYdX specific order parameters.
    """

    is_short_term_order: bool = True
    num_blocks_open: int = 20
    market_order_price: Price | None = None

    @property
    def value(self) -> str:
        return f"DYDXOrderTags:{self.json().decode()}"

    def __str__(self) -> str:
        return self.value
