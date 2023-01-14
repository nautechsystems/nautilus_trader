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

from nautilus_trader.cache.database cimport CacheDatabase
from nautilus_trader.serialization.base cimport Serializer


cdef class RedisCacheDatabase(CacheDatabase):
    cdef str _key_trader
    cdef str _key_currencies
    cdef str _key_instruments
    cdef str _key_accounts
    cdef str _key_orders
    cdef str _key_positions
    cdef str _key_strategies
    cdef str _key_commands

    cdef Serializer _serializer
    cdef object _redis
