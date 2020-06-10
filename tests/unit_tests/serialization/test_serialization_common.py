# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU General Public License Version 3.0 (the "License");
#  you may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/gpl-3.0.en.html
#
#  Unless required by applicable law or agreed to in writing, software
#  distributed under the License is distributed on an "AS IS" BASIS,
#  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
#  See the License for the specific language governing permissions and
#  limitations under the License.
# -------------------------------------------------------------------------------------------------

import unittest

from nautilus_trader.model.objects import Price
from nautilus_trader.serialization.common import *
from nautilus_trader.serialization.serializers import *

from tests.test_kit.stubs import UNIX_EPOCH


class SerializationFunctionTests(unittest.TestCase):

    def test_can_convert_price_to_string_from_none(self):
        # Arrange
        # Act
        result = convert_price_to_string(None)

        # Assert
        self.assertEqual('None', result)

    def test_can_convert_price_to_string_from_decimal(self):
        # Arrange
        # Act
        result = convert_price_to_string(Price(1.00000, 5))

        # Assert
        self.assertEqual('1.00000', result)

    def test_can_convert_string_to_price_from_none(self):
        # Arrange
        # Act
        result = convert_string_to_price('None')

        # Assert
        self.assertEqual(None, result)

    def test_can_convert_string_to_price_from_decimal(self):
        # Arrange
        # Act
        result = convert_string_to_price('1.00000')

        # Assert
        self.assertEqual(Price(1.00000, 5), result)

    def test_can_convert_datetime_to_string_from_none(self):
        # Arrange
        # Act
        result = convert_datetime_to_string(None)

        # Assert
        self.assertEqual('None', result)

    def test_can_convert_datetime_to_string(self):
        # Arrange
        # Act
        result = convert_datetime_to_string(UNIX_EPOCH)

        # Assert
        self.assertEqual('1970-01-01T00:00:00.000Z', result)

    def test_can_convert_string_to_time_from_datetime(self):
        # Arrange
        # Act
        result = convert_string_to_datetime('1970-01-01T00:00:00.000Z')

        # Assert
        self.assertEqual(UNIX_EPOCH, result)

    def test_can_convert_string_to_time_from_none(self):
        # Arrange
        # Act
        result = convert_string_to_datetime('None')

        # Assert
        self.assertEqual(None, result)
