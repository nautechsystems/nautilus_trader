# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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


def fixup_module_names(namespace: dict, target_module: str) -> None:
    """
    Reassign ``__module__`` on PyO3 types re-exported via star imports.

    The Rust extension registers types under internal paths like
    ``nautilus_trader.core.nautilus_pyo3.<submodule>``.  When a Python
    ``__init__.py`` does ``from nautilus_trader._libnautilus.<sub> import *``,
    the types land in the right namespace but their ``__module__`` still
    points to the internal path.  This breaks ``pickle`` (and confuses
    ``repr``) because Python cannot resolve the internal path at import time.

    Call this at the bottom of each ``__init__.py`` that re-exports from
    ``_libnautilus``::

        from nautilus_trader._fixup import fixup_module_names

        fixup_module_names(globals(), __name__)

    """
    for obj in namespace.values():
        module = getattr(obj, "__module__", "")
        if "nautilus_pyo3" in module:
            try:
                obj.__module__ = target_module
            except (AttributeError, TypeError):
                continue
