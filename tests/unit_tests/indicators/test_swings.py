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

from nautilus_trader.indicators.swings import Swings
from nautilus_trader.model.data import Bar
from nautilus_trader.model.data import BarSpecification
from nautilus_trader.model.data import BarType
from nautilus_trader.model.enums import BarAggregation
from nautilus_trader.model.enums import PriceType
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.test_kit.stubs.data import UNIX_EPOCH
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs


AUDUSD_SIM = TestIdStubs.audusd_id()
ONE_MIN_BID = BarSpecification(1, BarAggregation.MINUTE, PriceType.BID)
AUDUSD_1_MIN_BID = BarType(AUDUSD_SIM, ONE_MIN_BID)


class TestSwings:
    def setup(self):
        # Fixture Setup
        self.swings = Swings(3)

    def test_name_returns_expected_name(self):
        # Arrange, Act, Assert
        assert self.swings.name == "Swings"

    def test_str_repr_returns_expected_string(self):
        # Arrange, Act, Assert
        assert str(self.swings) == "Swings(3)"
        assert repr(self.swings) == "Swings(3)"

    def test_instantiate_returns_expected_property_values(self):
        # Arrange, Act, Assert
        assert self.swings.period == 3
        assert self.swings.initialized is False
        assert self.swings.direction == 0
        assert self.swings.changed is False
        assert self.swings.since_high == 0
        assert self.swings.since_low == 0

    def test_handle_bar(self):
        # Arrange
        bar = Bar(
            AUDUSD_1_MIN_BID,
            Price.from_str("1.00001"),
            Price.from_str("1.00004"),
            Price.from_str("1.00000"),
            Price.from_str("1.00003"),
            Quantity.from_int(100_000),
            0,
            0,
        )

        # Act
        self.swings.handle_bar(bar)

        # Assert
        assert self.swings.has_inputs

    def test_determine_swing_high(self):
        # Arrange
        self.swings.update_raw(1.00010, 1.00000, UNIX_EPOCH)
        self.swings.update_raw(1.00030, 1.00010, UNIX_EPOCH)
        self.swings.update_raw(1.00040, 1.00020, UNIX_EPOCH)
        self.swings.update_raw(1.00050, 1.00030, UNIX_EPOCH)
        self.swings.update_raw(1.00060, 1.00040, UNIX_EPOCH)
        self.swings.update_raw(1.00050, 1.00040, UNIX_EPOCH)

        # Act, Assert
        assert self.swings.direction == 1
        assert self.swings.high_price == 1.0006

    def test_determine_swing_low(self):
        # Arrange
        self.swings.update_raw(1.00100, 1.00080, UNIX_EPOCH)
        self.swings.update_raw(1.00080, 1.00060, UNIX_EPOCH)
        self.swings.update_raw(1.00060, 1.00040, UNIX_EPOCH)
        self.swings.update_raw(1.00040, 1.00030, UNIX_EPOCH)
        self.swings.update_raw(1.00020, 1.00010, UNIX_EPOCH)
        self.swings.update_raw(1.00020, 1.00020, UNIX_EPOCH)

        # Act, Assert
        assert self.swings.direction == -1
        assert self.swings.low_price == 1.0001

    def test_swing_change_high_to_low(self):
        # Arrange
        self.swings.update_raw(1.00010, 1.00000, UNIX_EPOCH)
        self.swings.update_raw(1.00020, 1.00010, UNIX_EPOCH)
        self.swings.update_raw(1.00030, 1.00020, UNIX_EPOCH)
        self.swings.update_raw(1.00040, 1.00030, UNIX_EPOCH)
        self.swings.update_raw(1.00050, 1.00040, UNIX_EPOCH)
        self.swings.update_raw(1.00060, 1.00050, UNIX_EPOCH)
        self.swings.update_raw(1.00050, 1.00040, UNIX_EPOCH)

        # Act, Assert
        assert self.swings.direction == -1
        assert self.swings.changed
        assert self.swings.since_low == 0
        assert self.swings.since_high == 1
        assert self.swings.length == 0  # Just changed

    def test_swing_change_low_to_high(self):
        # Arrange
        self.swings.update_raw(1.00090, 1.00080, UNIX_EPOCH)
        self.swings.update_raw(1.00080, 1.00070, UNIX_EPOCH)
        self.swings.update_raw(1.00070, 1.00060, UNIX_EPOCH)
        self.swings.update_raw(1.00060, 1.00050, UNIX_EPOCH)
        self.swings.update_raw(1.00050, 1.00040, UNIX_EPOCH)
        self.swings.update_raw(1.00060, 1.00050, UNIX_EPOCH)

        # Act, Assert
        assert self.swings.direction == 1
        assert self.swings.changed
        assert self.swings.since_high == 0
        assert self.swings.since_low == 1
        assert self.swings.length == 0  # Just changed

    def test_swing_changes(self):
        # Arrange
        self.swings.update_raw(1.00010, 1.00000, UNIX_EPOCH)
        self.swings.update_raw(1.00020, 1.00010, UNIX_EPOCH)
        self.swings.update_raw(1.00030, 1.00020, UNIX_EPOCH)
        self.swings.update_raw(1.00040, 1.00030, UNIX_EPOCH)
        self.swings.update_raw(1.00050, 1.00040, UNIX_EPOCH)
        self.swings.update_raw(1.00060, 1.00050, UNIX_EPOCH)
        self.swings.update_raw(1.00050, 1.00040, UNIX_EPOCH)
        self.swings.update_raw(1.00040, 1.00030, UNIX_EPOCH)
        self.swings.update_raw(1.00030, 1.00020, UNIX_EPOCH)
        self.swings.update_raw(1.00020, 1.00010, UNIX_EPOCH)
        self.swings.update_raw(1.00010, 1.00000, UNIX_EPOCH)
        self.swings.update_raw(1.00020, 1.00010, UNIX_EPOCH)
        self.swings.update_raw(1.00030, 1.00020, UNIX_EPOCH)
        self.swings.update_raw(1.00040, 1.00030, UNIX_EPOCH)

        # Act, Assert
        assert self.swings.direction == 1
        assert self.swings.since_low == 3
        assert self.swings.since_high == 0
        assert self.swings.length == 0.00039999999999995595
        assert self.swings.initialized

    def test_reset(self):
        # Arrange
        self.swings.update_raw(1.00100, 1.00080, UNIX_EPOCH)
        self.swings.update_raw(1.00080, 1.00060, UNIX_EPOCH)
        self.swings.update_raw(1.00060, 1.00040, UNIX_EPOCH)

        # Act
        self.swings.reset()

        # Assert
        assert self.swings.has_inputs == 0
        assert self.swings.direction == 0
