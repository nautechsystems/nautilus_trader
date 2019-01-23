#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="setup.py" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

import Cython.Build
import os
import setuptools

from Cython.Build import cythonize
from Cython.Compiler import Options
from typing import List
from setuptools import setup, Extension

# Command to compile c extensions
# python setup.py build_ext --inplace

VERSION = '0.75.0'
AUTHOR = 'Invariance'
INV_TRADER = 'inv_trader'
DESCRIPTION = 'The python trading client for Invariance.'
LICENSE = 'Invariance Software License, April 2018'
REQUIREMENTS = ['cython',
                'numpy',
                'pandas',
                'iso8601',
                'pyzmq',
                'msgpack',
                'psutil',
                'redis',
                'inv_indicators',
                'pyfolio',
                'pymc3',
                'theano']
DIRECTORIES = [INV_TRADER, 'test_kit']


# Cython compiler options
# -----------------------
Options.embed_pos_in_docstring = True  # Embed docstrings in extensions
Options.warning_errors = True  # Treat compiler warnings as errors
Options.cimport_from_pyx = True  # Allows cimporting from a pyx file without a pxd file
Profile_Hooks = True  # Write profiling hooks into methods (some overhead, use for profiling)


# Recursively scan given directories
def scan_directories(directories: List[str]) -> List[str]:
    file_names = []
    for directory in directories:
        files = scan_files(directory)
        for file in files:
            file_names.append(file)
    return file_names


# Recursively scan for all files to cythonize
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
        include_dirs=["."])


# Generate list of extensions
extensions = [make_extension(name) for name in scan_directories(DIRECTORIES)]

setup(
    name=INV_TRADER,
    version=VERSION,
    author=AUTHOR,
    description=DESCRIPTION,
    packages=setuptools.find_packages(),
    license=LICENSE,
    requires=REQUIREMENTS,
    ext_modules=cythonize(extensions, compiler_directives={'profile': Profile_Hooks}),
    cmdclass={'build_ext': Cython.Build.build_ext},
    options={'build_ext': {'inplace': False, 'force': False}},
    zip_safe=False)
