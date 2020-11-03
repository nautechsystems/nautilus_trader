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

cdef class ComponentTriggerParser:

    @staticmethod
    cdef str to_string(int value):
        if value == 1:
            return 'START'
        elif value == 2:
            return 'RUNNING'
        elif value == 3:
            return 'STOP'
        elif value == 4:
            return 'STOPPED'
        elif value == 5:
            return 'RESUME'
        elif value == 6:
            return 'RESET'
        elif value == 7:
            return 'DISPOSE'
        elif value == 8:
            return 'DISPOSED'
        else:
            return 'UNDEFINED'

    @staticmethod
    cdef ComponentTrigger from_string(str value):
        if value == 'START':
            return ComponentTrigger.START
        elif value == 'RUNNING':
            return ComponentTrigger.RUNNING
        elif value == 'STOP':
            return ComponentTrigger.STOP
        elif value == 'STOPPED':
            return ComponentTrigger.STOPPED
        elif value == 'RESUME':
            return ComponentTrigger.RESUME
        elif value == 'RESET':
            return ComponentTrigger.RESET
        elif value == 'DISPOSE':
            return ComponentTrigger.DISPOSE
        elif value == 'DISPOSED':
            return ComponentTrigger.DISPOSED
        else:
            return ComponentTrigger.UNDEFINED

    @staticmethod
    def to_string_py(int value):
        return ComponentTriggerParser.to_string(value)

    @staticmethod
    def from_string_py(str value):
        return ComponentTriggerParser.from_string(value)
