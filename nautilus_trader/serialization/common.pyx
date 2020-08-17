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

import pandas as pd
from cpython.datetime cimport datetime

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.datetime cimport format_iso8601
from nautilus_trader.core.types cimport Label
from nautilus_trader.model.objects cimport Price
from nautilus_trader.serialization.constants cimport *


cpdef str convert_price_to_string(Price price):
    """
    Return the converted string from the given price, can return a 'None' string..

    :param price: The price to convert.
    :return str.
    """
    return NONE if price is None else price.to_string()

cpdef str convert_label_to_string(Label label):
    """
    Return the converted string from the given label, can return a 'None' string.

    :param label: The label to convert.
    :return str.
    """
    return NONE if label is None else label.value

cpdef str convert_datetime_to_string(datetime time):
    """
    Return the converted ISO8601 string from the given datetime, can return a 'None' string.

    :param time: The datetime to convert
    :return str.
    """
    return NONE if time is None else format_iso8601(time)

cpdef Price convert_string_to_price(str price_string):
    """
    Return the converted price (or None) from the given price string.

    :param price_string: The price string to convert.
    :return Price or None.
    """
    Condition.valid_string(price_string, "price_string")  # string often 'None'

    return None if price_string == NONE else Price.from_string(price_string)

cpdef Label convert_string_to_label(str label_string):
    """
    Return the converted label (or None) from the given label string.

    :param label_string: The label string to convert.
    :return Label or None.
    """
    Condition.valid_string(label_string, "label_string")  # string often 'None'

    return None if label_string == NONE else Label(label_string)


cpdef datetime convert_string_to_datetime(str time_string):
    """
    Return the converted datetime (or None) from the given time string.

    :param time_string: The time string to convert.
    :return datetime or None.
    """
    Condition.valid_string(time_string, "time_string")  # string often 'None'

    return None if time_string == NONE else pd.to_datetime(time_string)
