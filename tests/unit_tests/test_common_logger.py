# -------------------------------------------------------------------------------------------------
# <copyright file="test_common_logger.py" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

import unittest
import logging

from nautilus_trader.common.logger import TestLogger, LoggerAdapter


class TestLoggerTests(unittest.TestCase):

    def setUp(self):
        print("\n")

    def test_can_log_debug_messages_to_console(self):
        # Arrange
        logger = TestLogger(level_console=logging.DEBUG)
        logger_adapter = LoggerAdapter('TEST_LOGGER', logger)

        # Act
        logger_adapter.debug("This is a log message.")

        # Assert
        self.assertTrue(True)  # Does not raise errors.

    def test_can_log_info_messages_to_console(self):
        # Arrange
        logger = TestLogger(level_console=logging.INFO)
        logger_adapter = LoggerAdapter('TEST_LOGGER', logger)

        # Act
        logger_adapter.info("This is a log message.")

        # Assert
        self.assertTrue(True)  # Does not raise errors.

    def test_can_log_warning_messages_to_console(self):
        # Arrange
        logger = TestLogger(level_console=logging.WARNING)
        logger_adapter = LoggerAdapter('TEST_LOGGER', logger)

        # Act
        logger_adapter.warning("This is a log message.")

        # Assert
        self.assertTrue(True)  # Does not raise errors.

    def test_can_log_error_messages_to_console(self):
        # Arrange
        logger = TestLogger(level_console=logging.ERROR)
        logger_adapter = LoggerAdapter('TEST_LOGGER', logger)

        # Act
        logger_adapter.error("This is a log message.")

        # Assert
        self.assertTrue(True)  # Does not raise errors.

    def test_can_log_critical_messages_to_console(self):
        # Arrange
        logger = TestLogger(level_console=logging.CRITICAL)
        logger_adapter = LoggerAdapter('TEST_LOGGER', logger)

        # Act
        logger_adapter.critical("This is a log message.")

        # Assert
        self.assertTrue(True)  # Does not raise errors.
