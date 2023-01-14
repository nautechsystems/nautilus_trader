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

"""Defines system level enums for use with framework components."""


from nautilus_trader.common.enums_c import component_state_from_str
from nautilus_trader.common.enums_c import component_state_to_str
from nautilus_trader.common.enums_c import component_trigger_from_str
from nautilus_trader.common.enums_c import component_trigger_to_str
from nautilus_trader.common.enums_c import log_level_from_str
from nautilus_trader.common.enums_c import log_level_to_str
from nautilus_trader.core.rust.common import ComponentState
from nautilus_trader.core.rust.common import ComponentTrigger
from nautilus_trader.core.rust.common import LogColor
from nautilus_trader.core.rust.common import LogLevel


__all__ = [
    "ComponentState",
    "ComponentTrigger",
    "LogColor",
    "LogLevel",
    "component_state_from_str",
    "component_state_to_str",
    "component_trigger_from_str",
    "component_trigger_to_str",
    "log_level_from_str",
    "log_level_to_str",
]
