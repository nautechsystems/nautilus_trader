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

from nautilus_trader.core.cache cimport ObjectCache
from nautilus_trader.common.cache cimport IdentifierCache
from nautilus_trader.serialization.base cimport DictionarySerializer, OrderSerializer, EventSerializer
from nautilus_trader.serialization.base cimport CommandSerializer, RequestSerializer, ResponseSerializer
from nautilus_trader.serialization.base cimport LogSerializer


cdef class MsgPackDictionarySerializer(DictionarySerializer):
    pass


cdef class MsgPackOrderSerializer(OrderSerializer):
    cdef ObjectCache symbol_cache


cdef class MsgPackCommandSerializer(CommandSerializer):
    cdef IdentifierCache identifier_cache
    cdef OrderSerializer order_serializer


cdef class MsgPackEventSerializer(EventSerializer):
    cdef IdentifierCache identifier_cache


cdef class MsgPackRequestSerializer(RequestSerializer):
    cdef DictionarySerializer dict_serializer


cdef class MsgPackResponseSerializer(ResponseSerializer):
    pass


cdef class MsgPackLogSerializer(LogSerializer):
    pass
