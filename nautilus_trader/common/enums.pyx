# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2022 Nautech Systems Pty Ltd. All rights reserved.
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

"""Defines system level enums for use with framework components."""


from nautilus_trader.common.c_enums.component_state import ComponentState
from nautilus_trader.common.c_enums.component_trigger import ComponentTrigger
from nautilus_trader.common.logging import LogColor
from nautilus_trader.common.logging import LogLevel


__all__ = [
    "ComponentState",
    "ComponentTrigger",
    "LogColor",
    "LogLevel",
]
