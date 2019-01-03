#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="cleanup.py" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

import os

cython_extension = (".c", ".so", ".o", ".pyd")

if __name__ == "__main__":
    for file in os.listdir('inv_trader'):
        path = os.path.join('inv_trader', file)
        if os.path.isfile(path) and path.endswith(cython_extension):
            os.remove(path)

    for file in os.listdir('inv_trader/core'):
        path = os.path.join('inv_trader/core', file)
        if os.path.isfile(path) and path.endswith(cython_extension):
            os.remove(path)

    for file in os.listdir('inv_trader/model'):
        path = os.path.join('inv_trader/model', file)
        if os.path.isfile(path) and path.endswith(cython_extension):
            os.remove(path)

    for file in os.listdir('test_kit'):
        path = os.path.join('test_kit', file)
        if os.path.isfile(path) and path.endswith(cython_extension):
            os.remove(path)
