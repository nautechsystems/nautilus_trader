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

from nautilus_trader.common.providers import InstrumentProvider
from nautilus_trader.model.identifiers import Venue
from tests.test_kit.stubs import TestStubs


BITMEX = Venue("BITMEX")
AUDUSD = TestStubs.audusd_id()


class TestInstrumentProvider:
    def setup(self):
        # Fixture Setup
        self.provider = InstrumentProvider()

    @pytest.mark.asyncio
    async def test_load_all_async_when_not_implemented_raises_exception(self):
        # Arrange, Act, Assert
        with pytest.raises(NotImplementedError):
            await self.provider.load_all_async()

    def test_load_all_when_not_implemented_raises_exception(self):
        # Arrange
        # Act
        # Assert
        with pytest.raises(NotImplementedError):
            self.provider.load_all()

    def test_load_when_not_implemented_raises_exception(self):
        # Arrange
        # Act
        # Assert
        with pytest.raises(NotImplementedError):
            self.provider.load(AUDUSD, {})

    def test_get_all_when_no_instruments_returns_empty_dict(self):
        # Arrange
        # Act
        result = self.provider.get_all()

        # Assert
        assert result == {}

    def test_find_when_no_instruments_returns_none(self):
        # Arrange
        # Act
        result = self.provider.find(AUDUSD)

        # Assert
        assert result is None
