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

from nautilus_trader.core.rust.common cimport ComponentState
from nautilus_trader.core.rust.common cimport ComponentTrigger
from nautilus_trader.core.rust.common cimport LogColor
from nautilus_trader.core.rust.common cimport LogLevel
from nautilus_trader.core.rust.common cimport component_state_from_cstr
from nautilus_trader.core.rust.common cimport component_state_to_cstr
from nautilus_trader.core.rust.common cimport component_trigger_from_cstr
from nautilus_trader.core.rust.common cimport component_trigger_to_cstr
from nautilus_trader.core.rust.common cimport log_color_from_cstr
from nautilus_trader.core.rust.common cimport log_color_to_cstr
from nautilus_trader.core.rust.common cimport log_level_from_cstr
from nautilus_trader.core.rust.common cimport log_level_to_cstr
from nautilus_trader.core.string cimport cstr_to_pystr
from nautilus_trader.core.string cimport pystr_to_cstr


cpdef ComponentState component_state_from_str(str value) except *:
    return component_state_from_cstr(pystr_to_cstr(value))


cpdef str component_state_to_str(ComponentState value):
    return cstr_to_pystr(component_state_to_cstr(value))


cpdef ComponentTrigger component_trigger_from_str(str value) except *:
    return component_trigger_from_cstr(pystr_to_cstr(value))


cpdef str component_trigger_to_str(ComponentTrigger value):
    return cstr_to_pystr(component_trigger_to_cstr(value))


cpdef LogColor log_color_from_str(str value) except *:
    return log_color_from_cstr(pystr_to_cstr(value))


cpdef str log_color_to_str(LogColor value):
    return cstr_to_pystr(log_color_to_cstr(value))


cpdef LogLevel log_level_from_str(str value) except *:
    return log_level_from_cstr(pystr_to_cstr(value))


cpdef str log_level_to_str(LogLevel value):
    return cstr_to_pystr(log_level_to_cstr(value))
