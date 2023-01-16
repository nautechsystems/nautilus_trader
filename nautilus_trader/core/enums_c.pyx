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

"""Defines core level enums."""


from nautilus_trader.core.rust.core cimport MessageCategory
from nautilus_trader.core.rust.core cimport message_category_from_cstr
from nautilus_trader.core.rust.core cimport message_category_to_cstr
from nautilus_trader.core.string cimport cstr_to_pystr
from nautilus_trader.core.string cimport pystr_to_cstr


cpdef MessageCategory message_category_from_str(str value) except *:
    return message_category_from_cstr(pystr_to_cstr(value))


cpdef str message_category_to_str(MessageCategory value):
    return cstr_to_pystr(message_category_to_cstr(value))
