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
package_name = 'inv_trader'
directories = [package_name,
               f'{package_name}/backtest',
               f'{package_name}/common',
               f'{package_name}/core',
               f'{package_name}/enums',
               f'{package_name}/model',
               f'{package_name}/portfolio',
               'test_kit']

if __name__ == "__main__":
    for directory in directories:
        for file in os.listdir(directory):
            path = os.path.join(directory, file)
            if os.path.isfile(path) and path.endswith(cython_extension):
                os.remove(path)
