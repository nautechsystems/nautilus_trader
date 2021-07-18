#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2021 Nautech Systems Pty Ltd. All rights reserved.
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
A utility script to remove specified directories and files.
"""

import os
import shutil


print("Running cleanup.py...")
ROOT_DIR = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
print(f"ROOT_DIR={ROOT_DIR}")


FILES_TO_REMOVE = {
    ".coverage",
    "coverage.xml",
    "dump.rdb",
}

DIRS_TO_REMOVE = {
    ".nox",
    ".profile",
    ".pytest_cache",
    "__pycache__",
    "build",
    "dist",
    "docs/build",
    "coverage.xml",
    "dump.rdb",
}

DIRS_TO_CLEAN = {
    "/nautilus_trader/",
    "/examples/",
    "/tests/",
}

EXTENSIONS_TO_CLEAN = (
    ".dll",
    ".html",
    ".o",
    ".prof",
    ".pyd",
    ".pyc",
    ".so",
)


def remove_benchmarks_dirs():
    for root, _dirs, _files in os.walk(ROOT_DIR):
        if root.endswith(".benchmarks"):
            shutil.rmtree(root, ignore_errors=True)
            print(f"Removed dir: {root}")


def remove_specific_dirs():
    for target in DIRS_TO_REMOVE:
        path = os.path.join(ROOT_DIR, target)
        if os.path.isdir(path):
            shutil.rmtree(path, ignore_errors=True)
            print(f"Removed dir: {path}")


def remove_specific_files():
    for target in FILES_TO_REMOVE:
        path = os.path.join(ROOT_DIR, target)
        if os.path.isfile(path):
            os.remove(path)
            print(f"Removed: {path}")


def clean_specific_directories():
    removed_count = 0
    for directory in DIRS_TO_CLEAN:
        for root, _dirs, files in os.walk(ROOT_DIR + directory):
            for name in files:
                path = os.path.join(root, name)
                if os.path.isfile(path) and path.endswith(EXTENSIONS_TO_CLEAN):
                    os.remove(path)
                    removed_count += 1

    print(f"Removed {removed_count} discrete files by extension.")


if __name__ == "__main__":
    remove_benchmarks_dirs()
    remove_specific_dirs()
    remove_specific_files()
    clean_specific_directories()
