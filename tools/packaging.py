# -------------------------------------------------------------------------------------------------
# <copyright file="packaging.py" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

import os

from typing import List
from setuptools import Extension


def parse_requirements(requirements_txt_path, strip=False) -> List[str]:
    """
    Return a list of requirement strings.

    :param requirements_txt_path: The path to the requirements.
    :param strip: If the strings should be stripped of all non-alphabet chars.
    :return: List[str].
    """
    with open(requirements_txt_path) as fp:
        requirements = fp.read().splitlines()
        if strip:
            requirements = [''.join([i for i in requirement if i.isalpha()]) for requirement in requirements]
        return requirements


def scan_directories(directories: List[str]) -> List[str]:
    """
    Return a list of all file names by recursive scan of the given directories.

    :param directories: The directory paths to scan.
    :return: List[str].
    """
    file_names = []
    for directory in directories:
        files = get_files(directory)
        for file in files:
            file_names.append(file)
    return file_names


def get_files(directory: str, files: List[str]=None) -> List[str]:
    """
    Return a list of all file names in the given directory with the given extension
    by recursive scan and appending to the given list of files.

    :param directory: The top level directory path.
    :param files: The current list of files.
    :return: List[str].
    """
    if files is None:
        files = []

    for path_name in os.listdir(directory):
        path = os.path.join(directory, path_name)
        if os.path.isfile(path):
            files.append(path)
        elif os.path.isdir(path):
            get_files(path, files)
    return files


def find_files(extension: str, directories: List[str]) -> List[str]:
    """
    Return a list of all file names with the given extension by recursive scan.

    :param extension: The extension to match.
    :param directories: The directory paths to scan.
    :return: List[str].
    """
    files = []
    for file in scan_directories(directories):
        if file.endswith(extension):
            files.append(file)
    return files


def make_extensions(directories: List[str]) -> List[Extension]:
    """
    Return a list of c extensions.

    :param directories: The directories to search for extensions.
    :return: List[Extension].
    """
    extensions = []
    for file in find_files('.pyx', directories):
        extensions.append(Extension(
            name=file.replace(os.path.sep, ".")[:-4],
            sources=[file],
            include_dirs=['.']))
    return extensions
