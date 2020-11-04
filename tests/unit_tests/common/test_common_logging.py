# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
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

from parameterized import parameterized

from nautilus_trader.backtest.logging import TestLogger
from nautilus_trader.common.clock import TestClock
from nautilus_trader.common.logging import LogLevel
from nautilus_trader.common.logging import LogLevelParser
from nautilus_trader.common.logging import LoggerAdapter


class LogLevelParserTests(unittest.TestCase):

    @parameterized.expand([
        [LogLevel.UNDEFINED, "UNDEFINED"],
        [LogLevel.VERBOSE, "VRB"],
        [LogLevel.DEBUG, "DBG"],
        [LogLevel.INFO, "INF"],
        [LogLevel.WARNING, "WRN"],
        [LogLevel.ERROR, "ERR"],
        [LogLevel.CRITICAL, "CRT"],
        [LogLevel.FATAL, "FTL"],
    ])
    def test_log_level_to_string(self, enum, expected):
        # Arrange
        # Act
        result = LogLevelParser.to_string_py(enum)

        # Assert
        self.assertEqual(expected, result)

    @parameterized.expand([
        ["", LogLevel.UNDEFINED],
        ["UNDEFINED", LogLevel.UNDEFINED],
        ["VRB", LogLevel.VERBOSE],
        ["DBG", LogLevel.DEBUG],
        ["INF", LogLevel.INFO],
        ["ERR", LogLevel.ERROR],
        ["CRT", LogLevel.CRITICAL],
        ["FTL", LogLevel.FATAL],
    ])
    def test_log_level_from_string(self, string, expected):
        # Arrange
        # Act
        result = LogLevelParser.from_string_py(string)

        # Assert
        self.assertEqual(expected, result)


class TestLoggerTests(unittest.TestCase):

    def setUp(self):
        print("\n")


    def test_log_verbose_messages_to_console(self):
        # Arrange
        logger = TestLogger(clock=TestClock(), level_console=LogLevel.VERBOSE)
        logger_adapter = LoggerAdapter("TEST_LOGGER", logger)

        # Act
        logger_adapter.verbose("This is a log message.")

        # Assert
        self.assertTrue(True)  # Does not raise errors.

    def test_log_debug_messages_to_console(self):
        # Arrange
        logger = TestLogger(clock=TestClock(), level_console=LogLevel.DEBUG)
        logger_adapter = LoggerAdapter("TEST_LOGGER", logger)

        # Act
        logger_adapter.debug("This is a log message.")

        # Assert
        self.assertTrue(True)  # Does not raise errors.

    def test_log_info_messages_to_console(self):
        # Arrange
        logger = TestLogger(clock=TestClock(), level_console=LogLevel.INFO)
        logger_adapter = LoggerAdapter("TEST_LOGGER", logger)

        # Act
        logger_adapter.info("This is a log message.")

        # Assert
        self.assertTrue(True)  # Does not raise errors.

    def test_log_warning_messages_to_console(self):
        # Arrange
        logger = TestLogger(clock=TestClock(), level_console=LogLevel.WARNING)
        logger_adapter = LoggerAdapter("TEST_LOGGER", logger)

        # Act
        logger_adapter.warning("This is a log message.")

        # Assert
        self.assertTrue(True)  # Does not raise errors.

    def test_log_error_messages_to_console(self):
        # Arrange
        logger = TestLogger(clock=TestClock(), level_console=LogLevel.ERROR)
        logger_adapter = LoggerAdapter("TEST_LOGGER", logger)

        # Act
        logger_adapter.error("This is a log message.")

        # Assert
        self.assertTrue(True)  # Does not raise errors.

    def test_log_critical_messages_to_console(self):
        # Arrange
        logger = TestLogger(clock=TestClock(), level_console=LogLevel.CRITICAL)
        logger_adapter = LoggerAdapter("TEST_LOGGER", logger)

        # Act
        logger_adapter.critical("This is a log message.")

        # Assert
        self.assertTrue(True)  # Does not raise errors.
