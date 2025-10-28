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

import pytest

from nautilus_trader.adapters.polymarket.common.constants import POLYMARKET_VENUE
from nautilus_trader.adapters.polymarket.common.symbol import get_polymarket_condition_id
from nautilus_trader.adapters.polymarket.common.symbol import get_polymarket_instrument_id
from nautilus_trader.adapters.polymarket.common.symbol import get_polymarket_token_id
from nautilus_trader.model.identifiers import InstrumentId


class TestPolymarketSymbol:
    def test_get_polymarket_instrument_id(self):
        condition_id = "0xdd22472e552920b8438158ea7238bfadfa4f736aa4cee91a6b86c39ead110917"
        token_id = "21742633143463906290569050155826241533067272736897614950488156847949938836455"

        result = get_polymarket_instrument_id(condition_id, token_id)

        expected = InstrumentId.from_str(f"{condition_id}-{token_id}.{POLYMARKET_VENUE}")
        assert result == expected

    def test_get_polymarket_condition_id_valid(self):
        condition_id = "0xdd22472e552920b8438158ea7238bfadfa4f736aa4cee91a6b86c39ead110917"
        token_id = "21742633143463906290569050155826241533067272736897614950488156847949938836455"
        instrument_id = get_polymarket_instrument_id(condition_id, token_id)

        result = get_polymarket_condition_id(instrument_id)

        assert result == condition_id

    def test_get_polymarket_token_id_valid(self):
        condition_id = "0xdd22472e552920b8438158ea7238bfadfa4f736aa4cee91a6b86c39ead110917"
        token_id = "21742633143463906290569050155826241533067272736897614950488156847949938836455"
        instrument_id = get_polymarket_instrument_id(condition_id, token_id)

        result = get_polymarket_token_id(instrument_id)

        assert result == token_id

    def test_get_polymarket_condition_id_no_dash_raises_error(self):
        instrument_id = InstrumentId.from_str(f"invalid_no_dash.{POLYMARKET_VENUE}")

        with pytest.raises(ValueError, match="Invalid Polymarket instrument ID format"):
            get_polymarket_condition_id(instrument_id)

    def test_get_polymarket_token_id_no_dash_raises_error(self):
        instrument_id = InstrumentId.from_str(f"invalid_no_dash.{POLYMARKET_VENUE}")

        with pytest.raises(ValueError, match="Invalid Polymarket instrument ID format"):
            get_polymarket_token_id(instrument_id)

    def test_get_polymarket_condition_id_too_many_dashes_raises_error(self):
        instrument_id = InstrumentId.from_str(f"too-many-dashes.{POLYMARKET_VENUE}")

        with pytest.raises(ValueError, match="Invalid Polymarket instrument ID format"):
            get_polymarket_condition_id(instrument_id)

    def test_get_polymarket_token_id_too_many_dashes_raises_error(self):
        instrument_id = InstrumentId.from_str(f"too-many-dashes.{POLYMARKET_VENUE}")

        with pytest.raises(ValueError, match="Invalid Polymarket instrument ID format"):
            get_polymarket_token_id(instrument_id)

    def test_get_polymarket_condition_id_missing_condition_raises_error(self):
        instrument_id = InstrumentId.from_str(f"-token_id.{POLYMARKET_VENUE}")

        with pytest.raises(ValueError, match="Invalid Polymarket instrument ID format"):
            get_polymarket_condition_id(instrument_id)

    def test_get_polymarket_token_id_missing_token_raises_error(self):
        instrument_id = InstrumentId.from_str(f"condition_id-.{POLYMARKET_VENUE}")

        with pytest.raises(ValueError, match="Invalid Polymarket instrument ID format"):
            get_polymarket_token_id(instrument_id)
