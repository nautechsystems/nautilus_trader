# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2022 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.adapters.interactive_brokers.config import InteractiveBrokersInstrumentFilter


pytestmark = pytest.mark.no_ci


@pytest.mark.parametrize(
    "value",
    [
        "AAPL.AMEX",
        "CLZ3.NYMEX",
        "EUR/USD.IDEALPRO",
        "TSLA230120C00100000.MIAX",
    ],
)
def test_from_instrument_id(self, value):
    # Arrange
    # Act
    filt = InteractiveBrokersInstrumentFilter.from_instrument_id(value)

    # Assert
    assert filt.validate()
