#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="test_logger.py" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

import unittest
import logging

from inv_trader.common.logger import Logger, LoggerAdapter


class LoggerTests(unittest.TestCase):

    def setUp(self):
        print("\n")

    def test_can_log_debug_messages_to_console(self):
        # Arrange
        logger = Logger(level_console=logging.DEBUG)
        logger_adapter = LoggerAdapter('TEST_LOGGER', logger)

        # Act
        logger_adapter.debug("This is a log message.")

        # Assert
        self.assertTrue(True)  # Does not raise errors.

    def test_can_log_info_messages_to_console(self):
        # Arrange
        logger = Logger(level_console=logging.INFO)
        logger_adapter = LoggerAdapter('TEST_LOGGER', logger)

        # Act
        logger_adapter.info("This is a log message.")

        # Assert
        self.assertTrue(True)  # Does not raise errors.

    def test_can_log_warning_messages_to_console(self):
        # Arrange
        logger = Logger(level_console=logging.WARNING)
        logger_adapter = LoggerAdapter('TEST_LOGGER', logger)

        # Act
        logger_adapter.warning("This is a log message.")

        # Assert
        self.assertTrue(True)  # Does not raise errors.

    def test_can_log_error_messages_to_console(self):
        # Arrange
        logger = Logger(level_console=logging.ERROR)
        logger_adapter = LoggerAdapter('TEST_LOGGER', logger)

        # Act
        logger_adapter.error("This is a log message.")

        # Assert
        self.assertTrue(True)  # Does not raise errors.

    def test_can_log_critical_messages_to_console(self):
        # Arrange
        logger = Logger(level_console=logging.CRITICAL)
        logger_adapter = LoggerAdapter('TEST_LOGGER', logger)

        # Act
        logger_adapter.critical("This is a log message.")

        # Assert
        self.assertTrue(True)  # Does not raise errors.
