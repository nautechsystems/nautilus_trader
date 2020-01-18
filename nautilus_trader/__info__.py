# -------------------------------------------------------------------------------------------------
# <copyright file="__info__.py" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

"""Define package location and version information."""

import os

PACKAGE_ROOT = os.path.dirname(os.path.abspath(__file__))


# Semantic Versioning (https://semver.org/)
_MAJOR_VERSION = 1
_MINOR_VERSION = 13
_PATCH_VERSION = 5

__version__ = '.'.join([
    str(_MAJOR_VERSION),
    str(_MINOR_VERSION),
    str(_PATCH_VERSION)])
