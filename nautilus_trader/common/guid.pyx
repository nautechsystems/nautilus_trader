# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU General Public License Version 3.0 (the "License");
#  you may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/gpl-3.0.en.html
#
#  Unless required by applicable law or agreed to in writing, software
#  distributed under the License is distributed on an "AS IS" BASIS,
#  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
#  See the License for the specific language governing permissions and
#  limitations under the License.
# -------------------------------------------------------------------------------------------------

import uuid

from nautilus_trader.core.types cimport GUID


cdef class GuidFactory:
    """
    The base class for all GUID factories.
    """

    cpdef GUID generate(self):
        """
        Return a generated GUID.

        :return GUID.
        """
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")


cdef class TestGuidFactory(GuidFactory):
    """
    Provides a fake GUID factory for testing purposes.
    """

    def __init__(self):
        """
        Initializes a new instance of the TestGuidFactory class.
        """
        super().__init__()

        self._guid = GUID(uuid.uuid4())

    cpdef GUID generate(self):
        """
        Return the single test GUID instance.
        
        :return GUID.
        """
        return self._guid
