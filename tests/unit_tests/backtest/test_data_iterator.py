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

import pytest

from nautilus_trader.backtest.engine import BacktestDataIterator
from nautilus_trader.test_kit.stubs.data import MyData


class TestBacktestDataIterator:
    def test_iterate_multiple_streams_sorted(self):
        """
        Test multiple streams; iterate and assert items are merged in non-decreasing
        ts_init order.
        """
        # Arrange
        it = BacktestDataIterator()
        data_len = 5
        data0 = [MyData(0, ts_init=3 * k) for k in range(data_len)]
        data1 = [MyData(1, ts_init=3 * k + 1) for k in range(data_len)]
        data2 = [MyData(2, ts_init=3 * k + 2) for k in range(data_len)]
        it.add_data("d0", data0)
        it.add_data("d1", data1)
        it.add_data("d2", data2)

        # Act
        merged = list(it)

        # Assert
        assert len(merged) == 15
        assert all(merged[i].ts_init <= merged[i + 1].ts_init for i in range(len(merged) - 1))

    def test_reset_reiterates_same_sequence(self):
        """
        Test by consuming some data, reset, and assert the full sequence repeats.
        """
        # Arrange
        it = BacktestDataIterator()
        data = [MyData(i, ts_init=i) for i in range(4)]
        it.add_data("seq", data)

        # Act
        first_pass = [x.ts_init for x in it]
        it.reset()
        second_pass = [x.ts_init for x in it]

        # Assert
        assert first_pass == second_pass == [0, 1, 2, 3]

    def test_all_data_returns_mapping(self):
        """
        Test that all_data returns a name-to-list mapping for all streams.
        """
        # Arrange
        it = BacktestDataIterator()
        lst = [MyData(0, ts_init=0)]
        it.add_data("only", lst)

        # Act
        mapping = it.all_data()

        # Assert
        assert list(mapping.keys()) == ["only"]
        assert mapping["only"] == lst

    def test_remove_stream_effect(self):
        """
        Test removing one stream affects iteration length accordingly.
        """
        # Arrange
        it = BacktestDataIterator()
        a = [MyData(0, ts_init=0)]
        b = [MyData(1, ts_init=1)]
        it.add_data("a", a)
        it.add_data("b", b)

        # Act & Assert before removal
        assert len(list(it)) == 2

        it.reset()
        it.remove_data("a")

        # Act & Assert after removal
        assert [x.value for x in it] == [1]

    def test_remove_all_streams_yields_empty(self):
        """
        Test removing all streams yields no data on iteration.
        """
        # Arrange
        it = BacktestDataIterator()
        it.add_data("x", [MyData(0, ts_init=0)])
        it.add_data("y", [MyData(1, ts_init=1)])

        # Act: remove both
        it.remove_data("x")
        it.remove_data("y")

        # Assert
        assert list(it) == []

    def test_backtest_data_iterator_callback(self):
        # Arrange
        callback_data = []

        def empty_data_callback(data_name, last_ts_init):
            callback_data.append((data_name, last_ts_init))

        data_iterator = BacktestDataIterator(empty_data_callback=empty_data_callback)

        # Create data with different lengths
        data_0 = [MyData(0, ts_init=k) for k in range(3)]  # 0, 1, 2
        data_1 = [MyData(0, ts_init=k) for k in range(5)]  # 0, 1, 2, 3, 4

        # Act
        data_iterator.add_data("short", data_0)
        data_iterator.add_data("long", data_1)

        # Consume all data
        _ = list(data_iterator)

        # Assert - Callbacks were called for both data streams
        # The callback is called when we try to access data beyond what's available
        assert len(callback_data) == 2

        # Check that both data streams triggered callbacks
        data_names = [item[0] for item in callback_data]
        assert "short" in data_names
        assert "long" in data_names

    def test_single_data_mode_and_no_callback(self):
        """
        Test single-stream mode should yield data in order without invoking the empty-
        data callback.
        """
        # Arrange
        callback_data = []

        def cb(name, ts):
            callback_data.append((name, ts))

        it = BacktestDataIterator(empty_data_callback=cb)
        data = [MyData(v, ts_init=v) for v in [1, 2, 3]]
        it.add_data("single", data)

        # Act: consume all items
        first = next(it).value
        second = next(it).value
        third = next(it).value
        with pytest.raises(StopIteration):
            next(it)

        # Reset and re-consume
        it.reset()
        callback_data = []
        values = [x.value for x in it]

        # Assert
        assert (first, second, third) == (1, 2, 3)
        assert callback_data == [("single", 3)]
        assert it.is_done()

        assert values == [1, 2, 3]
        assert it.is_done()

    def test_append_data_priority_changes_order(self):
        """
        Test two streams with identical ts_init: default append_data=True yields FIFO,
        append_data=False yields reversed insertion priority.
        """
        # Arrange
        data_a = [MyData(0, ts_init=100)]
        data_b = [MyData(1, ts_init=100)]

        # Act
        it1 = BacktestDataIterator()
        it1.add_data("a", data_a)
        it1.add_data("b", data_b)
        order1 = [x.value for x in it1]

        it2 = BacktestDataIterator()
        it2.add_data("a", data_a)
        it2.add_data("b", data_b, append_data=False)
        order2 = [x.value for x in it2]

        # Assert
        assert order1 == [0, 1]
        assert order2 == [1, 0]

    def test_set_index_and_data_accessor_and_is_done_empty(self):
        """
        Test is_done on empty iterator, data() accessor, and set_index restart.
        """
        # Arrange: empty iterator
        it = BacktestDataIterator()

        # Initially done and empty
        assert it.is_done()
        assert list(it) == []

        # Removing non-existent stream should be no-op
        it.remove_data("nope")

        # Add a data stream
        data = [MyData(10, ts_init=10), MyData(20, ts_init=20), MyData(30, ts_init=30)]
        it.add_data("stream", data)

        assert it.data("stream") == data

        with pytest.raises(KeyError):
            it.data("unknown")

        # Consume first element and reset index
        first = next(it).value
        it.set_index("stream", 0)
        remaining = [x.value for x in it]

        # Assert: correct restart and done state
        assert first == 10
        assert remaining == [10, 20, 30]
        assert it.is_done()

    def test_all_data_order_and_add_empty_list(self):
        """
        Test that all_data preserves insertion order and ignores empty streams.
        """
        # Arrange
        it = BacktestDataIterator()
        it.add_data("first", [MyData(1, ts_init=1)])
        it.add_data("second", [MyData(2, ts_init=2)])

        # Act
        keys_before = list(it.all_data().keys())
        it.add_data("empty", [])
        keys_after = list(it.all_data().keys())

        # Assert: empty list did not alter keys
        assert keys_before == ["first", "second"]
        assert keys_after == ["first", "second"]

    def test_readding_data_replaces_old(self):
        """
        Test adding a stream under an existing name replaces its data.
        """
        # Arrange
        it = BacktestDataIterator()
        data1 = [MyData(1, ts_init=1), MyData(2, ts_init=2)]
        it.add_data("X", data1)

        # Act: initial iteration yields old data
        assert [x.value for x in it] == [1, 2]

        # new data under same name
        data2 = [MyData(3, ts_init=3)]
        it.add_data("X", data2)
        it.reset()

        # Assert: iteration yields only new data
        assert [x.value for x in it] == [3]
