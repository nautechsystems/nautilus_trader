# -------------------------------------------------------------------------------------------------
# <copyright file="setup_tools.py" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
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
        files = get_files(directory)
        for file in files:
            file_names.append(file)
    return file_names


def get_files(directory: str, files: List[str]=[]) -> List[str]:
    # Recursively scan for all files
    for path_name in os.listdir(directory):
        path = os.path.join(directory, path_name)
        if os.path.isfile(path):
            files.append(path)
        elif os.path.isdir(path):
            get_files(path, files)
    return files


def find_files(extension: str, directories: List[str]) -> List[str]:
    # Recursively scan directories for all files with the given extension
    files = []
    for file in scan_directories(directories):
        if file.endswith(extension):
            files.append(file)
    return files


def get_directories(root_path: str):
    # Recursively scan directories for given root path and return the names if not ignored
    dir_names = []
    for directory in os.listdir(root_path):
        path = os.path.join(root_path, directory)
        if os.path.isdir(path):
            if not directory.startswith('__'):
                dir_names.append(directory)
    return dir_names
