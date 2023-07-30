# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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
Provides an API integration for Interactive Brokers.
"""

import importlib.util


if importlib.util.find_spec("ibapi") is None:
    raise ImportError(
        "This module requires the 'ibapi' package, which isn't included by default due to package "
        "distribution limitations on PyPI with dependencies on GitHub repositories. "
        "You can manually install 'ibapi' using the following command: "
        "`pip install git+https://github.com/nautechsystems/ibapi.git`",
    )
