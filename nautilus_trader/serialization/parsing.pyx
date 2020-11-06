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

from cpython.datetime cimport datetime

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.datetime cimport to_posix_ms
from nautilus_trader.core.datetime cimport from_posix_ms


cdef str NONE = str(None)


cdef class ObjectParser:

    @staticmethod
    cdef str datetime_to_string(datetime dt):
        return NONE if dt is None else str(to_posix_ms(dt))

    @staticmethod
    cdef datetime string_to_datetime(str time_string):
        Condition.valid_string(time_string, "time_string")

        # noinspection long
        # noinspection PyUnresolvedReferences
        return None if time_string == NONE else from_posix_ms(long(time_string))

    @staticmethod
    def datetime_to_string_py(datetime dt):
        """
        Return the converted ISO8601 string from the given datetime, can return a 'None' string.

        Parameters
        ----------
        dt : datetime
            The datetime to convert

        Returns
        -------
        str

        """
        return ObjectParser.datetime_to_string(dt)

    @staticmethod
    def string_to_datetime_py(str dt_string):
        """
        Return the converted datetime (or None) from the given time string.

        Parameters
        ----------
        dt_string : str
            The time string to convert.

        Returns
        -------
        datetime or None

        """
        return ObjectParser.string_to_datetime(dt_string)
