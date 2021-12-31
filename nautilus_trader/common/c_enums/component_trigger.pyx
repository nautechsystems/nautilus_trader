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


cdef class ComponentTriggerParser:

    @staticmethod
    cdef str to_str(int value):
        if value == 1:
            return "INITIALIZE"
        elif value == 2:
            return "START"
        elif value == 3:
            return "RUNNING"
        elif value == 4:
            return "STOP"
        elif value == 5:
            return "STOPPED"
        elif value == 6:
            return "RESUME"
        elif value == 7:
            return "RESET"
        elif value == 8:
            return "DISPOSE"
        elif value == 9:
            return "DISPOSED"
        elif value == 10:
            return "DEGRADE"
        elif value == 11:
            return "DEGRADED"
        elif value == 12:
            return "FAULT"
        elif value == 13:
            return "FAULTED"
        else:
            raise ValueError(f"value was invalid, was {value}")

    @staticmethod
    cdef ComponentTrigger from_str(str value) except *:
        if value == "INITIALIZE":
            return ComponentTrigger.INITIALIZE
        elif value == "START":
            return ComponentTrigger.START
        elif value == "RUNNING":
            return ComponentTrigger.RUNNING
        elif value == "STOP":
            return ComponentTrigger.STOP
        elif value == "STOPPED":
            return ComponentTrigger.STOPPED
        elif value == "RESUME":
            return ComponentTrigger.RESUME
        elif value == "RESET":
            return ComponentTrigger.RESET
        elif value == "DISPOSE":
            return ComponentTrigger.DISPOSE
        elif value == "DISPOSED":
            return ComponentTrigger.DISPOSED
        elif value == "DEGRADE":
            return ComponentTrigger.DEGRADE
        elif value == "DEGRADED":
            return ComponentTrigger.DEGRADED
        elif value == "FAULT":
            return ComponentTrigger.FAULT
        elif value == "FAULTED":
            return ComponentTrigger.FAULTED
        else:
            raise ValueError(f"value was invalid, was {value}")

    @staticmethod
    def to_str_py(int value):
        return ComponentTriggerParser.to_str(value)

    @staticmethod
    def from_str_py(str value):
        return ComponentTriggerParser.from_str(value)
