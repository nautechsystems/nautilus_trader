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


# `importlib.metadata` is available from 3.8 onward.
# Prior to that we need the `importlib_metadata` package.
try:
    from importlib.metadata import PackageNotFoundError
    from importlib.metadata import version
except ImportError:
    from importlib_metadata import PackageNotFoundError
    from importlib_metadata import version


PACKAGE_ROOT = os.path.dirname(os.path.abspath(__file__))


try:
    __version__ = version(__name__)
except (PackageNotFoundError, KeyError):
    # The version is pulled from the distribution metadata, not from local
    # source. That means that local non-packaged installs, (ie, running
    # out of the raw repo) may not have the version on them.
    __version__ = "<dev>"
