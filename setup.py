#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="setup.py" company="Invariance Pte">
#  Copyright (C) 2018 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

import Cython.Build
import os
import setuptools

from Cython.Build import cythonize
from typing import List
from setuptools import setup, Extension

# Command to compile c extensions
# python setup.py build_ext --inplace

INV_TRADER = 'inv_trader'


# Recursively scan root directory for all files to cythonize
def scan_dir(root_dir, files=[]) -> List[str]:
    for file in os.listdir(root_dir):
        path = os.path.join(root_dir, file)
        if os.path.isfile(path) and path.endswith(".pyx"):
            files.append(path.replace(os.path.sep, ".")[:-4])
        elif os.path.isdir(path):
            scan_dir(path, files)
    return files


# Generate an Extension object from its dotted name
def make_extension(ext_name) -> Extension:
    ext_path = ext_name.replace(".", os.path.sep) + ".pyx"
    return Extension(
        ext_name,
        [ext_path],
        include_dirs=["."])


# Generate list of extensions
extensions = [make_extension(name) for name in scan_dir(INV_TRADER)]

setup(
    name=INV_TRADER,
    version='0.2',
    author='Invariance',
    description='The python trading client for Invariance.',
    packages=setuptools.find_packages(),
    license='Invariance Software License',
    requires=['cython',
              'iso8601',
              'pytz',
              'redis',
              'msgpack',
              'PyPubSub'],
    ext_modules=cythonize(extensions),
    cmdclass={'build_ext': Cython.Build.build_ext},
    options={'build_ext': {'inplace': False, 'force': False}})
