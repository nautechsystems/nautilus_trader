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

from nautilus_trader.adapters.polymarket.common.constants import POLYMARKET_VENUE
from nautilus_trader.model.identifiers import InstrumentId


def get_polymarket_instrument_id(condition_id: str, token_id: str | int) -> InstrumentId:
    return InstrumentId.from_str(f"{condition_id}-{token_id}.{POLYMARKET_VENUE}")


def get_polymarket_condition_id(instrument_id: InstrumentId) -> str:
    return instrument_id.symbol.value.partition("-")[0]


def get_polymarket_token_id(instrument_id: InstrumentId) -> str:
    return instrument_id.symbol.value.partition("-")[2]
