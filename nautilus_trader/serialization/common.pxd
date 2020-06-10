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

from cpython.datetime cimport datetime

from nautilus_trader.core.types cimport Label
from nautilus_trader.model.objects cimport Price


cpdef str convert_price_to_string(Price price)
cpdef str convert_label_to_string(Label label)
cpdef str convert_datetime_to_string(datetime time)
cpdef Price convert_string_to_price(str price_string)
cpdef Label convert_string_to_label(str label)
cpdef datetime convert_string_to_datetime(str time_string)
