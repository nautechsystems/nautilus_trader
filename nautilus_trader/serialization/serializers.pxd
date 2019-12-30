# -------------------------------------------------------------------------------------------------
# <copyright file="serializers.pxd" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

from nautilus_trader.core.cache cimport ObjectCache
from nautilus_trader.common.cache cimport IdentifierCache
from nautilus_trader.serialization.base cimport (
    QuerySerializer,
    OrderSerializer,
    EventSerializer,
    CommandSerializer,
    RequestSerializer,
    ResponseSerializer,
    LogSerializer
)


cdef class MsgPackQuerySerializer(QuerySerializer):
    pass


cdef class MsgPackOrderSerializer(OrderSerializer):
    cdef ObjectCache symbol_cache


cdef class MsgPackCommandSerializer(CommandSerializer):
    cdef IdentifierCache identifier_cache
    cdef OrderSerializer order_serializer


cdef class MsgPackEventSerializer(EventSerializer):
    cdef IdentifierCache identifier_cache


cdef class MsgPackRequestSerializer(RequestSerializer):
    cdef QuerySerializer query_serializer


cdef class MsgPackResponseSerializer(ResponseSerializer):
    pass


cdef class MsgPackLogSerializer(LogSerializer):
    pass
