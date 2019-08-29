# -------------------------------------------------------------------------------------------------
# <copyright file="serializers.pxd" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

from nautilus_trader.serialization.base cimport (
    QuerySerializer,
    OrderSerializer,
    EventSerializer,
    CommandSerializer,
    RequestSerializer,
    ResponseSerializer,
)


cdef class MsgPackQuerySerializer(QuerySerializer):
    pass


cdef class MsgPackOrderSerializer(OrderSerializer):
    pass


cdef class MsgPackCommandSerializer(CommandSerializer):
    cdef OrderSerializer order_serializer


cdef class MsgPackEventSerializer(EventSerializer):
    pass


cdef class MsgPackRequestSerializer(RequestSerializer):
    cdef QuerySerializer query_serializer


cdef class MsgPackResponseSerializer(ResponseSerializer):
    pass
