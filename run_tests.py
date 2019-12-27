#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="setup.py" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

import unittest


TEST_DIRECTORIES = [
    'test_suite/unit_tests',
    'test_suite/integration_tests',
    'test_suite/performance_tests',
    'test_suite/acceptance_tests'
]

loader = unittest.TestLoader()
runner = unittest.TextTestRunner()

if __name__ == "__main__":
    for directory in TEST_DIRECTORIES:
        tests = loader.discover(directory)
        runner.run(tests)
