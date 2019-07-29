# -------------------------------------------------------------------------------------------------
# <copyright file="cleanup.py" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

import os


extensions_to_clean = ('.c', '.so', '.o', '.pyd', '.html')
directories = ['nautilus_trader', 'test_kit']

if __name__ == '__main__':
    for directory in directories:
        for root, dirs, files in os.walk(directory):
            for name in files:
                path = os.path.join(root, name)
                if os.path.isfile(path) and path.endswith(extensions_to_clean):
                    os.remove(path)
