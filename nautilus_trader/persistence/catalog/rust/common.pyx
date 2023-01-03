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

from nautilus_trader.core.rust.model cimport QuoteTick_t
from nautilus_trader.core.rust.model cimport TradeTick_t
from nautilus_trader.core.rust.persistence cimport ParquetType
from nautilus_trader.model.data.tick cimport QuoteTick
from nautilus_trader.model.data.tick cimport TradeTick


def py_type_to_parquet_type(type cls):
    if cls == QuoteTick:
        return ParquetType.QuoteTick
    elif cls == TradeTick:
        return ParquetType.TradeTick
    else:
        raise RuntimeError(f"Type {cls} not supported as a `ParquetType` yet.")


def parquet_type_to_struct_size(ParquetType parquet_type):
    if parquet_type == ParquetType.QuoteTick:
        return sizeof(QuoteTick_t)
    elif parquet_type == ParquetType.TradeTick:
        return sizeof(TradeTick_t)
    else:
        raise RuntimeError(f"`ParquetType` {parquet_type} not supported yet.")
