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

from nautilus_trader.model.identifiers cimport ClientOrderId
from nautilus_trader.model.identifiers cimport ExecAlgorithmId


cdef class ExecAlgorithmSpecification:
    cdef frozenset _key

    cdef readonly ClientOrderId client_order_id
    """The client order ID for the order being executed.\n\n:returns: `ExecAlgorithmId`"""
    cdef readonly ExecAlgorithmId exec_algorithm_id
    """The execution algorithm ID.\n\n:returns: `ExecAlgorithmId`"""
    cdef readonly dict params
    """The execution algorithm parameters for the order.\n\n:returns: `dict[str, Any]` or ``None``"""
