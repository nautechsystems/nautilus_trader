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


import pandas as pd
import pytest

from nautilus_trader.persistence.external.util import Singleton
from nautilus_trader.persistence.external.util import clear_singleton_instances
from nautilus_trader.persistence.external.util import is_filename_in_time_range
from nautilus_trader.persistence.external.util import parse_filename
from nautilus_trader.persistence.external.util import parse_filename_start
from nautilus_trader.persistence.external.util import resolve_kwargs


def test_resolve_kwargs():
    def func1():
        pass

    def func2(a, b, c):
        pass

    assert resolve_kwargs(func1) == {}
    assert resolve_kwargs(func2, 1, 2, 3) == {"a": 1, "b": 2, "c": 3}
    assert resolve_kwargs(func2, 1, 2, c=3) == {"a": 1, "b": 2, "c": 3}
    assert resolve_kwargs(func2, 1, c=3, b=2) == {"a": 1, "b": 2, "c": 3}
    assert resolve_kwargs(func2, a=1, b=2, c=3) == {"a": 1, "b": 2, "c": 3}


def test_singleton_without_init():
    # Arrange
    class Test(metaclass=Singleton):
        pass

    # Arrange
    test1 = Test()
    test2 = Test()

    # Assert
    assert test1 is test2


def test_singleton_with_init():
    # Arrange
    class Test(metaclass=Singleton):
        def __init__(self, a, b):
            self.a = a
            self.b = b

    # Act
    test1 = Test(1, 1)
    test2 = Test(1, 1)
    test3 = Test(1, 2)

    # Assert
    assert test1 is test2
    assert test2 is not test3


def test_clear_instance():
    # Arrange
    class Test(metaclass=Singleton):
        pass

    # Act
    Test()
    assert Test._instances

    clear_singleton_instances(Test)

    # Assert
    assert not Test._instances


def test_dict_kwarg():
    # Arrange
    class Test(metaclass=Singleton):
        def __init__(self, a, b):
            self.a = a
            self.b = b

    # Act
    test1 = Test(1, b={"hello": "world"})

    # Assert
    assert test1.a == 1
    assert test1.b == {"hello": "world"}
    instances = {(("a", 1), ("b", (("hello", "world"),))): test1}
    assert Test._instances == instances


@pytest.mark.parametrize(
    "filename, expected",
    [
        [
            "1577836800000000000-1578182400000000000-0.parquet",
            (1577836800000000000, 1578182400000000000),
        ],
        [
            "/data/test/sample.parquet/instrument_id=a/1577836800000000000-1578182400000000000-0.parquet",
            (None, None),
        ],
    ],
)
def test_parse_filename(filename, expected):
    assert parse_filename(filename) == expected


@pytest.mark.parametrize(
    "filename, start, end, expected",
    [
        [
            "1546383600000000000-1577826000000000000-SIM-1-HOUR-BID-EXTERNAL-0.parquet",
            0,
            9223372036854775807,
            True,
        ],
        [
            "0000000000000000005-0000000000000000008-0.parquet",
            4,
            7,
            True,
        ],
        [
            "0000000000000000005-0000000000000000008-0.parquet",
            6,
            9,
            True,
        ],
        [
            "0000000000000000005-0000000000000000008-0.parquet",
            6,
            7,
            True,
        ],
        [
            "0000000000000000005-0000000000000000008-0.parquet",
            4,
            9,
            True,
        ],
        [
            "0000000000000000005-0000000000000000008-0.parquet",
            7,
            10,
            True,
        ],
        [
            "0000000000000000005-0000000000000000008-0.parquet",
            9,
            10,
            False,
        ],
        [
            "0000000000000000005-0000000000000000008-0.parquet",
            2,
            4,
            False,
        ],
        [
            "0000000000000000005-0000000000000000008-0.parquet",
            0,
            9223372036854775807,
            True,
        ],
    ],
)
def test_is_filename_in_time_range(filename, start, end, expected):
    assert is_filename_in_time_range(filename, start, end) is expected


@pytest.mark.parametrize(
    "filename, expected",
    [
        [
            "/data/test/sample.parquet/instrument_id=a/1577836800000000000-1578182400000000000-0.parquet",
            ("a", pd.Timestamp("2020-01-01 00:00:00")),
        ],
        [
            "1546383600000000000-1577826000000000000-SIM-1-HOUR-BID-EXTERNAL-0.parquet",
            (None, pd.Timestamp("2019-01-01 23:00:00")),
        ],
        [
            "/data/test/sample.parquet/instrument_id=a/0648140b1fd7491a97983c0c6ece8d57.parquet",
            None,
        ],
        [
            "0648140b1fd7491a97983c0c6ece8d57.parquet",
            None,
        ],
    ],
)
def test_parse_filename_start(filename, expected):
    assert parse_filename_start(filename) == expected
