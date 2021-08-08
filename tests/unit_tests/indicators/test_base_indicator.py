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

from nautilus_trader.indicators.base.indicator import Indicator
from tests.test_kit.providers import TestInstrumentProvider
from tests.test_kit.stubs import TestStubs


AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy("AUD/USD")


class TestIndicator:
    def test_handle_quote_tick_raises_not_implemented_error(self):
        # Arrange
        indicator = Indicator([])

        tick = TestStubs.quote_tick_5decimal(AUDUSD_SIM.id)

        # Act
        # Assert
        with pytest.raises(NotImplementedError):
            indicator.handle_quote_tick(tick)

    def test_handle_trade_tick_raises_not_implemented_error(self):
        # Arrange
        indicator = Indicator([])

        tick = TestStubs.trade_tick_5decimal(AUDUSD_SIM.id)

        # Act
        # Assert
        with pytest.raises(NotImplementedError):
            indicator.handle_trade_tick(tick)

    def test_handle_bar_raises_not_implemented_error(self):
        # Arrange
        indicator = Indicator([])

        bar = TestStubs.bar_5decimal()

        # Act
        # Assert
        with pytest.raises(NotImplementedError):
            indicator.handle_bar(bar)

    def test_reset_raises_not_implemented_error(self):
        # Arrange
        indicator = Indicator([])

        # Act
        # Assert
        with pytest.raises(NotImplementedError):
            indicator.reset()
