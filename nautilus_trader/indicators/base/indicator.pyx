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


cdef class Indicator:
    """
    The base class for all indicators.
    """

    def __init__(self, list params not None, bint check_inputs=False):
        """
        Initialize a new instance of the abstract Indicator class.

        :param params: The initialization parameters for the indicator.
        :param params: A boolean flag indicating whether method preconditions should be used.
        """
        self.name = self.__class__.__name__
        self.params = "" if params is [] else str(params)[1:-1].replace("'", "").strip("()")
        self.check_inputs = check_inputs
        self.has_inputs = False
        self.initialized = False

    def __str__(self) -> str:
        """
        Return a string representation of this object.

        :return str.
        """
        return f"{self.name}({self.params})"

    def __repr__(self) -> str:
        """
        Return a string representation of this object which includes the objects
        location in memory.

        :return str.
        """
        return f"<{str(self)} object at {id(self)}>"

    cdef void _set_has_inputs(self, bint setting) except *:
        self.has_inputs = setting

    cdef void _set_initialized(self, bint setting) except *:
        self.initialized = setting

    cdef void _reset_base(self) except *:
        self.has_inputs = False
        self.initialized = False
