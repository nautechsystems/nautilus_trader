#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE file.
#  https://nautechsystems.io
# -------------------------------------------------------------------------------------------------

"""
A utility script to remove cython and pytest artifact files from the source code
directories.
"""

import os
import shutil


extensions_to_clean = ('.c', '.so', '.o', '.pyd', '.pyc', '.dll', '.html')


def remove_dir_if_exists(dir_name: str):
    if os.path.exists(dir_name):
        shutil.rmtree(dir_name)


if __name__ == '__main__':
    remove_dir_if_exists('.pytest_cache')
    remove_dir_if_exists('__pycache__')
    remove_dir_if_exists('build')
    for directory in ['nautilus_trader']:
        for root, dirs, files in os.walk(directory):
            for name in files:
                path = os.path.join(root, name)
                if os.path.isfile(path) and path.endswith(extensions_to_clean):
                    os.remove(path)
