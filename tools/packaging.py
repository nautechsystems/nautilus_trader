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
            requirements = ["".join([i for i in requirement if i.isalpha()]) for requirement in requirements]
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
    Return a list of C extensions.

    directories : list of str
        The directories to search for extensions.

    Returns
    -------
    list of Extension

    """
    extensions = []
    for file in find_files(".pyx", directories):
        extensions.append(Extension(
            name=file.replace(os.path.sep, ".")[:-4],
            sources=[file],
            include_dirs=["."]))

    return extensions
