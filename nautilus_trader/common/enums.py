# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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
Defines system level enums for use with framework components.
"""

from typing import TYPE_CHECKING

from nautilus_trader.common.component import component_state_from_str
from nautilus_trader.common.component import component_state_to_str
from nautilus_trader.common.component import component_trigger_from_str
from nautilus_trader.common.component import component_trigger_to_str
from nautilus_trader.common.component import log_level_from_str
from nautilus_trader.common.component import log_level_to_str
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

# mypy: disable-error-code=no-redef

if TYPE_CHECKING:

    class ComponentState:
        PRE_INITIALIZED: int = 0
        READY: int = 1
        STARTING: int = 2
        RUNNING: int = 3
        STOPPING: int = 4
        STOPPED: int = 5
        RESUMING: int = 6
        RESETTING: int = 7
        DISPOSING: int = 8
        DISPOSED: int = 9
        DEGRADING: int = 10
        DEGRADED: int = 11
        FAULTING: int = 12
        FAULTED: int = 13

    class ComponentTrigger:
        INITIALIZE: int = 1
        START: int = 2
        START_COMPLETED: int = 3
        STOP: int = 4
        STOP_COMPLETED: int = 5
        RESUME: int = 6
        RESUME_COMPLETED: int = 7
        RESET: int = 8
        RESET_COMPLETED: int = 9
        DISPOSE: int = 10
        DISPOSE_COMPLETED: int = 11
        DEGRADE: int = 12
        DEGRADE_COMPLETED: int = 13
        FAULT: int = 14
        FAULT_COMPLETED: int = 15

    class LogLevel:
        OFF: int = 0
        TRACE: int = 1
        DEBUG: int = 2
        INFO: int = 3
        WARNING: int = 4
        ERROR: int = 5

    class LogColor:
        NORMAL: int = 0
        GREEN: int = 1
        BLUE: int = 2
        MAGENTA: int = 3
        CYAN: int = 4
        YELLOW: int = 5
        RED: int = 6
