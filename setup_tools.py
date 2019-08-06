# -------------------------------------------------------------------------------------------------
# <copyright file="setup_tools.py" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

import os

from typing import List


def scan_directories(directories: List[str]) -> List[str]:
    # Recursively scan given directories for all files
    file_names = []
    for directory in directories:
        files = find_files(directory)
        for file in files:
            file_names.append(file)
    return file_names


def find_files(directory: str, files: List[str]=[]) -> List[str]:
    # Recursively scan for all files
    for path_name in os.listdir(directory):
        path = os.path.join(directory, path_name)
        if os.path.isfile(path):
            files.append(path)
        elif os.path.isdir(path):
            find_files(path, files)
    return files


def find_pyx_files(directories: List[str]) -> List[str]:
    # Recursively scan directories for all files to cythonize
    pyx_files = []
    for file in scan_directories(directories):
        if file.endswith('.pyx'):
            pyx_files.append(file)
    return pyx_files
