# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE file.
#  https://nautechsystems.io
# -------------------------------------------------------------------------------------------------

import os

from setuptools import Extension


def parse_requirements(requirements_txt_path: str, strip: bool=False) -> [str]:
    """
    Return a list of requirement strings.

    Parameters
    ----------
    requirements_txt_path : str
        The path to the requirements.
    strip : bool
        If the strings should be stripped of all non-alphabet chars.

    Returns
    -------
    list of str

    """
    with open(requirements_txt_path) as fp:
        requirements = fp.read().splitlines()
        if strip:
            requirements = [''.join([i for i in requirement if i.isalpha()]) for requirement in requirements]
        return requirements


def scan_directories(directories: list) -> list:
    """
    Return a list of all file names by recursive scan of the given directories.

    Parameters
    ----------
    directories : List[str]
        The directory paths to scan.

    Returns
    -------
    list of str
        The list of file name strings.

    """
    file_names = []
    for directory in directories:
        files = get_files(directory)
        for file in files:
            file_names.append(file)
    return file_names


def get_files(directory: str, files: list=None) -> list:
    """
    Return a list of all file names in the given directory with the given extension
    by recursive scan and appending to the given list of files.

    Parameters
    ----------
    directory : str
        The top level directory path.
    files : list of str
        The current list of files.

    Returns
    -------
    list of str
        The list of file name strings.

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


def find_files(extension: str, directories: list) -> list:
    """
    Return a list of all file names with the given extension by recursive scan.

    Parameters
    ----------
    extension : str
        The extension to match.
    directories : list of str
        The directory paths to scan.

    Returns
    -------
    list of str

    """
    files = []
    for file in scan_directories(directories):
        if file.endswith(extension):
            files.append(file)
    return files


def make_extensions(directories: list) -> list:
    """
    Return a list of c extensions.

    directories : list of str
        The directories to search for extensions.

    Returns
    -------
    list of Extension

    """
    extensions = []
    for file in find_files('.pyx', directories):
        extensions.append(Extension(
            name=file.replace(os.path.sep, ".")[:-4],
            sources=[file],
            include_dirs=['.']))
    return extensions
