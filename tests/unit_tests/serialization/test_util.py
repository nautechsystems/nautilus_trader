# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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
from nautilus_trader.model.data import TradeTick
from nautilus_trader.serialization.arrow.util import camel_to_snake_case
from nautilus_trader.serialization.arrow.util import class_to_filename
from nautilus_trader.serialization.arrow.util import clean_key


@pytest.mark.parametrize(
    ("s", "expected"),
    [
        ("BSPOrderBookDelta", "bsp_order_book_delta"),
        ("OrderBookDelta", "order_book_delta"),
        ("TradeTick", "trade_tick"),
    ],
)
def test_camel_to_snake_case(s, expected):
    assert camel_to_snake_case(s) == expected


@pytest.mark.parametrize(
    ("s", "expected"),
    [
        ("Instrument\\ID:hello", "Instrument-ID-hello"),
    ],
)
def test_clean_key(s, expected):
    assert clean_key(s) == expected


@pytest.mark.parametrize(
    ("s", "expected"),
    [
        (TradeTick, "trade_tick"),
        (OrderBookDelta, "order_book_delta"),
        (pd.DataFrame, "genericdata_data_frame"),
    ],
)
def test_class_to_filename(s, expected):
    assert class_to_filename(s) == expected
