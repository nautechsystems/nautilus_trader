# -------------------------------------------------------------------------------------------------
# <copyright file="message.pxd" company="Nautech Systems Pty Ltd">
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
    """
    Provides a serializer for data query objects for the MsgPack specification.
    """


cdef class MsgPackOrderSerializer(OrderSerializer):
    """
    Provides a command serializer for the MessagePack specification
    """


cdef class MsgPackCommandSerializer(CommandSerializer):
    """
    Provides a command serializer for the MessagePack specification.
    """
    cdef OrderSerializer order_serializer


cdef class MsgPackEventSerializer(EventSerializer):
    """
    Provides an event serializer for the MessagePack specification
    """


cdef class MsgPackRequestSerializer(RequestSerializer):
    """
    Provides a request serializer for the MessagePack specification
    """
    cdef QuerySerializer query_serializer


cdef class MsgPackResponseSerializer(ResponseSerializer):
    """
    Provides a response serializer for the MessagePack specification
    """
