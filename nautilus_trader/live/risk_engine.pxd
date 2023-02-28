# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.common.queue cimport Queue
from nautilus_trader.risk.engine cimport RiskEngine


cdef class LiveRiskEngine(RiskEngine):
    cdef object _loop
    cdef object _cmd_queue_task
    cdef object _evt_queue_task
    cdef Queue _cmd_queue
    cdef Queue _evt_queue

    cdef readonly bint is_running
    """If the risk engine is running.\n\n:returns: `bool`"""
