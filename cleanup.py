#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="cleanup.py" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

import os

types_to_clean = (".c", ".so", ".o", ".pyd", ".html")
package_name = 'nautilus_trader'
directories = [package_name,
               f'{package_name}/backtest',
               f'{package_name}/c_enums',
               f'{package_name}/common',
               f'{package_name}/core',
               f'{package_name}/model',
               f'{package_name}/network',
               f'{package_name}/portfolio',
               'test_kit']

if __name__ == "__main__":
    for directory in directories:
        for file in os.listdir(directory):
            path = os.path.join(directory, file)
            if os.path.isfile(path) and path.endswith(types_to_clean):
                os.remove(path)
