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

from nautilus_trader.adapters.betfair.data_types import BetfairStartingPrice
from nautilus_trader.adapters.betfair.data_types import BetfairTicker
from nautilus_trader.core.inspect import is_nautilus_class
from nautilus_trader.model.data import OrderBookDelta
from nautilus_trader.model.data import TradeTick
from nautilus_trader.model.events import OrderAccepted


@pytest.mark.parametrize(
    ("cls", "is_nautilus"),
    [
        (OrderBookDelta, True),
        (TradeTick, True),
        (OrderAccepted, True),
        (BetfairStartingPrice, False),  # BetfairStartingPrice is an adapter specific type
        (BetfairTicker, False),  # BetfairTicker is an adapter specific type
        (pd.DataFrame, False),
    ],
)
def test_is_nautilus_class(cls, is_nautilus):
    # Arrange, Act, Assert
    assert is_nautilus_class(cls=cls) is is_nautilus
