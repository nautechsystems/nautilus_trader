# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE file.
#  https://nautechsystems.io
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
