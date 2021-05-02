# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2021 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.common.providers cimport InstrumentProvider
from nautilus_trader.execution.client cimport ExecutionClient


cdef class LiveExecutionClientFactory:
    pass


cdef class LiveExecutionClient(ExecutionClient):
    cdef object _loop

    cdef InstrumentProvider _instrument_provider
    cdef dict _account_last_free
    cdef dict _account_last_used
    cdef dict _account_last_total

    cdef void _on_reset(self) except *
