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

import uuid

from nautilus_trader.core.types cimport GUID
from nautilus_trader.common.guid cimport GuidFactory


cdef class LiveGuidFactory(GuidFactory):
    """
    Provides a GUID factory for live trading. Generates UUID4's.
    """

    def __init__(self):
        """
        Initializes a new instance of the LiveGuidFactory class.
        """
        super().__init__()

    cpdef GUID generate(self):
        """
        Return a generated UUID1.

        :return GUID.
        """
        return GUID(uuid.uuid4())
