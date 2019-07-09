#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="setup.py" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

import Cython.Build
import os
import setuptools

from Cython.Build import cythonize
from Cython.Compiler import Options
from typing import List
from setuptools import setup, Extension

from nautilus_trader.version import __version__

AUTHOR = 'Nautech Systems Pty Ltd'
PACKAGE_NAME = 'nautilus_trader'
DESCRIPTION = 'The black box trading client and backtester for the Nautilus stack.'
LICENSE = 'Nautech Systems Software License, April 2018'
REQUIREMENTS = ['cython',
                'numpy',
                'scipy',
                'pandas',
                'iso8601',
                'pytz',
                'pyzmq',
                'msgpack',
                'psutil',
                'inv_indicators',
                'empyrical',
                'pymc3']
DIRECTORIES = [PACKAGE_NAME, 'test_kit']


# Command to compile c extensions
# python setup.py build_ext --inplace

# Cython compiler options
# -----------------------
Options.embed_pos_in_docstring = True  # Embed docstrings in extensions
Options.warning_errors = True  # Treat compiler warnings as errors
Options.cimport_from_pyx = True  # Allows cimporting from a pyx file without a pxd file
Profile_Hooks = False  # Write profiling hooks into methods (x2 overhead, use for profiling only)


# Recursively scan given directories
def scan_directories(directories: List[str]) -> List[str]:
    file_names = []
    for directory in directories:
        files = scan_files(directory)
        for file in files:
            file_names.append(file)
    return file_names


# Recursively scan directory for all files to cythonize
def scan_files(directory: str, files: List[str]=[]) -> List[str]:
    for file in os.listdir(directory):
        path = os.path.join(directory, file)
        if os.path.isfile(path) and path.endswith(".pyx"):
            files.append(path.replace(os.path.sep, ".")[:-4])
        elif os.path.isdir(path):
            scan_files(path, files)
    return files


# Generate an Extension object from its dotted name
def make_extension(ext_name) -> Extension:
    ext_path = ext_name.replace(".", os.path.sep) + ".pyx"
    return Extension(
        ext_name,
        [ext_path],
        include_dirs=["."],
        define_macros=[('CYTHON_TRACE', '1')])


# Generate list of extensions
extensions = [make_extension(name) for name in scan_directories(DIRECTORIES)]

setup(
    name=PACKAGE_NAME,
    version=__version__,
    author=AUTHOR,
    description=DESCRIPTION,
    packages=setuptools.find_packages(),
    include_package_data=True,
    package_data={'': ['*.pyx', '*.pxd']},
    license=LICENSE,
    requires=REQUIREMENTS,
    ext_modules=cythonize(extensions, compiler_directives={'profile': Profile_Hooks}),
    cmdclass={'build_ext': Cython.Build.build_ext},
    options={'build_ext': {'inplace': False, 'force': False}},
    zip_safe=False)
