#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="test_logging.py" company="Invariance Pte">
#  Copyright (C) 2018 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

import unittest
import logging

from inv_trader.core.logger import Logger


class LoggerTests(unittest.TestCase):

    def setUp(self):
        print("\n")

    def test_can_log_debug_messages_to_console(self):
        # Arrange
        logger = Logger(log_level_console=logging.DEBUG)

        # Act
        logger.debug("This is a log message.")

        # Assert
        self.assertTrue(True)  # Does not raise errors.

    def test_can_log_info_messages_to_console(self):
        # Arrange
        logger = Logger(log_level_console=logging.INFO)

        # Act
        logger.info("This is a log message.")

        # Assert
        self.assertTrue(True)  # Does not raise errors.

    def test_can_log_warning_messages_to_console(self):
        # Arrange
        logger = Logger(log_level_console=logging.WARNING)

        # Act
        logger.warning("This is a log message.")

        # Assert
        self.assertTrue(True)  # Does not raise errors.

    def test_can_log_critical_messages_to_console(self):
        # Arrange
        logger = Logger(log_level_console=logging.CRITICAL)

        # Act
        logger.critical("This is a log message.")

        # Assert
        self.assertTrue(True)  # Does not raise errors.
