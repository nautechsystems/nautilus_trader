# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.common.clock import TestClock
from nautilus_trader.common.enums import LogColor
from nautilus_trader.common.enums import LogLevel
from nautilus_trader.common.enums import log_level_from_str
from nautilus_trader.common.enums import log_level_to_str
from nautilus_trader.common.logging import Logger
from nautilus_trader.common.logging import LoggerAdapter


class TestLogLevel:
    @pytest.mark.parametrize(
        ("enum", "expected"),
        [
            [LogLevel.DEBUG, "DBG"],
            [LogLevel.INFO, "INF"],
            [LogLevel.WARNING, "WRN"],
            [LogLevel.ERROR, "ERR"],
        ],
    )
    def test_log_level_to_str(self, enum, expected):
        # Arrange, Act
        result = log_level_to_str(enum)

        # Assert
        assert result == expected

    @pytest.mark.parametrize(
        ("string", "expected"),
        [
            ["DBG", LogLevel.DEBUG],
            ["DEBUG", LogLevel.DEBUG],
            ["INF", LogLevel.INFO],
            ["INFO", LogLevel.INFO],
            ["WRN", LogLevel.WARNING],
            ["WARNING", LogLevel.WARNING],
            ["ERR", LogLevel.ERROR],
            ["ERROR", LogLevel.ERROR],
        ],
    )
    def test_log_level_from_str(self, string, expected):
        # Arrange, Act
        result = log_level_from_str(string)

        # Assert
        assert result == expected


class TestLoggerTests:
    def test_log_debug_messages_to_console(self):
        # Arrange
        logger = Logger(
            clock=TestClock(),
            level_stdout=LogLevel.DEBUG,
            bypass=True,
        )
        logger_adapter = LoggerAdapter(component_name="TEST_LOGGER", logger=logger)

        # Act
        logger_adapter.debug("This is a log message.")

        # Assert
        assert True  # No exceptions raised

    def test_log_info_messages_to_console(self):
        # Arrange
        logger = Logger(
            clock=TestClock(),
            level_stdout=LogLevel.INFO,
            bypass=True,
        )
        logger_adapter = LoggerAdapter(component_name="TEST_LOGGER", logger=logger)

        # Act
        logger_adapter.info("This is a log message.")

        # Assert
        assert True  # No exceptions raised

    def test_log_info_with_annotation_sends_to_stdout(self):
        # Arrange
        logger = Logger(
            clock=TestClock(),
            level_stdout=LogLevel.INFO,
            bypass=True,
        )
        logger_adapter = LoggerAdapter(component_name="TEST_LOGGER", logger=logger)

        annotations = {"my_tag": "something"}

        # Act
        logger_adapter.info("This is a log message.", annotations=annotations)

        # Assert
        assert True  # No exceptions raised

    def test_log_info_messages_to_console_with_blue_colour(self):
        # Arrange
        logger = Logger(
            clock=TestClock(),
            level_stdout=LogLevel.INFO,
            bypass=True,
        )
        logger_adapter = LoggerAdapter(component_name="TEST_LOGGER", logger=logger)

        # Act
        logger_adapter.info("This is a log message.", color=LogColor.BLUE)

        # Assert
        assert True  # No exceptions raised

    def test_log_info_messages_to_console_with_green_colour(self):
        # Arrange
        logger = Logger(
            clock=TestClock(),
            level_stdout=LogLevel.INFO,
            bypass=True,
        )
        logger_adapter = LoggerAdapter(component_name="TEST_LOGGER", logger=logger)

        # Act
        logger_adapter.info("This is a log message.", color=LogColor.GREEN)

        # Assert
        assert True  # No exceptions raised

    def test_log_warning_messages_to_console(self):
        # Arrange
        logger = Logger(
            clock=TestClock(),
            level_stdout=LogLevel.WARNING,
            bypass=True,
        )
        logger_adapter = LoggerAdapter(component_name="TEST_LOGGER", logger=logger)

        # Act
        logger_adapter.warning("This is a log message.")

        # Assert
        assert True  # No exceptions raised

    def test_log_error_messages_to_console(self):
        # Arrange
        logger = Logger(
            clock=TestClock(),
            level_stdout=LogLevel.ERROR,
            bypass=True,
        )
        logger_adapter = LoggerAdapter(component_name="TEST_LOGGER", logger=logger)

        # Act
        logger_adapter.error("This is a log message.")

        # Assert
        assert True  # No exceptions raised

    def test_log_exception_messages_to_console(self):
        # Arrange
        logger = Logger(
            clock=TestClock(),
            level_stdout=LogLevel.ERROR,
            bypass=True,
        )
        logger_adapter = LoggerAdapter(component_name="TEST_LOGGER", logger=logger)

        # Act
        logger_adapter.exception("We intentionally divided by zero!", ZeroDivisionError("Oops"))

        # Assert
        assert True  # No exceptions raised
