# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.model import DataType


def test_data_type_construction():
    dt = DataType("QuoteTick", metadata={"instrument_id": "AUD/USD.SIM"})

    assert dt.type_name == "QuoteTick"
    assert dt.metadata == {"instrument_id": "AUD/USD.SIM"}


def test_data_type_equality():
    dt1 = DataType("QuoteTick", metadata={"instrument_id": "AUD/USD.SIM"})
    dt2 = DataType("QuoteTick", metadata={"instrument_id": "AUD/USD.SIM"})
    dt3 = DataType("QuoteTick", metadata={"instrument_id": "GBP/USD.SIM"})

    assert dt1 == dt2
    assert dt1 != dt3


def test_data_type_hash():
    dt1 = DataType("QuoteTick", metadata={"instrument_id": "AUD/USD.SIM"})
    dt2 = DataType("QuoteTick", metadata={"instrument_id": "AUD/USD.SIM"})

    assert hash(dt1) == hash(dt2)


def test_data_type_topic():
    dt = DataType("QuoteTick", metadata={"instrument_id": "AUD/USD.SIM"})

    assert "QuoteTick" in dt.topic
    assert "AUD/USD.SIM" in dt.topic
