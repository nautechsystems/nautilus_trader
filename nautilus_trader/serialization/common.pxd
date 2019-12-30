# -------------------------------------------------------------------------------------------------
# <copyright file="common.pxd" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

from cpython.datetime cimport datetime

from nautilus_trader.model.identifiers cimport Label
from nautilus_trader.model.objects cimport Price


cpdef str convert_price_to_string(Price price)
cpdef Price convert_string_to_price(str price_string)
cpdef str convert_label_to_string(Label label)
cpdef Label convert_string_to_label(str label)
cpdef str convert_datetime_to_string(datetime time)
cpdef datetime convert_string_to_datetime(str time_string)
