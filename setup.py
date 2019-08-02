#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="setup.py" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

import os
import setuptools
import Cython.Build

from typing import List
from setuptools import setup, Extension
from Cython.Build import cythonize
from Cython.Compiler import Options

from nautilus_trader.version import __version__
from setup_tools import check_file_headers, find_pyx_files


PACKAGE_NAME = 'nautilus_trader'
AUTHOR = 'Nautech Systems Pty Ltd'
DESCRIPTION = 'An algorithmic trading framework written in Cython.'
LICENSE = 'Nautech Systems Software License, April 2018'
REQUIREMENTS = ['cython',
                'numpy',
                'scipy',
                'pandas',
                'iso8601',
                'pytz',
                'pyzmq',
                'pymongo',
                'msgpack',
                'psutil',
                'empyrical',
                'pymc3']

DIRECTORIES_TO_CYTHONIZE = [PACKAGE_NAME, 'test_kit']
DIRECTORIES_ALL = [PACKAGE_NAME, 'test_kit', 'tests']


# Cython build options (edit here only)
# -------------------------------------
# Create a html annotations file for each .pyx
Options.annotate = False

# Embed docstrings in extensions
Options.embed_pos_in_docstring = True

# Treat compiler warnings as errors
Options.warning_errors = True

# Allows cimporting from a pyx file without a pxd file
Options.cimport_from_pyx = True

# Write profiling hooks into methods (x2 overhead, use for profiling only)
Profile_Hooks = False

# Cython compiler directives
compiler_directives = {'language_level': 3, 'profile': Profile_Hooks}
# -------------------------------------


# Check file headers
artifacts_to_ignore = ['', '.c', '.so', '.gz', '.o', '.pyd', '.pyc', '.prof', '.html', '.csv']
check_file_headers(DIRECTORIES_ALL, ignore=artifacts_to_ignore, author=AUTHOR)


def make_cython_extensions(directories: List[str]) -> [Extension]:
    # Generate a a list of Extension objects from the given directories list
    extensions = []
    for file in find_pyx_files(directories):
        extensions.append(Extension(
            name=file.replace(os.path.sep, ".")[:-4],
            sources=[file],
            include_dirs=['.'],
            define_macros=[('CYTHON_TRACE', '1')]))
    return extensions


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
    ext_modules=cythonize(module_list=make_cython_extensions(DIRECTORIES_TO_CYTHONIZE),
                          compiler_directives=compiler_directives),
    cmdclass={'build_ext': Cython.Build.build_ext},
    options={'build_ext': {'inplace': False, 'force': False}},
    zip_safe=False)
