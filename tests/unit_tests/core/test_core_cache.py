# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2021 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.core.cache import ObjectCache
from nautilus_trader.model.identifiers import InstrumentId


class TestObjectCache:
    def test_cache_initialization(self):
        # Arrange
        cache = ObjectCache(InstrumentId, InstrumentId.from_str)

        # Act
        # Assert
        assert cache.type_key == str
        assert cache.type_value == InstrumentId
        assert cache.keys() == []

    @pytest.mark.parametrize(
        "value,ex",
        [
            [None, TypeError],
            ["", ValueError],
            [" ", ValueError],
            ["  ", ValueError],
            [1234, TypeError],
        ],
    )
    def test_get_given_none_raises_value_error(self, value, ex):
        # Arrange
        cache = ObjectCache(InstrumentId, InstrumentId.from_str)

        # Act
        # Assert
        with pytest.raises(ex):
            cache.get(value)

    def test_get_from_empty_cache(self):
        # Arrange
        cache = ObjectCache(InstrumentId, InstrumentId.from_str)
        instrument_id = "AUD/USD.SIM,FX,SPOT"

        # Act
        result = cache.get(instrument_id)

        # Assert
        assert str(result) == instrument_id
        assert cache.keys() == ["AUD/USD.SIM,FX,SPOT"]

    def test_get_from_cache(self):
        # Arrange
        cache = ObjectCache(InstrumentId, InstrumentId.from_str)
        instrument_id = "AUD/USD.SIM,FX,SPOT"
        cache.get(instrument_id)

        # Act
        cache.get(instrument_id)
        result1 = cache.get(instrument_id)
        result2 = cache.get(instrument_id)

        # Assert
        assert str(result1) == instrument_id
        assert id(result1) == id(result2)
        assert cache.keys() == ["AUD/USD.SIM,FX,SPOT"]

    def test_keys_when_cache_empty_returns_empty_list(self):
        # Arrange
        cache = ObjectCache(InstrumentId, InstrumentId.from_str)

        # Act
        result = cache.keys()

        # Assert
        assert [] == result

    def test_clear_cache(self):
        # Arrange
        cache = ObjectCache(InstrumentId, InstrumentId.from_str)
        instrument_id = "AUD/USD.SIM,FX,SPOT"
        cache.get(instrument_id)

        # Act
        cache.clear()

        # Assert
        assert cache.keys() == []
