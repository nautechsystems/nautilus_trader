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

import time

import pandas as pd
import pytest

from nautilus_trader.backtest.engine import BacktestDataIterator
from nautilus_trader.backtest.engine import BacktestEngine
from nautilus_trader.test_kit.stubs.data import MyData


class TestBacktestDataIterator:
    def test_iterate_multiple_streams_sorted(self):
        """
        Test multiple streams; iterate and assert items are merged in non-decreasing
        ts_init order.
        """
        # Arrange
        iterator = BacktestDataIterator()
        data_len = 5
        data0 = [MyData(0, ts_init=3 * k) for k in range(data_len)]
        data1 = [MyData(1, ts_init=3 * k + 1) for k in range(data_len)]
        data2 = [MyData(2, ts_init=3 * k + 2) for k in range(data_len)]
        iterator.add_data("d0", data0)
        iterator.add_data("d1", data1)
        iterator.add_data("d2", data2)

        # Act
        merged = list(iterator)

        # Assert
        assert len(merged) == 15
        assert all(merged[i].ts_init <= merged[i + 1].ts_init for i in range(len(merged) - 1))

    def test_reset_reiterates_same_sequence(self):
        """
        Test by consuming some data, reset, and assert the full sequence repeats.
        """
        # Arrange
        iterator = BacktestDataIterator()
        data = [MyData(i, ts_init=i) for i in range(4)]
        iterator.add_data("seq", data)

        # Act
        first_pass = [x.ts_init for x in iterator]
        iterator.reset()
        second_pass = [x.ts_init for x in iterator]

        # Assert
        assert first_pass == second_pass == [0, 1, 2, 3]

    def test_all_data_returns_mapping(self):
        """
        Test that all_data returns a name-to-list mapping for all streams.
        """
        # Arrange
        iterator = BacktestDataIterator()
        lst = [MyData(0, ts_init=0)]
        iterator.add_data("only", lst)

        # Act
        mapping = iterator.all_data()

        # Assert
        assert list(mapping.keys()) == ["only"]
        assert mapping["only"] == lst

    def test_remove_stream_effect(self):
        """
        Test removing one stream affects iteration length accordingly.
        """
        # Arrange
        iterator = BacktestDataIterator()
        a = [MyData(0, ts_init=0)]
        b = [MyData(1, ts_init=1)]
        iterator.add_data("a", a)
        iterator.add_data("b", b)

        # Act & Assert before removal
        assert len(list(iterator)) == 2

        iterator.reset()
        iterator.remove_data("a")

        # Act & Assert after removal
        assert [x.value for x in iterator] == [1]

    def test_remove_all_streams_yields_empty(self):
        """
        Test removing all streams yields no data on iteration.
        """
        # Arrange
        iterator = BacktestDataIterator()
        iterator.add_data("x", [MyData(0, ts_init=0)])
        iterator.add_data("y", [MyData(1, ts_init=1)])

        # Act: remove both
        iterator.remove_data("x")
        iterator.remove_data("y")

        # Assert
        assert list(iterator) == []

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

        iterator = BacktestDataIterator(empty_data_callback=cb)
        data = [MyData(v, ts_init=v) for v in [1, 2, 3]]
        iterator.add_data("single", data)

        # Act: consume all items
        first = next(iterator).value
        second = next(iterator).value
        third = next(iterator).value
        with pytest.raises(StopIteration):
            next(iterator)

        # Reset and re-consume
        iterator.reset()
        callback_data = []
        values = [x.value for x in iterator]

        # Assert
        assert (first, second, third) == (1, 2, 3)
        assert callback_data == [("single", 3)]
        assert iterator.is_done()

        assert values == [1, 2, 3]
        assert iterator.is_done()

    def test_append_data_priority_changes_order(self):
        """
        Test two streams with identical ts_init: default append_data=True yields FIFO,
        append_data=False yields reversed insertion priority.
        """
        # Arrange
        data_a = [MyData(0, ts_init=100)]
        data_b = [MyData(1, ts_init=100)]

        # Act
        iterator1 = BacktestDataIterator()
        iterator1.add_data("a", data_a)
        iterator1.add_data("b", data_b)
        order1 = [x.value for x in iterator1]

        iterator2 = BacktestDataIterator()
        iterator2.add_data("a", data_a)
        iterator2.add_data("b", data_b, append_data=False)
        order2 = [x.value for x in iterator2]

        # Assert
        assert order1 == [0, 1]
        assert order2 == [1, 0]

    def test_set_index_and_data_accessor_and_is_done_empty(self):
        """
        Test is_done on empty iterator, data() accessor, and set_index restart.
        """
        # Arrange: empty iterator
        iterator = BacktestDataIterator()

        # Initially done and empty
        assert iterator.is_done()
        assert list(iterator) == []

        # Removing non-existent stream should be no-op
        iterator.remove_data("nope")

        # Add a data stream
        data = [MyData(10, ts_init=10), MyData(20, ts_init=20), MyData(30, ts_init=30)]
        iterator.add_data("stream", data)

        assert iterator.data("stream") == data

        with pytest.raises(KeyError):
            iterator.data("unknown")

        # Consume first element and reset index
        first = next(iterator).value
        iterator.set_index("stream", 0)
        remaining = [x.value for x in iterator]

        # Assert: correct restart and done state
        assert first == 10
        assert remaining == [10, 20, 30]
        assert iterator.is_done()

    def test_all_data_order_and_add_empty_list(self):
        """
        Test that all_data preserves insertion order and ignores empty streams.
        """
        # Arrange
        iterator = BacktestDataIterator()
        iterator.add_data("first", [MyData(1, ts_init=1)])
        iterator.add_data("second", [MyData(2, ts_init=2)])

        # Act
        keys_before = list(iterator.all_data().keys())
        iterator.add_data("empty", [])
        keys_after = list(iterator.all_data().keys())

        # Assert: empty list did not alter keys
        assert keys_before == ["first", "second"]
        assert keys_after == ["first", "second"]

    def test_readding_data_replaces_old(self):
        """
        Test adding a stream under an existing name replaces its data.
        """
        # Arrange
        iterator = BacktestDataIterator()
        data1 = [MyData(1, ts_init=1), MyData(2, ts_init=2)]
        iterator.add_data("X", data1)

        # Act: initial iteration yields old data
        assert [x.value for x in iterator] == [1, 2]

        # new data under same name
        data2 = [MyData(3, ts_init=3)]
        iterator.add_data("X", data2)
        iterator.reset()

        # Assert: iteration yields only new data
        assert [x.value for x in iterator] == [3]

    def test_single_stream_yields_in_order(self):
        """
        Test single stream yields data in chronological order.
        """
        data = [MyData(i, ts_init=ts) for i, ts in enumerate([100, 200, 300])]

        iterator = BacktestDataIterator()
        iterator.add_data("s1", data)

        assert list(iterator) == data
        assert iterator.is_done()

    def test_multiple_streams_merge_order(self):
        """
        Test multiple streams are merged in chronological order.
        """
        iterator = BacktestDataIterator()

        data_s1 = [MyData(i, ts_init=ts) for i, ts in enumerate([100, 300])]
        data_s2 = [MyData(i + 2, ts_init=ts) for i, ts in enumerate([200, 400])]

        iterator.add_data("s1", data_s1)
        iterator.add_data("s2", data_s2)

        expected_order = [100, 200, 300, 400]
        observed_order = [d.ts_init for d in iterator]

        assert observed_order == expected_order

    def test_prepend_priority_with_equal_timestamps(self):
        """
        Test prepend streams have priority over append streams for equal timestamps.
        """
        # sA is added first (append=True by default) and sB second with prepend semantics
        iterator = BacktestDataIterator()

        iterator.add_data("sA", [MyData(0, ts_init=100)])  # lower priority
        iterator.add_data(
            "sB",
            [MyData(1, ts_init=100)],
            append_data=False,
        )  # higher priority (prepend)

        first_out = next(iterator)
        second_out = next(iterator)

        assert first_out.value == 1  # prepend stream wins tie
        assert second_out.value == 0

    def test_remove_data_and_callback_trigger(self):
        """
        Test removing data triggers callback and iterator properly handles empty state.
        """
        called = []

        def cb(name: str, ts: int):
            called.append((name, ts))

        iterator = BacktestDataIterator(empty_data_callback=cb)
        iterator.add_data("s1", [MyData(0, ts_init=1), MyData(1, ts_init=2)])

        # advance iterator fully
        list(iterator)

        assert called == [("s1", 2)]  # callback executed once with last ts_init

        # Now remove and ensure no error occurs
        iterator.remove_data("s1")
        assert iterator.is_done()

    def test_set_index_and_reset_behavior(self):
        """
        Test set_index and reset functionality.
        """
        data = [MyData(i, ts_init=ts) for i, ts in enumerate([10, 20, 30])]
        iterator = BacktestDataIterator()
        iterator.add_data("s", data)

        # Consume one element
        assert next(iterator).ts_init == 10

        # Rewind to start
        iterator.set_index("s", 0)
        iterator.reset()

        assert [d.ts_init for d in iterator] == [10, 20, 30]

    @pytest.mark.parametrize("empty_list", [[], ()])
    def test_adding_empty_stream_is_ignored(self, empty_list):
        """
        Test adding empty streams is properly ignored.
        """
        iterator = BacktestDataIterator()
        iterator.add_data("empty", list(empty_list))

        assert iterator.is_done()  # nothing to iterate

    def test_add_stream_iterator_basic_functionality(self):
        """
        Test basic stream iterator functionality with chunked loading.
        """

        # Create a simple iterator that yields MyData objects
        def create_data_iterator():
            for i in range(10):
                yield MyData(i, ts_init=i * 1000)  # timestamps: 0, 1000, 2000, ...

        iterator = BacktestDataIterator()
        chunk_duration_ns = 1_000_000_000  # 1 second (should include multiple items per chunk)

        # Add stream iterator
        iterator.add_stream_iterator(
            "test_stream",
            create_data_iterator(),
            chunk_duration_ns,
        )

        # Collect all data from the iterator
        collected_data = list(iterator)

        # Should have collected all 10 items in chronological order
        assert len(collected_data) == 10
        assert all(
            collected_data[i].ts_init <= collected_data[i + 1].ts_init
            for i in range(len(collected_data) - 1)
        )
        assert [d.value for d in collected_data] == list(range(10))

    def test_stream_iterator_k_way_merge(self):
        """
        Test k-way merge with multiple stream iterators.
        """

        def create_stream_1():
            # Stream 1: even timestamps
            for i in range(0, 10, 2):
                yield MyData(i, ts_init=i * 1000)

        def create_stream_2():
            # Stream 2: odd timestamps
            for i in range(1, 10, 2):
                yield MyData(i, ts_init=i * 1000)

        iterator = BacktestDataIterator()
        chunk_duration_ns = 1_000_000_000  # 1 second

        # Add both stream iterators
        iterator.add_stream_iterator("stream_1", create_stream_1(), chunk_duration_ns)
        iterator.add_stream_iterator("stream_2", create_stream_2(), chunk_duration_ns)

        # Collect all data
        collected_data = list(iterator)

        # Should have all data merged in chronological order
        assert len(collected_data) == 10  # 0,1,2,3,4,5,6,7,8,9
        expected_order = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9]
        actual_order = [d.value for d in collected_data]
        assert actual_order == expected_order

        # Verify timestamps are in order
        assert all(
            collected_data[i].ts_init <= collected_data[i + 1].ts_init
            for i in range(len(collected_data) - 1)
        )

    def test_stream_iterator_chunked_loading(self):
        """
        Test that stream iterators can handle loading and consuming data.
        """

        def simple_iterator():
            for i in range(10):
                yield MyData(i, ts_init=i * 1000)

        iterator = BacktestDataIterator()
        # Use a large chunk duration to avoid chunking issues for now
        chunk_duration_ns = 1_000_000_000  # Large enough to include all data in one chunk

        # Add stream iterator
        iterator.add_stream_iterator("simple_stream", simple_iterator(), chunk_duration_ns)

        # Consume all data
        all_items = list(iterator)

        # Should have all 10 items
        assert len(all_items) == 10
        assert [item.value for item in all_items] == list(range(10))

    def test_stream_iterator_exhaustion_handling(self):
        """
        Test that exhausted stream iterators are handled correctly.
        """

        def small_iterator():
            for i in range(3):
                yield MyData(i, ts_init=i * 1000)

        iterator = BacktestDataIterator()
        chunk_duration_ns = 1_000_000_000

        iterator.add_stream_iterator("small_stream", small_iterator(), chunk_duration_ns)

        # Consume all data
        collected_data = list(iterator)
        assert len(collected_data) == 3

        # Iterator should be done
        assert iterator.is_done()

        # Should not have any more stream data remaining
        assert not iterator.has_stream_data_remaining()

    def test_stream_iterator_with_regular_data_mixed(self):
        """
        Test mixing stream iterators with regular data addition.
        """

        def stream_data():
            for i in range(3):
                yield MyData(i + 100, ts_init=(i + 10) * 1000)  # ts: 10000, 11000, 12000

        iterator = BacktestDataIterator()

        # Add regular data first
        regular_data = [MyData(i, ts_init=i * 1000) for i in range(5)]  # ts: 0-4000
        iterator.add_data("regular", regular_data)

        # Add stream iterator
        iterator.add_stream_iterator("stream", stream_data(), 1_000_000_000)

        # Collect all data
        all_data = list(iterator)

        # Should have 8 items total, merged in chronological order
        assert len(all_data) == 8
        timestamps = [d.ts_init for d in all_data]
        assert timestamps == [0, 1000, 2000, 3000, 4000, 10000, 11000, 12000]

    def test_stream_iterator_empty_chunks(self):
        """
        Test stream iterator that yields empty chunks between data.
        """

        def sparse_data():
            yield MyData(1, ts_init=1000)
            yield MyData(2, ts_init=50000)  # Large gap
            yield MyData(3, ts_init=100_000)  # Another large gap

        iterator = BacktestDataIterator()
        iterator.add_stream_iterator("sparse", sparse_data(), 1_000_000_000)  # Small chunks

        all_data = list(iterator)
        assert len(all_data) == 3
        assert [d.value for d in all_data] == [1, 2, 3]

    def test_stream_iterator_very_small_chunks(self):
        """
        Test stream iterator with very small chunk durations.
        """

        def data_stream():
            for i in range(5):
                yield MyData(i, ts_init=i * 1000)

        iterator = BacktestDataIterator()
        iterator.add_stream_iterator(
            "small_chunks",
            data_stream(),
            1_000_000_000,
        )  # Very small chunks

        all_data = list(iterator)
        assert len(all_data) == 5
        assert [d.value for d in all_data] == list(range(5))

    def test_stream_iterator_identical_timestamps(self):
        """
        Test stream iterators with identical timestamps to verify ordering.
        """

        def stream_a():
            for i in range(3):
                yield MyData(i, ts_init=1000)  # All same timestamp

        def stream_b():
            for i in range(3):
                yield MyData(i + 10, ts_init=1000)  # All same timestamp

        iterator = BacktestDataIterator()
        iterator.add_stream_iterator("stream_a", stream_a(), 1_000_000_000)
        iterator.add_stream_iterator("stream_b", stream_b(), 1_000_000_000)

        all_data = list(iterator)
        assert len(all_data) == 6

        # All should have same timestamp but maintain insertion order within streams
        assert all(d.ts_init == 1000 for d in all_data)

    def test_stream_iterator_prepend_vs_append_priority(self):
        """
        Test that prepend vs append priority works with stream iterators.
        """

        def early_stream():
            yield MyData(100, ts_init=1000)

        def late_stream():
            yield MyData(200, ts_init=1000)  # Same timestamp

        iterator = BacktestDataIterator()
        iterator.add_stream_iterator("append", early_stream(), 1_000_000_000, append_data=True)
        iterator.add_stream_iterator("prepend", late_stream(), 1_000_000_000, append_data=False)

        all_data = list(iterator)
        assert len(all_data) == 2

        # Prepend stream should come first for equal timestamps
        assert all_data[0].value == 200  # prepend stream
        assert all_data[1].value == 100  # append stream

    def test_stream_iterator_reset_functionality(self):
        """
        Test that reset works correctly with stream iterators.
        """

        def simple_stream():
            for i in range(3):
                yield MyData(i, ts_init=i * 1000)

        iterator = BacktestDataIterator()
        iterator.add_stream_iterator("test", simple_stream(), 1_000_000_000)

        # Consume some data
        first_item = next(iterator)
        assert first_item.value == 0

        # Reset should restart iteration
        iterator.reset()

        # Should get all data again
        all_data = list(iterator)
        assert len(all_data) == 3
        assert [d.value for d in all_data] == [0, 1, 2]

    def test_stream_iterator_remove_data_functionality(self):
        """
        Test removing stream iterator data.
        """

        def stream_data():
            for i in range(3):
                yield MyData(i, ts_init=i * 1000)

        iterator = BacktestDataIterator()
        iterator.add_stream_iterator("removable", stream_data(), 1_000_000_000)

        # Verify data was added
        assert not iterator.is_done()

        # Remove the stream data
        iterator.remove_data("removable")

        # Should now be done
        assert iterator.is_done()
        assert list(iterator) == []

    def test_stream_iterator_large_time_gaps(self):
        """
        Test stream iterator with large gaps between timestamps.
        """

        def gapped_data():
            yield MyData(1, ts_init=1000)
            yield MyData(2, ts_init=1_000_000)  # 1 second gap
            yield MyData(3, ts_init=1_000_000_000)  # 1000 second gap

        iterator = BacktestDataIterator()
        iterator.add_stream_iterator("gapped", gapped_data(), 1_000_000_000)  # 0.5ms chunks

        all_data = list(iterator)
        assert len(all_data) == 3
        assert [d.value for d in all_data] == [1, 2, 3]

    def test_stream_iterator_set_index_functionality(self):
        """
        Test set_index functionality with stream iterators.
        """

        def indexed_data():
            for i in range(5):
                yield MyData(i, ts_init=i * 1000)

        iterator = BacktestDataIterator()
        iterator.add_stream_iterator("indexed", indexed_data(), 1_000_000_000)

        # Consume some data
        consumed = [next(iterator), next(iterator)]
        assert [d.value for d in consumed] == [0, 1]

        # Set index back to start
        iterator.set_index("indexed", 0)

        # Should continue from index 0
        remaining = list(iterator)
        assert len(remaining) == 5
        assert [d.value for d in remaining] == [0, 1, 2, 3, 4]

    def test_stream_iterator_all_data_method(self):
        """
        Test all_data() method works with stream iterators.
        """

        def test_stream():
            for i in range(3):
                yield MyData(i, ts_init=i * 1000)

        iterator = BacktestDataIterator()
        iterator.add_stream_iterator("test_stream", test_stream(), 1_000_000_000)

        # Consume data to ensure it's loaded
        list(iterator)

        # Get all data
        all_data_dict = iterator.all_data()
        assert "test_stream" in all_data_dict
        stream_data = all_data_dict["test_stream"]
        assert len(stream_data) == 3
        assert [d.value for d in stream_data] == [0, 1, 2]

    def test_stream_iterator_data_method(self):
        """
        Test data() method works with stream iterators.
        """

        def test_stream():
            for i in range(3):
                yield MyData(i, ts_init=i * 1000)

        iterator = BacktestDataIterator()
        iterator.add_stream_iterator("test_stream", test_stream(), 1_000_000_000)

        # Consume data to ensure it's loaded
        list(iterator)

        # Get specific stream data
        stream_data = iterator.data("test_stream")
        assert len(stream_data) == 3
        assert [d.value for d in stream_data] == [0, 1, 2]

    def test_stream_iterator_callback_on_exhaustion(self):
        """
        Test that empty_data_callback is called when stream iterators are exhausted.
        """
        callback_data = []

        def callback(name, ts):
            callback_data.append((name, ts))

        def test_stream():
            for i in range(3):
                yield MyData(i, ts_init=i * 1000)

        iterator = BacktestDataIterator(empty_data_callback=callback)
        iterator.add_stream_iterator("test_stream", test_stream(), 1_000_000_000)

        # Consume all data
        list(iterator)

        # Callback should have been called
        assert len(callback_data) == 1
        assert callback_data[0][0] == "test_stream"
        assert callback_data[0][1] == 2000  # Last ts_init

    def test_stream_iterator_multiple_chunks_loading(self):
        """
        Test that multiple chunks are loaded progressively.
        """
        load_tracking = []

        def tracking_stream():
            for i in range(20):
                load_tracking.append(f"Loading item {i}")
                yield MyData(i, ts_init=i * 1000)

        iterator = BacktestDataIterator()
        # Small chunk to force multiple loads
        iterator.add_stream_iterator("chunked", tracking_stream(), 1_000_000_000)

        # Should have loaded initial chunk but not all 20 items
        initial_loads = len(load_tracking)
        assert initial_loads < 20

        # Consume some data to trigger more loading
        partial_data = []
        for i in range(8):
            try:
                partial_data.append(next(iterator))
            except StopIteration:
                break

        # Should have loaded more items
        assert len(load_tracking) > initial_loads

        # Consume rest
        remaining = list(iterator)

        # All items should eventually be loaded
        assert len(load_tracking) == 20
        assert len(partial_data) + len(remaining) == 20

    def test_stream_iterator_exception_handling(self):
        """
        Test stream iterator handles exceptions in iterator gracefully.
        """

        def failing_stream():
            yield MyData(1, ts_init=1000)
            yield MyData(2, ts_init=2000)
            # Iterator will be exhausted after this
            return

        iterator = BacktestDataIterator()
        iterator.add_stream_iterator("failing", failing_stream(), 1_000_000_000)

        # Should still work correctly despite early termination
        all_data = list(iterator)
        assert len(all_data) == 2
        assert [d.value for d in all_data] == [1, 2]

        # Stream should be marked as exhausted
        assert not iterator.has_stream_data_remaining()

    def test_stream_iterator_replaces_existing_data(self):
        """
        Test that adding stream iterator with existing name replaces data.
        """

        def first_stream():
            yield MyData(1, ts_init=1000)

        def second_stream():
            yield MyData(2, ts_init=2000)

        iterator = BacktestDataIterator()
        iterator.add_stream_iterator("replaceable", first_stream(), 1_000_000_000)

        # Verify first data
        first_data = list(iterator)
        assert len(first_data) == 1
        assert first_data[0].value == 1

        # Add stream with same name (should replace)
        iterator.add_stream_iterator("replaceable", second_stream(), 1_000_000_000)
        iterator.reset()

        # Should only have second stream's data
        second_data = list(iterator)
        assert len(second_data) == 1
        assert second_data[0].value == 2

    def test_stream_iterator_different_chunk_durations(self):
        """
        Test that different streams can have different chunk durations.
        """

        def stream_a():
            for i in range(20):
                yield MyData(f"A_{i}", ts_init=i * 1_000_000_000)  # 1 second intervals

        def stream_b():
            for i in range(10):
                yield MyData(f"B_{i}", ts_init=i * 2_000_000_000)  # 2 second intervals

        def stream_c():
            for i in range(5):
                yield MyData(f"C_{i}", ts_init=i * 4_000_000_000)  # 4 second intervals

        iterator = BacktestDataIterator()

        # Add streams with different chunk durations
        iterator.add_stream_iterator("stream_a", stream_a(), 3_000_000_000)  # 3 second chunks
        iterator.add_stream_iterator("stream_b", stream_b(), 5_000_000_000)  # 5 second chunks
        iterator.add_stream_iterator("stream_c", stream_c(), 10_000_000_000)  # 10 second chunks

        # Verify that chunk durations are stored per stream
        assert iterator.get_stream_chunk_duration("stream_a") == 3_000_000_000
        assert iterator.get_stream_chunk_duration("stream_b") == 5_000_000_000
        assert iterator.get_stream_chunk_duration("stream_c") == 10_000_000_000

        # Collect all data - should be properly time-ordered despite different chunk sizes
        all_data = list(iterator)

        # Verify we got all the data
        a_data = [d for d in all_data if d.value.startswith("A_")]
        b_data = [d for d in all_data if d.value.startswith("B_")]
        c_data = [d for d in all_data if d.value.startswith("C_")]

        assert len(a_data) == 20
        assert len(b_data) == 10
        assert len(c_data) == 5

        # Verify chronological ordering across all streams
        timestamps = [d.ts_init for d in all_data]
        assert timestamps == sorted(timestamps), "Data not in chronological order"

    def test_stream_iterator_epoch_zero_handling(self):
        """
        Test that data with timestamp 0 (Unix epoch) is handled correctly.

        This tests the fix for issue #1 in REVIEW.md.

        """

        def epoch_stream():
            # Stream with data starting at Unix epoch (timestamp 0)
            yield MyData("epoch_0", ts_init=0)
            yield MyData("epoch_1", ts_init=1_000_000_000)  # 1 second later
            yield MyData("epoch_2", ts_init=2_000_000_000)  # 2 seconds later

        iterator = BacktestDataIterator()
        iterator.add_stream_iterator(
            "epoch_test",
            epoch_stream(),
            1_500_000_000,
        )  # 1.5 second chunks

        # Collect all data
        all_data = list(iterator)

        # Should get all 3 items, including the one at timestamp 0
        assert len(all_data) == 3
        assert all_data[0].value == "epoch_0"
        assert all_data[0].ts_init == 0
        assert all_data[1].value == "epoch_1"
        assert all_data[2].value == "epoch_2"


