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

"""
The `trading` sub-package groups the core trading operations components.

This is a top level package where the majority of users will interface with the
framework. Custom trading strategies can be implemented by inheriting from the
`TradingStrategy` base class.
"""

import os

PACKAGE_ROOT = os.path.dirname(os.path.abspath(__file__))


__author__ = "Nautech Systems"

# Semantic Versioning (https://semver.org/)
_MAJOR_VERSION = 1
_MINOR_VERSION = 83
_PATCH_VERSION = 11
_PRE_RELEASE = ''

__version__ = '.'.join([
    str(_MAJOR_VERSION),
    str(_MINOR_VERSION),
    str(_PATCH_VERSION)]) + _PRE_RELEASE
