#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
#  You may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
#
#  Unless required by applicable law or agreed to in writing, software
#  distributed under the License is distributed on an "AS IS" BASIS,
#  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
#  See the License for the specific language governing permissions and
#  limitations under the License.
# -------------------------------------------------------------------------------------------------

"""
A utility script to remove cython and pytest artifact files from source code directories.
"""

import os
import shutil

extensions_to_clean = (".c", ".so", ".o", ".pyd", ".pyc", ".dll", ".html")


def remove_dir_if_exists(dir_name: str):
    """
    Remove the directory with the given name if it exists.

    Parameters
    ----------
    dir_name : str
        The directory name.

    """
    if os.path.exists(dir_name):
        shutil.rmtree(dir_name)


if __name__ == "__main__":
    remove_dir_if_exists("../.pytest_cache")
    remove_dir_if_exists("../__pycache__")
    remove_dir_if_exists("../build")
    for directory in ["../nautilus_trader"]:
        for root, _dirs, files in os.walk(directory):
            for name in files:
                path = os.path.join(root, name)
                if os.path.isfile(path) and path.endswith(extensions_to_clean):
                    os.remove(path)
