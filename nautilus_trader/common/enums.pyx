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

"""
Defines system level enums for use with framework components.

Component State
---------------
Represents a discrete component state.

>>> from nautilus_trader.common.enums import ComponentState
>>> ComponentState.PRE_INITIALIZED
<ComponentState.PRE_INITIALIZED: 0>
>>> ComponentState.INITIALIZED
<ComponentState.INITIALIZED: 1>
>>> ComponentState.STARTING
<ComponentState.STARTING: 2>
>>> ComponentState.RUNNING
<ComponentState.RUNNING: 3>
>>> ComponentState.STOPPING
<ComponentState.STOPPING: 4>
>>> ComponentState.STOPPED
<ComponentState.STOPPED: 5>
>>> ComponentState.RESUMING
<ComponentState.RESUMING: 6>
>>> ComponentState.RESETTING
<ComponentState.RESETTING: 7>
>>> ComponentState.DISPOSING
<ComponentState.DISPOSING: 8>
>>> ComponentState.DISPOSED
<ComponentState.DISPOSED: 9>
>>> ComponentState.DEGRADING
<ComponentState.DEGRADING: 10>
>>> ComponentState.DEGRADED
<ComponentState.DEGRADED: 11>
>>> ComponentState.FAULTING
<ComponentState.FAULTING: 12>
>>> ComponentState.FAULTED
<ComponentState.FAULTED: 13>
>>> ComponentState.FAULTED
<ComponentState.FAULTED: 13>

Component Trigger
-----------------
Represents a trigger event which will cause a component state transition.

>>> from nautilus_trader.common.enums import ComponentTrigger
>>> ComponentTrigger.INITIALIZE
<ComponentTrigger.INITIALIZE: 1>
>>> ComponentTrigger.START
<ComponentTrigger.START: 2>
>>> ComponentTrigger.RUNNING
<ComponentTrigger.RUNNING: 3>
>>> ComponentTrigger.STOP
<ComponentTrigger.STOP: 4>
>>> ComponentTrigger.STOPPED
<ComponentTrigger.STOPPED: 5>
>>> ComponentTrigger.RESUME
<ComponentTrigger.RESUME: 6>
>>> ComponentTrigger.RESET
<ComponentTrigger.RESET: 7>
>>> ComponentTrigger.DISPOSE
<ComponentTrigger.DISPOSE: 8>
>>> ComponentTrigger.DISPOSED
<ComponentTrigger.DISPOSED: 9>
>>> ComponentTrigger.DEGRADE
<ComponentTrigger.DEGRADE: 10>
>>> ComponentTrigger.DEGRADED
<ComponentTrigger.DEGRADED: 11>
>>> ComponentTrigger.FAULT
<ComponentTrigger.FAULT: 12>
>>> ComponentTrigger.FAULTED
<ComponentTrigger.FAULTED: 13>

Log Level
---------
Represents a log level threshold for configuration.

Enums values match the built-in Python `LogLevel`.

>>> from nautilus_trader.common.enums import LogLevel
>>> LogLevel.DEBUG
<LogLevel.DEBUG: 10>
>>> LogLevel.INFO
<LogLevel.INFO: 20>
>>> LogLevel.WARNING
<LogLevel.WARNING: 30>
>>> LogLevel.ERROR
<LogLevel.ERROR: 40>
>>> LogLevel.CRITICAL
<LogLevel.CRITICAL: 50>

Log Color
---------
Represents log color constants.

>>> from nautilus_trader.common.enums import LogColor
>>> LogColor.NORMAL
<LogColor.NORMAL: 0>
>>> LogColor.GREEN
<LogColor.GREEN: 1>
>>> LogColor.BLUE
<LogColor.BLUE: 2>
>>> LogColor.YELLOW
<LogColor.YELLOW: 3>
>>> LogColor.RED
<LogColor.RED: 4>

"""

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
