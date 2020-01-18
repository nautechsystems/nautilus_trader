#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="setup.py" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

import os
import unittest

ABS_PATH = os.path.dirname(os.path.abspath(__file__))

TEST_DIRECTORIES = [
     ABS_PATH + '/test_suite/unit_tests',
     ABS_PATH + '/test_suite/integration_tests',
     ABS_PATH + '/test_suite/performance_tests',
     ABS_PATH + '/test_suite/acceptance_tests'
]

loader = unittest.TestLoader()
runner = unittest.TextTestRunner()


if __name__ == "__main__":
    for directory in TEST_DIRECTORIES:
        print(directory)
        tests = loader.discover(directory)
        runner.run(tests)