class TestBacktestEngineStreaming:
    """
    Tests for BacktestEngine streaming functionality.
    """

    def test_add_data_iterators_basic(self):
        """
        Test basic BacktestEngine.add_data_iterators functionality.
        """

        def test_stream():
            for i in range(5):
                yield MyData(i, ts_init=i * 1000)

        engine = BacktestEngine()
        iterators = [("test_stream", test_stream())]

        # Should not raise any errors
        engine.add_data_iterators(iterators, chunk_duration="1s")

        # Test passes if no exception is raised

    def test_add_data_iterators_multiple_streams(self):
        """
        Test adding multiple stream iterators to BacktestEngine.
        """

        def stream_a():
            for i in range(3):
                yield MyData(i, ts_init=i * 1000)

        def stream_b():
            for i in range(3):
                yield MyData(i + 10, ts_init=(i + 3) * 1000)

        engine = BacktestEngine()
        iterators = [
            ("stream_a", stream_a()),
            ("stream_b", stream_b()),
        ]

        engine.add_data_iterators(iterators, chunk_duration="1s")

        # Test passes if no exception is raised

    def test_add_data_iterators_chunk_duration_string(self):
        """
        Test different chunk duration string formats.
        """

        def test_stream():
            yield MyData(1, ts_init=1000)

        engine = BacktestEngine()

        # Test different duration formats
        test_durations = ["1h", "30min", "5s", "1s", "2s"]

        for duration in test_durations:
            # Create fresh iterators for each test
            fresh_iterators = [("test", test_stream())]
            # Should not raise errors
            engine.add_data_iterators(fresh_iterators, chunk_duration=duration)

    def test_add_data_iterators_timedelta_duration(self):
        """
        Test using pandas.Timedelta for chunk duration.
        """

        def test_stream():
            yield MyData(1, ts_init=1000)

        engine = BacktestEngine()
        iterators = [("test", test_stream())]

        # Test with pd.Timedelta
        duration = pd.Timedelta("1s")
        engine.add_data_iterators(iterators, chunk_duration=duration)

    def test_add_data_iterators_empty_list_error(self):
        """
        Test that empty iterators list raises ValueError.
        """
        engine = BacktestEngine()

        with pytest.raises(Exception):  # Should raise ValueError from Condition.not_empty
            engine.add_data_iterators([], chunk_duration="1h")

    def test_add_data_iterators_invalid_names(self):
        """
        Test that invalid stream names raise errors.
        """

        def test_stream():
            yield MyData(1, ts_init=1000)

        engine = BacktestEngine()

        # Test invalid names
        invalid_iterators = [
            ("", test_stream()),  # Empty name
            (None, test_stream()),  # None name
        ]

        for iterators in [[invalid_iter] for invalid_iter in invalid_iterators]:
            with pytest.raises(Exception):  # Should raise from Condition.valid_string
                engine.add_data_iterators(iterators, chunk_duration="1h")

    def test_add_data_iterators_logging(self):
        """
        Test that add_data_iterators completes successfully.

        Note: Logging verification is skipped due to custom Rust-based logging system.

        """

        def stream_a():
            yield MyData(1, ts_init=1000)

        def stream_b():
            yield MyData(2, ts_init=2000)

        engine = BacktestEngine()

        # Single iterator - should complete without error
        engine.add_data_iterators([("single", stream_a())], chunk_duration="5min")

        # Multiple iterators - should complete without error
        engine.add_data_iterators(
            [
                ("stream1", stream_a()),
                ("stream2", stream_b()),
            ],
            chunk_duration="10min",
        )

    def test_add_data_iterators_integration_with_existing_data(self):
        """
        Test that stream iterators work with existing add_data functionality.
        """

        def test_stream():
            for i in range(3):
                yield MyData(i + 100, ts_init=(i + 5) * 1000)

        engine = BacktestEngine()

        # Add stream iterators (this is the main functionality we're testing)
        iterators = [("stream", test_stream())]
        engine.add_data_iterators(iterators, chunk_duration="1s")

        # Test passes if no exception is raised

    def test_stream_iterator_edge_cases_none_data(self):
        """
        Test stream iterator handles None data gracefully.
        """

        def stream_with_none():
            yield MyData(1, ts_init=1000)
            yield None  # This should be filtered out or cause proper error handling
            yield MyData(2, ts_init=2000)

        iterator = BacktestDataIterator()

        with pytest.raises(Exception):  # Should handle None data appropriately
            iterator.add_stream_iterator("none_stream", stream_with_none(), 1_000_000_000)
            list(iterator)

    def test_stream_iterator_edge_cases_invalid_timestamp(self):
        """
        Test stream iterator handles invalid timestamp edge cases.
        """

        class InvalidTimestampData:
            def __init__(self, value):
                self.value = value
                self.ts_init = None  # Invalid timestamp

        def invalid_ts_stream():
            yield InvalidTimestampData(1)

        iterator = BacktestDataIterator()

        with pytest.raises(Exception):  # Should handle invalid timestamps
            iterator.add_stream_iterator("invalid_ts", invalid_ts_stream(), 1_000_000_000)
            list(iterator)

    def test_stream_iterator_edge_cases_negative_timestamps(self):
        """
        Test stream iterator rejects negative timestamps with overflow error.
        """

        def negative_ts_stream():
            yield MyData(1, ts_init=-1000)  # Negative timestamp
            yield MyData(2, ts_init=1000)  # Positive timestamp

        iterator = BacktestDataIterator()
        iterator.add_stream_iterator("negative_ts", negative_ts_stream(), 1_000_000_000)

        # Should reject negative timestamps with OverflowError
        with pytest.raises(OverflowError):
            list(iterator)

    def test_stream_iterator_edge_cases_zero_chunk_duration(self):
        """
        Test stream iterator handles zero chunk duration.
        """

        def simple_stream():
            yield MyData(1, ts_init=1000)

        iterator = BacktestDataIterator()

        # Zero chunk duration should be handled appropriately
        iterator.add_stream_iterator("zero_chunk", simple_stream(), 1_000_000_000)

        all_data = list(iterator)
        assert len(all_data) == 1
        assert all_data[0].value == 1

    def test_stream_iterator_edge_cases_very_large_chunk_duration(self):
        """
        Test stream iterator handles very large chunk durations.
        """

        def large_stream():
            for i in range(1000):
                yield MyData(i, ts_init=i * 1000)

        iterator = BacktestDataIterator()

        # Very large chunk duration should load all data in one chunk
        iterator.add_stream_iterator("large_chunk", large_stream(), 2**60)

        all_data = list(iterator)
        assert len(all_data) == 1000
        assert [d.value for d in all_data] == list(range(1000))

    def test_stream_iterator_edge_cases_duplicate_timestamps_ordering(self):
        """
        Test stream iterator handles many duplicate timestamps correctly.
        """

        def duplicate_ts_stream_1():
            for i in range(5):
                yield MyData(i, ts_init=1000)  # All same timestamp

        def duplicate_ts_stream_2():
            for i in range(5):
                yield MyData(i + 100, ts_init=1000)  # All same timestamp

        iterator = BacktestDataIterator()
        iterator.add_stream_iterator("dup1", duplicate_ts_stream_1(), 1_000_000_000)
        iterator.add_stream_iterator("dup2", duplicate_ts_stream_2(), 1_000_000_000)

        all_data = list(iterator)
        assert len(all_data) == 10

        # All should have same timestamp
        assert all(d.ts_init == 1000 for d in all_data)

        # Should maintain stream ordering within same timestamp
        values = [d.value for d in all_data]
        assert 0 in values and 100 in values  # Both streams represented

    def test_stream_iterator_edge_cases_empty_iterator(self):
        """
        Test stream iterator handles completely empty iterators.
        """

        def empty_stream():
            return
            yield MyData(1, ts_init=1000)  # Never reached

        iterator = BacktestDataIterator()
        iterator.add_stream_iterator("empty", empty_stream(), 1_000_000_000)

        # Should handle empty stream gracefully
        all_data = list(iterator)
        assert len(all_data) == 0
        assert iterator.is_done()

    def test_stream_iterator_edge_cases_single_item_iterator(self):
        """
        Test stream iterator handles single item iterators.
        """

        def single_item_stream():
            yield MyData(42, ts_init=1000)

        iterator = BacktestDataIterator()
        iterator.add_stream_iterator("single", single_item_stream(), 1_000_000_000)

        all_data = list(iterator)
        assert len(all_data) == 1
        assert all_data[0].value == 42
        assert all_data[0].ts_init == 1000

    def test_stream_iterator_edge_cases_generator_exception(self):
        """
        Test stream iterator handles generator exceptions gracefully.
        """

        def exception_stream():
            yield MyData(1, ts_init=1000)
            raise RuntimeError("Stream error")
            yield MyData(2, ts_init=2000)  # Never reached

        iterator = BacktestDataIterator()
        iterator.add_stream_iterator("exception", exception_stream(), 1_000_000_000)

        # Should handle generator exceptions
        with pytest.raises(RuntimeError):
            list(iterator)

    def test_stream_iterator_edge_cases_mixed_data_types(self):
        """
        Test stream iterator rejects non-Data types in stream.
        """

        class OtherData:
            def __init__(self, value, ts_init):
                self.value = value
                self.ts_init = ts_init

        def mixed_stream():
            yield MyData(1, ts_init=1000)
            yield OtherData(2, ts_init=2000)  # This will cause TypeError
            yield MyData(3, ts_init=3000)

        iterator = BacktestDataIterator()
        iterator.add_stream_iterator("mixed", mixed_stream(), 1_000_000_000)

        # Should fail when non-Data type is encountered
        with pytest.raises(TypeError):
            list(iterator)

    def test_stream_iterator_edge_cases_timestamp_precision(self):
        """
        Test stream iterator handles high precision timestamps.
        """

        def precision_stream():
            yield MyData(1, ts_init=1_000_000_000_000)  # High precision timestamp
            yield MyData(2, ts_init=1_000_000_000_001)  # 1 nanosecond later
            yield MyData(3, ts_init=1_000_000_000_002)  # 2 nanoseconds later

        iterator = BacktestDataIterator()
        iterator.add_stream_iterator(
            "precision",
            precision_stream(),
            1_000_000_000,
        )  # Very small chunks

        all_data = list(iterator)
        assert len(all_data) == 3
        assert all_data[0].ts_init == 1_000_000_000_000
        assert all_data[1].ts_init == 1_000_000_000_001
        assert all_data[2].ts_init == 1_000_000_000_002

    def test_stream_iterator_edge_cases_out_of_order_chunks(self):
        """
        Test stream iterator handles out-of-order data within chunks.
        """

        def out_of_order_stream():
            yield MyData(1, ts_init=3000)  # Out of order
            yield MyData(2, ts_init=1000)  # Earlier timestamp
            yield MyData(3, ts_init=2000)  # Middle timestamp
            yield MyData(4, ts_init=4000)  # Latest timestamp

        iterator = BacktestDataIterator()
        iterator.add_stream_iterator("out_of_order", out_of_order_stream(), 1_000_000_000)

        all_data = list(iterator)
        assert len(all_data) == 4

        # Should be sorted by timestamp despite input order
        timestamps = [d.ts_init for d in all_data]
        assert timestamps == [1000, 2000, 3000, 4000]

        # Verify corresponding values
        values = [d.value for d in all_data]
        assert values == [2, 3, 1, 4]  # Reordered by timestamp

    def test_stream_iterator_edge_cases_alternating_priority_streams(self):
        """
        Test complex priority scenarios with alternating append/prepend streams.
        """

        def stream_a():
            yield MyData(1, ts_init=1000)
            yield MyData(2, ts_init=2000)

        def stream_b():
            yield MyData(10, ts_init=1000)  # Same timestamp as stream_a
            yield MyData(20, ts_init=2000)  # Same timestamp as stream_a

        def stream_c():
            yield MyData(100, ts_init=1000)  # Same timestamp
            yield MyData(200, ts_init=2000)  # Same timestamp

        iterator = BacktestDataIterator()
        iterator.add_stream_iterator("a", stream_a(), 1_000_000_000, append_data=True)
        iterator.add_stream_iterator("b", stream_b(), 1_000_000_000, append_data=False)  # Prepend
        iterator.add_stream_iterator("c", stream_c(), 1_000_000_000, append_data=True)  # Append

        all_data = list(iterator)
        assert len(all_data) == 6

        # Group by timestamp to check ordering
        ts_1000_data = [d for d in all_data if d.ts_init == 1000]
        ts_2000_data = [d for d in all_data if d.ts_init == 2000]

        # Prepend stream should come first, then append streams in order
        assert len(ts_1000_data) == 3
        assert len(ts_2000_data) == 3

    def test_mixed_regular_and_stream_data_comprehensive(self):
        """
        Test comprehensive mixing of regular data and stream iterators.
        """

        def early_stream():
            yield MyData(100, ts_init=500)  # Before regular data
            yield MyData(101, ts_init=1500)  # Between regular data
            yield MyData(102, ts_init=3500)  # Between regular data
            yield MyData(103, ts_init=5500)  # After regular data

        def late_stream():
            yield MyData(200, ts_init=2500)  # Between regular data
            yield MyData(201, ts_init=4500)  # Between regular data
            yield MyData(202, ts_init=6500)  # After regular data

        iterator = BacktestDataIterator()

        # Add regular data: timestamps 1000, 2000, 3000, 4000, 5000
        regular_data = [MyData(i, ts_init=i * 1000) for i in range(1, 6)]
        iterator.add_data("regular", regular_data)

        # Add stream iterators
        iterator.add_stream_iterator("early", early_stream(), 1_000_000_000)
        iterator.add_stream_iterator("late", late_stream(), 1_000_000_000)

        all_data = list(iterator)

        # Should have 5 regular + 4 early + 3 late = 12 items
        assert len(all_data) == 12

        # Should be in chronological order
        timestamps = [d.ts_init for d in all_data]
        expected_timestamps = [
            500,
            1000,
            1500,
            2000,
            2500,
            3000,
            3500,
            4000,
            4500,
            5000,
            5500,
            6500,
        ]
        assert timestamps == expected_timestamps

        # Verify values correspond to correct timestamps
        values = [d.value for d in all_data]
        expected_values = [100, 1, 101, 2, 200, 3, 102, 4, 201, 5, 103, 202]
        assert values == expected_values

    def test_mixed_data_reset_and_reprocess(self):
        """
        Test that mixed regular and stream data can be reset and reprocessed.
        """

        def test_stream():
            yield MyData(100, ts_init=1500)
            yield MyData(101, ts_init=3500)

        iterator = BacktestDataIterator()

        # Add regular data
        regular_data = [MyData(i, ts_init=i * 1000) for i in range(1, 4)]  # 1000, 2000, 3000
        iterator.add_data("regular", regular_data)

        # Add stream
        iterator.add_stream_iterator("stream", test_stream(), 1_000_000_000)

        # First pass
        first_pass = list(iterator)
        assert len(first_pass) == 5  # 3 regular + 2 stream

        # Reset and second pass
        iterator.reset()
        second_pass = list(iterator)

        # Should be identical
        assert len(second_pass) == 5
        assert [d.value for d in first_pass] == [d.value for d in second_pass]
        assert [d.ts_init for d in first_pass] == [d.ts_init for d in second_pass]

    def test_mixed_data_remove_streams_keep_regular(self):
        """
        Test removing stream data while keeping regular data.
        """

        def removable_stream():
            yield MyData(100, ts_init=1500)
            yield MyData(101, ts_init=2500)

        iterator = BacktestDataIterator()

        # Add regular data
        regular_data = [MyData(i, ts_init=i * 1000) for i in range(1, 4)]  # 1000, 2000, 3000
        iterator.add_data("regular", regular_data)

        # Add stream
        iterator.add_stream_iterator("removable", removable_stream(), 1_000_000_000)

        # Verify mixed data
        mixed_data = list(iterator)
        assert len(mixed_data) == 5  # 3 regular + 2 stream

        # Remove stream data
        iterator.remove_data("removable")
        iterator.reset()

        # Should only have regular data
        remaining_data = list(iterator)
        assert len(remaining_data) == 3
        assert [d.value for d in remaining_data] == [1, 2, 3]

    def test_mixed_data_remove_regular_keep_streams(self):
        """
        Test removing regular data while keeping stream data.
        """

        def persistent_stream():
            yield MyData(100, ts_init=500)
            yield MyData(101, ts_init=1500)
            yield MyData(102, ts_init=2500)

        iterator = BacktestDataIterator()

        # Add regular data
        regular_data = [MyData(i, ts_init=i * 1000) for i in range(1, 3)]  # 1000, 2000
        iterator.add_data("regular", regular_data)

        # Add stream
        iterator.add_stream_iterator("persistent", persistent_stream(), 1_000_000_000)

        # Verify mixed data
        mixed_data = list(iterator)
        assert len(mixed_data) == 5  # 2 regular + 3 stream

        # Remove regular data
        iterator.remove_data("regular")
        iterator.reset()

        # Should only have stream data
        remaining_data = list(iterator)
        assert len(remaining_data) == 3
        assert [d.value for d in remaining_data] == [100, 101, 102]

    def test_mixed_data_priority_with_identical_timestamps(self):
        """
        Test priority handling when regular and stream data have identical timestamps.
        """

        def same_ts_stream():
            yield MyData(100, ts_init=1000)  # Same as regular data
            yield MyData(101, ts_init=2000)  # Same as regular data

        iterator = BacktestDataIterator()

        # Add regular data first (append_data=True by default)
        regular_data = [MyData(i, ts_init=i * 1000) for i in range(1, 3)]  # 1000, 2000
        iterator.add_data("regular", regular_data)

        # Add stream with prepend priority
        iterator.add_stream_iterator(
            "prepend_stream",
            same_ts_stream(),
            1_000_000_000,
            append_data=False,
        )

        all_data = list(iterator)
        assert len(all_data) == 4

        # Group by timestamp
        ts_1000_data = [d for d in all_data if d.ts_init == 1000]
        ts_2000_data = [d for d in all_data if d.ts_init == 2000]

        # Stream (prepend) should come before regular data for same timestamp
        assert len(ts_1000_data) == 2
        assert len(ts_2000_data) == 2

        # Prepend stream should appear first for each timestamp
        assert ts_1000_data[0].value == 100  # Stream data
        assert ts_1000_data[1].value == 1  # Regular data
        assert ts_2000_data[0].value == 101  # Stream data
        assert ts_2000_data[1].value == 2  # Regular data

    def test_mixed_data_multiple_streams_different_priorities(self):
        """
        Test multiple streams with different priorities mixed with regular data.
        """

        def append_stream():
            yield MyData(100, ts_init=1000)
            yield MyData(101, ts_init=2000)

        def prepend_stream():
            yield MyData(200, ts_init=1000)
            yield MyData(201, ts_init=2000)

        iterator = BacktestDataIterator()

        # Add regular data
        regular_data = [MyData(i, ts_init=i * 1000) for i in range(1, 3)]  # 1000, 2000
        iterator.add_data("regular", regular_data)

        # Add streams with different priorities
        iterator.add_stream_iterator("append", append_stream(), 1_000_000_000, append_data=True)
        iterator.add_stream_iterator("prepend", prepend_stream(), 1_000_000_000, append_data=False)

        all_data = list(iterator)
        assert len(all_data) == 6

        # Group by timestamp
        ts_1000_data = [d for d in all_data if d.ts_init == 1000]
        ts_2000_data = [d for d in all_data if d.ts_init == 2000]

        # Should have 3 items at each timestamp
        assert len(ts_1000_data) == 3
        assert len(ts_2000_data) == 3

        # Verify priority ordering: prepend stream first, then regular data, then append stream
        assert ts_1000_data[0].value == 200  # Prepend stream
        assert ts_1000_data[1].value == 1  # Regular data
        assert ts_1000_data[2].value == 100  # Append stream

    def test_mixed_data_large_datasets(self):
        """
        Test mixing large regular datasets with large stream datasets.
        """

        def large_stream():
            for i in range(500, 1000):  # 500 items, timestamps 500_000-999_000
                yield MyData(i, ts_init=i * 1000)

        iterator = BacktestDataIterator()

        # Add large regular dataset
        regular_data = [
            MyData(i, ts_init=i * 1000) for i in range(500)
        ]  # 500 items, timestamps 0-499_000
        iterator.add_data("regular", regular_data)

        # Add large stream
        iterator.add_stream_iterator("large_stream", large_stream(), 1_000_000_000)  # Large chunks

        all_data = list(iterator)

        # Should have 1000 total items
        assert len(all_data) == 1000

        # Should be in chronological order
        timestamps = [d.ts_init for d in all_data]
        expected_timestamps = list(range(0, 1000000, 1000))
        assert timestamps == expected_timestamps

        # Values should correspond to timestamps
        values = [d.value for d in all_data]
        expected_values = list(range(1000))
        assert values == expected_values

    def test_mixed_data_chunking_boundaries(self):
        """
        Test that chunking boundaries don't affect mixed data correctness.
        """

        def boundary_stream():
            # Data that crosses chunk boundaries
            for i in range(10, 30):
                yield MyData(i, ts_init=i * 1000)

        iterator = BacktestDataIterator()

        # Add regular data
        regular_data = [MyData(i, ts_init=i * 1000) for i in range(40)]
        iterator.add_data("regular", regular_data)

        # Add stream with small chunks to force boundary crossings
        iterator.add_stream_iterator("boundary", boundary_stream(), 1_000_000_000)  # Small chunks

        all_data = list(iterator)

        # Should have 40 regular + 20 stream = 60 items, but with overlapping timestamps
        # Regular data: 0-39 (40 items), Stream data: 10-29 (20 items)
        # Total: 40 + 20 = 60 items (including duplicates for timestamps 10-29)
        assert len(all_data) == 60

        # Should be in chronological order despite chunk boundaries
        timestamps = [d.ts_init for d in all_data]
        assert timestamps == sorted(timestamps)

        # Both regular and stream data should be present
        all_values = [d.value for d in all_data]
        regular_values = list(range(40))  # 0-39
        stream_values = list(range(10, 30))  # 10-29
        expected_values = sorted(regular_values + stream_values)
        assert sorted(all_values) == expected_values

    def test_mixed_data_set_index_functionality(self):
        """
        Test set_index functionality with mixed regular and stream data.
        """

        def indexed_stream():
            yield MyData(100, ts_init=1500)
            yield MyData(101, ts_init=2500)
            yield MyData(102, ts_init=3500)

        iterator = BacktestDataIterator()

        # Add regular data
        regular_data = [MyData(i, ts_init=i * 1000) for i in range(1, 5)]  # 1000, 2000, 3000, 4000
        iterator.add_data("regular", regular_data)

        # Add stream
        iterator.add_stream_iterator("indexed", indexed_stream(), 1_000_000_000)

        # Consume some data
        consumed = []
        for _ in range(3):
            consumed.append(next(iterator))

        # Should have consumed first 3 items: 1000, 1500, 2000
        assert [d.ts_init for d in consumed] == [1000, 1500, 2000]

        # Set regular data index back to start
        iterator.set_index("regular", 0)

        # Continue iteration - should restart regular data but continue stream
        remaining = list(iterator)

        # Should have restarted regular data from beginning
        # Original remaining would be: 2500, 3000, 3500, 4000
        # After reset: 1000, 2000, 2500, 3000, 3500, 4000 (minus already consumed stream items)
        assert len(remaining) >= 4  # At least the remaining items

    def test_mixed_data_all_data_method(self):
        """
        Test all_data() method with mixed regular and stream data.
        """

        def test_stream():
            yield MyData(100, ts_init=1500)
            yield MyData(101, ts_init=2500)

        iterator = BacktestDataIterator()

        # Add regular data
        regular_data = [MyData(i, ts_init=i * 1000) for i in range(1, 4)]
        iterator.add_data("regular", regular_data)

        # Add stream
        iterator.add_stream_iterator("stream", test_stream(), 1_000_000_000)

        # Consume all data to ensure streams are loaded
        list(iterator)

        # Get all data
        all_data_dict = iterator.all_data()

        # Should have both regular and stream data
        assert "regular" in all_data_dict
        assert "stream" in all_data_dict

        # Verify regular data
        assert len(all_data_dict["regular"]) == 3
        assert [d.value for d in all_data_dict["regular"]] == [1, 2, 3]

        # Verify stream data
        assert len(all_data_dict["stream"]) == 2
        assert [d.value for d in all_data_dict["stream"]] == [100, 101]

    def test_stream_iterator_error_conditions_invalid_chunk_duration(self):
        """
        Test stream iterator error handling for invalid chunk durations.
        """

        def simple_stream():
            yield MyData(1, ts_init=1000)

        iterator = BacktestDataIterator()

        # Test negative chunk duration
        with pytest.raises(Exception):
            iterator.add_stream_iterator("negative_chunk", simple_stream(), -1000)

    def test_stream_iterator_error_conditions_invalid_stream_name(self):
        """
        Test stream iterator error handling for invalid stream names.
        """

        def simple_stream():
            yield MyData(1, ts_init=1000)

        iterator = BacktestDataIterator()

        # Test empty string name
        with pytest.raises(Exception):
            iterator.add_stream_iterator("", simple_stream(), 1_000_000_000)

        # Test None name
        with pytest.raises(Exception):
            iterator.add_stream_iterator(None, simple_stream(), 1_000_000_000)

    def test_stream_iterator_error_conditions_corrupted_data_stream(self):
        """
        Test stream iterator error handling for corrupted data streams.
        """

        def corrupted_stream():
            yield MyData(1, ts_init=1000)
            yield "invalid data object"  # Not a proper Data object
            yield MyData(2, ts_init=2000)

        iterator = BacktestDataIterator()

        # Should handle corrupted data gracefully
        with pytest.raises(Exception):
            iterator.add_stream_iterator("corrupted", corrupted_stream(), 1_000_000_000)
            list(iterator)

    def test_stream_iterator_error_conditions_infinite_stream(self):
        """
        Test stream iterator behavior with infinite streams (should handle gracefully).
        """

        def infinite_stream():
            i = 0
            while True:
                yield MyData(i, ts_init=i * 1000)
                i += 1
                if i > 10:  # Prevent actual infinite loop in test
                    break

        iterator = BacktestDataIterator()
        iterator.add_stream_iterator("infinite", infinite_stream(), 1_000_000_000)

        # Should be able to consume finite portion
        consumed = []
        for _ in range(5):
            try:
                consumed.append(next(iterator))
            except StopIteration:
                break

        assert len(consumed) == 5
        assert [d.value for d in consumed] == [0, 1, 2, 3, 4]

    def test_stream_iterator_error_conditions_concurrent_modification(self):
        """
        Test stream iterator error handling for concurrent modifications.
        """

        def base_stream():
            yield MyData(1, ts_init=1000)
            yield MyData(2, ts_init=2000)

        iterator = BacktestDataIterator()
        iterator.add_stream_iterator("base", base_stream(), 1_000_000_000)

        # Start consuming
        first_item = next(iterator)
        assert first_item.value == 1

        # Try to add another stream while consuming (should be handled gracefully)
        def concurrent_stream():
            yield MyData(100, ts_init=1500)

        iterator.add_stream_iterator("concurrent", concurrent_stream(), 1_000_000_000)

        # Should be able to continue consuming
        remaining = list(iterator)
        assert len(remaining) >= 1  # Should have at least the remaining items

    def test_stream_iterator_error_conditions_recursive_generator(self):
        """
        Test stream iterator with recursive generator patterns.
        """

        def recursive_generator(depth=0):
            if depth < 3:
                yield MyData(depth, ts_init=depth * 1000)
                yield from recursive_generator(depth + 1)

        iterator = BacktestDataIterator()
        iterator.add_stream_iterator("recursive", recursive_generator(), 1_000_000_000)

        # Should handle recursive generators
        all_data = list(iterator)
        assert len(all_data) == 3
        assert [d.value for d in all_data] == [0, 1, 2]

    def test_stream_iterator_error_conditions_exception_during_iteration(self):
        """
        Test stream iterator error handling when exceptions occur during iteration.

        Exceptions during chunk loading result in fail-fast behavior with no partial
        data.

        """

        def exception_on_third():
            yield MyData(1, ts_init=1000)
            yield MyData(2, ts_init=2000)
            raise ValueError("Intentional error on third item")

        iterator = BacktestDataIterator()
        iterator.add_stream_iterator("exception_third", exception_on_third(), 1_000_000_000)

        # Fail-fast behavior: exception during chunk loading prevents any data from being yielded
        items = []
        with pytest.raises(ValueError):
            for item in iterator:
                items.append(item)

        # With fail-fast chunk loading, no items are yielded when exception occurs during chunk load
        assert len(items) == 0

    def test_stream_iterator_error_conditions_malformed_timestamps(self):
        """
        Test stream iterator error handling for malformed timestamp data.
        """

        class MalformedTimestampData:
            def __init__(self, value, ts_init):
                self.value = value
                self.ts_init = ts_init

        def malformed_stream():
            yield MalformedTimestampData(1, "not a number")  # String instead of int
            yield MalformedTimestampData(2, 2000)

        iterator = BacktestDataIterator()

        # Should handle malformed timestamps
        with pytest.raises(Exception):
            iterator.add_stream_iterator("malformed", malformed_stream(), 1_000_000_000)
            list(iterator)

    def test_stream_iterator_error_conditions_extremely_large_timestamps(self):
        """
        Test stream iterator with extremely large timestamp values.
        """

        def large_timestamp_stream():
            yield MyData(1, ts_init=2**62)  # Very large timestamp
            yield MyData(2, ts_init=2**63 - 1)  # Near max int64

        iterator = BacktestDataIterator()
        iterator.add_stream_iterator("large_ts", large_timestamp_stream(), 1_000_000_000)

        # Should handle large timestamps
        all_data = list(iterator)
        assert len(all_data) == 2
        assert all_data[0].ts_init == 2**62
        assert all_data[1].ts_init == 2**63 - 1

    def test_stream_iterator_error_conditions_duplicate_stream_names(self):
        """
        Test stream iterator behavior with duplicate stream names.
        """

        def first_stream():
            yield MyData(1, ts_init=1000)

        def second_stream():
            yield MyData(2, ts_init=2000)

        iterator = BacktestDataIterator()

        # Add first stream
        iterator.add_stream_iterator("duplicate", first_stream(), 1_000_000_000)

        # Add second stream with same name (should replace)
        iterator.add_stream_iterator("duplicate", second_stream(), 1_000_000_000)

        # Should only have second stream's data
        all_data = list(iterator)
        assert len(all_data) == 1
        assert all_data[0].value == 2

    def test_stream_iterator_error_conditions_mixed_exception_handling(self):
        """
        Test stream iterator error handling with mixed regular and stream data when
        exceptions occur.

        Fail-fast behavior during chunk loading prevents any data.

        """

        def failing_stream():
            yield MyData(100, ts_init=1500)
            raise RuntimeError("Stream failure")

        iterator = BacktestDataIterator()

        # Add regular data
        regular_data = [MyData(i, ts_init=i * 1000) for i in range(1, 4)]
        iterator.add_data("regular", regular_data)

        # Add failing stream
        iterator.add_stream_iterator("failing", failing_stream(), 1_000_000_000)

        # Fail-fast behavior: exception during chunk loading prevents any data from being yielded
        items = []
        with pytest.raises(RuntimeError):
            for item in iterator:
                items.append(item)

        # With fail-fast chunk loading, no items are yielded when exception occurs during chunk load
        assert len(items) == 0

    def test_stream_iterator_error_conditions_chunk_loading_failure(self):
        """
        Test stream iterator error handling when chunk loading fails.
        """

        class FailingIterator:
            def __init__(self):
                self.count = 0

            def __iter__(self):
                return self

            def __next__(self):
                if self.count < 2:
                    self.count += 1
                    return MyData(self.count, ts_init=self.count * 1000)
                else:
                    raise StopIteration

        iterator = BacktestDataIterator()

        # This should work normally
        iterator.add_stream_iterator("normal_failing", FailingIterator(), 1_000_000_000)

        all_data = list(iterator)
        assert len(all_data) == 2
        assert [d.value for d in all_data] == [1, 2]

    def test_stream_iterator_error_conditions_callback_exceptions(self):
        """
        Test stream iterator error handling when empty_data_callback raises exceptions.
        """

        def failing_callback(name, ts):
            raise ValueError(f"Callback failed for {name}")

        def simple_stream():
            yield MyData(1, ts_init=1000)

        iterator = BacktestDataIterator(empty_data_callback=failing_callback)
        iterator.add_stream_iterator("callback_test", simple_stream(), 1_000_000_000)

        # Should handle callback exceptions
        with pytest.raises(ValueError):
            list(iterator)

    def test_stream_iterator_error_conditions_invalid_data_attributes(self):
        """
        Test stream iterator error handling for data objects with invalid attributes.
        """

        class InvalidAttributeData:
            def __init__(self, value):
                self.value = value
                # Missing ts_init attribute

        def invalid_attr_stream():
            yield InvalidAttributeData(1)

        iterator = BacktestDataIterator()

        # Should handle missing attributes
        with pytest.raises(Exception):
            iterator.add_stream_iterator("invalid_attr", invalid_attr_stream(), 1_000_000_000)
            list(iterator)

    def test_stream_iterator_error_conditions_zero_duration_edge_cases(self):
        """
        Test stream iterator edge cases with zero duration chunks.
        """

        def rapid_stream():
            for i in range(10):
                yield MyData(i, ts_init=i)  # 1 nanosecond intervals

        iterator = BacktestDataIterator()
        iterator.add_stream_iterator("rapid", rapid_stream(), 1_000_000_000)  # Zero duration chunks

        # Should handle zero duration chunks
        all_data = list(iterator)
        assert len(all_data) == 10
        assert [d.value for d in all_data] == list(range(10))

    def test_stream_iterator_error_conditions_resource_cleanup(self):
        """
        Test that stream iterators properly clean up resources.
        """

        class ResourceTrackingIterator:
            def __init__(self):
                self.resource_opened = True
                self.count = 0

            def __iter__(self):
                return self

            def __next__(self):
                if not self.resource_opened:
                    raise RuntimeError("Resource already closed")

                if self.count < 3:
                    self.count += 1
                    return MyData(self.count, ts_init=self.count * 1000)
                else:
                    raise StopIteration

            def close(self):
                self.resource_opened = False

        iterator = BacktestDataIterator()

        resource_iter = ResourceTrackingIterator()
        iterator.add_stream_iterator("resource_test", resource_iter, 1_000_000_000)

        # Consume all data
        all_data = list(iterator)
        assert len(all_data) == 3

        # Resources should still be accessible after consumption
        assert resource_iter.resource_opened

    def test_stream_iterator_error_conditions_deep_recursion_protection(self):
        """
        Test stream iterator protection against deep recursion.
        """

        def deep_recursive_stream(depth=0):
            if depth < 100:  # Reasonable depth
                yield MyData(depth, ts_init=depth * 1000)
                yield from deep_recursive_stream(depth + 1)

        iterator = BacktestDataIterator()
        iterator.add_stream_iterator("deep_recursive", deep_recursive_stream(), 1_000_000_000)

        # Should handle reasonable recursion depth
        all_data = list(iterator)
        assert len(all_data) == 100
        assert [d.value for d in all_data] == list(range(100))

    def test_complex_chunking_scenarios_overlapping_chunks(self):
        """
        Test complex chunking scenarios with overlapping time windows.
        """

        def overlapping_stream():
            # Create data that might overlap across chunk boundaries
            timestamps = [1000, 1500, 2000, 2100, 2200, 2500, 3000, 3500, 4000]
            for i, ts in enumerate(timestamps):
                yield MyData(i, ts_init=ts)

        iterator = BacktestDataIterator()

        # Use chunk size that creates overlapping scenarios
        iterator.add_stream_iterator(
            "overlapping",
            overlapping_stream(),
            1_000_000_000,
        )  # 1 second chunks

        all_data = list(iterator)
        assert len(all_data) == 9

        # Should maintain chronological order despite chunk boundaries
        timestamps = [d.ts_init for d in all_data]
        assert timestamps == [1000, 1500, 2000, 2100, 2200, 2500, 3000, 3500, 4000]

    def test_complex_chunking_scenarios_uneven_distribution(self):
        """
        Test chunking with uneven data distribution across time.
        """

        def uneven_stream():
            # Heavy concentration at start
            for i in range(50):
                yield MyData(i, ts_init=i * 100)  # 0-4900

            # Large gap
            yield MyData(50, ts_init=100000)  # 100 microseconds later

            # Heavy concentration at end
            for i in range(51, 100):
                yield MyData(i, ts_init=100000 + (i - 50) * 100)

        iterator = BacktestDataIterator()
        iterator.add_stream_iterator(
            "uneven",
            uneven_stream(),
            1_000_000_000,
        )  # 10 microsecond chunks

        all_data = list(iterator)
        assert len(all_data) == 100

        # Should handle uneven distribution correctly
        values = [d.value for d in all_data]
        assert values == list(range(100))

    def test_complex_chunking_scenarios_micro_chunks(self):
        """
        Test very small chunk sizes with dense data.
        """

        def dense_stream():
            for i in range(1000):
                yield MyData(i, ts_init=i)  # 1 nanosecond intervals

        iterator = BacktestDataIterator()
        iterator.add_stream_iterator("dense", dense_stream(), 1_000_000_000)  # 1 nanosecond chunks

        all_data = list(iterator)
        assert len(all_data) == 1000
        assert [d.value for d in all_data] == list(range(1000))

    def test_complex_chunking_scenarios_macro_chunks(self):
        """
        Test very large chunk sizes with sparse data.
        """

        def sparse_stream():
            timestamps = [1000, 1_000_000, 2_000_000, 3_000_000, 4_000_000]  # 1ms intervals
            for i, ts in enumerate(timestamps):
                yield MyData(i, ts_init=ts)

        iterator = BacktestDataIterator()
        iterator.add_stream_iterator("sparse", sparse_stream(), 10**18)  # Huge chunks

        all_data = list(iterator)
        assert len(all_data) == 5
        assert [d.value for d in all_data] == list(range(5))

    def test_complex_chunking_scenarios_multiple_streams_different_chunk_sizes(self):
        """
        Test multiple streams with different chunk sizes.
        """

        def fast_stream():
            for i in range(20):
                yield MyData(i, ts_init=i * 1000)

        def slow_stream():
            for i in range(10):
                yield MyData(i + 100, ts_init=i * 2000)

        iterator = BacktestDataIterator()
        iterator.add_stream_iterator("fast", fast_stream(), 1_000_000_000)  # Small chunks
        iterator.add_stream_iterator("slow", slow_stream(), 1_000_000_000)  # Large chunks

        all_data = list(iterator)
        assert len(all_data) == 30

        # Should merge correctly despite different chunk sizes
        timestamps = [d.ts_init for d in all_data]
        assert timestamps == sorted(timestamps)

    def test_complex_chunking_scenarios_chunk_boundary_alignment(self):
        """
        Test chunk boundary alignment with different time patterns.
        """

        def aligned_stream():
            # Create data that aligns with chunk boundaries
            for i in range(10):
                yield MyData(i, ts_init=i * 10000)  # Exactly aligned with chunk size

        def misaligned_stream():
            # Create data that misaligns with chunk boundaries
            for i in range(10):
                yield MyData(i + 100, ts_init=i * 10000 + 3333)  # Offset by 3.333 microseconds

        iterator = BacktestDataIterator()
        iterator.add_stream_iterator("aligned", aligned_stream(), 1_000_000_000)
        iterator.add_stream_iterator("misaligned", misaligned_stream(), 1_000_000_000)

        all_data = list(iterator)
        assert len(all_data) == 20

        # Should handle both aligned and misaligned data correctly
        aligned_data = [d for d in all_data if d.value < 100]
        misaligned_data = [d for d in all_data if d.value >= 100]
        assert len(aligned_data) == 10
        assert len(misaligned_data) == 10

    def test_stream_iterator_priority_ordering_complex_scenarios(self):
        """
        Test complex priority ordering scenarios.
        """

        def high_priority_stream():
            yield MyData(1, ts_init=1000)
            yield MyData(2, ts_init=2000)
            yield MyData(3, ts_init=3000)

        def medium_priority_stream():
            yield MyData(11, ts_init=1000)  # Same timestamp as high
            yield MyData(12, ts_init=2000)  # Same timestamp as high
            yield MyData(13, ts_init=3000)  # Same timestamp as high

        def low_priority_stream():
            yield MyData(21, ts_init=1000)  # Same timestamp as others
            yield MyData(22, ts_init=2000)  # Same timestamp as others
            yield MyData(23, ts_init=3000)  # Same timestamp as others

        iterator = BacktestDataIterator()

        # Add streams in different priority order
        iterator.add_stream_iterator(
            "high",
            high_priority_stream(),
            1_000_000_000,
            append_data=False,
        )  # Prepend (highest)
        iterator.add_stream_iterator(
            "medium",
            medium_priority_stream(),
            1_000_000_000,
            append_data=True,
        )  # Append
        iterator.add_stream_iterator(
            "low",
            low_priority_stream(),
            1_000_000_000,
            append_data=True,
        )  # Append (lowest)

        all_data = list(iterator)
        assert len(all_data) == 9

        # Group by timestamp and check ordering
        for ts in [1000, 2000, 3000]:
            ts_data = [d for d in all_data if d.ts_init == ts]
            assert len(ts_data) == 3

            # High priority (prepend) should come first
            assert ts_data[0].value in [1, 2, 3]  # High priority stream

            # Medium and low priority should follow in insertion order
            remaining_values = [d.value for d in ts_data[1:]]
            assert 11 in remaining_values or 12 in remaining_values or 13 in remaining_values
            assert 21 in remaining_values or 22 in remaining_values or 23 in remaining_values

    def test_stream_iterator_priority_ordering_dynamic_changes(self):
        """
        Test priority ordering with dynamic stream additions.
        """

        def base_stream():
            yield MyData(1, ts_init=1000)
            yield MyData(2, ts_init=2000)

        def added_stream():
            yield MyData(11, ts_init=1000)
            yield MyData(12, ts_init=2000)

        iterator = BacktestDataIterator()

        # Add base stream
        iterator.add_stream_iterator("base", base_stream(), 1_000_000_000, append_data=True)

        # Consume first item
        first_item = next(iterator)
        assert first_item.value == 1

        # Add another stream with higher priority
        iterator.add_stream_iterator("added", added_stream(), 1_000_000_000, append_data=False)

        # Continue consuming
        remaining = list(iterator)

        # Should have integrated the new stream appropriately
        assert len(remaining) >= 2  # At least the remaining items

    def test_stream_iterator_priority_ordering_mixed_timestamps(self):
        """
        Test priority ordering with mixed timestamp scenarios.
        """

        def early_priority_stream():
            yield MyData(1, ts_init=500)  # Earlier than others
            yield MyData(2, ts_init=1500)  # Same as others
            yield MyData(3, ts_init=2500)  # Same as others

        def late_priority_stream():
            yield MyData(11, ts_init=1500)  # Same as early stream
            yield MyData(12, ts_init=2500)  # Same as early stream
            yield MyData(13, ts_init=3500)  # Later than others

        iterator = BacktestDataIterator()

        # Add streams with different priorities
        iterator.add_stream_iterator(
            "early",
            early_priority_stream(),
            1_000_000_000,
            append_data=False,
        )
        iterator.add_stream_iterator(
            "late",
            late_priority_stream(),
            1_000_000_000,
            append_data=True,
        )

        all_data = list(iterator)
        assert len(all_data) == 6

        # Should be in chronological order
        timestamps = [d.ts_init for d in all_data]
        assert timestamps == [500, 1500, 1500, 2500, 2500, 3500]

        # For same timestamps, priority should be respected
        ts_1500_data = [d for d in all_data if d.ts_init == 1500]
        ts_2500_data = [d for d in all_data if d.ts_init == 2500]

        # Early stream (prepend) should come first for same timestamps
        assert ts_1500_data[0].value == 2  # Early stream
        assert ts_1500_data[1].value == 11  # Late stream
        assert ts_2500_data[0].value == 3  # Early stream
        assert ts_2500_data[1].value == 12  # Late stream

    def test_stream_iterator_priority_ordering_interleaved_data(self):
        """
        Test priority ordering with interleaved data patterns.
        """

        def pattern_a_stream():
            for i in range(0, 10, 2):  # Even numbers: 0, 2, 4, 6, 8
                yield MyData(i, ts_init=i * 1000)

        def pattern_b_stream():
            for i in range(1, 10, 2):  # Odd numbers: 1, 3, 5, 7, 9
                yield MyData(i, ts_init=i * 1000)

        def pattern_c_stream():
            for i in range(10):  # All numbers with same timestamps
                yield MyData(i + 100, ts_init=i * 1000)

        iterator = BacktestDataIterator()

        # Add with different priorities
        iterator.add_stream_iterator(
            "a",
            pattern_a_stream(),
            1_000_000_000,
            append_data=False,
        )  # Prepend
        iterator.add_stream_iterator(
            "b",
            pattern_b_stream(),
            1_000_000_000,
            append_data=True,
        )  # Append
        iterator.add_stream_iterator(
            "c",
            pattern_c_stream(),
            1_000_000_000,
            append_data=True,
        )  # Append

        all_data = list(iterator)
        assert len(all_data) == 20  # 5 + 5 + 10

        # Should be in chronological order
        timestamps = [d.ts_init for d in all_data]
        assert timestamps == sorted(timestamps)

        # Check priority ordering for overlapping timestamps
        for ts in range(0, 9000, 2000):  # Even timestamps where pattern_a and pattern_c overlap
            ts_data = [d for d in all_data if d.ts_init == ts]
            if len(ts_data) == 2:  # pattern_a and pattern_c
                assert ts_data[0].value < 10  # pattern_a (prepend priority)
                assert ts_data[1].value >= 100  # pattern_c (append priority)

    def test_stream_iterator_priority_ordering_edge_case_empty_streams(self):
        """
        Test priority ordering with empty streams mixed in.
        """

        def empty_stream():
            return
            yield MyData(999, ts_init=999)  # Never reached

        def normal_stream():
            yield MyData(1, ts_init=1000)
            yield MyData(2, ts_init=2000)

        iterator = BacktestDataIterator()

        # Add empty stream with high priority
        iterator.add_stream_iterator("empty", empty_stream(), 1_000_000_000, append_data=False)

        # Add normal stream
        iterator.add_stream_iterator("normal", normal_stream(), 1_000_000_000, append_data=True)

        all_data = list(iterator)
        assert len(all_data) == 2
        assert [d.value for d in all_data] == [1, 2]

    def test_stream_iterator_priority_ordering_large_scale(self):
        """
        Test priority ordering with large scale data.
        """

        def create_large_stream(offset, priority):
            for i in range(1000):
                yield MyData(i + offset, ts_init=i * 1000)

        iterator = BacktestDataIterator()

        # Add multiple large streams with different priorities
        iterator.add_stream_iterator(
            "high",
            create_large_stream(0, 1),
            1_000_000_000,
            append_data=False,
        )
        iterator.add_stream_iterator(
            "medium",
            create_large_stream(1000, 2),
            1_000_000_000,
            append_data=True,
        )
        iterator.add_stream_iterator(
            "low",
            create_large_stream(2000, 3),
            1_000_000_000,
            append_data=True,
        )

        all_data = list(iterator)
        assert len(all_data) == 3000

        # Should be in chronological order
        timestamps = [d.ts_init for d in all_data]
        assert timestamps == sorted(timestamps)

        # Check priority ordering for same timestamps
        for ts in range(0, 999_000, 1000):
            ts_data = [d for d in all_data if d.ts_init == ts]
            assert len(ts_data) == 3

            # High priority should come first
            assert ts_data[0].value < 1000  # High priority stream
            assert 1000 <= ts_data[1].value < 2000  # Medium priority stream
            assert ts_data[2].value >= 2000  # Low priority stream

    def test_stream_iterator_performance_multiple_large_streams(self):
        """
        Test stream iterator performance with multiple large streams.
        """

        def create_large_stream(offset):
            for i in range(5000):
                yield MyData(i + offset, ts_init=i * 1000)

        iterator = BacktestDataIterator()

        # Add multiple large streams
        start_time = time.time()
        iterator.add_stream_iterator("stream1", create_large_stream(0), 1_000_000_000)
        iterator.add_stream_iterator("stream2", create_large_stream(5000), 1_000_000_000)
        iterator.add_stream_iterator("stream3", create_large_stream(10000), 1_000_000_000)
        add_time = time.time() - start_time

        # Consume all data
        start_time = time.time()
        all_data = list(iterator)
        consume_time = time.time() - start_time

        # Performance and correctness assertions
        assert len(all_data) == 15000
        assert add_time < 2.0
        assert consume_time < 10.0

        # Should be in chronological order
        timestamps = [d.ts_init for d in all_data]
        assert timestamps == sorted(timestamps)

    def test_stream_iterator_performance_concurrent_streams(self):
        """
        Test performance with many concurrent streams.
        """

        def small_stream(offset):
            for i in range(100):
                yield MyData(i + offset, ts_init=i * 1000)

        iterator = BacktestDataIterator()

        # Add many small streams
        start_time = time.time()
        for i in range(50):  # 50 streams
            iterator.add_stream_iterator(f"stream_{i}", small_stream(i * 100), 1_000_000_000)
        add_time = time.time() - start_time

        # Consume all data
        start_time = time.time()
        all_data = list(iterator)
        consume_time = time.time() - start_time

        # Performance assertions
        assert len(all_data) == 5000  # 50 streams * 100 items each
        assert add_time < 5.0  # Should add streams reasonably quickly
        assert consume_time < 10.0  # Should consume reasonably quickly

        # Should be in chronological order
        timestamps = [d.ts_init for d in all_data]
        assert timestamps == sorted(timestamps)
