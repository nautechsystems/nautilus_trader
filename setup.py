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

from typing import List
from setuptools import setup, Extension
from Cython.Build import cythonize, build_ext
from Cython.Compiler import Options

from nautilus_trader.version import __version__
from setup_tools import find_files, get_directories
from linter import check_file_headers


PACKAGE_NAME = 'nautilus_trader'
AUTHOR = 'Nautech Systems Pty Ltd'
MAINTAINER = 'Nautech Systems Pty Ltd'
MAINTAINER_EMAIL = 'info@nautechsystems.io'
DESCRIPTION = 'An algorithmic trading framework written in Cython.'
LICENSE = 'Nautech Systems Software License, April 2018'
URL = 'https://nautechsystems.io/nautilus'
PYTHON_REQUIRES = '>=3.6'
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
# Specify if modules should be re-compiled (if False .c files must exist)
COMPILE_WITH_CYTHON = True

# Create a html annotations file for each .pyx
Options.annotate = True

# Embed docstrings in extensions
Options.embed_pos_in_docstring = True

# Treat compiler warnings as errors
Options.warning_errors = True

# Allows cimporting from a pyx file without a pxd file
Options.cimport_from_pyx = True

# Write profiling hooks into methods (x2 overhead, use for profiling only)
Profile_Hooks = False

# Cython compiler directives
compiler_directives = {'language_level': 3, 'embedsignature': True, 'profile': Profile_Hooks}
# -------------------------------------


# Lint source code (throws exception on failure)
artifacts_to_ignore = ['', '.c', '.so', '.gz', '.o', '.pyd', '.pyc', '.prof', '.html', '.csv']
check_file_headers(DIRECTORIES_ALL, ignore=artifacts_to_ignore, author=AUTHOR)


def make_extensions(directories: List[str]) -> [Extension]:
    # Generate a a list of Extension objects from the given directories list
    extensions = []
    for file in find_files('.pyx' if COMPILE_WITH_CYTHON else '.c', directories):
        extensions.append(Extension(
            name=file.replace(os.path.sep, ".")[:-4],
            sources=[file],
            include_dirs=['.'],
            define_macros=[('CYTHON_TRACE', '1')]))
    return extensions


definition_ext = '*.pxd'
modules = (get_directories(PACKAGE_NAME))
package_data = {PACKAGE_NAME: [definition_ext]}
for module in modules:
    package_data[f'{PACKAGE_NAME}/{module}'] = [definition_ext]
print(f"Including package data; {package_data}")


setup(
    name=PACKAGE_NAME,
    version=__version__,
    author=AUTHOR,
    maintainer=MAINTAINER,
    maintainer_email=MAINTAINER_EMAIL,
    description=DESCRIPTION,
    license=LICENSE,
    url=URL,
    packages=setuptools.find_packages(),
    include_package_data=True,
    package_data=package_data,
    python_requires=PYTHON_REQUIRES,
    requires=REQUIREMENTS,
    ext_modules=cythonize(
        module_list=make_extensions(DIRECTORIES_TO_CYTHONIZE),
        compiler_directives=compiler_directives),
    cmdclass={'build_ext': build_ext},
    options={'build_ext': {'inplace': False, 'force': False}},
    zip_safe=False)  # Allows cimport of pxd files
