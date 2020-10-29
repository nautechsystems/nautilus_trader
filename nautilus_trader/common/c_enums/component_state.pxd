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


cpdef enum ComponentState:
    UNDEFINED = 0,  # Invalid value
    INITIALIZED = 1,
    STARTING = 2,
    RUNNING = 3,
    STOPPING = 4,
    STOPPED = 5,
    RESUMING = 6,
    RESETTING = 7,
    DISPOSING = 8,
    DISPOSED = 9,
    FAULTED = 10,


cdef inline str component_state_to_string(int value):
    if value == 1:
        return 'INITIALIZED'
    elif value == 2:
        return 'STARTING'
    elif value == 3:
        return 'RUNNING'
    elif value == 4:
        return 'STOPPING'
    elif value == 5:
        return 'STOPPED'
    elif value == 6:
        return 'RESUMING'
    elif value == 7:
        return 'RESETTING'
    elif value == 8:
        return 'DISPOSING'
    elif value == 9:
        return 'DISPOSED'
    elif value == 10:
        return 'FAULTED'
    else:
        return 'UNDEFINED'


cdef inline ComponentState component_state_from_string(str value):
    if value == 'INITIALIZED':
        return ComponentState.INITIALIZED
    elif value == 'STARTING':
        return ComponentState.STARTING
    elif value == 'RUNNING':
        return ComponentState.RUNNING
    elif value == 'STOPPING':
        return ComponentState.STOPPING
    elif value == 'STOPPED':
        return ComponentState.STOPPED
    elif value == 'RESUMING':
        return ComponentState.RESUMING
    elif value == 'RESETTING':
        return ComponentState.RESETTING
    elif value == 'DISPOSING':
        return ComponentState.DISPOSING
    elif value == DISPOSED:
        return ComponentState.DISPOSED
    elif value == 'FAULTED':
        return ComponentState.FAULTED
    else:
        return ComponentState.UNDEFINED
