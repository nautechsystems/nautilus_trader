# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2021 Nautech Systems Pty Ltd. All rights reserved.
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


cdef class BacktestDataContainer:
    cdef readonly dict clients
    cdef readonly list generic_data
    cdef readonly list order_book_snapshots
    cdef readonly list order_book_operations
    cdef readonly dict instruments
    cdef readonly dict quote_ticks
    cdef readonly dict trade_ticks
    cdef readonly dict bars_bid
    cdef readonly dict bars_ask
