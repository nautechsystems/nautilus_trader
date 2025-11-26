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

import pandas as pd
import pytest

from nautilus_trader.model.data import OrderBookDelta
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.data import TradeTick
from nautilus_trader.persistence.funcs import class_to_filename
from nautilus_trader.persistence.funcs import filename_to_class


@pytest.mark.parametrize(
    ("s", "expected"),
    [
        (TradeTick, "trade_tick"),
        (OrderBookDelta, "order_book_deltas"),
        (pd.DataFrame, "custom_data_frame"),
    ],
)
def test_class_to_filename(s, expected):
    assert class_to_filename(s) == expected


@pytest.mark.parametrize(
    ("filename", "expected"),
    [
        ("trade_tick", TradeTick),
        ("order_book_deltas", OrderBookDelta),
        ("quote_tick", QuoteTick),
        ("nonexistent_filename", None),
    ],
)
def test_filename_to_class(filename, expected):
    result = filename_to_class(filename)
    assert result == expected


def test_filename_to_class_custom_data():
    """
    Test that filename_to_class can find custom data types registered in
    _ARROW_ENCODERS.
    """
    import pyarrow as pa

    from nautilus_trader.serialization.arrow.serializer import _ARROW_ENCODERS
    from nautilus_trader.serialization.arrow.serializer import register_arrow

    # Create a mock custom data class
    class MockCustomData:
        pass

    # Register it in _ARROW_ENCODERS (temporarily)
    original_encoders = _ARROW_ENCODERS.copy()
    try:
        register_arrow(
            data_cls=MockCustomData,
            schema=pa.schema([pa.field("test", pa.string())]),
            encoder=lambda x: None,  # Mock encoder
        )

        # Test that filename_to_class can find it
        result = filename_to_class("custom_mock_custom_data")
        assert result == MockCustomData

        # Test that it returns None for non-existent custom data
        result = filename_to_class("custom_nonexistent")
        assert result is None

    finally:
        # Restore original encoders
        _ARROW_ENCODERS.clear()
        _ARROW_ENCODERS.update(original_encoders)
