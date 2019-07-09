#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="cleanup.py" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

import os

from setup import PACKAGE_NAME

types_to_clean = (".c", ".so", ".o", ".pyd", ".html")
directories_to_include = ['test_kit']

directories_all = [PACKAGE_NAME] + directories_to_include


if __name__ == "__main__":
    for directory in directories_all:
        for file in os.walk(directory):
            path = os.path.join(directory, file)
            print(path)
            if os.path.isfile(path) and path.endswith(types_to_clean):
                os.remove(path)
