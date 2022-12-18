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

from cpython.object cimport PyObject

from nautilus_trader.core.rust.model cimport AggregationSource
from nautilus_trader.core.rust.model cimport aggregation_source_from_pystr
from nautilus_trader.core.rust.model cimport aggregation_source_to_pystr
from nautilus_trader.core.string cimport pyobj_to_str


cpdef inline str aggregation_source_to_str(AggregationSource value):
    return pyobj_to_str(aggregation_source_to_pystr(value))


cpdef inline AggregationSource aggregation_source_from_str(str value) except *:
    return aggregation_source_from_pystr(<PyObject *>value)
