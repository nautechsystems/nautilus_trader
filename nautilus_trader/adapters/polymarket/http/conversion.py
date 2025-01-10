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

from py_clob_client.clob_types import OrderType

from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.enums import time_in_force_to_str


def convert_tif_to_polymarket_order_type(time_in_force) -> str:
    match time_in_force:
        case TimeInForce.GTC:
            return OrderType.GTC
        case TimeInForce.GTD:
            return OrderType.GTD
        case TimeInForce.FOK:
            return OrderType.FOK
        case _:
            time_in_force_str = time_in_force_to_str(time_in_force)
            raise ValueError(f"invalid `TimeInForce` for conversion, was {time_in_force_str}")
