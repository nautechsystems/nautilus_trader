# -------------------------------------------------------------------------------------------------
# <copyright file="setup_tools.py" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

import os
import re

from typing import List
from setuptools import Extension


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


def check_file_headers(directories: List[str], ignore: List[str], author: str) -> None:
    # Check file headers
    files = scan_directories(directories)
    checked_extensions = set()
    for file in files:
        if os.path.isfile(file):
            file_extension = os.path.splitext(file)[1]
            if file_extension not in ignore:
                checked_extensions.add(os.path.splitext(file)[1])
                with open(file, 'r') as open_file:
                    source_code = (open_file.read())
                    expected_file_name = file.split('/')[-1]
                    result = re.findall(r'\"(.+?)\"', source_code)
                    file_name = result[0]
                    company = result[1]
                    if file_name != expected_file_name:
                        raise ValueError(f"The file header for {file} is incorrect"
                                         f" (file= should be '{expected_file_name}' was '{file_name}')")
                    if company != author:
                        raise ValueError(f"The file header for {file} is incorrect"
                                         f" (company= should be '{author}' was '{company}')")

    print(f"Checked headers for extensions; {checked_extensions} (file name and company name all OK).")


def find_pyx_files(directories: List[str]) -> List[str]:
    # Recursively scan directories for all files to cythonize
    pyx_files = []
    for file in scan_directories(directories):
        if file.endswith('.pyx'):
            pyx_files.append(file)
    return pyx_files


def make_cython_extensions(directories: List[str]) -> [Extension]:
    # Generate an Extension object from its dotted name
    extensions = []
    for file in find_pyx_files(directories):
        if file.endswith('.pyx'):
            extensions.append(Extension(
                name=file.replace(os.path.sep, ".")[:-4],
                sources=[file],
                include_dirs=['.'],
                define_macros=[('CYTHON_TRACE', '1')]))
    return extensions
