#!/usr/bin/env python3
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

from Cython.Build import build_ext
from Cython.Build import cythonize
from Cython.Compiler import Options
import setuptools
from setuptools import setup

from nautilus_trader import __author__
from nautilus_trader import __version__
from tools.packaging import make_extensions
from tools.packaging import parse_requirements

PACKAGE_NAME = "nautilus_trader"
AUTHOR_EMAIL = "info@nautechsystems.io"
DESCRIPTION = "An algorithmic trading platform and event-driven backtester"
URL = "https://github.com/nautechsystems/nautilus_trader"
PYTHON_REQUIRES = ">=3.6.8"
DIRECTORIES_TO_CYTHONIZE = [PACKAGE_NAME]


# ------------------------------------------------------------------------------
# Cython (edit here only)
# ------------------------------------------------------------------------------
# https://cython.readthedocs.io/en/latest/src/userguide/source_files_and_compilation.html

# Cython build options
Options.annotate = True                # Create annotated html files for each .pyx
Options.docstrings = True              # Include docstrings in modules
Options.embed_pos_in_docstring = True  # Embed docstrings in extensions
Options.fast_fail = True               # Abort compilation on first error
Options.warning_errors = True          # Treat compiler warnings as errors
PROFILE_HOOKS = False                  # Write profiling hooks into methods (x2 performance overhead)
LINE_TRACING = False                   # Enable line tracing for code coverage

# Cython compiler directives
compiler_directives = {
    "language_level": 3,         # Python 3 default (can remove soon)
    "cdivision": True,           # If division is as per C with no check for zero (35% speed up)
    "embedsignature": True,      # If docstrings should be embedded into C signatures
    "emit_code_comments": True,  # If comments should be emitted to generated C code
    "profile": PROFILE_HOOKS,    # See above
    "linetrace": LINE_TRACING    # See above
}
# ------------------------------------------------------------------------------


# Create package description
with open("README.md", encoding='utf-8') as f:
    LONG_DESCRIPTION = f.read()


setup(
    name=PACKAGE_NAME,
    version=__version__,
    author=__author__,
    author_email=AUTHOR_EMAIL,
    maintainer=__author__,
    maintainer_email=AUTHOR_EMAIL,
    description=DESCRIPTION,
    long_description=LONG_DESCRIPTION,
    long_description_content_type="text/markdown",
    classifiers=[
        "Development Status :: 4 - Beta",
        "License :: OSI Approved :: GNU Lesser General Public License v3 or later (LGPLv3+)",
        "Programming Language :: Python :: 3.6",
        "Programming Language :: Python :: 3.7",
        "Programming Language :: Python :: 3.8",
    ],
    url=URL,
    python_requires=PYTHON_REQUIRES,
    requires=parse_requirements("requirements.txt", strip=True),
    install_requires=parse_requirements("requirements.txt"),
    tests_require=parse_requirements("requirements.txt"),
    packages=[module for module in setuptools.find_packages()],
    include_package_data=True,
    ext_modules=cythonize(
        module_list=make_extensions(DIRECTORIES_TO_CYTHONIZE),
        compiler_directives=compiler_directives,
        build_dir="build"),
    cmdclass={"build_ext": build_ext},
    options={"build_ext": {"inplace": True, "force": False}},
    zip_safe=False  # Allows cimport of pxd files
)
