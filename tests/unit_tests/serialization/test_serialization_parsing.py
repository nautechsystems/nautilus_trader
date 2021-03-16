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

import unittest

from nautilus_trader.serialization.parsing import ObjectParser
from tests.test_kit.stubs import UNIX_EPOCH


class SerializationFunctionTests(unittest.TestCase):
    def test_convert_datetime_to_str_from_none(self):
        # Arrange
        # Act
        result = ObjectParser.datetime_to_str_py(None)

        # Assert
        self.assertEqual("None", result)

    def test_convert_datetime_to_str(self):
        # Arrange
        # Act
        result = ObjectParser.datetime_to_str_py(UNIX_EPOCH)

        # Assert
        self.assertEqual("0", result)

    def test_convert_string_to_time_from_datetime(self):
        # Arrange
        # Act
        result = ObjectParser.string_to_datetime_py("0")

        # Assert
        self.assertEqual(UNIX_EPOCH, result)

    def test_convert_string_to_time_from_none(self):
        # Arrange
        # Act
        result = ObjectParser.string_to_datetime_py("None")

        # Assert
        self.assertEqual(None, result)
