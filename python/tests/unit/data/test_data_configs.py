# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.data import DataEngineConfig
from nautilus_trader.model import BarIntervalType


def test_data_engine_config_defaults():
    config = DataEngineConfig()
    assert config.time_bars_build_with_no_updates is True
    assert config.time_bars_timestamp_on_close is True
    assert config.time_bars_skip_first_non_full_bar is False
    assert config.time_bars_interval_type == BarIntervalType.LEFT_OPEN
    assert config.time_bars_build_delay == 0
    assert config.validate_data_sequence is False
    assert config.buffer_deltas is False
    assert config.emit_quotes_from_book is False
    assert config.emit_quotes_from_book_depths is False
    assert config.debug is False


def test_data_engine_config_with_overrides():
    config = DataEngineConfig(
        time_bars_build_with_no_updates=False,
        time_bars_timestamp_on_close=False,
        time_bars_skip_first_non_full_bar=True,
        time_bars_build_delay=500,
        validate_data_sequence=True,
        buffer_deltas=True,
        emit_quotes_from_book=True,
        emit_quotes_from_book_depths=True,
        debug=True,
    )
    assert config.time_bars_build_with_no_updates is False
    assert config.time_bars_timestamp_on_close is False
    assert config.time_bars_skip_first_non_full_bar is True
    assert config.time_bars_build_delay == 500
    assert config.validate_data_sequence is True
    assert config.buffer_deltas is True
    assert config.emit_quotes_from_book is True
    assert config.emit_quotes_from_book_depths is True
    assert config.debug is True


@pytest.mark.parametrize(
    ("value", "expected"),
    [
        (BarIntervalType.LEFT_OPEN, BarIntervalType.LEFT_OPEN),
        (BarIntervalType.RIGHT_OPEN, BarIntervalType.RIGHT_OPEN),
        ("left-open", BarIntervalType.LEFT_OPEN),
        ("right-open", BarIntervalType.RIGHT_OPEN),
        ("LEFT_OPEN", BarIntervalType.LEFT_OPEN),
        ("RIGHT_OPEN", BarIntervalType.RIGHT_OPEN),
        ("left_open", BarIntervalType.LEFT_OPEN),
        ("Left-Open", BarIntervalType.LEFT_OPEN),
    ],
)
def test_data_engine_config_bar_interval_type_coercion(value, expected):
    config = DataEngineConfig(time_bars_interval_type=value)
    assert config.time_bars_interval_type == expected


def test_data_engine_config_invalid_interval_type_string():
    with pytest.raises(ValueError, match="invalid `time_bars_interval_type`"):
        DataEngineConfig(time_bars_interval_type="nonsense")


def test_data_engine_config_invalid_interval_type_non_string():
    with pytest.raises(ValueError, match="must be a string or BarIntervalType"):
        DataEngineConfig(time_bars_interval_type=123)


def test_data_engine_config_repr():
    config = DataEngineConfig()
    assert "DataEngineConfig" in repr(config)
