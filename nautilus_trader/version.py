# -------------------------------------------------------------------------------------------------
# <copyright file="version.py" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

import os

PACKAGE_ROOT = os.path.dirname(os.path.abspath(__file__))


MAJOR = 0
MINOR = 99
MICRO = 404

__version__ = f'{MAJOR}.{MINOR}.{MICRO}'
# $Source$
