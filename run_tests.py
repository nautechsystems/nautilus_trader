#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE file.
#  https://nautechsystems.io
# -------------------------------------------------------------------------------------------------

import unittest

loader = unittest.TestLoader()
suite = unittest.TestSuite()
suite.addTests(loader.discover('tests/'))


if __name__ == "__main__":
    runner = unittest.TextTestRunner(verbosity=1)
    result = runner.run(suite)
