import pytest

from nautilus_trader.common.component import Logger
from nautilus_trader.common.enums import LogColor
from nautilus_trader.common.enums import LogLevel
from nautilus_trader.common.enums import log_level_from_str
from nautilus_trader.common.enums import log_level_to_str


class TestLogLevel:
    @pytest.mark.parametrize(
        ("enum", "expected"),
        [
            [LogLevel.TRACE, "TRACE"],
            [LogLevel.DEBUG, "DEBUG"],
            [LogLevel.INFO, "INFO"],
            [LogLevel.WARNING, "WARNING"],
            [LogLevel.ERROR, "ERROR"],
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
            ["TRACE", LogLevel.TRACE],
            ["DEBUG", LogLevel.DEBUG],
            ["INFO", LogLevel.INFO],
            ["WARN", LogLevel.WARNING],
            ["WARNING", LogLevel.WARNING],
            ["ERROR", LogLevel.ERROR],
        ],
    )
    def test_log_level_from_str(self, string, expected):
        # Arrange, Act
        result = log_level_from_str(string)

        # Assert
        assert result == expected


class TestLoggerTests:
    def test_name(self):
        # Arrange
        name = "TEST_LOGGER"
        logger = Logger(name=name)

        # Act, Assert
        assert logger.name == name

    def test_log_debug_messages_to_console(self):
        # Arrange
        logger = Logger(name="TEST_LOGGER")

        # Act
        logger.debug("This is a DEBUG log message.")

        # Assert
        assert True  # No exceptions raised

    def test_log_info_messages_to_console(self):
        # Arrange
        logger = Logger(name="TEST_LOGGER")

        # Act
        logger.info("This is an INFO log message.")

        # Assert
        assert True  # No exceptions raised

    def test_log_info_messages_to_console_with_blue_colour(self):
        # Arrange
        logger = Logger(name="TEST_LOGGER")

        # Act
        logger.info("This is an INFO log message.", color=LogColor.BLUE)

        # Assert
        assert True  # No exceptions raised

    def test_log_info_messages_to_console_with_green_colour(self):
        # Arrange
        logger = Logger(name="TEST_LOGGER")

        # Act
        logger.info("This is an INFO log message.", color=LogColor.GREEN)

        # Assert
        assert True  # No exceptions raised

    def test_log_warning_messages_to_console(self):
        # Arrange
        logger = Logger(name="TEST_LOGGER")

        # Act
        logger.warning("This is a WARNING log message.")

        # Assert
        assert True  # No exceptions raised

    def test_log_error_messages_to_console(self):
        # Arrange
        logger = Logger(name="TEST_LOGGER")

        # Act
        logger.error("This is an ERROR log message.")

        # Assert
        assert True  # No exceptions raised

    def test_log_exception_messages_to_console(self):
        # Arrange
        logger = Logger(name="TEST_LOGGER")

        # Act
        logger.exception("We intentionally divided by zero!", ZeroDivisionError("Oops"))

        # Assert
        assert True  # No exceptions raised
