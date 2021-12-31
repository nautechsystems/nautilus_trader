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


cdef class ComponentStateParser:

    @staticmethod
    cdef str to_str(int value):
        if value == 0:
            return "PRE_INITIALIZED"
        elif value == 1:
            return "INITIALIZED"
        elif value == 2:
            return "STARTING"
        elif value == 3:
            return "RUNNING"
        elif value == 4:
            return "STOPPING"
        elif value == 5:
            return "STOPPED"
        elif value == 6:
            return "RESUMING"
        elif value == 7:
            return "RESETTING"
        elif value == 8:
            return "DISPOSING"
        elif value == 9:
            return "DISPOSED"
        elif value == 10:
            return "DEGRADING"
        elif value == 11:
            return "DEGRADED"
        elif value == 12:
            return "FAULTING"
        elif value == 13:
            return "FAULTED"
        else:
            raise ValueError(f"value was invalid, was {value}")

    @staticmethod
    cdef ComponentState from_str(str value) except *:
        if value == "PRE_INITIALIZED":
            return ComponentState.PRE_INITIALIZED
        elif value == "INITIALIZED":
            return ComponentState.INITIALIZED
        elif value == "STARTING":
            return ComponentState.STARTING
        elif value == "RUNNING":
            return ComponentState.RUNNING
        elif value == "STOPPING":
            return ComponentState.STOPPING
        elif value == "STOPPED":
            return ComponentState.STOPPED
        elif value == "RESUMING":
            return ComponentState.RESUMING
        elif value == "RESETTING":
            return ComponentState.RESETTING
        elif value == "DISPOSING":
            return ComponentState.DISPOSING
        elif value == "DISPOSED":
            return ComponentState.DISPOSED
        elif value == "DEGRADING":
            return ComponentState.DEGRADING
        elif value == "DEGRADED":
            return ComponentState.DEGRADED
        elif value == "FAULTING":
            return ComponentState.FAULTING
        elif value == "FAULTED":
            return ComponentState.FAULTED
        else:
            raise ValueError(f"value was invalid, was {value}")

    @staticmethod
    def to_str_py(int value):
        return ComponentStateParser.to_str(value)

    @staticmethod
    def from_str_py(str value):
        return ComponentStateParser.from_str(value)
