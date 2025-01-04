# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

import uuid

from nautilus_trader.core.uuid import UUID4


def test_make_builtin_uuid(benchmark):
    benchmark(uuid.uuid4)


def test_make_nautilus_uuid(benchmark):
    benchmark(UUID4)


def test_nautilus_uuid_value(benchmark):
    uuid = UUID4()

    benchmark(lambda: uuid.value)


def test_nautilus_uuid_from_value(benchmark):
    uuid = UUID4()
    value = uuid.value

    benchmark(lambda: UUID4.from_str(value))
