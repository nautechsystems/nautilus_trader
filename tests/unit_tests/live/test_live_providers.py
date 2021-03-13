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

import asyncio
import unittest

from nautilus_trader.live.providers import InstrumentProvider
from nautilus_trader.model.identifiers import Venue
from tests.test_kit.stubs import TestStubs

BITMEX = Venue("BITMEX")
AUDUSD = TestStubs.audusd_id()


class LiveProvidersTests(unittest.TestCase):

    def setUp(self):
        # Fixture Setup

        self.provider = InstrumentProvider(
            venue=BITMEX,
            load_all=False,
        )

    def test_load_all_async_when_not_implemented_raises_exception(self):
        # Fresh isolated loop testing pattern
        loop = asyncio.new_event_loop()
        asyncio.set_event_loop(loop)

        async def run_test():
            # Arrange
            # Act
            # Assert
            try:
                await self.provider.load_all_async()
            except NotImplementedError as ex:
                self.assertEqual(NotImplementedError, type(ex))

        loop.run_until_complete(run_test())

    def test_load_all_when_not_implemented_raises_exception(self):
        # Arrange
        # Act
        # Assert
        self.assertRaises(NotImplementedError, self.provider.load_all)

    def test_get_all_when_not_implemented_raises_exception(self):
        # Arrange
        # Act
        # Assert
        self.assertRaises(NotImplementedError, self.provider.get_all)

    def test_get_when_not_implemented_raises_exception(self):
        # Arrange
        # Act
        # Assert
        self.assertRaises(NotImplementedError, self.provider.get, AUDUSD)

    def test_currency_when_not_implemented_raises_exception(self):
        # Arrange
        # Act
        # Assert
        self.assertRaises(NotImplementedError, self.provider.currency, "BTC")
