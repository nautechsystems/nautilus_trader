# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

from __future__ import annotations

from nautilus_trader._libnautilus.blockchain import *  # noqa: F403 (undefined-local-with-import-star)


def _reassign_module_names() -> None:
    for _name, _obj in list(globals().items()):
        module = getattr(_obj, "__module__", "")
        if module.startswith("nautilus_trader.core.nautilus_pyo3.blockchain"):
            try:
                _obj.__module__ = __name__
            except (AttributeError, TypeError):
                continue


_reassign_module_names()
del _reassign_module_names
